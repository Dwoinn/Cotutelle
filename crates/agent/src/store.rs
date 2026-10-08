//! État persistant de l'agent : identité, dernière politique connue, temps
//! d'écran et listes de blocage. Tout vit dans le répertoire d'état, lisible
//! par root uniquement.

use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate};
use cotutelle_common::protocol::DeviceState;
use cotutelle_common::{DeviceId, DomainSet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Identité de l'agent, obtenue à l'enrôlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub server: String,
    pub device: DeviceId,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct Paths {
    dir: PathBuf,
}

impl Paths {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn identity(&self) -> PathBuf {
        self.dir.join("agent.json")
    }

    fn state(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn usage(&self) -> PathBuf {
        self.dir.join("usage.json")
    }

    fn blocklist(&self, category: &str) -> PathBuf {
        self.dir.join("blocklists").join(format!("{category}.bin"))
    }

    fn checksum(&self, category: &str) -> PathBuf {
        self.dir.join("blocklists").join(format!("{category}.sha256"))
    }

    pub fn load_identity(&self) -> Result<Option<Identity>> {
        read_json(&self.identity())
    }

    pub fn save_identity(&self, identity: &Identity) -> Result<()> {
        write_private(&self.identity(), &serde_json::to_vec_pretty(identity)?)
    }

    /// Dernière politique connue (D3). Absente tant que le serveur n'a rien envoyé.
    pub fn load_state(&self) -> Result<Option<DeviceState>> {
        read_json(&self.state())
    }

    pub fn save_state(&self, state: &DeviceState) -> Result<()> {
        write_private(&self.state(), &serde_json::to_vec(state)?)
    }

    pub fn load_usage(&self) -> UsageStore {
        read_json(&self.usage()).ok().flatten().unwrap_or_default()
    }

    pub fn save_usage(&self, usage: &UsageStore) -> Result<()> {
        write_private(&self.usage(), &serde_json::to_vec(usage)?)
    }

    /// Somme de contrôle de la liste présente sur disque, s'il y en a une.
    pub fn blocklist_checksum(&self, category: &str) -> Option<String> {
        std::fs::read_to_string(self.checksum(category)).ok().map(|s| s.trim().to_string())
    }

    pub fn load_blocklist(&self, category: &str) -> Result<DomainSet> {
        let bytes = std::fs::read(self.blocklist(category))?;
        Ok(DomainSet::from_bytes(&bytes)?)
    }

    pub fn save_blocklist(&self, category: &str, bytes: &[u8], checksum: &str) -> Result<()> {
        write_private(&self.blocklist(category), bytes)?;
        write_private(&self.checksum(category), checksum.as_bytes())
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read(path) {
        Ok(raw) => Ok(Some(
            serde_json::from_slice(&raw)
                .with_context(|| format!("lecture de {}", path.display()))?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("lecture de {}", path.display())),
    }
}

/// Écrit de façon atomique un fichier réservé à son propriétaire.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = crate::platform::create_private(&tmp)
        .with_context(|| format!("écriture de {}", tmp.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Temps d'écran local : compte → journée → secondes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageStore {
    accounts: BTreeMap<String, BTreeMap<NaiveDate, u32>>,
}

impl UsageStore {
    pub fn add(&mut self, account: &str, day: NaiveDate, seconds: u32) {
        *self.accounts.entry(account.to_string()).or_default().entry(day).or_default() += seconds;
    }

    pub fn seconds(&self, account: &str, day: NaiveDate) -> u32 {
        self.accounts.get(account).and_then(|days| days.get(&day)).copied().unwrap_or(0)
    }

    /// Secondes cumulées depuis le lundi de la semaine de `day`.
    pub fn week_seconds(&self, account: &str, day: NaiveDate) -> u32 {
        let monday = day - Duration::days(i64::from(day.weekday().num_days_from_monday()));
        self.accounts.get(account).map_or(0, |days| days.range(monday..=day).map(|(_, s)| *s).sum())
    }

    /// Oublie ce qui est plus ancien que deux semaines.
    pub fn prune(&mut self, today: NaiveDate) {
        let oldest = today - Duration::days(14);
        for days in self.accounts.values_mut() {
            days.retain(|day, _| *day >= oldest);
        }
        self.accounts.retain(|_, days| !days.is_empty());
    }

    #[cfg(test)]
    pub fn accounts(&self) -> impl Iterator<Item = &str> {
        self.accounts.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn usage_sums_by_day_and_week() {
        let mut usage = UsageStore::default();
        // Le 7 octobre 2026 est un mercredi ; le lundi de la semaine est le 5.
        usage.add("louis", d(2026, 10, 4), 1000); // dimanche précédent
        usage.add("louis", d(2026, 10, 5), 600);
        usage.add("louis", d(2026, 10, 7), 300);
        usage.add("louis", d(2026, 10, 7), 60);
        assert_eq!(usage.seconds("louis", d(2026, 10, 7)), 360);
        assert_eq!(usage.week_seconds("louis", d(2026, 10, 7)), 960);
        assert_eq!(usage.seconds("emma", d(2026, 10, 7)), 0);
    }

    #[test]
    fn prune_drops_old_days() {
        let mut usage = UsageStore::default();
        usage.add("louis", d(2026, 9, 1), 100);
        usage.add("louis", d(2026, 10, 7), 100);
        usage.add("emma", d(2026, 9, 1), 100);
        usage.prune(d(2026, 10, 7));
        assert_eq!(usage.seconds("louis", d(2026, 9, 1)), 0);
        assert_eq!(usage.accounts().collect::<Vec<_>>(), ["louis"]);
    }

    #[test]
    fn files_are_private_and_roundtrip() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("cotutelle-test-{}", std::process::id()));
        let paths = Paths::new(dir.clone());
        let mut usage = UsageStore::default();
        usage.add("louis", d(2026, 10, 7), 42);
        paths.save_usage(&usage).unwrap();
        assert_eq!(paths.load_usage(), usage);
        let mode = std::fs::metadata(dir.join("usage.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(paths.load_state().unwrap().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
