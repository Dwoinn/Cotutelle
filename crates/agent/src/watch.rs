//! Surveillance des sessions : décompte du temps d'écran, préavis et
//! verrouillage quand l'accès se ferme (D4).

use crate::platform::Session;
use crate::runtime::Runtime;
use chrono::{DateTime, Local};
use cotutelle_common::protocol::{AgentMessage, TamperKind};
use cotutelle_common::{AccessStatus, ClosedReason};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_secs(5);
/// Au-delà, l'intervalle n'est pas du temps d'écran (veille, agent suspendu).
const MAX_STEP_SECONDS: u64 = 30;
const SAVE_EVERY: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(60);
/// Recul d'horloge toléré avant de le signaler.
const CLOCK_TOLERANCE_SECONDS: i64 = 120;
/// Tant que l'accès reste fermé, le message d'explication n'est pas répété
/// plus souvent que cela, même si la session est rouverte entre-temps.
const CLOSED_NOTICE_EVERY_SECONDS: i64 = 60;
/// Nombre d'échecs de verrouillage consécutifs avant d'alerter les parents.
const LOCK_FAILURES_BEFORE_ALERT: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Lock(Session),
    Notify(Session, &'static str, String),
}

/// Préavis déjà affichés pour un compte, pour ne pas les répéter.
#[derive(Debug, Default, Clone, Copy)]
pub struct Warned {
    five: bool,
    one: bool,
    /// Dernier affichage du message « accès fermé ».
    closed_notice: Option<DateTime<Local>>,
}

fn closed_message(reason: Option<ClosedReason>) -> (&'static str, &'static str) {
    match reason {
        Some(ClosedReason::Paused) => ("Écran en pause", "Tes parents ont mis l'écran en pause."),
        Some(ClosedReason::OutsideSchedule) => {
            ("Ce n'est pas l'heure", "L'écran est verrouillé en dehors de tes horaires.")
        }
        Some(ClosedReason::WeeklyQuota) => {
            ("Temps d'écran terminé", "Tu as utilisé tout ton temps pour cette semaine.")
        }
        Some(ClosedReason::DailyQuota) | None => {
            ("Temps d'écran terminé", "Tu as utilisé tout ton temps pour aujourd'hui.")
        }
    }
}

/// Un pas de surveillance : met à jour le temps d'écran et décide des actions.
/// Sans effet de bord sur le système, pour rester testable.
pub fn step(
    rt: &Runtime,
    sessions: &[Session],
    elapsed_seconds: u32,
    now: DateTime<Local>,
    warned: &mut HashMap<String, Warned>,
) -> Vec<Action> {
    let foreground = sessions.iter().find(|s| s.in_use());
    *rt.foreground.write().expect("verrou premier plan") = foreground.map(|s| s.user.clone());

    // Le temps ne compte que pour un compte géré, devant l'écran et actif.
    if let Some(session) = foreground.filter(|s| !s.idle && rt.account(&s.user).is_some()) {
        rt.usage.lock().expect("verrou temps").add(
            &session.user,
            now.date_naive(),
            elapsed_seconds,
        );
    }

    let mut actions = Vec::new();
    for session in sessions.iter().filter(|s| s.in_use()) {
        let Some(account) = rt.account(&session.user) else { continue };
        let status: AccessStatus = rt.access(&account, now);
        let memo = warned.entry(session.user.clone()).or_default();

        if !status.open {
            // Le verrouillage est retenté à chaque pas tant que la session
            // reste ouverte ; l'explication, elle, n'est pas répétée en rafale.
            let recently = memo
                .closed_notice
                .is_some_and(|at| (now - at).num_seconds() < CLOSED_NOTICE_EVERY_SECONDS);
            if !recently {
                let (title, body) = closed_message(status.reason);
                actions.push(Action::Notify(session.clone(), title, body.to_string()));
            }
            actions.push(Action::Lock(session.clone()));
            *memo = Warned {
                closed_notice: if recently { memo.closed_notice } else { Some(now) },
                ..Default::default()
            };
            continue;
        }
        memo.closed_notice = None;
        match status.remaining_minutes {
            Some(m) if m <= 1 && !memo.one => {
                memo.one = true;
                memo.five = true;
                actions.push(Action::Notify(
                    session.clone(),
                    "Plus qu'une minute",
                    "L'écran va se verrouiller.".to_string(),
                ));
            }
            Some(m) if m <= 5 && !memo.five => {
                memo.five = true;
                actions.push(Action::Notify(
                    session.clone(),
                    "Plus que 5 minutes",
                    "Pense à enregistrer ce que tu fais.".to_string(),
                ));
            }
            Some(m) if m > 5 => *memo = Warned::default(),
            None => *memo = Warned::default(),
            _ => {}
        }
    }
    actions
}

