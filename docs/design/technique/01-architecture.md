# Technique 01 — Architecture globale

> Statut : conception technique. Elle applique la conception générale
> ([01](../01-conception-generale.md)) et le contrat ([02](../02-contrat-interface.md)) sans les
> modifier. Les noms de composants sont ceux du doc 01 §3–4. Les paramètres par défaut indiquent leur
> origine : **[M]** mesuré ou éprouvé dans vcyclist / osm-surface, **[C]** à calibrer au MVP.
> Références : `OS/` = `/Volumes/Data/code/perso/osm-surface`, `VC/` = `/Volumes/Data/code/perso/vcyclist`.

## 0. Carte des types partagés

Référence unique des noms : une section qui consomme un type le prend ici, avec ce nom et cette forme.
T01…T07 = sections techniques 01 à 07. Ports objets (`dyn`) en `#[async_trait]` ; distances en `f64`
mètres le long du parcours ; plages en `Range<f64>` ; enums publiques en `snake_case` (doc 02).

| Type / trait | Propriétaire (crate::module — définition) | Consommateurs |
|---|---|---|
| `LatLon`, `RouteInput`, `ValidatedRequest`, `RequestOptions`, `Sampling`, `SectionSet`, `GainPreset` | `ord-domain::request` — T01 §4.2 | ord-server, ord-pipeline, T03, T04 |
| `PreparedRoute`, `RouteSample` | `ord-domain::route`, produit par `ord-core::prepare` — T01 §5 | T03, T04, T05 |
| `Grid`, `haversine_m`, `project_on_segment` | `ord-domain::geo` — T03 §3.1 | T02 (MVT, couloir), T03, T04 |
| `Budget`, `Deadline`, `Unbounded`, `RequestId` | `ord-domain::budget` — T01 §3.3 | ord-core (contrôle coopératif), T02, T03, T04 |
| `RequestCtx`, `DataLog`, `TileFailure`, `FailureKind` | `ord-domain::ctx` — T02 §2.3 | T01 (§4.4, logs), T03, T04, ord-pipeline |
| `Step<T>`, `TilePlan`, `SourceOutcome<T>`, `CoverageRanges` | `ord-domain::step`, `::coverage` — T01 §5 | ord-core::typology, ord-pipeline, T04 |
| `TileKey`, `SourceId`, `TileOutcome<T>`, `TtlClass`, `DecodeError`, `ElevationTile`, traits `TileCodec`, `TileStore<T>` | `ord-domain::tiles` — T02 §2.2, §6.1 | ord-adapters (codecs), ord-data (impl.), ord-pipeline, T03, T05 |
| trait `TileSource`, `Fetched`, `EntryMeta`, `FetchError`, `TileReader<C>` (impl. `TileStore`) | `ord-data::port` — T02 §2–5 | ord-server (câblage), ord-testkit (`Offline`, `ScriptedServer`) |
| trait `TerrainReader`, `TerrainSample`, `Missing`, `DsmSample`, `TerrainMode`, `TerrainOrigin`, `DemKind` | `ord-domain::elevation`, impl. `ord-data::elevation` — T02 §6.3 | T04 (`sample_raw`), T05 (`SyntheticTerrain`) |
| trait `CoverageRef`, `Extent` | `ord-domain::coverage`, impl. `ord-data::coverage` — T02 §7 | ord-core::coverage, ord-data::elevation |
| trait `DataCatalog`, `Attribution`, `DataVersion`, `DataVersions`, `SourceRole`, `Readiness` | `ord-domain::catalog`, impl. `ord-data` — T02 §9, T01 §9 | ord-server (`meta`, `/v1/attribution`, `/health/ready`), assembleur |
| `WayAttrs`, `SurfaceTag`, `PavedGroup`, `StructureTag`, `TrackType`, `StreetTile`, `StreetLine`, `PointFeature`, trait `StreetSchemaAdapter`, `SchemaCapabilities`, `SurfaceFidelity` | `ord-domain::street` — T03 §3 | ord-adapters::streets (Shortbread), ord-core::typology, ::classification |
| `StreetsCodec<A>`, `BilCodec`, `TerrariumCodec` | `ord-adapters` — T02 §6.1, §8 | ord-server (câblage) |
| `corridor_tiles` | `ord-core::typology::corridor` — T02 §8, T03 §4.1 | ord-pipeline::typology |
| trait `TypologyChain`, `TypologyRequest` | `ord-pipeline::typology` — T03 §10 | Orchestrateur |
| `TypologyOutput`, `TypologyCoverage`, `TypologyStats`, `TypologyError`, `TypedRun`, `RunMatch`, `ElementId`, `ClassId`, `EdgeClass`, `MatchParams` | `ord-core::typology` — T03 §5–6, §10 | ord-pipeline, T04 (`fuse`), assembleur |
| `Structure`, `StructureId`, `StructureKind`, `StructureElement`, `StructureEnd`, `EndKind`, `UnresolvedEnd`, `StructurePassage`, `PassageId`, `PassageDoubt` | `ord-core::typology::structures` — T03 §6 | T04 (fusion), points de rupture, assembleur |
| `Classified`, `StructureClass`, `SurfaceClass`, `SurfaceSource`, `SurfaceDetail`, `WayKind`, `StructureSource`, `RuleSet`, `classify` | `ord-core::classification` — T03 §7 | T04, assembleur, ord-server (DTO, `/v1/capabilities`), T05 |
| `reconcile`, `DetectedStructure`, `StructureVerdict`, `GroupRef` | `ord-core::classification` — T03 §7.1 | T04 (fusion, V1) |
| `build_segments`, `TypedSegment` | `ord-core::typology::segments` — T03 §8 | points de rupture, assembleur |
| `sample_raw`, `sample_structures`, `DsmScope`, `RawProfile` | `ord-pipeline::elevation` (async) — T04 §2, §7.1 | Orchestrateur, `fuse` |
| `fuse`, `ElevationProfile`, `EleMethod`, `TerrainRange`, `StructureOutcome`, `OutcomeMethod`, `CorrectionStatus`, `StructureCorrectionReport`, `GainParams`, `AdaptedRange`, `GainResult`, `ElevationRules`, `ElevationError` | `ord-core::elevation` — T04 §3–5, §7 | assembleur, points de rupture, T05 |
| `DeckDetectorOptions`, `TunnelDetectorOptions`, `detect_decks`, `detect_tunnels` | `ord-core::elevation::heuristics` — T04 §4 | `fuse` |
| `OutputPoint`, `PointOrigin`, `BreakpointReason`, `BreakpointRules`, `select_breakpoints`, `sample_output` | `ord-core::elevation::breakpoints` — T04 §6 | assembleur, ord-server (DTO) |
| `Warning`, `WarningCode` | `ord-domain::report` — T01 §5 | T03, T04, assembleur, ord-server |
| `Rules` (= `RuleSet` + `MatchParams` + `ElevationRules`), `RULES_VERSION` | `ord-core::rules` — T01 §9 | ord-pipeline, ord-server, T05 (`RulesFingerprint`) |
| `RouteReport`, `Status`, `Summary`, `Quality`, `assemble` ; `Versions` | `ord-core::assemble` — T06 §5 ; `Versions` : T01 §9 | ord-pipeline, ord-server (`to_dto`) |
| `Orchestrator`, `Sources`, `Admission`, `PipelineError`, `DataSide`, `Outcome`, `decide_outcome` | `ord-pipeline` — T01 §3, §5.3 | ord-server, T05 |
| `Capabilities`, `Limits`, `OptionBounds`, `EnumCatalog`, `DataFeatures` | `ord-server` — T07 §2, §4 | T05 (contrat) |
| `Config` (et ses sous-structures) | `ord-server` — T07 §5 | ord-server (amorçage), T02 (`DataConfig`) |
| `Problem`, `ProblemCode`, `StrictJson`, DTO v1, `AppState`, `router`, `openapi` | `ord-server` — T01 §4, §10 | T05 |
| `offline_sources`, `ScriptedServer`, `SyntheticTerrain`, types du corpus | `ord-testkit` — T05 §8 | tests, `tools/ord-bench` |

