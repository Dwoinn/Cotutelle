//! Service Windows : `cotutelle-agent service` est le point d'entrée que le
//! gestionnaire de services lance ; `install` et `uninstall` l'enregistrent
//! et le retirent, avec ce qui l'entoure (droits du répertoire d'état,
//! démarrage de l'assistant de session, raccourci « Mon temps d'écran »).

use super::run;
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use windows::Win32::Foundation::ERROR_SERVICE_EXISTS;
use windows_registry::LOCAL_MACHINE;
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceDependency, ServiceErrorControl, ServiceExitCode, ServiceFailureActions,
    ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus,
    ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

pub const SERVICE_NAME: &str = "cotutelle-agent";
const DISPLAY_NAME: &str = "Agent Cotutelle (contrôle parental)";
const DESCRIPTION: &str = "Applique les règles de Cotutelle sur cet ordinateur : filtrage des sites, horaires et temps d'écran, verrouillage de la session.";
/// Démarrage automatique de l'assistant dans chaque session.
const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Cotutelle";
const SHORTCUT_NAME: &str = "Mon temps d'écran.url";
const STOP_TIMEOUT: Duration = Duration::from_secs(20);

/// Ordre d'arrêt reçu du gestionnaire de services.
static STOP: Notify = Notify::const_new();
/// Ce que le service lance, posé avant d'entrer dans le répartiteur.
type Launch = Box<dyn FnOnce() -> Result<()> + Send>;
static LAUNCH: Mutex<Option<Launch>> = Mutex::new(None);

/// Se résout quand le gestionnaire de services demande l'arrêt.
pub async fn stopped() {
    STOP.notified().await;
}

/// Entre dans le répartiteur des services : `main` tourne sur le fil du
/// service, jusqu'à l'ordre d'arrêt. Ne revient qu'à la fin du service.
pub fn dispatch(main: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
    *LAUNCH.lock().expect("verrou lancement") = Some(Box::new(main));
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).context(
        "répartiteur des services : `cotutelle-agent service` est réservé au gestionnaire de services, utilisez `run` en console",
    )
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    let Some(main) = LAUNCH.lock().expect("verrou lancement").take() else { return };
    let handler = |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown | ServiceControl::Preshutdown => {
            STOP.notify_one();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status = match service_control_handler::register(SERVICE_NAME, handler) {
        Ok(status) => status,
        Err(e) => {
            tracing::error!(error = %e, "enregistrement auprès du gestionnaire de services");
            return;
        }
    };
    let report = |state, exit_code| {
        let accepts = if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        };
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accepts,
            exit_code,
            checkpoint: 0,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        });
    };
    report(ServiceState::Running, ServiceExitCode::Win32(0));
    let outcome = main();
    if let Err(e) = &outcome {
        tracing::error!(error = %e, "le service s'arrête sur une erreur");
    }
    let exit = if outcome.is_ok() {
        ServiceExitCode::Win32(0)
    } else {
        ServiceExitCode::ServiceSpecific(1)
    };
    report(ServiceState::Stopped, exit);
}

fn service_info(executable: PathBuf) -> ServiceInfo {
    ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: DISPLAY_NAME.into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: executable,
        launch_arguments: vec!["service".into()],
        // Client DNS et pare-feu Windows : l'agent s'y appuie dès le départ.
        dependencies: vec![
            ServiceDependency::Service("Dnscache".into()),
            ServiceDependency::Service("MpsSvc".into()),
        ],
        // LocalSystem.
        account_name: None,
        account_password: None,
    }
}

/// Raccourci du menu Démarrer commun vers l'espace enfant.
fn shortcut_path() -> Option<PathBuf> {
    let program_data = std::env::var_os("ProgramData")?;
    Some(
        PathBuf::from(program_data)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join(SHORTCUT_NAME),
    )
}

/// Contenu d'un raccourci Internet vers l'agent local.
pub(crate) fn shortcut_content(local_addr: &str, icon: &Path) -> String {
    format!(
        "[InternetShortcut]\r\nURL=http://{local_addr}/\r\nIconFile={}\r\nIconIndex=0\r\n",
        icon.display()
    )
}

