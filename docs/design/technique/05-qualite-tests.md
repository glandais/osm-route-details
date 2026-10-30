# 05 — Qualité et tests (conception technique)

> Statut : conception technique. Exigences : [01](../01-conception-generale.md) (§3–§6, §8.5, §10,
> §11.2) et [02](../02-contrat-interface.md). Aucune exigence n'est modifiée ; les écarts sont dans
> « Points de tension » (§10). Les références `chemin:ligne` sont relatives à la racine de vcyclist
> (`VC/`, 6.11.0, `develop` à jour au 2026-10-30) ou d'osm-surface (`OS/`, commit `44e5da6`).
> **[mesuré]** : chiffre publié par l'un de ces projets ; **[à calibrer]** : valeur de départ, fixée
> par la série de calibration, jamais par la série tenue à l'écart.

## 1. Principes

1. **Le cœur est pur.** Classification, fusion du profil, détecteurs, sélection des points de
   rupture et D+ prennent des tableaux et renvoient des valeurs : testés sans réseau, disque ni async.
2. **Hors ligne par défaut.** `cargo test` ne sort jamais de la machine : le port d'Accès données est
   servi par un cache pré-rempli (`Offline`, une tuile absente est une erreur) ou un faux serveur local.
3. **Une mesure sans critère écrit avant ne vaut rien** (exploration 01 §6.4) : critères et listes
   tenues à l'écart sont commités, avec empreintes SHA-256, **avant** le premier passage du code jugé.
4. **Toute différence de sortie s'explique** par `rules_version` ou `data_versions` (doc 02 §6, §4).
5. **Aucune dépendance de fichiers entre projets** (doc 01 §10, Q11) : les bancs de vcyclist et
   d'osm-surface sont **copiés une fois**, gelés et tracés, jamais lus à leur emplacement d'origine.

## 2. Stratégie de tests

| Niveau | Emplacement | Données | Réseau | Lancé par |
|---|---|---|---|---|
| Unitaires purs | `src/**` (`#[cfg(test)]`) | littéraux, générateurs | non | `cargo test`, CI |
| Property-based | `src/**`, `tests/prop_*.rs` | `proptest` | non | `cargo test`, CI |
| Décodage | `tests/decode.rs` | tuiles réelles commitées (`tests/fixtures/tiles/`) | non | `cargo test`, CI |
| Intégration hors ligne | `tests/api_*.rs` | cache pré-rempli + faux serveur local | boucle locale seulement | `cargo test`, CI |
| Contrat HTTP/OpenAPI | `tests/contract.rs` | `openapi.json` commité | non | `cargo test`, CI |
| Golden files | `tests/golden.rs` | `tests/golden/` + cache de banc local (§5.3) | non | à la main, en local (`bench-offline`, §6) ; snapshots commités |
| Banc qualité | binaire `ord-bench` | corpus `bench/corpus/` + cache de banc local | non (sauf `fill-cache`) | en local (calibration, jugement) ; rapport commité, vérifié en CI (§6) |
| Live | `tests/live_*.rs`, feature `live-tests` | sources réelles | oui | à la main, CI planifiée non bloquante |

### 2.1 Unitaires purs du cœur

Par composant (noms de doc 01 §4), les cas qui **doivent** exister, en plus des cas nominaux :

- **Orchestrateur / préparation** : doublons consécutifs fusionnés, non consécutifs gardés (boucle,
  aller-retour) ; indices d'entrée conservés après fusion ; densification à pas métrique sur un
  segment de 0 m, de 1 cm, de 200 km ; antiméridien et latitude limite Web Mercator rejetés (400) ;
  taille **densifiée** au-delà de la borne → 413 (règle de vcyclist, `VC/api/README.md` §limites).
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
7. en CI, `oasdiff breaking` entre le contrat de `main` et celui de la branche : toute rupture dans
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

### 2.7 Tests live (feature `live-tests`)

Compilés seulement avec `--features live-tests` ; aucun n'est requis pour fusionner. User-Agent
identifiable (explorations 05 et 06). Contenu :