**Hiérarchie d'erreurs** (une seule) : `DecodeError`, `FetchError` (T02) → `TileOutcome::Failed(TileFailure)`,
jamais propagés comme erreur ; `TypologyError` et `ElevationError` = `{Timeout, Cancelled,
Internal}` → `PipelineError` (`From`) → `Problem` (seul site de traduction HTTP, §4.3). Une panne de
source n'est jamais une erreur de chaîne : c'est une donnée (`TypologyCoverage`, `TerrainSample::missing`)
que `decide_outcome` transforme en 200 `partial` ou 503.

## 1. Principes

1. **Cœur pur, bords effectifs** (hexagonal) : les algorithmes sont des fonctions synchrones et
   déterministes, sans réseau, disque, horloge murale ni `tokio`. Seuls l'accès aux données,
   l'orchestrateur et le serveur touchent au monde extérieur.
2. **Style « sans I/O » par étapes** : un calcul du cœur qui a besoin de tuiles ne les lit pas ; il
   rend la liste des tuiles qui lui manquent (`Step::Need`). L'orchestrateur les obtient, puis relance
   l'étape. C'est ce qui permet d'exprimer, sans I/O dans le cœur, le couloir et les tuiles voisines
   lues jusqu'aux extrémités d'un ouvrage (doc 01 §3, étape 3). Le repli IGN → Mapterhorn, lui, est
   interne à l'adaptateur élévation (`TerrainReader`, T02 §6.3), appelé par l'orchestrateur.
3. **Une seule traduction des erreurs** vers RFC 9457, comme `configureProblems` de vcyclist
   (`VC/api/src/main/kotlin/io/github/glandais/api/http/StatusPagesConfig.kt:74-121`).
4. **Tout ce qui est contractuel est versionné** : `api_version`, `rules_version`, `data_versions` (§9).

## 2. Workspace Cargo

```
osm-route-details/
  Cargo.toml                 # [workspace], edition 2021, lints communs, profil release (lto=thin, codegen-units=1)
  crates/
    ord-domain/    # types partagés (§0), enums publiques, ports (traits), Deadline — aucun I/O
    ord-core/      # purs : prepare, coverage, typology::{corridor, graph, matcher, runs, structures, segments},
                   #        classification, elevation::{grid, fusion, heuristics, derive, breakpoints}, assemble, rules
    ord-adapters/  # codecs purs (TileCodec) : streets (MVT + Shortbread → modèle neutre), bil, terrarium
    ord-data/      # Accès données + Caches : port, http, local, disk, memory, elevation (TerrainReader), coverage, attribution
    ord-pipeline/  # Orchestrateur : admission, budget, typology (TypologyChain, boucle Step), elevation (sample_raw), décision
    ord-server/    # binaire `osm-route-details` : axum, DTO v1, config, observabilité, OpenAPI, câblage
    ord-testkit/   # (dev) port Offline, ScriptedServer, terrains synthétiques, corpus (T05 §8)
  tools/           # binaires hors image : ord-bench (T05 §5), inspect, fetch-coverage
  bench/           # données du banc (corpus, critères, rapports) ; bench/private/ hors git ;
                   # cache de banc local hors dépôt, ~/.cache/osm-route-details/bench (T05 §5.3)
  contract/openapi.json      # généré depuis le code, vérifié en test
```

### 2.1 Graphe de dépendances (strict, acyclique)

`ord-domain` ← `ord-core`, `ord-adapters`, `ord-data`, `ord-pipeline` ; `ord-core` ← `ord-pipeline` ;
`ord-adapters`, `ord-data`, `ord-pipeline`, `ord-core` ← `ord-server`. `ord-data` ne dépend pas
d'`ord-adapters` : il reçoit les codecs par généricité (`TileReader<C: TileCodec>`), instanciés par
`ord-server`. `ord-testkit` (← domain, adapters, data) n'est qu'une dépendance de dev ; il ne
dépend pas d'`ord-server`, dont les tests montent eux-mêmes le routeur sur `offline_sources`.

Règles vérifiées en CI (`cargo tree -p <crate> -e normal`) :
- `ord-domain`, `ord-core`, `ord-adapters` ne dépendent ni de `tokio`, ni de `reqwest`, ni d'`axum` ;
  `std::fs`, `std::net` et `std::time::SystemTime` y sont interdits (lint `disallowed-methods`).
- `ord-core` ne connaît pas Shortbread, ni BIL, ni Terrarium : il ne voit que le modèle neutre et
  des grilles d'altitude décodées (doc 01 §4, « deux adaptateurs et un modèle neutre »).
- `ord-pipeline` ne reçoit que des `Arc<dyn …>` ; `ord-server` seul assemble les implémentations.

Le découpage reprend les modules d'osm-surface (`OS/src/lib.rs:4-14`) en séparant ce que son
`trace.rs` mêle : parsing HTTP, orchestration et calcul (`OS/src/trace.rs:231-440`).

### 2.2 Crates externes

