# Cotutelle — conventions du dépôt

Contrôle parental client/serveur en Rust. Lire `docs/CADRAGE.md` avant toute
évolution d'architecture : les décisions D1 à D11 y sont justifiées et ne se
rouvrent pas sans discussion.

## Règles

- Langue : code et identifiants en anglais, commentaires, docs et interface
  en français.
- Workspace Cargo, édition 2024. Les dépendances sont déclarées dans
  `[workspace.dependencies]` et référencées avec `.workspace = true`.
- `cotutelle-common` ne fait aucune E/S. Tout ce qui parle au réseau, au
  disque ou à l'OS vit dans `server` ou `agent`.
- Dans l'agent, tout ce qui dépend de l'OS passe par le trait
  `platform::Platform`. Pas de `#[cfg(target_os)]` ailleurs.
- Sécurité par défaut : l'agent n'ouvre jamais l'accès en l'absence de
  politique ou de serveur (D3).
- Avant de proposer un commit : `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.
