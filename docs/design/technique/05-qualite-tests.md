# 05 — Qualité et tests (conception technique)

> Statut : conception technique. Exigences : [01](../01-conception-generale.md) (§3–§6, §8.5, §10,
> §11.2) et [02](../02-contrat-interface.md). Aucune exigence n'est modifiée ; les écarts sont dans
> « Points de tension » (§10). Les références `chemin:ligne` sont relatives à la racine de vcyclist
> (`VC/`, 6.11.0, `develop` à jour au 2026-10-30) ou d'osm-surface (`OS/`, commit `44e5da6`).
> **[mesuré]** : chiffre publié par l'un de ces projets ; **[à calibrer]** : valeur de départ, fixée
> par la série de calibration, jamais par la série tenue à l'écart.
> **Corpus, métriques (E1–E5, S1–S3, R1–R3, M1, L1–L2, Q1), seuils et protocole de mesure : la source
> de vérité est [`bench/README.md`](../../../bench/README.md)** (commit `b3a2269`). Ce fichier n'en
> garde que la conception logicielle : types de tests, `ord-bench`, formats d'échange, CI, golden
> files, non-régression (§3).

## 1. Principes

1. **Le cœur est pur.** Classification, fusion du profil, détecteurs, sélection des points de
   rupture et D+ prennent des tableaux et renvoient des valeurs : testés sans réseau, disque ni async.
2. **Hors ligne par défaut.** `cargo test` ne sort jamais de la machine : le port d'Accès données est
   servi par un cache pré-rempli (`Offline`, une tuile absente est une erreur) ou un faux serveur local.
3. **Une mesure sans critère écrit avant ne vaut rien** (exploration 01 §6.4) : séries tirées par
   règle sha256 et seuils gelés dans `bench/` **avant** le premier passage du code jugé (bench §2).
4. **Toute différence de sortie s'explique** par `rules_version` ou `data_versions` (doc 02 §6, §4).
5. **Chacun ses données** (doc 01 §10, Q11) : les fixtures de vcyclist et d'osm-surface sont
   **copiées une fois** dans `bench/data/`, avec leur provenance (dépôt, commit, sha256) inscrite au
   manifest. Aucun fichier n'est lu dans un autre dépôt, ni par chemin absolu. Une empreinte qui ne
   correspond plus arrête la mesure.

## 2. Stratégie de tests

| Niveau | Emplacement | Données | Réseau | Lancé par |
|---|---|---|---|---|
| Unitaires purs | `src/**` (`#[cfg(test)]`) | littéraux, générateurs | non | `cargo test`, CI |
| Property-based | `src/**`, `tests/prop_*.rs` | `proptest` | non | `cargo test`, CI |
| Décodage | `tests/decode.rs` | tuiles réelles commitées (`tests/fixtures/tiles/`) | non | `cargo test`, CI |
| Intégration hors ligne | `tests/api_*.rs` | cache pré-rempli + faux serveur local | boucle locale seulement | `cargo test`, CI |
| Contrat HTTP/OpenAPI | `tests/contract.rs` | `openapi.json` commité | non | `cargo test`, CI |
| Golden files | `tests/golden.rs` | `tests/golden/` + cache de banc local (§5.3) | non | à la main, en local (`bench-offline`, §6) ; snapshots commités |
| Banc qualité | binaire `ord-bench` | `bench/manifest.yaml` et `bench/data/` (§3) + cache de banc local | non (sauf `fill-cache`, `latency --profile cold`) | en local (calibration, jugement) ; rapport commité, vérifié en CI (§6) |
| Live | `tests/live_*.rs`, feature `live-tests` | sources réelles | oui | à la main, CI planifiée non bloquante |

### 2.1 Unitaires purs du cœur

Par composant (noms de doc 01 §4), les cas qui **doivent** exister, en plus des cas nominaux :

- **Orchestrateur / préparation** : doublons consécutifs fusionnés, non consécutifs gardés (boucle,
  aller-retour) ; indices d'entrée conservés après fusion ; densification à pas métrique sur un
  segment de 0 m, de 1 cm, de 200 km ; antiméridien et latitude limite Web Mercator rejetés (400) ;
  plus de 16 000 points d'entrée ou plus de 200 km → 413 ; les bornes ne portent que sur l'**entrée** :
  200 km en `distance` à 5 m (≈ 40 000 points de sortie) → 200, jamais 413 (doc 01 §10, doc 02 §7).
- **Classification** : table de T03 §7 exécutée **en table** (`rstest`) : chaque ligne `(kind,
  surface regroupée, tracktype)` → `(surface.class, surface.source, way_kind)`. Un test d'exhaustivité
  énumère toutes les combinaisons d'attributs neutres connues et vérifie qu'aucune ne panique et que
  `source = none` ⇔ `class = unknown`.
- **Adaptateur élévation** : valeur au **centre** du pixel (`VC/elevation/.../ElevationCalculator.kt:92-94`) :
  un point au centre d'un pixel renvoie exactement ce pixel ; continuité aux bords de tuile (deux tuiles
  synthétiques voisines, écart < 1e-4 m de part et d'autre) ; no-data IGN `≤ −9000`
  (`VC/.../Bil32.kt:17-21`) sur **un** des 4 voisins → point entier relu dans Mapterhorn, jamais
  mélangé ; Terrarium `R·256 + G + B/256 − 32768` ; MNS sans repli (erreur typée, jamais une valeur).
