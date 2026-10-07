//! Moteur de filtrage : ensembles de domaines compacts et verdict par requête.

use crate::catalog;
use crate::policy::{FilterPolicy, GrantTarget, TemporaryGrant};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Met un nom de domaine sous forme canonique : minuscules, sans point final.
pub fn normalize_domain(domain: &str) -> String {
    domain.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Itère sur un domaine et ses domaines parents : `a.b.c`, `b.c`, `c`.
pub fn suffixes(domain: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(domain);
    std::iter::from_fn(move || {
        let current = rest?;
        rest = current.split_once('.').map(|(_, parent)| parent);
        Some(current)
    })
}

/// Vrai si `domain` est `entry` ou l'un de ses sous-domaines.
pub fn domain_matches(domain: &str, entry: &str) -> bool {
    domain == entry
        || (domain.len() > entry.len()
            && domain.ends_with(entry)
            && domain.as_bytes()[domain.len() - entry.len() - 1] == b'.')
}

/// Hachage FNV-1a 64 bits, stable entre versions et plateformes.
pub fn hash_domain(domain: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in domain.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Ensemble de domaines stocké sous forme de hachages triés.
///
/// Huit octets par domaine : la catégorie `adult` d'UT1 (plusieurs millions
/// d'entrées) tient dans quelques dizaines de Mo. Le risque de faux positif
/// est de l'ordre de 10⁻¹² par requête.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomainSet {
    hashes: Vec<u64>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DomainSetError {
    #[error("taille de liste invalide : {0} octets")]
    InvalidLength(usize),
}

impl DomainSet {
    /// Construit l'ensemble depuis un fichier texte, un domaine par ligne.
    /// Les lignes vides et les commentaires `#` sont ignorés.
    pub fn from_text(text: &str) -> Self {
        Self::from_domains(text.lines())
    }

    pub fn from_domains<'a>(domains: impl IntoIterator<Item = &'a str>) -> Self {
        let mut hashes: Vec<u64> = domains
            .into_iter()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| hash_domain(&normalize_domain(l)))
            .collect();
        hashes.sort_unstable();
        hashes.dedup();
        Self { hashes }
    }

    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Vrai si le domaine (déjà normalisé) ou l'un de ses parents est listé.
    pub fn matches(&self, domain: &str) -> bool {
        suffixes(domain).any(|s| self.hashes.binary_search(&hash_domain(s)).is_ok())
    }

    /// Format d'échange : hachages triés en petit-boutiste, 8 octets chacun.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hashes.iter().flat_map(|h| h.to_le_bytes()).collect()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DomainSetError> {
        if !bytes.len().is_multiple_of(8) {
            return Err(DomainSetError::InvalidLength(bytes.len()));
        }
        let mut hashes: Vec<u64> =
            bytes.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c)).collect();
        // Ne fait pas confiance à l'ordre reçu : la recherche dichotomique en dépend.
        if !hashes.is_sorted() {
            hashes.sort_unstable();
        }
        hashes.dedup();
        Ok(Self { hashes })
    }
}

/// Listes chargées en mémoire, indexées par identifiant de catégorie.
#[derive(Debug, Clone, Default)]
pub struct Blocklists {
    categories: HashMap<String, Arc<DomainSet>>,
}

impl Blocklists {
    pub fn insert(&mut self, category: impl Into<String>, set: DomainSet) {
        self.categories.insert(category.into(), Arc::new(set));
    }

    pub fn get(&self, category: &str) -> Option<&DomainSet> {
        self.categories.get(category).map(Arc::as_ref)
    }

    pub fn contains(&self, category: &str) -> bool {
        self.categories.contains_key(category)
    }

    pub fn remove(&mut self, category: &str) {
        self.categories.remove(category);
    }

