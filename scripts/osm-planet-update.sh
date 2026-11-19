#!/usr/bin/env bash
# osm-planet-update.sh : tient à jour un planet OSM (.osm.pbf) avec les diffs quotidiens d'openstreetmap.org.
#
#   scripts/osm-planet-update.sh [options] <planet.osm.pbf>
#
# - Fichier absent : télécharge `planet-latest.osm.pbf` depuis un miroir, après avoir vérifié que la somme MD5 du
#   miroir est celle publiée par planet.openstreetmap.org (un miroir en retard est refusé), puis contrôle le fichier.
# - Fichier présent : applique les diffs quotidiens manquants, par lots de `--days` jours (chaque lot réécrit le
#   planet en entier, ≈ 96 Go ; `osmium apply-changes` tient les diffs d'un lot en mémoire : 4,0 Go mesurés pour 7 jours).
#   Le point de départ est `osmosis_replication_sequence_number` de l'en-tête, écrit par ce script (retenu seulement si
#   `osmosis_replication_base_url` est le serveur quotidien) ; à défaut (planet tout juste téléchargé), la séquence
#   quotidienne au plus tard à `osmosis_replication_timestamp` (pyosmium).
# - L'en-tête écrit déclare `Sort.Type_then_ID` (exigé par `ord-graph-build`) et la réplication (séquence, date,
#   serveur) : `ord-graph-build` date le graphe avec `osmosis_replication_timestamp`.
# - Le planet n'est remplacé qu'une fois le lot écrit en entier (`--fsync`, puis `mv` dans le même dossier) : une
#   interruption laisse l'ancien intact. Un verrou (`flock`) empêche deux mises à jour du même fichier.
# - Écrit à côté le polygone de couverture `<planet sans .osm.pbf>.poly` (copie de `scripts/planet.poly`, le
#   rectangle du monde), que le générateur du service lit avec le graphe, comme le `.poly` Geofabrik d'un extrait.
#
# Options :
#   --days N        jours de diffs par lot (défaut 7)
#   --until SEQ     s'arrête à la séquence quotidienne SEQ (défaut : la dernière publiée)
#   --mirror URL    miroir du dossier `pbf/` du planet (défaut https://ftp.fau.de/osm-planet/pbf)
#   --check         après chaque lot, `osmium fileinfo -e` vérifie l'ordre des objets (≈ 10 min de plus sur le planet)
#   --keep-old      garde le planet précédent en `<planet>.prev` (il faut alors ≈ 3 × 96 Go libres)
#
# Dépendances : osmium-tool, pyosmium (module Python `osmium`), curl, md5sum, flock.
# Place disque : le planet, plus un planet en cours d'écriture, plus les diffs du lot (≈ 1 Go pour 7 jours).
set -euo pipefail

REPLICATION=https://planet.openstreetmap.org/replication/day
OFFICIAL=https://planet.openstreetmap.org/pbf
MIRROR=https://ftp.fau.de/osm-planet/pbf
DAYS=7
UNTIL=
CHECK=0
KEEP_OLD=0

usage() { sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit "${1:-0}"; }
log() { printf '%s %s\n' "$(date -u +%FT%TZ)" "$*" >&2; }
die() { log "erreur : $*"; exit 2; }

while [ $# -gt 0 ]; do
  case $1 in
    --days) DAYS=$2; shift 2 ;;
    --until) UNTIL=$2; shift 2 ;;
    --mirror) MIRROR=${2%/}; shift 2 ;;
    --check) CHECK=1; shift ;;
    --keep-old) KEEP_OLD=1; shift ;;
    -h|--help) usage ;;
    -*) die "option inconnue : $1" ;;
    *) break ;;
  esac