- **Fusion du profil** : les 10 invariants de doc 01 §5.5, un test chacun, dont : correction avant
  lissage (un viaduc à −60 m ne laisse aucune trace à plus de la demi-fenêtre de lissage hors de
  l'ouvrage) ; extrémités de l'ouvrage et non de la trace (trace qui commence au milieu) ; ouvrages
  contigus fusionnés ; doute → MNT brut + `structure_uncorrected` ; même portion, même altitude dans
  les deux sens ; MNS jamais lu hors ouvrage au MVP, heuristiques désactivées (compteur de lectures MNS
  du faux port à 0 sur un parcours sans pont ; en V1, jamais **écrit** hors ouvrage, T04 §8.1) ; culée lue dans Mapterhorn → `interpolated_structure`.
- **Constructeur de tablier** (portage de `VC/gpx/.../DeckProfileBuilder.kt:85-115`) : < 2 ancres →
  abandon ; plafond de pente 10 % respecté ; rampes jusqu'au terrain aux deux bouts.
- **D+/D−** (hystérésis, preset `dem` 3 m / 30 m, `VC/docs/guides/elevation.md:193-216`) : jambe
  d'ouverture comptée (`gain − loss` = dénivelé net à un seuil près, `elevation.md:205-210`) ; plage
  GLO-30 → seuil adapté et restitué dans `gain_params.adapted[]` ; trou d'altitude → `partial`.
- **Sélection des points de rupture** : point d'entrée jamais déplacé ; rupture sur un point d'entrée
  sans doublon ; raisons obligatoires sur un point inséré ; `index` = dernier point d'entrée qui précède.
- **Assembleur** : segments contigus de `0` à `length_m` ; coupure sur chaque attribut de doc 02 §4.3
  et **pas** sur `ele_method` seul ; statuts `structure_correction` (`applied`, `partial`, `none_found`,
  `disabled`, `unavailable`) ; `attribution` présente même avec `sections: ["elevation"]` ; `status`
  à `partial` exactement quand l'un des quatre codes de T06 §3 est présent ; chaque code du contrat
  vient de son seul producteur (table de T06 §4, test en table) ; ratios dans `[0, 1]` ;
  `summary.structures[].method = kept_ground` pour un pont sans sursol ; un ouvrage mixte donne une
  entrée par type, contiguës ; un ouvrage `heuristic` non apparié garde `matched = false`.
- **Accès données / caches** : repris d'`OS/tests/tiles.rs:79-133` : page d'erreur reçue en 200 jamais
  cachée ; fichier de cache empoisonné supprimé puis relu ; 404 = indisponible et rien n'est écrit ;
  204 = tuile vide (≠ introuvable) ; cache négatif court ; déduplication des chargements concurrents
  (10 tâches, 1 requête) ; cadence par hôte ≤ 20 req/s pour l'IGN (`VC/.../IgnLidarHdLayer.kt:62-65`)
  mesurée en **temps virtuel** (`tokio::time::pause`) ; relance 429/503 jusqu'à 6 essais avec
  `Retry-After` puis 1, 2, 4 s (`VC/.../TileRequestPacing.kt:24-63`).

### 2.2 Property-based (`proptest`)

Générateurs : parcours aléatoires (2 à 2 000 points, pas de 0 à 5 km, boucles et demi-tours injectés),
**terrains analytiques** (somme de sinusoïdes, marches, plateaux) servis par un faux
`TerrainReader` (`SyntheticTerrain`), ouvrages synthétiques (intervalle + profil de tablier). Propriétés :

| Propriété | Composant |
|---|---|
| Distances cumulées croissantes ; pas densifié ≤ pas demandé ; chaque point d'entrée retrouvé à sa distance | Orchestrateur |
| Ajouter des points colinéaires sur un segment droit change le D+ de moins de 1 % **[à calibrer]** (indépendance à l'échantillonnage, doc 01 §3) | Préparation + Fusion |
| `gain ≥ 0`, `loss ≥ 0` ; `gain` décroît quand le seuil croît ; profil retourné ⇒ `gain` et `loss` échangés ; ajout d'une constante ⇒ D+ inchangé | D+ |
| Aller-retour A→B→A : altitude identique aux distances symétriques sur tout ouvrage corrigé | Fusion (invariant 8) |
| Sortie du plafond de pente : 10 %-lipschitzienne, ≥ entrée aux ancres, idempotente | Constructeur de tablier |
| Segments : partition exacte de `[0, length_m]`, sans chevauchement ni trou | Assembleur |
| Points : `d_m` strictement croissant, points `input` en nombre égal aux entrées dédoublonnées | Sélection des points de rupture |
| Encodage puis décodage polyline (précision 5 et 6) : identité à la précision près | API |
| `lat/lon → tuile → emprise` contient le point ; emprise WMS = emprise XYZ | Accès données |
| Même requête deux fois ⇒ même réponse hors `request_id`, `duration_ms` | Service complet sur faux port |

Les cas réduits trouvés par `proptest` sont commités (`proptest-regressions/`) et rejoués à chaque
passage. Le décodage MVT, BIL et Terrarium et le parseur de requête JSON reçoivent en plus une cible
`cargo fuzz` (hors CI, lancée avant chaque version mineure).

### 2.3 Décodage sur tuiles fixtures commitées

`tests/fixtures/tiles/`, organisé comme le cache disque (même espace de noms source/produit/version),
pour que les tests d'intégration le lisent tel quel (modèle `OS/tests/api.rs:18-31`, 140 Ko de
tuiles VersaTiles z14 réelles) :

| Fixture | Contenu | Taille |
|---|---|---|
| VersaTiles z14, tronçon np655 + 2 tuiles de mer | reprises d'osm-surface (`OS/tests/fixtures/tiles/`) | ≈ 140 Ko |
| VersaTiles z14 sur le Pont de Saint-Nazaire, les Grands Goulets, un pont coupé par un bord de tuile | à extraire | ≈ 300 Ko |
| IGN BIL z15 MNT + MNS : tablier de Saint-Nazaire, portail nord des Grands Goulets, une tuile de côte (no-data partiel) | à extraire, **gzip** dans le dépôt | ≈ 8 × 256 Ko brut |
| Mapterhorn Terrarium z12 : mêmes emprises + une tuile GLO-30 (Maroc) | à extraire | ≈ 4 × 65 Ko |
| Synthétiques : BIL tout `-9999`, BIL tronqué, WebP tronqué, HTML en 200, MVT vide, MVT avec `bridge` sans `kind` | générés par un test `#[ignore]` documenté | < 50 Ko |

Assertions : nombre de lignes et d'attributs décodés (modèle `OS/src/mvt.rs:381-420`) ; coordonnées
dans l'emprise étendue de la tuile ; **empreinte** SHA-256 du résultat décodé sérialisé de façon
canonique (doc 01 §8.5) ; quelques valeurs ponctuelles figées (altitude au centre d'un pixel connu,
comparée une fois à l'API alti IGN et notée dans `tests/fixtures/README.md`). Une empreinte qui
bouge sans changement de fixture signale un changement de décodeur : elle se met à jour dans le même
commit, avec justification.

### 2.4 Intégration hors ligne

Le routeur axum est appelé en mémoire (`tower::ServiceExt::oneshot`, modèle `OS/tests/api.rs:30-60`).
Deux montages du port d'Accès données :

- **`Offline`** (`cache.offline`, T02 §4.2) : cache disque pré-rempli en lecture seule ; une tuile
  absente du cache est `TileOutcome::Failed` (`FailureKind::Offline`, comportement
  `OS/tests/tiles.rs:129-133`). Sert aux scénarios nominaux et aux golden files.
- **`ScriptedServer`** : un serveur axum local sur `127.0.0.1:0`, dont la réponse à la n-ième requête
  est scriptée (`OS/tests/tiles.rs:28-55`). Sert à l'injection de pannes.

Scénarios obligatoires (un test chacun, statut HTTP et code d'avertissement vérifiés, doc 01 §6) :
IGN en 503 permanent + Mapterhorn sain → 200, `terrain_fallback`, `dsm_unavailable`,
`fallback_after_error: true`, ponts en `interpolated_structure` ; IGN et Mapterhorn en panne → 503 +
`Retry-After` ; même cas avec `accept_partial` → 200 `partial`, `elevation_unavailable` ; typologie en
panne → 503 / `typology_unavailable` ; cache périmé servi en secours ; parcours hors contour LiDAR →
**zéro** requête IGN (compteur du serveur) ; option inconnue → 400 **avant** toute lecture de tuile
(compteur à 0) ; budget dépassé (serveur qui répond après le budget, temps virtuel) → 504 ; file
saturée → 503 `overloaded` ; `debug` sans droit → 400.

Mise en œuvre (incrément 6) : `crates/ord-server/tests/api_failures.rs` (pannes des deux côtés,
`Retry-After`, cache périmé, 400 sans lecture, 504, `overloaded`, client parti), `api_ign.rs` (IGN en
503, hors contour), `api_route_details.rs` (typologie en panne), `api_validation.rs` (`debug`). Le 504
y est mesuré en temps réel avec un budget de 1 s : `ScriptedServer` est un vrai serveur TCP local, que
le temps virtuel de tokio ne pilote pas. Le même cas en temps virtuel (budget de 90 s) et l'annulation
d'une requête abandonnée sont des tests de l'orchestrateur (`ord-pipeline`).

### 2.5 Contrat HTTP et OpenAPI

Le contrat OpenAPI est **généré depuis le code** et commité (`contract/openapi.json`, T01 §10), comme dans vcyclist
(`VC/api/src/test/kotlin/io/github/glandais/api/OpenApiContractTest.kt:41-90`). Tests :

1. le contrat commité est celui rendu par le code ; le rendu est déterministe ;
2. chaque route exposée a une opération décrite (`operationId`, résumé), et réciproquement ;
3. le contrat servi (s'il est servi) est le contrat commité ;
4. **chaque réponse golden** (§2.6) et chaque corps d'erreur valident contre le schéma (`jsonschema`) ;
5. erreurs : `Content-Type: application/problem+json`, `type = urn:osm-route-details:problem:<code>`,
   `code` identique au suffixe, `status` = statut HTTP (doc 02 §5.1) ;
6. enums gelées : les valeurs de `surface.class`, `structure`, `ele_method`, `origin` du terrain sont
   comparées à une liste écrite en dur dans le test ; une valeur retirée ou renommée échoue ;
7. en CI, `oasdiff breaking` entre le contrat de `develop` et celui de la branche : toute rupture dans
   `/v1` échoue (doc 02 §6 : ajouter ne casse pas, retirer casse).

### 2.6 Golden files

`tests/golden/<scénario>/request.json` et `response.json`, produits sur le **cache de banc** (§5.3) en
mode `Offline`, comparés par `insta` (snapshots JSON) avec rédaction de `meta.request_id` et
`meta.duration_ms`. Les flottants sont comparés **après** la sérialisation du contrat, qui applique les arrondis de
l'assembleur (T01 §9 : cm, 0,01 % de pente, 1e-6°) ; tolérance d'une unité du dernier chiffre pour
les écarts flottants entre machines. En-tête de chaque snapshot :
`api_version`, `rules_version`, empreinte des données (§4).

Scénarios (au moins) : viaduc (Saint-Nazaire, `test1`) ; tunnel de montagne (Grands Goulets, `test4`
km 13,6–15,4) ; passage sous un pont (`n02-nantes-chevire-under`, `n15-lyon-berges-rhone`) ; ponts
contigus (`p05-saumur`, Pont de Bellevue en deux parties) ; trace qui commence au milieu du tablier
(`test1` coupé à 2,5 km) ; pont parcouru deux fois (Pont Général Audibert, `test2`) ; demi-tour sur un
pont (synthétique) ; trace hors France (Mapterhorn seul, `outside_primary_zone`) ; plage GLO-30
(`gain_params.adapted[]`) ; modes `input`, `distance`, `input_with_breakpoints` sur le même parcours ;
`sections: ["elevation"]` ; `structure_correction: false` ; erreurs 400, 413, 422.
Pour compléter doc 02 §5.3 ligne à ligne : doublons consécutifs et indices clients conservés ;
aller-retour sur un même pont ; points d'entrée espacés de 2 km autour d'un viaduc (`unknown`, droite,
points insérés) ; portion hors réseau (`unmatched_range`, `status: complete`) ; parcours en partie
hors d'un extrait (`Extent::Bbox` simulé, `typology_missing` et `elevation_missing`,
`status: partial`) ; trou court comblé et trou long (`SyntheticTerrain` avec no-data) ; parcours
transfrontalier France–Espagne (aucun avertissement de référentiel vertical). Le test
`contract_edge_cases` vérifie qu'à chaque ligne de doc 02 §5.3 correspond un scénario.

Deux règles viennent du banc. (1) **Aucune trace personnelle** dans `tests/golden/` : les traces
perso ne sont **jamais** versionnées (`bench/private/`, hors git) et les golden tests ne les utilisent
pas (doc 01 §10). Les scénarios perso restent dans `ord-bench`. (2) Un snapshot golden **montre** la
sortie du service : il ne doit porter que sur des cas et des ouvrages de **calibration** du manifest,
sinon il fait fuiter la série tenue à l'écart. L'appartenance se lit dans les listes du manifest
(§3.3) ; un scénario qui toucherait un ouvrage, une trace ou une fenêtre tenus à l'écart est reporté
après le jugement du MVP ou déplacé sur un ouvrage de calibration.

### 2.7 Tests live (feature `live-tests`)

Compilés seulement avec `--features live-tests` ; aucun n'est requis pour fusionner. User-Agent
identifiable (explorations 05 et 06). Contenu :

- points de contrôle de vcyclist sur Mapterhorn (`VC/elevation/src/jvmTest/.../ElevationProviderIntegrationTest.kt:24-56`) :
  Mont Blanc (45,8326 ; 6,8652) 4 805 ± 50 m, mer Morte (31,5 ; 35,5) −430 ± 50 m, Badwater (36,250 ; −116,832) −85 ± 50 m ;
- IGN WMS-R : un `GetMap` BIL z15 connu (MNT et MNS) renvoie 262 144 octets et la valeur figée en §2.3 à 0,05 m ;
- Mapterhorn : 404 à z13 au Maroc (GLO-30 seul, exploration 05 §1) ; `coverage-index.pmtiles` lisible, ETag noté ;
- VersaTiles : `streets` porte `kind`, `bridge`, `tunnel`, `surface`, `tracktype` ; revalidation ETag → 304.

## 3. Banc qualité : `bench/` fait foi

`bench/` est la **source de vérité** du corpus, des métriques, des seuils et du protocole de mesure.
Ce chapitre ne les recopie pas. Il décrit comment le logiciel les consomme. Si ce fichier et
`bench/README.md` divergent, le banc prime, et l'écart est un défaut de ce fichier.

| Sujet | Où, dans le banc |
|---|---|
| Bancs de vcyclist (copiés dans `bench/data/` avec leur provenance), chiffres de base, **pièges** | `bench/README.md` §1.1 ; `manifest.yaml` → `vcyclist` |
| Éval d'osm-surface (référence Stadia, pas une vérité terrain ; copiée dans `bench/data/` avec sa provenance) | `bench/README.md` §1.2 ; `manifest.yaml` → `osm_surface` |
| Traces barométriques personnelles : acquisition, période exclue, critère « fiable », groupes `dplus`, `plaine`, `ouvrage`, `hors_france`, `negatif`, vie privée | `bench/README.md` §1.3 ; `manifest.yaml` → `perso` ; fichiers dans `bench/private/` (hors git) |
| Corpus de 36 ouvrages (OSM + MNT/MNS/MNH IGN ponctuels) | `bench/README.md` §1.4 ; `data/ouvrages.yaml`, `data/ouvrages-geom.json`, `data/ouvrages-osm/` |
| Première mesure baro vs MNS LiDAR | `bench/README.md` §1.5 ; `data/deck_baro_vs_lidar.csv` |
| Discipline des séries (règle sha256, gel des seuils, jugement unique) | `bench/README.md` §2 « Discipline » ; listes dans `manifest.yaml` (`serie`, `ouvrages.serie_tenue_a_l_ecart`) |
| Campagne de mesure : empreintes, modes, grille de 5 m, décalage du baromètre | `bench/README.md` §2 « Campagne » |
| Métriques E1–E5, S1–S3, R1–R3, M1, L1–L2, Q1 | `bench/README.md` §3 |
| Seuils du MVP | `bench/README.md` §4 |
| MNS sur la trace ou sur la géométrie OSM (mesure V1.x) | `bench/README.md` §5 |
| Ce que l'utilisateur doit encore fournir | `bench/README.md` §6, repris au §11 |

### 3.1 Frontière entre `bench/tools/` et `ord-bench`

Les données ne circulent que dans un sens : `bench/tools/` constitue les références, `ord-bench` fait
passer le service sur ces références et calcule les métriques.

| | `bench/tools/` (Python, bibliothèque standard + PyYAML) | `tools/ord-bench` (Rust, T01 §2) |
|---|---|---|
| Rôle | Constituer et qualifier le corpus, en amont du service | Faire passer le service sur le corpus, mesurer, juger |
| Appelle le service | jamais | toujours (en processus par défaut, ou `--url`) |
| Réseau | Overpass, API alti IGN, MCP Strava, Stadia Maps (map matching des traces enregistrées, `trace_attributes` pour R3), au moment de la constitution | aucun en `run` et `judge` (cache de banc, §5.3) ; seulement `fill-cache` et `latency --profile cold` |
| Écrit | `manifest.yaml`, `data/ouvrages.yaml`, `data/ouvrages-geom.json`, `data/ouvrages-osm/`, `data/deck_baro_vs_lidar.csv`, copies des fixtures vcyclist et osm-surface dans `data/`, `private/traces/*.gpx`, traces recalées et réponses Stadia (hors git, régénérables) | `bench/reports/<date>-<rules_version>/` (`results.json`, `report.md`, CSV par cas), `bench/judgements.md`, `bench/baseline.json`, `bench/cache.lock` |
| Métriques | **Q1** : verdicts de `qualify_baro.py`, inscrits dans `manifest.yaml` (`qualification.verdict`) | **E1–E5, S1–S3, R1–R3, M1, L1–L2** ; Q1 recopié depuis le manifest, sans recalcul |

Règles :

- `ord-bench` **ne recalcule aucune référence** : il lit celles du banc. Les vérités de vcyclist
  (`truth/testN.csv`, `structures.csv`) sont lues dans leur copie de `bench/data/`, vérifiées par
  l'empreinte du manifest. La médiane du MNS sur 25 m au milieu de chaque pont, référence E3 de
  dernier recours (ni baromètre ni vérité `B`), est lue dans `ouvrages.yaml`
  (`milieu_mns_mediane_25m`, ajoutée par le banc). C'est la même donnée que celle du service : le
  rapport la marque `reference = mns_median_25m` (cohérence, pas exactitude ; le baromètre reste la
  seule référence indépendante). Les valeurs **ponctuelles** de `ouvrages.yaml` ne servent jamais de
  référence E3 : ce sont des indices (§3.5).
- **E2 est indicatif** (doc 01 §10) : D+ service / D+ baro ∈ [0,90 ; 1,20] est mesuré, rapporté et
  suivi en non-régression (§4). Seul critère bloquant : par groupe, la médiane ne se dégrade pas par
  rapport au D+ du **MNT brut** (service avec `structure_correction: false`, même preset). La forme
  exacte est celle de `bench/criteria.yaml`.
- `bench/tools/` ne lit jamais `results.json` pour produire un verdict : il n'y a pas de boucle. Une
  analyse ponctuelle en Python peut le lire, en lecture seule.
- **Seuils** : `ord-bench` lit `bench/criteria.yaml`, transcription machine de `bench/README.md` §4
  (un bloc par identifiant : métrique, groupe ou série visés, borne, bloquant ou rapporté). Ce fichier
  est à créer dans un commit séparé, avant le premier passage (discipline du banc, §2.2). Un test
  d'`ord-bench` vérifie que ses identifiants sont exactement ceux de la colonne `Id` de
  `bench/README.md` §3 et §4.
- **Règles de calcul non fixées par le banc** : distance maximale d'appariement entre la trace et la
  référence (30 m, `MATCH_MAX_M` de vcyclist), emprise d'un ouvrage (ways `osm.chain` projetées sur la
  trace), fenêtre du croisement E5 (± 10 m autour du point `croisement`). Ce sont des constantes `BenchRules` d'`ord-bench`, hachées dans `results.json`
  (`bench_rules_sha256`) et gelées avec les seuils : les changer obéit à la même règle que les seuils.

### 3.2 Format d'échange : `results.json`

Un fichier par campagne (bench §2, « Campagne »), sans aucune coordonnée :

```json
{ "schema": 1,
  "campagne": { "date": "…", "type": "calibration | jugement", "increment": "MVP",
    "commit": "…", "api_version": "…", "rules_version": "…", "rules_fingerprint": "…",
    "data_fingerprint": "…", "data_versions": { … }, "manifest_sha256": "…",
    "criteria_sha256": "…", "bench_rules_sha256": "…", "private_present": true },
  "cas": [ { "id": "perso-20", "serie": "calibration", "groupe": "ouvrage",
             "mode": "distance | input_with_breakpoints", "cache": "froid | chaud",
             "metriques": { "E1.rmse_m": 0.41, "E1.p95_m": 1.1, "E2.ratio": 1.02 } } ],
  "ouvrages": [ { "id": "stn-pont", "serie": "tenue_a_l_ecart",
                  "passages": [ { "cas": "vc-test1", "reference": "baro | vc_B | mns_median_25m",
                                  "E3.err_milieu_m": 0.3, "E3.err_max_m": 0.9 } ] } ],
  "agregats": { "E1": { "plaine.rmse_m": 0.6, "relief.rmse_m": 1.3 } },
  "verdicts": { "E1": "pass", "S3": "reported", "R3": "reported" },
  "diagnostics": { "structure_uncorrected": 0, "tuiles_lues": { "ign_mnt": 190 } } }
```

Verdicts : `pass`, `fail` (avec la valeur mesurée), `reported` (pas de seuil, par exemple S3 au MVP
ou E3 hors France, ou la plage indicative d'E2), `skipped` (référence absente : réponses Stadia
de R3 non régénérées, ou `bench/private/` absent). Un
`skipped` n'est jamais compté comme vert. Les **diagnostics** (nombre de `structure_uncorrected`,
de `structure_slope_implausible`, tuiles et octets lus par source, latence « chaud disque » et
« chaud mémoire ») sont des indicateurs logiciels, sans seuil, hors banc.

### 3.3 Séries tenues à l'écart dans `ord-bench`

- `ord-bench run` vérifie toutes les empreintes du manifest (traces privées comprises, si
  `bench/private/` est présent), puis **exclut** les cas `serie: tenue_a_l_ecart` et les ouvrages de
  `ouvrages.serie_tenue_a_l_ecart`, avant de lire une seule tuile. Les séries partitionnent les
  **mètres** de trace : série par groupe d'ouvrages et série par trace (sha256) ; la fenêtre de
  ± 500 m autour d'un ouvrage franchi suit la série de l'ouvrage (E3, E4, E5, S*) et, si elle diffère
  de celle de la trace, elle est masquée dans E1, E2 et E2b de la trace. La règle est celle de
  `bench/README.md` §2 « Discipline » : `ord-bench` applique les listes et les fenêtres masquées du
  manifest, sans règle propre.
- `ORD_JUDGE=1 ord-bench judge --increment MVP` ne lit que la série tenue à l'écart. Il vérifie que
  `manifest_sha256`, `criteria_sha256` et `bench_rules_sha256` sont ceux du commit de gel, juge
  **une fois** et ajoute le résultat à `bench/judgements.md` (date, commit, `rules_version`,
  `data_versions`, empreinte du manifest : bench §2.3). Il refuse un incrément déjà présent dans ce
  fichier. Après jugement, la série devient calibration, et une nouvelle série doit être constituée
  pour l'incrément suivant, par une règle écrite (bench §2.3, §6).
  Mise en œuvre (incrément 6, `tools/ord-bench/src/judge.rs`) : le gel est un fichier
  `bench/gel/<incrément>.json` (`increment`, `manifest_sha256`, `criteria_sha256`,
  `bench_rules_sha256`, tel qu'`ord-bench gel` l'affiche) commité dans le commit de gel, passé par
  `--gel <commit>`. Avant toute lecture, `judge` exige `ORD_JUDGE=1`, l'incrément absent de
  `judgements.md` (titre `## Jugement <incrément> (<date>)`) et sans dossier
  `bench/reports/*-jugement-<incrément>` (quelle qu'en soit la date), le commit ancêtre de `HEAD`, le fichier
  de gel présent à ce commit, `manifest.yaml`, `criteria.yaml` et le fichier de gel inchangés depuis,
  et les empreintes courantes égales à celles du gel. La campagne (`campaign::run_series`, série
  tenue à l'écart) est la même que celle de calibration, séries inversées : traces tenues à l'écart
  entières (fenêtres des ouvrages de calibration masquées), ouvrages de `ouvrages_tenus`, extraits de
  ± 500 m des traces de calibration autour de ces ouvrages ; ni vcyclist ni osm-surface (« déjà
  vu »). Un seul cas sauté interrompt le jugement sans rien consigner ; sinon `results.json` va dans
  `bench/reports/<date>-<rules_version>-<révision du banc>-jugement-<incrément>/` (révision omise
  quand `BENCH_REVISION` vaut `None`, comme pour la calibration ; `campagne.revision_banc` dans
  `results.json`) et l'entrée (versions, révision du banc, empreintes,
  verdicts par identifiant et par mode, accepté ou refusé) est ajoutée à `judgements.md`. Le cache de
  banc doit d'abord couvrir la série : `ord-bench fill-cache --serie tenue_a_l_ecart` (live, IGN à
  20 req/s, aucune mesure).
- Les séries `p09..p17`, `th*` et `hn*` de vcyclist sont **consommées** : pour nous, elles servent à la
  calibration (bench §1.1, piège 2). Une version précédente de ce fichier les gardait tenues à
  l'écart ; c'est le banc qui fait foi.
- Les « incréments » du banc (MVP, V1…) ne sont pas les incréments 0 à 6 du plan (README technique
  §3). Les incréments 1 à 5 sont acceptés sur la **calibration** seule. Le jugement unique de la série
  tenue à l'écart a lieu à l'incrément 6 et vaut pour le MVP entier.

### 3.4 Lecture des traces personnelles

- **Fichiers** : `bench/private/traces/<id>.gpx` (jamais versionnés), chemins relatifs à `bench/` tels
  que les donne le manifest (`--private-dir` pour les déplacer). `<ele>` est l'altitude enregistrée brute, sans `<time>`
  (`bench/tools/stream_to_gpx.py`). GPX est le seul format : le banc dérive tout des streams Strava,
  la lecture FIT est abandonnée.
- **Sans `bench/private/`** (CI, autre machine) : les cas perso sont `skipped`, jamais verts.
- **Requête** : le service v1 est conçu pour des parcours planifiés. Une trace enregistrée est donc
  d'abord **recalée sur la route** par l'outillage du banc (map matching Stadia Maps, fichier hors
  git, régénérable), puis la trace recalée est envoyée en `points` (rééchantillonnée à 10 m, pas
  élargi au-delà de 160 km pour rester sous 16 000 points d'entrée), dans les deux modes du banc
  (`distance` à 5 m, puis `input_with_breakpoints`, à froid puis à chaud : bench §2, étape 2). La
  sortie n'est pas bornée en points. Les traces de plus de 200 km (aujourd'hui `perso-06`, 205 km)
  sont **découpées par l'outillage du banc** avant envoi. Le service est comparé au baromètre de la
  trace d'origine. M1 se mesure sur ces parcours recalés et sur les parcours planifiés.
- **Comparaison** : protocole du banc, §2, étape 4. Décalage global = médiane(baro − service) sur les
  stations `ground`. Pour un ouvrage, décalage local entre 1 et 3 km de part et d'autre. La dérive lente
  (> 5 km) est retirée par médiane glissante avant les résidus d'altitude, jamais avant le D+. Le
  recalage de retard τ et l'offset local sur 300 m de la version précédente sont abandonnés.
- **Test de la méthode** (test d'`ord-bench`, pas une métrique du banc) : sur `strava.gpx` de vcyclist
  (`demo/public/gpx/`, copié dans `bench/data/` avec sa provenance),
  le calcul de D+ d'`ord-bench` doit redonner le rapport publié par vcyclist (714 / 637 m ≈ 1,12 au
  preset `dem`, `elevation.md:319-321`) à ± 3 points près.
- Les rapports des cas perso n'écrivent **aucune coordonnée** : identifiants `perso-NN`, distances
  curvilignes, identifiants d'ouvrage du corpus.

### 3.5 Lecture des résultats : ce que le banc a déjà appris

- **Cohérence, pas exactitude.** La vérité `test*` de vcyclist (`T`, `B`, `N`) est tirée du **même**
  MNT/MNS IGN que celui que lit le service. Une erreur mesurée contre elle est une cohérence. Les
  traces barométriques personnelles sont la **seule référence indépendante** : quand l'une traverse
  un ouvrage, elle prime sur `B` et sur `ouvrages.yaml` (bench §1.1, piège 1). `report.md` affiche une
  colonne par type de référence, jamais une moyenne qui les mélange.
- **Le MNS n'est pas une vérité sur tous les ponts.** Au milieu des grands ponts (Saint-Nazaire,
  Pirmil, Anne-de-Bretagne, Mauves), le MNS LiDAR est à ≤ 1 m du baromètre corrigé. Au pont
  Général-Audibert, il est **2,4 à 4,5 m au-dessus** sur 5 passages, et la médiane sur 25 m vaut la
  lecture ponctuelle (11,54 m) : le biais tient au MNS sur cet ouvrage, pas à un pixel. Cause ouverte
  (voie cyclable plus basse que la surface lue, ou baromètre). Le service garde ancres propres et
  médiane sur 25 m (T04 §3.2, étapes 4 à 6) contre les artefacts ponctuels ; un biais continu se verra
  en E3 contre le baromètre.
