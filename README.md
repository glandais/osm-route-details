# osm-route-details

Microservice Rust : en entrée un parcours, en sortie l'**élévation** et la **typologie**
(structure : sol / pont / tunnel ; revêtement : enum simple) le long du parcours.

Code : MVP et V1 réalisés, règles `2026.11.12`, API `1.4` (contrat
[`contract/openapi.json`](contract/openapi.json)). `POST /v1/route-details` rend le profil
d'élévation (IGN LiDAR HD en France, repli Mapterhorn point par point, GLO-30 ailleurs), la
typologie appariée sur les tuiles VersaTiles shortbread@1.1 (classe, revêtement, ouvrages), les
corrections d'ouvrage (tablier MNS, tunnels, ponts qui enjambent la trace), les heuristiques MNT/MNS
pour les ouvrages absents d'OSM et les points de rupture (mode par défaut
`input_with_breakpoints`, aussi `input` et `distance`). Autres routes : `/health/live`,
`/health/ready`, `/v1/capabilities`, `/v1/attribution` ; erreurs RFC 9457. Workspace `crates/`
(`ord-domain`, `ord-core`, `ord-adapters`, `ord-data`, `ord-pipeline`, `ord-server`,
`ord-testkit`) ; image : [`Dockerfile`](Dockerfile). Plan et décisions des incréments :
[technique/README §3](docs/design/technique/README.md). `cargo test` vérifie tout, contrat compris,
sans réseau ; `cargo run -p ord-server -- openapi` régénère le contrat.

Image : `ghcr.io/glandais/osm-route-details`, publiée par
[`docker.yml`](.github/workflows/docker.yml) (linux/amd64 et linux/arm64) : `develop` et `latest` à
chaque push sur `develop`, `1.2.3` et `1.2` pour un tag `v1.2.3`, `sha-<commit>` dans tous les cas.
Rien n'est publié si `cargo test` ou `cargo clippy` échoue ; sur une pull request, l'image est
seulement construite.