    pub fn categories(&self) -> impl Iterator<Item = &str> {
        self.categories.keys().map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum BlockReason {
    /// Domaine canari : répondu en NXDOMAIN pour désactiver le DNS chiffré.
    Canary,
    /// Hors plage horaire ou quota épuisé (appareils sans agent).
    Paused,
    /// Liste noire personnelle.
    DenyList,
    Service(String),
    Category(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block(BlockReason),
    /// Répondre par un CNAME vers ce nom (mode restreint YouTube).
    Rewrite(&'static str),
}

impl Verdict {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Verdict::Block(_))
    }
}

/// Tout ce qu'il faut pour décider du sort d'une requête.
pub struct FilterContext<'a> {
    pub policy: &'a FilterPolicy,
    pub grants: &'a [TemporaryGrant],
    pub blocklists: &'a Blocklists,
    pub now: DateTime<Utc>,
    /// Faux quand l'accès est fermé (horaires, quota) et que le DNS doit
    /// tout bloquer. Les agents, qui verrouillent la session, passent `true`.
    pub access_open: bool,
}

fn matches_any<'a>(domain: &str, entries: impl IntoIterator<Item = &'a String>) -> bool {
    entries.into_iter().any(|e| domain_matches(domain, &normalize_domain(e)))
}

fn service_matches(service_id: &str, domain: &str) -> bool {
    catalog::service(service_id)
        .is_some_and(|s| s.domains.iter().any(|d| domain_matches(domain, d)))
}

