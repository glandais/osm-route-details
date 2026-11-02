# 04 — Conception technique : élévation et profil

> Statut : conception technique, sans code. Exigences : [01](../01-conception-generale.md) (§3–§6, §10,
> §11) et [02](../02-contrat-interface.md) (§3.2, §4.2–§4.5). Composants couverts (doc 01 §4) :
> **Chaîne élévation**, **Fusion du profil**, **Détecteurs heuristiques**, **Sélection des points de
> rupture**. Les adaptateurs, l'accès données, le référentiel de couverture et la typologie sont
> décrits ailleurs. Ce document fixe seulement ce qu'il en consomme (« Interfaces »).
>
> Références : `vc:` = `/Volumes/Data/code/perso/vcyclist` (6.11.0), avec `G/` =
> `gpx/src/commonMain/kotlin/io/github/glandais/engine/path/`, `E/` =
> `engine/src/commonMain/kotlin/io/github/glandais/engine/`, `S/` =
> `elevation/src/commonMain/kotlin/io/github/glandais/elevation/`. `os:` = `/Volumes/Data/code/perso/osm-surface`.
> Chaque paramètre porte son origine : **[vc]** (mesuré dans vcyclist), **[os]** (osm-surface) ou
> **[cal]** (valeur de départ, à calibrer sur nos bancs, doc 01 §8.5). Tous les paramètres vivent
> dans `ElevationRules`, versionnée par `rules_version` et publiée dans `/v1/capabilities`.

## 1. Vue d'ensemble et modules

Ordre de doc 01 §3 (étapes 2 à 5) : `grid` → `sampling` (MNT + origine, puis MNS) → `fusion`
(ouvrages OSM, heuristiques + `reconcile`, trous, avec le résultat de la typologie) → `derive`
(lissage, D+/D−, pentes) → [`build_segments`, T03 §8] → `breakpoints` → Assembleur (étapes 4b à 9
de T01 §5.1).

| Module (crate, T01 §2) | Composant doc 01 | Nature |
|---|---|---|
| `ord-core::elevation::grid` | Chaîne élévation | Pur : grille de stations |
| `ord-pipeline::elevation` (`sample_raw`) | Chaîne élévation | **Seul module async** : lecture MNT/MNS par `TerrainReader`, plages de terrain |
| `ord-core::elevation::fusion::{osm, deck, tunnel, merge, gaps}` | Fusion du profil | Pur |
| `ord-core::elevation::heuristics::{deck_detector, tunnel_detector, gallery_detector}` | Détecteurs heuristiques (V1) | Pur ; tablier et tunnel portés de vcyclist, couverture (galerie) propre au service (§4.2 bis) |
| `ord-core::elevation::derive::{smooth, gain, grade}` | Fusion du profil (dérivés) | Pur |
| `ord-core::elevation::breakpoints` | Sélection des points de rupture | Pur |
| `ord-core::elevation::rules` (`ElevationRules`, dans `Rules`, T01 §9) | — | Paramètres versionnés, sérialisables (`serde`) |