Banc (`tools/ord-bench`, T05 §5, règles dans [`bench/README.md`](bench/README.md)) :
`ord-bench check` (empreintes du manifest, Q1), `ord-bench fill-cache` (cache de banc local,
`~/.cache/osm-route-details/bench`), `ord-bench run --offline` (campagne de calibration,
`results.json` dans `bench/reports/<date>-<rules_version>[-<révision>]/`), `gel` et `judge`
(jugement unique d'une série tenue à l'écart), plus des outils de diagnostic (`compare`,
`diagnose`, `dump-profils`…). Jugements : MVP, V1, V1.1, V1.2 et V1.3, tous refusés
([`bench/judgements.md`](bench/judgements.md)). Le vivier de traces inédites étant épuisé, V1.3
est le dernier jugement (décision de l'utilisateur, 2026-11-05, bench §4 octies) : la suite se
mesure sur la calibration seule.

## Conception

| Doc | Sujet |
|-----|-------|
| [01-conception-generale](docs/design/01-conception-generale.md) | Vision, périmètre, composants, stratégie de données, enums, forme de sortie, modes dégradés, trajectoire, exploitation, décisions, vérifications à faire |
| [02-contrat-interface](docs/design/02-contrat-interface.md) | Contrat d'API grosse maille : endpoints, entrée, sortie (points/segments/résumé/qualité/attribution), erreurs, versionnage |
| [technique/](docs/design/technique/README.md) | Conception technique : architecture (crates, pipeline, erreurs), accès données et caches, typologie (graphe, HMM, règles), élévation (ouvrages, D+, points de rupture), qualité et tests, plan MVP en incréments |

## Exploration

| Doc | Sujet |
|-----|-------|
| [01-vcyclist-elevation](docs/exploration/01-vcyclist-elevation.md) | vcyclist 6.11 : IGN LiDAR HD + repli Mapterhorn, détection ponts (MNS) et tunnels, provenance, D+ |
| [02-osm-surface-typologie](docs/exploration/02-osm-surface-typologie.md) | osm-surface : tuiles VersaTiles, map-matching HMM tiles-only, attributs |
| [03-sources-tuiles-vectorielles](docs/exploration/03-sources-tuiles-vectorielles.md) | VersaTiles, OSMF, OpenFreeMap, Protomaps, Planetiler sur mesure |
| [04-sources-elevation](docs/exploration/04-sources-elevation.md) | Mapterhorn, IGN (LiDAR HD, RGE ALTI, API alti), MNT nationaux, globaux |
| [05-mapterhorn-emprises-et-usage](docs/exploration/05-mapterhorn-emprises-et-usage.md) | Emprises des sources Mapterhorn (détection GLO-30), conditions d'usage de l'API |
| [06-versatiles-conditions-usage](docs/exploration/06-versatiles-conditions-usage.md) | Conditions d'usage de l'API publique VersaTiles, extrait France local |

### Synthèse

- **Positionnement** : projet indépendant de vcyclist (aucun n'appelle l'autre, seuls les
  enseignements sont partagés). Usage personnel d'abord, faible volume. France d'abord ;
  ailleurs, mode dégradé signalé.
- **Élévation** : en France, IGN LiDAR HD en direct (MNT par la Géoplateforme WMS-R, BIL
  float32, z15), comme vcyclist 6.11 ; Mapterhorn (Terrarium, qui sert d'ailleurs du LiDAR HD
  en France jusqu'à z17) en repli point par point et partout ailleurs, avec GLO-30 (MNS) comme
  fond mondial : seuil de D+ adapté automatiquement sur ces plages, paramètres effectifs
  renvoyés. Provenance du terrain exposée par tronçon. Référentiel vertical ignoré (écart jugé
  négligeable pour le vélo et la rando). Dépendance au WMS-R (40 req/s/IP, pas de SLA)
  acceptée : cache disque et repli Mapterhorn.
- **Ponts/tunnels** : un MNT donne le sol sous le pont / le relief au-dessus du tunnel.
  vcyclist les corrige déjà par heuristiques sur le LiDAR seul (MNS pour les tabliers, forme du
  MNT pour les tunnels), mais en rate beaucoup (hors échantillon : 4 ponts sur 11). Ici : OSM
  pour l'emprise et le type, LiDAR pour l'altitude (MNS sur le tablier dès le MVP, droite entre
  portails pour les tunnels) ; l'interpolation entre culées (GraphHopper, Valhalla) n'est qu'un
  repli, 22–40 m d'erreur sur les grands ponts bombés. Heuristiques portées librement en Rust
  (V1), en contrôle croisé et pour corriger les ouvrages absents d'OSM ; écart avec vcyclist
  assumé.
- **Sortie** : par défaut, les points d'entrée plus des points de rupture insérés (sommet,
  creux, rupture de pente, extrémité d'ouvrage, changement de revêtement ou de source de
  terrain), chacun avec son origine et ses raisons. Points d'entrée seuls ou pas fixe en
  option. 200 km par appel synchrone, 413 au-delà ; 503 + `Retry-After` en cas de panne, sauf
  `accept_partial`.
- **Typologie** : aucune source de tuiles publique ne fournit `surface` brut
  (VersaTiles/OpenFreeMap : paved/unpaved ; OSMF et Protomaps : rien). Pont/tunnel disponibles
  partout ; gué et `layer` seulement dans OpenMapTiles. v1 : revêtu / non revêtu (VersaTiles,
  API publique avec cache, fichier local en production). Revêtement fin et gué en V2, avec des
  tuiles maison (Planetiler, schéma dédié). 25–60 % des ways sans `surface` → inférence par
  `highway`/`tracktype` nécessaire.
- **Qualité** : bancs de vcyclist, corpus d'ouvrages et traces barométriques personnelles
  d'autres régions.
- **Map-matching** : le HMM tiles-only d'osm-surface est réutilisable ; les niveaux
  sol/pont/tunnel déconnectés évitent d'apparier la route sous un pont.
- **Licences** : ODbL (OSM), IGN LiDAR HD (Licence Ouverte Etalab 2.0), ~150 attributions
  Mapterhorn, FABDEM exclu (NC).
