//! Agrégation des requêtes DNS par site (niveau 2 du suivi).

use crate::filter::BlockReason;
use crate::protocol::DomainCount;
use std::collections::HashMap;
use std::hash::Hash;

/// Suffixes de second niveau courants : `bbc.co.uk` est un site, pas `co.uk`.
const SECOND_LEVEL: &[&str] =
    &["co", "com", "org", "net", "gov", "gouv", "ac", "edu", "asso", "nom", "or", "ne", "go"];

/// Ramène un nom d'hôte au site qu'un parent reconnaît :
/// `r4---sn-abc.googlevideo.com` → `googlevideo.com`.
///
/// Heuristique volontairement simple, sans liste des suffixes publics.
pub fn site_of(domain: &str) -> &str {
    let labels: Vec<&str> = domain.split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return domain;
    }
    let keep =
        if labels[n - 1].len() == 2 && SECOND_LEVEL.contains(&labels[n - 2]) { 3 } else { 2 };
    let skip: usize = labels[..n - keep.min(n)].iter().map(|l| l.len() + 1).sum();
    &domain[skip..]
}

/// Noms sans intérêt pour le suivi : résolution inverse, réseau local.
pub fn is_noise(domain: &str) -> bool {
    !domain.contains('.')
        || domain.ends_with(".arpa")
        || domain.ends_with(".local")
        || domain.ends_with(".lan")
        || domain.ends_with(".home")
        || domain.ends_with(".internal")
}

/// Compteurs en mémoire, vidés périodiquement vers le stockage.
#[derive(Debug)]
pub struct DnsStats<K> {
    counts: HashMap<(K, String), DomainCount>,
}

impl<K> Default for DnsStats<K> {
    fn default() -> Self {
        Self { counts: HashMap::new() }
    }
}

impl<K: Hash + Eq + Clone> DnsStats<K> {
    /// Enregistre une requête. `blocked` porte le motif si elle a été bloquée.
    pub fn record(&mut self, key: &K, domain: &str, blocked: Option<&BlockReason>) {
        if is_noise(domain) {
            return;
        }
        let site = site_of(domain);
        let entry = self.counts.entry((key.clone(), site.to_string())).or_insert_with(|| {
            DomainCount { domain: site.to_string(), allowed: 0, blocked: 0, reason: None }
        });
        match blocked {
            Some(reason) => {
                entry.blocked += 1;
                entry.reason = Some(reason.clone());
            }
            None => entry.allowed += 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Rend les compteurs accumulés, regroupés par clé, et repart de zéro.
    pub fn drain(&mut self) -> HashMap<K, Vec<DomainCount>> {
        let mut out: HashMap<K, Vec<DomainCount>> = HashMap::new();
        for ((key, _), count) in self.counts.drain() {
            out.entry(key).or_default().push(count);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_keeps_registrable_part() {
        assert_eq!(site_of("r4---sn-abc.googlevideo.com"), "googlevideo.com");
        assert_eq!(site_of("fr.wikipedia.org"), "wikipedia.org");
        assert_eq!(site_of("www.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(site_of("www.impots.gouv.fr"), "impots.gouv.fr");
        assert_eq!(site_of("example.com"), "example.com");
        assert_eq!(site_of("localhost"), "localhost");
    }

    #[test]
    fn stats_group_by_key_and_site() {
        let mut stats: DnsStats<&str> = DnsStats::default();
        stats.record(&"louis", "www.youtube.com", Some(&BlockReason::Service("youtube".into())));
        stats.record(&"louis", "m.youtube.com", Some(&BlockReason::Service("youtube".into())));
        stats.record(&"louis", "fr.wikipedia.org", None);
        stats.record(&"louis", "1.0.168.192.in-addr.arpa", None);
        let mut drained = stats.drain();
        let mut louis = drained.remove("louis").unwrap();
        louis.sort_by(|a, b| a.domain.cmp(&b.domain));
        assert_eq!(louis.len(), 2);
        assert_eq!((louis[1].domain.as_str(), louis[1].blocked), ("youtube.com", 2));
        assert_eq!((louis[0].domain.as_str(), louis[0].allowed), ("wikipedia.org", 1));
        assert!(stats.is_empty());
    }
}