- **Passage sous un pont.** Sous le viaduc de Cheviré, le MNS lu au croisement est à 57–59 m pour un
  sol à 0,4–6,9 m, soit ≈ 50 m d'erreur. C'est le cas **E5** (`chevire-dessous`, `rotonde-dessous`,
  `perso-23..25`), à tolérance quasi nulle (bench §4). Côté service, il est couvert par T04 §2.3 (le
  MNS n'est lu qu'aux stations d'un pont apparié) et T04 §7 (pont au-dessus de la trace écarté par la
  typologie, invariant 6).

### 3.6 Correspondance des anciens critères A1–A13

Les critères A1–A13 de la version précédente sont **retirés**. Le tableau dit par quoi chacun est
remplacé. Quand le seuil diffère, c'est celui du banc qui s'applique.

| Ancien | Objet | Remplacé par (bench §3–§4) | Écart notable |
|---|---|---|---|
| A1 | Ponts de Loire à Nantes, max ≤ 2 m sur `B` | **E3**, pont tagué < 300 m : \|milieu\| ≤ 2 m, max ≤ 3 m | Référence baro d'abord, `B` à défaut |
| A2 | Saint-Nazaire ≤ 5 m | **E3**, grands ponts : max ≤ 5 m ; ≥ 90 % des ponts passent | — |
| A3 | Rappel 21/21 de `structures.csv` | **S1** ≥ 95 % de la longueur taguée | Longueur, pas un compte |
| A4 | `p09..p17` ≥ 9/11, jugement unique | **S1** et **E3** sur la calibration ; la tenue à l'écart devient celle du banc (`manifest.yaml`) | `p09..p17` consommés |
| A5 | Tunnels du Vercors, max ≤ 2 m | **E4** : \|max\| ≤ 5 m et D+ fictif ≤ 5 m par tunnel | Seuil élargi |
| A6 | Tunnels `th*` ≥ 6/7 | **E4** : ≥ 6/7 des tunnels tagués ; `th*` en calibration | — |
| A7 | 0 station déplacée sur les négatifs | **S2** (0 ouvrage fictif, ≤ 0,5 % de longueur) et **E5** (passage sous un pont) | E5 isolé |
| A8 | RMS `T` ≤ 1,0 m (`test1..3`), ≤ 1,5 m (`test4`) | **E1** contre le baromètre ; l'ancien A8 reste un **test de cohérence nommé** (demi-pixel) | Référence indépendante |
| A9 | D+ / vérité ∈ [0,90 ; 1,15] | **E2** : D+ / baro ∈ [0,90 ; 1,20], médiane du groupe ∈ [0,95 ; 1,12] | Indicatif ; bloquant : ne pas dégrader le D+ du MNT brut (§3.1) |
| A10 | Revêtu / non revêtu ≥ 97,5 % | **R2** ≥ 97 % ; plus **R1** (classe ≥ 85 %) | — |
| A11 | `matched_ratio` ≥ 98 % | **M1** : non apparié ≤ 1 %, `unknown` ≤ 2 % | — |
| A12 | p95 à chaud ≤ 500 ms, 180 km | **L2** : p95 ≤ 500 ms, 100 km | — |
| A13 | Froid ≤ 25 s, 130 km (live) | **L1** : p95 ≤ 20 s pour 100 km en France, ≤ 5 s hors France | — |
| ancien R1 | D+ / baro sur les traces perso, non bloquant | **E2** et **E3** | Nom R1 réattribué au revêtement |
| — | (nouveaux) | **S3** (ouvrage non tagué, rapporté), **R3** (accord avec `trace_attributes` de Stadia, pas une vérité terrain), **Q1** (rejet des négatifs) | — |

