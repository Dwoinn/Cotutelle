//! État vivant de l'agent, partagé entre le résolveur DNS, la boucle de
//! surveillance des sessions et la synchronisation avec le serveur.

use crate::platform::{Platform, Session};
use crate::store::{Identity, Paths, UsageStore};
use chrono::{DateTime, Datelike, Local, NaiveDate};
use cotutelle_common::protocol::{AccountState, AgentMessage, DeviceState};
use cotutelle_common::session::{
    AgentEvent, Notice, OpenGrant, Opening, ServiceRef, SessionState, minutes_label,
};
use cotutelle_common::stats::DnsStats;
use cotutelle_common::{
    AccessStatus, BlockReason, Blocklists, FilterContext, FilterPolicy, GrantTarget, HolidayPeriod,
    Service, Usage, Verdict, catalog, evaluate, evaluate_access,
};
use cotutelle_dns::{Decision, Handler};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Secours quand l'état ne fournit aucun résolveur : DNS4EU « Protective »,
/// comme le serveur.
const DEFAULT_UPSTREAMS: &[&str] = &["86.54.11.1:53", "86.54.11.201:53"];
/// Un site bloqué n'est signalé qu'une fois par quart d'heure : une page en
/// sollicite des dizaines de fois.
const BLOCKED_NOTICE_EVERY: Duration = Duration::from_secs(15 * 60);

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
    /// Sessions telles que vues au dernier pas de surveillance.
    pub sessions: RwLock<Vec<Session>>,
    /// Compteurs DNS par compte, vidés vers le serveur chaque minute.
    pub stats: Mutex<DnsStats<String>>,
    /// Résolveurs annoncés par le réseau, rafraîchis périodiquement.
    pub network_dns: RwLock<Vec<SocketAddr>>,
    /// Messages en attente d'envoi au serveur.
    pub outbox: Mutex<Vec<AgentMessage>>,
    pub connected: AtomicBool,
    /// Assistants de session connectés, par compte (D19).
    pub helpers: Mutex<HashMap<String, Vec<mpsc::UnboundedSender<AgentEvent>>>>,
    /// Dernier état envoyé à l'assistant de chaque compte.
    pub helper_states: Mutex<HashMap<String, AgentEvent>>,
    /// Derniers avis de blocage, par (compte, service ou site).
    pub blocked_notices: Mutex<HashMap<(String, String), Instant>>,
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

    /// Périodes de vacances scolaires reçues avec l'état.
    pub fn holidays(&self) -> Vec<HolidayPeriod> {
        self.state
            .read()
            .expect("verrou état")
            .as_ref()
            .map(|s| s.holidays.clone())
            .unwrap_or_default()
    }

    pub fn foreground(&self) -> Option<String> {
        self.foreground.read().expect("verrou premier plan").clone()
    }

    pub fn sessions(&self) -> Vec<Session> {
        self.sessions.read().expect("verrou sessions").clone()
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

    /// État d'accès d'un compte géré à l'instant `now`, selon les règles du
    /// jour (période scolaire ou vacances).
    pub fn access(
        &self,
        account: &AccountState,
        holidays: &[HolidayPeriod],
        now: DateTime<Local>,
    ) -> AccessStatus {
        let usage = self.total_usage(account, now.date_naive());
        evaluate_access(
            account.policy.rules_on(now.date_naive(), holidays),
            &account.grants,
            now.to_utc(),
            now.weekday(),
            now.time(),
            usage,
        )
    }

    /// Enregistre un assistant pour ce compte et rend le flux de ses événements.
    pub fn attach_helper(&self, account: &str) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.helpers
            .lock()
            .expect("verrou assistants")
            .entry(account.to_string())
            .or_default()
            .push(tx);
        rx
    }

    pub fn helper_connected(&self, account: &str) -> bool {
        let mut helpers = self.helpers.lock().expect("verrou assistants");
        prune_helpers(&mut helpers);
        helpers.contains_key(account)
    }

    /// Comptes ayant un assistant connecté.
    pub fn helper_accounts(&self) -> Vec<String> {
        let mut helpers = self.helpers.lock().expect("verrou assistants");
        prune_helpers(&mut helpers);
        helpers.keys().cloned().collect()
    }

    /// Transmet un événement aux assistants d'un compte ; `false` si aucun
    /// n'est connecté : l'appelant se rabat sur les notifications du bureau.
    pub fn tell_helper(&self, account: &str, event: AgentEvent) -> bool {
        let mut helpers = self.helpers.lock().expect("verrou assistants");
        let Some(list) = helpers.get_mut(account) else { return false };
        list.retain(|tx| tx.send(event.clone()).is_ok());
        if list.is_empty() {
            helpers.remove(account);
            return false;
        }
        true
    }

    /// Situation d'un compte géré, telle que son assistant la montre.
    pub fn session_state(
        &self,
        account: &AccountState,
        holidays: &[HolidayPeriod],
        now: DateTime<Local>,
    ) -> SessionState {
        let rules = account.policy.rules_on(now.date_naive(), holidays);
        let access = self.access(account, holidays, now);
        let (day, time) = (now.weekday(), now.time());
        let closes_at =
            rules.schedule.ranges_for(day).iter().find(|r| r.contains(time)).and_then(|r| r.end);
        let next_opening = (!access.open)
            .then(|| rules.schedule.next_opening(day, time))
            .flatten()
            .map(|(day, time)| Opening { day, time });
        let state = self.state.read().expect("verrou état");
        let services = state.as_ref().map_or(&[][..], |s| s.services.as_slice());
        let grants = account
            .grants
            .iter()
            .filter(|g| g.is_active_at(now.to_utc()) && !matches!(g.target, GrantTarget::Pause))
            .map(|g| OpenGrant {
                label: grant_label(services, &g.target),
                expires_at: g.expires_at,
            })
            .collect();
        let filter = rules.filter;
        let requestable_services = services
            .iter()
            .filter(|s| {
                filter.blocked_services.contains(&s.id)
                    && filter.requestable_services.contains(&s.id)
            })
            .map(|s| ServiceRef { id: s.id.clone(), label: s.label.clone() })
            .collect();
        SessionState {
            child_name: account.child_name.clone(),
            access,
            closes_at,
            next_opening,
            holiday: rules.holiday.map(|h| h.label.clone()),
            grants,
            allow_requests: filter.allow_requests,
            requestable_services,
            server_connected: self.is_connected(),
            dry_run: self.dry_run,
        }
    }

    /// Ce qu'un assistant doit savoir de son compte à cet instant.
    pub fn helper_event(&self, account: &str, now: DateTime<Local>) -> AgentEvent {
        match self.account(account) {
            Some(account) => AgentEvent::State(self.session_state(&account, &self.holidays(), now)),
            None => AgentEvent::Unmanaged,
        }
    }

    /// Envoie à chaque assistant la situation de son compte si elle a changé.
    pub fn push_helper_states(&self, now: DateTime<Local>) {
        for account in self.helper_accounts() {
            let event = self.helper_event(&account, now);
            let mut last = self.helper_states.lock().expect("verrou états assistants");
            if last.get(&account) == Some(&event) {
                continue;
            }
            if self.tell_helper(&account, event.clone()) {
                last.insert(account, event);
            } else {
                last.remove(&account);
            }
        }
    }

    /// Signale un blocage à l'assistant du compte, une fois par service ou
    /// par site et par quart d'heure.
    fn notice_blocked(
        &self,
        account: &str,
        name: &str,
        reason: &BlockReason,
        services: &[Service],
        filter: &FilterPolicy,
    ) {
        let service = match reason {
            BlockReason::Service(id) => services.iter().find(|s| s.id == *id),
            BlockReason::DenyList | BlockReason::Category(_) => None,
            // Pas un site : rien à expliquer.
            BlockReason::Canary | BlockReason::Paused => return,
        };
        if !self.helper_connected(account) {
            return;
        }
        let domain = site_of(name);
        let key = service.map_or_else(|| domain.clone(), |s| s.id.clone());
        {
            let mut recent = self.blocked_notices.lock().expect("verrou avis de blocage");
            let now = Instant::now();
            recent.retain(|_, at| now.duration_since(*at) < BLOCKED_NOTICE_EVERY);
            let key = (account.to_string(), key);
            if recent.contains_key(&key) {
                return;
            }
            recent.insert(key, now);
        }
        let requestable = filter.allow_requests
            && service.is_some_and(|s| filter.requestable_services.contains(&s.id));
        tracing::info!(account, domain, ?reason, requestable, "site bloqué signalé à l'assistant");
        let notice = Notice::Blocked {
            domain,
            reason: reason.clone(),
            service: service.map(|s| ServiceRef { id: s.id.clone(), label: s.label.clone() }),
            requestable,
        };
        self.tell_helper(account, AgentEvent::Notice(notice));
    }
}

