# 01 — Étude de vcyclist : la chaîne d'élévation « HD » (ponts, tunnels, IGN LiDAR HD)

> Exploration en lecture seule de `/Volumes/Data/code/perso/vcyclist` (Kotlin Multiplatform),
> **version 6.11.0** (`CHANGELOG.md:1`), source d'inspiration pour la partie élévation
> d'`osm-route-details`. Ce document **remplace intégralement** la version écrite sur vcyclist 5.0.1
> (commit `6531671`) : 104 commits plus tard, vcyclist lit l'IGN LiDAR HD par défaut, détecte les
> tabliers de ponts et les tunnels, et rapporte la provenance du terrain kilomètre par kilomètre.
>
> Références `chemin:ligne` relatives à la racine de vcyclist, avec ces abréviations :
> `M/` = `elevation/src/commonMain/kotlin/io/github/glandais/elevation/`,
> `G/` = `gpx/src/commonMain/kotlin/io/github/glandais/engine/path/`,
> `E/` = `engine/src/commonMain/kotlin/io/github/glandais/engine/`,
> `guide` = `docs/guides/elevation.md`, `decks` = `docs/archive/tasks/elevation-bridge-decks.md`.
> Les remarques marquées **(observation)** sont les miennes.

## 0. Ce qui a changé depuis la version précédente de ce document

| L'ancien document disait | État en 6.11.0 | Référence |
|---|---|---|
| « Une seule source : Mapterhorn » | Catalogue `ElevationSource` à deux entrées ; **IGN LiDAR HD est le défaut** (6.9) partout sauf WASI, Mapterhorn en repli | `M/ElevationSource.kt:18-108` (défaut l. 91, WASI l. 98) |
| « Ponts/tunnels : aucun traitement » | Étape **3b** tabliers de pont (MNS LiDAR) et étape **3c** tunnels (MNT seul), actives par défaut, avec rapport | `E/ElevationCorrection.kt:96-133` |
| « Aucun no-data reconnu, une tuile en échec fait échouer le calcul » | `-9999` IGN = no-data → point relu dans Mapterhorn ; tuile en erreur → repli + avertissement, cache négatif d'une minute | `M/Bil32.kt:17-21`, `M/TileFailures.kt:44-88` |
| « Pixel traité au coin : décalage d'un demi-pixel (observation) » | **Confirmé et corrigé** (`elevation-half-pixel`) : interpolation entre centres | `M/ElevationCalculator.kt:92-94` |
| « Pas de retry HTTP » | Pacer par hôte et pour tout le processus (20 req/s IGN), retry 429/503 jusqu'à 6 essais avec `Retry-After` | `M/TileRequestPacing.kt:24-63`, `M/IgnLidarHdLayer.kt:65` |
| « MNT +35 % de D+ en montagne » (854 vs 634 m) | Non reproductible ; remesuré **+12 %** (714 vs 637 m), Mapterhorn z12 aux centres | `guide:317-326` |
| « z12 ≈ résolution native de Mapterhorn » | Faux en France : Mapterhorn y sert du LiDAR HD sous-échantillonné (z17 dispo) ; monter en zoom n'aide pas pour une autre raison (les grosses erreurs sont les ouvrages) | `decks:79-92`, `guide:328-351` |
| « D+ lu après simplification » (implicite) | D+ mesuré **avant** Douglas-Peucker et reporté (E5/R33) | `guide:238-286`, `E/Enhancer.kt:225` |
| Pas de service HTTP | Module `:api` Ktor, 17 opérations `/api/v1`, OpenAPI généré | `api/README.md` |

## 1. Vue d'ensemble des modules

| Module | Rôle pour l'élévation |
|---|---|
| `elevation/` | Catalogue de sources, fetch/décodage (Terrarium WebP, **BIL float32 IGN**), bilinéaire aux centres, repli, couverture IGN, `TerrainOrigin`, pacer, LRU. |
| `gpx/` (`engine.path`) | Détecteurs **purs** `DeckDetector`, `DeckProfileBuilder`, `TunnelDetector` (+ options), `ElevationStep`, `ElevationGain`, modèle `ElevationCorrections`. |
| `engine/` | `ElevationCorrection` (étapes 1–3c sans physique), `BridgeDeckStep`, `TunnelStep`, `ElevationCorrectionReport`, `Enhancer`. |
| `api/` | Service HTTP Ktor sans état (voir §8). |
| `cli/` | Options `--dem-source`, `--[no-]bridge-decks`, `--[no-]tunnels`, `--tunnel-scope` (`cli/README.md:28-33`). |
| `docs/deck-strava-data/analysis/` | Détecteur de référence Python (`analyze.py`, `common.py`), rapport Strava vs IGN (`rapport.md`). |
| `docs/archive/tasks/` | `elevation-bridge-decks.md` (2 900 lignes, E1–E7), `elevation-ign-first.md`, `elevation-half-pixel.md`, `api.md`. |
| `docs/ledgers/improvements-ledger.md` | Lignes R31 (tabliers), R32 (tunnels), R33 (D+) : `:83-85`, détails `:1235-1360`. |

