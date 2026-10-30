# 02 — Étude de `osm-surface` (typologie des voies depuis les tuiles)

Étude en lecture seule de `/Volumes/Data/code/perso/osm-surface` (état au commit `44e5da6`,
27/10/2026). Les références `chemin:ligne` sont relatives à ce dépôt. But : en tirer ce qui
sert à la partie « typologie » (surface, pont, tunnel…) d'`osm-route-details`, sans viser la
compatibilité `trace_attributes` de Valhalla.

## 1. Vue d'ensemble

`osm-surface` remplace l'endpoint `trace_attributes` de Stadia Maps/Valhalla pour le backend
`tribly` (Pédalons) (`README.md:3-6`). Il ne lit qu'une source, les tuiles vectorielles
**VersaTiles Shortbread 1.0** au zoom 14. Il n'a ni graphe Valhalla ni extrait OSM
(`README.md:8-10`). Chaîne de traitement (`src/trace.rs:380-437`) :

1. décodage de la trace (polyline6 ou `shape`) ;
2. chargement des tuiles d'un couloir de 100 m autour de la trace ;
3. construction d'un graphe de rues ;
4. map-matching HMM/Viterbi ;
5. regroupement en « arêtes » (suites de segments de même classe), puis réponse JSON.

Historique (`git log`) : 3 commits en une nuit. `d35065f` initial (README vide) ;
`b434a89` le service complet (≈ 10 700 lignes), avec un **index OSM PBF facultatif**
(`src/osm.rs`, binaire `osm-index`, dépendance `osmpbf`) ;
`44e5da6` « Remove the optional OSM index, keep tiles-only matching ». Il retire
  2 014 lignes (`src/osm.rs` 812 lignes, `src/bin/osm-index.rs`, la fixture `.bin`) ; voir §5.

## 2. Architecture

### API HTTP

- Framework : **axum 0.8** + tower-http (gzip/brotli, décompression des requêtes, limite de
  16 Mo) (`src/api.rs:23-32`).
- Endpoints : `POST /trace_attributes/v1` (chemin Stadia), `POST /trace_attributes`,
  `GET /health` (version + date OSM des tuiles, lue dans `tiles.json`) (`src/api.rs:24-41`,
  `src/config.rs:82-85`, `src/trace.rs:335-377`).
- Entrée (`src/trace.rs:43-55`) : `encoded_polyline` | `shape[{lat,lon}]`, `costing`,
  `shape_match`, `filters`, `trace_options.search_radius`, et `debug` (champ propre au
  service : renvoie les attributs Shortbread de chaque arête sous `osm`).
