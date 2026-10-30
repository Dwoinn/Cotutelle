//! Surveillance des sessions : décompte du temps d'écran, préavis et
//! verrouillage quand l'accès se ferme (D4).

use crate::platform::Session;
use crate::runtime::Runtime;
use chrono::{DateTime, Datelike, Local, NaiveTime, Weekday};
use cotutelle_common::protocol::{AccountState, AgentMessage, TamperKind};
use cotutelle_common::session::{AgentEvent, Notice, Opening};
use cotutelle_common::{AccessStatus, ClosedReason, GrantTarget, HolidayPeriod};
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
/// Avec un assistant de session, l'accès fermé laisse ce délai avant le
/// verrou : le temps de lire le motif et de demander du temps.
const HELPER_GRACE_SECONDS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Lock(Session),
    /// Avis à montrer dans la session : par l'assistant s'il est là, sinon
    /// par les notifications du bureau.
    Inform(Session, Notice),
}

/// Préavis déjà affichés pour un compte, pour ne pas les répéter.
#[derive(Debug, Default, Clone, Copy)]
pub struct Warned {
    ten: bool,
    five: bool,
    one: bool,
    /// Depuis quand l'accès est fermé sans interruption.
    closed_since: Option<DateTime<Local>>,
    /// Dernier affichage du message « accès fermé ».
    closed_notice: Option<DateTime<Local>>,
}

const END_OF_DAY: NaiveTime = NaiveTime::from_hms_opt(23, 59, 59).expect("heure valide");

