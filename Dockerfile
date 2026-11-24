# syntax=docker/dockerfile:1
# Image du service osm-route-details (T01 §10) : binaire unique, distroless, utilisateur non root.
# Une seule instance par IP sortante : la cadence IGN (20 req/s) est locale au processus (T07 §3).
# Construction : docker build -t osm-route-details .   (publication : .github/workflows/docker.yml)

# --- construction ---------------------------------------------------------------------------
# Rust et Debian épinglés : construction reproductible.
FROM rust:1.100.0-slim-bookworm AS build
WORKDIR /src

# Dépendances d'abord, dans une couche à part : manifestes du workspace et sources vides.
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY crates/ord-adapters/Cargo.toml crates/ord-adapters/
COPY crates/ord-core/Cargo.toml crates/ord-core/
COPY crates/ord-data/Cargo.toml crates/ord-data/
COPY crates/ord-domain/Cargo.toml crates/ord-domain/
COPY crates/ord-pipeline/Cargo.toml crates/ord-pipeline/
COPY crates/ord-server/Cargo.toml crates/ord-server/
COPY crates/ord-testkit/Cargo.toml crates/ord-testkit/
# Générateur de parcours (lot 3a) : le graphe, l'appariement des surcouches, le moteur et son contrôleur.
COPY crates/ord-graph/Cargo.toml crates/ord-graph/
COPY crates/ord-graph-match/Cargo.toml crates/ord-graph-match/
COPY crates/ord-loops/Cargo.toml crates/ord-loops/
COPY crates/ord-loops-check/Cargo.toml crates/ord-loops-check/
# Les outils ne sont pas dans l'image, mais le workspace doit pouvoir lire leurs manifestes.
COPY tools/ord-bench/Cargo.toml tools/ord-bench/
COPY tools/ord-graph-build/Cargo.toml tools/ord-graph-build/
COPY tools/ord-graph-overlay/Cargo.toml tools/ord-graph-overlay/
COPY tools/ord-loops-bench/Cargo.toml tools/ord-loops-bench/
RUN for d in crates/* tools/*; do mkdir -p "$d/src" && touch "$d/src/lib.rs"; done \
 && echo 'fn main() {}' > crates/ord-server/src/main.rs \
 && echo 'fn main() {}' > tools/ord-bench/src/main.rs \
 && cargo build --release --locked -p ord-server \
 && rm -rf crates/*/src tools/*/src

# Sources du service seulement (ord-testkit et les outils restent vides : hors image, T05 §5.1).
COPY crates/ord-adapters/src crates/ord-adapters/src
COPY crates/ord-core/src crates/ord-core/src
COPY crates/ord-data/src crates/ord-data/src
COPY crates/ord-domain/src crates/ord-domain/src
COPY crates/ord-pipeline/src crates/ord-pipeline/src
COPY crates/ord-graph/src crates/ord-graph/src
COPY crates/ord-graph-match/src crates/ord-graph-match/src
COPY crates/ord-loops/src crates/ord-loops/src
COPY crates/ord-loops-check/src crates/ord-loops-check/src
COPY crates/ord-server/src crates/ord-server/src
RUN for d in crates/ord-testkit tools/*; do mkdir -p "$d/src" && touch "$d/src/lib.rs"; done \
 && echo 'fn main() {}' > tools/ord-bench/src/main.rs \
 && find crates -name '*.rs' -exec touch {} + \
 && cargo build --release --locked -p ord-server \
 && mkdir -p /out/data /out/cache \
 && cp target/release/osm-route-details /out/

# --- exécution ------------------------------------------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/osm-route-details /usr/local/bin/osm-route-details
# /data : fichiers locaux en lecture seule (couverture Mapterhorn, .versatiles), montés ;
# /var/cache/osm-route-details : cache disque des tuiles, seul emplacement inscriptible (doc 01 §8.1).
COPY --from=build --chown=65532:65532 /out/data /data
COPY --from=build --chown=65532:65532 /out/cache /var/cache/osm-route-details
ENV ORD_SERVER__LISTEN_ADDR=0.0.0.0:8080 \
    ORD_SERVER__METRICS_ADDR=0.0.0.0:9090 \
    ORD_CACHE__DIR=/var/cache/osm-route-details \
    ORD_LOG__FORMAT=json \
    MALLOC_ARENA_MAX=2 \
    MALLOC_MMAP_THRESHOLD_=1048576
# malloc de glibc : au-delà de 1 Mo, les blocs (fenêtres du générateur) viennent de `mmap` et sont rendus au système
# à leur éviction ; sans ce réglage, l'anonyme dépasse le budget compté de 400 à 600 Mo (lot 6, mesures §2.2).
VOLUME ["/var/cache/osm-route-details"]
# 8080 : API publique ; 9090 : /metrics et /openapi.json, à ne pas publier.
EXPOSE 8080 9090
USER 65532:65532
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD ["/usr/local/bin/osm-route-details", "healthcheck"]
# Arrêt : SIGTERM, requêtes en vol attendues jusqu'au budget (90 s) ; `docker stop -t 100`.
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/osm-route-details"]
CMD ["serve"]
