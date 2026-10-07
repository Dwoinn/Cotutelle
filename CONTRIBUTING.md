# Contribuer à Cotutelle

Merci de votre intérêt. Avant une contribution importante, ouvrez une
discussion : les décisions d'architecture sont consignées dans
[docs/CADRAGE.md](docs/CADRAGE.md) et ne se rouvrent pas sans échange.

## Licence de vos contributions

Cotutelle est sous licence AGPL-3.0-or-later, avec les conditions
supplémentaires de [NOTICE.md](NOTICE.md). En contribuant, vous acceptez que
votre contribution soit publiée sous ces mêmes termes. Vous en restez
l'auteur : aucune cession de droits n'est demandée.

## Attestation d'origine

Chaque commit doit porter une ligne `Signed-off-by`, ajoutée par
`git commit -s`. Elle atteste, selon le
[Developer Certificate of Origin](https://developercertificate.org/), que
vous avez le droit de soumettre ce code sous la licence du projet.

Si une partie de votre contribution a été produite avec un assistant IA,
dites-le dans la demande de fusion.

## Avant d'ouvrir une demande de fusion

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd web && pnpm check && pnpm build)
```

Les conventions du dépôt sont dans [CLAUDE.md](CLAUDE.md) : identifiants en
anglais, commentaires, documentation et interface en français.

## Tester l'agent

Lancez toujours l'agent avec `run --dry-run` sur votre poste de travail. Sans
ce drapeau, il verrouille les sessions et modifie la configuration DNS et
nftables de la machine.