/// Enregistre le service et ce qui l'entoure. Rejouable : une mise à jour
/// reprend la configuration.
pub fn install(state_dir: &Path, local_addr: &str) -> Result<()> {
    let executable = std::env::current_exe().context("chemin de l'agent")?;
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .context("gestionnaire de services (lancez la console en administrateur)")?;
    let info = service_info(executable.clone());
    let access = ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS;
    let service = match manager.create_service(&info, access) {
        Ok(service) => service,
        Err(windows_service::Error::Winapi(e))
            if e.raw_os_error() == Some(ERROR_SERVICE_EXISTS.0 as i32) =>
        {
            let service = manager.open_service(SERVICE_NAME, access)?;
            service.change_config(&info)?;
            service
        }
        Err(e) => return Err(e).context("création du service"),
    };
    service.set_description(DESCRIPTION)?;
    // Relancé après un arrêt inattendu, comme `Restart=always` sous systemd.
    service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(3600)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![
            ServiceAction {
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(5)
            };
            3
        ]),
    })?;

    // Répertoire d'état réservé à SYSTEM et aux administrateurs.
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("création de {}", state_dir.display()))?;
    let dir = state_dir.to_string_lossy().into_owned();
    run(
        "icacls.exe",
        &[&dir, "/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"],
    )
    .context("droits du répertoire d'état")?;

    // L'assistant de session, s'il est livré à côté, démarre avec chaque session.
    let helper = executable.with_file_name("cotutelle-session.exe");
    if helper.exists() {
        LOCAL_MACHINE
            .create(RUN_KEY)?
            .set_string(RUN_VALUE, format!("\"{}\"", helper.display()))
            .context("démarrage automatique de l'assistant")?;
    }
    if let Some(shortcut) = shortcut_path()
        && let Err(e) = std::fs::write(&shortcut, shortcut_content(local_addr, &executable))
    {
        tracing::warn!(error = %e, "raccourci du menu Démarrer");
    }

    if state_dir.join("agent.json").exists() {
        service.start::<&str>(&[]).context("démarrage du service")?;
        println!("Service {SERVICE_NAME} enregistré et démarré.");
    } else {
        println!(
            "Service {SERVICE_NAME} enregistré. Rattachez maintenant cet ordinateur :\n  \
             cotutelle-agent enroll --server http://ADRESSE:8080 --code XXXX-XXXX\n\
             Le service démarrera à la fin de l'enrôlement."
        );
    }
    Ok(())
}

/// Démarre le service s'il est enregistré. Rend `false` s'il ne l'est pas.
pub fn start_if_installed() -> Result<bool> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let Ok(service) = manager.open_service(SERVICE_NAME, ServiceAccess::START) else {
        return Ok(false);
    };
    service.start::<&str>(&[]).context("démarrage du service")?;
    Ok(true)
}

/// Retire le service et ce que `install` a posé. Le répertoire d'état
/// (identité, politique) reste en place.
pub fn uninstall() -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .context("gestionnaire de services (lancez la console en administrateur)")?;
    let access = ServiceAccess::STOP | ServiceAccess::QUERY_STATUS | ServiceAccess::DELETE;
    match manager.open_service(SERVICE_NAME, access) {
        Ok(service) => {
            if service.query_status()?.current_state != ServiceState::Stopped {
                let _ = service.stop();
                let deadline = Instant::now() + STOP_TIMEOUT;
                while service.query_status()?.current_state != ServiceState::Stopped {
                    if Instant::now() >= deadline {
                        anyhow::bail!("le service ne s'arrête pas");
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            service.delete().context("suppression du service")?;
            println!("Service {SERVICE_NAME} retiré.");
        }
        Err(e) => tracing::info!(error = %e, "service non enregistré"),
    }
    if let Ok(key) = LOCAL_MACHINE.create(RUN_KEY) {
        let _ = key.remove_value(RUN_VALUE);
    }
    if let Some(shortcut) = shortcut_path() {
        let _ = std::fs::remove_file(shortcut);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_opens_the_local_agent() {
        let content =
            shortcut_content("127.0.0.1:7465", Path::new(r"C:\Cotutelle\cotutelle-agent.exe"));
        assert!(content.starts_with("[InternetShortcut]\r\nURL=http://127.0.0.1:7465/\r\n"));
        assert!(content.contains(r"IconFile=C:\Cotutelle\cotutelle-agent.exe"));
    }

    #[test]
    fn service_starts_itself_with_the_service_subcommand() {
        let info = service_info(PathBuf::from(r"C:\Cotutelle\cotutelle-agent.exe"));
        assert_eq!(info.launch_arguments, vec![OsString::from("service")]);
        assert_eq!(info.start_type, ServiceStartType::AutoStart);
        assert!(info.account_name.is_none(), "LocalSystem");
    }
}
