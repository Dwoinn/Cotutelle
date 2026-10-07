#!/usr/bin/env bash
# Remplit une instance de développement avec une famille d'exemple :
# un parent, deux enfants, une télévision partagée et une semaine d'activité.
#
# Usage : scripts/dev-seed.sh [adresse du serveur] [répertoire des données]
# Identifiants de test, à ne jamais utiliser hors développement :
#   nom « Parent », mot de passe ci-dessous.
set -euo pipefail

SERVER="${1:-http://127.0.0.1:8087}"
DATA_DIR="${2:-data/dev}"
DEV_PASSWORD="${COTUTELLE_DEV_PASSWORD:-cotutelle-dev}"
API="$SERVER/api/v1"
JAR="$(mktemp)"
trap 'rm -f "$JAR"' EXIT

call() { # méthode chemin [corps]
  curl -fsS -b "$JAR" -c "$JAR" -X "$1" -H 'content-type: application/json' ${3:+-d "$3"} "$API$2"
}
field() { python3 -c "import sys, json; print(json.load(sys.stdin)['$1'])"; }

if [ "$(call GET /status | field setup_done)" = "True" ]; then
  echo "Cette instance est déjà installée : rien à faire." >&2
  exit 0
fi

call POST /setup "{\"name\":\"Parent\",\"password\":\"$DEV_PASSWORD\"}" >/dev/null
YEAR=$(date +%Y)
LEO=$(call POST /children "{\"name\":\"Léo\",\"birth_year\":$((YEAR - 11))}" | field id)
MIA=$(call POST /children "{\"name\":\"Mia\",\"birth_year\":$((YEAR - 8))}" | field id)
TV=$(call POST /devices '{"name":"TV du salon","ip":"192.168.1.50"}' | field id)
CODE=$(call POST /devices/enroll-code '{"name":"Portable de Léo"}' | field code)

# Un ordinateur enrôlé comme le ferait l'agent, puis son compte rattaché à Léo.
PC=$(curl -fsS -H 'content-type: application/json' "$API/agent/enroll" \
  -d "{\"token\":\"$CODE\",\"hostname\":\"portable-leo\",\"os\":\"linux\",\"agent_version\":\"0.1.0\",\"os_accounts\":[\"leo\",\"parent\"]}" | field device)
call PUT "/devices/$PC/accounts" "{\"leo\":\"$LEO\"}" >/dev/null
call POST /grants "{\"child_id\":\"$MIA\",\"target\":{\"kind\":\"service\",\"service\":\"youtube\"},\"minutes\":45}" >/dev/null

# Une semaine d'historique, écrite directement en base.
python3 - "$DATA_DIR/cotutelle.db" "$LEO" "$PC" <<'PY'
import datetime, json, random, sqlite3, sys
db, child, device = sys.argv[1:4]
random.seed(7)
con = sqlite3.connect(db)
sites = ["wikipedia.org", "lumni.fr", "scratch.mit.edu", "minecraft.net", "youtube.com", "pronote.net", "vikidia.org", "ecoledirecte.com"]
blocked = [("tiktok.com", {"kind": "service", "id": "tiktok"}), ("discord.com", {"kind": "service", "id": "discord"}),
           ("jeux-argent.example", {"kind": "category", "id": "gambling"})]
today = datetime.date.today()
for back in range(7):
    day = (today - datetime.timedelta(days=back)).isoformat()
    seconds = random.randint(20, 95) * 60 if back else 34 * 60
    con.execute("INSERT OR REPLACE INTO usage VALUES (?, 'leo', ?, ?, ?)", (device, day, child, seconds))
    for site in random.sample(sites, 6):
        con.execute("INSERT OR REPLACE INTO dns_daily VALUES (?, ?, ?, ?, ?, 0, NULL)",
                    (day, device, child, site, random.randint(5, 220)))
    for site, reason in random.sample(blocked, 2):
        con.execute("INSERT OR REPLACE INTO dns_daily VALUES (?, ?, ?, ?, 0, ?, ?)",
                    (day, device, child, site, random.randint(1, 9), json.dumps(reason)))
# Une demande en attente, comme si Léo l'avait envoyée depuis son espace.
con.execute("INSERT INTO requests (id, child_id, device_id, target, minutes, message, created_at) VALUES (?, ?, ?, ?, 30, ?, ?)",
            ("00000000-0000-4000-8000-000000000001", child, device, json.dumps({"kind": "service", "service": "discord"}),
             "pour parler avec Tom du match", int(datetime.datetime.now().timestamp()) - 240))
con.commit()
PY

echo "Famille d'exemple créée sur $SERVER (parent « Parent », mot de passe dans ce script)."
