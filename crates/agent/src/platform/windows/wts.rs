//! Sessions Windows par l'API Terminal Services (WTS) : énumération, état
//! verrouillé, verrou, déconnexion, message dans la session, jeton de
//! l'utilisateur pour lancer un programme chez lui.
//!
//! L'identifiant de session WTS tient lieu d'`uid` dans [`Session`] : il
//! n'existe pas d'identifiant numérique de compte sous Windows.

use super::{from_wide, system_root, wide};
use crate::platform::Session;
use anyhow::{Context, Result};
use std::ptr::null_mut;
use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, SID_NAME_USE, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows::Win32::System::RemoteDesktop::{
    WTS_SESSION_INFOW, WTS_SESSIONSTATE_LOCK, WTSActive, WTSDisconnectSession, WTSDisconnected,
    WTSEnumerateSessionsW, WTSFreeMemory, WTSINFOEX_LEVEL1_W, WTSINFOEXW,
    WTSQuerySessionInformationW, WTSQueryUserToken, WTSSendMessageW, WTSSessionInfoEx,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, OpenProcess,
    OpenProcessToken, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MESSAGEBOX_RESULT,
};
use windows::core::{PCWSTR, PWSTR};

/// Détail d'une session, niveau 1 de `WTSSessionInfoEx`.
fn info_ex(session_id: u32) -> Option<WTSINFOEX_LEVEL1_W> {
    let mut buffer = PWSTR::null();
    let mut bytes = 0u32;
    // SAFETY : appel documenté ; le tampon rendu est libéré par WTSFreeMemory.
    unsafe {
        WTSQuerySessionInformationW(None, session_id, WTSSessionInfoEx, &mut buffer, &mut bytes)
            .ok()?;
        let info = buffer.as_ptr() as *const WTSINFOEXW;
        let level1 = (bytes as usize >= std::mem::size_of::<WTSINFOEXW>() && (*info).Level == 1)
            .then(|| (*info).Data.WTSInfoExLevel1);
        WTSFreeMemory(buffer.as_ptr().cast());
        level1
    }
}

/// Une session WTS telle que l'agent la considère, ou `None` si elle ne
/// compte pas : écran de connexion, services. Une session de bureau à
/// distance compte comme une session locale : l'enfant pourrait s'y
/// connecter depuis une tablette, et c'est ainsi qu'on essaie l'agent.
fn session_from_info(id: u32, state_known: bool, info: &WTSINFOEX_LEVEL1_W) -> Option<Session> {
    let user = from_wide(&info.UserName);
    if user.is_empty() {
        return None;
    }
    let active = state_known && info.SessionState == WTSActive;
    // Une session déconnectée (changement d'utilisateur) continue en
    // arrière-plan derrière l'écran de connexion : personne n'est devant.
    let locked =
        info.SessionFlags as u32 == WTS_SESSIONSTATE_LOCK || info.SessionState == WTSDisconnected;
    Some(Session { id: id.to_string(), user, uid: id, active, locked, idle: false })
}

pub fn sessions() -> Result<Vec<Session>> {
    let mut list: *mut WTS_SESSION_INFOW = null_mut();
    let mut count = 0u32;
    // SAFETY : appel documenté ; la liste est libérée par WTSFreeMemory.
    let ids: Vec<u32> = unsafe {
        WTSEnumerateSessionsW(None, 0, 1, &mut list, &mut count).context("énumération WTS")?;
        let entries = std::slice::from_raw_parts(list, count as usize);
        let ids = entries
            .iter()
            .filter(|s| s.State == WTSActive || s.State == WTSDisconnected)
            .map(|s| s.SessionId)
            .collect();
        WTSFreeMemory(list.cast());
        ids
    };
    Ok(ids
        .into_iter()
        .filter_map(|id| info_ex(id).and_then(|info| session_from_info(id, true, &info)))
        .collect())
}

pub fn is_locked(session_id: u32) -> bool {
    info_ex(session_id).is_some_and(|info| {
        info.SessionFlags as u32 == WTS_SESSIONSTATE_LOCK || info.SessionState == WTSDisconnected
    })
}

/// Jeton principal de l'utilisateur de la session, pour lancer un programme
/// dans sa session. Réservé au compte SYSTEM.
fn user_token(session_id: u32) -> Result<HANDLE> {
    let mut token = HANDLE::default();
    // SAFETY : appel documenté ; le jeton est fermé par l'appelant.
    unsafe { WTSQueryUserToken(session_id, &mut token) }.context("jeton de l'utilisateur")?;
    Ok(token)
}

