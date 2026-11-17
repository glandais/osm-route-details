//! Tables de goût de la sonde (note lot 4, §12.2) : parts de longueur roulées contre parts disponibles autour du
//! réseau, par classe de goût (`ord_loops::taste_class`, 72 classes), et le multiplicateur entier qui en sort.
//!
//! Tout le calcul est en `f64` à partir de **sommes entières** (l'ordre des sommes n'a donc aucun effet, quel que soit
//! le découpage des fils) ; seule la table finale, en entiers, entre dans le moteur (`ord_loops::Taste`). Aucune
//! coordonnée ni id n'en sort : des parts par classe.

use std::sync::atomic::{AtomicU64, Ordering};

use ord_graph::format::DIR_BIT;
use ord_graph::mmap::MappedGraph;
use ord_graph::overlay::Overlay;
use ord_loops::{taste_class, RideType, Taste, TASTE_CLASSES};
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use serde::Serialize;

/// Les trois types de la surcouche, avec leur nom dans `OverlayMeta::types` ; l'indice est `RideType::index()`.
pub const TYPES: [(&str, RideType); 3] =
    [("ROAD", RideType::Road), ("GRAVEL", RideType::Gravel), ("MTB", RideType::Mtb)];

/// Paramètres de l'apprentissage (§12.2).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TasteParams {
    /// Lissage `m` : une classe jamais prise tend vers `m / (1 + m)`.
    pub m: f64,
    /// `g` est borné à `[1 / bound, bound]`.
    pub bound: f64,
    /// Seuil de données : km-passages (surcouches), km de traces (recalée) ; sous le seuil, table neutre.
    pub min_km: f64,
    /// Côté de la trame du couloir, m.
    pub cell_m: f64,
    /// Dilatation du couloir, en cases, dans les deux axes.
    pub corridor_cells: u32,
    /// Recalé par équipe (§13.2.4) : une équipe pèse `N / (N + team_n0)` dans sa propre table, `N` étant ses boucles
    /// d'apprentissage du type.
    pub team_n0: f64,
    /// Recalé itéré (§13.2.2) : le multiplicateur **cumulé** d'une classe est borné à `[1 / mult_bound, mult_bound]`.
    pub mult_bound: f64,
}

impl Default for TasteParams {
    fn default() -> Self {
        Self {
            m: 0.5,
            bound: 3.0,
            min_km: 200.0,
            cell_m: 500.0,
            corridor_cells: 2,
            team_n0: 25.0,
            mult_bound: 4.0,
        }
    }
}

// --- Trame ---------------------------------------------------------------------------------------------------------

const M_PER_DEG: f64 = 111_320.0;

/// Trame de cases (lat, lon) en E7 : `LAT` fixe, `LON = LAT / cos 46,5°` (une case de 540 m de large à 42° N, 457 m à
/// 51° N).
#[derive(Clone, Copy, Debug)]
struct Grid {
    lat: i32,
    lon: i32,
}

impl Grid {
    fn new(cell_m: f64) -> Self {
        let lat = (cell_m * 1e7 / M_PER_DEG).round().max(1.0);
        let lon = (lat / 46.5f64.to_radians().cos()).round().max(1.0);
        Self { lat: lat as i32, lon: lon as i32 }
    }

    fn cell(&self, p: (i32, i32)) -> (i32, i32) {
        (p.0.div_euclid(self.lat), p.1.div_euclid(self.lon))
    }

    fn key(c: (i32, i32)) -> u64 {
        (u64::from(c.0 as u32) << 32) | u64::from(c.1 as u32)
    }

    fn key_of(&self, p: (i32, i32)) -> u64 {
        Self::key(self.cell(p))
    }