## 4. Non-régression : `rules_version` et `data_versions`

- **Empreinte des règles** : les tables de classification, les options des détecteurs, les seuils des
  points de rupture, les presets de D+ et l'adaptation GLO-30 sont des structures `Serialize`. Leur
  sérialisation canonique hachée (SHA-256) donne `RulesFingerprint`. Le fichier `rules/VERSIONS`
  associe chaque `rules_version` à son empreinte. Un test échoue si l'empreinte courante n'y figure
  pas : **changer une règle oblige à monter `rules_version`**.
- **Empreinte des données** : `DataFingerprint` = SHA-256 de la liste triée `(espace de noms, z/x/y,
  SHA-256 du contenu)` des tuiles du cache de banc, plus les identifiants de `data_versions`. En
  mode `Offline`, `data_versions` est donc exact, même pour l'IGN.
- **Empreinte du banc** : `manifest_sha256`, `criteria_sha256` et `bench_rules_sha256` (§3.2). Si l'une
  change, la comparaison avec la campagne précédente est invalide (bench §2.4) : `compare` le dit au
  lieu de produire un verdict.
- **Règle de CI** : si un snapshot golden ou un résultat de banc change, alors `rules_version`,
  l'empreinte des données **ou** l'empreinte du banc doit avoir changé dans la même PR. Sinon, c'est
  une régression du code (ou un non-déterminisme) : échec.
