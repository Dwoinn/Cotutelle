//! Identifiants typés. Évite de confondre un enfant et un appareil dans une
//! signature de fonction.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! typed_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

typed_id!(FamilyId, "Une famille : le périmètre d'une installation.");
typed_id!(ParentId, "Un compte parent (administrateur).");
typed_id!(ChildId, "Un enfant, porteur d'une politique.");
typed_id!(DeviceId, "Un appareil, avec ou sans agent.");
typed_id!(PolicyId, "Une politique : filtres, horaires, quota.");
typed_id!(GrantId, "Une exception temporaire accordée par un parent.");
typed_id!(RequestId, "Une demande émise depuis l'appareil d'un enfant.");
