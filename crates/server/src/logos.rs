//! Logos des services : l'icône du site principal, récupérée une fois par le
//! serveur puis servie par lui. L'appareil d'un enfant ne pourrait pas la
//! charger lui-même, le site étant justement bloqué chez lui.
//!
//! Le serveur ne contacte que le site du service, jamais un intermédiaire,
//! et refuse tout ce qui n'est pas un nom public en HTTPS : un nom saisi par
//! un parent ne doit pas servir à sonder le réseau local.

use crate::state::Shared;
use crate::util;
use anyhow::{Result, bail};
use cotutelle_common::Service;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, Url, redirect};
use serde::Serialize;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
/// Durée maximale de la recherche d'un logo, toutes tentatives comprises.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Une page d'accueil déclare ses icônes dans son en-tête : inutile de lire plus.
const MAX_PAGE: usize = 512 * 1024;
const MAX_IMAGE: usize = 256 * 1024;
const MAX_CANDIDATES: usize = 6;
/// Délai avant de retenter une recherche restée sans résultat.
const RETRY_AFTER: i64 = 86_400;
/// Certains sites ne répondent qu'à ce qui ressemble à un navigateur ; le
/// serveur se présente donc ainsi, en se nommant à la fin.
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0 Cotutelle/",
    env!("CARGO_PKG_VERSION")
);

/// Ce que l'interface reçoit pour afficher un logo.
#[derive(Debug, Clone, Serialize)]
pub struct LogoRef {
    pub url: String,
    /// Icône pleine et opaque, à afficher bord à bord.
    pub bleed: bool,
}

/// Logos disponibles, par identifiant de service.
pub async fn index(state: &Shared) -> Result<HashMap<String, LogoRef>> {
    let rows: Vec<(String, bool, i64)> = sqlx::query_as(
        "SELECT service_id, full_bleed, fetched_at FROM service_logos WHERE image IS NOT NULL",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, bleed, fetched_at)| {
            // L'horodatage change l'adresse quand le logo change : cache sans péremption.
            let url = format!("/api/v1/services/{id}/logo?v={fetched_at}");
            (id, LogoRef { url, bleed })
        })
        .collect())
}

/// Image d'un logo et son type, pour la route qui la sert.
pub async fn image(state: &Shared, service_id: &str) -> Result<Option<(String, Vec<u8>)>> {
    Ok(sqlx::query_as(
        "SELECT content_type, image FROM service_logos WHERE service_id = ? AND image IS NOT NULL",
    )
    .bind(service_id)
    .fetch_optional(&state.db)
    .await?)
}

/// Cherche le logo d'un service et enregistre le résultat, trouvé ou non.
pub async fn refresh(state: &Shared, service: &Service) {
    let Some(domain) = service.domains.first() else { return };
    let found = tokio::time::timeout(SEARCH_TIMEOUT, find(domain)).await.ok().flatten();
    if found.is_none() {
        tracing::info!(service = %service.id, domain, "aucun logo trouvé");
    }
    let (content_type, image, full_bleed) = match found {
        Some(logo) => (Some(logo.content_type), Some(logo.bytes), logo.full_bleed),
        None => (None, None, false),
    };
    let stored = sqlx::query(
        "INSERT INTO service_logos (service_id, domain, content_type, image, full_bleed, fetched_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (service_id) DO UPDATE SET
            domain = excluded.domain, content_type = excluded.content_type,
            image = excluded.image, full_bleed = excluded.full_bleed, fetched_at = excluded.fetched_at",
    )
    .bind(&service.id)
    .bind(domain)
    .bind(content_type)
    .bind(image)
    .bind(full_bleed)
    .bind(util::now())
    .execute(&state.db)
    .await;
    if let Err(e) = stored {
        tracing::error!(service = %service.id, error = ?e, "enregistrement du logo");
    }
}

/// Récupère les logos manquants : service nouveau, site principal changé, ou
/// recherche infructueuse assez ancienne pour être retentée.
pub async fn ensure(state: &Shared, services: &[Service]) -> Result<()> {
    let known: Vec<(String, String, bool, i64)> = sqlx::query_as(
        "SELECT service_id, domain, image IS NOT NULL, fetched_at FROM service_logos",
    )
    .fetch_all(&state.db)
    .await?;
    let now = util::now();
    for service in services {
        let Some(domain) = service.domains.first() else { continue };
        let fresh = known.iter().any(|(id, from, found, at)| {
            id == &service.id && from == domain && (*found || now - at < RETRY_AFTER)
        });
        if !fresh {
            refresh(state, service).await;
        }
    }
    Ok(())
}