- **Référence de banc** : `bench/baseline.json` (résultats de la dernière version acceptée, avec ses
  empreintes). `ord-bench compare` signale toute métrique qui se dégrade au-delà d'une tolérance
  (0,2 m sur une erreur d'ouvrage, 1 point d'accord, 2 points de rapport de D+, E2 compris
  **[à calibrer]**) et tout
  seuil de `bench/README.md` §4 qui bascule.
- **Changement de données** (nouvelle release VersaTiles, IGN publié sur une nouvelle zone,
  rafraîchissement du cache) : nouveau cache, nouveau `baseline.json`, golden régénérés dans une PR
  dédiée sans changement de code, rapport de diff joint.

## 5. Outillage

### 5.1 Binaire `ord-bench`

Membre du workspace (`tools/ord-bench`, T01 §2 ; données dans `bench/`), dépend d'`ord-pipeline`,
`ord-server` et `ord-testkit`, pas l'inverse : les crates de lecture GPX/YAML et de rapport n'entrent
pas dans l'image de production. Comme l'`eval` d'osm-surface (`OS/src/bin/eval.rs:1-8`), il appelle
le service **en processus** par défaut, ou une instance distante avec `--url`.

| Sous-commande | Rôle |
|---|---|
| `check` | vérifie les empreintes du manifest (privées comprises si présentes) et la cohérence `criteria.yaml` ↔ `bench/README.md` §3–§4 ; rapporte Q1 |
| `fill-cache [--cas <ids>]` | live : remplit (ou régénère) le cache de banc local (`~/.cache/osm-route-details/bench`, §5.3) pour les cas du manifest, écrit `bench/cache.lock` (empreinte, taille, date) |
| `run --offline [--cas <ids>]` | campagne de calibration (§3.3) : exclut la série tenue à l'écart, écrit `results.json` |
| `judge --increment <i>` | exige `ORD_JUDGE=1` ; jugement unique de la série tenue à l'écart (§3.3) |
| `compare <a.json> <b.json>` | diff de deux campagnes, verdict par identifiant du banc |
| `latency --profile cold\|warm` | L1 (live, caches disque vidés) et L2 (même requête rejouée) ; `warm-disk` et `warm-mem` en diagnostic |
| `dsm-placement` | méthodes A et B du protocole du banc §5 (§7) |