## 2. Sources d'élévation

### 2.1 Catalogue

| id | Modèle | Couverture | Échantillonnage par défaut | Tuile |
|---|---|---|---|---|
| `ign-lidar-hd` (**défaut** depuis 6.9, hors WASI) | IGN LiDAR HD **MNT** (terrain nu), grille 1 m, ~10 cm vertical | France métropolitaine + Corse | z15 × 256 px ≈ 3,4 m/px à 45° N | ~256 ko (BIL non compressé) |
| `mapterhorn` (défaut WASI) | MNT fusionné mondial, Terrarium WebP | monde | z12 × 512 px ≈ 13,5 m/px | ~65 ko |

Sources : `guide:454-463`, `M/ElevationSource.kt:21-52`, `M/IgnLidarHdLayer.kt:45-53`.

### 2.2 IGN LiDAR HD : service, format, produits

- **Service** : WMS raster de la Géoplateforme, `data.geopf.fr/wms-r/wms`, WMS 1.3.0 `GetMap`,
  `CRS=EPSG:4326`, `FORMAT=image/x-bil;bits=32` (float32 little-endian, mètres)
  (`M/IgnLidarHdLayer.kt:107-112`). Chaque tuile XYZ devient **un `GetMap` dont le `BBOX` est
  l'emprise de la tuile** en EPSG:4326 (ordre lat-first `south,west,north,east`,
  `M/ElevationSource.kt:110-119`). C'est le même service que `leaflet-relief`.
- **Produits** (`M/IgnLidarHdLayer.kt:28-43`) :
  - `IGNF_LIDAR-HD_MNT_ELEVATION.ELEVATIONGRIDCOVERAGE.WGS84G` — **MNT**, la seule source d'altitude ;
  - `…_MNS_…` — **MNS** (sommet de tout : tabliers, mais aussi cimes et toits), attaché comme
    `surfaceModel` **uniquement à z15** (`M/ElevationSource.kt:151-152`) ; jamais sélectionnable
    comme sol (sous une canopée il lit les arbres) ;
  - `…_MNH_…` — hauteur de sursol ; exposée mais **non utilisée** : le détecteur lit `MNS − MNT`
    (`G/DeckDetector.kt:34-45`), sur lequel ses seuils ont été mesurés (identique à 1e-5 m près,
    `rapport.md`, *Contrôles*).
- Décodage BIL pur Kotlin puis ré-encodage en Terrarium pour garder un contrat `RawTile` unique
  (`guide:477-482`). Seuil no-data : valeur ≤ −9000 (IGN envoie `-9999`) (`M/Bil32.kt:17-21,61`).
- Cadence : 20 req/s (moitié des 40 req/s/IP publiées), budget **par hôte pour tout le processus**,
  MNT et MNS le partagent (`M/IgnLidarHdLayer.kt:62-65`, `guide:619-630`) ; 429/503 relancés
  jusqu'à 6 fois après `Retry-After` ou 1, 2, 4… s (`M/TileRequestPacing.kt:24-63`).
- Licence : « © IGN — LiDAR HD, Licence Ouverte Etalab 2.0 » (`M/IgnLidarHdLayer.kt:66-70`).

### 2.3 Mapterhorn

- `https://tiles.mapterhorn.com/{z}/{x}/{y}.webp`, z12, 512 px, LRU 100 tuiles
  (`M/ElevationProviderConfig.kt:19-22`). Produit fusionné : Copernicus GLO-30 dans le monde,
  modèles nationaux là où ils existent — **en France, c'est le LiDAR HD lui-même**, sous-échantillonné :
  z12 à 0,46 m RMS du MNT LiDAR (GLO-30 : 2,8 m), z17 à 0,41 m/px, écart médian 0,00 m (`decks:79-92`).
- Mapterhorn ne publie **aucune emprise par source** (`attribution.json` liste `frhd*`, `fi`,
  `glo30`… sans géométrie), donc hors de France on ne sait pas si un point vient d'un LiDAR national
  ou de GLO-30 (`elevation-ign-first.md`, *Inputs*).

### 2.4 Priorité, repli, couverture