    /// Les cases que touche la polyligne, densifiée à un point tous les `cell_m / 2` au plus sur chaque segment
    /// (interpolation linéaire en E7, longueur de segment équirectangulaire).
    fn polyline_cells(&self, pts: &[(i32, i32)], cell_m: f64, out: &mut Vec<u64>) {
        if let Some(&p) = pts.first() {
            out.push(self.key_of(p));
        }
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let mid_lat = (f64::from(a.0) + f64::from(b.0)) * 0.5e-7;
            let dy = f64::from(b.0 - a.0) * 1e-7 * M_PER_DEG;
            let dx = f64::from(b.1 - a.1) * 1e-7 * M_PER_DEG * mid_lat.to_radians().cos();
            let n = (dx.hypot(dy) / (cell_m / 2.0)).ceil().max(1.0) as i64;
            for k in 1..=n {
                let f = k as f64 / n as f64;
                let lat = (f64::from(a.0) + f * f64::from(b.0 - a.0)).round() as i32;
                let lon = (f64::from(a.1) + f * f64::from(b.1 - a.1)).round() as i32;
                out.push(self.key_of((lat, lon)));
            }
        }
    }
}

/// Les cases de `raw` dilatées de `c` cases dans les deux axes (carré `(2c + 1)²`).
fn dilate(raw: &FxHashSet<u64>, c: u32) -> FxHashSet<u64> {
    let c = c as i32;
    let mut out: FxHashSet<u64> = FxHashSet::default();
    out.reserve(raw.len() * 4);
    for &k in raw {
        let (x, y) = ((k >> 32) as u32 as i32, k as u32 as i32);
        for dx in -c..=c {
            for dy in -c..=c {
                out.insert(Grid::key((x.wrapping_add(dx), y.wrapping_add(dy))));
            }
        }
    }
    out
}

// --- Membre d'une vue ----------------------------------------------------------------------------------------------

/// Un bit par arête de base et par vue (`road`, `gravel`) : l'arête est dans au moins une chaîne de la vue.
#[derive(Debug)]
pub struct ViewEdges {
    bits: [Vec<u64>; 2],
}

impl ViewEdges {
    /// Parcourt toutes les chaînes de chaque fichier de vue (`view_chain(c)` pour `c` dans `0..header().edges`).
    pub fn build(views: [&MappedGraph; 2], n_base_edges: usize) -> Self {
        let words = n_base_edges.div_ceil(64);
        let one = |v: &MappedGraph| -> Vec<u64> {
            let atoms: Vec<AtomicU64> = (0..words).map(|_| AtomicU64::new(0)).collect();
            (0..v.header().edges as u32).into_par_iter().for_each(|c| {
                for l in v.view_chain(c).expect("chaînes d'une vue valide") {
                    let e = (l.get() & !DIR_BIT) as usize;
                    atoms[e / 64].fetch_or(1 << (e % 64), Ordering::Relaxed);
                }
            });
            atoms.into_iter().map(AtomicU64::into_inner).collect()
        };
        Self { bits: [one(views[0]), one(views[1])] }
    }

    pub fn contains(&self, vi: usize, e: u32) -> bool {
        self.bits[vi].get(e as usize / 64).is_some_and(|w| w >> (e % 64) & 1 == 1)
    }
}

// --- Sommes par classe ---------------------------------------------------------------------------------------------

/// Sommes entières d'une surcouche, par type et par classe de goût (72 cases chacun). `Vec` : `serde` ne sérialise
/// pas les tableaux de plus de 32 éléments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClassSums {
    /// `Σ len_dm × passages` des arêtes roulées du type, dans la vue du type.
    pub ridden: [Vec<u64>; 3],
    /// `Σ len_dm` des arêtes de base de la vue du type dont un bout est dans le couloir du type.
    pub avail: [Vec<u64>; 3],
}

impl Default for ClassSums {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassSums {
    pub fn new() -> Self {
        let z = || vec![0u64; TASTE_CLASSES];
        Self { ridden: [z(), z(), z()], avail: [z(), z(), z()] }
    }