### 5.2 Rapports

Chaque campagne écrit `bench/reports/<date>-<rules_version>/` : `results.json` (§3.2, lu par
`compare` et la CI), `report.md` (tableaux comme `OS/eval/report.md` : un tableau par identifiant du
banc avec la valeur, le seuil et le verdict, les pires écarts, les ouvrages non corrigés, les matrices
de confusion du revêtement), et un CSV par cas (`s, référence, type de référence, service,
ele_method, structure`) pour tracer les profils. Les rapports des cas perso n'écrivent **aucune
coordonnée** (§3.4).

### 5.3 Cache de banc

Coût à froid estimé d'après vcyclist (`ElevationBenchProbeTest.kt:64-67`) : ≈ 190 tuiles IGN
(48 Mo) et 20 Mapterhorn pour les 4 routes, autant de MNS, ≈ 178 BIL (47 Mo) pour les contrôles de
ponts, plus les tunnels ; ≈ 11 Mo de VersaTiles pour 185 km (`OS/README.md:177`). Les 43 traces
personnelles (≈ 4 600 km) et les 36 ouvrages s'y ajoutent. La taille est **en cours de mesure côté
banc** (`bench/README.md` §6 bis) : ≈ 250–350 Mo sans les traces perso, **≈ 2 Go estimés** avec
elles ; mesure exacte au premier `fill-cache`.

**Décision** (doc 01 §10) : le cache de banc est **local, hors git**, dans
`~/.cache/osm-route-details/bench` par défaut (`ORD_BENCH_CACHE` ou `--cache-dir` pour le déplacer),
et **régénérable** par `ord-bench fill-cache`. Il n'est ni archivé ni stocké à distance (pas
d'asset de release, pas de stockage objet) : aucune redistribution des tuiles (Etalab, ODbL, sources
Mapterhorn), donc aucune question de licence. Les tuiles des traces personnelles y révèlent les zones
parcourues : elles ne quittent pas la machine, comme `bench/private/`. `bench/cache.lock` (commité)
n'en garde que l'empreinte `DataFingerprint`, la taille et la date de remplissage. Une régénération
lit des données vivantes : si l'empreinte change, c'est un **changement de données** au sens du §4
(nouveau `baseline.json`, golden régénérés dans une PR dédiée).

