# Installer Cotutelle

Deux éléments : le **serveur**, une seule fois, sur une machine allumée en
permanence ; l'**agent**, sur chaque ordinateur à protéger.

## 1. Le serveur

### Avec Docker

Sur un Raspberry Pi, un NAS ou tout ordinateur qui reste allumé :

```bash
git clone https://github.com/Dwoinn/cotutelle.git
cd cotutelle
docker compose -f docker/compose.yaml up -d
```

Ouvrez `http://<adresse de la machine>:8080` depuis n'importe quel appareil
du réseau et créez le premier compte parent. Les listes de blocage se
téléchargent seules au premier démarrage, en une à deux minutes.

Les données (base SQLite, listes) vivent dans le volume `cotutelle-data`.

### Sur OpenShift

Les manifestes de `deploy/openshift` créent le namespace `cotutelle`, font
construire l'image par le cluster depuis le dépôt, et déploient le serveur
avec son volume et une route HTTPS :

```bash
oc apply -k deploy/openshift
oc -n cotutelle start-build cotutelle-server --follow
oc -n cotutelle get route cotutelle
```

La route utilise le nom `cotutelle.infra.donovanwinter.fr`, avec un certificat
Let's Encrypt émis et renouvelé par cert-manager. Adaptez ce nom et l'émetteur
dans `deploy/openshift/network.yaml`, et faites pointer le nom, dans votre
DNS, vers l'adresse du routeur du cluster.

SQLite est intégré au binaire : il n'y a pas de base à déployer à côté,
seulement un volume bloc de 2 Gio. Le déploiement garde un seul réplica en
stratégie `Recreate`, car la base n'accepte qu'un écrivain. Une nouvelle
construction redéploie automatiquement.

Le DNS du réseau local n'est pas exposé dans ce mode : derrière le réseau du
cluster, le serveur ne verrait pas l'adresse des appareils. Les ordinateurs
avec agent ne sont pas concernés, ils filtrent en local.

### Sans Docker

```bash
(cd web && pnpm install && pnpm build)
cargo build --release -p cotutelle-server
./target/release/cotutelle-server --data-dir /var/lib/cotutelle-server --web-dir web/build
```

### Options

| Variable | Option | Défaut | Rôle |
|----------|--------|--------|------|
| `COTUTELLE_HTTP_ADDR` | `--http-addr` | `0.0.0.0:8080` | Interface et API |
| `COTUTELLE_DATA_DIR` | `--data-dir` | `data` | Base et listes |
| `COTUTELLE_WEB_DIR` | `--web-dir` | `web/build` | Interface compilée |
| `COTUTELLE_DNS_ADDR` | `--dns-addr` | désactivé | DNS du réseau local |
| `COTUTELLE_BLOCKLIST_SOURCE` | `--blocklist-source` | archive UT1 | URL ou fichier ; vide pour désactiver |
| `COTUTELLE_SECURE_COOKIES` | `--secure-cookies` | non | À activer derrière un proxy HTTPS |
| `TZ` | | fuseau du système | Fuseau du foyer, pour les horaires |

## 2. L'agent, sur un ordinateur Linux

Prérequis : systemd-resolved, nftables, et un compte **sans droits
d'administration** pour l'enfant. Cible principale : Ubuntu avec GNOME.
Essayé en réel sur Omarchy (Arch Linux, Hyprland) ; pas encore sur Ubuntu :
faites le premier essai sur une machine de test.

Chaque [version publiée](https://github.com/Dwoinn/cotutelle/releases) fournit
l'agent pour x86_64 et aarch64, lié à la glibc 2.28 (Debian 10, Ubuntu 20.04,
RHEL 8 et suivants) :

```bash
sudo apt install ./cotutelle-agent_*.deb    # Debian, Ubuntu
sudo dnf install ./cotutelle-agent-*.rpm    # Fedora, RHEL, openSUSE (zypper)
```

Pour construire le paquet depuis les sources : `scripts/build-deb.sh` ou
`scripts/build-rpm.sh`.

Dans l'interface : **Appareils › Ajouter un ordinateur**. Un code s'affiche,
valable 15 minutes. Sur l'ordinateur :

```bash
sudo cotutelle-agent enroll --server http://<adresse du serveur>:8080 --code XXXX-XXXX
sudo systemctl enable --now cotutelle-agent
```

