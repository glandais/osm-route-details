# 03 — Conception technique : typologie

> Statut : conception technique. Rien n'est codé. Exigences : [doc 01](../01-conception-generale.md)
> (§3, §4, §5.2–5.5, §6, §10) et [doc 02](../02-contrat-interface.md) (§4.3, §4.4, §5.3, §6).
> Section couverte : **Adaptateur typologie**, **Chaîne typologie** et **Classification** (doc 01 §4),
> plus la construction des segments homogènes de sortie. Les références `os:fichier:ligne` pointent
> vers `/Volumes/Data/code/perso/osm-surface` (commit `44e5da6`), `vc:` vers
> `/Volumes/Data/code/perso/vcyclist` (6.11.0). Les paramètres marqués **[os]** sont repris
> d'osm-surface (mesurés sur ses 3 parcours), **[vc]** de vcyclist, **[cal]** sont nouveaux et à
> calibrer sur le corpus d'ouvrages (doc 01 §8.5).

## 1. Rôle et périmètre

À partir du parcours préparé par l'orchestrateur, la typologie produit, **toujours en distance le
long du parcours** : (1) des **tronçons typés** (`TypedRun`) couvrant tout le parcours ; (2) les
**ouvrages** (`Structure`), dédoublonnés, avec leur géométrie OSM et leurs **extrémités sur
l'ouvrage**, et leurs **passages** (`StructurePassage`) ; (3) la **couverture** (plages sans tuile) ;
(4) après la fusion, les **segments homogènes** du contrat, réconciliés avec les heuristiques. Flux :
tuiles du couloir → adaptateur → modèle neutre → graphe par niveaux → HMM → tronçons + ouvrages →
[fusion, heuristiques] → `reconcile` → `build_segments`. Elle tourne toujours, même avec
`sections: ["elevation"]` ou `structure_correction: false` (doc 01 §5.5 inv. 5). Elle ne lit pas
l'altitude et ne décide pas de la correction (fusion).

## 2. Découpage en modules

| Module | Composant doc 01 | Contenu |
|---|---|---|
| `ord-domain::street` | Adaptateur typologie | Modèle d'attributs neutre « façon OSM », `StreetTile`, trait `StreetSchemaAdapter`. |
| `ord-adapters::streets::{mvt, shortbread}` | Adaptateur typologie | Décodage MVT générique et découpe canonique, Shortbread 1.0, `StreetsCodec<A>` (T02 §8). V1.x : `openmaptiles` ; V2 : `route_details`. |
| `ord-core::typology::corridor` | Chaîne typologie | Tuiles (`corridor_tiles`) et cellules du couloir. |
| `ord-core::typology::graph::{build, repair}` | Chaîne typologie | Graphe CSR par niveaux, recollage aux bords, jonctions, R-tree. |
| `ord-core::typology::matcher::{params, hmm, dijkstra}` | Chaîne typologie | Map-matching HMM/Viterbi porté d'osm-surface. |
| `ord-core::typology::runs` | Chaîne typologie | Projection du chemin apparié en distance le long du parcours. |
| `ord-core::typology::structures` | Chaîne typologie | Graphe des ouvrages, extension aux extrémités (`Step::Need`), dédoublonnage, doutes. |
| `ord-core::classification::{rules, tables}` | Classification | Règles versionnées attributs neutres → enums, réconciliation OSM/heuristique. |
| `ord-core::typology::segments` | Chaîne typologie → Assembleur | Segments homogènes du contrat. |
| `ord-pipeline::typology` | Chaîne typologie | Façade async `TypologyChain::run` : boucle lecture ⇄ étape pure (`Step<T>`, T01 §1), budget, erreurs. |

Seuls `ord-adapters::streets` connaît un schéma de tuiles, seul `classification` les enums publiques.

## 3. Modèle neutre et adaptateurs de schéma

### 3.1 Modèle neutre

Les attributs sont ceux qu'OSM porterait, avec un niveau de fidélité explicite (`SurfaceTag`). Une
source qui ne fournit pas un attribut le laisse à `None` : ce n'est jamais « valeur par défaut ».

```rust
pub type Sym = Arc<str>;                     // chaîne internée par tuile décodée

pub struct WayAttrs {
    pub highway: Sym,                        // highway brut (Shortbread `kind`)
    pub surface: SurfaceTag, pub tracktype: Option<TrackType>,   // None : absent ou non fourni
    pub bicycle: Option<Sym>, pub service: Option<Sym>, pub link: bool, pub rail: bool,
    pub structure: StructureTag,
    pub layer: Option<i8>, pub smoothness: Option<Sym>, pub osm_id: Option<u64>,   // V1.x / V2
    pub raw: Option<Box<[(Sym, Sym)]>>,      // seulement si debug (doc 02 §3.2)
}
pub enum SurfaceTag { Absent, Grouped(PavedGroup), Raw(Sym) }   // Grouped : Shortbread ; Raw : V2
pub enum PavedGroup { Paved, Unpaved }
pub enum StructureTag { None, Bridge, Tunnel, Covered, BuildingPassage, Ford }
pub enum TrackType { Grade1, Grade2, Grade3, Grade4, Grade5 }

pub struct StreetLine { pub attr: u32, pub points: Vec<[f64; 2]>, pub cut: [bool; 2] }
pub struct PointFeature { pub kind: PointKind, pub p: [f64; 2], pub osm_id: Option<u64> } // V2 : gués-nœuds
// ord-domain::street
pub struct StreetTile { pub key: TileKey, pub attrs: Vec<WayAttrs>, pub lines: Vec<StreetLine>,
                        pub points: Vec<PointFeature> }
```

Coordonnées en **unités globales** z14 × extent 4096, comme osm-surface (`os:src/geo.rs` `Grid`) :
toutes les tuiles d'un zoom partagent la même grille, ce qui rend le recollage exact. `cut[i]` dit si
l'extrémité i est un point de découpe au bord de la tuile (nouveau, voir §4.2) et non une vraie fin de
ligne.

### 3.2 Trait d'adaptateur

