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

Sous Debian et Ubuntu :

```bash
scripts/build-deb.sh
sudo apt install ./target/debian/cotutelle-agent_*.deb
```

Dans l'interface : **Appareils › Ajouter un ordinateur**. Un code s'affiche,
valable 15 minutes. Sur l'ordinateur :

```bash
sudo cotutelle-agent enroll --server http://<adresse du serveur>:8080 --code XXXX-XXXX
sudo systemctl enable --now cotutelle-agent
```

Sous une autre distribution, copiez le binaire `cotutelle-agent` dans
`/usr/bin` et `packaging/cotutelle-agent.service` dans `/etc/systemd/system`,
puis suivez les mêmes étapes.

De retour dans l'interface, indiquez quel compte de l'ordinateur appartient à
quel enfant. Un compte laissé sur « Non filtré » n'est soumis à aucune règle :
c'est le réglage des comptes parents.

L'enfant trouve « Mon temps d'écran » dans le menu de ses applications.

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

### Diagnostic

```bash
cotutelle-agent status              # état, compte au premier plan, temps restant
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

## 5. Accès hors de la maison

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