/// Décide du sort d'une requête DNS pour `domain`.
pub fn evaluate(domain: &str, ctx: &FilterContext<'_>) -> Verdict {
    let domain = normalize_domain(domain);
    let domain = domain.as_str();

    if catalog::CANARY_DOMAINS.iter().any(|c| domain_matches(domain, c)) {
        return Verdict::Block(BlockReason::Canary);
    }
    if !ctx.access_open {
        return Verdict::Block(BlockReason::Paused);
    }

    let active: Vec<&GrantTarget> =
        ctx.grants.iter().filter(|g| g.is_active_at(ctx.now)).map(|g| &g.target).collect();

    let allowed_by_parent = matches_any(domain, &ctx.policy.allow)
        || active.iter().any(|t| match t {
            GrantTarget::Domain { domain: d } => domain_matches(domain, &normalize_domain(d)),
            // Un service débloqué l'emporte aussi sur les catégories qui le contiennent.
            GrantTarget::Service { service } => service_matches(service, domain),
            _ => false,
        });

    if !allowed_by_parent {
        if matches_any(domain, &ctx.policy.deny) {
            return Verdict::Block(BlockReason::DenyList);
        }
        for service in &ctx.policy.blocked_services {
            if service_matches(service, domain) {
                return Verdict::Block(BlockReason::Service(service.clone()));
            }
        }
        for category in &ctx.policy.blocked_categories {
            let lifted = active
                .iter()
                .any(|t| matches!(t, GrantTarget::Category { category: c } if c == category));
            if lifted {
                continue;
            }
            if ctx.blocklists.get(category).is_some_and(|set| set.matches(domain)) {
                return Verdict::Block(BlockReason::Category(category.clone()));
            }
        }
    }

    if ctx.policy.youtube_restricted && catalog::YOUTUBE_RESTRICT_HOSTS.contains(&domain) {
        return Verdict::Rewrite(catalog::YOUTUBE_RESTRICT_TARGET);
    }
    Verdict::Allow
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GrantId;
    use chrono::Duration;

    fn lists() -> Blocklists {
        let mut b = Blocklists::default();
        b.insert(
            "adult",
            DomainSet::from_text("# commentaire\nexample-adult.com\n\nBad.Example.org\n"),
        );
        b.insert("audio-video", DomainSet::from_text("youtube.com\ndailymotion.com\n"));
        b
    }

    fn policy() -> FilterPolicy {
        FilterPolicy {
            blocked_categories: ["adult".to_string(), "audio-video".to_string()].into(),
            blocked_services: ["youtube".to_string()].into(),
            ..Default::default()
        }
    }

    fn grant(target: GrantTarget, now: DateTime<Utc>, minutes: i64) -> TemporaryGrant {
        TemporaryGrant {
            id: GrantId::new(),
            target,
            starts_at: now - Duration::minutes(1),
            expires_at: now + Duration::minutes(minutes),
        }
    }

    fn eval(
        domain: &str,
        policy: &FilterPolicy,
        grants: &[TemporaryGrant],
        now: DateTime<Utc>,
    ) -> Verdict {
        let blocklists = lists();
        evaluate(
            domain,
            &FilterContext { policy, grants, blocklists: &blocklists, now, access_open: true },
        )
    }

    #[test]
    fn suffix_walk() {
        let s: Vec<_> = suffixes("a.b.c").collect();
        assert_eq!(s, ["a.b.c", "b.c", "c"]);
    }

    #[test]
    fn domain_match_requires_label_boundary() {
        assert!(domain_matches("www.youtube.com", "youtube.com"));
        assert!(domain_matches("youtube.com", "youtube.com"));
        assert!(!domain_matches("notyoutube.com", "youtube.com"));
    }

    #[test]
    fn set_matches_subdomains_case_insensitively() {
        let set = DomainSet::from_text("Bad.Example.org\n");
        assert!(set.matches("bad.example.org"));
        assert!(set.matches("cdn.bad.example.org"));
        assert!(!set.matches("example.org"));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn set_roundtrips_through_bytes() {
        let set = DomainSet::from_text("a.com\nb.com\nc.com\n");
        let back = DomainSet::from_bytes(&set.to_bytes()).unwrap();
        assert_eq!(set, back);
        assert_eq!(DomainSet::from_bytes(&[1, 2, 3]), Err(DomainSetError::InvalidLength(3)));
    }

    #[test]
    fn category_and_service_block() {
        let now = Utc::now();
        let p = policy();
        assert_eq!(
            eval("www.example-adult.com.", &p, &[], now),
            Verdict::Block(BlockReason::Category("adult".into()))
        );
        assert_eq!(
            eval("r3---sn.googlevideo.com", &p, &[], now),
            Verdict::Block(BlockReason::Service("youtube".into()))
        );
        assert_eq!(eval("fr.wikipedia.org", &p, &[], now), Verdict::Allow);
    }

    #[test]
    fn service_grant_overrides_service_and_category() {
        let now = Utc::now();
        let p = policy();
        let g = [grant(GrantTarget::Service { service: "youtube".into() }, now, 60)];
        // youtube.com est aussi dans la catégorie audio-video : le grant l'emporte.
        assert_eq!(eval("www.youtube.com", &p, &g, now), Verdict::Allow);
        // … mais pas pour un autre site de la même catégorie.
        assert!(eval("www.dailymotion.com", &p, &g, now).is_blocked());
    }

    #[test]
    fn expired_grant_is_ignored() {
        let now = Utc::now();
        let p = policy();
        let g = [grant(GrantTarget::Service { service: "youtube".into() }, now, 60)];
        assert!(eval("www.youtube.com", &p, &g, now + Duration::minutes(61)).is_blocked());
    }

    #[test]
    fn allow_list_beats_categories_and_deny() {
        let now = Utc::now();
        let mut p = policy();
        p.allow.insert("example-adult.com".into());
        p.deny.insert("example-adult.com".into());
        assert_eq!(eval("example-adult.com", &p, &[], now), Verdict::Allow);
        p.allow.clear();
        assert_eq!(eval("example-adult.com", &p, &[], now), Verdict::Block(BlockReason::DenyList));
    }

    #[test]
    fn youtube_restricted_rewrites_when_allowed() {
        let now = Utc::now();
        let p = FilterPolicy { youtube_restricted: true, ..Default::default() };
        assert_eq!(eval("www.youtube.com", &p, &[], now), Verdict::Rewrite("restrict.youtube.com"));
        assert_eq!(eval("i.ytimg.com", &p, &[], now), Verdict::Allow);
    }

    #[test]
    fn canary_and_pause() {
        let now = Utc::now();
        let p = FilterPolicy::default();
        assert_eq!(
            eval("use-application-dns.net", &p, &[], now),
            Verdict::Block(BlockReason::Canary)
        );
        let blocklists = lists();
        let ctx = FilterContext {
            policy: &p,
            grants: &[],
            blocklists: &blocklists,
            now,
            access_open: false,
        };
        assert_eq!(evaluate("fr.wikipedia.org", &ctx), Verdict::Block(BlockReason::Paused));
    }
}