    /// Mise en commun : somme case à case.
    pub fn add(&mut self, o: &Self) {
        for t in 0..3 {
            for c in 0..TASTE_CLASSES {
                self.ridden[t][c] += o.ridden[t][c];
                self.avail[t][c] += o.avail[t][c];
            }
        }
    }
}

const CHUNK: usize = 1 << 16;

/// Les sommes d'une surcouche (§12.2) : un seul passage sur les arêtes de base traite les trois types. Réduction
/// par addition d'entiers : le résultat ne dépend pas du découpage.
pub fn class_sums(gf: &MappedGraph, ve: &ViewEdges, o: &Overlay, p: &TasteParams) -> ClassSums {
    let grid = Grid::new(p.cell_m);
    let idx: [Option<usize>; 3] = TYPES.map(|(name, _)| o.meta.types.iter().position(|t| t == name));
    let vi = TYPES.map(|(_, r)| r.view_index());
    let rows: Vec<(u32, &[u16])> = o.iter().collect();
    // 1. Le roulé, et les cases de la polyligne de chaque arête roulée par un type (dans la vue ou non).
    let (mut sums, raw) = rows
        .par_chunks(CHUNK)
        .map(|chunk| {
            let mut s = ClassSums::new();
            let mut cells: [FxHashSet<u64>; 3] = Default::default();
            let mut buf: Vec<u64> = Vec::new();
            for &(e, c) in chunk {
                let ed = gf.edge(e);
                let class = taste_class(ed.attrs);
                let mut computed = false;
                for t in 0..3 {
                    let Some(i) = idx[t] else { continue };
                    let passages = u64::from(c[2 * i]) + u64::from(c[2 * i + 1]);
                    if passages == 0 {
                        continue;
                    }
                    if ve.contains(vi[t], e) {
                        s.ridden[t][class] += u64::from(ed.len_dm) * passages;
                    }
                    if !computed {
                        let (a, b) = gf.edge_ends(e);
                        let mut pts = vec![gf.node_pos(a)];
                        pts.extend(gf.geometry(e));
                        pts.push(gf.node_pos(b));
                        buf.clear();
                        grid.polyline_cells(&pts, p.cell_m, &mut buf);
                        computed = true;
                    }
                    cells[t].extend(buf.iter().copied());
                }
            }
            (s, cells)
        })
        .reduce(
            || (ClassSums::new(), Default::default()),
            |(mut sa, mut ca): (ClassSums, [FxHashSet<u64>; 3]), (sb, cb)| {
                sa.add(&sb);
                for (x, y) in ca.iter_mut().zip(cb) {
                    if x.len() < y.len() {
                        let small = std::mem::replace(x, y);
                        x.extend(small);
                    } else {
                        x.extend(y);
                    }
                }
                (sa, ca)
            },
        );
    // 2. Les couloirs : les cases dilatées.
    let corridor: [FxHashSet<u64>; 3] = [0, 1, 2].map(|t| dilate(&raw[t], p.corridor_cells));
    drop(raw);
    // 3. Le disponible : toute arête de base d'une vue dont un bout est dans le couloir du type.
    let n_edges = gf.header().edges as usize;
    let avail = (0..n_edges.div_ceil(CHUNK))
        .into_par_iter()
        .map(|k| {
            let mut s = ClassSums::new();
            for e in (k * CHUNK..((k + 1) * CHUNK).min(n_edges)).map(|e| e as u32) {
                let in_view = [ve.contains(0, e), ve.contains(1, e)];
                if !(in_view[0] || in_view[1]) {
                    continue;
                }
                let ed = gf.edge(e);
                let (a, b) = gf.edge_ends(e);
                let (ka, kb) = (grid.key_of(gf.node_pos(a)), grid.key_of(gf.node_pos(b)));
                let class = taste_class(ed.attrs);
                for t in 0..3 {
                    if idx[t].is_some()
                        && in_view[vi[t]]
                        && (corridor[t].contains(&ka) || corridor[t].contains(&kb))
                    {
                        s.avail[t][class] += u64::from(ed.len_dm);
                    }
                }
            }
            s
        })
        .reduce(ClassSums::new, |mut a, b| {
            a.add(&b);
            a
        });
    sums.add(&avail);
    sums
}

// --- Tables --------------------------------------------------------------------------------------------------------

/// Table d'un type depuis deux vecteurs de parts : `(q8, g)` (§12.2).
///
/// - `num_km < min_km`, `Σ den = 0` ou `Σ num = 0` : neutre (256 partout, g = 1) ;
/// - `g(c) = (p_n(c) + m p_d(c)) / ((1 + m) p_d(c))` borné à `[1 / bound, bound]` (`p_d(c) = 0` : g = 1) ;
/// - `q8(c) = round(256 × g(c)^(−γ))` borné à `[1, 65 535]`.
pub fn ratio_table(
    num: &[f64],
    den: &[f64],
    num_km: f64,
    gamma: f64,
    p: &TasteParams,
) -> (Vec<u16>, Vec<f64>) {
    let (sn, sd): (f64, f64) = (num.iter().sum(), den.iter().sum());
    if num_km < p.min_km || sd <= 0.0 || sn <= 0.0 {
        return (vec![256; TASTE_CLASSES], vec![1.0; TASTE_CLASSES]);
    }
    let mut q8 = Vec::with_capacity(TASTE_CLASSES);
    let mut gs = Vec::with_capacity(TASTE_CLASSES);
    for c in 0..TASTE_CLASSES {
        let (pn, pd) = (num[c] / sn, den[c] / sd);
        let g =
            if pd > 0.0 { ((pn + p.m * pd) / ((1.0 + p.m) * pd)).clamp(1.0 / p.bound, p.bound) } else { 1.0 };
        q8.push((256.0 * g.powf(-gamma)).round().clamp(1.0, 65_535.0) as u16);
        gs.push(g);
    }
    (q8, gs)
}

/// Une table apprise, pour les sorties : agrégats seulement (des parts par classe).
#[derive(Clone, Debug, Serialize)]
pub struct TasteTable {
    /// Lettre d'équipe, `général` ou `recalé`.
    pub source: String,
    /// `full` ou `holdout`.
    pub overlay: String,
    /// `ROAD`, `GRAVEL` ou `MTB`.
    pub ride: String,
    /// Kilomètres-passages (surcouches) ou kilomètres de traces (recalée) du numérateur.
    pub num_km: f64,
    /// Table neutre : trop peu de données, ou rien à apprendre.
    pub neutral: bool,
    pub pn: Vec<f64>,
    pub pd: Vec<f64>,
    pub g: Vec<f64>,
    pub q8: Vec<u16>,
}

fn shares(v: &[f64]) -> Vec<f64> {
    let s: f64 = v.iter().sum();
    v.iter().map(|x| if s > 0.0 { x / s } else { 0.0 }).collect()
}

/// Les trois tables (un par type) d'une source et le [`Taste`] qui les porte ; `None` si les trois sont neutres (une
/// `Taste` neutre n'est **pas** passée au moteur). `num_div` : de l'unité de `num` aux kilomètres (surcouches :
/// dm × passages → km = 10 000 ; recalée : m → km = 1 000).
pub fn learn(
    source: &str,
    overlay: &str,
    num: &[Vec<f64>; 3],
    den: &[Vec<f64>; 3],
    num_div: f64,
    gamma: f64,
    p: &TasteParams,
) -> (Option<Taste>, Vec<TasteTable>) {
    let mut taste = Taste::neutral();
    let mut tables = Vec::with_capacity(3);
    for (t, (name, _)) in TYPES.iter().enumerate() {
        let num_km = num[t].iter().sum::<f64>() / num_div;
        let (q8, g) = ratio_table(&num[t], &den[t], num_km, gamma, p);
        let neutral = q8.iter().all(|&q| q == 256);
        taste.q8[t].copy_from_slice(&q8);
        tables.push(TasteTable {
            source: source.to_string(),
            overlay: overlay.to_string(),
            ride: (*name).to_string(),
            num_km,
            neutral,
            pn: shares(&num[t]),
            pd: shares(&den[t]),
            g,
            q8,
        });
    }
    ((!taste.is_neutral()).then_some(taste), tables)
}

/// [`learn`] depuis les sommes entières d'une surcouche (ou de plusieurs, mises en commun).
pub fn learn_from_sums(
    source: &str,
    overlay: &str,
    s: &ClassSums,
    gamma: f64,
    p: &TasteParams,
) -> (Option<Taste>, Vec<TasteTable>) {
    let f =
        |v: &[Vec<u64>; 3]| -> [Vec<f64>; 3] { [0, 1, 2].map(|t| v[t].iter().map(|&x| x as f64).collect()) };
    learn(source, overlay, &f(&s.ridden), &f(&s.avail), 10_000.0, gamma, p)
}

/// Les q8 de toutes les tables, dans l'ordre donné, en petit-boutiste : l'empreinte xxh3 de la variante.
pub fn taste_xxh3(tables: &[TasteTable]) -> String {
    let mut bytes: Vec<u8> = Vec::new();
    for t in tables {
        for q in &t.q8 {
            bytes.extend_from_slice(&q.to_le_bytes());
        }
    }
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(&bytes))
}

