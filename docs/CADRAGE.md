# Cotutelle — Document de cadrage

Version 0.1 — 7 octobre 2026 — statut : validé pour démarrage.

Cotutelle est un contrôle parental libre, multi-enfants et multi-appareils, piloté
depuis une interface web accessible sur le réseau local. Il s'inspire de
CTparental (filtrage par listes, horaires, quota de temps) et le réécrit
intégralement avec une architecture client/serveur et des technologies actuelles.

---

## 1. Contexte et origine

CTparental fonctionne bien pour ce qu'il fait : listes de blocage de
l'Université de Toulouse (UT1), horaires et temps de navigation, le tout en
local sur un poste Linux. Ses limites motivent ce projet :

- interface d'administration datée et peu intuitive ;
- administration uniquement en local sur la machine protégée ;
- autorisations temporaires pénibles (cas type : autoriser YouTube une heure,
  à la demande et sous surveillance) ;
- un seul poste, un seul enfant, Linux uniquement.

Son architecture (bash, PHP sur lighttpd, dnsmasq, privoxy, iptables, PAM) ne
survit pas à un modèle client/serveur multi-OS. **Décision : réécriture
complète sous un nouveau nom.** Cotutelle cite CTparental comme inspiration et
réutilise ses listes UT1, pas son code.

Le nom conserve les initiales « CT ». Une cotutelle est une tutelle exercée à
plusieurs : deux parents, plusieurs enfants, plusieurs appareils.

## 2. Objectifs

1. Un **serveur** d'administration, hébergeable sur la machine protégée ou sur
   une autre machine (conteneur Docker), accessible en web depuis le réseau
   local.
2. Un **agent** à installer sur chaque appareil à protéger, Linux d'abord,
   conçu pour être porté sur Windows et macOS.
3. Gestion de **plusieurs enfants**, chacun avec sa configuration (filtres,
   horaires, quotas), chacun pouvant avoir **plusieurs appareils**.
4. **Autorisations temporaires** simples : accorder un accès ou du temps
   supplémentaire, pour une durée donnée, depuis un téléphone.
5. **Suivi** du temps d'écran et de l'activité réseau agrégée, transparent
   pour l'enfant.
6. Une interface **simple et agréable**, pour les parents comme pour les
   enfants.

## 3. Hors périmètre (v1)

- Inspection du contenu HTTPS (certificat racine, proxy MITM).
- Agents Android et iOS (phase 2 : profil DNS privé sans gestion du temps).
- Captures d'écran, enregistrement des frappes.
- Exposition directe sur Internet : on documente Tailscale ou un tunnel,
  on ne l'intègre pas.
- Serveur DHCP intégré (à reconsidérer si la box ne permet pas de distribuer
  un DNS personnalisé).

## 4. Contexte familial de référence

| Qui / quoi            | Détail                                                    |
|-----------------------|-----------------------------------------------------------|
| Parents               | deux comptes administrateurs, mêmes droits                |
| Aîné, 11 ans          | laptop personnel sous Ubuntu, avec agent                  |
| Cadet, 8 ans          | utilise l'ordinateur familial (agent, compte de session)  |
| TV connectée          | partagée, sans agent, filtrée par le DNS du serveur       |
| Serveur               | conteneur Docker sur une machine du réseau local          |

## 5. Décisions d'architecture

| # | Décision | Justification |
|---|----------|---------------|
| D1 | **Filtrage par DNS uniquement.** Blocage par domaine, jamais par page ou mot-clé. | Seul mécanisme portable sur tous les OS sans certificat racine. Tout est en HTTPS. |
| D2 | **L'agent embarque son propre résolveur DNS et ses listes.** Le serveur distribue la politique, l'agent l'applique. | Permet le mode hors ligne et protège le laptop hors de la maison. |
| D3 | **Hors ligne : dernière politique connue.** Jamais d'ouverture par défaut. | Un serveur éteint ne doit pas désactiver la protection. |
| D4 | **Temps écoulé : verrouillage de session** (logind), préavis à 5 min et 1 min. | Ne détruit aucun travail en cours, réversible par un parent. |
| D5 | **Le serveur est aussi résolveur DNS du réseau local.** Appareils sans agent filtrés par adresse IP/MAC via un « profil appareil ». | Couvre la TV connectée, les consoles, les invités. Par défaut : politique la plus stricte de la maison. |
| D6 | **Suivi niveaux 1 et 2 par défaut**, rétention 30 jours. Niveau 3 en mode diagnostic, rétention 48 h. Niveau 4 en phase 2, désactivable par enfant. Niveau 5 exclu. | Proportionné à 8 et 11 ans. Voir §8. |
| D7 | **Transparence : l'enfant voit sur son tableau de bord ce que les parents voient de lui.** | Rend l'outil acceptable, évite qu'il devienne un défi. |
| D8 | **Alertes de contournement toujours actives.** | Surveillance de l'outil, pas de l'enfant. Indispensable. |
| D9 | **Rust** pour serveur et agent, **SvelteKit + Tailwind** pour le web. | Binaires uniques, code DNS partagé, compilation croisée native. Front en TypeScript pour la vitesse d'itération sur l'interface. |
| D10 | **SQLite par défaut**, Postgres en option. | Zéro administration pour une famille. |
| D11 | **Demande d'accès depuis l'appareil de l'enfant**, validation par le parent avec notification. | Transforme la friction du cas YouTube en fonctionnalité. |