/// Explique une fermeture : le motif, et quand ça rouvre.
fn closed_notice(
    account: &AccountState,
    holidays: &[HolidayPeriod],
    status: AccessStatus,
    now: DateTime<Local>,
    grace_seconds: u32,
) -> Notice {
    let rules = account.policy.rules_on(now.date_naive(), holidays);
    let today = now.weekday();
    let label = |(day, time)| Opening { day, time }.label(today);
    let reopens = match status.reason {
        Some(ClosedReason::OutsideSchedule) => {
            rules.schedule.next_opening(today, now.time()).map(label)
        }
        // Le quota du jour est épuisé : la prochaine ouverture est demain.
        Some(ClosedReason::DailyQuota) => rules.schedule.next_opening(today, END_OF_DAY).map(label),
        Some(ClosedReason::WeeklyQuota) => {
            rules.schedule.next_opening(Weekday::Sun, END_OF_DAY).map(label)
        }
        Some(ClosedReason::Paused) => account
            .grants
            .iter()
            .filter(|g| matches!(g.target, GrantTarget::Pause) && g.is_active_at(now.to_utc()))
            .map(|g| g.expires_at.with_timezone(&Local))
            .max()
            .map(|at| label((at.weekday(), at.time()))),
        None => None,
    };
    Notice::Closed {
        reason: status.reason.unwrap_or(ClosedReason::DailyQuota),
        reopens,
        quota_minutes: status.quota_today_minutes,
        grace_seconds,
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
    *rt.sessions.write().expect("verrou sessions") = sessions.to_vec();

    // Le temps ne compte que pour un compte géré, devant l'écran et actif.
    if let Some(session) = foreground.filter(|s| !s.idle && rt.account(&s.user).is_some()) {
        rt.usage.lock().expect("verrou temps").add(
            &session.user,
            now.date_naive(),
            elapsed_seconds,
        );
    }

    let mut actions = Vec::new();
    let holidays = rt.holidays();
    for session in sessions.iter().filter(|s| s.in_use()) {
        let Some(account) = rt.account(&session.user) else { continue };
        let status: AccessStatus = rt.access(&account, &holidays, now);
        let memo = warned.entry(session.user.clone()).or_default();

        let helper = rt.helper_connected(&session.user);
        if !status.open {
            // Avec un assistant, la fermeture s'explique avant le verrou ;
            // sans lui, le verrou suit la notification tout de suite.
            let since = *memo.closed_since.get_or_insert(now);
            let grace = if helper {
                (HELPER_GRACE_SECONDS - (now - since).num_seconds()).max(0)
            } else {
                0
            };
            // Le verrouillage est retenté à chaque pas tant que la session
            // reste ouverte ; l'explication, elle, n'est pas répétée en rafale.
            let recently = memo
                .closed_notice
                .is_some_and(|at| (now - at).num_seconds() < CLOSED_NOTICE_EVERY_SECONDS);
            if !recently {
                memo.closed_notice = Some(now);
                let notice = closed_notice(&account, &holidays, status, now, grace as u32);
                actions.push(Action::Inform(session.clone(), notice));
            }
            if grace == 0 {
                actions.push(Action::Lock(session.clone()));
            }
            (memo.ten, memo.five, memo.one) = (false, false, false);
            continue;
        }
        memo.closed_notice = None;
        memo.closed_since = None;
        let warn = |minutes| Action::Inform(session.clone(), Notice::Warning { minutes });
        match status.remaining_minutes {
            Some(m) if m <= 1 && !memo.one => {
                (memo.ten, memo.five, memo.one) = (true, true, true);
                actions.push(warn(1));
            }
            Some(m) if m <= 5 && !memo.five => {
                (memo.ten, memo.five) = (true, true);
                actions.push(warn(5));
            }
            // Le préavis de dix minutes est réservé à l'assistant : sans lui,
            // l'agent garde ses deux rappels d'origine.
            Some(m) if m <= 10 && !memo.ten && helper => {
                memo.ten = true;
                actions.push(warn(10));
            }
            Some(m) if m > 10 => *memo = Warned::default(),
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
        Action::Inform(session, notice) => {
            let title = notice.title();
            tracing::info!(user = %session.user, title, dry_run = rt.dry_run, "avis");
            // L'assistant reçoit l'avis même en mode d'essai : c'est ainsi
            // qu'on regarde l'interface sans rien verrouiller.
            if rt.tell_helper(&session.user, AgentEvent::Notice(notice.clone())) || rt.dry_run {
                Ok(())
            } else {
                rt.platform.notify(session, &title, &notice.body())
            }
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

/// Montre un avis à un compte, par son assistant ou dans sa session en usage.
pub fn inform_account(rt: &Runtime, os_account: &str, notice: Notice) {
    if rt.tell_helper(os_account, AgentEvent::Notice(notice.clone())) {
        return;
    }
    if let Some(session) = rt.sessions().into_iter().find(|s| s.user == os_account && s.in_use()) {
        perform(rt, &Action::Inform(session, notice));
    }
}

/// Résumé des sessions pour le journal : `louis (en usage), papa (arrière-plan)`.
fn describe(sessions: &[Session]) -> String {
    if sessions.is_empty() {
        return "aucune".to_string();
    }
    sessions.iter().map(|s| format!("{} ({})", s.user, s.state())).collect::<Vec<_>>().join(", ")
}

/// Boucle de surveillance. `enforced` est l'adresse du résolveur local quand
/// la protection DNS est active et doit être vérifiée périodiquement.
pub async fn run(rt: Arc<Runtime>, enforced: Option<SocketAddr>) {
    let mut warned = HashMap::new();
    let mut lock_watch = LockWatch::default();
    let mut last_sessions: Option<Vec<Session>> = None;
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
                // Chaque changement d'état est journalisé : c'est ce qui
                // permet de comprendre après coup pourquoi le temps a compté.
                if last_sessions.as_ref() != Some(&sessions) {
                    tracing::info!(sessions = %describe(&sessions), "sessions");
                    last_sessions = Some(sessions.clone());
                }
                let actions = step(&rt, &sessions, elapsed, now, &mut warned);
                rt.push_helper_states(now);
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
        ChildId, DeviceId, GrantId, GrantTarget, HolidayPeriod, HolidayPolicy, Policy, Quota,
        TemporaryGrant, Usage,
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
        fn account_name(&self, _: u32) -> Option<String> {
            None
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
                services: vec![],
                upstream_dns: vec![],
                holidays: vec![],
            })
            .into(),
            blocklists: Default::default(),
            loaded_lists: Default::default(),
            usage: Default::default(),
            foreground: Default::default(),
            sessions: Default::default(),
            stats: Default::default(),
            network_dns: Default::default(),
            outbox: Default::default(),
            connected: Default::default(),
            helpers: Default::default(),
            helper_states: Default::default(),
            blocked_notices: Default::default(),
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
    fn holiday_rules_come_from_the_last_known_state() {
        // Quota de 60 min à l'école, 180 en vacances ; le 7 octobre est en vacances.
        let rt = runtime(60, vec![], 0);
        {
            let mut state = rt.state.write().unwrap();
            let state = state.as_mut().unwrap();
            state.accounts[0].policy.holidays = Some(HolidayPolicy {
                schedule: WeeklySchedule::always(),
                quota: Quota { daily_minutes: Some(180), weekly_minutes: None },
                ..Default::default()
            });
            state.holidays = vec![HolidayPeriod {
                label: "Essai".into(),
                start: noon().date_naive(),
                end: noon().date_naive(),
            }];
            state.accounts[0].usage_elsewhere.today_minutes = 100;
        }
        let mut warned = HashMap::new();
        // Sans serveur, l'agent applique les règles de vacances de son état.
        let actions = step(&rt, &[session("louis", false, false)], 5, noon(), &mut warned);
        assert!(actions.is_empty(), "100 min sur 180 : rien à signaler");
        // Le lendemain, hors période : le quota scolaire est dépassé.
        let tomorrow = noon() + chrono::Duration::days(1);
        let actions = step(&rt, &[session("louis", false, false)], 5, tomorrow, &mut warned);
        assert!(matches!(actions.as_slice(), [Action::Inform(..), Action::Lock(_)]));
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
        assert!(matches!(first.as_slice(), [Action::Inform(_, Notice::Warning { minutes: 5 })]));
        assert!(step(&rt, &sessions, 5, noon(), &mut warned).is_empty(), "pas de répétition");

        rt.state.write().unwrap().as_mut().unwrap().accounts[0].usage_elsewhere.today_minutes = 58;
        let second = step(&rt, &sessions, 5, noon(), &mut warned);
        assert!(matches!(second.as_slice(), [Action::Inform(_, Notice::Warning { minutes: 1 })]));
    }

    #[test]
    fn ten_minute_warning_is_for_the_helper_only() {
        let rt = runtime(60, vec![], 52);
        let sessions = [session("louis", false, false)];
        assert!(step(&rt, &sessions, 5, noon(), &mut HashMap::new()).is_empty());
        let _helper = rt.attach_helper("louis");
        let actions = step(&rt, &sessions, 5, noon(), &mut HashMap::new());
        assert!(matches!(actions.as_slice(), [Action::Inform(_, Notice::Warning { minutes: 10 })]));
    }

    #[test]
    fn locks_when_quota_is_spent_across_devices() {
        // 60 minutes de quota déjà consommées sur un autre appareil.
        let rt = runtime(60, vec![], 60);
        let actions = step(&rt, &[session("louis", false, false)], 5, noon(), &mut HashMap::new());
        assert!(matches!(
            actions.as_slice(),
            [Action::Inform(_, Notice::Closed { reason: ClosedReason::DailyQuota, quota_minutes: Some(60), grace_seconds: 0, .. }), Action::Lock(s)]
                if s.user == "louis"
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
        assert!(matches!(first.as_slice(), [Action::Inform(..), Action::Lock(_)]));
        // La session est toujours ouverte cinq secondes plus tard : on
        // reverrouille, sans réafficher le message.
        let later = noon() + ChronoDuration::seconds(5);
        let second = step(&rt, &sessions, 5, later, &mut warned);
        assert!(matches!(second.as_slice(), [Action::Lock(_)]));
        // Une minute après, l'enfant a rouvert sa session : on réexplique.
        let much_later = noon() + ChronoDuration::seconds(70);
        let third = step(&rt, &sessions, 5, much_later, &mut warned);
        assert!(matches!(third.as_slice(), [Action::Inform(..), Action::Lock(_)]));
    }

    #[test]
    fn with_a_helper_the_lock_waits_for_the_explanation() {
        let rt = runtime(60, vec![], 60);
        let mut helper = rt.attach_helper("louis");
        let mut warned = HashMap::new();
        let sessions = [session("louis", false, false)];
        let first = step(&rt, &sessions, 5, noon(), &mut warned);
        assert!(matches!(
            first.as_slice(),
            [Action::Inform(_, Notice::Closed { grace_seconds: 30, .. })]
        ));
        // L'avis part à l'assistant, pas aux notifications du bureau.
        assert!(perform(&rt, &first[0]));
        assert!(matches!(helper.try_recv(), Ok(AgentEvent::Notice(Notice::Closed { .. }))));
        let soon = noon() + ChronoDuration::seconds(10);
        assert!(step(&rt, &sessions, 5, soon, &mut warned).is_empty());
        let later = noon() + ChronoDuration::seconds(30);
        assert!(matches!(
            step(&rt, &sessions, 5, later, &mut warned).as_slice(),
            [Action::Lock(_)]
        ));
        // Assistant parti : plus de délai.
        drop(helper);
        let actions = step(&rt, &sessions, 5, noon(), &mut HashMap::new());
        assert!(matches!(actions.as_slice(), [Action::Inform(..), Action::Lock(_)]));
    }

    #[test]
    fn helper_state_says_what_the_child_can_ask() {
        let rt = runtime(60, vec![], 20);
        {
            let mut state = rt.state.write().unwrap();
            let state = state.as_mut().unwrap();
            state.services = cotutelle_common::catalog::builtin_services();
            let filter = &mut state.accounts[0].policy.filter;
            filter.blocked_services.insert("youtube".into());
            filter.requestable_services.insert("youtube".into());
            filter.allow_requests = true;
        }
        let mut helper = rt.attach_helper("louis");
        rt.push_helper_states(noon());
        let Ok(AgentEvent::State(state)) = helper.try_recv() else { panic!("état attendu") };
        assert_eq!(state.child_name, "Louis");
        assert_eq!(state.access.remaining_minutes, Some(40));
        assert_eq!(state.requestable_services.len(), 1);
        assert_eq!(state.requestable_services[0].label, "YouTube");
        assert!(state.allow_requests);
        // Rien de neuf : rien d'envoyé.
        rt.push_helper_states(noon());
        assert!(helper.try_recv().is_err());
        // Un compte non géré reçoit « non géré » une fois.
        let mut other = rt.attach_helper("papa");
        rt.push_helper_states(noon());
        assert!(matches!(other.try_recv(), Ok(AgentEvent::Unmanaged)));
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
            [
                Action::Inform(
                    _,
                    Notice::Closed { reason: ClosedReason::Paused, reopens: Some(_), .. }
                ),
                Action::Lock(_)
            ]
        ));
    }
}
