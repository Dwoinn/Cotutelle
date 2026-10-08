//! Types partagés entre le serveur et l'agent Cotutelle.
//!
//! Ce crate ne contient aucune E/S : uniquement le modèle du domaine
//! (§6 du document de cadrage), le moteur de filtrage, l'évaluation des
//! horaires et quotas, et le protocole agent ↔ serveur.

pub mod access;
pub mod catalog;
pub mod filter;
pub mod ids;
pub mod policy;
pub mod protocol;
pub mod schedule;
pub mod stats;

pub use access::*;
pub use filter::*;
pub use ids::*;
pub use policy::*;
pub use schedule::*;

/// Version du protocole agent ↔ serveur. Incrémentée à chaque changement
/// incompatible des messages de [`protocol`].
///
/// 2 : l'état porte le catalogue des services. Un agent resté en version 1
/// l'ignore et n'applique que les services fournis avec lui.
pub const PROTOCOL_VERSION: u16 = 2;