- points de contrôle de vcyclist sur Mapterhorn (`VC/elevation/src/jvmTest/.../ElevationProviderIntegrationTest.kt:24-56`) :
  Mont Blanc (45,8326 ; 6,8652) 4 805 ± 50 m, mer Morte (31,5 ; 35,5) −430 ± 50 m, Badwater (36,250 ; −116,832) −85 ± 50 m ;
- IGN WMS-R : un `GetMap` BIL z15 connu (MNT et MNS) renvoie 262 144 octets et la valeur figée en §2.3 à 0,05 m ;
- Mapterhorn : 404 à z13 au Maroc (GLO-30 seul, exploration 05 §1) ; `coverage-index.pmtiles` lisible, ETag noté ;
- VersaTiles : `streets` porte `kind`, `bridge`, `tunnel`, `surface`, `tracktype` ; revalidation ETag → 304.

## 3. Banc qualité

### 3.1 Corpus

| Série | Origine (copiée dans `bench/corpus/`) | Vérité | Sert à mesurer |
|---|---|---|---|
| `vc-routes` : `test1` Saint-Nazaire 10 km, `test2` Nantes 25 km (16 ponts de Loire), `test3` Mauves 10 km, `test4` Vercors 111 km (4 tunnels) | `VC/docs/deck-strava-data/test1..4.gpx` | `VC/docs/deck-strava-data/analysis/truth/test*.csv` : pas de 5 m, `s,lat,lon,truth,mnt,kind`, kind `T` MNT, `B` tablier (MNS médiane 25 m), `b`, `N` droite entre portails, `S` balcon (Strava − offset) ; en-tête `strava_offset_m` | altitude sur ouvrages et sol, D+ |
| `vc-structures` : 21 ponts, 4 tunnels, 6 balcons, 15 ponts manqués | `VC/.../truth/structures.csv` (garde-fous : `VC/engine/src/jvmTest/.../ElevationBenchProbeTest.kt:188-217`, `STRUCTURE_COUNTS` l. 3466) | plages `i0..i1`, `z0`, `z1`, `zmax`, nom | détection, erreur par ouvrage |
| `vc-decks` : 16 négatifs `n01..n16`, 8 positifs `p01..p08`, 9 ponts tenus à l'écart `p09..p17` (11 spans) | `VC/docs/deck-strava-data/controls/*.gpx`, `truth/controls/{index,spans}.csv` | kind par station `G O L X x D`, way OSM | rappel ponts, faux positifs |
| `vc-tunnels` : 8 réglage `tt*`, 13 négatifs raides `tn*`, 7 tenus à l'écart `th*`, 4 négatifs tenus à l'écart `hn*` | `VC/.../controls/tunnels/`, `truth/tunnels/` ; empreintes `VC/.../analysis/tunnel-judge.sha256` | kind `G O L X x N` | rappel tunnels, faux positifs |
| `os-stadia` : 3 parcours gravel autour de Nantes, 185 km | `OS/testdata/*.route.json` (référence Stadia projetée par tribly, `OS/src/bin/eval.rs:1-8`) | plages `PAVED`, `PAVED_ROUGH`, `GRAVEL`, `DIRT`, `PATH`, `UNKNOWN` | accord revêtement, appariement, latence |
| `vc-baro-proxy` : `strava.gpx` 21,1 km, 1 Hz, Pyrénées | `VC/demo/public/gpx/strava.gpx` | flux `<ele>` Strava (proxy barométrique, `elevation.md:376-388`) | méthode de comparaison baro |
| `user-baro` : traces barométriques personnelles | **à fournir** (§3.3), hors dépôt public | baromètre recalé | D+, ouvrages hors Loire-Atlantique et Vercors |
| `synthetic` : demi-tour sur pont, pont au bord de tuile, trace hors réseau, points espacés de 2 km autour d'un viaduc | généré | par construction | cas limites de doc 02 §5.3 |

Chaque série a un `bench/corpus/<série>/MANIFEST.toml` : chemin source, commit d'origine, licence
(vcyclist : Apache-2.0 ; GPX routés : © OpenStreetMap contributors, ODbL ; osm-surface : dépôt
personnel), SHA-256 de chaque fichier, **rôle** de chaque entrée (`calibration`, `negative`,
`holdout`, `holdout_negative`). Le chargeur refuse un fichier dont l'empreinte ne correspond pas.