1. **Filtre de couverture** : `IgnLidarHdCoverage`, contour grossier de la métropole et de la Corse
   tracé 5 à 20 km au-delà de la frontière et de la côte (`M/IgnLidarHdCoverage.kt:19-125`). Une
   tuile IGN entièrement dehors = no-data **sans requête** (avant 6.5.2, une sortie en Italie
   téléchargeait 256 ko de `-9999` par tuile, ×2 avec le MNS) (`guide:494-501`).
2. **No-data point par point** : un point dont un des 4 voisins bilinéaires est no-data est
   **entièrement** relu dans Mapterhorn, jamais mélangé aux 3 voisins IGN (sinon marche à chaque
   côte/frontière) (`guide:484-492`). Sans repli (WASI, provider fait main) : 0 m.
3. **Erreur de tuile** (HTTP après retries, réseau, corps non décodable) : points relus dans le
   repli, tuile mémorisée illisible 60 s (`UnreadableTiles`, `M/TileFailures.kt:44-88`), échecs
   collectés **par sortie** dans un `TileFailureLog` du contexte coroutine (le provider est partagé
   entre requêtes) (`M/TileFailures.kt:19`). Conséquence : avertissement `terrain: N tile(s)…` et
   **aucun tablier** sur cette sortie (`E/ElevationCorrection.kt:104-108`).
4. Les couches brutes MNT/MNS/MNH (`IgnLidarHdLayer.provider`) répondent `NaN` hors données et
   refusent tout repli (`withFallback` lève) : on ne compare jamais un MNS IGN à un sol Mapterhorn
   (`M/IgnLidarHdLayer.kt:16-21,74-90`).

### 2.5 Provenance par tronçon (« where every kilometre of terrain was read »)

`ElevationProvider.setElevationsWithOrigin` renvoie une `TerrainOrigin` par point
(`M/TerrainOrigin.kt:15-60`), agrégée en km dans `TerrainProvenance` (`G/ElevationCorrections.kt:90-98`) :

| Origine | Champ | « Mesurée » | Sens |
|---|---|---|---|
| `ign-lidar-hd` | `ignLidarHdKm` | oui | MNT IGN |
| `mapterhorn-measured` | `mapterhornMeasuredKm` | oui | Mapterhorn **dans** l'emprise France (LiDAR dérivé, banc mesuré) |
| `mapterhorn` | `mapterhornKm` | non | Mapterhorn ailleurs (national ou GLO-30, indiscernables) |
| `other` | `otherKm` | non | gabarit inconnu (hôte WASI non déclaré, config manuelle) |
| `none` | `noDataKm` | non | aucune donnée : 0 m |

Plus `fallbackAfterErrorKm` et `failedTiles`. Les km sont mesurés sur la grille de 30 m de
l'étape 2, chaque point valant la moitié des segments adjacents : ils somment à la longueur et ne
bougent pas au rééchantillonnage (`guide:665-684`). Pas de champ de provenance **par point**
(changement de format refusé). Origine déduite du gabarit d'URL, sinon de la source déclarée
(`servingSource`, `M/ElevationProvider.kt:131`).

### 2.6 Référentiels

- Horizontal : tuiles Web Mercator XYZ ; requête IGN en EPSG:4326 sur l'emprise de la tuile.
- Vertical : **non traité dans le code**. L'étude note IGN69 (LiDAR) contre EGM2008 (GLO-30) :
  +0,76 m dans l'estuaire de la Loire, marche visible seulement en bordure de couverture
  (`decks:94-96`, A19) ; jugé négligeable pour le vélo, « à ne pas oublier dans un futur raccord ».
- Pixel = valeur au **centre** pour toutes les sources (§6.1).

## 3. Chaîne complète du profil HD

`ElevationCorrection.runLogged` (`E/ElevationCorrection.kt:79-141`), appelé par l'`Enhancer`
(`E/Enhancer.kt:160`) ou directement via `fixAndCorrect` pour un appelant sans physique
(`E/ElevationCorrection.kt:47-55`) :

