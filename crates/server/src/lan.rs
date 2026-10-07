//! DNS du réseau local (D5) : filtre les appareils sans agent selon leur
//! adresse IP. Un appareil inconnu n'est ni filtré ni journalisé.

use crate::model;
use crate::state::Shared;
use crate::util;
use anyhow::Result;
use cotutelle_common::stats::DnsStats;
use cotutelle_common::{FilterContext, Policy, TemporaryGrant, Usage, Verdict, evaluate};
use cotutelle_dns::{Decision, Handler};
use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, RwLock};

use crate::blocklists::BlocklistStore;

pub const DEFAULT_UPSTREAMS: &str = "9.9.9.9:53, 149.112.112.112:53";

#[derive(Debug, Clone)]
struct Profile {
    device_id: String,
    /// Enfant auquel l'appareil est attribué, `''` pour un appareil partagé.
    child_id: String,
    policy: Policy,
    grants: Vec<TemporaryGrant>,
    usage: Usage,
}

pub struct LanFilter {
    profiles: RwLock<HashMap<IpAddr, Profile>>,
    upstreams: RwLock<Vec<SocketAddr>>,
    blocklists: Arc<BlocklistStore>,
    /// Compteurs par (appareil, enfant), vidés chaque minute vers la base.
    stats: Mutex<DnsStats<(String, String)>>,
}

impl LanFilter {
    pub fn new(blocklists: Arc<BlocklistStore>) -> Self {
        Self {
            profiles: RwLock::default(),
            upstreams: RwLock::new(parse_upstreams(DEFAULT_UPSTREAMS)),
            blocklists,
            stats: Mutex::default(),
        }
    }
}

/// Lit une liste `ip[:port]` séparée par des virgules ; le port vaut 53 par défaut.
pub fn parse_upstreams(value: &str) -> Vec<SocketAddr> {
    value
        .split([',', ' ', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            s.parse::<SocketAddr>()
                .ok()
                .or_else(|| s.parse::<IpAddr>().ok().map(|ip| (ip, 53).into()))
        })
        .collect()
}

fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

impl Handler for LanFilter {
    fn decide(&self, client: IpAddr, name: &str) -> Decision {
        let profiles = self.profiles.read().expect("verrou profils");
        let Some(profile) = profiles.get(&canonical(client)) else {
            return Decision::Forward;
        };
        let access = model::access_now(&profile.policy, &profile.grants, profile.usage);
        let verdict = self.blocklists.with_loaded(|blocklists| {
            evaluate(
                name,
                &FilterContext {
                    policy: &profile.policy.filter,
                    grants: &profile.grants,
                    blocklists,
                    now: chrono::Utc::now(),
                    access_open: access.open,
                },
            )
        });
        let blocked = match &verdict {
            Verdict::Block(reason) => Some(reason),
            _ => None,
        };
        self.stats.lock().expect("verrou stats").record(
            &(profile.device_id.clone(), profile.child_id.clone()),
            name,
            blocked,
        );
        verdict.into()
    }

    fn upstreams(&self) -> Vec<SocketAddr> {
        self.upstreams.read().expect("verrou amonts").clone()
    }
}

/// Recharge les profils des appareils réseau et les listes dont ils ont besoin.
pub async fn rebuild(state: &Shared) -> Result<()> {
    type Row = (String, Option<String>, Option<String>, Option<String>);
    let rows: Vec<Row> =
        sqlx::query_as("SELECT id, ip, child_id, policy FROM devices WHERE kind = 'network'")
            .fetch_all(&state.db)
            .await?;
    let grants = model::live_grants(&state.db).await?;

    let mut profiles = HashMap::new();
    let mut categories = BTreeSet::new();
    for (device_id, ip, child_id, policy) in rows {
        let Some(ip) = ip.and_then(|ip| ip.parse::<IpAddr>().ok()) else { continue };
        let mut device_grants = model::grants_of_device(&grants, &device_id);
        let (policy, usage) = match &child_id {
            Some(child_id) => {
                let Some(child) = model::child(&state.db, child_id).await? else { continue };
                device_grants.extend(model::grants_of_child(&grants, child_id));
                let usage = model::usage_from_seconds(
                    model::usage_seconds(&state.db, child_id, None).await?,
                );
                (child.policy, usage)
            }
            None => (
                policy.and_then(|p| serde_json::from_str(&p).ok()).unwrap_or_default(),
                Usage::default(),
            ),
        };
        categories.extend(policy.filter.blocked_categories.iter().cloned());
        profiles.insert(
            canonical(ip),
            Profile {
                device_id,
                child_id: child_id.unwrap_or_default(),
                policy,
                grants: device_grants,
                usage,
            },
        );
    }

    // Chargement de listes volumineuses : hors du fil asynchrone.
    let blocklists = state.blocklists.clone();
    tokio::task::spawn_blocking(move || blocklists.set_loaded(&categories)).await?;

    let upstreams = upstreams_setting(state).await;
    *state.lan.upstreams.write().expect("verrou amonts") = upstreams;
    *state.lan.profiles.write().expect("verrou profils") = profiles;
    Ok(())
}

pub async fn upstreams_setting(state: &Shared) -> Vec<SocketAddr> {
    let configured = crate::notify::setting(state, "upstream_dns").await;
    let parsed = parse_upstreams(configured.as_deref().unwrap_or(DEFAULT_UPSTREAMS));
    if parsed.is_empty() { parse_upstreams(DEFAULT_UPSTREAMS) } else { parsed }
}

/// Ajoute des compteurs DNS à l'agrégat quotidien.
pub async fn store_counts(
    state: &Shared,
    day: &str,
    device_id: &str,
    child_id: &str,
    counts: &[cotutelle_common::protocol::DomainCount],
) -> Result<()> {
    let mut tx = state.db.begin().await?;
    for count in counts {
        let reason = count.reason.as_ref().and_then(|r| serde_json::to_string(r).ok());
        sqlx::query(
            "INSERT INTO dns_daily (day, device_id, child_id, domain, allowed, blocked, reason)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (day, device_id, child_id, domain) DO UPDATE SET
                allowed = allowed + excluded.allowed,
                blocked = blocked + excluded.blocked,
                reason = COALESCE(excluded.reason, reason)",
        )
        .bind(day)
        .bind(device_id)
        .bind(child_id)
        .bind(&count.domain)
        .bind(count.allowed)
        .bind(count.blocked)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Vide les compteurs du DNS LAN vers la base.
pub async fn flush_stats(state: &Shared) -> Result<()> {
    let drained = state.lan.stats.lock().expect("verrou stats").drain();
    let day = util::day_str(util::today());
    for ((device_id, child_id), counts) in drained {
        store_counts(state, &day, &device_id, &child_id, &counts).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstreams_accept_ports_and_bare_addresses() {
        let parsed = parse_upstreams("9.9.9.9, 1.1.1.1:5353\n[2620:fe::fe]:53 pas-une-ip");
        let expected: Vec<SocketAddr> = ["9.9.9.9:53", "1.1.1.1:5353", "[2620:fe::fe]:53"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(parsed, expected);
    }

    #[test]
    fn mapped_ipv4_is_canonicalised() {
        let mapped: IpAddr = "::ffff:192.168.1.20".parse().unwrap();
        assert_eq!(canonical(mapped), "192.168.1.20".parse::<IpAddr>().unwrap());
    }
}