/// Lance un programme dans la session, sur son bureau interactif, sans
/// attendre sa fin.
fn run_in_session(session_id: u32, program: &str, arguments: &str) -> Result<()> {
    let token = user_token(session_id)?;
    let application = wide(&system_root().join("System32").join(program).to_string_lossy());
    let mut command_line = wide(&format!("{program} {arguments}"));
    let mut desktop = wide(r"winsta0\default");
    let mut environment = null_mut();
    let mut startup =
        STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    startup.lpDesktop = PWSTR(desktop.as_mut_ptr());
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY : appels documentés ; les tampons vivent jusqu'à la fin de
    // l'appel, les descripteurs sont tous fermés.
    let launched = unsafe {
        let env =
            CreateEnvironmentBlock(&mut environment, Some(token), false).map(|()| environment).ok();
        let result = CreateProcessAsUserW(
            Some(token),
            PCWSTR(application.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            env.map(|e| e as *const _),
            PCWSTR::null(),
            &startup,
            &mut process,
        );
        if let Some(env) = env {
            let _ = DestroyEnvironmentBlock(env);
        }
        let _ = CloseHandle(token);
        if result.is_ok() {
            let _ = CloseHandle(process.hProcess);
            let _ = CloseHandle(process.hThread);
        }
        result
    };
    launched.with_context(|| format!("lancement de {program} dans la session"))
}

/// Verrouille la session par `LockWorkStation`, qui doit être appelé depuis
/// la session elle-même.
pub fn lock_workstation(session_id: u32) -> Result<()> {
    run_in_session(session_id, "rundll32.exe", "user32.dll,LockWorkStation")
}

/// Déconnecte la session : l'écran de connexion revient, les programmes
/// continuent en arrière-plan.
pub fn disconnect(session_id: u32) -> Result<()> {
    // SAFETY : appel documenté.
    unsafe { WTSDisconnectSession(None, session_id, true) }.context("WTSDisconnectSession")
}

/// Affiche une boîte de message dans la session, sans attendre de réponse.
pub fn send_message(session_id: u32, title: &str, body: &str) -> Result<()> {
    let title: Vec<u16> = title.encode_utf16().collect();
    let body: Vec<u16> = body.encode_utf16().collect();
    let mut response = MESSAGEBOX_RESULT::default();
    // SAFETY : appel documenté ; les longueurs sont en octets, sans nul final.
    unsafe {
        WTSSendMessageW(
            None,
            session_id,
            PCWSTR(title.as_ptr()),
            (title.len() * 2) as u32,
            PCWSTR(body.as_ptr()),
            (body.len() * 2) as u32,
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST,
            0,
            &mut response,
            false,
        )
    }
    .context("message dans la session")
}

/// Nom du compte qui a ouvert ce processus.
fn process_account(pid: u32) -> Result<String> {
    // SAFETY : appels documentés ; descripteurs fermés, tampons dimensionnés
    // par l'API elle-même avant d'être lus.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .context("processus du pair")?;
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        let _ = CloseHandle(process);
        opened.context("jeton du pair")?;

        let mut needed = 0u32;
        let sized = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        if sized.is_ok() || needed == 0 {
            let _ = CloseHandle(token);
            anyhow::bail!("taille du jeton inconnue");
        }
        if let Err(e) = sized
            && e.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult()
        {
            let _ = CloseHandle(token);
            return Err(e).context("lecture du jeton");
        }
        let mut buffer = vec![0u8; needed as usize];
        let read = GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        );
        let _ = CloseHandle(token);
        read.context("utilisateur du jeton")?;
        let sid = (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid;

        let mut name = [0u16; 256];
        let mut name_len = name.len() as u32;
        let mut domain = [0u16; 256];
        let mut domain_len = domain.len() as u32;
        let mut kind = SID_NAME_USE::default();
        LookupAccountSidW(
            PCWSTR::null(),
            sid,
            Some(PWSTR(name.as_mut_ptr())),
            &mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_len,
            &mut kind,
        )
        .context("nom du compte")?;
        Ok(from_wide(&name))
    }
}

/// Compte du client connecté à ce tube nommé.
pub fn pipe_client_account(pipe: HANDLE) -> Result<String> {
    let mut pid = 0u32;
    // SAFETY : appel documenté.
    unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) }.context("processus du client")?;
    process_account(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::RemoteDesktop::{WTS_SESSIONSTATE_UNLOCK, WTSConnected};

    fn info(user: &str, station: &str, state: i32, flags: u32) -> WTSINFOEX_LEVEL1_W {
        let mut info = WTSINFOEX_LEVEL1_W {
            SessionState: windows::Win32::System::RemoteDesktop::WTS_CONNECTSTATE_CLASS(state),
            SessionFlags: flags as i32,
            ..Default::default()
        };
        let user: Vec<u16> = user.encode_utf16().collect();
        info.UserName[..user.len()].copy_from_slice(&user);
        let station: Vec<u16> = station.encode_utf16().collect();
        info.WinStationName[..station.len()].copy_from_slice(&station);
        info
    }

    #[test]
    fn console_session_in_use() {
        let session = session_from_info(
            2,
            true,
            &info("louis", "Console", WTSActive.0, WTS_SESSIONSTATE_UNLOCK),
        )
        .unwrap();
        assert_eq!((session.user.as_str(), session.uid, session.in_use()), ("louis", 2, true));
    }

    #[test]
    fn locked_and_disconnected_sessions_are_not_in_use() {
        let locked = session_from_info(
            2,
            true,
            &info("louis", "Console", WTSActive.0, WTS_SESSIONSTATE_LOCK),
        )
        .unwrap();
        assert!(locked.active && locked.locked && !locked.in_use());
        let away = session_from_info(
            3,
            true,
            &info("papa", "", WTSDisconnected.0, WTS_SESSIONSTATE_UNLOCK),
        )
        .unwrap();
        assert!(!away.active && away.locked);
    }

    #[test]
    fn logon_screen_is_ignored_but_remote_desktop_counts() {
        let logon = info("", "Console", WTSConnected.0, WTS_SESSIONSTATE_UNLOCK);
        let rdp = info("louis", "RDP-Tcp#0", WTSActive.0, WTS_SESSIONSTATE_UNLOCK);
        assert!(session_from_info(1, true, &logon).is_none());
        let rdp = session_from_info(4, true, &rdp).unwrap();
        assert!(rdp.active && !rdp.locked && rdp.in_use());
    }
}