**Attention, vérité circulaire** : la vérité `B` de vcyclist est la médiane 25 m du MNS IGN, c'est-à-dire
la donnée que lit notre méthode `dsm_deck`. L'erreur mesurée sur `B` est donc une **cohérence**, pas
une exactitude. La seule référence indépendante sur les tabliers est Strava (1,1 m RMS à Saint-Nazaire,
≤ 1,8 m max à Nantes, exploration 01 §6.2) puis les traces barométriques ; les rapports affichent les
deux colonnes.

### 3.2 Discipline des séries tenues à l'écart

Reprise de vcyclist (`ElevationBenchProbeTest.kt:68-71`, `TunnelBenchProbeTest.kt:57-63`) :

- `bench/criteria.md` (seuils du §3.5) et `bench/holdout.lock` (identifiants + SHA-256 des fichiers de
  vérité des séries `holdout*`) sont commités **seuls**, avant tout passage du code jugé ;
- `ord-bench run` **exclut** les rôles `holdout*` avant de lire une seule tuile ; seul
  `ORD_JUDGE=1 ord-bench judge --series <s>` les lit, vérifie les verrous, imprime et ajoute le
  résultat à `bench/judgements.md` (date, commit, `rules_version`, empreinte des données) ;
- une série jugée est **consommée** : un réglage fait après la lecture la reclasse en calibration, et
  une nouvelle série tenue à l'écart doit être constituée par une règle écrite (« le plus long pont de
  25–80 m au-dessus de l'A4 », modèle de `controls/index.csv`) ;
- les séries `p09..p17` et `th*/hn*` sont « dépensées » pour vcyclist, **pas pour nous** : nous ne les
  avons jamais lues. Elles restent tenues à l'écart pour les heuristiques V1 et pour les seuils des
  points de rupture.

### 3.3 Traces barométriques personnelles

**Emplacement.** `bench/private/baro/` par défaut, surchargé par `ORD_BARO_DIR`. `bench/private/` est
déjà ignoré par git (`bench/.gitignore`) : un parcours est une donnée personnelle (doc 01 §8.3). Un
`MANIFEST.toml` par répertoire, un bloc `[[trace]]` par trace : `id` stable sans lieu de domicile,
`file`, `format` (`fit` | `gpx` | `strava_streams`), `device` (le décalage et la dérive en
dépendent), `role` (`calibration` | `holdout`, fixé avant tout passage), `notes` (météo, front
orageux).

- **FIT** (`fitparser`) : messages `record` → `timestamp`, `position_lat/long` (semicercles),
  `enhanced_altitude` sinon `altitude`, `distance` ; refus si l'altitude manque.
- **GPX** (`gpx`) : `trkpt` avec `<ele>` et `<time>` ; accepté seulement si `device` est déclaré
  barométrique (un GPX ne le dit pas).