## 6. Modèle de domaine

```
Famille
 ├─ Parent (compte admin, 2 minimum)
 ├─ Enfant
 │   ├─ Politique (filtres, horaires, quota)
 │   └─ Appareil* ──┐
 ├─ Appareil         │  un appareil peut être partagé :
 │   ├─ Compte de session (utilisateur OS) ──→ Enfant
 │   └─ Agent (jeton, version, dernier contact)
 ├─ Profil appareil sans agent (IP/MAC → Politique)
 ├─ Exception temporaire (cible, durée, accordée par, pour qui)
 ├─ Demande (enfant → parents : accès à X / N minutes de plus)
 ├─ Session d'écran (appareil, compte, début, fin, inactivité)
 ├─ Journal DNS (niveau 3, rétention courte)
 ├─ Agrégats quotidiens (temps, catégories, top domaines, blocages)
 └─ Alerte (contournement, agent silencieux, quota atteint)
```

**La politique est rattachée au couple appareil + compte de session**, pas à
l'appareil seul. L'ordinateur familial a un compte par enfant et un compte
parents non filtré.

### Politique

- **Filtres** : catégories UT1 bloquées, listes additionnelles optionnelles
  (HaGeZi, OISD), liste blanche et liste noire personnelles, mode restreint
  YouTube (par CNAME DNS), blocage du DNS-over-HTTPS.
- **Horaires** : plages autorisées par jour de la semaine, distinction
  école / week-end / vacances.
- **Quota** : minutes par jour, optionnellement par semaine, avec bonus
  ponctuel accordé par un parent.
- **Exceptions temporaires** : « YouTube pendant 1 h », « +30 min
  aujourd'hui ». Durée obligatoire, expiration automatique.

## 7. Architecture technique

```
                 ┌───────────────────────────────────────────┐
  Parents  ───►  │  cotutelle-server (Rust, axum, Docker)    │
  (web, mobile)  │  • API REST + WebSocket agents            │
                 │  • UI web statique (SvelteKit)            │
  TV, console ─► │  • DNS LAN :53 (hickory) → profils IP     │
  (DNS direct)   │  • planificateur, agrégation, alertes     │
                 │  • SQLite / Postgres                      │
                 └──────────────┬────────────────────────────┘
                                │ politique, listes, grants  ▲ événements, sessions
                                ▼                            │
                 ┌───────────────────────────────────────────┐
                 │  cotutelle-agent (Rust, service root)     │
  Laptop aîné    │  • DNS local 127.0.0.1:53 (hickory)       │
  PC familial    │  • cache politique + listes (hors ligne)  │
                 │  • nftables : force DNS, bloque DoH/VPN   │
                 │  • logind : sessions, verrouillage        │
                 │  • helper session : préavis, temps restant│
                 └───────────────────────────────────────────┘
```

### Crates

| Crate | Rôle |
|-------|------|
| `cotutelle-common` | Types du domaine, moteur de filtrage DNS, formats de listes, protocole agent/serveur. |
| `cotutelle-server` | API, UI, DNS LAN, planification, agrégation, notifications. |
| `cotutelle-agent` | Résolveur local, application de la politique, intégration OS derrière un trait `Platform`. |
| `web/` | SvelteKit + Tailwind, espaces parents et enfant, page de blocage. |

### Protocole agent ↔ serveur

- Enrôlement par jeton à usage unique généré dans l'interface.
- Connexion WebSocket persistante : le serveur pousse politique et exceptions,
  l'agent envoie battement de cœur, sessions et événements par lots.
- Repli en long-polling HTTP si le WebSocket est bloqué.
- L'agent conserve politique et listes sur disque, signées par le serveur.

### Anti-contournement (agent Linux)

- Compte enfant sans sudo, agent en service systemd root durci.
- nftables : seul l'agent peut sortir en 53/853 ; redirection de tout DNS
  sortant vers le résolveur local.
- Blocage du canari `use-application-dns.net` et des fournisseurs DoH connus ;
  politique d'entreprise Firefox/Chrome désactivant le DNS sécurisé.
- Détection : changement de `resolv.conf`/NetworkManager, arrêt du service,
  horloge modifiée, agent silencieux → alerte au serveur.