```rust
pub trait StreetSchemaAdapter: Send + Sync {
    fn id(&self) -> &'static str;                       // "versatiles-shortbread-1.0"
    fn zoom(&self) -> u8;                                // 14
    fn capabilities(&self) -> SchemaCapabilities;
    fn decode(&self, key: TileKey, bytes: &[u8]) -> Result<StreetTile, DecodeError>;
}
pub struct SchemaCapabilities { pub surface: SurfaceFidelity,   // Grouped (VersaTiles, OFM) | Raw (V2)
    pub tracktype: bool, pub layer: bool, pub ford_ways: bool, pub ford_nodes: bool,
    pub covered_distinct: bool, pub osm_id: bool, pub smoothness: bool, pub lines_merged: bool }
```

`capabilities()` alimente `/v1/capabilities` (gué détectable, `surface.detail` disponible) et
conditionne la classification (§7). Le décodage MVT (protobuf, zigzag, `ClosePath`, gzip accepté,
corps vide = tuile vide) est générique et repris d'`os:src/mvt.rs:182-286` ; la découpe au carré de
la tuile, en **ordre canonique** des extrémités pour que deux tuiles voisines calculent le même point,
est reprise d'`os:src/mvt.rs:291-378`.

**Shortbread 1.0** (`shortbread.rs`) : couche `streets`, clés `kind`, `surface`, `tracktype`, `service`,
`bicycle`, `link`, `rail`, `bridge`, `tunnel` (`os:src/mvt.rs:102-115, 215-241`) ; `surface` →
`Grouped` (VersaTiles ne sert que `paved`/`unpaved`, doc exploration 02 §4) ; `bridge=true` →
`Bridge` (y compris les gués bétonnés `low_water_crossing`, doc 01 §5.3) ; `tunnel=true` → `Tunnel`
(y compris `covered` et `building_passage`, indiscernables). `oneway`, `horse` ne sont plus lus.

**V2, « route-details »** (Planetiler) : `surface` → `Raw`, `ford` ways → `Ford`, nœuds `ford=yes`
→ `PointFeature`, `covered` distinct, `layer`, `osm_id`, sans fusion de lignes : un adaptateur et une
table de plus, sans toucher au graphe ni au HMM.

**Crates** : `prost` (protobuf MVT, choix d'osm-surface sans crate MVT), `flate2` (tuiles gzip),
`thiserror`. Le décodage tourne dans le chargeur du cache mémoire de tuiles **décodées** (doc 01 §4
Caches) : une tuile est décodée une fois par processus, partagée en `Arc<StreetTile>`.

## 4. Construction du graphe local

### 4.1 Couloir et chargement

- Tuiles à moins de `search_radius_m + 60` = **100 m** de la trace **[os]** (`os:src/trace.rs:387`,
  `os:src/geo.rs:93-116`), ≈ 40 pour 90 km, triées ; seules les arêtes touchant une cellule de 128
  unités du couloir entrent dans le graphe d'appariement **[os]** (`os:src/network.rs:54-105`).
- Chargement par le port d'accès (`TileStore<StreetTile>::get_many`, concurrence et cadence gérées
  par lui, doc 01 §4). Chaque tuile revient en `TileOutcome` (T02 §2.1) : `Data`, `Empty` (pas de
  rue), `Absent` (hors emprise de la source / extrait → plage *missing*) ou `Failed` (panne → plage
  *unavailable*). Une tuile en échec n'est **jamais** lue comme vide (doc 01 §4).

### 4.2 Nœuds, niveaux, recollage

```rust
pub enum LevelKind { Ground, Bridge, Tunnel }
pub struct Level { pub kind: LevelKind, pub layer: i8 }   // layer = 0 tant que la source n'en a pas
```

`Level` est dérivé par la classification (§7, table `level_of`) : `Bridge` → Bridge ; `Tunnel`,
`Covered`, `BuildingPassage` → Tunnel ; `Ford`, `None` → Ground. En V1.x/V2, `layer` entre dans la
clé : deux ponts superposés deviennent deux niveaux.

