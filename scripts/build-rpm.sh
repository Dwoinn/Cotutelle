#!/usr/bin/env bash
# Construit le paquet RPM de l'agent : target/rpm/RPMS/<arch>/cotutelle-agent-<version>-1.<arch>.rpm
# Ne dépend que de cargo et de rpmbuild. AGENT_BIN désigne un binaire déjà
# compilé (voir docker/Dockerfile.agent) ; sans lui, le script compile l'agent.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(cargo metadata --no-deps --format-version 1 |
  python3 -c "import sys, json; print(next(p['version'] for p in json.load(sys.stdin)['packages'] if p['name'] == 'cotutelle-agent'))")
ARCH=$(uname -m)
TOP="$PWD/target/rpm"

if [ -z "${AGENT_BIN:-}" ]; then
  cargo build --release -p cotutelle-agent
  AGENT_BIN=target/release/cotutelle-agent
fi

rm -rf "$TOP"
rpmbuild -bb packaging/rpm/cotutelle-agent.spec \
  --define "_topdir $TOP" \
  --define "pkg_version $VERSION" \
  --define "srcdir $PWD" \
  --define "agent_bin $(realpath "$AGENT_BIN")" >/dev/null
echo "target/rpm/RPMS/$ARCH/cotutelle-agent-$VERSION-1.$ARCH.rpm"