| Crate | Où | Rôle, justification |
|---|---|---|
| `tokio` | data, pipeline, server | Runtime multi-thread, `spawn_blocking`, `time::timeout`, `sync::Semaphore`. Éprouvé dans osm-surface. |
| `axum`, `tower`, `tower-http` | server | Routage, extracteurs ; couches `compression` (gzip, br), `decompression`, `limit`, `catch-panic`, `request-id`, `trace` (`OS/src/api.rs:23-32`). |
| `serde`, `serde_json` | partout (serde seul dans le cœur) | DTO, enums publiques en `snake_case`. |
| `serde_ignored` | server | Collecte **toutes** les clés inconnues (chemins), pour un 400 qui les liste toutes, comme `decodeStrict` de vcyclist (`VC/api/.../http/StrictJson.kt:34-68`). `deny_unknown_fields` s'arrête à la première. |
| `thiserror` | partout | Erreurs typées par couche. `anyhow` seulement dans `main` et `tools`. |
| `async-trait` | domain, data, pipeline | Ports objets (`Arc<dyn TileStore<T>>`, `TerrainReader`, `CoverageRef`, `TypologyChain`) choisis par configuration ; sans runtime, donc admis dans `ord-domain`. |
| `reqwest` (rustls, gzip, brotli, http2) | data | Client HTTP unique par processus, pool de connexions (`OS/Cargo.toml`). |
| `moka` (future) | data | LRU mémoire pondéré en octets, déduplication des chargements concurrents (`try_get_with`, `OS/src/tiles.rs:81-83`). |
| `prost` | adapters | Décodage MVT (`OS/src/mvt.rs`). |
| `image-webp` | adapters | Décodage des tuiles Terrarium WebP de Mapterhorn, sans dépendance C. |
| `pmtiles` | data | Lecture des PMTiles de couverture Mapterhorn (exploration 05) et, plus tard, de Mapterhorn auto-hébergé. |
| lecteur `.versatiles` | data | Crate de l'écosystème versatiles-rs si son API de lecture par tuile convient, sinon lecteur maison du format (en-tête, index de blocs, brotli/gzip). Décision au premier jalon production. |
| `rstar` | core | Index spatial du graphe (`OS/Cargo.toml`). |
| `figment` (toml, env) | server | Configuration fichier + environnement fusionnés, typée, erreurs avec le chemin de la clé. |
| `tracing`, `tracing-subscriber` (json, env-filter) | partout sauf domain | Logs structurés (`OS/src/main.rs:10-18`). |
| `metrics`, `metrics-exporter-prometheus` | data, pipeline, server | Façade de métriques ; exporteur servi sur le port interne. |
| `utoipa` | server | OpenAPI 3.1 dérivé des DTO et des handlers, comme le contrat généré de vcyclist (`VC/api/.../openapi/OpenApiContract.kt:26-56`). |
| `uuid` (v7), `tokio-util` | server, pipeline | `request_id` ; `CancellationToken` relié au `Deadline`. |

Écartés : `rayon` dans le cœur (ordre de réduction variable, §9) ; `governor` (pas fixe suffisant).

## 3. Runtime et concurrence

### 3.1 Modèle

- Runtime `tokio` multi-thread, un worker par cœur. Les handlers restent `async` et ne calculent pas.
- **Tout calcul du cœur** passe par `spawn_blocking`, sous un sémaphore `compute_permits`
  (défaut : nombre de CPU **[M]** `OS/src/config.rs:36,71-73`), le permis étant **déplacé dans la
  tâche** : un client qui abandonne ne libère pas un permis encore utilisé (`OS/src/trace.rs:400-415`).
- Les I/O de tuiles sont `async`, bornées à trois niveaux : par hôte et pour tout le processus
  (cadence §6), par source (concurrence), et par requête (`per_request_fetch_concurrency`, défaut
  10 **[M]**, la taille de lot du `BatchCalculator` de vcyclist, `VC/elevation/.../TileRequestPacing.kt:27-29`),
  pour qu'une requête froide n'affame pas les autres.
- Partagé : en lecture seule (`Arc`) la configuration, la couverture, les règles ; mutables et
  auto-protégés, les caches et cadences. Le journal d'échecs est **par requête** (doc 01 §4), comme
  `TileFailureLog` de vcyclist (`VC/elevation/.../TileFailures.kt:10-31`), passé dans `RequestCtx`.

### 3.2 Admission : file bornée et 503 `overloaded`

- `max_in_flight` (défaut : nombre de CPU **[C]**) requêtes dans le pipeline, `max_queued` (défaut
  `2 × max_in_flight` **[C]**) en attente. Une requête qui trouve la file pleine reçoit **aussitôt**
  503 `overloaded`, `Retry-After: overloaded_retry_after_s` (défaut 2 s **[C]**).
- L'attente d'un jeton est bornée par le budget : expirée en file → 504 `timeout` (le budget est
  dépassé, la file n'était pas pleine). osm-surface répondait 503 dans ce cas
  (`OS/src/trace.rs:403-409`) ; le contrat sépare les deux.
- Validation et bornes (400, 413) passent **avant** l'admission : un refus ne prend aucun jeton
  (`VC/api/.../http/ComputeGate.kt:41-45`).
- Mise en œuvre (incrément 6) : `ord-pipeline::admission::Admission` (sémaphore de `max_in_flight`
  jetons, compteur de file), appelée en tête d'`Orchestrator::run` ; le `Retry-After` du 503
  `overloaded` est posé par le handler. `max_queued = 0` est admis (aucune attente). Les jetons de
  calcul (`compute_permits`) sont un second sémaphore, partagé entre l'orchestrateur et la chaîne
  typologie (HMM et ouvrages), le permis étant déplacé dans chaque `spawn_blocking`.

### 3.3 Budget de temps et annulation

- Un `Deadline` est créé à la réception (`request_budget`, défaut **90 s**, décision doc 01 §10) :
  200 km froids ≈ 400 tuiles MNT IGN (264 pour 130 km, exploration 01 §6.3) à 20 req/s ≈ 20 s,
  plus le MNS et Mapterhorn. Il est **dimensionné large sur le cas froid** (doc 02 §7) : l'usage est
  personnel et la latence à froid secondaire. La cadence IGN reste à 20 req/s pour tout le processus
  (§6) : deux parcours froids de 200 km simultanés (≈ 40 s chacun) et le MNS partout des
  heuristiques V1 (≈ 800 tuiles, ≈ 40 s, T02 §10) tiennent sous ce budget.
- Le `Deadline` est lu par l'accès données (pas de relance ni d'attente de `Retry-After` qui
  dépasserait l'échéance : l'échec est alors enregistré) et par le cœur, **coopérativement**, via
  `&dyn Budget` aux frontières d'étape et dans les boucles longues (HMM, fusion). Leçon de
  vcyclist : sans contrôle coopératif, le délai borne la réponse mais pas le calcul, qui garde son
  permis jusqu'au bout (`VC/api/.../http/ComputeGate.kt:13-27`).