| # | Étape | Paramètres | Où |
|---|---|---|---|
| 1 | Densification | ≤ 30 m (`PRE_FIX_SPACING_M`) | `E/Enhancer.kt:72`, `E/ElevationCorrection.kt:88` |
| 2 | Lecture MNT + origine par point | bilinéaire aux centres ; repli Mapterhorn | `G/ElevationStep.kt:50`, `E/ElevationCorrection.kt:91-96` |
| 3 | Raffinement | 1–2 m (`REFINE_*`, 5–10 m acceptables pour un serveur : 352 o/point) | `E/Enhancer.kt:75-78`, `E/ElevationCorrection.kt:99` |
| 3b | **Tabliers** (si `bridgeDecks` et provider avec `surfaceModel`) | stations 5 m ; MNT + MNS | `E/ElevationCorrection.kt:101-110`, `E/path/BridgeDeckStep.kt:117-178` |
| 3c | **Tunnels** (si `tunnels`) | stations 5 m ; MNT seul, réutilise les échantillons de 3b ; ne réécrit jamais un tablier | `E/ElevationCorrection.kt:112-133` |
| — | Rapport `Path.elevationCorrections` + provenance | | `E/ElevationCorrection.kt:137-139`, `G/Path.kt:78` |
| 4 | Lissage physique | noyau triangulaire, demi-largeur **150 m** (≈ 61 m effectifs), toujours ; profil brut gardé dans `SOURCE_ELEVATION` | `G/ElevationStep.kt:22,80`, `E/Enhancer.kt:170` |
| 5 | Physique, 1 Hz (hors sujet ici) | | |
| 6 | **D+/D−** `ElevationGain` sur `sourceElevation` (donc avec tabliers et tunnels, sans le noyau 150 m), preset `dem` = 3 m sur 30 m | `E/Enhancer.kt:225`, `guide:93-99` |
| 7 | Douglas-Peucker (10 m, ×3), qui **reporte** le D+ calculé avant | `E/Enhancer.kt:240`, `guide:238-246` |

Points clés : les corrections ne s'appliquent **qu'après** une correction MNT (jamais sur les `<ele>`
d'un GPX/baromètre) ; elles se placent **avant** le lissage pour que le D+ les porte ; les
détecteurs ont leur propre grille de 5 m indépendante de l'espacement du chemin.

## 4. Détection des ponts (tabliers) — étape 3b

**Données** : uniquement LiDAR — MNT et MNS IGN à z15, échantillonnés tous les 5 m le long de la
trace, `MNH = MNS − MNT`. **Pas d'OSM** (vcyclist n'a pas les tags ; OSM sert seulement au banc).
Une station dont le MNT ne vient pas de l'IGN a son MNS mis à `NaN` avant détection
(`E/path/BridgeDeckStep.kt:154`) : « ride no deck over a foreign one ».

**Piège fondateur** : le MNS est aussi la canopée et les toits ; « MNS là où MNH > seuil » ferait de
chaque route forestière des montagnes russes de 20 m. **Le MNS n'est lu que sur un ouvrage
détecté** (`decks:116-130`).

**Principe** (portage du Python de référence, `G/DeckDetector.kt:147-232`) :

1. Profil du tablier `mf` = médiane glissante (ignorant les NaN) du MNS sur 25 m ; rugosité = RMS
   de `MNS − mf` sur 35 m.
2. Stations « hautes » `MNH > 5 m`, runs ≥ 15 m ; un run est gardé si ≥ 60 % de ses stations sont
   lisses (rugosité < 0,8 m) — sinon canopée/bâti (`ROUGH`).
3. Fusion des runs gardés à travers < 300 m de trous (trous de MNS au-dessus de l'eau, piles).
4. Extension le long de `MNH > 1 m` jusqu'aux culées, bornée à 100 m par côté.
5. **Canyon** : le MNT minimum sous le span est > 3 m sous le plus bas des deux MNT extérieurs
   (un toit au-dessus d'une route n'a pas de canyon).
6. **Pente** : span long → P95 de |pente| du MNS filtré (fenêtre 20 m) ≤ 10 %, lue sur un profil
   « ponté » à travers les stations rugueuses (pylônes, haubans) ; span court (≤ 25 m) → rugosité
   médiane < 0,4 m. Un span fusionné jugé trop pentu est rejugé par parties.
7. Masque de tablier : runs `MNH > 1 m` ≥ 40 m (tous sur un span ≤ 60 m).

**Remplacement** (`G/DeckProfileBuilder.kt:85-115`) : ancres = stations propres du masque ; MNS
aux ancres, linéaire entre elles, médiane 25 m, puis **plafond de pente** 10 % (plus petit majorant
10 %-lipschitzien : remplit les creux, élargit les pics en tente) ; rampes linéaires jusqu'au
terrain aux deux bouts. Une région avec < 2 ancres est abandonnée (`bridgeDecksDropped` +
avertissement).

**Paramètres** : `G/DeckDetectorOptions.kt:28-148` (`stepM` 5, `tallMnhM` 5, `maxRoughnessM` 0,8,
`mergeGapM` 300, `deckMnhM` 1, `maxSlopeP95` 0,10, `minCanyonDropM` 3, `maxAbutmentM` 100…). Les
fenêtres doivent tomber sur un nombre entier (impair pour les médianes) de stations : sinon échec à
la construction.

**Résultats** (`guide:532-567`, `decks` E1–E3) : Saint-Nazaire, `sourceElevation` à 0,54 m de la
vérité au pire au lieu de 65,8 m dessous ; D+ de la route 0,74 → 1,08 × vrai. Calibration : 18 ponts
sur 21 trouvés sur 4 routes ; **série tenue à l'écart : 4 spans sur 11, 30 % des stations de
tablier**, 0 faux positif. **Échecs connus** : ponts courts (≤ 60 m), longs tabliers sous
superstructure lourde (treillis, suspendu), arches longues > 10 % (Pont-Saint-Esprit) ; toit lisse
à < 300 m d'un pont fusionné dans son masque ; creux longs et lisses (Cheviré). Un pont manqué est
roulé au fond de la vallée. Coût : une tuile MNS par tuile MNT (≈ +35 Mo sur 130 km à froid).

## 5. Détection des tunnels — étape 3c

**Données** : **le profil MNT seul**, sur n'importe quelle source (pas de MNS : il est lui aussi
au-dessus du relief). Aucun LiDAR ne voit dans un tunnel : aux Grands Goulets (1,75 km) le profil
lu monte de 220 m et redescend (`guide:569-574`).

