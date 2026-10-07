# Cotutelle

Contrôle parental libre, multi-enfants et multi-appareils, piloté depuis une
interface web sur le réseau local. Réécriture inspirée de
[CTparental](https://gitlab.com/marsat/CTparental), dont il reprend l'idée
(listes UT1, horaires, quota de temps) mais pas le code.

Une cotutelle est une tutelle exercée à plusieurs : deux parents, plusieurs
enfants, plusieurs appareils.

## Principes

- **Filtrage par DNS** : portable sur tous les OS, sans certificat racine.
- **Un serveur, des agents** : le serveur décide et distribue, l'agent
  applique en local et continue à protéger hors ligne.
- **Transparent pour l'enfant** : il voit ce que ses parents voient de lui.
- **Autorisations temporaires en un geste** : « YouTube pendant 1 h »,
  « +30 min », validées depuis un téléphone.

Le détail des décisions est dans [docs/CADRAGE.md](docs/CADRAGE.md).

## Structure

```
crates/common   types du domaine, moteur DNS, protocole agent/serveur
crates/server   API, interface web, DNS réseau local, planification
crates/agent    service installé sur l'appareil protégé
web/            interface SvelteKit + Tailwind
docker/         image du serveur et compose d'exemple
```

## Démarrer (phase 0)

```bash
cargo build --workspace
cargo test --workspace
cargo run -p cotutelle-server
curl http://localhost:8080/api/v1/version
```

## État

Phase 0 : squelette. Rien n'est utilisable pour protéger un appareil.
Voir la feuille de route dans le document de cadrage.

## Licence

À décider (voir questions ouvertes du cadrage).