- Échéance → `PipelineError::Timeout` → 504 `timeout`, **jamais de réponse tronquée**. Client
  déconnecté : un garde `Drop` déclenche le `CancellationToken`, le calcul s'arrête au prochain
  contrôle. Mise en œuvre (incrément 6) : pas de `tokio-util` ; le garde
  `ord-pipeline::admission::CancelOnDrop`, pris en tête d'`Orchestrator::run`, appelle
  `Deadline::cancel()` quand le futur de la requête est lâché (client parti) ; l'échéance atteinte
  annule aussi le `Deadline`. Les tâches bloquantes lisent ce `Deadline` (`&dyn Budget`) ; les
  jetons d'admission et de calcul sont rendus. Les chargements de tuiles, partagés entre requêtes
  (T02 §5), ne sont pas annulés. Le délai de lecture du reverse proxy doit dépasser `request_budget` (90 s), et
  `shutdown_grace_s` vaut le budget (T07 §1). Les délais réseau par tuile (`connect_timeout_s` 4 s,
  `fetch_timeout_s` 8 s) et les relances (T02 §4) ne changent pas : seul le budget global s'allonge.

### Interfaces (runtime)

```rust
// ord-domain
pub trait Budget: Send + Sync { fn expired(&self) -> bool; fn remaining(&self) -> Duration; }
pub struct Deadline { pub at: std::time::Instant, cancelled: Arc<AtomicBool> }  // impl Budget
pub struct Unbounded;                                                          // impl Budget (tests)
// ord-pipeline relie un `CancellationToken` (tokio-util) et le garde `Drop` à `Deadline::cancel()` ;
// ord-data convertit `at` en `tokio::time::Instant` pour `timeout_at`.
// ord-pipeline
pub struct Admission { /* Semaphore + compteur de file */ }
impl Admission { pub async fn admit(&self, d: &Deadline) -> Result<AdmissionPermit, PipelineError>; }
```

## 4. Couche HTTP (composant API HTTP)

### 4.1 Routes et ports

Port public (`listen_addr`, `0.0.0.0:8080`) : `POST /v1/route-details` (validation → admission →
`Orchestrator::run` → DTO) ; `GET /v1/capabilities` et `GET /v1/attribution` (octets JSON
précalculés au démarrage, des seules sources configurées) ; `GET /health/live`, `/health/ready`.
Port interne (`metrics_addr`, `0.0.0.0:9090`, non publié) : `GET /metrics` (Prometheus) et
`GET /openapi.json` (aide au développement, hors contrat public).

Couches (de l'extérieur vers l'intérieur) : `SetRequestId`/`PropagateRequestId` (`X-Request-Id`
repris si présent et ≤ 128 car., sinon UUID v7, comme `CallId` de vcyclist,
`VC/api/.../Application.kt:68-73`) → `TraceLayer` (span par requête, sans coordonnées) →
`CatchPanic` (→ 500 `internal`) → `Compression` (gzip, br) → `RequestDecompression` →
`RequestBodyLimit(max_body_bytes)`. L'ordre décompression puis limite applique la borne au corps
**décompressé** (bombe gzip), comme osm-surface (`OS/src/api.rs:28-30`). La limite par défaut
d'axum (`DefaultBodyLimit`) est désactivée au profit de celle-ci. Les 404/405 passent par un
`fallback` et `method_not_allowed_fallback` qui rendent un problème RFC 9457.

### 4.2 Validation de l'entrée

Le corps est lu en `Bytes` par un extracteur maison (`StrictJson<T>`), pour que **toute** erreur
d'entrée soit un problème RFC 9457 et non un rejet texte d'axum :

1. `Content-Type` : `application/json` (paramètre `charset=utf-8` toléré), sinon 415.
2. JSON mal formé → 400 `invalid_input`, `details.errors[0].pointer` = position.
3. Désérialisation via `serde_ignored` : **toutes** les clés inconnues → 400 `invalid_input`,
   `details.unknown_keys = ["/options/foo", …]`, **avant tout calcul** (doc 02 §5.1). Un nombre
   entre guillemets est refusé par serde_json (pas de mode laxiste).
4. Forme du parcours : exactement un de `points` / `polyline`, sinon 400. `polyline` est décodée
   avec `polyline_precision` ∈ {5, 6} (décodeur d'osm-surface généralisé à la précision,
   `OS/src/polyline.rs:7`) ; chaîne invalide → 400. Une altitude dans `points` est ignorée (doc 02
   §3.1) : le champ `ele`/`alt` est **toléré** et non signalé comme inconnu.
5. Coordonnées : finies, `lat ∈ [-85.05112878, 85.05112878]` (plage Web Mercator), `lon ∈ [-180,
   180]` ; chaque erreur porte son pointeur (`/points/12/lat`), **jamais la valeur**.
6. Options : bornes (`sampling.step_m` ∈ [5 ; 1 000] m, `elevation.smoothing_m` ∈ {0} ∪
   [10 ; 1 000] m, T07 §2), valeurs d'enum ; `debug: true` sans `debug_allowed` en configuration → 400.
7. Bornes (après dédoublonnage des consécutifs) : moins de 2 points distincts → 400 ; plus de
   `max_points` (16 000 **[M]**, `OS/src/trace.rs:17`) ou longueur > `max_length_m` (200 000
   **[M]**, `OS/src/trace.rs:18`) → 413 `route_too_large`, `details.limit` ∈ {`points`, `length_m`,
   `body_bytes`}. Les bornes ne portent que sur l'**entrée** (doc 02 §7) ; la sortie n'est pas bornée
   en points (`distance` à 5 m sur 200 km ≈ 40 000 points). La taille densifiée interne
   `n + L / densify_step_m` (stations, §5.1) découle des deux bornes : ce n'est pas une borne 413.
8. Corps > `max_body_bytes` (8 Mio **[C]** ; 16 000 points ≈ 1 Mo) → 413, `limit = "body_bytes"`.

**Formats d'entrée.** En v1 : JSON `points` ou `polyline`, seuls formats du contrat (doc 02 §3.1).
Le GeoJSON LineString (V1.x) s'ajoutera comme une troisième variante exclusive de `RouteInput`
sans changer le reste du pipeline. Le GPX n'est pas dans le contrat (§11, points de tension).

### Interfaces (API HTTP)

```rust
// ord-domain : la requête validée, seule chose que l'orchestrateur reçoit
pub struct LatLon { pub lat: f64, pub lon: f64 }                    // Debug masqué (§8.3)
pub enum RouteInput { Points(Vec<LatLon>), Polyline { encoded: String, precision: u8 } /* V1.x: GeoJson */ }
pub struct ValidatedRequest { pub points: Vec<LatLon>, pub input_len: usize, pub options: RequestOptions }
pub struct RequestOptions {
    pub sections: SectionSet, pub sampling: Sampling, pub smoothing_m: f64,
    pub gain_preset: GainPreset, pub structure_correction: bool, pub accept_partial: bool, pub debug: bool,
}
pub enum Sampling { InputWithBreakpoints, Input, Distance { step_m: f64 } }
pub enum GainPreset { Raw, Dem, Barometric, Gps }                    // valeurs : T04 §5.4
// ord-server
pub struct StrictJson<T>(pub T);                                     // extracteur, rejet = Problem
pub fn validate(dto: RouteDetailsRequestV1, limits: &LimitsConfig) -> Result<ValidatedRequest, Problem>;
pub fn to_dto(report: &RouteReport, opts: &RequestOptions, meta: &ResponseMeta) -> RouteDetailsResponseV1;
```