## 6. CI (GitHub Actions)

`ci.yml`, sur chaque PR et sur `develop` :

| Job | Commandes | Bloquant |
|---|---|---|
| `fmt` | `cargo fmt --all --check` | oui |
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | oui |
| `test` | `cargo nextest run --workspace --locked` puis `cargo test --doc` ; `proptest` à 256 cas | oui |
| `contract` | test §2.5 + `oasdiff breaking` contre `develop` | oui |
| `deny` | `cargo deny check` (licences, avis de sécurité, doublons) | oui |
| `bench-report` | sans cache ni `bench/private/` : vérifie que le `results.json` commité par la PR porte le `RulesFingerprint` courant, l'empreinte de `bench/cache.lock` et les empreintes **courantes** de `bench/manifest.yaml`, `bench/criteria.yaml` et des `BenchRules`, puis `ord-bench compare` contre `baseline.json` et règle du §4 | oui |
| `docker` | construction de l'image, `/health/live` sur le conteneur | oui sur `develop` |

**`bench-offline` en local** (la CI n'a ni le cache, §5.3, ni les traces privées) :
`cargo test --test golden` ; `ord-bench check` ; `ord-bench run --offline` (tous les cas de
calibration du manifest) ; `ord-bench compare` contre `baseline.json`. À lancer avant toute PR qui
touche le cœur ; le `results.json` produit est commité dans `bench/reports/` et contrôlé par le job
`bench-report`. Sans cache, les tests golden sont ignorés (et non verts). La CI ne peut pas vérifier
les empreintes des traces privées : elle fait confiance au `results.json` (`private_present`).

Mise en œuvre (incrément 6) : `ord-bench bench-report` retient le dernier `results.json` de
calibration de `bench/reports/` dont `rules_fingerprint`, `manifest_sha256`, `criteria_sha256` et
`bench_rules_sha256` sont les empreintes courantes (échec s'il n'y en a aucun) ; `bench/cache.lock`
et `bench/baseline.json` sont vérifiés s'ils existent, sinon le rapport dit que le contrôle est sauté
(ni l'un ni l'autre n'est commité à ce jour : `fill-cache` écrit `cache.lock` dans le cache).
`ord-bench compare` donne les bascules de verdict par identifiant et par mode, et refuse de comparer
deux campagnes aux empreintes de banc différentes ; la tolérance par métrique (§4) n'est pas encore
appliquée. `bench-offline` est `scripts/bench-offline.sh`, aussi déclarable dans `ci.yml` sur un
runner local (`workflow_dispatch`, étiquette `ord-bench`), jamais sur un runner GitHub. Le job
`docker` construit l'image et sonde `/health/live` par `docker exec … healthcheck`, conteneur sans
réseau.

`live.yml`, planifié chaque semaine et à la demande, **non bloquant** : `cargo test --features
live-tests`, `ord-bench latency --profile cold` sur `sample.gpx`, rapport en artefact. Ni `judge` ni
les cas perso ne tournent en CI.

## 7. Mesure avant la V1.x : MNS sur la trace ou sur la géométrie OSM du pont

Question, cas, mesure et **règle de décision écrite avant** : `bench/README.md` §5 (doc 01 §11.2).
Ce chapitre ne garde que la mise en œuvre.

- `ord-bench dsm-placement` calcule, sur le cache de banc, la méthode **A** (MNS aux stations de la
  trace, celle du MVP) et la méthode **B** (stations projetées orthogonalement sur l'axe OSM densifié
  à 5 m), toutes deux suivies de la médiane sur 25 m, puis les métriques du banc §5 : écart latéral
  par passage et classe (0–5, 5–15, > 15 m), RMSE et erreur max sur le tablier, erreur aux culées,
  test de robustesse aux décalages latéraux de 5, 10 et 15 m.
- Il passe les ouvrages par le **constructeur de tablier du service** (T04 §3.2), mêmes options, pour
  mesurer aussi les effets propres au service : tabliers abandonnés (< 2 ancres), nouveaux
  `structure_slope_implausible`. Ces deux compteurs sont des diagnostics : la décision suit la seule
  règle du banc.
- Calibration d'abord, puis `dsm-placement --judge` **une seule fois** sur la série tenue à l'écart
  désignée par le banc ; résultat dans `bench/judgements.md`, décision consignée dans doc 01 §11.
- En mode B, l'invariant 8 (même altitude dans les deux sens) est revérifié par `proptest` (§10.5).

## 8. Interfaces

Le module qualité n'est pas un composant du service (doc 01 §4) : il vit dans le crate de dev
`ord-testkit` (T01 §2), les `tests/` de chaque crate, `tools/ord-bench` et les données de `bench/`.

**Expose** (aux tests d'intégration et à `ord-bench`) :

```rust
pub enum Serie { Calibration, TenueALEcart }                 // manifest.yaml, champ `serie`
pub enum CaseSource { Vcyclist, OsmSurface, Perso }
pub struct ManifestEntry { pub id: String, pub source: CaseSource, pub groupe: Option<String>,
    pub serie: Serie, pub fichier: PathBuf, pub sha256: Option<[u8; 32]>, pub hors_git: bool,
    pub qualification: Option<BaroQualification> }        // perso : verdict de qualify_baro.py (Q1)
pub struct Manifest { pub entries: Vec<ManifestEntry>, pub ouvrages_tenus: BTreeSet<String>,
    pub sha256: [u8; 32] }                                    // refuse une empreinte fausse
pub struct Ouvrage { pub id: String, pub kind: String, pub chain: Vec<i64>, pub associees: Vec<i64>,
    pub longueur_m: f64, pub points: BTreeMap<String, (f64, f64)>,
    pub milieu_mns_mediane_25m: Option<f64>,                  // ouvrages.yaml ; référence E3 de cohérence
    pub indices: BTreeMap<String, IgnPointValues> }           // ouvrages.yaml ; ponctuels, jamais une référence E3
pub enum StationKind { Terrain, Deck, OffDeck, Tunnel, Balcony, Ground, Overhead, Alongside, Excluded, Margin }
pub struct TruthProfile { pub route: String, pub step_m: f64, pub s: Vec<f64>, pub lat: Vec<f64>,
    pub lon: Vec<f64>, pub z: Vec<Option<f64>>, pub mnt: Vec<Option<f64>>, pub kind: Vec<StationKind>,
    pub strava_offset_m: Option<f64> }                        // vcyclist truth/testN.csv
pub struct StructureTruth { pub route: String, pub kind: StructureTruthKind, pub i0: usize, pub i1: usize,
    pub z0: f64, pub z1: f64, pub zmax: f64, pub name: String }
pub struct RecordedPoint { pub lat: f64, pub lon: f64, pub ele_baro: f64 }
pub struct RecordedTrack { pub id: String, pub groupe: String, pub serie: Serie, pub points: Vec<RecordedPoint> }
pub struct BaroOffset { pub global_m: f64, pub local_m: Option<f64> }   // bench §2, étape 4
pub enum ReferenceKind { Baro, VcTruth, MnsMedian25m }
pub struct MetricId(pub &'static str);                        // "E1" … "Q1", ceux de bench §3
pub struct Criterion { pub id: MetricId, pub blocking: bool, pub threshold: Threshold } // criteria.yaml
pub struct BenchRules { /* MATCH_MAX_M, emprise, fenêtre E5 */ }          // Serialize, haché
pub enum Verdict { Pass, Fail { measured: f64 }, Reported, Skipped }
pub struct RulesFingerprint(pub [u8; 32]);  pub struct DataFingerprint(pub [u8; 32]);
pub struct BenchResults { pub campagne: CampaignMeta, pub cas: Vec<CaseResult>,
    pub verdicts: BTreeMap<String, Verdict> }                // sérialisé en results.json (§3.2)
pub struct SyntheticTerrain { /* sinusoïdes, marches, ouvrages */ }   // impl TerrainReader (T02 §6.3)
pub struct ScriptedServer { pub url_template: String, pub hits: Arc<AtomicUsize> } // OS/tests/tiles.rs
pub fn offline_sources(cache_dir: &Path) -> Sources;   // TileReader en mode Offline ; les tests
// d'ord-server montent `ord_server::router(AppState::new(offline_sources(…)))` (pas de cycle de crates)
```

**Consomme** (propriétaires : carte des types, T01 §0) :

- Accès données (T02 §2) : `TileSource` (`fetch(TileKey, Option<&EntryMeta>) -> Result<Fetched,
  FetchError>`), `TileReader<C>` et son mode `Offline`, `TileStore<T>`, `TileOutcome::{Data, Empty,
  Absent, Failed}`, `TileKey`, `SourceId`, `DataLog` (compteurs de lectures par source) ;
- Adaptateur élévation (T02 §6.3) : `TerrainReader`, `TerrainSample`, `DsmSample`, `TerrainOrigin` ;
- Orchestrateur (T01 §5) : `PreparedRoute`, `Sources`, `Orchestrator::run(ValidatedRequest,
  RequestCtx) -> Result<RouteReport, PipelineError>` ;
- Fusion du profil (T04) : `ElevationProfile`, `EleMethod`, `StructureOutcome`, `ElevationRules`
  (dont les options du constructeur de tablier), `GainParams` ;
- Classification (T03 §7) : `WayAttrs`, `SchemaCapabilities`, `classify`, `SurfaceClass`,
  `SurfaceSource`, `WayKind`, `StructureClass`, `RuleSet` ;
- Points de rupture (T04 §6) : `OutputPoint`, `BreakpointReason`, `BreakpointRules` ;
- API HTTP (T01 §4, §10) : `ord_server::router(AppState) -> axum::Router`,
  `ord_server::openapi() -> utoipa::openapi::OpenApi`, `Problem` (RFC 9457), les DTO de doc 02 ;
- Versions (T01 §9) : `RULES_VERSION`, `Rules: Serialize` (haché en `RulesFingerprint`), `DataVersions`.

## 9. Crates

| Crate | Rôle | Pourquoi |
|---|---|---|
| `proptest` | property-based, réduction des cas | réduction automatique et fichiers de régression commitables |
| `rstest` | tests en table (classification, statuts) | table doc 02 §6 lisible ligne à ligne |
| `insta` (features `json`, `redactions`) | golden files | rédaction de `request_id`/`duration_ms`, revue des diffs (`cargo insta review`) |
| `tokio` (`test-util`) | temps virtuel | cadence, relances, budget sans attente réelle |
| `tower`, `http-body-util` | appel du routeur en mémoire | déjà le modèle d'osm-surface |
| `jsonschema` | validation des réponses contre l'OpenAPI | contrat vérifié sur des sorties réelles |
| `gpx` | lecture des GPX (copies vcyclist et osm-surface, traces perso et recalées) | seule dépendance de lecture de traces, confinée à `ord-bench` |
| parseur YAML serde (à choisir, `serde_yaml` n'étant plus maintenu) ; `csv`, `serde_json` | `manifest.yaml`, `ouvrages.yaml`, `criteria.yaml` ; vérités vcyclist ; `results.json` | formats du banc et de vcyclist |
| `sha2`, `hdrhistogram`, `clap` | empreintes ; p50/p95 de latence ; CLI d'`ord-bench` | gel, non-régression, mesure |
| `criterion` (optionnel) ; outils `cargo-nextest`, `cargo-deny`, `cargo-fuzz`, `oasdiff` | micro-bancs ; exécution, licences, fuzz, rupture d'API | hors cœur |

## 10. Points de tension

1. **Vérité circulaire sur les tabliers.** La vérité `B` de vcyclist est le MNS IGN lui-même : E3
   mesuré contre `B` est une cohérence ; les traces barométriques personnelles sont la seule
   référence indépendante (§3.5). Un flux `altitude` Strava peut en outre avoir été remplacé par le
   fond Strava (« corriger l'altitude ») : à exclure au manifeste, faute de marqueur fiable dans les
   streams (§11).
2. **Déterminisme flottant** : l'exigence « même réponse » (doc 02 §6) suppose une sérialisation
   arrondie ou une arithmétique identique entre machines (FMA x86/ARM). Tranché par T01 §9 (arrondis
   de l'assembleur) ; reste un cas rare de valeur à cheval sur un arrondi, d'où la tolérance d'une unité.
3. **S1 ≥ 95 % et E3 ≥ 90 % des ponts** sont des paris sur l'apport d'OSM : si l'appariement rate les
   tabliers étroits, le seuil sera manqué et rapporté, pas abaissé (bench §4, dernier paragraphe).
4. **L1 dépend du réseau** (IGN, 40 req/s/IP, pas de SLA) : critère mesuré en live, donc non
   reproductible et non bloquant en CI. Un p95 à froid exige plusieurs passages caches vidés, donc
   autant de téléchargements complets.
5. **Méthode B (§7)** réutilise le constructeur de tablier sur une géométrie que la trace ne suit pas :
   l'invariant 8 (même altitude dans les deux sens) doit être revérifié par `proptest` dans ce mode.

## 11. Reste à fournir

Ce que le banc attend encore de l'utilisateur (bench §6). Rien n'est tranché ici. Les décisions du
2026-10-30 (E2 indicatif, traces recalées, bornes d'entrée, fixtures copiées, traces perso jamais
versionnées, référence R3, séries par groupe, référence E3, cache de banc) sont dans doc 01 §10.

- **Qualification hors France** : qualifier `perso-37..39` contre Mapterhorn, faute de MNT IGN. Il faut
  aussi choisir le MNT de référence de cette qualification (`perso-38` et `perso-39` n'ont aujourd'hui
  aucun indicateur).
- **Licences** : conditions de réutilisation de Stadia (référence d'osm-surface, map matching et
  `trace_attributes`). osm-surface est sous Apache 2.0 et ses `.route.json` sont expurgés à la copie
  (fait).
- **S1 sur les traces recalées** : M1 s'y mesure (doc 01 §10) ; le statut de S1 n'est pas tranché
  explicitement.
- **Points divers** : accès vélo réel des ponts du corpus (Krk, Venise, Cheviré `bicycle=no`) ; source
  GLO-30 réelle de Krk ; RGE ALTI ou MNT LiDAR au tunnel d'Arbois ; streams Strava dont l'altitude
  aurait été « corrigée » par le fond Strava (§10.1) ; nouvelle série tenue à l'écart après le jugement du MVP.
- **Ajouts à `bench/`**, dans un commit séparé, avant le premier passage : `criteria.yaml` (§3.1, avec
  le critère bloquant d'E2) ; copie de `strava.gpx` de vcyclist dans `bench/data/` et entrée au
  manifest, si le test de méthode du §3.4 doit être tracé par empreinte.
