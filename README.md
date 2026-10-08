<h1 align="center">
  <img src="web/static/icon.svg" width="112" alt="Logo de Cotutelle : deux moitiés de bouclier autour d’un cadran"><br>
  Cotutelle
</h1>

Contrôle parental libre, multi-enfants et multi-appareils, piloté depuis une
interface web sur le réseau local. Réécriture inspirée de
[CTparental](https://gitlab.com/marsat/CTparental), dont il reprend l'idée
(listes de blocage, horaires, temps d'écran) mais pas le code.

Une cotutelle est une tutelle exercée à plusieurs : deux parents, plusieurs
enfants, plusieurs appareils.

> **État : MVP en cours de validation.** Le serveur, l'interface et la logique
> de l'agent sont testés. La protection système de l'agent Linux (nftables,
> systemd-resolved, verrouillage) n'a pas encore été essayée sur une vraie
> machine : faites le premier essai sur une machine de test.

## Ce que ça fait

- **Filtrage par DNS** : catégories de sites (listes de l'Université Toulouse
  Capitole, 5,7 millions de domaines), services nommés comme YouTube ou
  TikTok, listes personnelles, mode restreint YouTube. Un service réunit
  tous les domaines dont une appli a besoin ; la liste se complète et
  s'étend depuis l'interface.
- **Horaires et temps d'écran** par enfant, partagé entre tous ses appareils.
  La session se verrouille après un préavis.
- **Exceptions en un geste** : « YouTube pendant 1 h », « +30 min », « pause ».
  Elles prennent effet en moins d'une minute et expirent seules.
- **Demandes de l'enfant** depuis son espace, validées par un parent.
- **Suivi transparent** : temps d'écran et sites consultés, sans les pages ni
  les recherches. L'enfant voit exactement ce que ses parents voient.
- **Appareils sans agent** (télévision, console) filtrés par le DNS du serveur.
- **Hors ligne** : sans serveur, l'agent applique la dernière politique connue.

## Comment ça marche

```
Parents (web) ──► cotutelle-server ──► DNS du réseau local ──► TV, console
                        │  ▲
     politique, listes  │  │  temps d'écran, sites, alertes
                        ▼  │
                  cotutelle-agent  (sur chaque ordinateur protégé)
                  DNS local · temps d'écran · verrouillage
```

Le serveur décide et distribue, l'agent applique en local. Les décisions
d'architecture et leurs raisons sont dans [docs/CADRAGE.md](docs/CADRAGE.md).

## Installer

Voir [docs/INSTALLATION.md](docs/INSTALLATION.md). En résumé :

```bash
docker compose -f docker/compose.yaml up -d
```

puis ouvrez `http://<adresse du serveur>:8080`, créez le compte parent,
ajoutez un enfant, et installez l'agent sur son ordinateur avec le code
affiché dans « Appareils ».

## Développer

Prérequis : Rust (la version est épinglée par `rust-toolchain.toml`),
Node 22 et pnpm.

```bash
cargo test --workspace                  # tests Rust
(cd web && pnpm install && pnpm build)  # interface web
cargo run -p cotutelle-server -- --http-addr 127.0.0.1:8087 --data-dir data/dev
scripts/dev-seed.sh                     # famille d'exemple dans l'instance ci-dessus
```

Pour travailler sur l'interface avec rechargement à chaud :

```bash
cd web && COTUTELLE_API=http://127.0.0.1:8087 pnpm dev
```

Pour essayer l'agent sans toucher au système ni verrouiller votre session :

```bash
cargo run -p cotutelle-agent -- --state-dir /tmp/agent enroll --server http://127.0.0.1:8087 --code XXXX-XXXX
cargo run -p cotutelle-agent -- --state-dir /tmp/agent run --dry-run --dns-addr 127.0.0.1:15354
dig -p 15354 @127.0.0.1 www.tiktok.com
```

### Structure

```
crates/common   modèle du domaine, moteur de filtrage, horaires, protocole (aucune E/S)
crates/dns      relais DNS filtrant, partagé par le serveur et l'agent
crates/server   API, hub des agents, DNS du réseau local, listes de blocage
crates/agent    service installé sur l'appareil protégé
web/            interface SvelteKit + Tailwind (parents et enfant)
packaging/      unité systemd, lanceur et scripts du paquet Debian
docker/         image du serveur et compose d'exemple
deploy/         manifestes OpenShift
scripts/        seed de développement, construction du .deb et du .rpm
```

## Crédits

- [CTparental](https://gitlab.com/marsat/CTparental), pour l'idée.
- Les listes de blocage viennent de l'[Université Toulouse
  Capitole](https://dsi.ut-capitole.fr/blacklists/), sous licence
  CC BY-SA 4.0. Elles sont téléchargées par le serveur, pas redistribuées
  dans ce dépôt.

## Licence

Cotutelle est un logiciel libre sous licence
[GNU AGPL version 3 ou ultérieure](LICENSE). Vous pouvez l'utiliser, le
modifier et le redistribuer, y compris en service hébergé, à deux conditions :

- publier le code source de votre version sous la même licence ;
- conserver l'attribution « Basé sur Cotutelle » dans l'interface.

Le détail est dans [NOTICE.md](NOTICE.md). Le nom et le logo Cotutelle sont
réservés au projet officiel : voir [MARQUE.md](MARQUE.md). Pour contribuer,
lisez [CONTRIBUTING.md](CONTRIBUTING.md).