#[cfg(test)]
mod tests {
    use ord_graph::build::{build_graph, BuildOptions, MemSource};
    use ord_graph::format::write_graph;
    use ord_graph::overlay::{encode, OverlayMeta};
    use ord_graph::view::{build_view, ViewSpec};

    use super::*;

    fn p() -> TasteParams {
        TasteParams::default()
    }

    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn equal_shares_leave_the_table_neutral() {
        let v = vec![1.0; TASTE_CLASSES];
        let (q, g) = ratio_table(&v, &v, 1_000.0, 1.0, &p());
        assert!(q.iter().all(|&x| x == 256) && g.iter().all(|&x| (x - 1.0).abs() < 1e-12));
    }

    #[test]
    fn a_class_never_ridden_goes_to_a_third_and_the_bound_is_respected() {
        let mut num = vec![1.0; TASTE_CLASSES];
        let den = vec![1.0; TASTE_CLASSES];
        num[3] = 0.0;
        let (q1, g1) = ratio_table(&num, &den, 1_000.0, 1.0, &p());
        let (q05, _) = ratio_table(&num, &den, 1_000.0, 0.5, &p());
        // Les autres classes montent un peu : g(3) = m / (1 + m) × (parts) ; ici avec p_n(3) = 0 et p_d(3) = 1/72.
        let sn: f64 = num.iter().sum();
        let sd: f64 = den.iter().sum();
        let want = (0.0 + 0.5 * (1.0 / sd)) / (1.5 * (1.0 / sd));
        near(g1[3], want);
        near(want, 1.0 / 3.0);
        assert_eq!((q1[3], q05[3]), (768, 443));
        // Une classe roulée deux fois sa part : g = (2 p + m p) / ((1 + m) p) avec p_n = 2 p_d (valeur calculée ici).
        let mut num2 = vec![1.0; TASTE_CLASSES];
        num2[5] = 2.0;
        let sn2: f64 = num2.iter().sum();
        let (pn, pd) = (2.0 / sn2, 1.0 / sd);
        let (q, g) = ratio_table(&num2, &den, 1_000.0, 1.0, &p());
        near(g[5], (pn + 0.5 * pd) / (1.5 * pd));
        assert_eq!(q[5], (256.0 / g[5]).round() as u16);
        assert!(q[5] < 256 && sn > 0.0);
        // Borne 3 : une classe qui prend tout (p_n = 1, p_d minuscule) sature à g = 3, q8 = 85 à γ = 1.
        let mut num3 = vec![0.0; TASTE_CLASSES];
        num3[7] = 1.0;
        let mut den3 = vec![1_000.0; TASTE_CLASSES];
        den3[7] = 1.0;
        let (q, g) = ratio_table(&num3, &den3, 1_000.0, 1.0, &p());
        near(g[7], 3.0);
        assert_eq!(q[7], 85);
    }

