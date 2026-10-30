# 01 — Étude de vcyclist : la chaîne d'élévation

> Exploration en lecture seule de `/Volumes/Data/code/perso/vcyclist` (Kotlin Multiplatform),
> source d'inspiration pour la partie élévation d'`osm-route-details`. Références au format
> `chemin:ligne`, relatives à la racine de vcyclist. Les remarques marquées **(observation)** sont
> les miennes et ne figurent pas dans la doc de vcyclist.

## 1. Vue d'ensemble

| Module | Rôle pour l'élévation |
|---|---|
| `elevation/` | Récupération + décodage des tuiles MNT Terrarium, interpolation, densification, lissage, Douglas-Peucker 3D. JVM, JS (navigateur/Node), wasmWasi. |
| `gpx/` (package `engine.path`) | `ElevationStep` (pont vers le `Path`), `ElevationGain` (D+/D− à hystérésis), `PointPerDistance` (rééchantillonnage), calcul de la pente dans `Path.computeDerivedData`. |
| `engine/` | `Enhancer` : orchestre l'ordre des étapes ; `ClimbDetector` consomme le profil. |
| `cli/` | `DiskCachedElevation.kt` : cache disque des tuiles MNT. |
| `map/` | Rendu hypsométrique (`SrtmMapProducer`) et tuiles de fond (`TileMapProducer`), JVM seulement. |
| `docs/guides/elevation.md` | Le document de référence : mesures, pièges, comparaison Strava. **À lire en entier.** |
| `docs/ledgers/improvements-ledger.md` | Lignes R26–R30 (largeur OSM, D+ à seuil, fenêtre de lissage, recalage route, zoom MNT). |

## 2. Source de données

| Élément | Valeur | Référence |
|---|---|---|
| Fournisseur | **Mapterhorn** (`tiles.mapterhorn.com`) — seul fournisseur configuré | `elevation/.../ElevationProviderConfig.kt:16` |
| URL | `https://tiles.mapterhorn.com/{z}/{x}/{y}.webp` (template XYZ, configurable) | `ElevationProviderConfig.kt:16`, `TileManager.kt:27-31` |
| Format | WebP **lossless** (VP8L), encodage **Terrarium** ; le décodeur accepte aussi PNG | `TileFetcher.kt:15-18` |
| Projection | Web Mercator (XYZ), latitude bornée à ±85.0511° (exception au-delà) | `ElevationFunctions.kt:12-13,57-70` |
| Zoom | **12** par défaut ; validé 0..15 | `ElevationProviderConfig.kt:14`, `ElevationProvider.kt:15` |
| Taille de tuile | **512 px** (puissance de 2 exigée) | `ElevationProviderConfig.kt:17`, `ElevationProvider.kt:21` |
| Résolution au sol | 19.1 m/px à 0°, **13.5 m/px à 45°**, 9.6 m/px à 60° | `docs/guides/elevation.md:22-28` |
| Contenu | Produit fusionné : Copernicus 30 m mondial, MNT nationaux haute résolution là où ils existent | `docs/guides/elevation.md:299-300` |
| Licence / attribution | Objet `Attribution` obligatoire, renvoie vers `https://mapterhorn.com/attribution/` ; les conditions « découragent l'usage automatisé massif » (tests live hors CI) | `ElevationProviderConfig.kt:18-22`, `elevation/README.md` (section *Live HTTP integration tests*) |

**Absents** : aucune trace d'IGN (RGE ALTI), de Copernicus en direct, de COG, de PMTiles, de MNS,
ni de fusion multi-sources/priorités. Une seule source, un seul zoom. La fusion est déléguée à
Mapterhorn en amont.

### Décodage Terrarium

`élévation = R·256 + G + B/256 − 32768`, arrondie à 2 décimales (`Tile.kt:50-57`). Le décodage est
paresseux par pixel et mémoïsé dans un `DoubleArray` initialisé à NaN (`Tile.kt:18,31-42`).
`(0,0,0)` décode en −32768 (`elevation/src/commonTest/.../TileTest.kt:61`) : **aucune valeur
no-data n'est reconnue**.

