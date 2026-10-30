# Technique 06 — Assembleur (summary, quality, statut)

> Statut : conception technique, extraite de [T01](01-architecture.md) (ex-§5.4). Elle applique la
> conception générale ([01](../01-conception-generale.md)) et le contrat
> ([02](../02-contrat-interface.md)). Composant : **Assembleur** (doc 01 §4), module
> `ord-core::assemble`, étape 9 du pipeline (T01 §5.1). Les noms de types sont ceux de la carte
> des types partagés (T01 §0).

## 1. Rôle

Fonction pure, dernière étape bloquante (même tâche `spawn_blocking` que les étapes 6 à 8, T01
§5.1). Elle reçoit la décision de `decide_outcome` (T01 §5.3), qui ne fixe que le **rejet** ; le
`status` d'une réponse 200 est fixé ici (§3).

Toutes les longueurs sont des sommes de plages en distance, chaque station valant la moitié de ses
deux intervalles, comme `TerrainRange`. Les sommes sont faites dans l'ordre des distances.
`L = PreparedRoute::length_m`, qui est > 0 (≥ 2 points distincts).

## 2. Calcul de chaque champ

| Champ du contrat | Calcul |
|---|---|
| `segments[]` | `TypedSegment` (T03 §8) + `ele_start`/`ele_end` = `profile.at(from_m/to_m)`, `grade_pct` = `profile.grade_pct`. `ele_method` = méthode qui couvre la plus grande longueur du segment ; à égalité, `dsm_deck` > `interpolated_structure` > `gap_filled` > `dem` > `none`. `structure_source` absent sur `ground` et `unknown`. Un ouvrage `heuristic` sur une portion non appariée garde `matched = false` (T03 §8, doc 02 §4.3). `osm` = `debug_attrs` |
| `summary.length_m`, `gain_m`, `loss_m`, `gain_params` | `L` ; `profile.gain` (T04 §5.4) |
| `ele_min_m`, `ele_max_m` | Extrema de `ele` corrigé non lissé sur les stations non `none` ; absents si aucune |
| `by_structure_m`, `by_surface_class_m`, `by_surface_source_m`, `by_way_kind_m` | Somme des longueurs de segments par valeur (`BTreeMap`, valeurs nulles omises) |
| `structures[]` | Un élément par `StructureOutcome` (T04 §5.5), trié par `from_m`. `fuse` en produit un par **suite d'éléments de même nature** d'un ouvrage : deux ponts contigus font une entrée, un pont enchaîné à un tunnel en fait deux, **contiguës** (`to_m` de l'une = `from_m` de la suivante), chacune avec sa méthode (doc 02 §4.4). Champs : `kind`, `from_m`/`to_m` coupés au parcours, `length_m = to_m − from_m`, `structure_source`, `method` ← `OutcomeMethod` (`DsmDeck` → `dsm_deck`, `InterpolatedStructure` → `interpolated_structure`, `KeptGround` → `kept_ground`, `Uncorrected` → `none`). Avec `structure_correction: false`, tous en `none` |
| `terrain_m` ; `unknown_m` | Somme des `TerrainRange` par `origin` ; somme des segments `matched = false`, ouvrages heuristiques non appariés compris |
| `quality.matched_ratio` | `(L − unknown_m) / L` |
| `inferred_surface_ratio` | Longueur `surface.source ∈ {inferred_tracktype, inferred_highway}` / `L` |
| `elevation_coverage` | Longueur des stations dont `ele ≠ none` / `L` |
| `structure_correction` | `profile.correction` (T04 §5.2) : statut par nature, compteurs `counts` par méthode (`dsm_deck`, `interpolated_structure`, `kept_ground`, `uncorrected`) et par `structure_source` ; un compteur nul est omis |
| `terrain` | `profile.terrain_ranges` |
| `warnings[]` | Union des producteurs du §4. Plages contiguës d'un même code fusionnées. Tri par `(from_m, code)`. `message` = gabarit anglais fixe par code, sans coordonnées |
| `status` | §3 |
| arrondis | T01 §9 : altitudes et distances au cm, ratios et pentes à 1e-4 et 0,01 %, coordonnées à 1e-6° |

## 3. Statut

`partial` si un avertissement `elevation_missing`, `typology_missing`, `typology_unavailable` ou
`elevation_unavailable` est présent, sinon `complete` (doc 02 §4.1). Une portion non appariée
(`unmatched_range`), même porteuse d'un ouvrage heuristique, ne rend pas la réponse partielle.

## 4. Producteur de chaque avertissement

Un seul producteur par code :

| Code | Producteur | Plage |
|---|---|---|
| `unmatched_range` | assemble | segments `matched = false` hors plages `typology_*` |
| `typology_missing` | assemble | `TypologyCoverage::missing` ∪ `CoverageRanges::typology_absent` |
| `typology_unavailable` | assemble | `TypologyCoverage::unavailable`, si `Proceed { typology_failed }` |
| `outside_primary_zone` | assemble | `CoverageRanges::outside_primary_zone` (zone France : T02 §7.2) |
| `elevation_missing`, `elevation_unavailable` | `fuse` (T04 §5.1) | stations `none` restées après comblement, selon `Missing::{NoData, Failed}` |
| `elevation_gap_filled` | `fuse` (T04 §5.1) | trous comblés |
| `terrain_fallback` | `fuse` | `TerrainRange` avec `fallback_after_error` |
| `dsm_unavailable`, `structure_uncorrected`, `structure_slope_implausible` | `fuse` (T04 §3) | plage de l'ouvrage |
| `dsm_source` | `fuse` (T04 §5.4) | plages `adapted[]` |
| `structure_untagged` (V1) | `fuse`, verdict `heuristic` de `reconcile` | plage du verdict |

## 5. Interfaces

```rust
// ord-core::assemble
pub struct RouteReport { pub status: Status, pub points: Vec<OutputPoint>, pub segments: Vec<ReportSegment>,
    pub summary: Summary, pub quality: Quality, pub warnings: Vec<Warning>, pub versions: Versions,
    pub attribution: Vec<Attribution> }   // ReportSegment = TypedSegment + ele_method, ele_start/end, grade_pct
pub enum Status { Complete, Partial }
pub struct Summary { pub length_m: f64, pub gain: GainResult, pub ele_min_m: Option<f64>, pub ele_max_m: Option<f64>,
    pub by_structure_m: BTreeMap<StructureClass, f64>, pub by_surface_class_m: BTreeMap<SurfaceClass, f64>,
    pub by_surface_source_m: BTreeMap<SurfaceSource, f64>, pub by_way_kind_m: BTreeMap<WayKind, f64>,
    pub structures: Vec<StructureOutcome>, pub terrain_m: BTreeMap<TerrainOrigin, f64>, pub unknown_m: f64 }
pub struct Quality { pub matched_ratio: f64, pub inferred_surface_ratio: f64, pub elevation_coverage: f64,
    pub structure_correction: StructureCorrectionReport, pub terrain: Vec<TerrainRange> } // warnings : RouteReport
pub fn assemble(route: &PreparedRoute, typo: Option<&TypologyOutput>, segs: Vec<TypedSegment>,
    profile: ElevationProfile, points: Vec<OutputPoint>, cov: &CoverageRanges, outcome: &Outcome,
    versions: Versions, attribution: Vec<Attribution>) -> RouteReport;
```

Consommés : `PreparedRoute`, `CoverageRanges`, `Outcome`, `Warning` (T01 §5) ; `TypologyOutput`,
`TypedSegment` (T03) ; `ElevationProfile`, `StructureOutcome`, `StructureCorrectionReport`,
`OutputPoint` (T04) ; `Versions`, `Attribution` (T01 §9, T02 §9). Le DTO v1 est produit par
`ord-server::to_dto` (T01 §4.2), qui filtre les `sections`.

Tests : T05 §2.1 (assembleur : contiguïté des segments, `status`, producteur unique de chaque code,
ratios dans `[0, 1]`).