Sous une autre distribution, l'archive `cotutelle-agent-*-linux-*.tar.gz`
contient le binaire `cotutelle-agent`, à copier dans `/usr/bin`, et
`cotutelle-agent.service`, à copier dans `/etc/systemd/system` ; suivez
ensuite les mêmes étapes.

De retour dans l'interface, indiquez quel compte de l'ordinateur appartient à
quel enfant. Un compte laissé sur « Non filtré » n'est soumis à aucune règle :
c'est le réglage des comptes parents.

L'enfant trouve « Mon temps d'écran » dans le menu de ses applications.

### L'assistant de session

Le paquet installe aussi `cotutelle-session`, lancé dans chaque session
graphique (`/etc/xdg/autostart`). Dans la session d'un enfant, il affiche une
icône de barre avec son temps restant et, dans son menu, le détail : jusqu'à
quelle heure, temps du jour, règles de vacances, ouvertures en cours. Il
prévient à 10, 5 et 1 minute, explique une fermeture (temps écoulé, horaires,
pause) et quand ça rouvre, signale un site bloqué avec le motif, et propose
« Demander du temps » ou « Demander l'accès » quand les parents ont activé les
demandes. Elles arrivent aux parents comme celles de l'espace enfant, et la
réponse revient sur le bureau. Dans la session d'un parent, l'icône reste
cachée.

Deux fenêtres complètent les notifications. Quand l'accès se ferme, ou s'il
est déjà fermé à l'ouverture de session, un écran plein donne le motif, compte
les secondes avant le verrou et propose « Demander du temps » (trente minutes)
ou « Fermer ». L'entrée de menu « Demander autre chose… » ouvre un formulaire :
un service proposé par les parents, un autre site ou du temps en plus, pendant
combien de temps (de 5 minutes à 2 heures) et un mot pour eux. Ces fenêtres
sont en GTK 4 et libadwaita, que les paquets recommandent (`libgtk-4-1` et
`libadwaita-1-0` sous Debian et Ubuntu, `gtk4` et `libadwaita` sous Fedora et
RHEL) ; l'assistant publié demande une distribution de 2022 ou plus récente
(Debian 12, Ubuntu 22.04, RHEL 9). Sans fenêtres possibles (session sans
affichage), la fermeture reste une notification critique et « Demander autre
chose… » ouvre l'espace enfant dans le navigateur. Pour un assistant sans GTK
du tout, compilez-le avec `--no-default-features`.

Quand l'assistant est là, l'agent laisse trente secondes entre l'explication
et le verrouillage ; sans lui (autre bureau, session texte), il garde ses
notifications `notify-send` et verrouille aussitôt. Sous GNOME, l'icône de
barre passe par l'extension AppIndicator, active par défaut sous Ubuntu ; les
notifications fonctionnent partout. L'assistant n'a aucun privilège : il
parle à l'agent par `/run/cotutelle/agent.sock` et ne voit que son propre
compte. Ses messages sont dans `journalctl --user`.