    #[test]
    fn no_disposable_share_no_data_or_too_few_km_give_neutral() {
        let mut num = vec![1.0; TASTE_CLASSES];
        let mut den = vec![1.0; TASTE_CLASSES];
        den[4] = 0.0;
        let (q, g) = ratio_table(&num, &den, 1_000.0, 1.0, &p());
        assert_eq!((q[4], g[4]), (256, 1.0), "p_d = 0 : neutre");
        let (q, _) = ratio_table(&num, &den, 199.0, 1.0, &p());
        assert!(q.iter().all(|&x| x == 256), "sous le seuil : neutre partout");
        num.iter_mut().for_each(|x| *x = 0.0);
        let (q, _) = ratio_table(&num, &den, 1_000.0, 1.0, &p());
        assert!(q.iter().all(|&x| x == 256), "rien de roulé : neutre partout");
        let (q, _) = ratio_table(&[1.0; TASTE_CLASSES], &[0.0; TASTE_CLASSES], 1_000.0, 1.0, &p());
        assert!(q.iter().all(|&x| x == 256));
    }

    // --- Un petit graphe : une grille 3 × 3 de residential, une cycleway, une primary à 3 km, un tronçon hors vue.

    const LAT0: f64 = 47.0;
    const LON0: f64 = -1.80;
    const M_LAT: f64 = 111_194.926_6;

