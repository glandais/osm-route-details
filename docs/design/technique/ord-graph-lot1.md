# Graphe vélo OSM — lot 1 : surcouche de fréquentation par équipe, rattachée au graphe

> Statut : **implémenté** (crate `ord-graph` : modules `overlay` et `util`, `GraphFile::graph_identity` ; outil
> `tools/ord-graph-overlay`) ; les écarts à cette conception et le relevé sont en fin de note (« Écarts », « Relevé »). Exigences : plan Pédalons `docs/plans/2026-11-15-generateur-parcours-osm.md`
> (§4.1, §7.1, §11, §12 lot 1, §12 bis ; ledger `API-204`). Socle : [ord-graph-lot0.md](ord-graph-lot0.md) et son relevé
> [ord-graph-lot0-mesures.md](ord-graph-lot0-mesures.md). **[mesuré]** : relevé du 15 novembre 2026 (scripts jetables hors
> dépôt, sur l'export ci-dessous et `pays-de-la-loire-20261114-z11.ordg`) ; **[estimé]** : à confirmer par l'outil.

## 0. Périmètre

- **Entrée** : l'export `prod-network.csv.gz` du banc (**privé** : lu en local, jamais copié ni envoyé ; rapports sans
  coordonnée ni id de parcours, de trace ou d'arête) et le `.ordg` du lot 0, lu par `GraphFile` (format **inchangé**).
- **Sorties** : une surcouche par équipe sous `~/.cache/osm-route-details/graph/overlays/` (« arête de base → parcours
  distincts, par type et par sens » ; arête de base = id global `u32` du `.ordg`, qu'une vue contractée future
  référencera) et un rapport JSON (§8). Hors lot : HTTP, travail planifié, goût appris (§7.2), agrégat entre équipes.

## 1. Ce que dit la donnée [mesuré]

**Export.** 3 377 lignes de données, 9 équipes (2 726, 446, 108, 71, 10, 7, 7, 1, 1), une trace par parcours ; tous `v = 1`,
`api = 1.8`, `status = ok` ; ROAD 2 451, GRAVEL 913, MTB 13 ; aucun exclu ; CSV RFC 4180 (JSON entre guillemets doublés,
LF), 981 Mo décompressés. **2 366 982 arêtes d'équipe**, 219 367 km, **354 888 `g` distinctes** (×6,7) ; `p` : 40 807
arêtes, 3 048 km ; `fn` absent 1 966, `tn` 1 869, `len` 3 814 ; `ow` `both` 2 004 127, `forward` 328 031, `against`
34 824 ; `wk` `major_road` 1 543 586, `minor_road` 488 245, `path` 127 132, `cycleway` 85 096, `track` 50 914,
`footway` 50 448, absent 21 272, `steps` 289.

**Sens des clés** (`NetworkEdgesDocument`, `TrackInputMapper` ; ici `ord-core/src/typology/edges.rs`) : `g` = **partie
parcourue**, recalée sur la rue de la tuile, dans le sens de parcours ; `fn`/`tn` indexent `nodes` (`[id z14, lat, lon,
degré]`), bouts de l'**arête complète** dans le sens de parcours, absents hors du graphe lu ; `len` = arête complète,
`trv` = mètres parcourus, `fo`/`to` = entrée et sortie depuis `fn` ; `p` dès que l'arête n'est pas parcourue de bout en
bout ; `ow` `forward` = sens unique de tuile suivi, `against` = remonté (les tuiles n'ont pas `oneway:bicycle`) ; `wk`
et surface seulement si homogènes ; éperons de moins de 5 m déjà retirés par ord-core.

**Région** (sommets de `g` dans le `.poly`) : 3 184 traces dedans, 50 en partie, **143 dehors** ; 193 048 km sur 219 367
dans la région (88 %), 192 757 à plus de 1 km du bord. …9207 : 2 705 / 20 / 1, 165 007 km sur 166 939 ; grands tours
…0796 et …4356 : 3 235 km sur 12 710 et 2 911 sur 16 626 ; …0123 et …3245 **entièrement dehors**. Réseau distinct de
…9207 : 127 729 arêtes de tuile, 15 066 km (toutes régions).

**Écart tuile / graphe OSM** (à plus de 1 km du bord, pondéré par les passages ; d₁ = distance à l'arête la plus proche) :

| Mesure | Résultat |
|---|---|
| 127 053 nœuds d'équipe → nœud OSM le plus proche | p50 0,16 m, p90 0,24, **p99 0,30** ; **99,6 % à 1 m ou moins**, 99,7 % à 3 m |
| 2 582 687 points tous les 10 m le long de `g` → arête OSM la plus proche | p50 0,09 m, p90 0,21, **p99 0,43** ; > 3 m 0,09 %, > 5 m 0,06 %, > 10 m 0,01 %, aucune à 50 m : 0 |
| idem par `wk` | homogène ; le pire, `cycleway` : p99 0,46 m, > 5 m 0,10 % |
| cap de `g` contre celui de l'arête la plus proche | p50 0,2°, p99 6,3°, > 45° 0,08 % |
| **parallèle** : arête OSM non adjacente à moins de d₁ + 5 m (et de 15 m) | **7,7 %** des points (`cycleway` 29,6 %, `footway` 9,7 %, `major_road` 8,4 %) ; à d₁ + 2 m : 0,39 % (`cycleway` 3,7 %) |
| sens de parcours fermé au vélo sur l'arête la plus proche | 0,36 % |
| classe OSM sous les points | `tertiary` 34,0 %, `secondary` 17,9, `unclassified` 16,2, `residential` 8,5, `path` 7,8, `track` 5,5, `cycleway` 4,5, `primary` 3,4, `footway` 1,2, `service` 0,7 ; accès `No` 0,22 %, `Private` 0,06 % |

Lecture : la géométrie des tuiles est **au demi-mètre** du graphe et les jonctions de tuile **sont** des nœuds OSM. Le
désaccord est topologique (une arête de tuile = p50 2, p99 7 arêtes OSM au prototype) et non géométrique ; le danger est le
**parallèle à 5 m** (trottoir, piste le long de la route), pas l'écart.

## 2. Méthode : chaîne entre bouts recalés, dans un couloir

Pour chaque `g` distincte : recaler ses bouts, chercher les chaînes OSM qui les relient **dans un couloir** autour de `g`,
garder celle qui épouse le mieux `g`.

1. **Bouts.** Nœud OSM à 1,5 m ou moins : état « nœud ». Sinon, et en plus : jusqu'à 3 positions projetées sur des arêtes
   à 8 m ou moins dont le cap au point est à ±45° de celui de `g` sur ses 10 premiers (derniers) mètres ; le signe du
   produit scalaire donne le sens a→b ou b→a sur l'arête.
2. **Couloir** : arêtes OSM entièrement à 5 m ou moins de `g` (segments de l'index, §5). Les premières versions du
   prototype donnent le même résultat à 3, 5 et 10 m (±0,04 point) : 5 m garde une marge sans admettre la piste à 8 m.
3. **Recherche** : Dijkstra par nœuds sur le seul couloir, de chaque départ vers chaque arrivée (au plus 4 × 4), coupé à
   3 × `trv` + 50 m ; même arête aux deux bouts dans le bon ordre : un seul morceau. Le sens vélo n'entre pas dans la
   recherche, il est jugé après (§3) pour être compté comme cause et non perdu.
4. **Sélection par la géométrie, pas par le coût** : Hausdorff exact (densifié à 5 cm) entre `g` et la chaîne coupée à
   ses positions, arrondi à 0,1 m, puis |longueur − `trv`|. Leçon du prototype : choisir la plus courte faisait tomber les
   arêtes de 5 à 10 m aux carrefours sur une chaîne quasi nulle (1,3 % des km en écart de longueur, 1,9 % en Hausdorff),
   tout disparaît avec ce critère.
5. **Validation** : Hausdorff ≤ 3 m et |longueur − `trv`| ≤ max(5 m, 5 %). **Ambiguë** : une autre chaîne valide, d'un
   ensemble d'arêtes différent (morceaux ≥ 1 m), à moins de 0,5 m de Hausdorff de la meilleure.

**Prototype** [mesuré] (Python, jetable ; 20 000 `g` tirées par la graine 20261115, dont 13 021 entièrement intérieures,
pondérées par les km-passages, 10 880 km) : **appariées 99,2 %**, sens fermé 0,54 %, bout sans arête 0,16 %, chaîne non
reliée 0,08 %, Hausdorff > 3 m 0,03 %, ambiguës 0,19 % (18 arêtes) ; Hausdorff p50 0,25 m, p90 0,47, **p99 0,62**, p99,9
2,8 ; longueur / `trv` p1 0,992, p50 0,996, p99 1,003 ; deux bouts sur un nœud dans 89 % des cas. Préférer, à 0,5 m près,
une chaîne au sens ouvert n'en a rattrapé que 3 sur 400 : pas retenu.

**Pourquoi pas les deux autres.** *Vote de points sur l'arête la plus proche* : simple, mais 7,7 % des points ont un
parallèle à 5 m, les points proches d'un carrefour votent pour les rues croisées, une traversée de 33 m (`footway` moyen)
n'a que trois points, et il ne rend ni ordre ni adjacence (le contrôle de continuité perdrait son sens). *HMM à la
ord-core* : son modèle de bruit (σ, β) sert une trace GPS bruitée ; ici l'entrée est déjà recalée à 0,5 m et porte ses
jonctions : on paierait candidats et transitions tous les quelques mètres pour retrouver ce que `fn`/`tn` donnent.

**Ce qu'elle rate.** Topologie changée entre tuiles et PBF (rond-point créé, route déviée : non reliée ; mais au relevé,
81 % des km `chain_not_connected` viennent de la règle des bouts — un nœud d'une autre voie à 1,5 m devient le seul état —
et sont rattrapés par la variante « nœud et positions ») ; deux voies à
moins de 0,5 m (ambiguë, non tranchée) ; tout ou rien par `g` (arête moyenne 93 m) ; un `g` qui revient à son départ n'a
que le couloir pour guide ; les identités OSM en double (3 dans le graphe) se valent.

## 3. Sens, sens uniques, arêtes partielles, comptage

- **Sens** : `g` est dans le sens de parcours, donc chaque morceau de chaîne a un sens a→b ou b→a (bit 31 de `DirEdge`).
  Un morceau d'au moins 1 m dont le sens n'est pas ouvert au vélo (`fwd_ok` / `bwd_ok`) rend l'arête d'équipe **non
  appariée, cause `direction_closed`** : le générateur ne pourra pas l'emprunter. Croisé avec `ow` (prototype : 257 cas
  sur 399 sont des `against`, contresens que les tuiles ne savaient pas dire et qu'OSM ferme ; 138 des `both`). Le
  contresens ouvert par OSM (`contraflow`) est apparié normalement et compté.
- **Partielles** : `g` étant déjà la partie parcourue, `p`, `fo`, `to` ne guident pas l'appariement ; ils servent au
  contrôle (`to − fo ≈ trv` ; bout « nœud » attendu si `fo = 0` côté départ, si `to = len` côté arrivée : écart compté).
- **Comptage** : pour un parcours, une arête de base compte dans un sens si l'union de ses morceaux dans ce sens couvre au
  moins min(½ longueur, 50 m) et au moins 1 m (bruit de recalage aux carrefours). Un parcours qui repasse ne compte
  qu'une fois par sens ; un demi-tour compte dans les deux. Les km sous le seuil sont rapportés (`below_threshold`).
- **Type** : `surface_type` (table ouverte dans l'en-tête ; inconnu → MIXED, comme `kindOf`) ; exclus : ignorés, comptés.

## 4. Causes, dans l'ordre où elles sont jugées

`outside_region` (tous les sommets hors du `.poly` : **compté à part, pas un échec**) ; `border` (arête à cheval sur la
limite, échec quelconque) ; `invalid` (polyligne illisible, `trv` négatif) ; `no_osm_edge` (un bout sans candidat, ou un
point de `g` tous les 10 m sans arête à 5 m) ; `chain_not_connected` ; `geometry` (Hausdorff ou longueur hors tolérance) ;
`ambiguous_parallel` ; `way_kind` (familles incompatibles sur plus de la moitié de la chaîne : route — `major_road`,
`minor_road` — contre non-route — `track`, `path`, `cycleway`, `footway`, `steps` ; `wk` absent : pas de contrôle ; la
signature d'une piste prise pour la route, non mesurée par le prototype) ; `direction_closed`. Tous en km-passages **et**
par `wk`.

## 5. Index spatial

`rstar` (déjà au workspace, ord-core) : un `RTree<GeomWithData<Line<[f32; 2]>, (u32 arête, u16 segment)>>` **par cellule
z11**, en mètres dans le repère local de la cellule (origine au coin, échelle cos φ du centre ; f32 : < 1 cm sur 15 km),
construit à la demande par `bulk_load`. Un segment est inscrit dans chaque cellule que sa boîte touche (arêtes à cheval,
459 traversées sans nœud, lot 0) ; une requête de rayon r interroge les ≤ 4 cellules du disque. Index par segment, pas
par arête : une arête va jusqu'à 5,6 km, sa boîte serait inutile. Mémoire : 6 099 170 segments [mesuré, 7 752 992 points
pour 1 653 822 arêtes], 24 o par entrée, ≈ 150 à 200 Mo pour toute la région [estimé, à relever] ; la France passera par
un LRU de cellules et un traitement trié par cellule (Morton). Nœuds : recherche dans les cellules du disque.

## 6. Fichier de surcouche `.ordo`

Nom `<team_id>-<nom du graphe>.ordo` ; écrit comme le graphe (`.tmp`, `fsync`, relecture, `0444`, `rename`) ; petit-boutiste.

| Partie | Contenu |
|---|---|
| En-tête (128 o) | magie `ORDOVRL1`, `format_major` / `format_minor` u16 ; **identité du graphe** : `format_major` et `attr_layout` du graphe, zoom, comptes de nœuds / arêtes / orientées, **xxh3 stockés de `ID_EDGES` et `ID_NODES`**, xxh3 et réplication du PBF (provenance) ; id d'équipe u64 ; xxh3 de l'export et lignes lues ; empreinte des paramètres d'appariement ; version de l'outil ; date ; nombre d'entrées ; table des types (≤ 8 noms) |
| Corps | entrées triées par arête : delta d'id (varint), masque u8 des couples (type, sens) non nuls, un varint par bit (comptes saturés à 65 535) |
| Pied | xxh3 de tout ce qui précède |

**Identité** : `ID_EDGES` (way, a, b par id d'arête) et `ID_NODES` (id OSM par nœud) fixent exactement ce que veut dire un
id de base ; leurs sommes sont déjà dans la table des sections, sans recalcul. Elles survivent à une reconstruction
identique (lot 0 : octets identiques hors en-tête et `STATS`), changent dès qu'une arête change. **Une surcouche dont
l'identité diffère est refusée**, comme un `format_major` inconnu. Taille : ≈ 4 à 6 o par arête, ≈ 0,5 à 1 Mo pour …9207
(130 000 à 200 000 arêtes de base) [estimé]. Ajouts à `ord-graph` : `GraphFile::identity() -> GraphIdentity` (sommes
stockées), module `overlay` (écriture, lecture vérifiée, `get(edge) -> [u16; 2 × types]` par dichotomie), et le `.poly`
et `SplitMix64` d'`ord-graph-build/util.rs` déplacés en `ord_graph::util`. Aucun `unsafe` nouveau.

## 7. Outil `tools/ord-graph-overlay`

`build --graph G.ordg --export E.csv.gz --poly P.poly --out-dir …/overlays --report R.json [--teams …] [--threads N]
[--verify-sample 2000] [--seed 20261115]` ; `inspect --overlay F.ordo --graph G.ordg`. Modules : `export` (CSV RFC 4180
écrit à la main, ≈ 40 lignes, `csv` n'étant pas au `Cargo.lock` ; `flate2` en flux ; `serde`, clés inconnues ignorées ;
polyligne 6), `index`, `matcher`, `count`, `checks`, `report`. Appariement **mémoïsé par (`g`, `wk`)**, parallèle
(rayon) sur les géométries distinctes, agrégation séquentielle : sortie identique quel que soit le nombre de fils.
`--explain <track_id>` écrit un GeoJSON de débogage dans le cache, jamais ailleurs.

## 8. Rapport JSON

Par équipe et au total : traces dedans / en partie / dehors / exclues ; **km-passages** (somme des `trv`) : total, dans la
région, appariés, comptés, sous le seuil, non appariés par cause (§4), par `wk` et par cause × `wk` ; arêtes et géométries
par cause ; **surcouche** : arêtes de base, **km de réseau** (chaque arête une fois), par sens, classe OSM, accès et type ;
parcours par arête (p50, p90, p99, max ; classes 1, 2, 3–4, 5–9, 10–19, 20–49, 50–99, 100+) ; octets ; contrôles (§9) ;
durées, `VmHWM`, identités du graphe, de l'export et des paramètres. Ni coordonnée, ni id de parcours, trace ou arête.

## 9. Deux contrôles indépendants

1. **Continuité.** Pour deux arêtes d'équipe consécutives d'une trace, **contiguës dans le document** (`tn` = `fn`, ou
   `tm` = `fm` à 1 m) et appariées : la fin de la première chaîne et le début de la seconde doivent être le même nœud OSM
   ou la même arête à 2 m près. Rapport : paires, part adjacente, part « même arête », écarts (p50, p90, max) ; adjacence
   interne des chaînes aussi (100 % par construction : un écart est un bogue).
2. **Re-vérification d'un échantillon.** 2 000 arêtes d'équipe appariées, tirées par `SplitMix64` (graine 20261115)
   **sur les passages** (le poids de la surcouche). Méthode distincte de l'appariement : ni index, ni couloir, ni repère
   local ; géométrie de la chaîne relue par `GraphFile::geometry` et coupée à ses positions ; les deux lignes densifiées à
   1 m ; **Hausdorff et Fréchet discrets en haversine**, rapport des longueurs haversine. Distributions (p50, p90, p99,
   max), parts au-delà de 1 m et 3 m. Seuil d'alerte : p99 > 1 m (prototype : 0,62).

## 10. Tests (synthétiques seulement : graphes `MemSource` → `build_graph` → `write_graph`, documents fabriqués)

1. Route A–B–C coupée en B par une allée : une arête d'équipe A→C → chaîne [AB, BC], deux bouts « nœud ». `g` décalée
   de 0,4 m et simplifiée : appariée ; décalée de 7 m : `no_osm_edge`.
2. Piste parallèle à 4 m : `g` sur la route → route, sur la piste → piste ; à 0,3 m d'écart des deux : ambiguë.
3. `oneway=yes` remonté : `direction_closed` ; avec `oneway:bicycle=no` : apparié, sens arrière compté.
4. Partielle : 30 m d'une arête de 200 m → sous le seuil, 150 m → comptée ; demi-tour → les deux sens, une fois chacun ;
   parcours passant deux fois, types différents, parcours exclu : comptes exacts.
5. Lien absent : `chain_not_connected` ; `wk = major_road` et seul un `footway` au couloir : `way_kind` ; hors du
   `.poly` : `outside_region`, à cheval : `border` ; polyligne illisible, clés inconnues, `fn` absent.
6. Surcouche : aller-retour ; autre identité de graphe, `format_major` inconnu, octet modifié, fichier tronqué → refus ;
   mêmes octets à 1 et 4 fils ; comptes saturés. Lecteur CSV : guillemets doublés, champ vide, dernière ligne sans LF.
7. Continuité : chaînes contiguës adjacentes, écart injecté détecté ; Hausdorff et Fréchet sur des cas à valeur connue.

## 11. Performance (cible : tout l'export en minutes)

981 Mo de CSV, gunzip 1,9 s [mesuré] ; JSON et index ≈ 2 à 4 s chacun [estimé] ; 354 888 géométries distinctes au lieu
de 2,37 M ; prototype Python ≈ 3 ms par géométrie [mesuré], natif ≈ 30 à 100 µs [estimé], soit 10 à 35 s sur un fil.
**Cible : moins de 2 minutes de mur et 1,5 Go de pic sur la machine du lot 0 ; au-delà de 10 minutes, le lot échoue.**

## 12. Risques et questions ouvertes

- **PBF du 14 novembre contre tuiles d'une autre date** : la date des tuiles n'est pas dans `dv` (plan d'équipe §2.2) ;
  `chain_not_connected` et `no_osm_edge` mêleront changements réels et défauts : le rapport ne sait pas les séparer.
- `direction_closed` (≈ 0,5 %) hors surcouche : si ce sont des contresens réels non tagués dans OSM, le générateur perd
  des routes que l'équipe roule ; à relire sur carte (`--explain`) avant le lot 2.
- Le lot 0 bis (branche sœur) ne doit pas renuméroter les arêtes de base sans que l'identité (§6) le dise.
- Tolérances calées sur un prototype dominé par …9207 (85 % des km de la région) : rapport par équipe obligatoire.
  `way_kind` non mesuré ; mémoire de l'index estimée ; ambiguës comptées, non tranchées (0,19 % au prototype).
- Surcouche **privée** (où roule une équipe) : jamais servie ni commitée ; son transport est une question du lot 3.

## Écarts à la conception (constatés à l'implémentation)

- **Nom** : `GraphFile::identity(edge)` existe déjà (identité d'une arête). Celle du graphe s'appelle
  `GraphFile::graph_identity() -> GraphIdentity`, avec `GraphIdentity::same_ids` (provenance exclue : `pbf_xxh3` et
  `replication_timestamp` ne comptent pas, seuls les comptes et les sommes de `ID_EDGES` / `ID_NODES`). `SplitMix64::next`
  devient `next_u64` (lint `should_implement_trait` une fois la structure exposée) ; les erreurs du `.poly` sont typées
  (`PolyError`, plus d'`anyhow` dans `ord-graph`).
- **Fichier `.ordo`** : en-tête de **256 o** (et non 128 : l'identité du graphe et la table de 8 types × 12 o ne tiennent pas),
  **masque `u16`** (8 types × 2 sens = 16 bits, et non `u8`). Le reste est tel que décrit (§6). Taille mesurée : 4,2 à 5,0 o
  par entrée (1,04 Mo pour 225 736 entrées, 174 910 arêtes de base distinctes, toutes équipes).
- **Index (§5)** : construit **d'un coup** pour toute la région, en parallèle (0,4 s, 6 115 067 segments inscrits, dont les
  copies des arêtes à cheval), et non « à la demande » : un segment doit être connu des cellules voisines qu'il touche, ce
  qui exige de lire toute la géométrie. Un `RTree` par cellule du graphe, segments `f32` ; la donnée du segment est
  l'id d'arête seul (pas de `u16` de segment, inutile à l'appariement). Pic du processus entier : 720 Mo ; **la part de
  l'index n'a pas été mesurée à part**, l'estimation de 150 à 200 Mo reste une estimation.
- **Appariement (§2)**, trois précisions :
  1. Un seul Dijkstra par couple (départ, arrivée), mais le **coût d'une arête est sa longueur majorée de l'écart moyen
     au carré** (`len × (1 + dev² / 4)`), pour départager deux parallèles dans le couloir ; la sélection finale reste celle
     de la note (Hausdorff arrondi à 0,1 m, puis écart de longueur).
  2. **Ambiguïté** : outre les autres couples, une arête **pleine** de la chaîne retenue est remplacée par un autre
     chemin du couloir (arête bannie) ; si la chaîne ainsi obtenue est valide, d'un autre ensemble d'arêtes et à moins de
     0,5 m de Hausdorff, l'arête d'équipe est ambiguë. C'est ce qui attrape deux voies entre les mêmes nœuds.
  3. **Hausdorff échantillonné à 0,25 m** (et non densifié à 5 cm), au plus 6 000 échantillons par ligne, arrêt dès que
     3 m sont dépassés : l'écart d'échantillonnage est borné par 0,125 m, sous l'arrondi à 0,1 m près.
- **Bouts** : « nœud à 1,5 m, sinon positions projetées » (lecture stricte de « sinon »). La variante « sinon, **et en
  plus** » (drapeau `EDGE_STATES_WITH_NODES`) a été **mesurée** sur l'export entier : appariés 191 610 km contre 191 557
  (+52 km), `chain_not_connected` 114 → 14 km, mais `ambiguous_parallel` 177 → 221 km et l'appariement 3,5 fois plus long
  (6,8 s contre 1,9 s). **Non retenue** ; le drapeau reste dans le code, à `false`.
- **Mémoïsation par `g` seule**, pas par (`g`, `wk`) : le contrôle `way_kind` se juge à l'agrégation, d'après les mètres
  « route » / « hors route » de la chaîne (stockés avec elle). Même résultat, moitié moins de travail.
- **Agrégation** : `count.rs` est fondu dans `aggregate.rs` ; `build_cmd.rs` et `pipeline.rs` portent l'orchestration et la
  lecture. Les paires contiguës dont une arête est hors région ou à cheval sont comptées à part
  (`skipped_outside_region`), `skipped_unmatched` ne garde que les paires entièrement dans la région.
- **Contrôles** : le tirage de re-vérification est fait **par équipe** (2 000 arêtes, graine `20261115 ^ id d'équipe`) dans
  les arêtes **finalement apparentes à la surcouche**, puis agrégé : le total mélange donc des échantillons de tailles
  différentes (12 316 sur 18 000 demandés : les équipes à moins de 2 000 arêtes appariées donnent tout ce qu'elles ont).
  Fréchet et Hausdorff sont limités à 200 points par ligne (pas de 1 m, plus grossier au-delà de 200 m).
  Ajout non prévu : contrôle des champs `fo` / `to` (§3), `partial_fields` dans le rapport.
- **Équipes sans arête dans la région** : une surcouche **vide** (264 o) est quand même écrite, pour que « pas de fichier »
  ne veuille jamais dire « équipe oubliée ».
- **Non fait** : `--explain <track_id>` (GeoJSON de débogage, donnée privée) ; `inspect` reconstitue `routes_per_edge` avec le
  plus grand des comptes par couple, faute des parcours distincts toutes catégories (le rapport de `build` le donne exact).
  Le test « mêmes octets à 1 et 4 fils » compare le contenu décodé (arêtes, comptes, empreinte des paramètres) et le
  rapport, pas les octets : l'en-tête porte la date de construction.

## Relevé du lot 1 [mesuré, 15 novembre 2026]

> **Remplacé par [ord-graph-lot1-mesures.md](ord-graph-lot1-mesures.md)** (relevé refait après la correction du sens
> fermé, par équipe, avec la ventilation des échecs). Les chiffres ci-dessous sont ceux du binaire corrigé.

Machine du lot 0 (16 fils), `--release`, export entier, `pays-de-la-loire-20261114-z11.ordg` ; rapport complet dans
`~/.cache/osm-route-details/graph/overlays/report-20261114-z11.json` (aucune coordonnée, aucun id de parcours).

| Mesure | Résultat |
|---|---|
| Durée / pic mémoire | **8,7 s** de mur (lecture de l'export 4,1 s, index 0,4, appariement 1,9, agrégation 0,7, écriture et re-vérification 1,3) ; **722 Mo** (cible : 2 min, 1,5 Go) |
| Traces | 3 184 dedans, 50 en partie, 143 dehors (comme §1) ; 3 377 lignes, aucune illisible |
| km parcourus | 219 367 ; hors région 26 298 ; **dans la région 193 069** |
| **km appariés** | **191 557 (99,22 %)** ; comptés dans la surcouche 191 238 ; sous le seuil de couverture 193 |
| Non appariés (1 512 km) | `direction_closed` 692 ; `no_osm_edge` 286 ; `geometry` 184 ; `ambiguous_parallel` 177 ; `chain_not_connected` 114 ; `border` 42 ; `way_kind` 17 ; `invalid` 0 |
| Par équipe (part appariée de la région) | 99,2 % (…9207, 165 016 km) ; 99,4 % (…4603, 19 893 km) ; 99,2 % (…4356, 2 916) ; 99,4 % (…0796, 3 236) ; 99,1 % (…4536, 1 748) ; 100 % et 99,9 % pour les deux équipes d'une trace ; …0123 et …3245 hors région |
| Par type de voie de l'équipe | non appariés, sur les km **dans la région** : `cycleway` 148 km sur 8 426 (1,75 %), `path` 156 / 12 996 (1,20 %), `footway` 31 / 2 268 (1,37 %), `track` 52 / 8 898 (0,59 %), `major_road` 781 / 107 104 (0,73 %), `minor_road` 318 / 48 109 (0,66 %) |
| `direction_closed` par `ow` (équipe 898…207) | `against` 520 km, `both` 95, `forward` 13 : la plupart sont des contresens que les tuiles ne savaient pas dire |
| Surcouche, toutes équipes | **174 910 arêtes de base** (union des équipes), 16 976 km de réseau (chaque arête une fois) ; sens a→b 139 409 arêtes, b→a 105 973 ; ROAD 151 068, GRAVEL 72 128, MTB 2 838 ; accès `yes` 174 160, `destination` 159, `private` 152, `no` 439 (l'équipe roule des voies qu'OSM ferme au vélo : à examiner au lot 2) |
| Parcours par arête | p50 3, p90 41, p99 189, max 905 ; 1 : 50 588 arêtes, 2 : 24 499, 3–4 : 22 863, 5–9 : 24 993, 10–19 : 19 453, 20–49 : 18 260, 50–99 : 8 571, 100+ : 5 683 |
| Fichiers `.ordo` | 662 610 o (…9207) ; 135 843 ; 84 363 ; 81 684 ; 64 964 ; 6 722 ; 5 211 ; 264 ×2 (vides) ; 1 041 925 o au total |
| Hausdorff de l'appariement (265 103 géométries appariées, comptées par équipe ; 224 130 distinctes) | p50 0,23 m, p90 0,42, **p99 0,87**, max 2,99 |
| **Contrôle 1, continuité** | 2 178 244 paires contiguës appariées : **adjacentes 2 177 866 (99,98 %)**, même arête 0,6 % ; 378 écarts, p50 2,8 m, p90 103 m, max 741 m ; 156 843 paires laissées de côté (hors région) et 16 129 (une chaîne non appariée) ; **raccords internes non adjacents : 0** sur 231 266 chaînes |
| **Contrôle 2, re-vérification** (12 316 arêtes, haversine, pas de 1 m jusqu'à 200 m de ligne, 200 points au-delà ; ne départage pas deux parallèles à moins de 3 m : voir le test des parallèles du relevé) | Hausdorff p50 0,24 m, p90 0,50, **p99 0,64**, max 2,95 ; au-delà de 1 m : 0,4 % ; au-delà de 3 m : 0 ; Fréchet p99 0,64 ; longueur chaîne / `g` p1 0,96, p50 1,00, p99 1,045 ; **alerte (p99 > 1 m) : non** |
| Champs `fo` / `to` | `to − fo` s'écarte de `trv` : 0 sur 2 171 054 arêtes ; départ attendu à un nœud (`fo` ≤ 1,5 m) mais premier morceau ailleurs : 4 458 sur 2 154 812 (0,2 %) ; arrivée : 4 318 sur 2 152 491 |

Lecture : la méthode tient ce que le prototype Python promettait (99,2 % ; Hausdorff p99 0,6 à 0,9 m), sur **toutes** les équipes
et non sur un tirage, en moins de dix secondes. Le contrôle indépendant ne trouve pas de dérive (p99 0,64 m contre 0,87 m côté
appariement ; pas de cause isolée, les deux méthodes échantillonnent à 1 m et à 0,25 m et ne portent pas sur les mêmes arêtes). Ce qui reste ouvert : les 692 km de `direction_closed`
(dont 540 km longés par une voie parallèle ouverte dans le sens roulé : chaussée d'en face prise en amont, voir le relevé),
les 439 arêtes d'accès `no` et les 1,75 % de `cycleway` non appariés.
