//! État vivant de l'agent, partagé entre le résolveur DNS, la boucle de
//! surveillance des sessions et la synchronisation avec le serveur.

use crate::platform::Platform;
use crate::store::{Identity, Paths, UsageStore};
use chrono::{DateTime, Datelike, Local, NaiveDate};
use cotutelle_common::protocol::{AccountState, AgentMessage, DeviceState};
use cotutelle_common::stats::DnsStats;
use cotutelle_common::{
    AccessStatus, Blocklists, FilterContext, Usage, Verdict, evaluate, evaluate_access,
};
use cotutelle_dns::{Decision, Handler};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

const DEFAULT_UPSTREAMS: &[&str] = &["9.9.9.9:53", "149.112.112.112:53"];

pub struct Runtime {
    pub identity: Identity,
    pub paths: Paths,
    pub platform: Box<dyn Platform>,
    /// Mode d'essai : journalise les verrouillages et notifications sans les exécuter.
    pub dry_run: bool,
    pub http: reqwest::Client,
    pub state: RwLock<Option<DeviceState>>,
    pub blocklists: RwLock<Blocklists>,
    /// (catégorie, somme de contrôle) des listes actuellement en mémoire.
    pub loaded_lists: Mutex<Vec<(String, String)>>,
    pub usage: Mutex<UsageStore>,
    /// Compte actuellement devant l'écran, s'il y en a un.
    pub foreground: RwLock<Option<String>>,
    /// Compteurs DNS par compte, vidés vers le serveur chaque minute.
    pub stats: Mutex<DnsStats<String>>,
    /// Résolveurs annoncés par le réseau, rafraîchis périodiquement.
    pub network_dns: RwLock<Vec<SocketAddr>>,
    /// Messages en attente d'envoi au serveur.
    pub outbox: Mutex<Vec<AgentMessage>>,
    pub connected: AtomicBool,
}

impl Runtime {
    pub fn account(&self, os_account: &str) -> Option<AccountState> {
        let state = self.state.read().expect("verrou état");
        state.as_ref()?.accounts.iter().find(|a| a.os_account == os_account).cloned()
    }

    pub fn managed_accounts(&self) -> Vec<AccountState> {
        self.state
            .read()
            .expect("verrou état")
            .as_ref()
            .map(|s| s.accounts.clone())
            .unwrap_or_default()
    }

    pub fn foreground(&self) -> Option<String> {
        self.foreground.read().expect("verrou premier plan").clone()
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Relit les résolveurs du réseau. Une lecture vide (systemd-resolved en
    /// cours de redémarrage) ne remplace pas la liste connue.
    pub fn refresh_network_dns(&self) {
        let servers = self.platform.network_dns();
        if !servers.is_empty() {
            *self.network_dns.write().expect("verrou DNS réseau") = servers;
        }
    }

    pub fn queue(&self, message: AgentMessage) {
        self.outbox.lock().expect("verrou file").push(message);
    }

    /// Temps consommé par un compte, tous appareils confondus.
    pub fn total_usage(&self, account: &AccountState, today: NaiveDate) -> Usage {
        let usage = self.usage.lock().expect("verrou temps");
        Usage {
            today_minutes: usage.seconds(&account.os_account, today) / 60
                + account.usage_elsewhere.today_minutes,
            week_minutes: usage.week_seconds(&account.os_account, today) / 60
                + account.usage_elsewhere.week_minutes,
        }
    }

    /// État d'accès d'un compte géré à l'instant `now`.
    pub fn access(&self, account: &AccountState, now: DateTime<Local>) -> AccessStatus {
        let usage = self.total_usage(account, now.date_naive());
        evaluate_access(
            &account.policy,
            &account.grants,
            now.to_utc(),
            now.weekday(),
            now.time(),
            usage,
        )
    }
}

/// Nom d'hôte du serveur, extrait de son adresse (`http://hôte:port/…`).
fn server_host(server: &str) -> &str {
    let rest = server.split_once("://").map_or(server, |(_, rest)| rest);
    let authority = rest.split(['/', '?']).next().unwrap_or(rest);
    match authority.strip_prefix('[') {
        // Adresse IPv6 littérale : `[::1]:8080`.
        Some(v6) => v6.split(']').next().unwrap_or(v6),
        None => authority.rsplit_once(':').map_or(authority, |(host, _)| host),
    }
}

impl Handler for Runtime {
    fn decide(&self, _client: IpAddr, name: &str) -> Decision {
        // Personne devant l'écran, ou un compte non géré (un parent) : pas de filtrage.
        let Some(os_account) = self.foreground() else { return Decision::Forward };
        let Some(account) = self.account(&os_account) else { return Decision::Forward };

        let blocklists = self.blocklists.read().expect("verrou listes");
        let verdict = evaluate(
            name,
            &FilterContext {
                policy: &account.policy.filter,
                grants: &account.grants,
                blocklists: &blocklists,
                now: chrono::Utc::now(),
                // Accès fermé : la session est verrouillée (D4) et, en seconde
                // barrière si le verrou ne tient pas, le DNS est coupé. Le
                // serveur reste joignable pour apprendre la réouverture.
                access_open: self.access(&account, chrono::Local::now()).open
                    || cotutelle_common::normalize_domain(name)
                        == server_host(&self.identity.server),
            },
        );
        let blocked = match &verdict {
            Verdict::Block(reason) => Some(reason),
            _ => None,
        };
        self.stats.lock().expect("verrou stats").record(&os_account, name, blocked);
        verdict.into()
    }

    fn upstreams(&self) -> Vec<SocketAddr> {
        let state = self.state.read().expect("verrou état");
        let configured: Vec<SocketAddr> = state
            .as_ref()
            .map(|s| s.upstream_dns.iter().filter_map(|u| u.parse().ok()).collect())
            .unwrap_or_default();
        let fallback: Vec<SocketAddr> = if configured.is_empty() {
            DEFAULT_UPSTREAMS.iter().filter_map(|u| u.parse().ok()).collect()
        } else {
            configured
        };
        // D'abord les DNS du réseau : eux seuls connaissent les noms privés
        // (serveur Cotutelle en interne, NAS, portail captif). Les résolveurs
        // configurés restent en secours. Le filtrage, lui, a déjà eu lieu.
        let mut upstreams = self.network_dns.read().expect("verrou DNS réseau").clone();
        for addr in fallback {
            if !upstreams.contains(&addr) {
                upstreams.push(addr);
            }
        }
        upstreams
    }
}

#[cfg(test)]
mod tests {
    use super::server_host;

    #[test]
    fn server_host_is_extracted_from_url() {
        assert_eq!(server_host("http://cotutelle.local:8080"), "cotutelle.local");
        assert_eq!(server_host("https://cotutelle.example/"), "cotutelle.example");
        assert_eq!(server_host("http://192.168.1.10:8080/chemin"), "192.168.1.10");
        assert_eq!(server_host("http://[fd00::1]:8080"), "fd00::1");
    }
}