Pour le voir à l'œuvre sans rien verrouiller : `cotutelle-agent run
--dry-run`, puis `cotutelle-session` dans la session (`--socket` si la socket
n'est pas à l'endroit habituel).

### Ce que l'agent change sur la machine

- `/etc/systemd/resolved.conf.d/cotutelle.conf` : tout le DNS passe par le
  résolveur local de l'agent.
- Une table nftables `inet cotutelle` : seuls root et la boucle locale peuvent
  émettre du DNS vers l'extérieur.
- Des politiques désactivant le DNS chiffré intégré des navigateurs
  installés (Firefox, Chromium, Chrome, Brave, Edge). Un fichier de politique
  Firefox déjà présent n'est jamais écrasé.

Tout est retiré par `sudo cotutelle-agent release`, exécuté automatiquement à
l'arrêt du service et à la désinstallation.

### Verrouillage selon le bureau

GNOME et KDE verrouillent sur ordre de logind. Si votre bureau l'ignore,
l'agent lance son verrou habituel (`omarchy-system-lock`, `hyprlock`,
`swaylock`…). Pour en imposer un autre, ajoutez à l'unité systemd :

```ini
[Service]
Environment=COTUTELLE_LOCK_COMMAND=mon-verrou --option
```

Si aucun verrou ne fonctionne, les parents reçoivent une alerte et le DNS du
compte est coupé tant que l'accès reste fermé.

### Temps d'écran et écran éteint

Le temps ne compte que pour la session au premier plan, déverrouillée et en
usage. L'agent lit l'état de logind (`LockedHint`, `IdleHint`) et, comme tous
les bureaux ne le renseignent pas, demande aussi au bureau si son économiseur
d'écran est actif (`org.gnome.ScreenSaver`, `org.freedesktop.ScreenSaver`
pour KDE, équivalents Cinnamon, MATE et XFCE, `hyprctl` pour Hyprland). Un
écran noir, voilé ou verrouillé ne compte donc pas, qu'il le soit à la main,
après un moment d'inactivité ou au retour de veille. Une vidéo en plein
écran, qui retient l'économiseur, compte.

Pour vérifier ce que l'agent voit :

```bash
cotutelle-agent status
```

La clé `sessions` donne, pour chaque session ouverte, `en usage`,
`écran éteint ou inactive`, `verrouillée` ou `arrière-plan`, et le journal
(`journalctl -u cotutelle-agent`) consigne chaque changement.

### Diagnostic

```bash
cotutelle-agent status              # état, compte au premier plan, temps restant, assistant connecté
journalctl -u cotutelle-agent -f    # journal
```

## 3. Les appareils sans agent

Télévision, console, tablette : déclarez-les dans **Appareils › Appareils sans
agent** avec leur adresse IP, puis faites-les passer par le DNS du serveur.

1. Activez le DNS du serveur. Avec Docker, décommentez les deux lignes `53`
   de `docker/compose.yaml` en y mettant l'adresse de la machine.
2. Donnez à chaque appareil déclaré une adresse IP fixe (bail DHCP statique
   sur la box).
3. Indiquez l'adresse du serveur comme DNS : soit dans le DHCP de la box, pour
   tout le réseau, soit dans les réglages réseau de chaque appareil. Certaines
   box, dont la Livebox, n'autorisent que la seconde méthode.

Un appareil du réseau qui n'est pas déclaré utilise le DNS sans filtrage ni
journalisation.

## 4. Notifications sur téléphone

Dans **Réglages › Notifications**, collez l'adresse d'un sujet
[ntfy](https://ntfy.sh), par exemple `https://ntfy.sh/famille-x7k2m9q4`, et
abonnez-vous à ce sujet dans l'application ntfy. Le nom du sujet tient lieu de
mot de passe : choisissez-le long et aléatoire. ntfy peut aussi s'auto-héberger.

## 5. Vacances scolaires

Dans **Réglages › Vacances scolaires**, choisissez votre zone (A, B, C, Corse
ou outre-mer) : le serveur télécharge le calendrier officiel de l'Éducation
nationale, le garde en base et le vérifie chaque semaine. Vous pouvez écarter
une période ou en ajouter une : un pont, un voyage, une semaine de stage.

Ces périodes ne changent rien tant qu'un enfant n'a pas de règles de
vacances. Pour lui en donner, ouvrez ses **Paramètres**, onglet **Temps**,
et activez « Règles différentes pendant les vacances scolaires » : ses
horaires, ses quotas et son filtre de vacances partent de ceux de la période
scolaire, à ajuster dans les onglets Temps et Filtre. Les agents reçoivent
les périodes avec la politique et changent de règles au premier jour, même
sans serveur.

## 6. Accès hors de la maison

N'ouvrez pas le port 8080 sur Internet. Utilisez un VPN personnel comme
Tailscale ou WireGuard pour joindre le serveur depuis l'extérieur. Le portable
d'un enfant, lui, reste protégé partout : son agent applique la dernière
politique reçue et se resynchronise au retour.

## Sécurité : à savoir

- L'interface est servie en HTTP : sur le réseau local, mots de passe et
  cookies circulent en clair. Placez un proxy HTTPS (Caddy, Traefik) devant
  le serveur dès que possible, avec `COTUTELLE_SECURE_COOKIES=true`.
- Un filtre DNS se contourne avec un VPN, un autre système démarré sur clé
  USB ou un partage de connexion. Protégez le BIOS par mot de passe et
  désactivez le démarrage sur USB. Aucun filtre ne remplace la discussion.