struct Logo {
    content_type: &'static str,
    bytes: Vec<u8>,
    full_bleed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    url: Url,
    /// Côté annoncé de l'icône, en pixels.
    size: u32,
    full_bleed: bool,
}

/// Cherche le logo d'un site : les icônes que sa page d'accueil déclare,
/// puis les emplacements habituels.
async fn find(domain: &str) -> Option<Logo> {
    let mut candidates = Vec::new();
    let mut bases = Vec::new();
    // Certains sites ne répondent que sous `www.`.
    for host in [domain.to_string(), format!("www.{domain}")] {
        let Ok(url) = Url::parse(&format!("https://{host}/")) else { continue };
        if let Ok((base, html)) = fetch_page(&url).await {
            candidates = icon_links(&html, &base);
            bases = vec![base];
            break;
        }
        bases.push(url);
    }
    candidates.truncate(MAX_CANDIDATES);
    for base in &bases {
        for (path, size, full_bleed) in
            [("/apple-touch-icon.png", 180, true), ("/favicon.ico", 32, false)]
        {
            if let Ok(url) = base.join(path) {
                candidates.push(Candidate { url, size, full_bleed });
            }
        }
    }

    for candidate in candidates {
        match fetch_image(&candidate.url).await {
            Ok((content_type, bytes)) => {
                return Some(Logo { content_type, bytes, full_bleed: candidate.full_bleed });
            }
            Err(e) => tracing::debug!(url = %candidate.url, error = %e, "icône écartée"),
        }
    }
    None
}

async fn fetch_page(url: &Url) -> Result<(Url, String)> {
    let response = CLIENT.get(url.clone()).send().await?.error_for_status()?;
    let base = response.url().clone();
    let bytes = read_limited(response, MAX_PAGE, true).await?;
    Ok((base, String::from_utf8_lossy(&bytes).into_owned()))
}

async fn fetch_image(url: &Url) -> Result<(&'static str, Vec<u8>)> {
    let response = CLIENT.get(url.clone()).send().await?.error_for_status()?;
    let bytes = read_limited(response, MAX_IMAGE, false).await?;
    match sniff(&bytes) {
        Some(content_type) => Ok((content_type, bytes)),
        None => bail!("ce n'est pas une image"),
    }
}

/// Lit une réponse sans dépasser `limit` octets : au-delà, tronque ou échoue.
async fn read_limited(
    mut response: reqwest::Response,
    limit: usize,
    truncate: bool,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if out.len() + chunk.len() > limit {
            if !truncate {
                bail!("réponse de plus de {limit} octets");
            }
            out.extend_from_slice(&chunk[..limit - out.len()]);
            break;
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Reconnaît une image à ses premiers octets, sans se fier au type annoncé.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        [0, 0, 1, 0, ..] => Some("image/x-icon"),
        _ => {
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]);
            let head = head.trim_start_matches('\u{feff}').trim_start();
            (head.starts_with('<') && head.contains("<svg")).then_some("image/svg+xml")
        }
    }
}

/// Icônes déclarées par une page (`<link rel="icon">`, `apple-touch-icon`),
/// de la plus adaptée à la moins adaptée.
fn icon_links(html: &str, base: &Url) -> Vec<Candidate> {
    let lower = html.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<link") {
        let start = from + at + "<link".len();
        let Some(len) = lower[start..].find('>') else { break };
        from = start + len;
        let attrs = attributes(&html[start..from]);
        let get = |name: &str| attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
        let (Some(rel), Some(href)) = (get("rel"), get("href")) else { continue };
        let rel: Vec<String> = rel.split_whitespace().map(str::to_ascii_lowercase).collect();
        let has = |token: &str| rel.iter().any(|r| r == token);
        let full_bleed = has("apple-touch-icon") || has("apple-touch-icon-precomposed");
        if !full_bleed && !has("icon") {
            continue;
        }
        let Ok(url) = base.join(&href.trim().replace("&amp;", "&")) else { continue };
        if url.scheme() != "https" {
            continue;
        }
        let declared = get("sizes").and_then(|sizes| {
            sizes.split_whitespace().filter_map(|s| s.split(['x', 'X']).next()?.parse().ok()).max()
        });
        let size = declared.unwrap_or(if full_bleed { 180 } else { 32 });
        found.push(Candidate { url, size, full_bleed });
    }
    // Une icône d'application remplit sa pastille ; sinon, la plus grande.
    found.sort_by_key(|c| (!c.full_bleed, std::cmp::Reverse(c.size)));
    found.dedup_by(|a, b| a.url == b.url);
    found
}