### 4.3 Modèle d'erreurs RFC 9457

Corps `application/problem+json` : `type = urn:osm-route-details:problem:<code>`, `title`,
`status`, `detail` (diagnostic anglais, **sans coordonnées**), extension `code`, `details`
facultatif, `instance` = `request_id`. Calqué sur `Problem`/`ProblemKind` de vcyclist
(`VC/api/.../http/Problem.kt:17-77`), en codes `snake_case` du doc 02.

| Origine (type Rust) | HTTP | `code` | `Retry-After` |
|---|---|---|---|
| validation (`Problem::invalid_input`) | 400 | `invalid_input` | — |
| bornes, corps trop gros | 413 | `route_too_large` | — |
| `PipelineError::OutOfCoverage` | 422 | `out_of_coverage` | — |
| `PipelineError::Unavailable { source, retry_after }` | 503 | `data_unavailable`, `details.source` ∈ {`typology`, `elevation`, `both`} | toujours (§4.4) |
| `PipelineError::Overloaded` (file pleine) | 503 | `overloaded` | `overloaded_retry_after_s` |
| `PipelineError::Timeout` | 504 | `timeout` | — |
| `PipelineError::Internal`, panique, `JoinError` | 500 | `internal` (détail générique, cause dans le log) | — |
| protocole : route, méthode, média | 404 / 405 / 415 | `not_found`, `method_not_allowed`, `unsupported_media_type` | — |

Un repli absorbé (IGN → Mapterhorn) est un 200 avec avertissement (doc 01 §6). Le 429 est délégué au proxy.

### 4.4 Valeur de `Retry-After` (503 `data_unavailable`)

`max(Retry-After amont le plus grand vu dans le journal d'échecs, échéance restante du cache
négatif des tuiles en cause)`, borné à `[retry_after_min_s, retry_after_max_s]` (défauts 5 s et
120 s **[C]**) ; sans indice, `retry_after_default_s` (30 s **[C]**). Le cache négatif dure 60 s
**[M]** (`VC/elevation/.../TileFailures.kt:88`). Mise en œuvre (incrément 6) : « tuiles en cause » =
échecs des sources du côté en panne (`streets` pour la typologie, les autres pour l'élévation,
`both` : le plus grand des deux) ; chaque échec porte la fin de son cache négatif
(`TileFailure::negative_until`). Toute tuile en échec entrant au cache négatif, le défaut ne sert
qu'en l'absence d'échec journalisé. L'en-tête est en secondes entières arrondies au-dessus.

### 4.5 Santé

`/health/live` : 200 dès l'écoute. `/health/ready` : 200 si le référentiel de couverture est
chargé, chaque fichier local ouvert et lisible, le cache disque inscriptible ; les sources
distantes n'y entrent pas (§11, point 3) ; le corps donne leur état observé, sans URL.

### Interfaces (erreurs)

```rust
// ord-server
pub enum ProblemCode { InvalidInput, RouteTooLarge, OutOfCoverage, DataUnavailable, Overloaded,
                       Timeout, Internal, NotFound, MethodNotAllowed, UnsupportedMediaType }
pub struct Problem { pub code: ProblemCode, pub detail: String,
                     pub details: Option<ProblemDetails>, pub retry_after: Option<Duration> }
pub struct FieldError { pub pointer: String, pub message: String }
impl IntoResponse for Problem { /* statut, en-têtes, problem+json */ }
impl From<PipelineError> for Problem { /* seul site de traduction */ }
// ord-pipeline
pub enum PipelineError {
    Unavailable { source: DataSide, retry_after: Option<Duration> }, OutOfCoverage,
    Timeout, Overloaded, Cancelled /* client parti : rien n'est envoyé, journalisé */, Internal(String),
}
pub enum DataSide { Typology, Elevation, Both }
impl From<TypologyError> for PipelineError { /* Timeout, Cancelled, Internal → mêmes variantes */ }
impl From<ElevationError> for PipelineError { /* idem */ }
```

## 5. Pipeline d'orchestration (composant Orchestrateur)

### 5.1 Étapes de bout en bout

