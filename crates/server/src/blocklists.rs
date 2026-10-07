//! Listes de blocage : téléchargement de l'archive UT1, conversion en
//! ensembles compacts, stockage sur disque et chargement à la demande.
//!
//! Source : listes de l'Université Toulouse Capitole, licence CC BY-SA 4.0.

use crate::util;
use anyhow::{Context, Result, bail};
use cotutelle_common::protocol::BlocklistRef;
use cotutelle_common::{Blocklists, DomainSet};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub updated_at: i64,
    pub source: String,
    pub categories: BTreeMap<String, ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub checksum: String,
    pub entries: u64,
}

pub struct BlocklistStore {
    dir: PathBuf,
    manifest: RwLock<Manifest>,
    /// Catégories chargées en mémoire pour le DNS du réseau local.
    loaded: RwLock<Blocklists>,
}

impl BlocklistStore {
    pub fn open(dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&dir).with_context(|| format!("création de {}", dir.display()))?;
        let manifest = match std::fs::read(dir.join("manifest.json")) {
            Ok(raw) => serde_json::from_slice(&raw).unwrap_or_default(),
            Err(_) => Manifest::default(),
        };
        Ok(Self { dir, manifest: RwLock::new(manifest), loaded: RwLock::default() })
    }

    pub fn manifest(&self) -> Manifest {
        self.manifest.read().expect("verrou manifest").clone()
    }

    pub fn path(&self, category: &str) -> Option<PathBuf> {
        // Le nom vient du réseau : on n'accepte que les catégories connues.
        self.manifest()
            .categories
            .contains_key(category)
            .then(|| self.dir.join(format!("{category}.bin")))
    }

    /// Références des listes disponibles parmi les catégories demandées.
    pub fn refs_for<'a>(
        &self,
        categories: impl IntoIterator<Item = &'a String>,
    ) -> Vec<BlocklistRef> {
        let manifest = self.manifest.read().expect("verrou manifest");
        let mut refs: Vec<BlocklistRef> = categories
            .into_iter()
            .filter_map(|c| {
                manifest.categories.get(c).map(|e| BlocklistRef {
                    category: c.clone(),
                    checksum: e.checksum.clone(),
                    entries: e.entries,
                })
            })
            .collect();
        refs.sort_by(|a, b| a.category.cmp(&b.category));
        refs.dedup_by(|a, b| a.category == b.category);
        refs
    }

    /// Charge en mémoire exactement les catégories demandées.
    pub fn set_loaded<'a>(&self, wanted: impl IntoIterator<Item = &'a String>) {
        let wanted: Vec<&String> = wanted.into_iter().collect();
        let mut loaded = self.loaded.write().expect("verrou listes");
        let stale: Vec<String> = loaded
            .categories()
            .filter(|c| !wanted.iter().any(|w| w == c))
            .map(str::to_string)
            .collect();
        for category in stale {
            loaded.remove(&category);
        }
        for category in wanted {
            if loaded.contains(category) {
                continue;
            }
            let Some(path) = self.path(category) else { continue };
            match std::fs::read(&path)
                .map_err(anyhow::Error::from)
                .and_then(|b| Ok(DomainSet::from_bytes(&b)?))
            {
                Ok(set) => loaded.insert(category.clone(), set),
                Err(e) => tracing::warn!(category, error = %e, "liste illisible"),
            }
        }
    }

    /// Exécute `f` avec les listes chargées, sans les copier.
    pub fn with_loaded<T>(&self, f: impl FnOnce(&Blocklists) -> T) -> T {
        f(&self.loaded.read().expect("verrou listes"))
    }

    /// Télécharge l'archive et reconstruit toutes les listes.
    /// `source` est une URL http(s) ou le chemin d'une archive locale.
    pub async fn update(&self, source: &str) -> Result<usize> {
        let archive = self.dir.join("archive.tar.gz");
        if source.starts_with("http://") || source.starts_with("https://") {
            download(source, &archive).await?;
        } else {
            tokio::fs::copy(source, &archive)
                .await
                .with_context(|| format!("lecture de {source}"))?;
        }

        let dir = self.dir.clone();
        let source_name = source.to_string();
        let manifest = tokio::task::spawn_blocking(move || -> Result<Manifest> {
            let categories = convert_archive(&archive, &dir)?;
            if categories.is_empty() {
                bail!("l'archive ne contient aucune liste");
            }
            let manifest = Manifest { updated_at: util::now(), source: source_name, categories };
            std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest)?)?;
            let _ = std::fs::remove_file(&archive);
            Ok(manifest)
        })
        .await??;

        let count = manifest.categories.len();
        *self.manifest.write().expect("verrou manifest") = manifest;
        // Force le rechargement des catégories en mémoire avec les nouveaux fichiers.
        let loaded: Vec<String> =
            self.loaded.read().expect("verrou listes").categories().map(str::to_string).collect();
        *self.loaded.write().expect("verrou listes") = Blocklists::default();
        self.set_loaded(&loaded);
        Ok(count)
    }
}

async fn download(url: &str, dest: &Path) -> Result<()> {
    // Le serveur UT1 pose un cookie avant de rediriger vers l'archive.
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .user_agent(concat!("cotutelle/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let response = client.get(url).send().await?.error_for_status()?;
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk?).await?;
    }
    file.flush().await?;
    Ok(())
}

/// Parcourt l'archive et écrit un fichier `<catégorie>.bin` par liste `domains`.
fn convert_archive(archive: &Path, dir: &Path) -> Result<BTreeMap<String, ManifestEntry>> {
    let file = std::fs::File::open(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut categories = BTreeMap::new();

    for entry in tar.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue; // Les alias de catégories sont des liens symboliques.
        }
        let path = entry.path()?.into_owned();
        let parts: Vec<String> =
            path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        // Attendu : `<racine>/<catégorie>/domains`.
        let [.., category, name] = parts.as_slice() else { continue };
        if name != "domains" || !is_safe_name(category) {
            continue;
        }
        let mut raw = Vec::new();
        entry.read_to_end(&mut raw)?;
        let set = DomainSet::from_text(&String::from_utf8_lossy(&raw));
        if set.is_empty() {
            continue;
        }
        let bytes = set.to_bytes();
        std::fs::write(dir.join(format!("{category}.bin")), &bytes)?;
        categories.insert(
            category.clone(),
            ManifestEntry { checksum: util::sha256_hex(&bytes), entries: set.len() as u64 },
        );
    }
    Ok(categories)
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