- Hors périmètre logiciel mais à documenter : mot de passe BIOS, boot sur
  disque uniquement, chiffrement du disque.

### Listes

- UT1 (Université de Toulouse) par catégorie, mise à jour quotidienne par le
  serveur, diffusion aux agents sous forme compacte avec somme de contrôle.
- Listes additionnelles optionnelles. Allow/deny personnels prioritaires.

### Cas YouTube

- Blocage par domaines (`youtube.com`, `googlevideo.com`, `ytimg.com`, apps).
- Exception temporaire : TTL DNS court (≤ 30 s), vidage du cache local par
  l'agent à l'activation et à l'expiration.
- Mode restreint imposable par CNAME `restrict.youtube.com`.

## 8. Suivi : niveaux et choix

| Niveau | Contenu | Statut |
|--------|---------|--------|
| 1 | Temps d'écran : sessions, quota, horaires réels | par défaut |
| 2 | Activité réseau agrégée par jour : catégories, top domaines, blocages | par défaut |
| 3 | Journal DNS brut horodaté | mode diagnostic, 48 h |
| 4 | Applications utilisées (processus, fenêtre active) | phase 2, par enfant |
| 5 | Captures d'écran, frappes | exclu |

Rappel : le filtrage DNS ne voit que des noms de domaine. Pas de recherches,
pas de titres de vidéos, pas de pages.

## 9. Sécurité

- Comptes parents : mot de passe (argon2), TOTP optionnel, sessions par
  cookie sécurisé, protection CSRF.
- TLS : via reverse proxy (Caddy, Traefik) ou certificat local ; HTTP
  autorisé uniquement sur le réseau local.
- Agents : jeton par appareil, révocable ; enrôlement à usage unique.
- Données : rétention paramétrable, export et purge par enfant.
- Image Docker non root, multi-arch (amd64, arm64 pour Raspberry Pi).

## 10. Interface

**Espace parents** : tableau de bord (état des appareils, temps du jour par
enfant, demandes en attente, alertes), fiches enfants, appareils, politiques
par assistant, exceptions rapides (« +30 min », « YouTube 1 h »), journal.

**Espace enfant** : temps restant en grand, prochain créneau, bouton « demander
», ce que mes parents voient, accessible depuis l'appareil sans mot de passe.

**Page de blocage** : explication en langage simple, catégorie concernée,
bouton de demande d'exception.

**Notifications parents** : Web Push dans un premier temps, ntfy ou Telegram
en option.

**Langue** : français d'abord, interface prête pour l'i18n.

## 11. Feuille de route

**Phase 0 — squelette** (ce dépôt) : workspace Cargo, crates vides mais
compilées, types du domaine, CI, Dockerfile.

**Phase 1 — MVP**
1. Serveur : familles, parents, enfants, appareils, politiques, API, UI.
2. Agent Linux : DNS local avec listes UT1, horaires, quota, verrouillage
   logind, préavis.
3. Exceptions temporaires et demandes depuis l'appareil de l'enfant, Web Push.
4. Tableau de bord : niveaux 1 et 2, alertes de contournement.
5. DNS LAN pour la TV connectée avec profil appareil.
6. Paquet `.deb` pour l'agent, image Docker pour le serveur.

**Phase 2** : niveau 3 diagnostic, niveau 4 applications, agent Windows,
profil DNS privé Android/iOS, listes additionnelles, mode vacances.

**Phase 3** : agent macOS, Postgres, multi-famille (hébergement partagé),
mise à jour automatique des agents.

## 12. Risques

| Risque | Parade |
|--------|--------|
| La box ne permet pas de distribuer un DNS personnalisé (Livebox) | DNS manuel par appareil, ou DHCP intégré en phase 2 |
| DoH, ECH, VPN gratuits | nftables + politiques navigateurs + alertes ; aucune parade n'est totale |
| Cache DNS côté navigateur retarde les exceptions | TTL court, vidage par l'agent |
| Notifications dans la session enfant sous Wayland | helper de session via D-Bus `org.freedesktop.Notifications` |
| Listes UT1 volumineuses (plusieurs millions de domaines) | structure compacte en mémoire, chargement paresseux par catégorie |
| Contournement physique (live USB) | documentation BIOS / chiffrement |
| Toolchain Rust système ancienne (1.87) | `rust-toolchain.toml` épingle 1.99.0 pour ce projet, sans toucher au défaut |

## 13. Questions ouvertes

1. **Licence** : GPLv3 (esprit CTparental), AGPLv3 (serveur réseau) ou MIT ?
2. **Modèle de box** : permet-elle de changer le DNS du DHCP ?
3. **Ordinateur familial** : existe-t-il aujourd'hui, sous quel OS ?
4. **Canal de notification** préféré : Web Push, ntfy, Telegram ?
5. **Hébergement du dépôt** : GitHub public dès le départ ?