- Sortie (`src/trace.rs:57-113`) : `shape` (polyline6 du tracé apparié) ; `edges[]` avec
  `surface`, `use`, `cycle_lane`, `road_class`, `unpaved`, `bridge`, `tunnel`, `length` (km),
  `begin/end_shape_index` ; `matched_points[]` (type, edge_index, fraction le long de l'arête,
  distance au point d'origine).
- Bornes : 16 000 points, 200 km (`src/trace.rs:17-18`). Budget de 13 s pour obtenir un créneau
  d'appariement (`src/trace.rs:20`). Calcul CPU dans `spawn_blocking`, limité par un sémaphore
  (`src/trace.rs:400-426`).
- Erreurs au format Valhalla (100, 123-126, 153, 154, 171, 444). Tuiles indisponibles → **503**
  (transitoire), jamais une tuile vide (`README.md:65-85`).

### Modules (`src/lib.rs:4-14`)

| Module | Rôle |
|---|---|
| `tiles.rs` | Fetch HTTP borné, cache disque avec TTL, LRU mémoire (moka) |
| `mvt.rs` | Décodage MVT écrit à la main (prost), couche `streets` seule, découpe au carré de la tuile |
| `network.rs` | Graphe : nœuds retrouvés par les coordonnées, recollage, R-tree (rstar) |
| `matcher.rs` | HMM/Viterbi façon meili, Dijkstra borné |
| `valhalla.rs` | Dérivation surface / use / road_class / accès « à la Valhalla » |
| `trace.rs` | Requête/réponse, orchestration, construction des arêtes |
| `tribly.rs` | Portage de la projection tribly (eval + tests) |
| `bin/eval.rs`, `bin/inspect.rs` | Évaluation contre Stadia ; outil de diagnostic |

### Crates (`Cargo.toml:6-25`)

axum 0.8.9, tokio 1.54.1, tower-http 0.7.1, reqwest 0.13.5 (rustls, gzip, brotli, http2),
moka 0.12.16 (future), prost 0.14.4, rstar 0.13.0, flate2 1.1.10, serde 1.0.229 /
serde_json 1.0.154, thiserror 2.0.21, anyhow 1.0.104, tracing 0.1.44 /
tracing-subscriber 0.3.23. Le service n'utilise ni crate MVT, ni crate geo, ni crate
osmpbf : MVT, haversine, Mercator et clipping sont codés à la main (`src/mvt.rs:1-66`,
`src/geo.rs`).

### Déploiement (`Dockerfile:1-36`)

Build multi-étapes avec `rust:1.99.1-slim-bookworm` épinglé, plus cmake et clang pour
`aws-lc-sys` (rustls). Les dépendances sont compilées dans une couche à part grâce à un
`main.rs` factice. Image d'exécution `distroless/cc-debian12:nonroot`, cache dans le volume
`/data`, port 8002. Toute la configuration passe par des variables d'environnement
(`README.md:143-160`).

## 3. Source de tuiles

- **Schéma** : Shortbread 1.0. **Serveur** : `https://tiles.versatiles.org/tiles/osm/{z}/{x}/{y}`
  (`src/config.rs:49`), en HTTP tuile par tuile. Le fichier `.versatiles` et PMTiles ne sont pas
  utilisés, mais le README propose de servir soi-même le `.versatiles` (`README.md:310-312`).
- **Zoom 14** (le zoom maximal de VersaTiles), extent 4096 (`README.md:147`,
  `src/mvt.rs:183`). Au 47°N, 1 unité de tuile ≈ 0,41 m. Les géométries sont donc **quantifiées
  et simplifiées** : des sommets de jonction disparaissent.
- **Sélection** : les tuiles qui touchent un couloir de `search_radius + 60` m = 100 m autour de
  la trace (`src/trace.rs:387-388`, `src/geo.rs:93-116`), soit ≈ 40 tuiles pour 90 km.
- **Cache** (`src/tiles.rs:100-165`) :
  - disque : tuiles brutes, TTL 7 j ; une tuile périmée est réutilisée si le serveur ne répond
    pas ; suppression à 2 × TTL ;
  - espace de nommage = slug + hash de `TILES_URL` (`src/tiles.rs:254-256`) ;
  - écriture atomique, et seulement après un décodage réussi ;
  - LRU mémoire de 256 Mo des couches `streets` décodées ;
  - 8 fetchs concurrents, un seul nouvel essai (300 ms) sur 5xx ou 429 (`src/tiles.rs:189-235`) ;
  - 204 = tuile vide ; 404 = erreur (« jamais "pas de rue" ») ;
  - mode `TILES_OFFLINE` ; nettoyage au démarrage puis toutes les 6 h (`src/main.rs:50-69`).
- **Fraîcheur et volume** : OSM du 2026-06-07 (`eval/report.md:5`), plusieurs mois de retard,
  ≈ 14 % des écarts (§7) ; ≈ 11 Mo de cache pour 185 km (`README.md:165, 176-177`).

## 4. Attributs OSM disponibles dans les tuiles

### Ce que la couche `streets` contient réellement

Clés lues (`src/mvt.rs:99-115, 215-241`) : `kind`, `surface`, `tracktype`, `service`, `bicycle`,
`horse`, `link`, `rail`, `bridge`, `tunnel`, `oneway`, `oneway_reverse`. J'ai vérifié une tuile
du cache (`14/8118/5742.mvt`, chaînes du protobuf) : ce sont exactement ces clés, et il n'y a
**ni `layer`, ni `ford`, ni `smoothness`**. La tuile contient aussi les couches
`street_labels` (noms), `water_lines`, `water_polygons`, `land`, `sites`, `pois`, `buildings`,
`addresses`, `public_transport`, `place_labels`.

Statistiques réelles sur les 95 tuiles des 3 parcours (`inspect stats`, hors ligne, depuis le
cache) :

- `kind` observés : footway (10 027 lignes), service (8 095), residential (6 382),
  path (3 947), track (1 828), tertiary, steps (838), unclassified, secondary, cycleway (624),
  pedestrian, rail, tram, living_street, trunk, primary, motorway, busway. **Pas de `bridleway`**,
  et aucun `sidewalk` ni `crossing`, même si le code les accepte (`src/valhalla.rs:255-279`).
- `surface` : uniquement `paved`, `unpaved` ou absente. Exemples : footway 5 396 sans surface,
  3 800 paved, 822 unpaved ; path 1 935 / 785 / 1 174 ; track 1 477 sans surface ni tracktype.
- `tracktype` : rare (≈ 200 lignes sur 1 828 tracks, grade1 à grade4 observés).
- `service` : valeurs brutes OSM, y compris des valeurs fantaisistes, sans normalisation.

### Disponible et exploité

| Attribut | Usage dans osm-surface |
|---|---|
| `kind` (≈ highway) | Classe de route, `use`, accès par défaut, surface par défaut |
| `surface` (paved/unpaved) | Surface (1er critère) |
| `tracktype` | Surface si pas de `surface` ; sert d'indice pour départager les `unpaved` |
| `bicycle` | Accès vélo (`designated` → « voie verte » → compacted) |
| `service` | `use` (driveway, alley, parking_aisle…) |
| `link`, `rail` | Bretelles ; exclusion du rail et du tram |
| `bridge`, `tunnel` | Niveau du graphe (sol/pont/tunnel) et flags de sortie (`src/trace.rs:607-608`) |
| `oneway*`, `horse` | Lus mais ignorés (une trace planifiée est supposée légale) |

### Absent ou généralisé

- `surface` est **réduit à paved/unpaved** par Shortbread : on perd asphalt/gravel/
  compacted/ground/grass/sett/cobblestone. Une valeur que Shortbread ne classe pas (ex. `wood`)
  devient nulle (`README.md:296-298`).
- `smoothness`, `mtb:scale`, `sac_scale`, `access`, `foot`, `vehicle`, `cycleway:*`,
  `segregated`, `footway=sidewalk` : absents (`src/valhalla.rs:5-8`, `README.md:292-298`).
- `highway=bridleway` : absent. Ces passages sortent `unmatched` (`README.md:299-300`).
- `layer` : absent. On ne connaît que 3 niveaux (sol / pont / tunnel), pas l'empilement.
- `ford` : absent. Aucune détection de gué n'est possible avec `streets` seule.
- Pas d'identifiant de way ni de nœud OSM : la topologie est reconstruite par la géométrie.

## 5. Map-matching tuiles seules

### Construction du graphe (`src/network.rs`)

- Lignes `streets` routables pour le mode (`is_routable_kind` + accès dérivé), découpées en
  segments. On ne garde que les segments du couloir (cellules de 128 unités)
  (`src/network.rs:54-105, 116-160`).
- Nœuds = coordonnées quantifiées au 1/16 d'unité (`src/network.rs:137-143`).
- **Bords de tuile** : chaque ligne est d'abord découpée au carré exact de sa tuile, avec un calcul
  du point de coupe dans un ordre canonique des extrémités. Les deux tuiles voisines calculent ainsi
  le même point (`src/mvt.rs:288-330`). Ensuite, les extrémités pendantes (degré 1) à moins de
  `MERGE_UNITS` = 2 unités (≈ 0,8 m) sont fusionnées par union-find (`src/network.rs:18-19,
  259-322`).
- **Jonctions perdues à la simplification** : un sommet situé à moins de `SPLICE_UNITS` = 1,5 unité
  (≈ 0,6 m) de l'intérieur d'un segment **du même niveau** est inséré dans ce segment
  (`src/network.rs:20-21, 324-387`).
- **Ponts et tunnels** : `layer()` vaut 0 au sol, 1 sur un pont, 2 dans un tunnel. Le raccord
  n'a lieu qu'entre niveaux identiques : un pont ne se connecte pas à la route qu'il enjambe
  (`src/network.rs:235-244`). Réserve : la fusion des extrémités pendantes (étape 1) ne filtre
  pas le niveau, seule l'insertion (étape 2) le fait.

### HMM / Viterbi (`src/matcher.rs`)

Paramètres par défaut (`src/matcher.rs:39-52`), tous surchargeables par `MATCH_*` :

| Paramètre | Valeur | Rôle |
|---|---|---|
| `step_m` | 10 m | densification de la trace (un tracé planifié est droit entre ses points) |
| `search_radius_m` | 40 m | rayon des candidats |
| `max_candidates` | 8 | dédoublonnés par position (`src/matcher.rs:347-355`) |
| `sigma_z` | 5 m | émission gaussienne `0,5·(d/σ)²` |
| `beta` | 3 m | transition `|dist_réseau − dist_observée| / β` |
| `jump_cost` | 12 | saut en ligne droite si le réseau ne relie pas deux candidats |
| `max_jump_m` | 60 m | longueur maximale d'un saut |
| `footway_cost` | 0,75 | malus d'émission en vélo pour footway, steps, pedestrian |

- Dijkstra borné à `2 × distance + 60 m` (`src/matcher.rs:395-424`).
- Une observation sans candidat coupe la chaîne. On fait un backtrack par chaîne
  (`src/matcher.rs:426-466`).
- Le malus footway tranche l'ambiguïté « trottoir parallèle à la chaussée » en faveur de la
  chaussée, comme le profil vélo de Valhalla.
- **Pont au-dessus d'une route** : il n'y a pas de traitement explicite. Les deux niveaux étant
  déconnectés, la transition (distance réseau) favorise le niveau sur lequel la trace arrive.
  Un passage sur le pont depuis la route du dessous coûterait un saut (12 + écart/β). C'est
  donc la continuité qui décide, pas la géométrie locale.
- **Sauts dans la sortie** : la première moitié d'un saut va à la rue quittée, la seconde à la
  rue atteinte. Les arêtes couvrent ainsi la forme sans trou (`src/trace.rs:521-539`).
- Les arêtes de sortie sont les **suites de segments de même classe** (`src/trace.rs:568-620`).
  La classe dépend des attributs Shortbread, donc un pont produit naturellement sa propre arête.

### Pourquoi l'index OSM a été retiré

Le message du commit `44e5da6` ne donne aucune raison. Le diff du README (`git show 44e5da6 --
README.md`) permet de mesurer l'arbitrage :

- **Gain de l'index** : 86-94 % → **98-99 %** d'accord par classe avec Stadia, et
  « revêtu/non revêtu » 98-99 % → 99,3-99,5 %. L'index ajoutait aussi les `bridleway` et les tags
  exacts (`mtb:scale`, `smoothness`, `access`…).
- **Coût** :
  - un extrait Geofabrik à télécharger et un `osm-index` à relancer chaque semaine (cron) ;
  - pour les Pays de la Loire : PBF de 370 Mo, 11 s, index de 70 Mo, ≈ 270 Mo de RAM ;
  - pour la France : ≈ 3 Go de RAM ;
  - un rechargement à chaud et une jointure segment de tuile → way OSM : voie la plus proche
    à moins de 8 m, à moins de 40° de cap, pénalités de 2 à 6 m selon que le type de voie est
    voisin ou non, vote à la majorité des points ;
  - l'intégration des `bridleway` au réseau.
- **Lecture probable** (inférence) : pour un besoin « revêtu vs non revêtu » déjà couvert à 98-99 %
  par les tuiles, le gain sur la finesse GRAVEL/DIRT ne justifiait pas l'exploitation d'un
  pipeline OSM ni sa RAM. Le retrait garde un service sans état et sans donnée à maintenir.

## 6. Mapping actuel attributs → surface

Règle Valhalla reproduite (`src/valhalla.rs:386-458`) : le tag `surface`, sinon `tracktype`,
sinon une valeur par défaut selon la classe de route et l'usage. Pour Shortbread, le tableau
complet est dans `README.md:100-114`. L'essentiel :

- `paved` → `paved_smooth` (`src/valhalla.rs:398`).
- `unpaved` → `unpaved_guess` (`src/valhalla.rs:423-437`), le seul choix propre au service :
  - `bicycle=designated` sans grade4/5 → `compacted` (voie verte) ;
  - tracktype grade4/5 → `dirt` ; grade1-3 → `gravel` ;
  - sinon `path`/`bridleway` → `dirt`, et tout le reste → `gravel`.
- Sans surface : tracktype grade1 → `paved_rough`, grade2 → `compacted`, grade3 → `dirt`,
  grade4 → `gravel`, grade5 → `path` (`src/valhalla.rs:98-107`).
- Sans rien : route (motorway…residential) et service → `paved_smooth` ; track → `dirt` ;
  path, footway, pedestrian → `compacted` ; cycleway, steps → `paved`
  (`src/valhalla.rs:440-458`).

Ce qu'en dit la table de calibration (`eval/report.md:160-239`, mètres de référence Stadia par
combinaison `kind|surface|tracktype|bicycle`) :

