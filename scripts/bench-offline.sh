#!/usr/bin/env bash
# bench-offline (T05 §6) : à lancer en local avant toute PR qui touche le cœur. Exige le cache de
# banc (~/.cache/osm-route-details/bench ou ORD_BENCH_CACHE) et, pour les cas perso,
# bench/private/ ; aucun réseau. Le results.json produit va dans bench/reports/ (non réécrit s'il
# existe) ; il est commité avec la PR et contrôlé par le job CI bench-report.
# Ne lance jamais `judge` : le jugement de la série tenue à l'écart est unique (T05 §3.3).
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release --locked -p ord-bench
bench=target/release/ord-bench

# Golden files sur le cache de banc (ignorés sans cache, jamais verts).
cargo test --release --locked -p ord-bench --test golden -- --include-ignored
"$bench" check
"$bench" run --offline "$@"
# Comparaison avec la référence de banc, si elle existe ; contrôle du job CI en local.
if [[ -f bench/baseline.json ]]; then
  latest=$(ls -d bench/reports/*/ | sort | tail -n 1)
  "$bench" compare bench/baseline.json "${latest}results.json"
fi
"$bench" bench-report