Crates : **`tokio`**/**`futures`** (lectures concurrentes, annulation ; comme `os:Cargo.toml`),
**`thiserror`** (erreurs typées), **`serde`** (publication des règles et paramètres effectifs),
**`tracing`** (journal **sans coordonnées**, doc 01 §8.4), **`smallvec`** (raisons d'un point). Pas
de crate géo : médianes de 5 à 7 valeurs, interpolation et haversine à la main, comme osm-surface
(`os:src/geo.rs:11-17`). Tests : **`proptest`** (invariances), **`insta`** (bancs), **`criterion`**.

Modules purs sur `Vec<f64>`, `NaN` = absent (comme vcyclist), comparaisons écrites pour qu'un `NaN`
échoue (`!(x > t)`, `vc:G/DeckProfileBuilder.kt:175-178`) ; `Option` aux frontières du module.

## 2. Grille de stations et échantillonnage

### 2.1 Grille (`grid`)

- La grille **est** `PreparedRoute::samples`, construite par `ord-core::prepare` (T01 §5.1.1).
  `grid` n'y ajoute que l'index des stations uniformes, pour les détecteurs, et le rang de chaque
  point d'entrée, pour la sortie. Elle ne fait aucun rééchantillonnage.
- Stations **uniformes** à `k × station_step_m`, avec **`station_step_m = 5 m`** [vc]. C'est le
  `stepM` de `DeckDetectorOptions` et de `TunnelDetectorOptions` (`vc:G/DeckDetectorOptions.kt:30`,
  `vc:G/TunnelDetectorOptions.kt:21`), et les fenêtres de ces détecteurs comptent des stations.
  Position : interpolation linéaire lat/lon entre les deux points d'entrée qui encadrent la
  distance, sur la **trace d'origine** et non sur la trace appariée (doc 01 §3). C'est le schéma de
  `alongTrackStations` (`vc:E/path/BridgeDeckStep.kt:53-77`).
- Chaque point d'entrée est **aussi** une station exacte (lue au point même ; fusionnée avec une
  station uniforme à moins de 1 cm). La grille garde les indices uniformes (détecteurs) et la station
  de chaque point d'entrée (sortie). Taille : 40 001 + 16 000 stations au plus (doc 02 §7), quelques Mo.

Le pas de 5 m vaut pour tout le profil. vcyclist lit à 30 m puis raffine
(`vc:E/ElevationCorrection.kt:82`) ; ici, 5 m coûte du calcul mais pas de tuiles (une tuile IGN z15
fait ≈ 870 m), et supprime la marge de relecture `refreshM` (`vc:E/path/TunnelStep.kt:77-80`).

### 2.2 Lecture du MNT (`sampling`)

1. L'adaptateur d'élévation (`TerrainReader::sample_terrain`, T02 §6.3) lit le **MNT** à toutes
   les stations. Il applique lui-même la politique de doc 01 §5.1 : pas de requête IGN hors du
   contour LiDAR ; un point dont un voisin bilinéaire est no-data est relu **entièrement** dans
   Mapterhorn ; une tuile IGN en échec est relue dans Mapterhorn et marquée `fallback_after_error`.
   Il renvoie pour chaque station un `TerrainSample { ele, origin, fallback_after_error, dem_kind,
   missing }`, où `missing` vaut `NoData` (absence par conception) ou `Failed` (panne).
2. L'origine publique est **déjà qualifiée** par l'adaptateur avec le référentiel de couverture
   (T02 §7.3) : `ign_lidar_hd` ; `mapterhorn_measured` en zone France hors GLO-30 ; `mapterhorn`
   sinon ; `none`. `dem_kind` ∈ {`Dtm`, `Glo30`, `Seam`, `Unknown`} (doc 01 §11.1-1 : index de
   couverture z12, polygones, couture de 150 m). `measured` vaut vrai pour les deux premières
   origines (`vc:S/TerrainOrigin.kt:15-60`). Les plages hors France viennent de `CoverageRanges`
   (T01 §5, étape 3).
3. `TerrainRange` = suites de stations de même `(origin, fallback_after_error)`, chaque station
   valant la moitié de ses intervalles (comme vcyclist, exploration 01 §2.5) : les plages somment à la
   longueur. Même construction pour les plages GLO-30 et hors France (`outside_primary_zone`).

### 2.3 Lecture du MNS (`sampling`)

- Le MNS n'est lu **qu'à l'IGN**, sans aucun repli (`vc:S/IgnLidarHdLayer.kt:16-21`). On le lit aux
  stations d'un pont OSM **et** à ses marges de culée (§3.2). En V1, quand les heuristiques sont
  actives, on le lit en plus à toutes les stations uniformes dont l'origine est `ign_lidar_hd`
  (doc 01 §5.1 : « et, en V1, pour les heuristiques »).
- Heuristiques actives : lecture du MNS **en parallèle de la typologie** (les ponts OSM en sont un
  sous-ensemble). Sans heuristiques (MVP), seconde phase courte, limitée aux tuiles des ponts appariés.
- Une station dont le MNT n'est pas `ign_lidar_hd` (ou vient d'un repli après erreur) reçoit un MNS
  `NaN` avant tout usage. C'est la règle « ride no deck over a foreign one »
  (`vc:E/path/BridgeDeckStep.kt:152-154`) et l'invariant 10 de doc 01 §5.5.
- Un échec de tuile MNS ne fait pas échouer la requête : les ponts touchés passent en interpolation
  (`dsm_unavailable`). L'échec reste **local au pont**, là où vcyclist renonce à tous les tabliers
  (`vc:E/ElevationCorrection.kt:104-108`).
- Résultat : `RawProfile` (MNT, MNS creux, terrain par station, plages), jamais modifié ensuite ; il
  sert aux détecteurs et au mode `debug`.

## 3. Correction des ouvrages OSM (`fusion::osm`, `fusion::deck`, `fusion::tunnel`)

### 3.1 Groupes d'ouvrages et axe

- La typologie fournit des `Structure` (T03 §6.3) : un ouvrage = suite d'éléments contigus, par
  exemple un viaduc découpé en plusieurs ways, ou un pont puis un tunnel (invariant 3), avec une
  identité stable `StructureId`, un axe OSM orienté A → B avec chaînage, et deux `StructureEnd`. Ses
  `StructurePassage` donnent la plage sur le parcours, le chaînage `(d_m, chaînage)` et les doutes.
  Le **groupe** de la fusion (`StructureGroup`, interne) = un `Structure` et ses passages ; les
  parties sur le parcours sont les `StructureElement` projetés par le chaînage du passage. Une
  extrémité **hors du parcours** (le chaînage du passage n'atteint pas 0 ou `length_m` ; trace qui
  commence au milieu d'un viaduc, invariant 2) est lue sur `Structure::axis` entre la fin du passage
  et la culée ; le point `beyond` est `StructureEnd::approach[0]`, juste après la culée sur la voie
  qui continue.
- Deux groupes séparés par moins de `2 × abutment_probe_m` sont fusionnés : la sonde de l'un
  tomberait dans l'autre.
- **Tunnels en série** (règles 2026.10.10, décision de l'utilisateur du 2026-11-01) : sur une route
  en falaise (Combe Laval, Petits Goulets), des tunnels OSM sont séparés par des balcons à l'air
  libre non tagués où le MNT lit la paroi, pas la chaussée (26–29 m d'écart E4 sur `combe-laval#2`
  et `#3` en 2026.10.9). Deux passages simples consécutifs dont le premier **sort** d'un tunnel et
  le second **entre** dans un tunnel, séparés sur le parcours par moins de
  `tunnel_series_gap_m = 120 m` [cal] (`CorrectionRules`, T07 §4), forment un même groupe lu sur le
  parcours, de proche en proche (une série de n tunnels). Le tronçon entre eux doit être apparié de
  bout en bout : une portion non appariée ou sans donnée coupe la série. La droite va du portail
  extérieur d'entrée au portail extérieur de sortie, **balcons compris**, comme les spans de
  vcyclist (`vc:G/TunnelDetectorOptions.kt`, `minValleyHeightM` : terrasse gardée dans le span). La
  sonde de portail n'est lue qu'aux deux bouts de la série ; la garde de pente
  (`max_tunnel_chord_grade`) juge la droite de la série entière ; un doute sur un passage, une sonde
  sans altitude ou une plage sans typologie laissent toute la série non corrigée. Les balcons
  restent des routes : ni segment ni entrée de `summary.structures[]` n'y est créé, seule leur
  altitude suit la droite (`ele_method = interpolated_structure`, et donc celle d'un segment de
  balcon, T06 §2). Un ouvrage à plusieurs passages n'entre jamais dans une série (comme pour la
  fusion ci-dessus).
- **Axe du groupe** : stations à 5 m sur l'extension amont (géométrie OSM), puis celles du parcours
  dans `[from_m, to_m]`, puis l'extension aval. Sur le parcours, on lit toujours sur la trace
  (la lecture sur la géométrie OSM reste V1.x, doc 01 §11.2).
- **Sondes de culée et de portail** : une station à `abutment_probe_m` au-delà de chaque extrémité
  (sur le parcours, sinon depuis `beyond`), avec `max(10 m, 2 × pixel lu)` [cal] : 10 m sur IGN z15,
  ≈ 30 m sur Mapterhorn z12, le `refreshM` de vcyclist contre le front de taille
  (`vc:E/path/TunnelStep.kt:51-56`). Les stations entre extrémité et sonde sont corrigées.
- **Pas de correction en cas de doute** (invariant 4) : MNT brut et `structure_uncorrected` sur la
  plage si un passage du groupe porte un `PassageDoubt` (T03 §6.3 : appariement incertain, portion
  `unknown` qui le touche), si une sonde n'a pas d'altitude, si une extrémité est `EndKind::Unresolved`
  (hors des tuiles), ou si le groupe touche une plage de `TypologyCoverage`.
- **Invariant 8** (même altitude à chaque passage) : le profil d'un groupe est calculé **une fois
  par `StructureId`**, en chaînage d'axe (orientation canonique A → B de la typologie). On choisit le
  passage qui couvre la plus grande part de l'axe, et le premier en cas d'égalité. Les autres
  passages lisent ce profil par leur abscisse d'axe. C'est déterministe.

### 3.2 Pont, terrain IGN : profil de tablier (`dsm_deck`)

C'est un portage du constructeur de vcyclist (`vc:G/DeckProfileBuilder.kt:76-115`). Comme le
préconise l'exploration 01 (enseignement 3), l'emprise OSM remplace les tests « tall », de
rugosité et de canyon du détecteur. Les étapes, sur l'axe du groupe :

1. **Éligibilité** (invariant 10). Toutes les stations de l'axe et des deux sondes doivent avoir
   une origine `ign_lidar_hd` sans repli d'erreur, et toutes les tuiles MNS doivent avoir été lues.
   Sinon, on passe à l'interpolation (§3.3), avec `dsm_unavailable` si c'est une panne.
2. `MNH = MNS − MNT`. Si `MNT > MNS + 0,5 m`, ce qui est physiquement impossible, la station est
   traitée comme haute (`mntAboveMnsAsTall`, `vc:G/DeckDetectorOptions.kt:116-126`) [vc].
3. **Pont sans sursol** (doc 01 §5.5) : moins de `min_deck_presence = 30 %` [cal] des stations de
   l'axe ont `MNH > deckMnhM = 1 m` [vc] (`:58-59`), **et** il n'y a pas de canyon. On dit qu'il n'y
   a pas de canyon quand le MNT minimal sous l'axe n'est pas plus de `minCanyonDropM = 3 m` [vc]
   (`:84`) sous le plus bas des deux MNT de sonde. Dans ce cas, l'altitude du sol est gardée
   (`ele_method = dem` aux stations), le pont est compté comme traité, et son issue est
   `OutcomeMethod::KeptGround`, exposée `kept_ground` dans `summary.structures[].method` et dans les
   compteurs de `quality.structure_correction` (décision doc 01 §10, contrat §4.4–4.5).
4. `mf` = médiane glissante du MNS sur 25 m, qui ignore les `NaN`. La rugosité est le RMS de
   `MNS − mf` sur 35 m. Une station est **propre** si le maximum de rugosité sur la fenêtre de 35 m
   est sous `maxRoughnessM = 0,8 m` [vc] (`:39-50`, `vc:G/DeckDetector.kt:470`).
5. **Ancres** : les stations propres, avec un MNS fini et `MNS ≥ MNT − 0,5 m`. Les treillis et les
   haubans sont rugueux et n'ancrent donc jamais. S'il y a moins de 2 ancres, on passe à
   l'interpolation (`vc:G/DeckProfileBuilder.kt:150-155`).
6. **Profil** entre la première et la dernière ancre. On prend le MNS aux ancres, linéaire entre
   elles, puis une médiane sur 25 m qui ne lit jamais au-delà des ancres. On applique ensuite le
   **plafond de pente** `maxSlopeP95 = 10 %` [vc] : c'est le plus petit majorant 10 %-lipschitzien,
   obtenu en deux passes linéaires (`capSlope`, `vc:G/DeckProfileBuilder.kt:183-233`). Il ne fait que
   remonter : un creux est comblé, un pic est élargi en tente.
7. **Rampes** linéaires entre chaque sonde (MNT) et l'ancre la plus proche (`withEnds`,
   `vc:G/DeckProfileBuilder.kt:48-57`). Si le groupe touche un tunnel, il n'y a pas de sonde de ce
   côté : le tablier est tenu plat jusqu'à la jonction, comme vcyclist en bout de parcours (`:214-215`).
8. **Cohérence MNS/MNT** [cal] : on abandonne le profil pour l'interpolation (avec journal) si une
   rampe dépasse `max_ramp_grade = 15 %` (ancre sur une superstructure lisse, ou pont au-dessus de la
   trace), ou si plus de 10 % des stations ont un tablier sous `MNT − 0,5 m`. **Exception** : la
   surface continue au-delà du way quand le MNS de la sonde ne descend pas sous le tablier plus vite
   que `max_ramp_grade` (ouvrage qui se prolonge sans tag, way plus court que le pont) ; le tablier
   est alors tenu plat jusqu'à l'extrémité du way et la rampe ne commence qu'au-delà. Elle ne vaut
   que pour un tablier au-dessus du sol de la sonde (rampe qui descend), et le test est
   **unilatéral** (règles 2026.10.7) : le MNS est une surface au-dessus du sol, un MNS de sonde plus
   haut que le tablier (garde-corps, véhicule, végétation sur un pixel) ne dit jamais que la surface
   s'arrête. Un tablier sous le sol de la sonde (tablier absent du MNS, qui lit l'eau d'une écluse)
   n'est jamais prolongé. Le test symétrique faisait passer en interpolation un pont plat dont le way s'arrête
   avant la culée réelle : sonde sur le vide de culée (MNT 4 m sous le tablier), MNS 2 m au-dessus
   (`nv-13`, banc 2026.10.6, tablier interpolé 3 m sous le baromètre).

Ligne de base vcyclist (exploration 01 §4) : Saint-Nazaire à 0,54 m de la vérité au pire au lieu
de 65,8 m dessous ; D+ de 0,74 à 1,08 × vrai. OSM règle les échecs du **détecteur** (ponts courts,
superstructures) mais pas ceux du **constructeur** (creux longs et lisses de Cheviré, toit lisse
fusionné, arche de plus de 10 % plafonnée par l'étape 6, §8).

Première mesure du banc (`bench/README.md` §1.5) : au milieu des grands ponts (Saint-Nazaire,
Pirmil, Anne-de-Bretagne, Mauves), le MNS LiDAR est à ≤ 1 m du baromètre corrigé. Au pont
Général-Audibert, le MNS est 2,4 à 4,5 m au-dessus du baromètre sur 5 passages, et la médiane sur
25 m (11,54 m, valeurs de 11,2 à 11,7 m) ne fait pas mieux que la lecture ponctuelle : le biais n'est
pas un pixel isolé mais le MNS lui-même sur cet ouvrage. Cause ouverte : voie cyclable plus basse que
la surface lue, ou décalage local du baromètre. Le constructeur garde malgré tout ancres propres
(étapes 4–5) et médiane sur 25 m (étape 6), qui protègent des artefacts ponctuels. Un biais continu
de ce type n'est pas corrigé au MVP : il se verra en E3 contre le baromètre (bench §3).

### 3.3 Repli d'interpolation (`interpolated_structure`)

- **Pont sans MNS exploitable** (hors LiDAR, MNS en échec, moins de 2 ancres, incohérence) : droite
  entre les MNT des deux sondes, pratique GraphHopper/Valhalla (22 à 40 m d'erreur sur les grands
  ponts bombés).
- **Tunnel** : droite entre les MNT des sondes de portail, **juste à l'extérieur** du way, sur toute
  source ; jamais de MNS (`TunnelStep.write`, `vc:E/path/TunnelStep.kt:178-191`).
- **Groupe mixte** : chaque pont garde son tablier s'il en a un ; chaque tunnel est une droite entre
  ses voisins connus (fin de tablier ou sonde) ; sans aucun tablier, une seule droite de sonde à sonde.
  Le patch du groupe porte **un `StructureOutcome` par suite d'éléments de même nature**, contigus,
  chacun avec sa méthode (pont `dsm_deck`, tunnel `interpolated_structure`, par exemple) : c'est la
  forme de `summary.structures[]` pour un ouvrage mixte (décision doc 01 §10, T06 §2), sans liste
  d'éléments dans le contrat.
- **Plausibilité** : si la pente de la droite dépasse 12 % sur un tunnel (`maxChordGrade`,
  `vc:G/TunnelDetectorOptions.kt:60`) [vc] ou 10 % sur un pont (`maxSlopeP95`) [vc], on émet
  `structure_slope_implausible` et on **garde** la valeur (doc 01 §6).

### 3.4 Pont au-dessus de la trace au sol (`fusion::overpass`, règles 2026.11.6)

- **Défaut corrigé** (décision de l'utilisateur, 2026-11-01, après la calibration
  2026.11.5-bench1). Le MNT LiDAR HD de l'IGN est **interpolé sous les tabliers** : les points
  sol manquent sous le pont, et l'interpolation s'appuie sur les remblais d'accès et les culées,
  classés sol. Sous un pont qui enjambe la voie suivie, le MNT lit donc une **bosse** de quelques
  mètres là où la chaussée est plane. Le service la rendait telle quelle (le passage dessous n'est
  pas un ouvrage, invariant 6). Ni `no-data` ni trou comblé ne la signalent : c'est la géométrie OSM
  qui la trahit.
- **Repérage** : les croisements de la trace avec les lignes `bridge` des tuiles lues, toutes
  classes (route, voie fermée au vélo, rail), viennent de la typologie (`overpasses`, T03 §6.4). Un
  croisement est retenu si la trace y est **au sol** : tronçon apparié à une voie de niveau sol
  hors élément d'ouvrage, ou voie fermée au vélo suivie de niveau sol (`OffGraph`) ; jamais une
  portion non appariée ou sans donnée (doute : invariant 4).
- **Bornes, tirées de la géométrie d'un ouvrage** (`CorrectionRules::overpass`), jamais des cas du banc :
  - `deck_half_width_m = 15 m` : demi-largeur d'un tablier porté par **une** ligne OSM (2 × 3
    voies, bandes d'arrêt et trottoirs ≈ 30 m ; une chaussée séparée est cartographiée par deux
    lignes, dont les fenêtres se réunissent) ;
  - `min_angle_deg = 20°` : un pont qui enjambe une voie la croise franchement ; en deçà, la trace
    longe le pont ou est dessus (même borne que le banc, README §4 ter.7, choisie séparément) ;
    l'emprise du tablier sur la trace vaut `deck_half_width_m / sin(angle)`, de 15 m à 90° à
    ≈ 44 m à 20° ;
  - `end_clearance_m = 1 m` : un croisement plus proche d'une extrémité réelle de la ligne du pont
    est sa culée (la trace rejoint le pont ou le frôle, nœud partagé) ; moitié de la plus petite
    ouverture sous tablier d'une voie cyclable (≈ 2 m de piste), car le way d'un petit pont peut
    n'être pas plus long que l'ouverture qu'il franchit (première valeur, 2,5 m pour une ouverture
    supposée de 5 m, corrigée : elle écartait un passage de piste dont les ways du pont s'arrêtent à 1,8 m de la trace) ; une extrémité posée sur une autre
    ligne de pont (way découpé au-dessus de la voie) n'est pas une culée (T03 §6.4) ;
  - sonde : `abutment_probe_min_m = 10 m` au-delà de l'emprise, comme les culées (§3.1) : la
    droite va de l'approche au sol amont à l'approche aval, soit 25 m de part et d'autre à 90°,
    ≈ 54 m à 20° ;
  - `min_bump_m = 1 m` : rugosité du MNT LiDAR d'une chaussée (approche lisse à 1 m du détecteur
    de couverture, §4.2 bis). Une bosse moindre n'est pas distinguable du bruit ; rien n'est
    réécrit. Pas de borne haute : sous un pont, la chaussée ne peut pas monter vers le tablier ;
    le gabarit (≥ 4,5 m) et l'épaisseur du tablier bornent la bosse lue, sans servir de seuil.
- **Garde-fous** : aucune correction si la fenêtre (approche à approche) touche un passage
  d'ouvrage OSM (la trace sur un pont ou dans un tunnel garde la correction de son ouvrage ; un
  pont `kept_ground` compte), une plage sans typologie, une station déjà corrigée (tablier,
  ouvrage heuristique : seules les stations `dem` sont lues et réécrites), ou une station hors IGN
  LiDAR HD (le défaut est celui de ce MNT ; Mapterhorn et GLO-30 ne sont pas corrigés). **Seule la
  bosse est retirée** : une station sous la droite (cuvette, passage creusé sous l'ouvrage) garde
  son altitude ; un vrai relief sans pont au-dessus n'est jamais lu.
- **Écriture : altitude seule**, après les ouvrages OSM et heuristiques, **avant** les
  couvertures (§4.2 bis) : une couverture qui touche, approches comprises, une fenêtre corrigée est
  écartée (le pont OSM confirme ce que la couverture devine). Stations au-dessus de la droite
  ramenées dessus, méthode `gap_filled`, avertissement `elevation_overpass_corrected` (api 1.3) sur
  la plage réécrite ; ni ouvrage, ni verdict, ni changement de `structure`. Désactivée avec
  `structure_correction: false` (MNT brut, comme les ouvrages).
- **Tests** : unitaires sur le réseau `overpass` (bosse retirée ; cuvette, vraie bosse sans pont,
  trace sur le pont, bosse sous 1 m, correction désactivée, terrain hors LiDAR : rien) ; golden
  `tt04-sous-pont` (témoin `tt04-tuileries` de vcyclist, calibration : bosse de 2,6 m retirée).
  Cas du banc qui ont motivé la règle (calibration 2026.11.5-bench1, non utilisés pour régler les
  bornes) : 14 croisements à bosse de 2,5 à 11 m (README §4 ter.7).

### 3.5 Interfaces

```rust
// Consommés de T03 §6.3 : Structure, StructureId, StructureKind, StructureElement, StructureEnd,
// EndKind, StructurePassage, PassageDoubt ; GroupRef de T03 §7.1.
struct StructureGroup<'a> { structure: &'a Structure, passages: Vec<&'a StructurePassage>,
    doubtful: bool /* un PassageDoubt, extrémité Unresolved ou plage de TypologyCoverage */ } // interne
