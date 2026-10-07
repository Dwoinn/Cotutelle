#!/usr/bin/env bash
# Construit le paquet Debian de l'agent : target/debian/cotutelle-agent_<version>_<arch>.deb
# Ne dépend que de cargo et de dpkg-deb.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(cargo metadata --no-deps --format-version 1 |
  python3 -c "import sys, json; print(next(p['version'] for p in json.load(sys.stdin)['packages'] if p['name'] == 'cotutelle-agent'))")
ARCH=$(dpkg --print-architecture)
ROOT="target/debian/cotutelle-agent_${VERSION}_${ARCH}"

cargo build --release -p cotutelle-agent

rm -rf "$ROOT"
install -D -m 0755 target/release/cotutelle-agent "$ROOT/usr/bin/cotutelle-agent"
install -D -m 0644 packaging/cotutelle-agent.service "$ROOT/usr/lib/systemd/system/cotutelle-agent.service"
install -D -m 0644 packaging/cotutelle.desktop "$ROOT/usr/share/applications/cotutelle.desktop"
install -D -m 0644 web/src/lib/assets/favicon.svg "$ROOT/usr/share/icons/hicolor/scalable/apps/cotutelle.svg"
install -D -m 0644 README.md "$ROOT/usr/share/doc/cotutelle-agent/README.md"
install -d "$ROOT/DEBIAN"
install -m 0755 packaging/debian/postinst packaging/debian/prerm packaging/debian/postrm "$ROOT/DEBIAN/"

cat > "$ROOT/DEBIAN/control" <<CONTROL
Package: cotutelle-agent
Version: ${VERSION}
Architecture: ${ARCH}
Maintainer: Dwoinn <dwoinn@users.noreply.github.com>
Section: admin
Priority: optional
Depends: libc6, systemd, nftables, libnotify-bin, xdg-utils
Installed-Size: $(du -sk "$ROOT" | cut -f1)
Homepage: https://github.com/Dwoinn/cotutelle
Description: Agent de contrôle parental Cotutelle
 Applique sur cet ordinateur les règles définies par les parents sur le
 serveur Cotutelle : filtrage DNS, horaires, temps d'écran et verrouillage
 de session. Continue de protéger sans connexion au serveur.
CONTROL

dpkg-deb --build --root-owner-group "$ROOT" >/dev/null
echo "$ROOT.deb"