/// Exécute une action ; rend `false` si elle a échoué.
fn perform(rt: &Runtime, action: &Action) -> bool {
    let result = match action {
        Action::Lock(session) => {
            tracing::info!(user = %session.user, dry_run = rt.dry_run, "verrouillage de la session");
            if rt.dry_run { Ok(()) } else { rt.platform.lock_session(session) }
        }
        Action::Notify(session, title, body) => {
            tracing::info!(user = %session.user, title, dry_run = rt.dry_run, "notification");
            if rt.dry_run { Ok(()) } else { rt.platform.notify(session, title, body) }
        }
    };
    if let Err(e) = &result {
        tracing::warn!(error = %e, "action impossible");
    }
    result.is_ok()
}

/// Suit les échecs de verrouillage par compte et alerte les parents quand un
/// accès fermé ne peut pas être imposé. Une alerte par épisode.
#[derive(Default)]
struct LockWatch {
    failures: HashMap<String, u32>,
}

impl LockWatch {
    fn record(&mut self, rt: &Runtime, user: &str, locked: bool, now: DateTime<Local>) {
        if locked {
            self.failures.remove(user);
            return;
        }
        let count = self.failures.entry(user.to_string()).or_default();
        *count += 1;
        if *count == LOCK_FAILURES_BEFORE_ALERT {
            rt.queue(AgentMessage::Tamper {
                kind: TamperKind::LockFailed,
                at: now.to_utc(),
                details: format!("compte {user}"),
            });
        }
    }

    /// Oublie les comptes qui n'ont plus besoin d'être verrouillés.
    fn retain(&mut self, still_closed: &[String]) {
        self.failures.retain(|user, _| still_closed.contains(user));
    }
}

/// Verrouille immédiatement les sessions d'un compte (ordre du serveur).
pub fn lock_account(rt: &Runtime, os_account: &str) {
    let Ok(sessions) = rt.platform.sessions() else { return };
    for session in sessions.into_iter().filter(|s| s.user == os_account && !s.locked) {
        perform(rt, &Action::Lock(session));
    }
}