| # | Étape | Où | Sortie | Erreurs |
|---|---|---|---|---|
| 0 | Lecture, validation, bornes | handler | `ValidatedRequest` | 400, 413, 415 |
| 1 | Admission | `Admission` | jeton | 503 `overloaded`, 504 |
| 2 | Préparation (§5.1.1) : dédoublonnage des consécutifs, distance cumulée, stations à `densify_step_m` = 5 m **[M]** (`stepM` des détecteurs, `VC/gpx/.../DeckDetectorOptions.kt:30`) + points d'entrée | `ord-core::prepare` (bloquant) | `PreparedRoute` | — |
| 3 | Zonage : plages hors zone France, hors emprise de chaque source configurée (les plages GLO-30 viennent par point de `TerrainSample::dem_kind`, 4b) | `ord-core::coverage` + `CoverageRef` | `CoverageRanges` | 422 si aucune source ne couvre rien |
| 4a | **Typologie** : `corridor_tiles` → lecture VersaTiles → graphe + HMM → `Step::Need` (extrémités d'ouvrage hors couloir : au plus 4 tours, 32 tuiles, 20 km **[C]**, T03 §6.2) → classification | `TypologyChain` (pipeline) sur `ord-core::typology` | `TypologyOutput` | tuile en échec → `TypologyCoverage`, jamais une erreur |
| 4b | **Élévation MNT** : `sample_raw` → `TerrainReader::sample_terrain` aux stations de 5 m (IGN dans le contour LiDAR, repli Mapterhorn point par point **dans l'adaptateur**) | `ord-pipeline::elevation` | `RawProfile` (MNT, origine) | `missing = Failed` par station |
| 4c | **MNS** (`sample_dsm`, jamais de repli) : MVP, après 4a, sur l'emprise des ponts OSM et leurs sondes ; V1 (heuristiques), en parallèle de 4a sur toutes les stations du contour LiDAR | `ord-pipeline::elevation` | `RawProfile` (MNS) | `DsmSample::Unavailable` → `dsm_unavailable` |
| 5 | Décision : complet, partiel accepté, ou erreur (§5.3) | `decide_outcome` (pur) | `Outcome` | 503 |
| 6 | Fusion du profil : ouvrages OSM, détecteurs + `reconcile` (V1), trous, lissage, D+/D−, pentes | `ord-core::elevation::fuse` (bloquant) | `ElevationProfile` (dont `StructureVerdict`) | — |
| 7 | Segments homogènes (`build_segments`, verdicts de 6) | `ord-core::typology::segments` | `Vec<TypedSegment>` | — |
| 8 | Points de sortie (`sample_output` ; ruptures si `InputWithBreakpoints`) | `ord-core::elevation::breakpoints` | `Vec<OutputPoint>` | — |
| 9 | Assemblage : `ele_*`/pente des segments, résumé, `quality`, avertissements, versions, attribution (T06) | `ord-core::assemble` | `RouteReport` | — |
| 10 | DTO v1, filtrage `sections`, sérialisation | handler | JSON compressé | — |

Les étapes 6 à 9 forment **une seule** tâche bloquante (un permis). La typologie tourne même si
`sections` ne la demande pas (doc 01 §3) : seule l'étape 10 filtre.

#### 5.1.1 Préparation (`ord-core::prepare`)

1. Doublons consécutifs : deux points d'entrée consécutifs égaux à 1e-7° près (≈ 1 cm) sont
   fusionnés ; le premier garde son indice client, les suivants n'ont pas de station (doc 02 §5.3).
   Les répétitions non consécutives sont gardées.
2. Distance cumulée haversine (`ord-domain::geo`), en `f64`, sommée dans l'ordre.
3. Stations : `k × 5 m` pour `k = 0..⌊L/5⌋`, interpolées linéairement en lat/lon sur le segment
   d'entrée qui les encadre (trace d'origine, jamais la trace appariée). Chaque point d'entrée est
   aussi une station. Une station uniforme à moins de 1 cm d'un point d'entrée se confond avec lui.
   `PreparedRoute::samples` **est** la grille de T04 §2.1. Le HMM y prend une observation sur deux
   (10 m, T03 §5.1).
4. Bornes 413 (§4.2, étape 7) sur `n_dédoublonnés` et `L` seulement. La taille densifiée interne,
   `n + L/5 ≤ 56 000` stations, s'en déduit ; elle dimensionne le calcul, sans 413 propre.

### 5.2 Parallélisme

```
            ┌─ 4a typologie ── fetch ⇄ calcul (Step) ──┬─► 4c MNS ponts (MVP) ─┐
prépa ─ zonage ┤                                        │                       ├─► 5 ─ 6·7·8·9 ─ 10
            └─ 4b MNT (+ 4c MNS partout en V1) ── sample_raw ─────────────────┘
```

4a et 4b démarrent ensemble (`tokio::join!`, **pas** `try_join!` : l'échec d'une branche ne doit
pas annuler l'autre, la réponse partielle en a besoin). Au MVP, 4c attend 4a seule et chevauche la
fin de 4b, dont le cache sert déjà les tuiles MNT des culées ; en V1, le MNS est lu avec 4b (T04 §2.3).
Chemin critique : HMM à chaud, cadence IGN à froid.

### 5.3 Décision du mode de réponse

Fonction pure, testée sur la table du doc 01 §6 ligne à ligne. Elle ne décide que du **rejet** ;
le `status` est fixé par l'assembleur (T06 §3).

- Une source est `Failed` si **une** plage est en panne après relances, cache périmé et repli :
  typologie, `TypologyCoverage::unavailable ≠ ∅` ; élévation, une station `missing = Failed`.
  `Fallback` (IGN relu dans Mapterhorn) et `Missing` (hors emprise) ne rejettent jamais.
- Les deux `Failed` → 503 `data_unavailable`, `source = both`, même avec `accept_partial`. Une
  seule `Failed` → 503 sans `accept_partial`, sinon `Proceed`, et l'assembleur pose
  `typology_unavailable` ou `elevation_unavailable` sur les plages concernées.
- Règle **stricte**, confirmée (doc 01 §10) : il n'y a **aucun seuil de tolérance**. Une seule
  tuile VersaTiles ou MNT en panne non rattrapée par un repli suffit pour un 503 sur tout le
  parcours, sauf `accept_partial` ; les deux côtés en panne, même sur des plages disjointes, donnent
  toujours un 503.
- 422 : décidé dès l'étape 3, quand `typology_absent` et `elevation_absent` couvrent chacun `[0, L]`.

```rust
pub enum Outcome { Proceed { typology_failed: bool, elevation_failed: bool }, Reject(PipelineError) }
```

### 5.4 Assembleur et `/v1/capabilities` (renvois)

L'assembleur (`ord-core::assemble`, étape 9 : calcul de chaque champ de `summary` et `quality`,
`status`, producteur de chaque avertissement) est décrit en [T06](06-assembleur.md).
`/v1/capabilities` (construction au démarrage, bornes publiées) est décrit en
[T07 §4](07-configuration.md).

### Interfaces (Orchestrateur)

```rust
// ord-domain
pub enum Step<T> { Done(T), Need(TilePlan) }
pub struct TilePlan { pub requests: Vec<(SourceId, TileKey)> }          // triées, sans doublon
pub struct RequestCtx { pub request_id: RequestId, pub deadline: Deadline, log: Mutex<DataLog> } // T02 §2.3
pub struct RouteSample { pub at: LatLon, pub d_m: f64,
                         pub input_index: Option<u32> /* indice client si point d'entrée */, pub prev_input: u32 }
pub struct PreparedRoute { pub samples: Vec<RouteSample>, pub input_pos: Vec<u32> /* rang dans samples */,
                           pub length_m: f64 }                           // Debug masqué
pub struct CoverageRanges { pub outside_primary_zone: Vec<Range<f64>>,
                            pub typology_absent: Vec<Range<f64>>, pub elevation_absent: Vec<Range<f64>> }
pub enum SourceOutcome<T> { Ok(T), Fallback(T), Missing(T), Failed { retry_after: Option<Duration> } }
pub enum WarningCode { UnmatchedRange, StructureUncorrected, StructureSlopeImplausible, ElevationGapFilled,
    ElevationMissing, TypologyMissing, TerrainFallback, DsmUnavailable, DsmSource, OutsidePrimaryZone,
    TypologyUnavailable, ElevationUnavailable, StructureUntagged }       // = doc 02 §4.5, snake_case
pub struct Warning { pub code: WarningCode, pub message: String, pub range: Option<Range<f64>> }
// RouteReport, Status, Summary, Quality, assemble : ord-core::assemble, T06 §5
// ord-pipeline
pub struct Orchestrator { /* Arc<Sources>, Arc<dyn TypologyChain>, Arc<Rules>, Admission, Semaphore */ }
impl Orchestrator {
    pub async fn run(&self, req: ValidatedRequest, ctx: RequestCtx) -> Result<RouteReport, PipelineError>;
}
pub struct Sources { pub streets: Arc<dyn TileStore<StreetTile>>, pub terrain: Arc<dyn TerrainReader>,
                     pub coverage: Arc<dyn CoverageRef>, pub catalog: Arc<dyn DataCatalog> }
pub fn decide_outcome(t: &SourceOutcome<()>, e: &SourceOutcome<()>, accept_partial: bool) -> Outcome;
```

## 6. Accès données : ce que l'architecture impose

Le détail relève de la section Accès données ; l'architecture fixe le port et les règles de processus :
- **Un port** `TileSource` (octets bruts) par support, dans `ord-data` ; au-dessus, `TileReader<C>`
  (LRU mémoire `moka` pondéré en octets + `TileCodec` d'`ord-adapters`), dédupliquant les chargements
  concurrents, vu par le pipeline comme `Arc<dyn TileStore<T>>`. Défauts : LRU élévation 512 Mo et
  LRU typologie 512 Mo **[C]** (T02 §5.4 ; osm-surface : 256 Mo, `OS/src/config.rs:22-24`).
- Résultat de lecture à quatre états **distincts** (`TileOutcome`) : `Data`, `Empty` (204 / tuile
  vide : « pas de rue »), `Absent` (hors emprise par construction), `Failed` (jamais lu comme « pas
  de rue », `OS/src/tiles.rs:207-212`).
- **Cadence par hôte pour tout le processus**, donc pour tout le service, déployé en une seule
  instance (§10) : IGN 20 req/s **[M]** (moitié des 40 publiées,
  `VC/elevation/.../TileRequestPacing.kt:24-63`), MNT et MNS sur le **même** budget ; relance des
  429/503 jusqu'à 6 essais après `Retry-After` ou 1, 2, 4… s **[M]**, sans dépasser le `Deadline`.
  VersaTiles : concurrence ≤ 8, backoff sur 429/5xx **[M]** (exploration 06). Mapterhorn :
  concurrence 4 **[C]** (« peu de requêtes simultanées », exploration 05).
- Chaque lecture alimente le `DataLog` **de la requête** (`RequestCtx`) : échecs, tuiles périmées
  servies, date de lecture pour `data_versions`, compteurs de cache pour les logs et métriques.

### Interfaces (consommées de la section Accès données)

`TileKey`, `SourceId`, `TileOutcome<T>`, `TileCodec`, `TileStore<T>`, `TileFailure` : définitions
uniques en T02 §2.2–2.3 (non recopiées ici pour éviter deux versions).

## 7. Configuration (renvoi)

Sources et priorité (défauts < TOML < `ORD_<SECTION>__<CLÉ>`), table des clés et défauts, bornes
des options, `User-Agent`, `/v1/capabilities` et `Config` : voir [T07](07-configuration.md).

## 8. Observabilité

**Logs** (`tracing`, JSON en production, `OS/src/main.rs:10-18`) : un span `request` (`request_id`,
route, statut, `code`, `duration_ms`), un span par étape du §5.1, et une ligne de synthèse par
requête sur le modèle d'osm-surface (`OS/src/api.rs:57-79` : points, tuiles, segments, `tiles_ms`,
`match_ms`, `total_ms`), complétée des tuiles par source et par issue, des avertissements par code,
du `status` et des méthodes appliquées aux ouvrages. 4xx en `info`, 5xx et replis subis en `warn`.

**Métriques** (préfixe `ord_`, port interne). Techniques : `http_requests_total` et
`http_request_duration_seconds{route, status, code}` ; `inflight`, `queued`,
`rejected_total{reason}` ; `stage_duration_seconds{stage}` ; `tile_reads_total{source, outcome = mem
| disk | fetched | stale | empty | not_found | failed | negative}` ; `upstream_responses_total{host,
status}`, `upstream_retries_total`, `pacer_wait_seconds` ; `cache_bytes{tier, source}`. Produit,
étiquetées par versions (doc 01 §8.4) : `responses_total{status, rules_version, typology_version}` ;
histogrammes `unknown_ratio`, `inferred_surface_ratio`, `elevation_coverage` ;
`structures_total{kind, method, structure_source}` ; `warnings_total{code}`. Seules les versions à
faible cardinalité sont des étiquettes, jamais la date de lecture IGN.

**Vie privée : jamais de coordonnées.** Une **clé de tuile est une coordonnée** (z14 ≈ 2,4 km) et
une URL WMS contient une emprise : ni l'une ni l'autre n'est journalisée, à aucun niveau.
`LatLon`, `TileKey`, `PreparedRoute`, `ValidatedRequest` implémentent `Debug` à la main
(`PreparedRoute { samples: 51234 }`) : un `?valeur` dans un log ne fuit rien. `TileFailure` garde
source, hôte, statut et `Retry-After`, jamais l'URL (même choix chez vcyclist,
`VC/api/.../StatusPagesConfig.kt:24-25`). Les erreurs client donnent un pointeur, jamais la valeur.
Le corps n'est jamais journalisé. Test : un parcours sentinelle traverse le pipeline sous un
collecteur de logs ; aucune ligne ne doit contenir ses chiffres. Mise en œuvre (incrément 6) :
`crates/ord-server/tests/observability_sentinel.rs`, logs JSON avec spans (`trace` pour les crates
du service, `debug` pour les dépendances) et exporteur Prometheus ; parcours qui traverse un pont,
un repli IGN → Mapterhorn, une réponse partielle, un 503 et un 400 ; fragments interdits : chaque
coordonnée de 2 à 9 décimales (et sa partie décimale seule dès 5), les tuiles `z/x/y` aux zooms 12,
14 et 15, les bords d'emprise WMS à 4 décimales.