**Piège majeur documenté** : le canal B porte la partie fractionnaire ; toute conversion d'espace
colorimétrique ou alpha prémultiplié (typiquement `createImageBitmap` dans un navigateur) corrompt
silencieusement les bits de poids faible. D'où un test d'octet-exactitude cross-plateforme par
SHA-256 sur une tuile de référence (`ReferenceTileDigestTest.kt:9-17`, `ReferenceTile.kt:17`,
`JsDecodePathTest.kt:23`). Trois décodeurs coexistent (ImageIO/TwelveMonkeys sur JVM,
canvas navigateur, `@jsquash/webp` sous Node) + un décodeur VP8L pur Kotlin pour WASI
(`TileFetcher.kt:25-37`, `elevation/.../webp/`).

## 3. Récupération et cache des tuiles

- **Transport séparé du décodage** : `fetchTileBytes(url)` / `decodeTileBytes(bytes)` publics,
  `fetchAndDecodeTile` par défaut (`TileFetcher.kt:12,41,60`). Le fetcher est injectable dans
  `ElevationProvider` (`ElevationProvider.kt:10-13`) pour brancher un cache.
- **HTTP JVM** : `java.net.http.HttpClient`, connect timeout 10 s, requête 30 s, redirections
  suivies, échec si statut ≠ 2xx ; **pas de retry** (`TileFetcher.jvm.kt:13-48`).
- **Cache mémoire** : LRU de tuiles *décodées*, 100 entrées par défaut
  (`ElevationProviderConfig.kt:15`), avec **déduplication des chargements concurrents** sur la même
  clé via `CompletableDeferred` (`LruCache.kt:7-84`). À 512×512×4 octets RGBA + 8 octets/pixel de
  cache de décodage, une tuile pèse ~3 Mo → ~300 Mo pour 100 tuiles **(observation)**.
- **Cache disque (CLI)** : `{cache}/{host}/{z}/{x}/{y}.webp`, octets compressés, tuiles considérées
  immuables (jamais expirées), échecs non cachés (`cli/.../DiskCachedElevation.kt:15-50,88-95`).