/// Boucle de surveillance. `enforced` est l'adresse du résolveur local quand
/// la protection DNS est active et doit être vérifiée périodiquement.
pub async fn run(rt: Arc<Runtime>, enforced: Option<SocketAddr>) {
    let mut warned = HashMap::new();
    let mut lock_watch = LockWatch::default();
    let mut last_tick = Instant::now();
    let mut last_save = Instant::now();
    let mut last_check = Instant::now();
    let mut last_wall = Local::now();
    let mut interval = tokio::time::interval(TICK);

    loop {
        interval.tick().await;
        let elapsed = last_tick.elapsed().as_secs().min(MAX_STEP_SECONDS) as u32;
        last_tick = Instant::now();
        let now = Local::now();

        // L'horloge murale a reculé alors que le temps a avancé : à signaler.
        if (last_wall - now).num_seconds() > CLOCK_TOLERANCE_SECONDS {
            rt.queue(AgentMessage::Tamper {
                kind: TamperKind::ClockChanged,
                at: now.to_utc(),
                details: format!("de {} à {}", last_wall.format("%H:%M"), now.format("%H:%M")),
            });
        }
        last_wall = now;

        let sessions = {
            let rt = rt.clone();
            tokio::task::spawn_blocking(move || rt.platform.sessions()).await
        };
        match sessions {
            Ok(Ok(sessions)) => {
                let actions = step(&rt, &sessions, elapsed, now, &mut warned);
                // Verrouiller attend la confirmation du bureau : hors du fil asynchrone.
                let outcomes = {
                    let rt = rt.clone();
                    tokio::task::spawn_blocking(move || {
                        actions.into_iter().map(|a| (perform(&rt, &a), a)).collect::<Vec<_>>()
                    })
                    .await
                    .unwrap_or_default()
                };
                let mut closed = Vec::new();
                for (ok, action) in outcomes {
                    if let Action::Lock(session) = action {
                        lock_watch.record(&rt, &session.user, ok, now);
                        closed.push(session.user);
                    }
                }
                lock_watch.retain(&closed);
            }
            Ok(Err(e)) => tracing::warn!(error = %e, "lecture des sessions"),
            Err(e) => tracing::warn!(error = %e, "lecture des sessions"),
        }

        if last_save.elapsed() >= SAVE_EVERY {
            last_save = Instant::now();
            // Le réseau a pu changer (Wi-Fi, câble, retour à la maison).
            let refresh = rt.clone();
            let _ = tokio::task::spawn_blocking(move || refresh.refresh_network_dns()).await;
            let mut usage = rt.usage.lock().expect("verrou temps");
            usage.prune(now.date_naive());
            if let Err(e) = rt.paths.save_usage(&usage) {
                tracing::warn!(error = %e, "sauvegarde du temps d'écran");
            }
        }

        if let Some(resolver) = enforced.filter(|_| last_check.elapsed() >= CHECK_EVERY) {
            last_check = Instant::now();
            if let Some((kind, details)) = rt.platform.check_enforcement(resolver) {
                tracing::warn!(?kind, details, "protection altérée, remise en place");
                rt.queue(AgentMessage::Tamper { kind, at: now.to_utc(), details });
                if let Err(e) = rt.platform.enforce_dns(resolver) {
                    tracing::error!(error = %e, "remise en place de la protection DNS");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::store::{Identity, Paths};
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use cotutelle_common::protocol::{AccountState, DeviceState};
    use cotutelle_common::schedule::WeeklySchedule;
    use cotutelle_common::{
        ChildId, DeviceId, GrantId, GrantTarget, Policy, Quota, TemporaryGrant, Usage,
    };

    struct NoPlatform;

    impl Platform for NoPlatform {
        fn name(&self) -> &'static str {
            "test"
        }
        fn hostname(&self) -> String {
            "test".into()
        }
        fn os_accounts(&self) -> Vec<String> {
            vec![]
        }
        fn sessions(&self) -> anyhow::Result<Vec<Session>> {
            Ok(vec![])
        }
        fn lock_session(&self, _: &Session) -> anyhow::Result<()> {
            Ok(())
        }
        fn notify(&self, _: &Session, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn network_dns(&self) -> Vec<SocketAddr> {
            vec![]
        }
        fn enforce_dns(&self, _: SocketAddr) -> anyhow::Result<()> {
            Ok(())
        }
        fn release_dns(&self) -> anyhow::Result<()> {
            Ok(())
        }
        fn check_enforcement(&self, _: SocketAddr) -> Option<(TamperKind, String)> {
            None
        }
    }

    /// Agent gérant le compte `louis` avec un quota journalier en minutes.
    fn runtime(daily_minutes: u32, grants: Vec<TemporaryGrant>, elsewhere_minutes: u32) -> Runtime {
        let account = AccountState {
            os_account: "louis".into(),
            child: ChildId::new(),
            child_name: "Louis".into(),
            policy: Policy {
                schedule: WeeklySchedule::always(),
                quota: Quota { daily_minutes: Some(daily_minutes), weekly_minutes: None },
                ..Default::default()
            },
            grants,
            usage_elsewhere: Usage {
                today_minutes: elsewhere_minutes,
                week_minutes: elsewhere_minutes,
            },
        };
        let device = DeviceId::new();
        Runtime {
            identity: Identity { server: "http://test".into(), device, token: "t".into() },
            paths: Paths::new(std::env::temp_dir().join("cotutelle-unused")),
            platform: Box::new(NoPlatform),
            dry_run: true,
            http: reqwest::Client::new(),
            state: Some(DeviceState {
                device,
                issued_at: Utc::now(),
                accounts: vec![account],
                blocklists: vec![],
                upstream_dns: vec![],
            })
            .into(),
            blocklists: Default::default(),
            loaded_lists: Default::default(),
            usage: Default::default(),
            foreground: Default::default(),
            stats: Default::default(),
            network_dns: Default::default(),
            outbox: Default::default(),
            connected: Default::default(),
        }
    }

    fn session(user: &str, locked: bool, idle: bool) -> Session {
        Session { id: "1".into(), user: user.into(), uid: 1001, active: true, locked, idle }
    }

    fn noon() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
    }

    fn used(rt: &Runtime) -> u32 {
        rt.usage.lock().unwrap().seconds("louis", noon().date_naive())
    }

    #[test]
    fn counts_time_only_when_in_use_and_not_idle() {
        let rt = runtime(60, vec![], 0);
        let mut warned = HashMap::new();
        step(&rt, &[session("louis", false, false)], 5, noon(), &mut warned);
        assert_eq!(used(&rt), 5);
        assert_eq!(rt.foreground().as_deref(), Some("louis"));

        step(&rt, &[session("louis", false, true)], 5, noon(), &mut warned);
        step(&rt, &[session("louis", true, false)], 5, noon(), &mut warned);
        assert_eq!(used(&rt), 5, "ni l'inactivité ni l'écran verrouillé ne comptent");
        assert_eq!(rt.foreground(), None, "une session verrouillée n'est pas au premier plan");
    }

    #[test]
    fn unmanaged_account_is_left_alone() {
        let rt = runtime(0, vec![], 0);
        let actions = step(&rt, &[session("papa", false, false)], 5, noon(), &mut HashMap::new());
        assert!(actions.is_empty());
        assert_eq!(rt.foreground().as_deref(), Some("papa"));
    }

    #[test]
    fn warns_once_at_five_minutes_then_at_one() {
        let rt = runtime(60, vec![], 0);
        let mut warned = HashMap::new();
        let sessions = [session("louis", false, false)];

        // 54 min consommées ailleurs + 60 s ici : il reste 5 minutes.
        rt.state.write().unwrap().as_mut().unwrap().accounts[0].usage_elsewhere.today_minutes = 54;
        let first = step(&rt, &sessions, 60, noon(), &mut warned);
        assert!(matches!(first.as_slice(), [Action::Notify(_, "Plus que 5 minutes", _)]));
        assert!(step(&rt, &sessions, 5, noon(), &mut warned).is_empty(), "pas de répétition");

        rt.state.write().unwrap().as_mut().unwrap().accounts[0].usage_elsewhere.today_minutes = 58;
        let second = step(&rt, &sessions, 5, noon(), &mut warned);
        assert!(matches!(second.as_slice(), [Action::Notify(_, "Plus qu'une minute", _)]));
    }

    #[test]
    fn locks_when_quota_is_spent_across_devices() {
        // 60 minutes de quota déjà consommées sur un autre appareil.
        let rt = runtime(60, vec![], 60);
        let actions = step(&rt, &[session("louis", false, false)], 5, noon(), &mut HashMap::new());
        assert!(matches!(
            actions.as_slice(),
            [Action::Notify(_, "Temps d'écran terminé", _), Action::Lock(s)] if s.user == "louis"
        ));
        // Une fois verrouillée, la session n'est plus sollicitée.
        assert!(
            step(&rt, &[session("louis", true, false)], 5, noon(), &mut HashMap::new()).is_empty()
        );
    }

    #[test]
    fn closed_notice_is_not_repeated_while_lock_is_retried() {
        let rt = runtime(60, vec![], 60);
        let mut warned = HashMap::new();
        let sessions = [session("louis", false, false)];
        let first = step(&rt, &sessions, 5, noon(), &mut warned);
        assert!(matches!(first.as_slice(), [Action::Notify(..), Action::Lock(_)]));
        // La session est toujours ouverte cinq secondes plus tard : on
        // reverrouille, sans réafficher le message.
        let later = noon() + ChronoDuration::seconds(5);
        let second = step(&rt, &sessions, 5, later, &mut warned);
        assert!(matches!(second.as_slice(), [Action::Lock(_)]));
        // Une minute après, l'enfant a rouvert sa session : on réexplique.
        let much_later = noon() + ChronoDuration::seconds(70);
        let third = step(&rt, &sessions, 5, much_later, &mut warned);
        assert!(matches!(third.as_slice(), [Action::Notify(..), Action::Lock(_)]));
    }

    #[test]
    fn lock_failures_alert_parents_once_per_episode() {
        let rt = runtime(60, vec![], 60);
        let mut watch = LockWatch::default();
        for _ in 0..5 {
            watch.record(&rt, "louis", false, noon());
        }
        let alerts = |rt: &Runtime| rt.outbox.lock().unwrap().len();
        assert_eq!(alerts(&rt), 1);
        // Verrou réussi puis nouvel échec prolongé : nouvel épisode, nouvelle alerte.
        watch.record(&rt, "louis", true, noon());
        for _ in 0..3 {
            watch.record(&rt, "louis", false, noon());
        }
        assert_eq!(alerts(&rt), 2);
    }

    #[test]
    fn bonus_minutes_reopen_and_pause_closes() {
        let now = noon();
        let grant = |target| TemporaryGrant {
            id: GrantId::new(),
            target,
            starts_at: now.to_utc() - ChronoDuration::minutes(1),
            expires_at: now.to_utc() + ChronoDuration::minutes(30),
        };
        let sessions = [session("louis", false, false)];

        let bonus = runtime(60, vec![grant(GrantTarget::ExtraMinutes { minutes: 30 })], 60);
        assert!(step(&bonus, &sessions, 5, now, &mut HashMap::new()).is_empty());

        let paused = runtime(600, vec![grant(GrantTarget::Pause)], 0);
        let actions = step(&paused, &sessions, 5, now, &mut HashMap::new());
        assert!(matches!(
            actions.as_slice(),
            [Action::Notify(_, "Écran en pause", _), Action::Lock(_)]
        ));
    }
}