pub struct CorrectionPatch { pub target: GroupRef, pub from_m: f64, pub to_m: f64,
    pub values: Vec<f64>, pub methods: Vec<EleMethod>,
    pub outcomes: Vec<StructureOutcome> /* une par suite d'éléments de même nature, contiguës */ }
```

## 4. Heuristiques V1 et fusion avec OSM (`heuristics`, `fusion::merge`)

### 4.1 Détecteur de tablier (LiDAR seul)

- C'est un portage fidèle de `DeckDetector.detect` (`vc:G/DeckDetector.kt:147-232`), avec
  `DeckDetectorOptions::DEFAULT` (`vc:G/DeckDetectorOptions.kt:28-148`) comme valeurs de départ. Il
  tourne sur les stations **uniformes** seulement, avec MNT, MNS et `MNH = MNS − MNT`, et un MNS à
  `NaN` hors IGN.
- Étapes reprises : `mf` médiane 25 m, rugosité RMS 35 m ; stations hautes `MNH > 5 m` en runs
  d'au moins 15 m ; run gardé si 60 % de stations lisses (sinon `ROUGH`) ; fusion à travers moins de
  300 m ; extension le long de `MNH > 1 m` bornée à 100 m par côté ; canyon de plus de 3 m ; P95 de
  |pente| d'au plus 10 % sur le profil « ponté » (E2 ter), ou rugosité médiane sous 0,4 m si le span
  fait au plus 25 m ; nouvelle évaluation par parties d'un span fusionné trop pentu
  (`splitSteepMerged`) ; masque de tablier en runs d'au moins 40 m (tous si span ≤ 60 m).
- Fenêtres validées comme chez vcyclist (nombre entier de stations, impair pour les médianes,
  `:150-193`), mais **au démarrage**. Profil par le constructeur de §3.2 (jonction de régions `:161-180`).
- Mesuré par vcyclist : 18/21 ponts en calibration ; **série tenue à l'écart** 4 spans sur 11 (30 %
  des stations de tablier), 0 faux positif (exploration 01 §4).

### 4.2 Détecteur de tunnel (profil MNT)

- C'est un portage de `TunnelDetector.detect` (`vc:G/TunnelDetector.kt:62-142`, portails `:166-195`,
  jugement `:197-267`), avec `TunnelDetectorOptions::DEFAULT` (`vc:G/TunnelDetectorOptions.kt:19-93`).
  Il tourne sur le MNT brut des stations uniformes.
- Seuils repris : portails à côté route ≤ 20 % sur 20 m et 50 m, front ≥ 25 % sur 20 m ; paires de
  50 m à 3 km, rectitude ≥ 0,8, corde ≤ 12 % ; hauteur ≥ 15 m atteinte dès 100 m de chaque portail ;
  flancs ≥ 30 % sur 50 m ; jamais plus de 3 m sous la corde ; vallée intérieure ≥ 15 m ; garde
  « 0 m exact » (`:113`).
- **`tunnelScope = measured-terrain`**, seul mode : toutes les stations de `[portail − 100 m,
  portail + 100 m]` doivent être mesurées (`vc:E/path/TunnelStep.kt:72, 136-141`), sinon le tunnel
  est seulement journalisé. Un tunnel qui contient un tablier (OSM ou heuristique) est abandonné :
  piles gardées par le MNT (`:132-133`). Écriture : droite entre MNT des portails (`:178-187`).
- Mesuré par vcyclist : 7/8 en réglage (IGN), 6/7 hors échantillon, 0 faux tunnel, 0 station de sol
  déplacée sur les négatifs ; Grands Goulets à 0,9 m au lieu de 220 m.
- Les heuristiques ne tournent que si `structure_correction = true` et là où la typologie a été lue
  (ni panne ni `typology_missing`, doc 01 §6 : « élévation non corrigée »).

### 4.2 bis Détecteur de couverture (galerie, règles 2026.11.4 ; altitude seule depuis 2026.11.5)

- **Pourquoi.** Les galeries pare-avalanche ou pare-pierres (`tunnel=avalanche_protector`) sont
  absentes du schéma Shortbread des tuiles : la typologie les lit comme des routes à l'air libre.
  Leur toit, couvert de terre et raccordé au versant, est classé « sol » par le LiDAR : le MNT y lit
  le toit, en plateau de quelques mètres au-dessus de la route, et le MNS l'égale. Ni le détecteur
  de tablier (MNS ≈ MNT) ni celui de tunnel (15 m de hauteur, flancs de montagne) ne le voient.
  vcyclist n'a pas de détecteur de galerie (ses galeries restent `X`, exclues) : ce détecteur est
  propre au service (`ord-core::elevation::heuristics::gallery_detector`), sur les stations
  uniformes du profil brut, MNT et MNS de l'enveloppe LiDAR (`NaN` hors IGN : pas de galerie hors
  IGN LiDAR HD).
- **Forme cherchée** : un portail d'entrée (dernière station de route avant la marche) et un
  portail de sortie (première après), avec :
  - approche de chaque portail : 20 m de route lisse (aucune station à plus de 1 m de sa droite),
    pente ≤ 15 % ;
  - marche vers l'intérieur : ≥ 3 m au-dessus du prolongement de la route en ≤ 15 m, dont ≥ 2 m
    sur un seul pas de 5 m ; dans une suite de portails candidats consécutifs, la station la plus
    proche de la marche ;
  - droite entre portails : pente ≤ 15 %, à ≤ 6 points de pente de chaque approche ; longueur de
    15 m à 1 km ; la sortie la plus proche qui convient ;
  - toit (de la première à la dernière station à 3 m au-dessus de la droite) : hauteur médiane de
    4 à 12 m, ≥ 80 % de stations à plus de 2 m ; aucune station à plus de 2 m sous la droite ; sur
    les stations hautes, hors stations de portail, pente propre ≤ 3 m sur la longueur et aucun écart
    > 3 m à sa droite ;
  - MNS ≈ MNT sur le toit : médiane de (MNS − MNT) ≤ 2 m.
- **Origine des bornes** (physique d'une galerie et échantillonnage, pas les cas de calibration) :
  hauteur ≥ 4 m = gabarit routier (4,30 m au moins, 4,50 à 4,75 m d'ordinaire) plus la dalle, moins
  le lissage du MNT ; ≤ 12 m = gabarit, dalle (≤ 1,5 m) et coussin de terre amortisseur de quelques
  mètres, sous les 15 m du détecteur de tunnel ; marche de 3 m = trois quarts de la hauteur
  minimale (le reste se perd au lissage du front) ; saut de 2 m sur 5 m = 40 % de pente relative,
  le double de la plus forte pente d'une route ouverte ; fenêtre de 15 m = front vertical lissé
  par le MNT à 1 m, station à un demi-pas du front, front biais ; pente de route ≤ 15 % et
  changement ≤ 6 points (raccords verticaux lents : sous une galerie, la route garde sa pente) ;
  approche lisse à 1 m (MNT LiDAR d'une chaussée) ; 20 % de stations basses tolérées (galerie
  ouverte côté vallée, trace décalée qui lit la pente dessous) ; pente propre et écart du toit
  ≤ 3 m (épaisseur variable du coussin ; une bosse arrondie, un versant ou une crête ne sont pas
  un toit) ; MNS − MNT ≤ 2 m, moitié de la hauteur minimale (garde-corps, arbustes ; un bâtiment,
  une forêt ou un tablier sur le plateau l'excèdent) ; longueur ≥ 3 pas pour que deux stations
  lisent le toit, ≤ 1 km pour borner l'appariement.
- **Garde-fous** (fusion) : une couverture qui touche un passage OSM (pont ou tunnel : l'ouvrage
  OSM prime) ou un tablier détecté, approches de 20 m comprises (plateau entre deux ponts : une
  levée), ou qui recouvre un tunnel détecté (le tunnel prime), ou qui touche une plage sans
  typologie, est écartée. Remblai, rampe, crête,
  tranchée lue de biais : pas de marche verticale ou toit non parallèle ; bâtiment, forêt :
  MNS ≫ MNT ; pont emprunté : la route passe sous la droite.
- **Écriture : altitude seule** (décision de l'utilisateur, 2026-11-01, règles 2026.11.5). Hors
  OSM, aucun critère général ne distingue une galerie d'un passage inférieur ou d'une couverture
  non cartographiée lus dans le MNT : toutes les détections sont donc corrigées **en altitude
  seule**, jamais rendues comme ouvrage. Droite entre les stations à `abutment_probe_min_m`
  au-delà des portails (sondes restées `dem`), sur les stations restées `dem`, après tous les
  ouvrages (`fusion::heuristic::correct_covers`) ; méthode `gap_filled` ; avertissement
  `elevation_cover_corrected` (api 1.2) sur la plage. Ni `reconcile`, ni verdict, ni entrée de
  `summary.structures[]` : `structure` et `structure_source` des segments ne changent pas. Une
  galerie taguée dans OSM et lue dans les tuiles reste un tunnel OSM (§3).
- **Couvertures voisines fusionnées** (accepté par l'utilisateur, 2026-11-01) : deux couvertures
  séparées par moins de 20 m de route (l'approche exigée) ne donnent pas deux portails : si le
  plateau qui les réunit passe les critères (la route à l'air libre entre elles compte comme
  stations basses, ≤ 20 %), une seule droite les couvre, tronçon intermédiaire compris (la route
  y est sur la droite à quelques décimètres près).
- **Limite** : un toit qui redescend progressivement à une sortie (pas de marche nette) n'est pas
  détecté ; la marche n'est pas assouplie (décision de l'utilisateur, 2026-11-01). Cas : `nv-30`,
  qui reste en limite des données au banc.
- Mesuré en calibration (prototype puis service, 2026-11-01, rapport 2026.11.4) : `arly-galerie`
  (22 m) et `nv-37` (122 m) détectées, `nv-30` (256 m) non (pas de marche nette à la sortie : le toit y redescend
  sur 25 m) ; la galerie de `tn12-arly-galleries` détectée (témoin sorti de S2, README §4 ter.6) ;
  aucun autre témoin négatif touché. Sur les traces perso de calibration, d'autres plateaux de
  25 à 120 m sont détectés hors de tout ouvrage connu ; là où le baro existe, il ne voit pas la
  marche du MNT (couverture non cartographiée ou passage inférieur lu dans le MNT).

### 4.3 Fusion OSM ↔ heuristique

Tout est calculé sur le profil **brut**. Les détections sont converties en `DetectedStructure`,
puis la fusion appelle `reconcile` (Classification, T03 §7.1), **seul** propriétaire de la règle et
de ses paramètres (`RuleSet`) ; le tableau en est le résumé. Recouvrement d'une détection H et d'un
passage OSM O : `r = |H ∩ O| / min(|H|, |O|)` (`confirm_overlap`), sur les plages sans les marges.

| Cas | Décision |
|---|---|
| H et une partie de O de **même nature**, avec `r ≥ 0,5` [cal] | O devient `osm_heuristic` ; le profil de O (§3) s'applique ; H est écarté |
| H recouvre O de nature opposée (tablier sur tunnel OSM, ou l'inverse) | OSM l'emporte, H est écarté (journal et métrique de conflit) |
| H dépasse O de plus de `untagged_min_m` = 40 m [vc, `deckMinRunM`] d'un côté | Le reste devient un ouvrage `heuristic` contigu, et le groupe est recalculé d'un seul tenant (invariant 3) |
| H ne touche aucun ouvrage OSM (sol apparié ou portion `unknown`) | Ouvrage `heuristic` **corrigé** (doc 01 §10, Q17), avec l'avertissement `structure_untagged` sur sa plage |
| Ouvrage OSM non détecté | Reste `osm`, jamais mis en doute (doc 01 §5.5) |

Chaque décision est un `StructureVerdict` (T03 §7.1), rendu dans `ElevationProfile::verdicts` et
consommé par `build_segments` (T03 §8) pour découper les segments.
Invariant 8 : deux détections de passages différents à moins de 20 m partout [cal] sont unifiées
(profil de la plus longue).

### 4.3 bis Mise en œuvre V1 (règles 2026.11.2)

- `ElevationRules::heuristics = true` ; MNS lu sur l'enveloppe LiDAR (`DsmScope::LidarHull`,
  `ord-pipeline::elevation::sample_dsm_hull`) : toutes les stations dont le MNT est IGN LiDAR HD
  sans repli, dans la branche « terrain » de l'étape 4b, après le MNT et **en parallèle de la
  typologie** ; seulement si `structure_correction = true`. Valeurs dans `RawProfile::dsm`, une par
  station (`NoData` ailleurs). Le MNS des ponts OSM (étape 4c) reste lu par `sample_structures`.
- Détecteurs (`ord-core::elevation::heuristics`) sur les stations uniformes (`k × 5 m`, à 1 cm) du
  profil **brut** ; MNS `NaN` hors IGN mesuré sans repli (invariant 10). Détections qui touchent
  une plage `TypologyCoverage` écartées. Tunnel gardé si toutes les stations à ± 100 m de chaque
  portail sont mesurées (`ign_lidar_hd`, `mapterhorn_measured`) et s'il ne contient ni pont OSM ni
  tablier détecté. Couverture gardée hors de tout passage OSM et de tout tablier (approches
  comprises) et de tout tunnel détecté, corrigée en altitude seule (§4.2 bis, règles 2026.11.5).
- `reconcile` (T03 §7.1) : confirmation `osm_heuristic` à 50 % ; nature opposée écartée ; parts
  d'une détection hors des passages OSM qu'elle touche gardées au-delà de 40 m
  (`UNTAGGED_MIN_M`), détection sans passage OSM gardée entière : verdicts `heuristic`.
- Correction d'un ouvrage `heuristic` (`fusion::heuristic`), après les ouvrages OSM, sur les seules
  stations restées `dem` : sondes à `abutment_probe_min_m` au-delà de chaque extrémité, lues sur le
  profil déjà corrigé ; pont : constructeur de tablier de §3.2 (sinon droite entre sondes, ou sol
  gardé : ouvrage écarté) ; tunnel : droite entre sondes. Sans sonde ou sans altitude de sonde, l'ouvrage n'est ni
  corrigé ni rendu. Issue `summary.structures[]` de source `heuristic`, avertissement
  `structure_untagged` sur la plage ; `build_segments` impose `structure` et `structure_source`
  sur la plage du verdict (revêtement, `way_kind` et `matched` gardés).
- **Raccord rampe OSM / tablier détecté** (règles 2026.11.3) : une station corrigée par un pont OSM
  hors de ses passages (rampe ou marge vers sa sonde au sol) est reprise par un tablier détecté qui
  la recouvre, et une sonde du tablier détecté posée sur une telle rampe recule jusqu'au pont OSM :
  la rampe ne plonge plus sur un viaduc non tagué qui prolonge le way (Saint-Nazaire nord). Les
  stations des passages OSM ne sont jamais réécrites. Golden `test1-raccord-acces-nord`.
- Non fait en V1 : unification des détections de passages différents à 20 m (invariant 8 sans
  identité), recalcul d'un seul tenant d'un ouvrage OSM prolongé par une détection (la part
  heuristique est corrigée seule, sondée sur le profil corrigé de l'ouvrage OSM).

### 4.4 Interfaces

```rust
// StructureSource, DetectedStructure, StructureVerdict, reconcile : T03 §7 (Classification)
pub struct DeckDetectorOptions { /* champs de vc:G/DeckDetectorOptions.kt:28-148, en mètres */ }
pub struct TunnelDetectorOptions { /* champs de vc:G/TunnelDetectorOptions.kt:19-93 */ }
pub struct GalleryDetectorOptions { /* §4.2 bis, bornes physiques documentées champ par champ */ }
pub fn detect_decks(mnt: &[f64], mns: &[f64], o: &DeckDetectorOptions) -> DeckDetection;
pub fn detect_tunnels(mnt: &[f64], lat: &[f64], lon: &[f64], measured: &[bool],
    o: &TunnelDetectorOptions) -> Vec<TunnelSpan>;