/// Oublie les assistants partis.
fn prune_helpers(helpers: &mut HashMap<String, Vec<mpsc::UnboundedSender<AgentEvent>>>) {
    helpers.retain(|_, list| {
        list.retain(|tx| !tx.is_closed());
        !list.is_empty()
    });
}

/// Libellé d'une ouverture pour l'enfant : « YouTube », « 30 min de plus ».
fn grant_label(services: &[Service], target: &GrantTarget) -> String {
    match target {
        GrantTarget::Service { service } => services
            .iter()
            .find(|s| s.id == *service)
            .map_or_else(|| service.clone(), |s| s.label.clone()),
        GrantTarget::Category { category } => catalog::CATEGORIES
            .iter()
            .find(|c| c.id == category)
            .map_or_else(|| category.clone(), |c| c.label.to_string()),
        GrantTarget::Domain { domain } => domain.clone(),
        GrantTarget::ExtraMinutes { minutes } => format!("{} de plus", minutes_label(*minutes)),
        GrantTarget::IgnoreSchedule => "Hors horaires".to_string(),
        GrantTarget::Pause => "Pause".to_string(),
    }
}

/// Le site tel que l'enfant le voit dans sa barre d'adresse : `i.ytimg.com`
/// devient `ytimg.com`, `www.bbc.co.uk` devient `bbc.co.uk`.
fn site_of(name: &str) -> String {
    let labels: Vec<&str> = name.split('.').filter(|l| !l.is_empty()).collect();
    let n = labels.len();
    if n <= 2 {
        return name.to_string();
    }
    // Suffixes à deux niveaux les plus courants (`co.uk`, `com.au`) : sans
    // liste publique complète, on garde trois labels dans ce cas.
    let second_level = matches!(labels[n - 2], "co" | "com" | "org" | "net" | "gov" | "ac" | "edu")
        && labels[n - 1].len() == 2;
    let keep = if second_level { 3 } else { 2 };
    labels[n - keep..].join(".")
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
        let state = self.state.read().expect("verrou état");
        let Some(state) = state.as_ref() else { return Decision::Forward };
        let Some(account) = state.accounts.iter().find(|a| a.os_account == os_account) else {
            return Decision::Forward;
        };

        let now = chrono::Local::now();
        let rules = account.policy.rules_on(now.date_naive(), &state.holidays);
        let blocklists = self.blocklists.read().expect("verrou listes");
        let verdict = evaluate(
            name,
            &FilterContext {
                policy: rules.filter,
                grants: &account.grants,
                blocklists: &blocklists,
                services: &state.services,
                now: chrono::Utc::now(),
                // Accès fermé : la session est verrouillée (D4) et, en seconde
                // barrière si le verrou ne tient pas, le DNS est coupé. Le
                // serveur reste joignable pour apprendre la réouverture.
                access_open: self.access(account, &state.holidays, now).open
                    || cotutelle_common::normalize_domain(name)
                        == server_host(&self.identity.server),
            },
        );
        let blocked = match &verdict {
            Verdict::Block(reason) => Some(reason),
            _ => None,
        };
        self.stats.lock().expect("verrou stats").record(&os_account, name, blocked);
        if let Some(reason) = blocked {
            self.notice_blocked(&os_account, name, reason, &state.services, rules.filter);
        }
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
    use super::{server_host, site_of};

    #[test]
    fn site_is_the_address_bar_domain() {
        assert_eq!(site_of("i.ytimg.com"), "ytimg.com");
        assert_eq!(site_of("www.youtube.com"), "youtube.com");
        assert_eq!(site_of("www.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(site_of("example.org"), "example.org");
    }

    #[test]
    fn server_host_is_extracted_from_url() {
        assert_eq!(server_host("http://cotutelle.local:8080"), "cotutelle.local");
        assert_eq!(server_host("https://cotutelle.example/"), "cotutelle.example");
        assert_eq!(server_host("http://192.168.1.10:8080/chemin"), "192.168.1.10");
        assert_eq!(server_host("http://[fd00::1]:8080"), "fd00::1");
    }
}