## 9. Versions, déterminisme, idempotence

- `api_version` : constante `API_VERSION` (`ord-server`, "1.4" depuis le 2026-11-02), montée **à la main** quand le
  contrat change de façon observable, jamais la version du paquet (`VC/api/.../ApiVersion.kt:5-12`).
- `rules_version` : constante `RULES_VERSION` (`AAAA.MM.n`, doc 02 §4.6) dans `ord-core`. Un test
  calcule l'empreinte de la table de classification et des paramètres de fusion, de D+, de ruptures
  et des heuristiques ; une modification sans montée de `RULES_VERSION` casse la CI.
- `data_versions` : `data_version()` de chaque source **utilisée**. VersaTiles : date OSM du
  `tiles.json` lu au démarrage en tâche de fond (`OS/src/main.rs:23-26`, `OS/src/trace.rs:335-371`)
  ou en-tête `.versatiles`. Mapterhorn : version, zoom, rôle `fallback`. IGN : produits, zoom et
  `read_at` = date UTC de la plus ancienne lecture parmi les tuiles de la requête (`DataLog`).
- `Rules` (`ord-core::rules`) réunit `RuleSet` (classification, réconciliation, T03 §7),
  `MatchParams` (HMM, T03 §5.2) et `ElevationRules` (fusion, détecteurs, D+, ruptures, T04) ; tous
  `Serialize`, c'est l'objet haché par le test d'empreinte et publié dans `/v1/capabilities`.

