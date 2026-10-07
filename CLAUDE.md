# Cotutelle — conventions du dépôt

Contrôle parental client/serveur en Rust. Lire `docs/CADRAGE.md` avant toute
évolution d'architecture : les décisions D1 à D11 y sont justifiées et ne se
rouvrent pas sans discussion.

## Repères

- `crates/common` : modèle, filtrage, horaires, protocole. `crates/dns` : relais
  DNS. `crates/server` : API et DNS LAN. `crates/agent` : service sur l'appareil.
  `web/` : interface SvelteKit.
- Instance de développement : `cargo run -p cotutelle-server -- --http-addr
  127.0.0.1:8087 --data-dir data/dev`, puis `scripts/dev-seed.sh`.
- L'agent se teste avec `run --dry-run` : il ne verrouille rien et ne touche
  ni à nftables ni à systemd-resolved. Ne jamais le lancer sans ce drapeau sur
  un poste de travail.

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
- Toute route de l'API réservée aux parents prend un extracteur `Parent` ;
  après une modification d'enfant, d'appareil, de politique ou d'exception,
  appeler `state.changed()` pour propager aux agents et au DNS LAN.
- Les types de `web/src/lib/types.ts` suivent ceux de `cotutelle-common`.
- Avant de proposer un commit : `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, et dans `web/` : `pnpm check` et `pnpm build`.