pub fn detect_galleries(mnt: &[f64], mns: &[f64], o: &GalleryDetectorOptions) -> Vec<GallerySpan>;
// DeckDetection, TunnelSpan et GallerySpan (internes) → Vec<DetectedStructure> avant reconcile
```

## 5. Profil corrigé, méthode par point, dérivés (`fusion`, `derive`)

### 5.1 Application et méthode

1. Départ : `ele = MNT brut`, `method = dem` (ou `none`).
2. `CorrectionPatch` appliqués par `from_m` croissant ; ils ne se recouvrent pas (§3.1, §4.3), sinon
   `ElevationError::Internal`.
3. **Comblement des trous courts** (`gaps`), **après** les corrections (doc 01 §3, étape 4) : une
   suite de `none` d'au plus `gap_fill_max_m = 100 m` [cal], encadrée de valeurs, est interpolée
   linéairement (`gap_filled`, avertissement `elevation_gap_filled`). Ce seuil est publié dans
   `/v1/capabilities`. Au-delà, on garde `none` : plage `elevation_missing`, ou `elevation_unavailable`
   si l'absence vient d'une panne acceptée par `accept_partial`.
4. `ele_method` par station ∈ {`dem`, `dsm_deck`, `interpolated_structure`, `gap_filled`, `none`}
   (`gap_filled` aussi sur une couverture, §4.2 bis, et sous un pont au-dessus de la trace, §3.4) ;
   rampes et marges de sonde prennent celle de leur patch. `structure_correction = false` : ni patch
   ni heuristique.

### 5.2 `quality.structure_correction`

Statut par nature (`bridges`, `tunnels`), vocabulaire de `vc:G/ElevationCorrections.kt:7-30`, par
priorité : `disabled` (option à `false`), `unavailable` (typologie absente sur tout le parcours),
`none_found`, `partial` (au moins un ouvrage non corrigé), `applied`. Un pont sans sursol
(`kept_ground`) compte comme corrigé. On tient des compteurs par méthode (`dsm_deck`,
`interpolated_structure`, `kept_ground`, `uncorrected` : noms du contrat §4.5) et par
`StructureSource`, comptés par entrée de `summary.structures[]`.

### 5.3 Lissage optionnel

`elevation.smoothing_m > 0` ajoute `ele_smoothed` : noyau triangulaire **métrique** de vcyclist
(`vc:S/ElevationSmoother.kt:32-112`, intégrale exacte, renormalisé aux bouts, invariant par
rééchantillonnage), `smoothing_m` = demi-largeur. Jamais à travers une plage `none`, toujours
**après** les corrections (invariant 1), sans toucher `ele`.

### 5.4 D+/D− à hystérésis

- On utilise l'accumulateur à points de retournement de `ElevationGain` (`vc:G/ElevationGain.kt:218-...`,
  prologue `dir == 0` et jambe d'ouverture compris), sur une **copie privée** lissée à l'échelle du
  preset (`:48-55`), sur `ele` corrigé et non lissé par l'option client. Le résultat dépend seulement
  des extrema, donc il ne dépend pas de l'échantillonnage.
- Presets [vc] (`vc:G/ElevationGainOptions.kt:15-46`), seuil / lissage : `raw` 0 / 0, **`dem`
  (défaut) 3 m / 30 m**, `barometric` 2 m / 15 m, `gps` 10 m / 50 m. vcyclist le dit : 3 m est « un
  point de départ défendable, pas une mesure ».
- **Adaptation automatique sur GLO-30** (doc 01 §10, Q12) : τ(station) = seuil du preset, ou
  `glo30_threshold_m = 10 m` sur GLO-30 (BRouter ≈ 10 m sur SRTM, exploration 04 §5) [cal].
  Plages GLO-30 : stations `Glo30` ou `Seam` d'origine Mapterhorn, trous de moins de 200 m fermés,
  plages de moins de 200 m écartées [cal]. Une jambe est confirmée quand le retour depuis l'extremum
  atteint `max(τ(extremum), τ(courant))` : à cheval sur une frontière, le seuil le plus prudent
  s'applique. Pas d'adaptation sous `raw` ; un seuil de preset plus haut n'est pas abaissé. Sortie :
  `gain_params.adapted[] = {from_m, to_m, dsm: "glo30", threshold_m}` et `dsm_source` sur ces plages.
- **Trous** : l'accumulateur redémarre après chaque plage `none` ; sommes additionnées, `partial`.
- **Pentes** : `grade_pct(d0, d1) = 100 × Δele / Δd` sur `ele` corrigé non lissé, `None` si une borne
  est `none` (utilisé par l'Assembleur pour `segments[]`). Tous les agrégats sont calculés sur les
  stations, jamais sur `points[]` (doc 01 §5.6).

### 5.5 Interfaces

```rust
pub enum EleMethod { Dem, DsmDeck, InterpolatedStructure, GapFilled, None }   // = doc 02 §4.2
// TerrainOrigin, DemKind : ord-domain::elevation (T02 §6.3)
pub struct StationTerrain { pub origin: TerrainOrigin, pub fallback_after_error: bool, pub dem_kind: DemKind }
pub struct TerrainRange { pub from_m: f64, pub to_m: f64, pub origin: TerrainOrigin,
    pub measured: bool, pub fallback_after_error: bool }