- `track` sans rien (27,7 km) : 98 % DIRT. `path` sans rien (26,7 km) : 90 % GRAVEL. Ces
  valeurs par défaut sont fiables.
- `path|unpaved` (19,2 km) : 31 % GRAVEL / 67 % DIRT. `track|unpaved` : 58/42. Aucune règle
  ne départage bien ces cas.
- `path|unpaved|designated` : 99 % GRAVEL. La règle « voie verte » est confirmée.
- Tout ce qui est `paved` : ≥ 94 % PAVED. La seule exception, `unclassified|paved|grade2`, vient
  d'un `mtb:scale=0` qui est absent des tuiles (`eval/report.md:28-30`).
- PAVED_ROUGH n'apparaît que via `tracktype=grade1` sans surface : 44 m sur 90 km chez nous
  contre 253 m chez Stadia. Les pavés (`sett`, `cobblestone`) sont invisibles.

## 7. Évaluation (`eval/`, `src/bin/eval.rs`)

**Méthode** :
- 3 parcours vélo réels autour de Nantes (`testdata/*.route.json`, 185 km au total, profil
  gravel). Pour chacun, l'outil envoie la requête exacte de tribly et projette la réponse comme
  tribly (`TraceSurfaceProjection` → `SurfaceRanges`, MIN_SPAN 20 m → `TrackSurface`). Il compare
  ensuite **mètre par mètre** aux plages que tribly avait calculées avec les vraies réponses Stadia
  (`src/bin/eval.rs:1-8`, `README.md:222-227`).
