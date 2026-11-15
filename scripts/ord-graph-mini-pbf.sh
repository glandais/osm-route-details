#!/usr/bin/env bash
# Régénère le PBF minuscule du test de ord-graph (décodage réel) depuis `mini.osm`.
# `osmium sort` écrit l'en-tête `Sort.Type_then_ID` que `PbfSource` exige.
set -euo pipefail
dir="$(cd "$(dirname "$0")/.." && pwd)/crates/ord-graph/tests/data"
osmium sort --overwrite -o "$dir/mini.osm.pbf" "$dir/mini.osm"
ls -l "$dir/mini.osm.pbf"