**Déterminisme** (doc 01 §3) : aucune itération de `HashMap` vers une sortie (`BTreeMap` ou tri) ;
aucune réduction flottante parallèle, sommation dans l'ordre des distances ; `TilePlan` triés, donc
indifférents à l'ordre d'arrivée des tuiles ; arrondis de sortie fixés dans l'assembleur et
versionnés (altitudes et distances au cm, pentes à 0,01 %, coordonnées à 1e-6° **[C]**), ce qui
absorbe les écarts flottants entre machines (T05 §10, point 2) ; `request_id` et `duration_ms` ajoutés par le handler, hors `RouteReport`. Seules
causes de variation : externes et signalées (repli après panne, tuile IGN nouvellement publiée).
Test : chaque parcours d'`ord-testkit` rejoué deux fois sur tuiles figées donne des réponses égales,
ces deux champs retirés. **Idempotence** : aucun effet de bord observable (les caches chauffent
sans changer le résultat), aucun cache de réponses en v1 (doc 01 §8.3).

### Interfaces (versions)

```rust
// ord-domain
pub enum SourceRole { Primary, Fallback, Coverage }
pub struct DataVersion { pub source: &'static str, pub id: Option<String>, pub date: Option<String>,
                         pub zoom: Option<u8>, pub products: Vec<&'static str>, pub role: SourceRole,
                         pub indicative: bool /* lu en ligne, doc 01 §8.3 */ }
pub struct DataVersions { pub typology: Option<DataVersion>, pub elevation: Vec<DataVersion> } // doc 02 §4.6
pub struct Versions { pub api_version: &'static str, pub rules_version: &'static str, pub data_versions: DataVersions }
// ord-core::rules
pub const RULES_VERSION: &str;
pub struct Rules { pub classification: RuleSet, pub matching: MatchParams, pub elevation: ElevationRules }
```

## 10. Performance, démarrage, packaging

**Complexité** : préparation, zonage, échantillonnage, fusion, ruptures et assemblage sont O(n)
sur n stations (≤ 16 000 + 200 km / 5 m ≈ 56 000), soit quelques Mo par requête.
L'appariement HMM est O(n × k), k candidats par point (osm-surface : < 100 ms à chaud pour ses
parcours, doc 01 §5.2). À froid, la cadence IGN domine (≈ 19 s pour 130 km, exploration 01 §6.3).
Objectif à chaud, à mesurer au MVP : < 300 ms pour 100 km **[C]**.

**Démarrage** : configuration → tracing → client HTTP → fichiers locaux → référentiel de
couverture → caches → `Orchestrator` → précalcul de `/v1/capabilities` et `/v1/attribution` →
tâches de fond (`tiles.json` ; balayage du cache disque au démarrage puis toutes les 6 h,
`OS/src/main.rs:50-69`) → écoute des deux ports. **Arrêt** sur SIGTERM : plus d'admission,
requêtes en vol attendues jusqu'à `shutdown_grace_s` (`OS/src/main.rs:30-46`).

**Docker** (modèle `OS/Dockerfile`) : build `rust:<version>-slim-bookworm` épinglé, `cmake clang`
pour aws-lc, couche de dépendances séparée, `cargo build --release --locked -p ord-server` ; image
`gcr.io/distroless/cc-debian12:nonroot` (65532), binaire unique ; `/data` en **lecture seule**
(`.versatiles`, couverture), `/var/cache/osm-route-details` **inscriptible** à part (doc 01 §8.1) ;
`EXPOSE 8080` et `9090` (non publié) ; sonde `osm-route-details healthcheck` (pas de `curl` en
distroless). Les fichiers de couverture Mapterhorn viennent de `tools/fetch-coverage` et sont
montés, pas intégrés à l'image : ils suivent les publications de Mapterhorn. Le contour LiDAR HD (et
zone France) est embarqué dans le binaire (T02 §7.1–7.2). Mise en œuvre (incrément 6) : `Dockerfile`
et `.dockerignore` à la racine ; la crypto de rustls est `ring` (pas `aws-lc`), donc ni `cmake` ni
`clang` ; le contexte exclut `bench/` (traces privées), `.env*`, `docs/` et les tests ; couche de
dépendances construite sur des sources vides des membres du workspace ; `HEALTHCHECK` par la
sous-commande `healthcheck` (HTTP/1.0 sur `127.0.0.1`, port de `server.listen_addr`) ; à l'arrêt,
les requêtes en vol sont attendues au plus `shutdown_grace_s`, puis abandonnées (`docker stop -t`
supérieur au budget). Aucune image n'est poussée par le dépôt.

**Déploiement : une seule instance** (décision doc 01 §10). La cadence IGN (20 req/s) et les budgets
par hôte sont locaux au processus (§6, T02 §4.2) : deux réplicas derrière une même IP doubleraient
la cadence vue par l'IGN. La v1 tourne donc en un seul conteneur ; la mise à l'échelle horizontale
(cadence partagée ou divisée par réplica) est hors v1 (T07 §3).

**OpenAPI** : `utoipa` dérive le contrat des DTO ; `osm-route-details openapi` écrit
`contract/openapi.json`, et un test vérifie que le fichier versionné est celui du code, comme
`OpenApiContractTest` de vcyclist, problèmes déclarés par opération
(`VC/api/.../OpenApiContract.kt:85-97`). Tests HTTP : routeur sur faux ports, via
`tower::ServiceExt::oneshot` (`OS/tests/api.rs:30-46`), sans réseau.

## 11. Points de tension

1. **GPX et GeoJSON en entrée.** La consigne cite GPX et GeoJSON ; le contrat v1 n'accepte que
   `points` et `polyline` (GeoJSON en V1.x, GPX absent, doc 02 §3.1). Je garde le contrat ;
   `RouteInput` est extensible. Le GPX demanderait de trancher sa forme (média, une trace, `rtept`/`trkpt`).
2. **`read_at` IGN** rend `data_versions` variable d'un jour à l'autre sans changement de contenu :
   conforme au caractère indicatif (doc 02 §6), mais défavorable au cache client.
3. **`/health/ready` et sources distantes.** J'exclus les pannes amont de la disponibilité, sinon le
   service serait retiré du répartiteur au lieu de répondre ses 503 `Retry-After` et ses replis.
4. **Mémoire sous concurrence.** Les `Arc` de tuiles tenus par une requête échappent à la borne du
   LRU : jusqu'à ≈ 100 Mo de tuiles IGN par parcours froid de 200 km. Il faudra un plafond par
   requête ou un échantillonnage qui relâche les tuiles au fil de l'eau.
5. **Annulation coopérative.** Le cœur lit une horloge monotone via `&dyn Budget` : seule entorse au
   « cœur sans effet », sans incidence sur le résultat.