- **Flux Strava** (JSON des streams `latlng`, `altitude`, `distance`, `time` d'une activité) : piste
  amorcée dans `bench/private/strava/` (inventaire et pré-tri `bench/tools/strava_scan_dplus.py`, qui
  compare le D+ Strava au D+ RGE ALTI pour repérer les périodes où le baromètre décroche). Une
  activité hors de la tendance de ce pré-tri est exclue **avant** tout passage du service.
- Normalisation en `RecordedTrack` (§8) : points sans position supprimés, pauses (vitesse < 1 km/h
  pendant > 30 s) fusionnées, découpage en morceaux ≤ 200 km pour respecter les bornes de l'API.

**Requête envoyée.** La trace enregistrée est rééchantillonnée à 10 m puis envoyée en `points`, mode
`distance`, pas de 5 m. C'est une **trace enregistrée**, hors du périmètre validé de v1 (doc 01 §2.3) :
le taux d'appariement est mesuré et reporté à part (§10).

**Comparaison.**

1. **Grille commune** : les deux profils (baro et service) sont ramenés sur la distance curviligne de
   la trace, pas de 5 m.
2. **Retard du capteur** : décalage `τ ∈ [−50 m, +50 m]` qui maximise la corrélation des dérivées sur le
   sol mesuré (le baromètre filtre la pression sur quelques secondes).
3. **Décalage d'altitude** : `offset = médiane(baro − service)` sur les stations « sol sûr » : origine
   `ign_lidar_hd`, `ele_method = dem`, à plus de 100 m de tout ouvrage, pente < 8 % sur 50 m. C'est la
   méthode de `strava_offset_m` de vcyclist (en-tête de `truth/test*.csv`, offsets de −0,52 à +1,48 m).
4. **Dérive** (pression qui change pendant la sortie) : même médiane sur des fenêtres glissantes de
   2 km ; si l'écart entre fenêtres dépasse 3 m **[à calibrer]**, on utilise l'offset **local** : médiane
   sur 300 m de sol sûr de part et d'autre de chaque ouvrage, et on marque la trace `drifting` pour le
   D+ global.
5. **Métriques** : D+ des deux profils avec le **même** preset (`dem` 3 m / 30 m et `barometric`
   2 m / 15 m, `elevation.md:205-216`), rapport `D+service / D+baro` ; erreur sur ouvrage (station par
   station, offset local) ; RMS sur sol sûr. Le D+ est insensible à l'offset, pas à la dérive.

Validation de la méthode avant les traces personnelles : `strava.gpx` doit redonner le rapport publié
par vcyclist (714 / 637 m ≈ 1,12 au preset `dem`, `elevation.md:319-321`) à ± 3 points près, sources
identiques.

### 3.4 Métriques

| Métrique | Définition | Séries |
|---|---|---|
| Erreur d'altitude sur ouvrage | par ouvrage : RMS et max de `ele − vérité` sur les stations `B` (ponts) ou `N` (tunnels), appariement trace ↔ vérité à 30 m max (`MATCH_MAX_M`, `ElevationBenchProbeTest.kt:3429`) ; colonne Strava à côté pour les ponts | `vc-routes` |
| Erreur au sol | RMS sur les stations `T` hors 35 m des ouvrages, et nombre de stations à > 15 % sur 50 m (`STEEP_GRADE`) | `vc-routes` |
| D+ / vérité | D+ preset `dem` sur notre profil densifié / même calcul sur la vérité | `vc-routes` |
| D+ / baro | §3.3 | `vc-baro-proxy`, `user-baro` |
| Rappel ouvrages | part des ouvrages de vérité dont ≥ 80 % des stations sont en `structure = bridge/tunnel` **et** corrigées (`dsm_deck` ou `interpolated_structure`) | `vc-structures`, `vc-decks`, `vc-tunnels` |
| Faux positifs | stations `G`, `O`, `L` avec `structure ≠ ground` ou `|ele − MNT| > 1 m` ; un ouvrage « faux » = un run contigu ≥ 15 m | négatifs |
| Accord revêtement | longueur où `paved` ⇔ référence ∈ {`PAVED`, `PAVED_ROUGH`}, sur la longueur où aucun des deux n'est inconnu ; plus le calcul d'osm-surface (inconnu compté « non revêtu », `OS/tests/api.rs:183-206`) pour comparer à 98,1–99,3 % | `os-stadia` |
| Couverture d'appariement | `quality.matched_ratio` ; longueur `unmatched_range` | toutes |
| Correction incertaine | nombre de `structure_uncorrected`, `structure_slope_implausible` | toutes |
| Latence | p50/p95 (`hdrhistogram`) en **froid** (cache disque vide, serveur réel — live seulement), **chaud disque** (LRU vide), **chaud mémoire** ; tuiles et octets lus par source | `os-stadia`, `test4`, `sample.gpx` 130 km |

### 3.5 Seuils d'acceptation du MVP

À écrire tels quels dans `bench/criteria.md` avant le premier passage. Le MVP est livrable si **tous**
les seuils bloquants passent sur la calibration, puis si le jugement unique des séries tenues à l'écart
est publié (qu'il passe ou non : un échec est rapporté, pas réglé).

| # | Critère | Seuil | Origine |
|---|---|---|---|
| A1 | Ponts de Loire à Nantes (`test2`), `dsm_deck` | max ≤ 2 m sur `B` | cible de vcyclist (`ElevationBenchProbeTest.kt:614-621`) **[mesuré]** |
| A2 | Pont de Saint-Nazaire | max ≤ 5 m sur `B` ; vcyclist obtient 0,54 m | idem **[mesuré]** |
| A3 | Rappel des 21 ponts de `structures.csv` | 21/21 (vcyclist 18/21 en calibration) | OSM donne l'emprise **[à calibrer]** |
| A4 | Ponts `p09..p17` (11 spans, jugement unique) | ≥ 9/11 corrigés ; vcyclist 4/11 | ligne de base à battre (doc 01 §8.5) **[à calibrer]** |
| A5 | Tunnels Vercors (`test4`) | 4/4 corrigés, max ≤ 2 m sur `N` ; Grands Goulets ≤ 2 m (vcyclist 0,9 m) | `elevation.md:578-597` **[mesuré]** |
| A6 | Tunnels tenus à l'écart `th*` | ≥ 6/7 (vcyclist 6/7) | **[mesuré]** |
| A7 | Faux positifs sur tous les négatifs (`n*`, `tn*`, `hn*`) | 0 station `G/O/L` déplacée de > 1 m | vcyclist : 0 faux positif **[mesuré]** |
| A8 | Erreur au sol `T`, `test1..3` | RMS ≤ 1,0 m ; `test4` ≤ 1,5 m | vcyclist IGN 0,59–0,78 m (`exploration 01 §6.1`) **[à calibrer]** |
| A9 | D+ / vérité, `test1`, `test2`, `test4` | ∈ [0,90 ; 1,15] ; `test3` rapporté, non bloquant (bruit MNT, `elevation.md:268-271`) | vcyclist IGN 1,08 / 0,95 / 1,05 **[mesuré]** |
| A10 | Accord revêtu / non revêtu, `os-stadia` | ≥ 97,5 % par parcours (calcul osm-surface) | 98,1–99,3 % **[mesuré]** |
| A11 | Couverture d'appariement, `os-stadia` | `matched_ratio` ≥ 98 % | inconnu 0–1,3 % **[mesuré]** |
| A12 | Latence chaud mémoire, 180 km / 15 000 points | p95 ≤ 500 ms | typologie seule < 160 ms (`OS/README.md:250`) + élévation **[à calibrer]** |
| A13 | Latence froide, 130 km (live) | ≤ 25 s, sous le budget de requête | vcyclist 19 s à froid, 66 Mo (`exploration 01 §6.3`) **[mesuré]** |
| R1 | D+ / baro sur `user-baro` | rapporté, **non bloquant** au MVP (doc 01 §1 : pas de promesse) ; aucun ouvrage avec un creux > 5 m sous le baro recalé localement | **[à calibrer]** |

## 4. Non-régression : `rules_version` et `data_versions`

- **Empreinte des règles** : les tables de classification, les options des détecteurs, les seuils des
  points de rupture, les presets de D+ et l'adaptation GLO-30 sont des structures `Serialize`. Leur
  sérialisation canonique hachée (SHA-256) donne `RulesFingerprint`. Le fichier `rules/VERSIONS`
  associe chaque `rules_version` à son empreinte. Un test échoue si l'empreinte courante n'y figure
  pas : **changer une règle oblige à monter `rules_version`**.
- **Empreinte des données** : `DataFingerprint` = SHA-256 de la liste triée `(espace de noms, z/x/y,
  SHA-256 du contenu)` des tuiles du cache de banc, plus les identifiants de `data_versions`. En
  mode `Offline`, `data_versions` est donc exact, même pour l'IGN.
- **Règle de CI** : si un snapshot golden ou un résultat de banc change, alors `rules_version` **ou**
  l'empreinte des données doit avoir changé dans la même PR. Sinon, c'est une régression du code (ou
  un non-déterminisme) : échec.
- **Référence de banc** : `bench/baseline.json` (résultats de la dernière version acceptée, avec ses
  deux empreintes). `ord-bench compare` signale toute métrique qui se dégrade au-delà d'une tolérance
  (0,2 m sur une erreur d'ouvrage, 1 point d'accord, 2 points de rapport de D+ **[à calibrer]**) et tout
  critère du §3.5 qui bascule.
- **Changement de données** (nouvelle release VersaTiles, IGN publié sur une nouvelle zone,
  rafraîchissement du cache) : nouveau cache, nouveau `baseline.json`, golden régénérés dans une PR
  dédiée sans changement de code, rapport de diff joint.

## 5. Outillage

### 5.1 Binaire `ord-bench`

Membre du workspace (`tools/ord-bench`, T01 §2 ; données dans `bench/`), dépend d'`ord-pipeline`,
`ord-server` et `ord-testkit`, pas l'inverse : les crates de
lecture GPX/FIT et de rapport n'entrent pas dans l'image de production. Comme l'`eval` d'osm-surface
(`OS/src/bin/eval.rs:1-8`), il appelle le service **en processus** par défaut, ou une instance
distante avec `--url`.

| Sous-commande | Rôle |
|---|---|
| `fill-cache --corpus <série>` | live : remplit (ou régénère) le cache de banc local (`~/.cache/osm-route-details/bench`, §5.3), écrit `bench/cache.lock` (empreinte, taille, date) |
| `run --corpus <séries> --offline` | calibration : calcule les métriques, exclut les rôles `holdout*` |
| `judge --series <s>` | exige `ORD_JUDGE=1`, vérifie `holdout.lock`, jugement unique (§3.2) |
| `baro ingest` / `baro compare` | §3.3 ; lit `ORD_BARO_DIR` |
| `compare <a.json> <b.json>` | diff de deux passages, verdict par critère |
| `latency --profile cold\|warm-disk\|warm-mem` | §3.4 |
| `dsm-placement` | mesure du §7 |

### 5.2 Rapports

Chaque passage écrit `bench/reports/<date>-<rules_version>/` : `results.json` (machine, lu par
`compare` et la CI), `report.md` (tableaux comme `OS/eval/report.md` : synthèse, critères, pires
écarts, ouvrages non corrigés, matrices de confusion du revêtement), et un CSV par ouvrage
(`s, vérité, strava, nous, méthode`) pour tracer les profils. Les rapports `user-baro` n'écrivent
**aucune coordonnée** : distances, identifiants de trace et noms d'ouvrage OSM seulement.

### 5.3 Cache de banc

Coût à froid estimé d'après vcyclist (`ElevationBenchProbeTest.kt:64-67`) : ≈ 190 tuiles IGN
(48 Mo) et 20 Mapterhorn pour les 4 routes, autant de MNS, ≈ 178 BIL (47 Mo) pour les contrôles de
ponts, plus les tunnels ; ≈ 11 Mo de VersaTiles pour 185 km (`OS/README.md:177`) : **250 à 350 Mo**.

**Décision** (doc 01 §10) : le cache de banc est **local, hors git**, dans
`~/.cache/osm-route-details/bench` par défaut (`ORD_BENCH_CACHE` ou `--cache-dir` pour le déplacer),
et **régénérable** par `ord-bench fill-cache`. Il n'est ni archivé ni stocké à distance (pas
d'asset de release, pas de stockage objet) : aucune redistribution des tuiles (Etalab, ODbL, sources
Mapterhorn), donc aucune question de licence. `bench/cache.lock` (commité) n'en garde que
l'empreinte `DataFingerprint`, la taille et la date de remplissage. Une régénération lit des données
vivantes : si l'empreinte change, c'est un **changement de données** au sens du §4 (nouveau
`baseline.json`, golden régénérés dans une PR dédiée).

## 6. CI (GitHub Actions)

`ci.yml`, sur chaque PR et sur `main` :

| Job | Commandes | Bloquant |
|---|---|---|
| `fmt` | `cargo fmt --all --check` | oui |
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | oui |
| `test` | `cargo nextest run --workspace --locked` puis `cargo test --doc` ; `proptest` à 256 cas | oui |
| `contract` | test §2.5 + `oasdiff breaking` contre `main` | oui |
| `deny` | `cargo deny check` (licences, avis de sécurité, doublons) | oui |
| `bench-report` | sans cache : vérifie que le `results.json` commité par la PR porte le `RulesFingerprint` courant et l'empreinte de `bench/cache.lock`, puis `ord-bench compare` contre `baseline.json` et règle du §4 | oui |
| `docker` | construction de l'image, `/health/live` sur le conteneur | oui sur `main` |

**`bench-offline` en local** (la CI n'a pas le cache, §5.3) : `cargo test --test golden` ;
`ord-bench run --offline --corpus vc-routes,vc-structures,vc-decks,vc-tunnels,os-stadia,synthetic` ;
`ord-bench compare` contre `baseline.json`. À lancer avant toute PR qui touche le cœur ; le
`results.json` produit est commité dans `bench/reports/` et contrôlé par le job `bench-report`. Sans
cache, les tests golden sont ignorés (et non verts).

`live.yml`, planifié chaque semaine et à la demande, **non bloquant** : `cargo test --features
live-tests`, `ord-bench latency --profile cold` sur `sample.gpx`, rapport en artefact. Ni `judge` ni
`baro` ne tournent en CI.

## 7. Mesure avant la V1.x : MNS sur la trace ou sur la géométrie OSM du pont

Question (doc 01 §11.2) : quand la trace passe à côté du tablier, lire le MNS **aux positions de la
trace** (variante A, celle du MVP) ou **sur la géométrie du way `bridge`** (variante B) donne-t-il le
meilleur profil ?

**Variante B** : pour chaque ouvrage apparié, stations tous les 5 m le long de la géométrie OSM
recollée ; chaque station de la trace prend la station OSM de même **fraction de longueur** entre les
extrémités de l'ouvrage ; même constructeur de tablier, mêmes options.

**Protocole.**

1. **Critère de décision écrit avant**, dans `bench/criteria.md` : B est retenue si, sur la série tenue
   à l'écart, la médiane des RMS par pont baisse d'au moins 0,3 m pour un décalage latéral ≥ 6 m,
   **sans** dégrader de plus de 0,2 m à décalage nul, **sans** augmenter le nombre de tabliers
   abandonnés (< 2 ancres) et **sans** nouveau `structure_slope_implausible` **[à calibrer]**. Sinon A
   reste.
2. **Corpus** : calibration = ponts de `test1`, `test2` et `p01..p08` ; tenue à l'écart = `p09..p17` et
   les ponts des traces `user-baro`. Les tabliers étroits (`p04-langeais`, `p12-tonnay-charente`) et les
   ponts doublés d'un ouvrage parallèle (`p06-normandie`, 60 stations `L` ; `n01`) sont les cas
   discriminants.
3. **Traces décalées synthétiques** : pour chaque pont, la géométrie OSM du tablier décalée
   latéralement de δ ∈ {0, 2, 4, 6, 8, 10, 15} m, des deux côtés, prolongée par la route d'accès.
   **Traces réelles** : traces `user-baro`, dont on mesure le décalage effectif (distance médiane
   trace ↔ way sur l'ouvrage).
4. **Références** : (i) profil Strava des routes `test1`, `test2` (indépendant du MNS) ; (ii) baro
   recalé par l'offset local (§3.3) ; (iii) médiane 25 m du MNS sur l'axe OSM, rapportée mais **non
   décisive** (biaisée en faveur de B).
5. **Métriques par pont et par δ** : RMS et max sur les stations de tablier, P95 de pente (> 10 % =
   implausible), ancres retenues, abandon, et écart d'altitude aux culées.
6. **Jugement** : `ord-bench dsm-placement --judge` une seule fois, résultat dans
   `bench/judgements.md`, puis décision consignée dans doc 01 §11.

## 8. Interfaces

Le module qualité n'est pas un composant du service (doc 01 §4) : il vit dans le crate de dev
`ord-testkit` (T01 §2), les `tests/` de chaque crate, `tools/ord-bench` et les données de `bench/`.

**Expose** (aux tests d'intégration et à `ord-bench`) :

```rust
pub enum Role { Calibration, Negative, Holdout, HoldoutNegative }
pub enum StationKind { Terrain, Deck, OffDeck, Tunnel, Balcony, Ground, Overhead, Alongside, Excluded, Margin }
pub struct TruthProfile { pub route: String, pub step_m: f64, pub s: Vec<f64>, pub lat: Vec<f64>,
    pub lon: Vec<f64>, pub z: Vec<Option<f64>>, pub mnt: Vec<Option<f64>>, pub kind: Vec<StationKind>,
    pub strava_offset_m: Option<f64> }
pub struct StructureTruth { pub route: String, pub kind: StructureTruthKind, pub i0: usize, pub i1: usize,
    pub z0: f64, pub z1: f64, pub zmax: f64, pub name: String }
pub struct CorpusEntry { pub id: String, pub series: String, pub role: Role, pub input: PathBuf,
    pub truth: Option<PathBuf>, pub sha256: [u8; 32] }
pub struct Corpus { pub entries: Vec<CorpusEntry> }          // refuse une empreinte fausse
pub struct RecordedPoint { pub t: Option<i64>, pub lat: f64, pub lon: f64, pub ele_baro: f64 }
pub struct RecordedTrack { pub id: String, pub device: String, pub role: Role, pub points: Vec<RecordedPoint> }
pub struct BaroAlignment { pub lag_m: f64, pub offset_m: f64, pub drift_m: f64, pub drifting: bool }
pub struct RulesFingerprint(pub [u8; 32]);  pub struct DataFingerprint(pub [u8; 32]);
pub struct Criterion { pub id: &'static str, pub blocking: bool, pub threshold: Threshold }
pub enum Verdict { Pass, Fail { measured: f64 }, Reported }
pub struct BenchResults { pub rules_version: String, pub rules: RulesFingerprint,
    pub data: DataFingerprint, pub metrics: BTreeMap<String, f64>, pub verdicts: BTreeMap<String, Verdict> }
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
| `gpx`, `fitparser` | ingestion GPX et FIT | seules dépendances de lecture, confinées à `bench/` |
| `csv`, `toml`, `serde_json` | vérités, manifestes, résultats | formats des bancs de vcyclist |
| `sha2`, `hdrhistogram`, `clap` | empreintes ; p50/p95 de latence ; CLI d'`ord-bench` | gel, non-régression, mesure |
| `criterion` (optionnel) ; outils `cargo-nextest`, `cargo-deny`, `cargo-fuzz`, `oasdiff` | micro-bancs ; exécution, licences, fuzz, rupture d'API | hors cœur |

## 10. Points de tension

1. **Vérité circulaire sur les tabliers.** La vérité `B` de vcyclist est le MNS IGN lui-même : A1/A2
   mesurent une cohérence ; Strava et le baro ne sont que des proxys indépendants. Exigence gardée.
   Un flux `altitude` Strava peut en outre avoir été remplacé par le fond Strava (« corriger
   l'altitude ») : à exclure au manifeste, faute de marqueur fiable dans les streams.
2. **Traces barométriques = traces enregistrées**, que v1 ne valide pas (doc 01 §2.3, V2). Le D+ et
   les ouvrages s'y mesurent, mais l'appariement d'une trace bruitée peut rater des ponts : le rappel
   d'ouvrages sur `user-baro` est rapporté à part, jamais mêlé aux critères bloquants.
3. **Q11 « aucune dépendance de fichiers entre projets »** contre doc 01 §8.5 « bancs de vcyclist » :
   résolu par une **copie gelée** avec manifeste et empreintes. Une régénération côté vcyclist ne nous
   atteint pas ; c'est voulu.
4. **Déterminisme flottant** : l'exigence « même réponse » (doc 02 §6) suppose une sérialisation
   arrondie ou une arithmétique identique entre machines (FMA x86/ARM). Tranché par T01 §9 (arrondis
   de l'assembleur) ; reste un cas rare de valeur à cheval sur un arrondi, d'où la tolérance d'une unité.
5. **A4 (≥ 9/11) et A3 (21/21)** sont des paris sur l'apport d'OSM : si l'appariement rate les tabliers
   étroits, le seuil sera manqué et rapporté, pas abaissé.
6. **A13 dépend du réseau** (IGN, 40 req/s/IP, pas de SLA) : critère mesuré en live, donc non
   reproductible et non bloquant en CI.
7. **Variante B (§7)** réutilise le constructeur de tablier sur une géométrie que la trace ne suit pas :
   l'invariant 8 (même altitude dans les deux sens) doit être revérifié par `proptest` dans ce mode.