**Principe** (`G/TunnelDetector.kt:63-142`), cherche « la forme qu'aucune route n'a » :

1. **Portails** : côté extérieur routier (pente ≤ 20 % sur 20 m **et** 50 m), côté intérieur qui
   monte (entrée) / descend (sortie) ≥ 25 % sur 20 m (front de taille).
2. **Paires** entrée/sortie de 50 m à 3 km, acceptées si : rectitude corde/longueur ≥ 0,8 (on ne
   perce pas des lacets) ; pente de la droite entre portails ≤ 12 % ; terrain ≥ 15 m au-dessus de
   cette droite, et **déjà** ≥ 15 m dans les 100 m de chaque portail (un vrai front de montagne,
   pas la rive d'un ravin dont le MNT a effacé le pont) ; flancs ≥ 30 % sur 50 m des deux côtés
   dans les 50 m ; jamais > 3 m sous la droite ; une vallée intérieure < 15 m = deux tunnels.
   Garde : une station à 0 m exactement près d'un portail (trou d'un provider sans repli) rejette.
3. Pour chaque entrée, la paire de plus grande hauteur.

**Écriture** (`E/path/TunnelStep.kt:42-70`) : la droite entre le **MNT aux stations portails**
(pas l'altitude du chemin, échantillonnée à 30 m et contaminée par le front), avec 30 m relus
autour de chaque portail ; un tunnel contenant un tablier 3b est abandonné (`tunnelsDropped` :
les piles d'un pont que le MNT a gardées, Normandie, Millau).

**Paramètres** : `G/TunnelDetectorOptions.kt:19-93`, chacun justifié par un balayage de
sensibilité (ex. `minSideGrade` 0,30 : à 20–25 % un faux tunnel apparaît sur un parcours pyrénéen ;
`maxChordGrade` 0,12 : à 15 % une crête de barrage passe).

**`tunnelScope`** (6.8, `E/EnhanceOptions.kt:73-77,142-157`) : `measured-terrain` (défaut) garde un
tunnel seulement si toutes les stations de [portail − 100 m, portail + 100 m] ont une origine
mesurée (`E/path/TunnelStep.kt:71-72,116`) ; sinon il est laissé tel quel, compté dans
`tunnelsOutOfScope` et localisé par km dans un avertissement. `everywhere` = comportement antérieur.
Motivation : précision mesurée uniquement sur du terrain LiDAR ; une sonde sur les gorges du Dadès
(Maroc, GLO-30) a trouvé 3 faux tunnels (`guide:598-617`).

**Résultats** (`guide:578-597`, `decks:2455-2533`) : réglé pour la **précision** (un faux tunnel
aplatit une vraie montée). Sur 13 négatifs + 16 négatifs-ponts + 10 sorties : 0 station de sol
déplacée sur IGN. 7/8 tunnels de réglage (IGN), 5/8 (Mapterhorn). Grands Goulets : 0,9 m (IGN) de
la droite des portails au lieu de 220 m ; D+ Vercors 1,19 → 1,05 × vrai. **Série tenue à l'écart :
6/7 tunnels, 0 faux tunnel** (mais jugée avant la revue qui a ajouté le critère de front : le code
actuel n'a pas été rejugé). **Échecs connus** : tranchée couverte sous terrain plat (bosse 4–6 m),
< 50 m, courbe/hélicoïdal, chaîne de tunnels courts en gorge (Petits Goulets : +68 m restants),
tunnel sous colline douce (voie verte de Dordogne). **Faux positifs possibles** : vraie butte à
≥ 30 % sur un chemin/VTT/sentier (désactiver `--no-tunnels`). Le « repli canyon » (même détecteur
retourné, pour ponts sans MNS) a été mesuré et **différé** : 2 ponts sur 29 (ledger `:1320-1322`).

## 6. Mesures, pièges, correction demi-pixel

### 6.1 Demi-pixel (`elevation-half-pixel.md`)

Le calculateur plaçait la valeur du pixel sur son coin nord-ouest alors que toutes les sources la
définissent au centre (le `BBOX` WMS nomme les bords extérieurs ; Mapterhorn/Terrarium sont des
warps GDAL aux centres) : chaque altitude était celle du terrain ½ px au sud-est — 6,2–6,9 m par axe
pour Mapterhorn z12, 1,55–1,72 m pour IGN z15. Correction : `fx = pf.x − ½`, `fy = pf.y − ½`
(`M/ElevationCalculator.kt:92-94`). Effets : RMS terrain ouvert Mapterhorn `test4` 2,63 → 0,58 m,
`test3` 1,68 → 0,66 m ; une régression (IGN `test3`, 0,59 → 0,78 m) ; le détecteur de tablier a dû
être recalibré (16 → 14 ponts sur 21 en mode Python, rattrapés par E2 quinquies)
(`elevation-half-pixel.md:267-434`).

### 6.2 Comparaison Strava (`docs/deck-strava-data/analysis/rapport.md`)

Quatre itinéraires tracés dans Strava (altitude = fond Strava) contre IGN LiDAR HD, rééchantillonnés
à 5 m : `test1` Saint-Nazaire 10 km, `test2` Nantes 25 km (16 ponts de Loire), `test3` Mauves 10 km,
`test4` Vercors 111 km (4 tunnels).

- **Ponts** : Strava suit le tablier sur les 21 ponts détectés (Saint-Nazaire 1,1 m RMS ; Nantes
  ≤ 1,8 m max), ne tombe jamais dans le canyon (6–66 m selon le pont).
- **Tunnels** : 3/4 bien traités ; **Grands Goulets non** : escalier de paliers, +190 m d'erreur,
  ≈ +112 m de D+ fictif. Hypothèse de l'auteur : altitude attribuée par nœud OSM.
- **Hors ouvrages** : RMSE 0,75–0,85 m (Loire-Atlantique), 2,3 m (Vercors) ; offset médian
  Strava − MNT de −0,52 à +1,48 m.
- **D+** (`test4`, MM45 h3) : Strava 2 479, MNT brut 2 837, MNS brut 3 303, **chaussée estimée
  2 362 m**. Sur `test2` le MNS brut donne 1 267 m pour 37 m vrais : jamais de MNS hors ouvrage.
- **Balcons/encorbellements** : le MNT au point lit la falaise (+76 m, Combe Laval) ou la gorge
  (−22 m, Bourne) ; non traité, la pente transversale (> 100 % sur ±10 m) est peu discriminante.

### 6.3 D+ et lissage (inchangés sur le fond, remesurés)

- D+ = mesure dépendante de l'échelle (Kollár) ; toujours l'annoncer avec (seuil, fenêtre)
  (`guide:107-117`). Presets `raw` / `barometric` 2 m·15 m / **`dem` 3 m·30 m** / `gps` 10 m·50 m.
- Mesuré avant simplification (E5) : Nantes IGN 0,49 → 0,95 × vrai ; Vercors IGN 1,05
  (`guide:238-262`). Le noyau 150 m et le preset `dem` ont été remis en question sur LiDAR et
  **gardés** : un noyau plus petit gagne quelques dixièmes en plaine et crée des pentes absurdes en
  montagne (81 stations > 15 % au lieu de 0, vrai : 2) (`guide:288-300`).
- MNT vs baromètre : +12 % sur `strava.gpx` (714 vs 637 m), pas un problème de recalage (R29 ❌,
  le tracé est déjà le minimum de rugosité de son couloir ±15 m) ni de zoom (R30 : z12→z15, 1,5–3 %)
  (`guide:317-351,390-434`).
- Mapterhorn vs IGN sur `sample.gpx` (130 km Savoie) : D+ 2 903 vs 2 897 m, |Δ| moyen 2,0 m, p95
  4,6 m, max 26,6 m (route en corniche) ; IGN coûte ~15× le volume (`guide:516-530`). Avec les
  corrections : 4 599 → 4 652 m (+1,2 %), 9 tabliers, 264 tuiles / 66 Mo / 19 s à froid contre
  16 tuiles / 2,3 Mo / 3 s (`M/ElevationSource.kt:84-91`).

### 6.4 Méthodologie de banc (à imiter)

Vérité = MNS médiane 25 m sur ouvrage, MNT ailleurs, interpolation portail-à-portail en tunnel ;
séries **tenues à l'écart** gelées par règle écrite avant toute requête, jugées **une seule fois**,
empreintes SHA-256 (`analysis/tunnel-judge.sha256`) ; négatifs explicites (lacets, balcons, barrages,
canopée). Le constat honnête que le premier juge n'avait pas de critère d'acceptation figé
(`decks:2462-2468`) est une leçon en soi.

## 7. Rapport des corrections

`ElevationCorrections` (`G/ElevationCorrections.kt:63-73`) : statut par correction
(`not-run`, `unavailable`, `no-coverage`, `none-found`, `applied`, `fetch-failed`,
`G/ElevationCorrections.kt:7-30`), compteurs `bridgeDecksApplied/Dropped`,
`tunnelsApplied/Dropped/OutOfScope`, `warnings` (une phrase par sortie **sciemment fausse ou
dépendante du réseau**, jamais pour le cas normal), `terrain` (§2.5). Les tronçons corrigés eux-mêmes
**ne sont exposés sur aucune porte** : leurs distances sont sur l'axe densifié que les étapes
suivantes décalent (`guide:686-704`).

## 8. Service HTTP `:api`

Ktor JVM, **sans état**, 17 opérations sous `/api/v1`, contrat OpenAPI 3.1 **généré depuis le code**
et testé (`api/contract/openapi.yaml`, `OpenApiContractTest`) (`api/README.md:1-16`). Pertinent :

| Endpoint | Rôle |
|---|---|
| `GET /elevation/sources` | sources, zoom, attribution |
| `GET /elevation/point?lat=&lon=` (+ `source`, `zoomLevel`, `interpolation`) | une altitude |
| `POST /elevation/points` | `{points:[{latitude,longitude}…]}` → altitudes |
| `POST /elevation/along` | polyligne + `step`, lissage, filtre → profil (≤ 10 000 points densifiés) |
| `POST /enhance` | GPX (multipart `gpx` + `request` JSON, ou corps GPX) → résumé JSON avec `elevationCorrections` par trace, ou GPX/FIT/CSV/JSON |
| `GET /catalog`, `/defaults`, `/fields` | valeurs d'énumérations (dont statuts de correction), défauts **appliqués**, limites |

Choix transposables (`api/README.md:105-207`) : erreurs RFC 9457 `application/problem+json` avec
`type = urn:vcyclist:problem:<slug>` (`dem-unavailable` 502, `dem-rate-limited` 503 + `Retry-After`,
`too-many-points` 413…) ; clé inconnue = 400 avant tout calcul ; borne sur la taille **densifiée**
(longueur/30 m + longueur/2 m), pas le nombre de points d'entrée ; échec du MNS = 200 avec
avertissement ; aucune URL de tuile fournie par le client (SSRF) ; `X-Request-Id` ; chaque réponse
MNT porte l'attribution de sa source **et** de son repli ; provider par `(source, zoom)` partagé
sans verrou depuis 6.9.1 (`api/src/main/kotlin/io/github/glandais/api/service/ElevationGateway.kt:15-25`).
**(observation)** `guide:474-476` dit encore qu'une requête IGN froide bloque la suivante : périmé
depuis `c4d4338`.

## 9. Transposable en Rust vs spécifique

**À porter** (logique pure, testable) : catalogue de sources avec repli et filtre de couverture ;
client WMS `GetMap` BIL float32 par emprise de tuile XYZ ; no-data point entier (pas de mélange de
voisins) ; bilinéaire **aux centres** ; pacer par hôte + retry 429/503 ; cache négatif court ; log
d'échecs par requête ; `TerrainOrigin` + km par origine ; `DeckDetector`/`DeckProfileBuilder`/
`TunnelDetector` (tableaux → spans, déterministes, ~1 000 lignes avec leurs options) ; ordre
corrections → lissage → D+ → simplification ; D+ à hystérésis.

**Spécifique à vcyclist** : multiplateforme (JS/WASI, façades générées) ; noyau 150 m dimensionné
pour la physique ; `Path` à champs générés ; contrainte WASI « une seule couche, pas de repli ».

## Enseignements pour osm-route-details

1. **Source par défaut en France : IGN LiDAR HD MNT via WMS-R, Mapterhorn en repli.** Reprendre
   tel quel le schéma de vcyclist (emprise de couverture sans requête, no-data → repli au point
   entier, panne → repli + avertissement). Prévoir un **cache disque** obligatoire : 66 Mo à froid
   pour 130 km, WMS non compressé, 40 req/s/IP. Gérer l'écart IGN69/EGM2008 (~0,8 m) aux bordures.
2. **La provenance est une sortie de l'API**, pas un détail : km (ou tronçons) par origine,
   `mesuré` vs non, `fallbackAfterError`. Pour un service qui renvoie des tronçons, une provenance
   **par tronçon** est naturelle — ce que vcyclist n'a pas pu faire (contrainte de format).
3. **OSM apporte ce que vcyclist doit deviner.** Les heuristiques sur profil sont réglées pour la
   précision et ratent beaucoup (ponts : 30 % des stations hors échantillon ; tunnels courts,
   courbes, tranchées couvertes). Les tags `bridge=*`/`tunnel=*`/`layer`/`covered` donnent
   l'**emprise** et la **typologie** directement, y compris pour les cas manqués (petits ponts,
   tunnels courbes, Grands Goulets que Strava rate). Mais :
   - **les tags ne suffisent pas pour l'altitude d'un pont** : interpoler linéairement entre culées
     OSM donne 22–40 m d'erreur sur les grands ouvrages bombés (Cheviré, Saint-Nazaire), et OSM
     manque des ouvrages (viaduc nord de Saint-Nazaire, ~1 050 m, non tagué `bridge`) (`decks:52-67`) ;
   - **meilleure stratégie probable : OSM pour l'emprise, LiDAR pour le profil.** Sur un way
     `bridge=*`, lire le MNS (médiane 25 m, ancres lisses, plafond de pente) comme
     `DeckProfileBuilder`, ce qui évite le piège canopée **sans** le détecteur (l'emprise OSM remplace
     les tests tall/rugosité/canyon) ; repli linéaire entre culées hors LiDAR. Sur `tunnel=*`,
     droite entre le MNT des portails pris **juste à l'extérieur** du way (le front contamine sinon).
   - **garder les heuristiques comme contrôle et filet** : un `DeckDetector` qui trouve un tablier
     hors de tout way `bridge` signale un tag manquant ; un tunnel détecté sans way `tunnel` aussi ;
     inversement un `bridge` OSM sans MNH (passage inférieur, tablier au ras du sol) ne doit pas être
     relevé. Croiser les deux donne un indicateur de confiance par tronçon.
4. **Le matching trace ↔ way devient central.** Les corrections vcyclist travaillent sur la
   distance curviligne de la trace ; avec OSM il faut projeter les extrémités de ways sur la trace
   (et décider quoi faire quand la trace GPS passe *à côté* du pont : lire le MNS sur la géométrie OSM,
   pas sur la trace, est à évaluer). Ponts « au-dessus » de la trace (`layer` plus haut, route
   franchie dessous) : ne pas les appliquer — vcyclist les rejette par le test de canyon ; OSM doit le
   faire par la topologie (le way parcouru est-il le way `bridge` ?).
5. **Ordre du pipeline à figer** : échantillonnage métrique (5 m pour les corrections, ≤ 30 m pour
   la lecture), lecture MNT + origine, corrections ouvrages, **puis** lissage optionnel, D+ à
   hystérésis sur le profil corrigé non lissé, avec `(seuil, fenêtre)` dans la réponse (défaut
   3 m / 30 m), simplification en dernier et sans effet sur le D+.
6. **Exposer ce qui a été corrigé, par tronçon** (type d'ouvrage, source de la décision : tag OSM /
   heuristique / les deux, statut, avertissements) — c'est exactement ce que vcyclist voudrait et ne
   peut pas exposer. Statuts à reprendre : `not-run`, `unavailable`, `no-coverage`, `none-found`,
   `applied`, `fetch-failed`.
7. **Ne pas faire confiance au MNT hors LiDAR pour les heuristiques** (`tunnelScope =
   measured-terrain`) : à l'étranger, OSM est la seule source fiable pour les tunnels ; sur GLO-30
   (un MNS à 30 m !) les ponts sont souvent déjà « dans » le modèle et les canyons absents.
8. **Contrat HTTP** : s'inspirer de `:api` — sans état, OpenAPI généré et testé, RFC 9457 avec slugs,
   `/defaults` publiant les valeurs appliquées, borne sur la taille densifiée, attribution des
   sources dans chaque réponse, pas d'URL de tuile cliente.
9. **Bancs** : réutiliser les itinéraires `docs/deck-strava-data/test1..4.gpx`, la vérité
   `analysis/truth/`, et la discipline « série tenue à l'écart, critères écrits avant, jugée une
   fois ». Mesurer le gain OSM contre les chiffres vcyclist (ponts 18/21 calibration, 4/11 hors
   échantillon ; tunnels 6/7) : c'est la ligne de base à battre.
10. **Rester critique sur ce qui reste ouvert chez vcyclist** : balcons/encorbellements (MNT lit
    falaise ou gorge), creux lisses sur tablier, toits fusionnés, Finlande et GLO-30 non mesurés,
    revue d'E4 non rejugée. OSM n'aide pas les balcons ; un MNT à 1 m lu sur la géométrie OSM
    (plutôt que la trace GPS) pourrait.