- La référence est Stadia (Valhalla sur l'OSM de fin septembre 2026), **pas la vérité terrain**.
- Classes comparées : PAVED, PAVED_ROUGH, GRAVEL, DIRT, PATH, UNKNOWN (`src/tribly.rs:70-80`).
- Le rapport donne la répartition, la matrice de confusion, les 10 plus grands écarts avec la rue
  appariée, nos plages inconnues, et la calibration échantillonnée tous les 5 m
  (`src/bin/eval.rs:424-449`).

**Résultats** (`eval/report.md:9-13`) :

| Parcours | km | Accord classe | Revêtu/non revêtu | Inconnu | Latence |
|---|---:|---:|---:|---:|---:|
| graymond-ride-138 | 90,2 | 86,0 % | 98,1 % | 0,9 % | 77 ms |
| np-655-gravel | 52,2 | 91,6 % | 99,0 % | 1,3 % | 71 ms |
| np677-chill-gravel | 42,8 | 93,5 % | 99,3 % | 0,0 % | 55 ms |

Charge : 15 000 points sur 180 km en moins de 160 ms avec les tuiles en cache ; à froid,
≈ 0,9 s pour 40 tuiles (`README.md:192-194, 250`).

**Origine des écarts** (environ 20 km, inspectés tronçon par tronçon, `eval/report.md:15-35`) :

| Part | Cause |
|---:|---|
| 59 % | surface réduite à paved/unpaved (confusion GRAVEL ↔ DIRT) |
| 14 % | âge des tuiles |
| 6 % | `mtb:scale` |
| 3 % | bridleways absents |
| ≈ 12 % | la référence elle-même |
| reste | voies parallèles (trottoir/chaussée, allée/chemin) |

Les matrices de confusion montrent que les erreurs se concentrent dans le bloc
GRAVEL/DIRT (`eval/report.md:53-60, 91-98, 128-135`). Les confusions PAVED ↔ non revêtu
sont faibles, et la plus grosse s'explique par `mtb:scale`.

**Limites de la méthode** : 3 parcours seulement, tous autour de Nantes (règles d'accès
françaises) ; pas de vérité terrain ; `bridge` et `tunnel` **jamais évalués** ; aucune trace GPS
bruitée réelle (uniquement des tracés planifiés).

## 8. Pièges et leçons

1. **Shortbread n'est pas OSM** : l'information de surface tient en 1 bit (paved/unpaved) et des
   tags entiers manquent. Toute classe plus fine qu'« revêtu / non revêtu » est une
   **inférence**, à présenter comme telle.
2. **Topologie à reconstruire** : sans ID, il faut un recollage aux bords de tuile (clipping
   canonique + fusion à < 1 m) et une insertion des jonctions supprimées par la simplification.
   Sans cela, le HMM « saute » partout.
3. **Niveaux** : séparer sol/pont/tunnel dans le graphe est indispensable pour ne pas apparier
   la route du dessous. Mais sans `layer`, deux ponts superposés sont confondus.
4. **Voies parallèles** : trottoir et chaussée, ou piste cyclable et route, posent la principale
   ambiguïté géométrique. Un malus par type de voie selon le mode la règle à peu de frais.
5. **Trous de connectivité** : le saut à coût fixe (12, ≤ 60 m) évite qu'un raccord manquant ne
   casse toute la chaîne.
6. **Densifier** une trace planifiée (10 m) est nécessaire : sinon, deux points distants de
   plusieurs centaines de mètres laissent le HMM sans contrainte.
7. **Robustesse du fetch** : ne jamais mettre en cache une page d'erreur reçue en 200, distinguer
   204 (rien) de 404 (mauvaise URL), et garder le cache périmé en secours
   (`tests/tiles.rs`, `src/tiles.rs:100-165`).
8. **Fraîcheur** : les tuiles publiques ont des mois de retard, et les écarts qui en résultent
   ne se corrigent pas par l'algorithme.
9. **Accès** : filtrer le réseau par mode (footway ouvert aux vélos en France, trunk fermé)
   change l'appariement. Il faut l'expliciter.
10. **Ce que l'index OSM apportait** : quand la finesse compte, seul un import OSM (PBF) donne
    les tags exacts. C'est un choix d'exploitation (RAM, cron), pas un détail technique.

## 9. Ébauches d'enums (à partir des données réellement disponibles)

### Surface

Arguments tirés des données :
- revêtu/non revêtu est fiable à 98-99 % : c'est le socle ;
- GRAVEL et DIRT ne sont qu'une estimation (la confusion pèse 59 % des écarts) ;
- PAVED_ROUGH et PATH ne sont presque jamais produits depuis Shortbread (44 m et 0 m sur 90 km).

Proposition : une enum courte, accompagnée d'un indicateur de provenance plutôt que de classes
fines inventées.

```rust
enum Surface { Paved, Gravel, Dirt, Unknown }   // Gravel = compacté/gravier roulant ; Dirt = terre/herbe
enum SurfaceSource { Tagged, TrackType, KindDefault } // « Tagged » = surface explicite paved/unpaved
```

- `Paved` ← `surface=paved` ; sinon tracktype grade1 ; sinon `kind` route/service/cycleway/steps.
- `Gravel` ← `unpaved` avec `bicycle=designated` ou grade1-3 ; sinon `track|unpaved` ;
  sinon `path` sans rien, ou tracktype grade2/4.
- `Dirt` ← `unpaved` avec grade4/5 ; sinon `path|unpaved` ; sinon `track` sans rien, ou
  tracktype grade3/5.
- `Unknown` ← point non apparié.

Variante plus prudente : `Paved | Unpaved | Unknown`, avec `Gravel`/`Dirt` exposés en
`hint` seulement. On peut garder `PavedRough` en réserve pour une source future plus riche
(PBF), sans le produire depuis Shortbread.

### Structure

```rust
enum Structure { Ground, Bridge, Tunnel }   // depuis bridge / tunnel des tuiles
```

`Ford` n'est **pas détectable** avec la couche `streets` (aucune clé `ford`). Deux options :
1. l'omettre ;
2. le réserver et l'inférer plus tard, par intersection avec `water_lines` hors pont/tunnel
   (heuristique non éprouvée, risque de faux positifs avec les buses et ponceaux).

`layer` est absent : impossible de distinguer un pont sur pont.

### Type de voie (utile en complément)

Une enum `WayKind` dérivée de `kind`, à côté de la surface et de la structure : `MajorRoad`
(motorway…tertiary), `MinorRoad` (unclassified, residential, living_street, service), `Track`,
`Path`, `Cycleway`, `Footway` (footway, pedestrian), `Steps`, `Unknown`. Elle explique la
surface déduite et sert au malus d'appariement.

## Enseignements pour osm-route-details

- **Réutiliser l'approche « tuiles seules »** : VersaTiles Shortbread z14, cache disque + LRU,
  couloir de ≈ 100 m. Pas d'état, pas de pipeline OSM, latence < 100 ms en cache. Prévoir de
  pouvoir pointer `TILES_URL` vers un `.versatiles` auto-hébergé.
- **Reprendre presque tel quel** :
  - décodage MVT de la seule couche `streets` (`src/mvt.rs`) ;
  - clipping canonique, fusion des bords de tuile et insertion par niveau (`src/network.rs`) ;
  - HMM avec densification de 10 m, σ 5 m, β 3 m, saut à coût fixe et malus footway
    (`src/matcher.rs`).

  Ce sont des parties éprouvées et indépendantes du format Valhalla.
- **Abandonner** : la compatibilité Valhalla (codes d'erreur, `use`, `cycle_lane`,
  `road_class`, fraction `distance_along_edge`) et le portage tribly. On ne garde que la logique
  attributs → enum.
- **Sortie par segment** : nos segments (entre points GPS) doivent porter `Surface`,
  `Structure` et `WayKind`. Les « suites de même classe » d'osm-surface s'y prêtent : il suffit
  de couper aux indices de points d'entrée.
- **Être honnête sur la confiance** : exposer `Paved/Unpaved` comme fait établi et
  `Gravel/Dirt` comme estimation (`SurfaceSource`). Ne pas promettre `Ford` ni `PavedRough`
  avec cette source.
- **Pont/tunnel** : la séparation des niveaux dans le graphe suffit pour l'appariement. Prévoir
  quand même un test dédié (pont au-dessus d'une route, tunnel sous une route), car osm-surface
  ne l'a jamais évalué.
- **Élévation** : elle ne figure pas dans osm-surface et relève d'une source séparée (MNT). Le
  `shape` apparié (coordonnées sur la voie) ou les points d'origine peuvent servir à
  l'échantillonnage. Attention : sur un pont ou dans un tunnel, le MNT donne le sol ou la
  vallée, pas le tablier. La `Structure` sert donc aussi à **corriger l'élévation**, par
  interpolation linéaire entre les extrémités du pont ou du tunnel.
- **Si la finesse devient nécessaire** : l'index OSM PBF retiré (`git show b434a89:src/osm.rs`)
  montre le chemin, avec jointure à 8 m / 40° et 98-99 % d'accord, et son coût (≈ 270 Mo par
  région, ≈ 3 Go pour la France, cron hebdomadaire). Garder une architecture où cette source
  pourrait s'ajouter plus tard sans refonte.
- **Évaluation** : réutiliser les 3 parcours de `testdata/` comme jeu de non-régression. Viser
  une vérité terrain partielle, ou au minimum Stadia/Valhalla, et y ajouter des cas pont, tunnel
  et gué.