- **Leçons du commit `6f355ed`** (fix récent) :
  - décoder **avant** d'écrire sur disque — un 2xx dont le corps n'est pas une image (portail
    captif, page d'erreur CDN) était caché et cassait toutes les exécutions suivantes
    (`DiskCachedElevation.kt:23-27,45-48`) ;
  - une entrée cachée qui ne se décode plus est supprimée et re-téléchargée une fois
    (`DiskCachedElevation.kt:41-44`) ;
  - écriture atomique fichier temporaire + `ATOMIC_MOVE`, car jusqu'à 10 tuiles sont récupérées en
    parallèle (`DiskCachedElevation.kt:64-86`).
- **Parallélisme** : les points sont groupés par tuile puis traités avec au plus **10 tuiles en
  parallèle** (sémaphore) (`BatchCalculator.kt:23-49`, `Flux.kt:14-29`).
- Coût mesuré : ~30–50 ko par tuile WebP (`elevation/README.md`, section *Cost*).

## 4. Interpolation

`ElevationCalculator.kt:15-58` :

- Deux modes : plus proche pixel, ou **bilinéaire sur 4 voisins** (par défaut,
  `ElevationProvider.kt:78-79`).
- Piège documenté : `dx`/`dy` doivent être calculés depuis la position **flottante** du pixel ;
  depuis des coordonnées déjà tronquées, le bilinéaire dégénère silencieusement en plus proche voisin
  (`ElevationCalculator.kt:10-13`, commit `409ed40` « true bilinear interpolation »).
- Les voisins qui débordent de la tuile sont renormalisés vers la tuile adjacente
  (`ElevationFunctions.kt:19-51`) → l'interpolation traverse correctement les bords de tuiles (au
  prix d'un fetch supplémentaire).
- **(observation)** Le pixel `(i, j)` est traité comme situé au coin `(i, j)` et non au centre
  `(i+0.5, j+0.5)` (`ElevationFunctions.kt:105-106`, `ElevationCalculator.kt:40-45`). Si les tuiles
  suivent la convention *pixel-is-area*, cela induit un décalage systématique d'un demi-pixel
  (~6.7 m vers le sud-est à 45°N, z12/512). Non discuté dans vcyclist ; à vérifier sur Mapterhorn.
- **(observation)** `ElevationCalculator` a un `tileSize` par défaut à 256 alors que la config
  impose 512 (`ElevationCalculator.kt:17` vs `ElevationProviderConfig.kt:17`) ; sans effet car
  `ElevationProvider` passe la valeur, mais c'est un piège latent.

## 5. Échantillonnage le long du parcours

Deux mécanismes distincts :

| Contexte | Algorithme | Paramètres | Référence |
|---|---|---|---|
| API `getElevationsAlong` (lib) | Interpolation **linéaire en lat/lon** entre waypoints, un point tous les `step` m ; segments < `minDistance` ignorés | `step` = **10 m** (doit être > 1), `minDistance` = **1 m** | `ElevationProvider.kt:71-80`, `BatchCalculator.kt:58-130` |
| Pipeline `Enhancer` | `PointPerDistance(-1, 30)` : densification seule, écart max **30 m**, avant lookup MNT | puis `PointPerDistance(1, 2)` : 1–2 m pour la physique | `engine/.../Enhancer.kt:113-122`, `gpx/.../PointPerDistance.kt:5-17` |

Distances : haversine sur sphère de rayon 6 371 000 m (`Distance.kt:9-26`, `Constants.kt:10`).
Justification du pas de 30 m : ~2 pixels MNT, « assez de granularité » (`docs/guides/elevation.md:45`).

## 6. Lissage et filtrage

Aucun Kalman, ni Savitzky-Golay, ni moyenne glissante en nombre de points : **un seul noyau
triangulaire paramétré en distance**.

- `ElevationSmoother.smoothProfile` : pour chaque point, moyenne pondérée par `1 − |d|/W` de tous
  les points à moins de `W` mètres le long du parcours ; `W` est une **demi-largeur** (150 m ⇒ 300 m
  de fenêtre). Deux curseurs monotones ⇒ O(n·k) (`ElevationSmoother.kt:30-80`).
- Propriété clé : fenêtre métrique ⇒ **invariant au rééchantillonnage** ; longueur de moyennage
  effective `W/√6` (150 m → ~61 m) (`docs/guides/elevation.md:57-61`).
- Fenêtres utilisées :

| Usage | Fenêtre | Référence |
|---|---|---|
| API lib (`SmoothingOptions`, désactivé par défaut) | 50 m | `BatchCalculator.kt:3-7` |
| Pipeline physique (toujours appliqué) | **150 m** | `gpx/.../ElevationStep.kt:21`, `Enhancer.kt:124-125` |
| Calcul du D+ (preset `dem`) | 30 m (copie privée) | `gpx/.../ElevationGainOptions.kt:42` |

- Le profil avant lissage est conservé dans `SOURCE_ELEVATION` (écrit une seule fois, un second
  lissage ne l'écrase pas) (`ElevationStep.kt:45-70`). Le D+ lit ce profil source, pas le profil
  lissé pour la physique.
- **Douglas-Peucker 3D** (optionnel) : en coordonnées ECEF avec exagération verticale
  (tolérance 10 m, `zExaggeration` 3) (`DouglasPeucker.kt:11-19`, `BatchCalculator.kt:9-14`). Le
  cœur `simplifyIndices` est réutilisé sur des profils (distance, altitude) par le détecteur de
  montées (`DouglasPeucker.kt:21-45`).
- Pas de rejet d'outliers (assumé : `docs/guides/elevation.md:230`).

## 7. Pente

`grade = Δalt / Δdist` en **différence centrée** sur `(i−1, i+1)`, ratio (pas %) ; 0 si Δdist = 0
(`gpx/.../Path.kt:249-262`). Calculée sur le profil courant, donc après lissage 150 m dans le
pipeline : c'est ce lissage qui stabilise les pentes vues par la physique. Le détecteur de montées
exprime la pente en % (`engine/.../climb/ClimbDetector.kt:173`).

## 8. D+ / D−

Deux grandeurs coexistent, volontairement :

1. `Path.elevationGain/Loss` : somme brute de tous les deltas, sans seuil (`Path.kt:230-245`) —
   gardée comme témoin, et utilisée par le `ClimbDetector`.
2. `ElevationGain` (R27) : **accumulateur à points de retournement avec hystérésis**
   (`gpx/.../ElevationGain.kt:153-249`) sur une copie lissée à sa propre échelle.

Presets (seuil, lissage) — `ElevationGainOptions.kt:22-45` :

| Preset | Seuil | Lissage | Usage |
|---|---|---|---|
| `raw` | 0 | 0 | somme brute |
| `barometric` | 2 m | 15 m | seuil Strava baro |
| **`dem`** (défaut) | **3 m** | **30 m** | altitudes issues du MNT |
| `gps` | 10 m | 50 m | seuil Strava GPS |

Pièges documentés :
- Le filtre naïf `if (dEle > seuil) gain += dEle` donne **zéro** sur une montée régulière
  échantillonnée à 2 m (`ElevationGain.kt:40-46`). Il faut suivre les extrema locaux.
- Prologue `dir == 0` : sans lui, un parcours qui commence par un creux le compte comme une montée
  (`ElevationGain.kt:167-169`) ; flush final, sinon une arrivée au sommet perd la dernière montée
  (`ElevationGain.kt:238-246`).
- Le D+ est une mesure dépendante de l'échelle (effet « côte de Bretagne », Kollár, arXiv
  1011.4778) : il faut toujours l'annoncer avec son échelle et son seuil
  (`docs/guides/elevation.md:75-85`).
- Le lissage et le seuil attaquent le même bruit ; une fois lissé à 150 m, tout seuil de 0 à 10 m
  donne la même valeur. Un seuil fort (10 m) **efface** le relief réel d'un parcours plat (6 m → 0)
  (`docs/guides/elevation.md:131-147`).
- Le bruit MNT est **spatialement corrélé** (surface bilinéaire sur des poteaux de ~13.5 m), pas
  blanc : d'où 3 m et non les 10 m de Strava (`docs/guides/elevation.md:30-34`).

## 9. Ponts, tunnels, no-data : non traités

- **Aucune gestion des ponts ni des tunnels** dans vcyclist (recherche de `tunnel|bridge` dans
  `elevation/`, `map/`, `gpx/`, `engine/`, `docs/guides`, `docs/ledgers` : rien de pertinent). Le
  lookup MNT est fait à la coordonnée enregistrée ; un viaduc renverra le fond de vallée et un tunnel
  le sommet de la montagne. Le seul palliatif implicite est le lissage 150 m.
- **No-data** : rien n'est détecté côté `:elevation`. Une tuile en échec lève une exception qui fait
  échouer tout le calcul (`ElevationCalculator.kt:31-33`, `DiskCachedElevation.kt:29-30` : « errors
  stay loud »). `SrtmMapProducer.kt:77` affirme que le provider renvoie NaN hors couverture, mais je
  n'ai trouvé aucun chemin de code produisant NaN dans `:elevation` **(observation)**.
- Les tags OSM `highway`/`surface` sont lus seulement s'ils sont déjà présents dans le GPX
  (exports gpx.studio) et servent à la largeur de route, pas à l'altitude
  (`gpx/.../gpx/OsmHighway.kt`, ledger R26 `docs/ledgers/improvements-ledger.md:598-640`).
  `surface` est parsé mais ignoré ; 4965/4966 points `asphalt` sur l'échantillon de 128 km.

## 10. Ce qui a été mesuré (et rejeté)

| Ligne | Hypothèse | Résultat | Référence |
|---|---|---|---|
| R28 | La fenêtre de lissage compte | 150 m retire −1.5 % (route MNT propre) à −48 % (altitude GPS) du D+ ; 17.6 % d'écart de durée sur Stelvio entre 10 et 300 m (lacets) | `improvements-ledger.md:1033-1063` |
| R29 | Recaler le point sur la route dans le MNT (corridor ±15 m) | ❌ Le tracé enregistré est déjà le minimum de rugosité de son corridor ; tout décalage dégrade D+ et rugosité | `docs/guides/elevation.md:254-284` |
| R30 | Monter au-delà de z12 | ❌ z12→z15 : 0.8 % d'écart sur le D+, rien sur la durée, ×4 tuiles par niveau. z12 ≈ résolution native de la source | `docs/guides/elevation.md:197-215,292-300` |
| — | MNT vs baromètre | Le MNT **surestime de 35 %** le D+ en montagne (854 m vs 634 m baro sur `strava.gpx`) ; ni la résolution ni le recalage ne l'expliquent | `docs/guides/elevation.md:197-201,275-279` |

Piège sur R29 : choisir le meilleur décalage **indépendamment par station** produit une trace en
dents de scie (rugosité 9.3 vs 0.23) ; il faudrait une pénalité de régularité sur les décalages
eux-mêmes (`docs/guides/elevation.md:281-284`).

Sur les lacets, une cellule MNT mélange route et talus : le profil échantillonné oscille ; une
partie de ce que retire le lissage est artefact, une partie est réelle, et rien ne permet de
trancher sans vérité terrain (`improvements-ledger.md:1058-1062`).

## 11. Paramètres numériques notables

| Paramètre | Valeur | Référence |
|---|---|---|
| Zoom MNT / taille tuile | 12 / 512 px | `ElevationProviderConfig.kt:14,17` |
| Cache LRU mémoire | 100 tuiles | `ElevationProviderConfig.kt:15` |
| Tuiles en parallèle | 10 | `BatchCalculator.kt:27` |
| Timeouts HTTP | 10 s connexion, 30 s requête | `TileFetcher.jvm.kt:16,39` |
| Pas de densification (lib / pipeline) | 10 m / 30 m | `ElevationProvider.kt:73`, `Enhancer.kt:114` |
| Distance min entre points | 1 m | `ElevationProvider.kt:76` |
| Lissage physique | 150 m demi-largeur | `ElevationStep.kt:21` |
| Lissage D+ / seuil (preset dem) | 30 m / 3 m | `ElevationGainOptions.kt:42` |
| Douglas-Peucker 3D | 10 m, exagération ×3 | `BatchCalculator.kt:10-14` |
| Points min pour lisser | 3 | `Constants.kt:38` |
| Tolérance de test altitude | ±1 m | `CLAUDE.md:257` |
| Tests live | Mont Blanc ~4805 m, mer Morte ~−430 m, Badwater ~−85 m, ±50 m | `elevation/README.md` |

## 12. Transposable en Rust vs spécifique

**Transposable directement** (logique pure, quelques centaines de lignes) :
- tuiles XYZ, conversion lat/lon → tuile/pixel flottant, décodage Terrarium, bilinéaire avec
  débordement inter-tuiles ;
- cache LRU de tuiles décodées avec déduplication des chargements concurrents (en Rust : `moka`
  avec `get_with`/`try_get_with` fournit exactement ce comportement) ;
- cache disque « décoder avant d'écrire, écriture atomique, auto-réparation » ;
- groupement des points par tuile + parallélisme borné (`tokio` + `futures::stream::buffer_unordered`
  ou un `Semaphore`) ;
- densification métrique, noyau triangulaire en distance, accumulateur D+ à hystérésis, pente
  centrée, Douglas-Peucker (la crate `geo` fournit haversine, densification et `Simplify`, mais en
  2D ; le DP sur profil (distance, altitude) se réécrit trivialement) ;
- décodage image : crate `image` (WebP lossless, PNG) — un seul décodeur natif, donc le problème
  d'octet-exactitude multi-plateforme disparaît, mais un test de non-régression par empreinte sur une
  tuile de référence reste une bonne garde ; `reqwest` pour HTTP.

**Spécifique à vcyclist, à ne pas reprendre** :
- toute la machinerie multiplateforme (JS/Node/WASI, trois décodeurs, façades générées) ;
- le lissage à 150 m, dimensionné pour la physique d'un simulateur, pas pour décrire un parcours ;
- le `Path` à taille fixe et ses champs générés.

## Enseignements pour osm-route-details

1. **Mapterhorn z12/512 est un bon point de départ** : source fusionnée mondiale, résolution
   native atteinte à z12, aller plus loin coûte ×4 tuiles par niveau pour <1 % d'effet mesuré. Garder
   le zoom configurable. Prévoir l'attribution dans la réponse de l'API et respecter les conditions
   d'usage (cache disque obligatoire pour un service). Évaluer séparément si Mapterhorn publie des
   PMTiles/COG téléchargeables pour un service auto-hébergé (non étudié par vcyclist).
2. **Ponts et tunnels sont le vrai trou** : vcyclist ne les traite pas et en souffre (MNT +35 % en
   montagne). C'est précisément là qu'osm-route-details apporte de la valeur : identifier les
   segments `bridge=*`/`tunnel=*` (et `layer`, `covered`, éventuellement `man_made=bridge`) puis
   **remplacer l'altitude MNT sur ces segments par une interpolation linéaire entre les altitudes
   MNT des culées/portails**, mesurées légèrement à l'extérieur de l'ouvrage. Faire cette correction
   **avant** tout lissage, sinon le creux du viaduc contamine 2×W mètres de profil.
3. **Densifier à pas métrique** (10–30 m) avant le lookup ; travailler en distance curviligne, jamais
   en nombre de points, pour que lissage et D+ soient invariants au rééchantillonnage.
4. **Bilinéaire vrai**, avec position flottante et débordement inter-tuiles ; **vérifier la
   convention centre/coin de pixel** de la source (vcyclist semble décalé d'un demi-pixel).
5. **No-data explicite** : définir une stratégie (valeur sentinelle Terrarium, tuile 404, NaN →
   `null` dans la réponse + interpolation sur trous courts) plutôt que l'échec global de vcyclist.
6. **Exposer brut et lissé séparément**, et annoncer l'échelle : renvoyer l'altitude brute (après
   correction ouvrages d'art), une altitude lissée (fenêtre paramétrable), et un D+/D− calculé par
   accumulateur à hystérésis avec `(seuil, fenêtre)` explicites dans la réponse. Défaut raisonnable
   pour du MNT : 3 m / 30 m. Ne pas utiliser le filtre naïf par delta.
7. **Pente** en différence centrée sur le profil lissé, en distance ; pour des segments OSM, une pente
   moyenne par segment (Δalt/longueur) est plus parlante qu'une pente par point.
8. **Ne pas tenter de recaler la trace sur la route via le MNT** : mesuré inutile (R29). Le
   map-matching OSM sert à la typologie, pas à améliorer l'altitude.
9. **Cache disque robuste dès le départ** : clé `{host}/{z}/{x}/{y}`, décoder avant d'écrire, écriture
   atomique, supprimer et re-télécharger une entrée corrompue, ne jamais cacher un échec ; un LRU
   mémoire de tuiles décodées par-dessus, borné en octets plutôt qu'en nombre (~3 Mo par tuile
   512 px décodée).
10. **Tests de référence** : reprendre les points de contrôle de vcyclist (Mont Blanc, mer Morte,
    Badwater, ±50 m) en tests d'intégration hors CI, et une empreinte SHA-256 d'une tuile décodée en
    test unitaire hors ligne (tuile embarquée).
