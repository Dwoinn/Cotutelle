//! Messages échangés entre l'agent et le serveur (WebSocket, JSON).
//! Voir §7 du document de cadrage.
//!
//! Le serveur envoie toujours un état complet ([`DeviceState`]) : l'agent
//! n'a jamais à fusionner des deltas, et sa copie sur disque est directement
//! la dernière politique connue (D3).

use crate::{BlockReason, ChildId, DeviceId, Policy, TemporaryGrant, Usage};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Politique d'un compte de session géré sur l'appareil.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    /// Nom du compte OS (ex. `louis`).
    pub os_account: String,
    pub child: ChildId,
    pub child_name: String,
    pub policy: Policy,
    pub grants: Vec<TemporaryGrant>,
    /// Temps consommé par cet enfant sur ses *autres* appareils.
    pub usage_elsewhere: Usage,
}

/// Référence d'une liste de blocage que l'agent doit détenir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlocklistRef {
    pub category: String,
    /// Somme SHA-256 hexadécimale du fichier binaire.
    pub checksum: String,
    pub entries: u64,
}

/// État complet destiné à un appareil.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    pub device: DeviceId,
    pub issued_at: DateTime<Utc>,
    /// Comptes gérés. Un compte absent de cette liste n'est pas filtré.
    pub accounts: Vec<AccountState>,
    pub blocklists: Vec<BlocklistRef>,
    /// Résolveurs amont, sous la forme `ip:port`.
    pub upstream_dns: Vec<String>,
}

/// Envoyé par le serveur à l'agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    State(Box<DeviceState>),
    /// Verrouille immédiatement la session de ce compte.
    LockNow {
        os_account: String,
    },
    Pong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TamperKind {
    ResolverConfigChanged,
    FirewallRulesMissing,
    ClockChanged,
    AgentRestarted,
    /// La session n'a pas pu être verrouillée alors que l'accès est fermé.
    LockFailed,
}

/// Compteur de requêtes DNS pour un domaine sur une fenêtre de temps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainCount {
    pub domain: String,
    pub allowed: u32,
    pub blocked: u32,
    /// Motif du dernier blocage observé.
    pub reason: Option<BlockReason>,
}

/// Envoyé par l'agent au serveur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    Hello {
        agent_version: String,
        protocol_version: u16,
        hostname: String,
        /// Comptes de session présents sur l'appareil.
        os_accounts: Vec<String>,
        /// Horodatage de l'état actuellement appliqué.
        state_issued_at: Option<DateTime<Utc>>,
    },
    Ping,
    /// Temps d'écran cumulé d'un compte pour une journée locale. La valeur
    /// est absolue : la renvoyer deux fois ne compte pas double.
    Usage {
        os_account: String,
        day: NaiveDate,
        seconds: u32,
        /// Compte actuellement au premier plan et déverrouillé.
        active: bool,
    },
    /// Agrégat de requêtes DNS (niveau 2 du suivi).
    DnsSummary {
        os_account: Option<String>,
        day: NaiveDate,
        domains: Vec<DomainCount>,
    },
    Tamper {
        kind: TamperKind,
        at: DateTime<Utc>,
        details: String,
    },
}

/// Corps de `POST /api/v1/agent/enroll`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub token: String,
    pub hostname: String,
    pub os: String,
    pub agent_version: String,
    pub os_accounts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollResponse {
    pub device: DeviceId,
    /// Jeton permanent de l'appareil, à conserver en lecture root seule.
    pub device_token: String,
}

/// Corps de `POST /api/v1/agent/child-link` : lien à usage unique ouvrant
/// l'espace enfant depuis l'appareil.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildLinkRequest {
    pub os_account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildLinkResponse {
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_tagged_by_type() {
        let json = serde_json::to_string(&AgentMessage::Ping).unwrap();
        assert_eq!(json, r#"{"type":"ping"}"#);
        let msg: ServerMessage =
            serde_json::from_str(r#"{"type":"lock_now","os_account":"louis"}"#).unwrap();
        assert_eq!(msg, ServerMessage::LockNow { os_account: "louis".into() });
    }
}
