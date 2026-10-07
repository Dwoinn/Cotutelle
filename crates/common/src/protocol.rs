//! Messages échangés entre l'agent et le serveur (WebSocket, repli HTTP).
//! Sérialisés en JSON. Voir §7 du document de cadrage.

use crate::{ChildId, DeviceId, Policy, TemporaryGrant};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Politique effective pour un compte de session d'un appareil.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPolicy {
    /// Nom du compte OS (ex. `louis`). `None` = politique par défaut de l'appareil.
    pub os_account: Option<String>,
    pub child: Option<ChildId>,
    pub policy: Policy,
    pub grants: Vec<TemporaryGrant>,
}

/// Envoyé par le serveur à l'agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Politique complète, remplace la copie locale.
    PolicyUpdate {
        device: DeviceId,
        issued_at: DateTime<Utc>,
        sessions: Vec<SessionPolicy>,
    },
    /// Nouvelle exception, à appliquer immédiatement (vidage du cache DNS).
    GrantAdded(TemporaryGrant),
    GrantRevoked {
        grant: crate::GrantId,
    },
    /// Demande au client de verrouiller une session maintenant.
    LockNow {
        os_account: String,
    },
    /// Les listes de blocage ont changé : l'agent les retélécharge.
    BlocklistsUpdated {
        checksum: String,
    },
    Pong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    Login,
    Logout,
    Lock,
    Unlock,
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TamperKind {
    ResolverConfigChanged,
    DohAttempt,
    ServiceStopped,
    ClockChanged,
    FirewallRulesMissing,
}

/// Envoyé par l'agent au serveur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    Hello {
        device: DeviceId,
        agent_version: String,
        protocol_version: u16,
        /// Horodatage de la politique actuellement appliquée, pour éviter
        /// un renvoi inutile.
        policy_issued_at: Option<DateTime<Utc>>,
    },
    Ping,
    Session {
        os_account: String,
        kind: SessionEventKind,
        at: DateTime<Utc>,
    },
    /// Agrégat de requêtes DNS pour une fenêtre de temps (niveau 2).
    DnsSummary {
        os_account: Option<String>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        /// Domaine → (requêtes autorisées, requêtes bloquées).
        domains: Vec<(String, u32, u32)>,
    },
    Tamper {
        kind: TamperKind,
        at: DateTime<Utc>,
        details: String,
    },
    /// Demande émise par l'enfant depuis la page de blocage ou le helper.
    AccessRequest {
        os_account: String,
        target: crate::GrantTarget,
        message: Option<String>,
    },
}