done
[ $# -eq 1 ] || usage 2
PLANET=$(realpath -m "$1")
DIR=$(dirname "$PLANET")
[[ $DAYS =~ ^[1-9][0-9]*$ ]] || die "--days : entier positif attendu"
[ -z "$UNTIL" ] || [[ $UNTIL =~ ^[0-9]+$ ]] || die "--until : séquence attendue"
for c in osmium curl md5sum flock python3; do command -v "$c" >/dev/null || die "$c introuvable"; done
python3 -c 'import osmium.replication.server' 2>/dev/null || die "pyosmium (module Python osmium) introuvable"
mkdir -p "$DIR"

exec 9>"$PLANET.lock"
flock -n 9 || die "une autre mise à jour de $PLANET est en cours"
# Restes d'une mise à jour tuée (le `trap` ne voit pas SIGKILL) : un planet partiel pèse jusqu'à ≈ 96 Go. Le verrou
# est tenu : aucun autre lancement sur ce planet ne les écrit.
for stale in "$DIR/.planet-update.$(basename "$PLANET")".*; do
  [ -e "$stale" ] && { log "restes d'une mise à jour interrompue supprimés : $stale"; rm -rf "$stale"; }
done

CURL=(curl --fail --location --silent --show-error --retry 5 --retry-delay 30)

# Octets libres dans le dossier du planet.
free_bytes() { df --output=avail -B1 "$DIR" | tail -1; }

# --- Téléchargement initial ------------------------------------------------------------------------------------
if [ ! -e "$PLANET" ]; then
  official=$("${CURL[@]}" "$OFFICIAL/planet-latest.osm.pbf.md5" | cut -d' ' -f1)
  mirrored=$("${CURL[@]}" "$MIRROR/planet-latest.osm.pbf.md5" | cut -d' ' -f1)
  [ -n "$official" ] || die "somme MD5 officielle illisible"
  [ "$official" = "$mirrored" ] || die "le miroir $MIRROR n'a pas le dernier planet (MD5 $mirrored, officiel $official)"
  [ "$(free_bytes)" -gt $((110 * 1024 ** 3)) ] || die "moins de 110 Gio libres dans $DIR"
  log "téléchargement de $MIRROR/planet-latest.osm.pbf (MD5 $official)"
  # Reprise d'un téléchargement interrompu : `.part` garde ce qui est déjà là.
  "${CURL[@]}" --continue-at - --output "$PLANET.part" "$MIRROR/planet-latest.osm.pbf"
  log "contrôle MD5"
  echo "$official  $PLANET.part" | md5sum --check --status || { rm -f "$PLANET.part"; die "MD5 faux, fichier supprimé"; }
  mv "$PLANET.part" "$PLANET"
  log "planet téléchargé : $PLANET"
fi

# --- Polygone de couverture ---------------------------------------------------------------------------------------
POLY_SRC=$(dirname "$(realpath "$0")")/planet.poly
POLY=${PLANET%.osm.pbf}.poly
[ -f "$POLY_SRC" ] || die "$POLY_SRC introuvable"
if ! cmp -s "$POLY_SRC" "$POLY"; then
  cp "$POLY_SRC" "$POLY.tmp" && mv "$POLY.tmp" "$POLY"
  log "polygone écrit : $POLY"
fi

# --- Point de départ et dernière séquence publiée -----------------------------------------------------------------
header() { osmium fileinfo --no-progress -g "header.option.$1" "$PLANET"; }

# La séquence de l'en-tête ne vaut que si elle vient du même serveur : un extrait Geofabrik porte la sienne.
seq=
if [ "$(header osmosis_replication_base_url)" = "$REPLICATION" ]; then
  seq=$(header osmosis_replication_sequence_number)
fi
if [ -z "$seq" ]; then
  ts=$(header osmosis_replication_timestamp)
  [ -n "$ts" ] || die "l'en-tête n'a ni séquence ni date de réplication"
  seq=$(python3 -I - "$REPLICATION/" "$ts" <<'PY'
import sys, datetime as dt
from osmium.replication.server import ReplicationServer
ts = dt.datetime.fromisoformat(sys.argv[2].replace('Z', '+00:00'))
# Dernière séquence dont la date est au plus tard celle du planet : les diffs suivants couvrent tout ce qui manque
# (un recouvrement de moins d'un jour est sans effet, apply-changes garde la version la plus récente).
s = ReplicationServer(sys.argv[1]).timestamp_to_sequence(ts)
if s is None: sys.exit(1)
print(s)
PY
) || die "séquence introuvable pour $ts"
  log "en-tête sans séquence : départ à la séquence quotidienne $seq (planet du $ts)"
fi

# Séquence et date d'un état du serveur (`state` : le dernier).
state() {
  python3 -I - "$REPLICATION/" "${1:-}" <<'PY'
import sys
from osmium.replication.server import ReplicationServer
st = ReplicationServer(sys.argv[1]).get_state_info(int(sys.argv[2]) if sys.argv[2] else None)
if st is None: sys.exit(1)
print(st.sequence, st.timestamp.strftime('%Y-%m-%dT%H:%M:%SZ'))
PY
}

read -r latest _ < <(state) || die "état du serveur de réplication illisible"
target=${UNTIL:-$latest}
[ "$target" -le "$latest" ] || die "--until $target au-delà de la dernière séquence publiée ($latest)"
if [ "$seq" -ge "$target" ]; then
  log "à jour : séquence $seq (dernière publiée $latest)"
  exit 0
fi
log "séquence $seq → $target : $((target - seq)) jour(s), par lots de $DAYS"

# --- Lots ---------------------------------------------------------------------------------------------------------
WORK=$(mktemp -d "$DIR/.planet-update.$(basename "$PLANET").XXXXXX")
trap 'rm -rf "$WORK"' EXIT

diff_url() { printf '%s/%03d/%03d/%03d.osc.gz' "$REPLICATION" $(($1 / 1000000)) $(($1 / 1000 % 1000)) $(($1 % 1000)); }

while [ "$seq" -lt "$target" ]; do
  end=$((seq + DAYS < target ? seq + DAYS : target))
  read -r _ end_ts < <(state "$end") || die "état $end illisible"
  log "lot $((seq + 1))..$end (jusqu'au $end_ts) : téléchargement"
  diffs=()
  for ((s = seq + 1; s <= end; s++)); do
    f=$WORK/$s.osc.gz
    "${CURL[@]}" --output "$f" "$(diff_url "$s")"
    gzip --test "$f" || die "diff $s corrompu"
    diffs+=("$f")
  done

  size=$(stat -c %s "$PLANET")
  [ "$(free_bytes)" -gt $((size + size / 10)) ] || die "pas assez de place dans $DIR pour réécrire le planet ($size octets)"
  log "lot $((seq + 1))..$end : application"
  osmium apply-changes --no-progress --fsync "$PLANET" "${diffs[@]}" -o "$WORK/planet.osm.pbf" \
    --output-header=sorting=Type_then_ID \
    --output-header="timestamp=$end_ts" \
    --output-header="osmosis_replication_timestamp=$end_ts" \
    --output-header="osmosis_replication_sequence_number=$end" \
    --output-header="osmosis_replication_base_url=$REPLICATION"
  if [ "$CHECK" = 1 ]; then
    log "lot $((seq + 1))..$end : contrôle de l'ordre"
    osmium fileinfo --no-progress -e "$WORK/planet.osm.pbf" | grep -q 'Objects ordered (by type and id): yes' \
      || die "objets hors d'ordre après le lot $((seq + 1))..$end ; planet inchangé"
  fi

  [ "$KEEP_OLD" = 1 ] && ln -f "$PLANET" "$PLANET.prev"
  mv "$WORK/planet.osm.pbf" "$PLANET"
  rm -f "${diffs[@]}"
  seq=$end
  log "planet à la séquence $seq ($end_ts)"
done
log "à jour : séquence $seq"