Étapes (reprises d'`os:src/network.rs:109-160, 249-387`, avec trois écarts signalés ▲) :

1. **Classes** : un `WayAttrs` routable pour le mode vélo devient une classe (dédoublonnée par
   hachage). Routable et accès : `is_routable_kind` et `derive` d'osm-surface avec règles d'accès
   France (`os:src/valhalla.rs:255-279, 306-330`) : motorway, trunk (France), pedestrian, bridleway,
   busway exclus pour le vélo, footway ouvert ; `bicycle=*` prime ; `rail` exclu.
2. **Nœuds** par coordonnée quantifiée au 1/16 d'unité (`os:src/network.rs:138`). ▲ La clé inclut le
   `Level` : `(x16, y16, level)`. Dans osm-surface un sommet d'un pont qui tombe exactement sur un
   sommet de la route du dessous (même entier de tuile, ≈ 0,4 m) les relie ; ici c'est impossible.
3. **Recollage aux bords de tuile** : une extrémité `cut` de degré 1 fusionne (union-find) avec le
   nœud le plus proche à ≤ `MERGE_UNITS` = **2 unités** (≈ 0,8 m) **[os]** (`os:src/network.rs:18,
   259-322`), en gardant comme représentant le nœud de plus haut degré. ▲ La fusion est restreinte au
   **même niveau** (osm-surface ne filtrait pas, réserve de l'exploration 02 §5).
4. **Jonctions inter-niveaux** ▲ (nouveau) : une **vraie fin de ligne** (non `cut`) d'une ligne
   pont/tunnel se relie à un nœud d'un autre niveau situé à ≤ `MERGE_UNITS`, ou s'insère dans un
   segment d'un autre niveau à ≤ `SPLICE_UNITS`. C'est la culée : dans OSM, le way pont partage son
   nœud d'extrémité avec la voie au sol. Un sommet **intérieur** ne se relie jamais à un autre niveau :
   c'est ce qui déconnecte le pont de la route qu'il enjambe.
5. **Jonctions perdues à la simplification** : un sommet à ≤ `SPLICE_UNITS` = **1,5 unité** (≈ 0,6 m)
   de l'intérieur d'un segment **du même niveau** y est inséré **[os]** (`os:src/network.rs:20,
   324-387`). ▲ Sur les niveaux pont et tunnel, seules les fins de ligne sont insérées (deux tabliers
   qui se croisent dans un échangeur sans `layer` ne se relient pas par un sommet de passage).
6. Longueurs haversine, adjacence **CSR**, R-tree bulk-load (`os:src/network.rs:163-212`). Chaque
   segment porte `class`, `level` et `line: LineId` ; les morceaux d'une même ligne recollés
   à une extrémité `cut` reçoivent le même `LineId` (union-find sur les lignes). `LineId` sert à
   repérer les **éléments** d'ouvrage (§6.3).

```rust
pub struct GraphSeg { pub a: NodeId, pub b: NodeId, pub len_m: f32, pub class: ClassId,
                      pub level: Level, pub line: LineId }
pub struct StreetGraph {
    pub grid: Grid, pub nodes: Vec<[f64; 2]>, pub segs: Vec<GraphSeg>,
    pub classes: Vec<EdgeClass>,             // WayAttrs + Classified (§7), calculé une fois par classe
    adj_start: Vec<u32>, adj: Vec<(SegId, NodeId)>,
    pub index: RTree<GeomWithData<Line<[f64; 2]>, SegId>>,
}
```

**Déterminisme** : `rustc-hash` (graine fixe), tuiles et lignes en ordre trié, égalités départagées
par identifiant, `f64::total_cmp` ; aucune itération de table ne détermine un résultat. **Crates** :
`rstar` (R-tree, d'osm-surface), `rustc-hash`, `smallvec`. Pas de `petgraph` (CSR et Dijkstra à
tableaux réutilisés d'osm-surface, éprouvés) ni de crate `geo` (module partagé `geo`, `os:src/geo.rs`).

## 5. Map-matching HMM (tiles-only)

Porté d'`os:src/matcher.rs` sans la couche Valhalla (Newson & Krumm, à la meili).

### 5.1 Observations

Le parcours de l'orchestrateur est déjà densifié à pas métrique avec sa distance `d_m`. Le HMM y
prélève ses observations : chaque point d'entrée, plus un point tous les `step_m` = **10 m** **[os]**
(`os:src/matcher.rs:144-170`) le long de la droite entre deux points d'entrée. Chaque observation
garde `d_m` et l'indice d'entrée précédent : les résultats sont directement en distance de parcours.

### 5.2 Paramètres

| Paramètre | Défaut | Origine | Rôle |
|---|---|---|---|
| `search_radius_m` | 40 m | [os] `matcher.rs:39-52` | Rayon des candidats |
| `max_candidates` | 8 | [os] | Dédoublonnés par position au ¼ d'unité (`matcher.rs:347-355`) |
| `sigma_z` | 5 m | [os] | Émission `0,5·(d/σ)²` |
| `beta` | 3 m | [os] | Transition `|d_réseau − d_obs| / β` |
| `jump_cost` | 12 | [os] | Saut en ligne droite si le réseau ne relie pas deux candidats |
| `max_jump_m` | 60 m | [os] | Longueur maximale d'un saut |
| `footway_cost` | 0,75 | [os] | Malus d'émission footway, steps, pedestrian (trottoir vs chaussée) |
| Borne Dijkstra | `2·gc + 60 m`, relâchement ≤ 1,5× | [os] `matcher.rs:214-250, 396` | Recherche réseau entre deux observations |
| `min_chain_m` | 50 m | [cal] | Chaîne plus courte rejetée en non appariée (croisements parasites d'une trace trop espacée) |
| `closed_way_cost` | 12 | [cal] | Passage entre le graphe vélo et l'état « hors graphe » d'une voie fermée au vélo (étape 6 ci-dessous), dans un sens ou dans l'autre |
| `heading_window_m`, `max_structure_heading_deg` | 20 m, 60° | [cal] | Ouvrage apparié à contre-cap rejeté (étape 7) |

Surchargeables par configuration (comme `MATCH_*`) ; valeurs effectives dans `rules_version` et
`/v1/capabilities`.

### 5.3 Algorithme

1. **Candidats** : segments du R-tree à ≤ `search_radius_m`, projection orthogonale, tri par distance,
   dédoublonnage par position, troncature à 8 (`os:src/matcher.rs:329-357`). Une observation dont le
   disque de recherche touche une tuile `Absent`/`Failed` n'a **aucun candidat** et est marquée
   `NoData` (et non `Unmatched`) : on n'apparie pas sur des données à moitié chargées.
2. **Viterbi en coûts** : pour chaque candidat de t−1 de coût fini, un Dijkstra borné depuis les deux
   bouts de son segment ; transition `|route − gc| / β` si la route existe dans la borne, sinon saut
   `jump_cost + |droite − gc| / β` si la droite ≤ `max_jump_m`, sinon interdit. Émission plus malus
   footway. Normalisation par le minimum à chaque pas (`os:src/matcher.rs:360-440`).
3. **Chaînes** : une observation sans candidat, ou dont aucun candidat n'est atteignable, ouvre une
   nouvelle chaîne ; backtrack par chaîne depuis sa fin (`os:src/matcher.rs:442-466`).
4. **Pas** : entre deux observations consécutives d'une même chaîne, `Route(pieces)` (morceaux de
   segments parcourus de t0 à t1) ou `Jump` (`os:src/matcher.rs:469-492`).
5. ▲ Les chaînes de moins de `min_chain_m` sont rejetées (osm-surface prolongeait un point isolé sur
   son segment, `os:src/trace.rs:541-550` ; c'est ce qui fabrique des appariements parasites quand
   des points d'entrée espacés coupent des rues en ligne droite).
6. ▲ **État hors graphe des routes fermées au vélo** (2026.10.6). Les routes motorisées
   (`way_kind` `major_road` ou `minor_road`) que la règle d'accès exclut du graphe (étape 1 du
   §4.2 : motorway, trunk, route `bicycle=no`…) restent connues des tuiles. Les rails n'en sont pas,
   ni les voies non motorisées fermées au vélo (rue piétonne, sentier `bicycle=no`) : le cycliste
   les emprunte réellement, et la voie du graphe voisine reste la meilleure approximation (sur le
   banc, les inclure dégradait M1 au-delà de son seuil sur `os-np655`). Leurs segments du couloir forment un index à part, sans topologie.
   Chaque observation a, en plus de ses candidats, un état « hors graphe » : sa projection sur la
   voie fermée la plus proche, à ≤ `search_radius_m`, avec la même émission. Transitions : de cet
   état à lui-même `|droite − gc| / β` (la voie est suivie, sans routage) ; entre lui et un candidat
   du graphe, `closed_way_cost + |droite − gc| / β` si la droite ≤ `max_jump_m`. Une observation dans
   cet état est non appariée et coupe la chaîne. ▲ **Typologie de la voie suivie** (règles 2026.11.1,
   api 1.1 ; décision de l'utilisateur du 2026-11-01) : l'index des voies fermées garde leur classe
   (`EdgeClass`, ajoutée à `classes` sans entrer au graphe) ; l'observation retenue dans cet état
   devient `ObsState::OffGraph { class }` (voie fermée la plus proche), et l'intervalle entre deux
   observations hors graphe un tronçon `RunMatch::OffGraph { class }` (moitié à chaque classe si elle
   change ; un intervalle entre le graphe et l'état hors graphe reste `Unmatched`). `build_segments`
   (§8) en rend `structure`, revêtement et `way_kind`, avec `matched = false` : la portion n'est
   toujours pas appariée au graphe vélo (`unmatched_range`, `unknown_m`), et l'assembleur y ajoute
   `bicycle_forbidden` (T06 §4). Un pont ou un tunnel de la voie fermée n'est **pas** un passage
   d'ouvrage (§6) : typé dans `segments[]`, jamais corrigé, absent de `summary.structures[]`. Motif : sans cet état, le Viterbi normalisé n'a pas
   d'alternative à « la rue la plus proche » ; un parcours sur un pont, un tunnel ou une autoroute
   fermés au vélo accrochait les rues à 20–40 m (dessous, dessus ou à côté) en les enchaînant par
   des sauts, et fabriquait des ouvrages (passages supérieurs parallèles d'une autoroute en
   tranchée). Le coût du passage (deux par excursion) garde sur la piste une trace bruitée qui longe
   une autoroute.
7. Ouvrage apparié à contre-cap (2026.10.5) : une observation appariée sur un pont ou un tunnel dont
   le cap s'écarte de plus de `max_structure_heading_deg` de celui du parcours (sur
   ± `heading_window_m`) est non appariée, ainsi que les morceaux restants trop courts.

**Pas de pénalité de demi-tour** (`route_to`, `os:src/matcher.rs:262-282`) : aller-retour, boucle,
demi-tour sur un pont sont des parcours ordinaires.

**Pont au-dessus, passage sous puis sur** : sol et pont ne se relient qu'aux culées ; passer de la
route du dessous au tablier coûte un saut (12 + écart/β), la continuité garde le niveau. Le passage
dessous est apparié au sol, le passage dessus au pont après la rampe ; seul le second crée un passage
d'ouvrage. Cas de test obligatoire du corpus (jamais évalué par osm-surface).

### 5.4 Sortie

```rust
pub enum ObsState { Matched { cand: Cand, chain: u32 }, Unmatched, OffGraph { class: ClassId }, NoData }
pub struct MatchedPath { pub obs: Vec<Obs>, pub state: Vec<ObsState>, pub steps: Vec<Option<Step>> }
```

**Complexité** : O(n · K · D) avec n = L/10 m (≤ 20 000 pour 200 km), K ≤ 8, D = taille d'un Dijkstra
borné (quelques dizaines de nœuds à 10 m). Mesuré par osm-surface : 15 000 points sur 180 km en
moins de 160 ms tuiles en cache, ≈ 0,9 s à froid pour 40 tuiles (exploration 02 §7).

## 6. Tronçons typés, ouvrages, passages

### 6.1 Projection en distance (`runs`)

Chaque intervalle d'observations [d_t, d_t+1] reçoit les classes de ses `pieces`, réparties au prorata
des longueurs réseau. Un **saut** est coupé en deux : première moitié à la rue quittée, seconde à la rue
atteinte (`os:src/trace.rs:521-539`). Les changements de classe aux nœuds sont **recalés** en
projetant le nœud du graphe sur la polyligne du parcours dans la fenêtre [d_t − step, d_t+1 + step]
(à ≤ `search_radius_m`), sinon au prorata. Les intervalles hors chaîne donnent `Unmatched` ou
`NoData`, ou `OffGraph` entre deux observations hors graphe (§5.3, étape 6). Les tronçons adjacents
de même clé sont fusionnés. Pour les doutes d'un passage (§6.3), `OffGraph` compte comme `Unmatched`.

```rust
pub enum RunMatch { Matched { class: ClassId, element: Option<ElementId> }, Unmatched,
                   OffGraph { class: ClassId }, NoData }
pub struct TypedRun { pub from_m: f64, pub to_m: f64, pub matched: RunMatch,
                      pub passage: Option<PassageId> }
```

### 6.2 Graphe des ouvrages et extrémités

Le graphe d'appariement ne contient que le couloir : un pont y est coupé au bord des cellules. Les
extrémités se lisent donc dans un **`StructureGraph`** séparé, petit (les ouvrages sont rares) :

- toutes les lignes de niveau pont ou tunnel des tuiles chargées, **sans filtre de couloir**, avec la
  même clé de nœud que le graphe d'appariement (lien exact par coordonnée) ;
- l'ensemble des sommets et un R-tree des segments **au sol** à moins de 2 unités d'une fin de ligne
  d'ouvrage, pour reconnaître une **culée** (fin de ligne reliée au sol selon la règle du §4.2 étape 4).

Pour chaque suite de segments d'ouvrage contiguë sur le chemin apparié (bout à bout, sans
interruption de chaîne) :

1. ensemble couvert S = segments d'ouvrage parcourus ; chemin simple P qu'ils forment dans
   `StructureGraph` ;
2. chaque bout de P qui n'est pas une culée est **prolongé** dans `StructureGraph` : à chaque nœud, la
   continuation d'ouvrage de plus faible changement de cap (pont ↔ tunnel permis : inv. 3, ouvrages
   contigus = un seul ouvrage) jusqu'à une culée ou une impasse ;
3. impasse sur un bord de tuile non chargée → l'étape rend `Step::Need(TilePlan)` avec les tuiles
   voisines ; `TypologyChain` les lit (tour supplémentaire, asynchrone) puis relance l'étape, le `StructureGraph` est complété, on reprend. Bornes :
   `max_structure_extent_m` = **20 km** [cal] (Fréjus routier 12,9 km), `max_extension_rounds` = 4,
   `max_extension_tiles` = 32 [cal] ;
4. depuis chaque culée, la voie au sol est suivie sur `approach_m` = **100 m** (marge de vcyclist
   autour d'un portail, `vc:engine/.../path/TunnelStep.kt:72`) : la fusion y lit le MNT « juste à
   l'extérieur » et vérifie le terrain mesuré.

Trace qui commence au milieu d'un viaduc, demi-tour au milieu d'un pont (P = A…milieu, prolongé
jusqu'à B), ouvrage qui sort du couloir : les extrémités sont celles **de l'ouvrage** (inv. 2).
**Dédoublonnage** (inv. 8) : deux passages dont les chemins prolongés partagent un segment visent le
même ouvrage ; `StructureId` = plus petit `SegId` de l'ouvrage (déterministe). Aller-retour ou boucle :
un `Structure`, deux passages.

### 6.3 Éléments et chaînage

La polyligne de l'ouvrage, orientée de la culée A à la culée B (A = extrémité de plus petite
coordonnée canonique, pour un sens indépendant de la trace), porte un **chaînage** en mètres. Les
**éléments** sont les morceaux d'un même `LineId` (un way pont ou tunnel, recollé aux bords de tuile)
: deux ponts contigus = deux éléments, un ouvrage.

```rust
pub struct Structure { pub id: StructureId, pub axis: Vec<LatLon>, pub chain: Vec<f64>, // A → B
    pub length_m: f64, pub elements: Vec<StructureElement>, pub ends: [StructureEnd; 2],
    pub osm_ids: Vec<u64> }                              // vide en v1
pub enum StructureKind { Bridge, Tunnel }                // nature d'un élément ou d'une détection
pub struct StructureElement { pub id: ElementId, pub kind: StructureKind, pub chain_from: f64,
                              pub chain_to: f64, pub class: ClassId }
pub struct StructureEnd { pub at: LatLon, pub kind: EndKind,
                          pub approach: Vec<LatLon> /* 100 m au sol ; approach[0] = « beyond » de T04 §3.1 */ }
pub enum EndKind { Abutment, DeadEnd, Unresolved(UnresolvedEnd) }
pub enum UnresolvedEnd { TileAbsent, TileFailed, ExtentLimit }

pub struct StructurePassage { pub id: PassageId, pub structure: StructureId,
    pub from_m: f64, pub to_m: f64,                      // sur le parcours
    pub chainage: Vec<(f64, f64)>,                       // (d_m, chaînage), croissant en d_m
    pub doubts: SmallVec<[PassageDoubt; 2]> }
pub enum PassageDoubt { EndUnresolved, UnmatchedAdjacent, JumpInside, FarFromAxis, ChainBreak }
```

Doutes : extrémité `Unresolved` ; `Unmatched`/`NoData` à moins de `doubt_guard_m` ; saut dans
l'ouvrage ou saut > 20 m [cal] à la culée ; distance moyenne obs–axe > 3·σ = 15 m [cal] ; coupure de
chaîne dans l'ouvrage. Un saut court qui chevauche une borne du passage et dont l'extrémité sur
l'ouvrage est à une culée (chaînage 0 ou `length_m`, à `end_reached_m` près) est un saut **à la
culée**, pas dans l'ouvrage (règles 2026.10.8) : c'est un trou du graphe entre la voie au sol et le
pont (ways sans nœud commun). Compté comme saut dans l'ouvrage, il laissait le MNT brut sur tout le
pont (`nv-02`, banc 2026.10.7 : saut de 3 m à la culée, MNT LiDAR erratique sous un tablier haut
au-dessus de l'eau, 41 m d'écart). Une arrivée au milieu d'un ouvrage reste un doute, même courte
(trace qui longe un ouvrage par-dessous). `chainage` donne, à chaque observation, la position du candidat projetée sur l'axe : la fusion lit le
profil de l'ouvrage **une fois**, en chaînage, puis le rend à chaque passage, dans chaque sens (inv. 8).
`doubt_guard_m` = 20 m [cal]. La typologie **constate** les doutes ; la fusion décide (inv. 4 :
pas de correction en cas de doute, `structure_uncorrected`) et y ajoute les siens (extrémité sans
altitude).

### 6.4 Ponts au-dessus du parcours (`overpasses`, règles 2026.11.6)

Le graphe par niveaux (§4.2) ne relie un pont au sol que par ses culées : un pont qui croise la
route du dessous par un sommet intérieur l'enjambe, et le passage dessous reste apparié au sol
(§5.3). La fusion a besoin de savoir où, pour retirer du MNT la bosse du tablier interpolé (T04
§3.4). `TypologyOutput::overpasses` liste les croisements de la **trace** (polyligne des
observations, qui contient tous les points d'entrée : c'est sur elle que l'altitude est lue) avec
les lignes `bridge` de **toutes** les tuiles lues (couloir et extensions), **sans filtre de classe** :
un pont ferroviaire ou autoroutier, hors du graphe vélo, fait la même bosse qu'un pont routier.

```rust
pub struct Overpass { pub d_m: f64,      // distance de parcours du croisement
    pub angle_deg: f64,                  // angle trace / pont, dans [0°, 90°]
    pub end_m: f64 }                     // distance à l'extrémité réelle (non `cut`) la plus proche
```

La typologie **constate** ; la fusion décide (niveau de la trace au croisement, angle, culée). Un
même croisement lu dans deux tuiles (géométrie tamponnée) est compté une fois (< 1 m). Une extrémité
posée (≤ `MERGE_UNITS`) sur une autre ligne de pont est une jonction du même tablier (way découpé
au-dessus de la voie du dessous), pas une culée : `end_m` ne la lit pas. Limite : une
ligne coupée au bord de tuile n'a pas d'extrémité réelle de ce côté ; une culée juste au-delà du
bord n'est pas vue (`end_m` infini de ce côté).

## 7. Classification (règles versionnées)

Composant séparé et testable seul (doc 01 §4). Entrée : `WayAttrs` + `SchemaCapabilities` ; sortie :

```rust
pub struct Classified { pub level: Level, pub structure: StructureClass,
    pub surface_class: SurfaceClass, pub surface_source: SurfaceSource,
    pub surface_detail: Option<SurfaceDetail>, pub way_kind: WayKind }
pub enum StructureClass { Ground, Bridge, Tunnel, Ford, Unknown }
pub enum SurfaceClass { Paved, Unpaved, Unknown }                    // gelée v1
pub enum SurfaceSource { Tagged, InferredTracktype, InferredHighway, None }
#[non_exhaustive] pub enum SurfaceDetail { Asphalt, PavingStones, Sett, Cobblestone, Concrete,
    Compacted, FineGravel, Gravel, Dirt, Grass, Wood /* … */ }        // V2 seulement
pub enum WayKind { MajorRoad, MinorRoad, Track, Path, Cycleway, Footway, Steps, Unknown }
pub enum StructureSource { Osm, Heuristic, OsmHeuristic }
pub struct RuleSet { pub version: &'static str, /* tables ci-dessous */ }
pub fn classify(a: &WayAttrs, caps: &SchemaCapabilities, rules: &RuleSet) -> Classified;
```

La classification est appelée **une fois par classe** à la construction du graphe (quelques centaines
de classes par requête), jamais par segment. Une portion `Unmatched`/`NoData` donne
`Unknown`/`Unknown`/`None`/`Unknown` sans passer par les règles.

**Table v1** (tables Rust `const` dans `classification::tables`, ordre = priorité, première règle
applicable gagne). Les défauts de revêtement sont ceux de la doc exploration 02 §6, eux-mêmes ceux de
Valhalla repris par osm-surface (`os:src/valhalla.rs:396-458`, `os:README.md:100-114`) ; la classe est
`unpaved` dès que la surface Valhalla est pire que `paved_rough` (`os:src/valhalla.rs:43-45`).

| Sortie | Règle (v1) |
|---|---|
| `structure` | `Bridge` → bridge ; `Tunnel`, `Covered`, `BuildingPassage` → tunnel ; `Ford` → ford **si** `caps.ford_ways`, sinon ground ; `None` → ground |
| surface 1 | `Grouped(Paved)` → paved / tagged ; `Grouped(Unpaved)` → unpaved / tagged ; `Raw(v)` (V2) → table brute → classe / tagged, `detail` rempli ; `Raw` inconnu → règle suivante (▲ Valhalla sautait le tracktype) |
| surface 2 | tracktype `grade1` → paved ; `grade2`–`grade5` → unpaved ; source `inferred_tracktype` |
| surface 3 | highway motorway…residential, `*_link`, living_street, service, road, busway, cycleway, steps → paved ; track, path, footway, pedestrian, bridleway, sidewalk, crossing → unpaved ; source `inferred_highway` |
| surface 4 | sinon unknown / `none` |
| `way_kind` | motorway, trunk, primary, secondary, tertiary (+ link) → major_road ; unclassified, residential, living_street, service, road, busway → minor_road ; track → track ; path, bridleway → path ; cycleway → cycleway ; footway, pedestrian, sidewalk, crossing → footway ; steps → steps ; autre → unknown (exploration 02 §9) |
| `level_of` | §4.2 ; `layer` pris en compte seulement si `caps.layer` |

`surface.detail` reste vide en v1 ; `unpaved_guess` d'osm-surface (classe fine) n'est pas repris.

**Version** : `rules_version` est unique pour le service (doc 02 §6). La table de typologie et les
paramètres HMM (§5.2) en font partie ; un test fige une empreinte (hachage) de ces tables, si bien
qu'une modification sans changement de `rules_version` casse la CI. Tests de table : un cas par ligne,
plus les combinaisons de la calibration d'osm-surface (`os:eval/report.md:160-239`) en non-régression.

### 7.1 Réconciliation OSM / heuristiques (`structure_source`)

Au MVP, tout ouvrage est `osm` : la fusion appelle quand même `reconcile`, avec `detected = []`, et
reçoit un verdict `osm` par passage. Le chemin de code est le même au MVP et en V1. En V1, la
fusion passe les détections (terrain mesuré) à :

```rust
pub struct DetectedStructure { pub id: u32, pub kind: StructureKind, pub from_m: f64, pub to_m: f64 } // T04 §4
pub enum GroupRef { Osm(StructureId), Heuristic(u32 /* DetectedStructure::id */) }
pub struct StructureVerdict { pub target: GroupRef, pub kind: StructureKind, pub from_m: f64, pub to_m: f64,
    pub passage: Option<PassageId>, pub source: StructureSource }
pub fn reconcile(passages: &[StructurePassage], structures: &[Structure],
                 detected: &[DetectedStructure], rules: &RuleSet) -> Vec<StructureVerdict>;
```

Règles (dans `RuleSet`, [cal]) : un passage OSM recouvert à ≥ `confirm_overlap` = 50 % de la plus
courte des deux plages par une détection du même type → `osm_heuristic` ; non recouvert → `osm`, sans
doute (les détecteurs ratent beaucoup). Une détection qui ne recouvre aucun passage OSM → verdict
`heuristic` (corrigé, `structure_untagged`, décision Q17) ; la part d'une détection qui dépasse un
passage OSM de plus de `untagged_min_m` = 40 m [vc, `deckMinRunM`, `vc:G/DeckDetectorOptions.kt`] devient un verdict `heuristic` contigu (viaduc nord
de Saint-Nazaire). Une détection qui recouvre un ouvrage OSM d'un autre type est ignorée (OSM prime)
et comptée en métrique. Mise en œuvre V1 (règles 2026.11.2) : constantes `CONFIRM_OVERLAP` et
`UNTAGGED_MIN_M` de `classification::reconcile` ; une détection qui ne touche aucun passage OSM
donne un verdict `heuristic` entier, une détection qui en touche donne un verdict par part
extérieure de plus de 40 m ; T04 §4.3 bis.

## 8. Segments homogènes

`build_segments(runs, verdicts, route) -> Vec<TypedSegment>`, appelée après la fusion :

1. Superposer les verdicts aux tronçons : `heuristic` impose `structure` (bridge/tunnel) et
   `structure_source` sur sa plage en gardant `surface`, `way_kind` et `matched` du dessous ;
   `osm_heuristic` change seulement la source. **Sur une portion non appariée** (décision doc 01
   §10, autorisé par doc 02 §4.3) : `structure = bridge|tunnel`, `structure_source = heuristic`,
   `matched = false`, `surface.class = unknown`, `surface.source = none`, `way_kind = unknown` ;
   `unknown` reste réservé au non apparié **sans** ouvrage détecté (doc 01 §5.3). La plage compte
   dans `unknown_m` et garde `unmatched_range` (T06 §2, §4).
   **Sur une voie fermée au vélo** (`RunMatch::OffGraph`, §5.3 étape 6, règles 2026.11.1) :
   `structure`, revêtement et `way_kind` de cette voie (`structure_source = osm` sur un pont ou un
   tunnel), `matched = false`, `off_graph = true`.
2. Clé de coupe (doc 02 §4.3) : `structure`, `structure_source`, `surface.class`, `surface.source`,
   `way_kind`, `matched`, `off_graph`, **et** l'élément d'ouvrage (`ElementId`) ; ni `ele_method` ni
   l'origine du terrain ne coupent.
3. Absorption : un tronçon **au sol apparié** de moins de `min_segment_m` = **20 m** (`MIN_SPAN_M`
   de tribly, `os:src/tribly.rs:14`) est absorbé par le voisin le plus long ; jamais un ouvrage, ni un
   changement de `matched` (sinon on fabrique de l'appariement).
4. `from_index` = dernier point d'entrée à `d ≤ from_m`, `to_index` = premier à `d ≥ to_m` (exemple
   doc 02 §4.6). Les segments couvrent [0, L] sans trou.

```rust
pub struct TypedSegment { pub from_m: f64, pub to_m: f64, pub from_index: u32, pub to_index: u32,
    pub structure: StructureClass, pub structure_source: Option<StructureSource>,
    pub surface_class: SurfaceClass, pub surface_source: SurfaceSource,
    pub surface_detail: Option<SurfaceDetail>, pub way_kind: WayKind, pub matched: bool,
    pub off_graph: bool /* voie fermée au vélo suivie, bicycle_forbidden */,
    pub passage: Option<PassageId>, pub debug_attrs: Option<WayAttrs> }
```

`ele_*` et `grade_pct` sont ajoutés par l'assembleur ; la sélection des points de rupture lit
`structure_start/end` et `surface_change` sur `TypedSegment` et `StructurePassage`.

## 9. Cas limites

| Cas | Traitement |
|---|---|
| Aller-retour, boucle | Observations indexées en `d_m` ; même ouvrage, deux passages, même classe aux deux passages. |
| Demi-tour au milieu d'un pont | Un passage, ouvrage prolongé jusqu'à la culée opposée (§6.2). |
| Passage sous puis sur un même pont | Niveaux déconnectés hors culées ; continuité HMM (§5.3). |
| Trace qui commence/finit sur l'ouvrage | Extension vers la culée, tuiles voisines au besoin. |
| Points trop espacés | Observations sur la droite : pas de candidat → `Unmatched` ; chaînes < `min_chain_m` rejetées ; `unmatched_range`. Aucun traitement spécial de l'espacement. |
| Portion hors réseau (bridleway absent, bac, champ) | `Unmatched` → segment `unknown`, `matched: false` ; `status` inchangé. |
| Tuile `Absent` / `Failed` | Observations `NoData` ; plages rendues dans `TypologyCoverage` ; l'orchestrateur en fait `typology_missing` (partial) ou 503 / `typology_unavailable`. |
| Ouvrage dont une culée est hors tuiles | `EndUnresolved` → non corrigé par la fusion. |
| Pont + tunnel contigus | Un ouvrage, deux éléments, deux segments ; dans `summary.structures[]`, une entrée par type, contiguës (T06 §2), sans champ d'éléments. |

## 10. Erreurs, concurrence, budget

```rust
pub struct TypologyRequest<'a> { pub route: &'a PreparedRoute, pub debug: bool, pub ctx: &'a RequestCtx }
pub struct TypologyOutput { pub runs: Vec<TypedRun>, pub structures: Vec<Structure>,
    pub passages: Vec<StructurePassage>, pub coverage: TypologyCoverage, pub classes: Arc<[EdgeClass]>,
    pub overpasses: Vec<Overpass> /* §6.4 */, pub data_version: DataVersion, pub stats: TypologyStats }
pub struct TypologyCoverage { pub missing: Vec<Range<f64>>, pub unavailable: Vec<Range<f64>> }
pub enum TypologyError { Timeout, Cancelled, Internal(String) }
#[async_trait] pub trait TypologyChain: Send + Sync {          // ord-pipeline::typology
    async fn run(&self, req: TypologyRequest<'_>) -> Result<TypologyOutput, TypologyError>; }
// TypologyOutput, TypologyCoverage, TypologyError : ord-core::typology ; From<TypologyError> for PipelineError (T01 §4.3)
```

- Une tuile en échec n'est **pas** une erreur de la chaîne : elle devient une plage de couverture.
  L'orchestrateur applique la politique 503 / `accept_partial` (doc 01 §6). Une tuile qui ne se décode
  pas revient du port en `Failed(Invalid)` (jamais mise en cache comme valide, T02 §5.2) et compte comme une panne.
- Chargement des tuiles : `async`, par le port partagé, sous la date limite de la requête
  (`ctx.deadline`, T01 §3.3) ; tour d'extension des ouvrages compris.
- Calcul (graphe, HMM, ouvrages) : CPU pur, en `spawn_blocking` sous un sémaphore, permis déplacé
  dans la tâche (`os:src/trace.rs:400-426`) ; `&dyn Budget` vérifié toutes les 1 000 observations →
  `Timeout` (ou `Cancelled`). Pas de parallélisme intra-requête en v1 (Viterbi séquentiel ; `rayon` seulement si mesuré).
- Mémoire : quelques centaines de milliers de segments pour 200 km. Logs : durées, compteurs, doutes ;
  jamais de coordonnées.

## 11. Interfaces

**Expose**

| Type / trait | Consommateur |
|---|---|
| `TypologyChain`, `TypologyRequest`, `TypologyOutput`, `TypologyError`, `TypologyCoverage`, `TypologyStats` | Orchestrateur |
| `Structure`, `StructureId`, `StructureKind`, `StructureElement`, `StructureEnd`, `EndKind`, `UnresolvedEnd`, `StructurePassage`, `PassageId`, `PassageDoubt`, `LevelKind` | Fusion du profil, détecteurs (plages à exclure ou confirmer), sélection des points de rupture, assembleur |
| `TypedRun`, `RunMatch`, `ElementId`, `ClassId`, `EdgeClass` | Fusion (portion `unknown` touchant un ouvrage), assembleur |
| `StructureClass`, `SurfaceClass`, `SurfaceSource`, `SurfaceDetail`, `WayKind`, `StructureSource`, `Classified`, `RuleSet`, `classify` | Assembleur, API (sérialisation), `/v1/capabilities` |
| `reconcile`, `DetectedStructure`, `StructureVerdict`, `GroupRef` | Fusion du profil (V1) |
| `build_segments`, `TypedSegment` | Assembleur, sélection des points de rupture |
| `StreetSchemaAdapter`, `SchemaCapabilities`, `SurfaceFidelity`, `WayAttrs`, `StreetTile`, `StreetLine`, `PointFeature` (dans `ord-domain::street`) | `StreetsCodec<A>` (T02 §8), `/v1/capabilities` |
| `MatchParams` (valeurs effectives) | `/v1/capabilities` |

**Consomme**

| Type / trait | Fournisseur |
|---|---|
| `PreparedRoute` / `RouteSample` (`at`, `d_m`, `input_index: Option<u32>`, `prev_input: u32` ; `length_m`) | Orchestrateur (T01 §5) |
| `RequestCtx` (`Deadline`, `DataLog`), `Budget`, `Step<T>`, `TilePlan` | Orchestrateur (T01 §3.3, §5) |
| `TileStore<StreetTile>` (port + LRU décodé dédoublonné), `TileKey`, `SourceId::Streets`, `TileOutcome::{Data, Empty, Absent, Failed}`, `DecodeError` | Accès données, Caches (T02 §2) |
| `DetectedStructure` | Détecteurs heuristiques (V1), via la fusion |
| `LatLon`, `Grid`, `haversine_m`, `project_on_segment` | module partagé `geo` |
| `DataVersion`, `Warning`/`WarningCode` (`unmatched_range`, `typology_missing`) | T01 §0 (identifiant et date de la source typologie ; avertissements) |

## 12. Points de tension

1. **Revêtement inféré des footway/pedestrian** : la règle exigée (exploration 02 §6, doc 01 §5.4)
   les classe `unpaved` sans tag (Valhalla `compacted`), souvent faux en ville. Les 98–99 % d'accord
   avec Stadia ne la valident pas : Stadia applique la même règle. Gardée (`inferred_highway`), à
   remesurer sur les traces personnelles avant la V1.
2. **Voies interdites au vélo hors graphe** (motorway, trunk en France, pedestrian, busway) : un
   parcours planifié qui les emprunte devient `unknown`, et un pont de trunk n'est pas corrigé.
   Depuis 2026.10.6, sur une route motorisée fermée au vélo, l'état hors graphe (§5.3, étape 6) le
   dit explicitement au lieu d'apparier les rues voisines. Depuis 2026.11.1 (api 1.1), la portion
   porte la typologie de cette route (classe, revêtement, pont ou tunnel) avec `bicycle_forbidden`,
   toujours `matched = false` ; son pont ou son tunnel n'est toujours pas corrigé. Une rue piétonne
   ou un sentier `bicycle=no` restent appariés à la voie voisine du graphe, comme avant : la règle
   M1 hors graphe vélo du banc (bench README §4 ter.3) les compte pourtant parmi les voies fermées
   (écart signalé à l'utilisateur, non tranché ici).
3. **Trois écarts à osm-surface jamais mesurés** (clé de nœud par niveau, recollage au même niveau,
   jonction inter-niveaux par les fins de ligne) : nécessaires aux niveaux déconnectés, mais les 3
   parcours d'osm-surface doivent être rejoués en non-régression.
4. **Deux ponts contigus = deux segments** (doc 02 §4.3) : sans `osm_id`, deux ways pont de mêmes
   attributs fusionnés par Shortbread font une seule ligne, donc un seul segment.
5. **Extrémités hors couloir** (inv. 2) : un long tunnel peut coûter jusqu'à 32 tuiles et 4 tours
   réseau de plus à froid, sous le budget ; au-delà, `EndUnresolved` et pas de correction.
6. **Sauts** : un saut (≤ 60 m) compte comme apparié, moitié à chaque rue ; il peut masquer un court
   pont absent des tuiles, sans signal au MVP (seules les heuristiques V1 le rattrapent).