pub struct GainParams { pub preset: GainPreset, pub threshold_m: f64, pub smoothing_m: f64,
    pub adapted: Vec<AdaptedRange>, pub partial: bool }
pub struct AdaptedRange { pub from_m: f64, pub to_m: f64, pub threshold_m: f64 } // sérialisé avec "dsm": "glo30"
pub struct GainResult { pub gain_m: f64, pub loss_m: f64, pub params: GainParams,
    pub turning_points: Vec<TurningPoint> /* réutilisés par breakpoints */ }
pub struct StructureOutcome { pub target: GroupRef, pub kind: StructureKind, pub from_m: f64, pub to_m: f64,
    pub source: StructureSource, pub method: OutcomeMethod }
pub enum OutcomeMethod { DsmDeck, InterpolatedStructure, KeptGround, Uncorrected(UncorrectedReason) }
pub enum CorrectionStatus { Applied, Partial, NoneFound, Disabled, Unavailable }
pub struct StructureCorrectionReport { pub bridges: CorrectionStatus, pub tunnels: CorrectionStatus,
    pub counts: BTreeMap<&'static str, u32> }
pub struct ElevationProfile { pub grid: Arc<StationGrid>, pub ele: Vec<f64>, pub method: Vec<EleMethod>,
    pub terrain: Vec<StationTerrain>, pub ele_smoothed: Option<Vec<f64>>, pub gain: GainResult,
    pub terrain_ranges: Vec<TerrainRange>, pub missing: Vec<MissingRange>,
    pub structures: Vec<StructureOutcome>, pub verdicts: Vec<StructureVerdict> /* T03 §7.1 */,
    pub correction: StructureCorrectionReport, pub warnings: Vec<Warning> /* T01 §5 */ }
impl ElevationProfile { pub fn at(&self, d_m: f64) -> PointElevation; pub fn grade_pct(&self, d0: f64, d1: f64) -> Option<f64>; }
```

## 6. Sélection des points de rupture (`breakpoints`)

Mode `input_with_breakpoints` ; `input` renvoie `at(d_i)` par point d'entrée dédoublonné ;
`distance` renvoie `at(k × step_m)` pour `k = 0..⌊L/step_m⌋`, plus `L` si le dernier pas n'y tombe
pas, avec `lat`/`lon` interpolés et sans `origin`. `at(d)` interpole linéairement entre les deux
stations qui encadrent `d`. La méthode retenue est celle de la station la plus proche, `none` si
l'une des deux est `none`.

### 6.1 Événements candidats

Événements `(d_m, raison, classe, poids)`, tirés du profil corrigé et des segments typés :

| Raison | Source | Classe |
|---|---|---|
| `structure_start` / `structure_end` | Bornes de chaque ouvrage (`StructureOutcome`, sans les marges), coupées au parcours. Deux ponts contigus gardent leur borne commune. | A (fait) |
| `elevation_gap_start` / `_end` | Bornes de `MissingRange` après comblement, portées par la dernière (première) station lue qui les encadre, pour que le point ait une altitude (incrément 5) | A |
| `terrain_source_change` | Bornes des `TerrainRange` (changement d'`origin`) et des plages GLO-30 retenues (§5.4) | A |
| `overpass_crossing` | Croisement d'un pont qui enjambe la trace au sol (§3.4 : angle ≥ 20°, plus de 1 m d'une culée, trace au sol hors ouvrage), avec ou sans correction de la bosse et avec ou sans `structure_correction`, si le profil a une altitude au croisement. Le point porte l'altitude du profil en ce point, celle du mode `distance`, au lieu de la droite entre les points d'entrée qui l'encadrent (décision de l'utilisateur du 2026-11-02, échec E5 de `nv-423` : 1,30 m ; règles 2026.11.8, api 1.4) | A |
| `surface_change` | Frontière où `surface.class` change entre deux segments | B |
| `local_max` / `local_min` | Points de retournement confirmés par l'accumulateur de D+ (§5.4), recalés sur l'argmax ou l'argmin de `ele` non lissé à ±`smoothing` du preset | C, poids = amplitude de la jambe |
| `grade_change` | Voir ci-dessous | C, poids = \|Δpente\| |

**Seuil de signification des extrema** : c'est le seuil effectif du D+ (3 m, ou τ adapté sur
GLO-30). Chaque jambe comptée dans `gain_m` a donc ses deux extrémités dans `points[]`. Un client
qui somme les dénivelés entre points retrouve le D+ du `summary`, aux jambes éliminées par la borne
de densité près.

**`grade_change`** : Douglas-Peucker **vertical** (erreur en altitude) sur `ele`, entre points fixes
consécutifs (points d'entrée, événements A et B, extrema), tolérance `grade_tolerance_m = 2 m` [cal].
Un sommet n'est retenu que si les deux pièces font au moins `grade_min_run_m = 50 m` [cal] et que leur
pente diffère d'au moins `grade_min_delta_pct = 2 points` [cal]. Vaut aussi **dans un ouvrage
corrigé** (tablier bombé). Récursion dans l'ordre des indices : déterministe. Un sommet de
corde qui ne remplit pas ces conditions arrête la recherche sur sa plage (incrément 5).

### 6.2 Déduplication et densité

1. **Fusion** : événements à moins de `merge_m = 5 m` [cal] → un point, union des raisons, position
   de l'événement prioritaire (structure > trou > terrain > pont au-dessus > revêtement > extremum >
   pente ; puis plus
   petite distance).
2. **Rattachement** : à moins de `snap_m = 5 m` [cal] d'un point d'entrée (le plus proche, le
   précédent en cas d'égalité), l'événement lui donne ses raisons sans créer de point ; le point
   d'entrée n'est jamais déplacé (doc 01 §5.6). Un `overpass_crossing` n'est rattaché qu'à moins de
   `overpass_snap_m = 0,5 m` : à quelques mètres, la bosse ou la cuvette du MNT sous le tablier
   change déjà l'altitude (`nv-423`, point d'entrée à ≈ 5 m du croisement, règles 2026.11.8).
3. **Espacement** : un événement C à moins de `min_spacing_m = 5 m` [cal ; 20 m au départ, ramené à
   5 m sur `test1..4` à l'incrément 5, un sommet remplacé par un point d'entrée voisin faussant le D+
   des points] d'un point retenu (entrée,
   A ou B) est abandonné. A n'est jamais abandonné ; B seulement par la fusion.
4. **Plafond** : au plus `max_inserted_per_km = 20` points insérés par km [cal] sur toute fenêtre
   glissante d'un km, et au plus 16 000 points insérés au total (borne d'entrée, doc 02 §7). Au-delà,
   on retire d'abord les `grade_change`, puis les extrema, par poids croissant, puis par distance
   décroissante. Les classes A et B ne sont jamais retirées.
5. **Sortie** triée par `d_m`. Un point inséré reçoit `lat`/`lon` interpolés sur la trace
   d'origine, `ele`, `ele_smoothed` et `ele_method` de sa station, et `index` = dernier point
   d'entrée tel que `d ≤ d_m`.

**Déterminisme** : `f64` séquentiel, ordre de réduction fixe, égalités tranchées par indice de
station, `BTreeMap`/`Vec` (jamais l'ordre d'un `HashMap`), aucun parallélisme dans la partie pure.

### 6.3 Interfaces

```rust
pub enum BreakpointReason { LocalMax, LocalMin, GradeChange, StructureStart, StructureEnd,
    SurfaceChange, TerrainSourceChange, ElevationGapStart, ElevationGapEnd }
pub struct OutputPoint { pub d_m: f64, pub index: u32, pub origin: Option<PointOrigin> /* None en mode distance */,
    pub reasons: SmallVec<[BreakpointReason; 2]>, pub latlon: Option<LatLon>,
    pub ele: Option<f64>, pub ele_smoothed: Option<f64>, pub ele_method: EleMethod }
pub enum PointOrigin { Input, Inserted }
pub fn select_breakpoints(p: &ElevationProfile, segs: &[TypedSegment], r: &BreakpointRules) -> Vec<OutputPoint>;
pub fn sample_output(p: &ElevationProfile, mode: &Sampling, segs: &[TypedSegment], r: &ElevationRules) -> Vec<OutputPoint>;
```

## 7. Interfaces globales, erreurs, concurrence, performance, cas limites

### 7.1 Ce que le module expose et consomme

```rust
// Consommé — Orchestrateur (T01 §0, §4.2, §5) : PreparedRoute/RouteSample, RequestCtx (Deadline,
//   DataLog), RequestOptions (smoothing_m, gain_preset, structure_correction), Sampling, GainPreset,
//   CoverageRanges (plages hors France), Warning/WarningCode.
// Consommé — Adaptateur élévation (T02 §6.3) : trait TerrainReader { sample_terrain, sample_ign_dtm,
//   sample_dsm, pixel_size_m }, TerrainSample { ele: Option<f32>, origin, fallback_after_error,
//   dem_kind, missing: Option<Missing> }, DsmSample { Value, NoData, Unavailable }, TerrainOrigin, DemKind.
// Consommé — Chaîne typologie / Classification (T03) : TypologyOutput (runs, structures, passages,
//   coverage), Structure*, StructurePassage, StructureKind, StructureSource, DetectedStructure,
//   StructureVerdict, GroupRef, reconcile, TypedSegment (après build_segments), SurfaceClass.
// Exposé
pub struct ElevationRules { pub station_step_m: f64 /* 5 */, pub heuristics: bool /* MVP false, V1 true */,
    pub deck: DeckBuilderRules,
    pub deck_detector: DeckDetectorOptions, pub tunnel_detector: TunnelDetectorOptions,
    pub gap_fill_max_m: f64, pub gain: GainRules /* presets, glo30_threshold_m */,
    pub breakpoints: BreakpointRules }                       // Serialize, partie de Rules (T01 §9)
pub async fn sample_raw(route: &PreparedRoute, reader: &dyn TerrainReader, ranges: &CoverageRanges,
    ctx: &RequestCtx, r: &ElevationRules) -> Result<RawProfile, ElevationError>; // ord-pipeline::elevation, 4b
pub async fn sample_structures(raw: &mut RawProfile, scope: DsmScope<'_>, reader: &dyn TerrainReader,
    ctx: &RequestCtx, r: &ElevationRules) -> Result<(), ElevationError>;  // 4c : MNS (+ MNT des extensions)
pub enum DsmScope { Structures /* MVP : sample_structures */, LidarHull /* V1 : sample_dsm_hull, RawProfile::dsm */ }
pub fn fuse(raw: &RawProfile, typo: Option<&TypologyOutput> /* None : typologie absente */,
    opts: &RequestOptions, rules: &Rules, budget: &dyn Budget)
    -> Result<ElevationProfile, ElevationError>;                       // ord-core::elevation
#[derive(thiserror::Error)] pub enum ElevationError { Timeout, Cancelled, Internal(String) }
```

### 7.2 Erreurs

Une panne absorbée par un repli est une donnée (`fallback_after_error`, `terrain_fallback`). Une
station `missing = Failed` après repli est une **panne**, jamais une `ElevationError` : l'orchestrateur
(`decide_outcome`, T01 §5.3) en fait un 503 sans `accept_partial` ; avec, station `none` et
`elevation_unavailable`. `NoData` (tuile absente, hors extrait) donne
`elevation_missing` et un 200 `partial`. Le MNS en échec n'est jamais une erreur (§2.3). La partie
pure ne lit rien ; ses incohérences (patchs qui se recouvrent, tailles) sont des `Internal`, testées
par `proptest`.

### 7.3 Concurrence et performance

- `sampling` groupe les stations par tuile, lit en concurrence sous la cadence **globale** de
  l'accès données, respecte `ctx.deadline` (échéance et annulation, T01 §3.3 ; 504, doc 02 §5.2).
- Partie pure dans `tokio::task::spawn_blocking`, un fil par requête, O(n·w) (w ≤ 13 stations ou
  fenêtre de lissage) : cible < 50 ms pour 56 000 stations [cal, `criterion`].
- **Risque** : le détecteur de tunnels est en O(E·X·L) (paires à moins de 3 km, jugement linéaire,
  `vc:G/TunnelDetector.kt:127-140`), jusqu'à 10⁹ opérations sur un terrain pathologique. Parades :
  sommes préfixes pour la hauteur et « jamais sous la corde », et plafond `max_tunnel_pairs` [cal]
  au-delà duquel les tunnels heuristiques sont sautés (journal, métrique, déterministe).
- Coût réseau du MNS « partout » pour les heuristiques V1 : environ +35 Mo à froid sur 130 km (vcyclist).

### 7.4 Cas limites

| Cas | Traitement |
|---|---|
| Parcours de 2 points très proches (moins de 5 m) | Grille réduite aux deux points d'entrée ; pas d'heuristique (fenêtres trop courtes) ; D+ sur 2 stations |
| Trace qui commence ou finit sur un pont | Axe prolongé par la géométrie OSM jusqu'à l'extrémité (§3.1). Sans `beyond` : `structure_uncorrected` |
| Tunnel dont un portail est en no-data | `structure_uncorrected` (invariant 4), jamais de 0 m |
| Pont OSM au-dessus de la trace | Écarté par la typologie (invariant 6), et le MNS n'est lu qu'aux stations d'un pont apparié (§2.3). Sinon, la rampe de plus de 15 % le rejette (§3.2, étape 8). Cas du banc : piste sous Cheviré, MNS à 57 m au croisement pour un sol à 6,9 m (E5). Depuis 2026.11.6, la bosse du MNT LiDAR sous son tablier est retirée (§3.4) |
| Plage GLO-30 de moins de 200 m (littoral) | Pas d'adaptation. La provenance la montre quand même |
| Trou long au milieu d'un ouvrage | L'ouvrage est remplacé en entier si ses sondes sont valides. Sinon, il n'est pas corrigé |

## 8. Points de tension

1. **Invariant 9 et heuristiques V1** : le détecteur de tablier lit le MNS sur tout le terrain IGN
   (autorisé par doc 01 §5.1). C'est une donnée de détection, jamais écrite comme altitude hors
   ouvrage. Coût à froid : T02 §10, sous le budget de 90 s (T01 §3.3).
2. **Invariant 8 et lecture sur la trace** : deux passages ne lisent pas le même MNS ; il faut un
   profil par ouvrage, donc une **identité stable** et un **chaînage d'axe** : fournis par T03 §6.3
   (`StructureId`, `StructurePassage::chainage`). Sans identité (heuristique seule), l'unification à
   20 m reste imparfaite.
3. **Invariant 2 et extrémité hors parcours** : il faut lire MNT et MNS sur la géométrie OSM de
   l'extension, alors que cette lecture attend une mesure (V1.x) ; on la limite au hors-trace
   (géométrie : `Structure::axis`, `beyond` : `StructureEnd::approach`, T03 §6.3). Le MNT et le MNS de
   l'extension sont lus par `sample_structures` (étape 4c), après la typologie : une seconde lecture
   courte, sur le chemin critique à froid, y compris au MVP.
4. **Plafond de pente à 10 %** : les arches plus raides (Pont-Saint-Esprit) restent lissées, même
   avec l'emprise OSM ; seuil à recalibrer.
5. **Seuil de D+ variable** : pas de référence publiée pour une hystérésis à τ par plage (BRouter :
   seuil par source) ; `max(τ_ext, τ_courant)` et 10 m sont des choix non mesurés.
6. **Typologie absente → pas d'heuristiques** : les détecteurs n'ont pas besoin d'OSM, mais doc 01
   §6 impose « élévation non corrigée » ; on s'y tient.
7. **`structure_correction: false` → « `dem` partout »** (doc 02 §5.3) : compris « sur les
   ouvrages » ; `gap_filled` et `none` restent possibles.
8. **Points de rupture et D+** : la borne de densité peut retirer des extrema ; la somme des
   dénivelés de `points[]` s'écarte alors du `summary` (voulu, mais surprenant).
9. **Pire cas des tunnels** (§7.3) : le plafond de paires peut sauter la détection sur un parcours
    inhabituel.
10. **Ordre ouvrages puis trous** (doc 01 §3) : une sonde dans un trou court rend l'ouvrage non
    corrigé, alors que le comblement lui aurait donné une valeur. On garde l'ordre du doc.
