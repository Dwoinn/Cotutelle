//! Comptes locaux (`NetUserEnum`) et résolveurs DNS des cartes réseau
//! (`GetAdaptersAddresses`). La règle NRPT de l'agent ne change pas la
//! configuration des cartes : ce qu'elles annoncent reste lisible.

use anyhow::{Result, bail};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::ptr::null_mut;
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_FRIENDLY_NAME, GAA_FLAG_SKIP_MULTICAST,
    GAA_FLAG_SKIP_UNICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::NetworkManagement::NetManagement::{
    FILTER_NORMAL_ACCOUNT, MAX_PREFERRED_LENGTH, NERR_Success, NetApiBufferFree, NetUserEnum,
    UF_ACCOUNTDISABLE, USER_INFO_1,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC};
use windows::core::PCWSTR;

/// Comptes créés par Windows lui-même, jamais des comptes de session.
const BUILTIN_ACCOUNTS: &[&str] = &["defaultaccount", "defaultuser0", "wdagutilityaccount"];

/// Garde les comptes qu'une personne peut ouvrir : actifs, hors comptes
/// intégrés. Les administrateurs restent (ce sont les parents).
fn human_accounts(entries: impl Iterator<Item = (String, bool)>) -> Vec<String> {
    entries
        .filter(|(name, disabled)| {
            !disabled && !BUILTIN_ACCOUNTS.contains(&name.to_ascii_lowercase().as_str())
        })
        .map(|(name, _)| name)
        .collect()
}

pub fn local_accounts() -> Result<Vec<String>> {
    let mut buffer: *mut u8 = null_mut();
    let mut read = 0u32;
    let mut total = 0u32;
    // SAFETY : appel documenté ; le tampon est libéré par NetApiBufferFree.
    unsafe {
        let status = NetUserEnum(
            PCWSTR::null(),
            1,
            FILTER_NORMAL_ACCOUNT,
            &mut buffer,
            MAX_PREFERRED_LENGTH,
            &mut read,
            &mut total,
            None,
        );
        if status != NERR_Success && status != ERROR_MORE_DATA.0 {
            bail!("NetUserEnum : erreur {status}");
        }
        let entries = std::slice::from_raw_parts(buffer as *const USER_INFO_1, read as usize);
        let accounts = human_accounts(entries.iter().map(|u| {
            (
                u.usri1_name.to_string().unwrap_or_default(),
                (u.usri1_flags & UF_ACCOUNTDISABLE) == UF_ACCOUNTDISABLE,
            )
        }));
        NetApiBufferFree(Some(buffer.cast()));
        Ok(accounts)
    }
}

/// Lit l'adresse d'un `SOCKADDR` brut : famille sur deux octets, puis
/// l'adresse à l'endroit propre à chaque famille.
fn parse_sockaddr(raw: &[u8]) -> Option<IpAddr> {
    let family = u16::from_ne_bytes([*raw.first()?, *raw.get(1)?]);
    if family == AF_INET.0 {
        let bytes: [u8; 4] = raw.get(4..8)?.try_into().ok()?;
        Some(IpAddr::V4(Ipv4Addr::from(bytes)))
    } else if family == AF_INET6.0 {
        let bytes: [u8; 16] = raw.get(8..24)?.try_into().ok()?;
        Some(IpAddr::V6(Ipv6Addr::from(bytes)))
    } else {
        None
    }
}

/// Une adresse de résolveur utilisable telle quelle : ni boucle locale, ni
/// portée lien (qui demanderait l'interface).
/// Un résolveur joignable tel quel : ni la boucle locale (l'agent
/// lui-même), ni une adresse de lien (il faudrait la carte), ni les trois
/// adresses de site `fec0:0:0:ffff::1` à `::3` que Windows inscrit par
/// défaut sur toute carte IPv6 et que personne n'écoute.
fn usable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback(),
        IpAddr::V6(v6) => {
            !v6.is_loopback()
                && !v6.is_unicast_link_local()
                && (v6.segments()[0] & 0xffc0) != 0xfec0
        }
    }
}

pub fn adapter_dns() -> Result<Vec<SocketAddr>> {
    let flags = GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_UNICAST
        | GAA_FLAG_SKIP_FRIENDLY_NAME;
    let mut size = 16 * 1024u32;
    let mut buffer: Vec<u8> = Vec::new();
    // SAFETY : appels documentés ; le tampon est redimensionné tant que
    // l'API le demande, puis parcouru par les pointeurs qu'elle y a écrits.
    unsafe {
        loop {
            buffer.resize(size as usize, 0);
            let adapters = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
            let status =
                GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(adapters), &mut size);
            if status == ERROR_SUCCESS.0 {
                break;
            }
            if status != ERROR_BUFFER_OVERFLOW.0 {
                bail!("GetAdaptersAddresses : erreur {status}");
            }
        }
        let mut servers = Vec::new();
        let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !adapter.is_null() {
            if (*adapter).OperStatus == IfOperStatusUp {
                let mut dns = (*adapter).FirstDnsServerAddress;
                while !dns.is_null() {
                    let address = (*dns).Address;
                    let raw = std::slice::from_raw_parts(
                        address.lpSockaddr as *const u8,
                        address.iSockaddrLength.max(0) as usize,
                    );
                    if let Some(ip) = parse_sockaddr(raw).filter(|ip| usable(*ip)) {
                        let addr = SocketAddr::new(ip, 53);
                        if !servers.contains(&addr) {
                            servers.push(addr);
                        }
                    }
                    dns = (*dns).Next;
                }
            }
            adapter = (*adapter).Next;
        }
        Ok(servers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_and_disabled_accounts_are_dropped() {
        let entries = [
            ("Administrateur", true),
            ("Invité", true),
            ("DefaultAccount", true),
            ("WDAGUtilityAccount", true),
            ("papa", false),
            ("louis", false),
            ("defaultuser0", false),
        ];
        let accounts = human_accounts(entries.iter().map(|(n, d)| ((*n).to_string(), *d)));
        assert_eq!(accounts, ["papa", "louis"]);
    }

    #[test]
    fn sockaddr_families_are_read() {
        // SOCKADDR_IN : famille 2, port, adresse 192.168.3.1.
        let v4 = [2, 0, 0, 53, 192, 168, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(parse_sockaddr(&v4), Some("192.168.3.1".parse().unwrap()));
        // SOCKADDR_IN6 : famille 23, port, flowinfo, adresse, scope.
        let mut v6 = vec![23, 0, 0, 53, 0, 0, 0, 0];
        v6.extend_from_slice(&"fe80::1".parse::<Ipv6Addr>().unwrap().octets());
        v6.extend_from_slice(&[3, 0, 0, 0]);
        let ip = parse_sockaddr(&v6).unwrap();
        assert_eq!(ip, "fe80::1".parse::<IpAddr>().unwrap());
        assert!(!usable(ip), "une adresse de portée lien n'est pas utilisable seule");
        assert!(!usable("127.0.0.1".parse().unwrap()));
        assert!(!usable("fec0:0:0:ffff::1".parse().unwrap()), "adresse de site par défaut");
        assert!(usable("9.9.9.9".parse().unwrap()));
        assert!(usable("2620:fe::fe".parse().unwrap()));
        assert_eq!(parse_sockaddr(&[17, 0]), None);
    }
}