/// Attributs d'une balise : `nom="valeur"`, `nom='valeur'`, `nom=valeur`.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag.trim_start();
    while !rest.is_empty() {
        let end = rest.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(rest.len());
        let name = rest[..end].trim_end_matches('/').to_ascii_lowercase();
        rest = rest[end..].trim_start();
        let mut value = "";
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            (value, rest) = match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let inner = &after[1..];
                    let close = inner.find(quote).unwrap_or(inner.len());
                    (&inner[..close], inner.get(close + 1..).unwrap_or(""))
                }
                _ => {
                    let close = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..close], &after[close..])
                }
            };
            rest = rest.trim_start();
        }
        if !name.is_empty() {
            out.push((name, value.to_string()));
        }
    }
    out
}

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(REQUEST_TIMEOUT)
        .https_only(true)
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("trop de redirections")
            } else if names_public_host(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirection hors d'Internet")
            }
        }))
        .dns_resolver(Arc::new(PublicResolver))
        .build()
        .expect("client HTTP des logos")
});

/// Vrai si l'adresse désigne son hôte par un nom, pas par une adresse IP :
/// une adresse littérale échapperait au contrôle du résolveur.
fn names_public_host(url: &Url) -> bool {
    url.host_str().is_some_and(|host| !host.starts_with('[') && host.parse::<Ipv4Addr>().is_err())
}

/// Résolveur qui ne rend que des adresses publiques : ni boucle locale, ni
/// réseau privé, ni lien local.
struct PublicResolver;

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let resolved = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let public: Vec<SocketAddr> = resolved.filter(|a| is_public(a.ip())).collect();
            if public.is_empty() {
                return Err("aucune adresse publique pour ce nom".into());
            }
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                // Adresses partagées des opérateurs (100.64.0.0/10) et réservées.
                || (a == 100 && (64..128).contains(&b))
                || a >= 240)
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public(IpAddr::V4(v4)),
            None => {
                let first = v6.segments()[0];
                !(v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    // Adresses locales uniques (fc00::/7) et de lien (fe80::/10).
                    || first & 0xfe00 == 0xfc00
                    || first & 0xffc0 == 0xfe80)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://www.example.com/accueil").unwrap()
    }

    #[test]
    fn icons_are_read_from_link_tags_and_ranked() {
        let html = r##"<html><head>
            <LINK REL="shortcut icon" href=/favicon.ico>
            <link rel="stylesheet" href="/style.css">
            <link href='https://cdn.example.com/icon-192.png?a=1&amp;b=2' sizes="96x96 192x192" rel=icon />
            <link rel="mask-icon" href="/pin.svg" color="#000">
            <link rel="apple-touch-icon" href="touch.png">
            <link rel="icon" href="http://example.com/clair.png" sizes="512x512">
        </head>"##;
        let found = icon_links(html, &base());
        let urls: Vec<_> = found.iter().map(|c| (c.url.as_str(), c.size, c.full_bleed)).collect();
        assert_eq!(
            urls,
            [
                ("https://www.example.com/touch.png", 180, true),
                ("https://cdn.example.com/icon-192.png?a=1&b=2", 192, false),
                ("https://www.example.com/favicon.ico", 32, false),
            ]
        );
    }

    #[test]
    fn images_are_recognised_by_content() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff(&[0, 0, 1, 0, 2, 0]), Some("image/x-icon"));
        assert_eq!(sniff(b"RIFF\x10\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"<?xml version=\"1.0\"?><svg xmlns=\"\">"), Some("image/svg+xml"));
        assert_eq!(sniff(b"<!doctype html><html>"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn only_public_addresses_are_contacted() {
        for ip in ["127.0.0.1", "10.1.2.3", "192.168.1.10", "169.254.169.254", "100.64.0.1"] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["::1", "fd00::1", "fe80::1", "::ffff:192.168.1.10"] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        assert!(is_public("9.9.9.9".parse().unwrap()));
        assert!(is_public("2620:fe::fe".parse().unwrap()));

        let named = |url: &str| names_public_host(&Url::parse(url).unwrap());
        assert!(named("https://www.youtube.com/"));
        assert!(!named("https://192.168.1.1/"));
        assert!(!named("https://[fd00::1]/"));
    }
}
