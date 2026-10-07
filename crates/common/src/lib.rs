//! Types partagés entre le serveur et l'agent Cotutelle.
//!
//! Ce crate ne contient aucune E/S : uniquement le modèle du domaine
//! (§6 du document de cadrage), l'évaluation des politiques et le protocole
//! agent ↔ serveur. Le moteur de filtrage DNS viendra ici en phase 1.

pub mod ids;
pub mod policy;
pub mod protocol;
pub mod schedule;

pub use ids::*;
pub use policy::*;
pub use schedule::*;

/// Version du protocole agent ↔ serveur. Incrémentée à chaque changement
/// incompatible des messages de [`protocol`].
pub const PROTOCOL_VERSION: u16 = 1;