    struct World {
        base: MappedGraph,
        road: MappedGraph,
        gravel: MappedGraph,
        _dir: tempfile::TempDir,
    }

    fn node(s: &mut MemSource, id: i64, north: f64, east: f64) {
        s.node(id, LAT0 + north / M_LAT, LON0 + east / (M_LAT * LAT0.to_radians().cos()));
    }

    fn world() -> World {
        let mut s = MemSource::default();
        let id = |r: i64, c: i64| 1 + r * 3 + c;
        for r in 0..3 {
            for c in 0..3 {
                node(&mut s, id(r, c), r as f64 * 300.0, c as f64 * 300.0);
            }
        }
        let mut way = 1;
        for r in 0..3 {
            for c in 0..3 {
                for (dr, dc) in [(0, 1), (1, 0)] {
                    if r + dr < 3 && c + dc < 3 {
                        let kind = if (r, c, dr, dc) == (1, 0, 0, 1) { "cycleway" } else { "residential" };
                        s.way(way, &[id(r, c), id(r + dr, c + dc)], &[("highway", kind)]);
                        way += 1;
                    }
                }
            }
        }
        // Une primary à 3 km au nord, sans lien avec la grille, et un escalier hors de toute vue au bord de la grille.
        node(&mut s, 100, 3_000.0, 0.0);
        node(&mut s, 101, 3_300.0, 0.0);
        s.way(500, &[100, 101], &[("highway", "primary")]);
        node(&mut s, 200, -300.0, 0.0);
        s.way(600, &[id(0, 0), 200], &[("highway", "steps")]);
        let dir = tempfile::tempdir().unwrap();
        let mut data = build_graph(&mut s, &BuildOptions { zoom: 11 }, "test", &mut |_| {}).unwrap();
        let path = dir.path().join("g.ordg");
        write_graph(&path, &mut data, "test", 1_700_000_000).unwrap();
        let base = MappedGraph::open_mmap(&path).unwrap();
        let view = |name: &str| {
            let mut d = build_view(&base, &ViewSpec::parse(name).unwrap()).unwrap();
            let p = dir.path().join(format!("{name}.ordv"));
            write_graph(&p, &mut d, "test", 1_700_000_000).unwrap();
            MappedGraph::open_mmap(&p).unwrap()
        };
        let (road, gravel) = (view("road"), view("gravel"));
        World { base, road, gravel, _dir: dir }
    }

    fn edge(gf: &MappedGraph, a: i64, b: i64) -> u32 {
        (0..gf.header().edges as u32)
            .find(|&e| {
                let (x, y) = gf.edge_ends(e);
                let (x, y) = (gf.osm_node_id(x) as i64, gf.osm_node_id(y) as i64);
                (x, y) == (a, b) || (x, y) == (b, a)
            })
            .expect("arête")
    }

    fn overlay(gf: &MappedGraph, rows: &[(u32, [u16; 8])]) -> Overlay {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|r| r.0);
        let meta = OverlayMeta {
            graph: gf.graph_identity(),
            team_id: 1,
            export_xxh3: 0,
            export_rows: 0,
            params_hash: 0,
            tool_version: "test".into(),
            build_epoch: 0,
            types: vec!["ROAD".into(), "GRAVEL".into(), "MTB".into(), "MIXED".into()],
        };
        let edges: Vec<u32> = rows.iter().map(|r| r.0).collect();
        let counts: Vec<u16> = rows.iter().flat_map(|r| r.1).collect();
        Overlay::decode(&encode(&meta, &edges, &counts).unwrap()).unwrap()
    }

    fn len_dm(gf: &MappedGraph, e: u32) -> u64 {
        u64::from(gf.edge(e).len_dm)
    }

    #[test]
    fn sums_count_the_ridden_the_corridor_and_nothing_out_of_the_view() {
        let w = world();
        let ve = ViewEdges::build([&w.road, &w.gravel], w.base.header().edges as usize);
        let gf = &w.base;
        let (e1, e2) = (edge(gf, 1, 2), edge(gf, 2, 3));
        let (far, steps, near_street) = (edge(gf, 100, 101), edge(gf, 1, 200), edge(gf, 7, 8));
        assert!(!ve.contains(0, steps) && !ve.contains(1, steps), "l'escalier est hors de toute vue");
        assert!(ve.contains(0, e1) && ve.contains(1, far));
        // Deux rues de la rangée du bas roulées en ROAD (passages 1, puis 2 + 1) ; l'escalier aussi, hors vue.
        let o = overlay(
            gf,
            &[
                (e1, [1, 0, 0, 0, 0, 0, 0, 0]),
                (e2, [2, 1, 0, 0, 0, 0, 0, 0]),
                (steps, [1, 1, 0, 0, 0, 0, 0, 0]),
            ],
        );
        let s = class_sums(gf, &ve, &o, &p());
        let street = taste_class(gf.edge(e1).attrs);
        let mut want = vec![0u64; TASTE_CLASSES];
        want[street] = len_dm(gf, e1) + 3 * len_dm(gf, e2);
        assert_eq!(s.ridden[0], want, "Σ len × passages par classe, l'arête hors vue jamais comptée");
        assert!(s.ridden[1].iter().chain(&s.ridden[2]).all(|&x| x == 0));
        // La primary à 3 km est absente du disponible, la rue de la rangée du haut (600 m) y est.
        let primary = taste_class(gf.edge(far).attrs);
        assert_ne!(primary, street);
        assert_eq!(s.avail[0][primary], 0, "la primary lointaine n'est pas disponible");
        assert!(s.avail[0][street] >= len_dm(gf, near_street) + len_dm(gf, e1));
        // Une arête roulée est toujours disponible ; le disponible ne compte que la vue (pas l'escalier).
        let cyc = taste_class(gf.edge(edge(gf, 4, 5)).attrs);
        assert!(s.avail[0][cyc] > 0);
        assert!(s.avail[0].iter().sum::<u64>() >= s.ridden[0].iter().sum::<u64>() / 3);
        // Les types non roulés n'ont ni roulé ni disponible.
        assert!(s.avail[1].iter().chain(&s.avail[2]).all(|&x| x == 0));
        // Même résultat sur 1 fil et sur 4.
        let on = |n: usize| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(n)
                .build()
                .unwrap()
                .install(|| class_sums(gf, &ve, &o, &p()))
        };
        assert_eq!(on(1), on(4));
        assert_eq!(on(1), s);
    }

    #[test]
    fn pooling_adds_case_by_case_and_equal_teams_give_the_same_table() {
        let w = world();
        let ve = ViewEdges::build([&w.road, &w.gravel], w.base.header().edges as usize);
        let gf = &w.base;
        let o = overlay(
            gf,
            &[(edge(gf, 1, 2), [3, 0, 0, 0, 0, 0, 0, 0]), (edge(gf, 4, 5), [5, 0, 0, 0, 0, 0, 0, 0])],
        );
        let one = class_sums(gf, &ve, &o, &p());
        let mut two = one.clone();
        two.add(&one);
        for t in 0..3 {
            for c in 0..TASTE_CLASSES {
                assert_eq!(two.ridden[t][c], 2 * one.ridden[t][c]);
                assert_eq!(two.avail[t][c], 2 * one.avail[t][c]);
            }
        }
        // 10 000 dm × passages = 1 km-passage : le seuil est abaissé pour que la table existe sur ce petit graphe.
        let lo = TasteParams { min_km: 0.0, ..p() };
        let (_, a) = learn_from_sums("A", "full", &one, 1.0, &lo);
        let (_, b) = learn_from_sums("général", "full", &two, 1.0, &lo);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.q8, y.q8);
            assert_eq!(x.ride, y.ride);
        }
        assert!(a[0].q8.iter().any(|&q| q != 256), "la rue roulée n'a pas la part du disponible");
        assert!(a[1].neutral && a[2].neutral);
        assert_eq!(taste_xxh3(&a), taste_xxh3(&a));
    }
}
