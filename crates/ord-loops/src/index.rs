//! Index globaux d'une vue, calculés une fois à l'ouverture (note lot 6, §4.1 et §6) : ce qui ne se calcule pas dans
//! une fenêtre parce que la réponse dépend du réseau entier.
//!
//! - **chaînes étrangères** : pour chaque tuile, les chaînes qui la touchent sans lui appartenir (elle appartient à la
//!   cellule du fichier qui la range). Sans elles, un nœud près du bord d'une fenêtre aurait des chaînes manquantes ;
//! - **composantes fortement connexes**, sous forme creuse : la longueur de la composante d'une chaîne ne sert qu'à
//!   comparer à `0,9 D`, donc seules comptent les chaînes dont la composante fait moins de `CAP` ;
//! - la **plus longue chaîne** : elle borne la zone où chercher les candidats d'une accroche.

use ord_graph::format::GraphFile;
use ord_graph::grid::{tile_key, tile_of_e7, tile_rect_e7};

use std::sync::Arc;

use ord_graph::service_index::{IndexFile, Pair};

use crate::engine::{strong_components, Params};
use crate::geo::M_PER_DEG;

/// Les tables d'un fichier d'index de service (`.ordi`), vues sans leur type d'octets : le moteur les lit en place.
pub(crate) trait IndexTables: Send + Sync + std::fmt::Debug {
    fn foreign(&self, vi: usize) -> &[Pair];
    fn short(&self, vi: usize) -> &[Pair];
}

impl<F: AsRef<[u8]> + Send + Sync + std::fmt::Debug> IndexTables for IndexFile<F> {
    fn foreign(&self, vi: usize) -> &[Pair] {
        IndexFile::foreign(self, vi)
    }
    fn short(&self, vi: usize) -> &[Pair] {
        IndexFile::short(self, vi)
    }
}

/// Où sont les tables : calculées à l'ouverture, ou lues dans le fichier d'index de service.
#[derive(Debug)]
enum Tables {
    Computed {
        /// `(clé de tuile, id global de chaîne)`, triés : chaînes qui touchent la tuile sans la posséder.
        foreign: Vec<(u32, u32)>,
        /// `(id global de chaîne, longueur dm de sa composante)` pour les chaînes dont la composante fait moins de
        /// `cap_dm` (0 si les deux bouts sont dans deux composantes), triés.
        short_comp: Vec<(u32, u64)>,
    },
    File {
        file: Arc<dyn IndexTables>,
        vi: usize,
    },
}

/// Les chaînes étrangères d'une tuile, d'où qu'elles viennent.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ForeignSlice<'a> {
    Computed(&'a [(u32, u32)]),
    File(&'a [Pair]),
}

impl<'a> ForeignSlice<'a> {
    pub fn len(&self) -> usize {
        match self {
            Self::Computed(s) => s.len(),
            Self::File(s) => s.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `(clé de tuile, id global de chaîne)`, dans l'ordre.
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32)> + 'a {
        let (c, f) = match *self {
            Self::Computed(s) => (s, &[][..]),
            Self::File(s) => (&[][..], s),
        };
        c.iter().copied().chain(f.iter().map(|p| (p[0].get(), p[1].get())))
    }
}

#[derive(Debug)]
pub(crate) struct ViewIndex {
    pub zoom: u8,
    tables: Tables,
    pub cap_dm: u64,
    /// Plus longue chaîne, en dm.
    pub lmax_dm: u32,
    /// Durées de construction, en ms : balayage des étrangères, composantes (0 : lues dans le fichier).
    pub foreign_ms: u64,
    pub scc_ms: u64,
}

/// Tuiles `(clé)` touchées par une polyligne : celles des boîtes de ses segments.
pub(crate) fn tiles_of_polyline(pts: &[(i32, i32)], zoom: u8, mut f: impl FnMut(u32)) {
    for w in pts.windows(2) {
        let (la0, la1) = (w[0].0.min(w[1].0), w[0].0.max(w[1].0));
        let (lo0, lo1) = (w[0].1.min(w[1].1), w[0].1.max(w[1].1));
        let (x0, y0) = tile_of_e7(la1, lo0, zoom);
        let (x1, y1) = tile_of_e7(la0, lo1, zoom);
        for y in y0..=y1 {
            for x in x0..=x1 {
                f(tile_key(x, y));
            }
        }
    }
}

/// Distance (m) d'un point à la frontière de sa tuile, approchée en équirectangulaire (par défaut, 0 si le point
/// n'est pas dans la tuile).
fn margin_to_tile_edge_m(p: (i32, i32), tile: (u32, u32), zoom: u8) -> f64 {
    if tile_of_e7(p.0, p.1, zoom) != tile {
        return 0.0;
    }
    let (la0, la1, lo0, lo1) = tile_rect_e7(tile.0, tile.1, zoom);
    // Le rectangle est élargi d'une unité : on le rétrécit d'autant (côté sûr).
    let (la0, la1, lo0, lo1) = (la0 + 1, la1 - 1, lo0 + 1, lo1 - 1);
    let ky = M_PER_DEG * 1e-7;
    let kx = ky * (f64::from(p.0) * 1e-7).to_radians().cos();
    let dy = f64::from((p.0 - la0).min(la1 - p.0)) * ky;
    let dx = f64::from((p.1 - lo0).min(lo1 - p.1)) * kx;
    dx.min(dy).max(0.0)
}

/// Chaînes étrangères des cellules `cells` de la vue.
fn foreign_of_cells<B: AsRef<[u8]>>(view: &GraphFile<B>, cells: std::ops::Range<usize>) -> Vec<(u32, u32)> {
    let zoom = view.header().zoom;
    let mut out: Vec<(u32, u32)> = Vec::new();
    for ci in cells {
        let cv = view.cell(ci);
        let info = *cv.info();
        let own = tile_key(info.x, info.y);
        for i in 0..info.n_edges {
            let c = info.first_edge + i;
            let (a, b) = view.edge_ends(c);
            let pa = view.node_pos(a);
            // Un point de la chaîne est à moins de sa longueur de `a` : si `a` est plus loin que cela du bord de sa
            // tuile, la chaîne n'en sort pas (les boîtes de ses segments non plus : un rectangle est convexe).
            let len_m = f64::from(cv.owned(i as usize).len_dm) / 10.0;
            if margin_to_tile_edge_m(pa, (info.x, info.y), zoom) > len_m * 1.02 + 5.0 {
                continue;
            }
            let mut pts = vec![pa];
            pts.extend(view.geometry(c));
            pts.push(view.node_pos(b));
            let first = out.len();
            tiles_of_polyline(&pts, zoom, |k| {
                if k != own {
                    out.push((k, c));
                }
            });
            // Retire les doublons de cette chaîne tout de suite : une longue chaîne en pousse beaucoup.
            if out.len() - first > 1 {
                out[first..].sort_unstable();
                let mut keep = first + 1;
                for j in first + 1..out.len() {
                    if out[j] != out[keep - 1] {
                        out[keep] = out[j];
                        keep += 1;
                    }
                }
                out.truncate(keep);
            }
        }
    }
    out
}

/// Index des chaînes étrangères d'une vue (note lot 6 §4.1) : `(clé de tuile, id de chaîne)` pour chaque tuile que
/// touche une chaîne sans lui appartenir, triés et sans doublon. `threads` fils, par paquets de cellules.
pub fn foreign_index<B: AsRef<[u8]> + Sync>(view: &GraphFile<B>, threads: usize) -> Vec<(u32, u32)> {
    let n_cells = view.n_cells();
    let threads = threads.clamp(1, n_cells.max(1));
    let mut foreign: Vec<(u32, u32)> = if threads == 1 {
        foreign_of_cells(view, 0..n_cells)
    } else {
        let step = n_cells.div_ceil(threads);
        std::thread::scope(|s| {
            let parts: Vec<_> = (0..threads)
                .map(|t| {
                    let r = (t * step).min(n_cells)..((t + 1) * step).min(n_cells);
                    s.spawn(move || foreign_of_cells(view, r))
                })
                .collect();
            parts.into_iter().flat_map(|h| h.join().expect("fil d'index")).collect()
        })
    };
    foreign.sort_unstable();
    foreign.dedup();
    foreign.shrink_to_fit();
    foreign
}

/// Composantes fortement connexes d'une vue (note lot 6 §6) : `(id de chaîne, longueur dm de sa composante)` pour les
/// chaînes dont la composante fait moins de `cap_dm` (0 si ses deux bouts sont dans deux composantes), triés ; et la
/// plus longue chaîne, en dm. Tarjan sur toute la vue : 13 o par nœud.
pub fn short_components<B: AsRef<[u8]>>(view: &GraphFile<B>, cap_dm: u64) -> (Vec<(u32, u64)>, u32) {
    let n_cells = view.n_cells();
    let n_nodes = view.header().nodes as usize;
    let comp = strong_components(n_nodes, &|x, buf| {
        let cv = view.cell(view.node_cell(x));
        let local = (x - cv.info().first_node) as usize;
        buf.extend(cv.out(local).iter().map(|d| d.target()));
    });
    let mut comp_len = vec![0u64; comp.iter().max().map_or(0, |&m| m as usize + 1)];
    let mut lmax_dm = 0u32;
    for ci in 0..n_cells {
        let cv = view.cell(ci);
        let info = *cv.info();
        for i in 0..info.n_edges {
            let (a, b) = view.edge_ends(info.first_edge + i);
            let len = cv.owned(i as usize).len_dm;
            lmax_dm = lmax_dm.max(len);
            if comp[a as usize] == comp[b as usize] {
                comp_len[comp[a as usize] as usize] += u64::from(len);
            }
        }
    }
    let mut short_comp: Vec<(u32, u64)> = Vec::new();
    for ci in 0..n_cells {
        let info = *view.cell(ci).info();
        for i in 0..info.n_edges {
            let c = info.first_edge + i;
            let (a, b) = view.edge_ends(c);
            let l =
                if comp[a as usize] == comp[b as usize] { comp_len[comp[a as usize] as usize] } else { 0 };
            if l < cap_dm {
                short_comp.push((c, l));
            }
        }
    }
    short_comp.sort_unstable();
    short_comp.shrink_to_fit();
    (short_comp, lmax_dm)
}

/// Seuil des composantes courtes (dm) des réglages : `⌈min_component_share × max_distance_m × 10⌉`.
pub fn cap_dm_of(params: &Params) -> u64 {
    (params.min_component_share * params.max_distance_m * 10.0).ceil() as u64
}

impl ViewIndex {
    pub fn build<B: AsRef<[u8]> + Sync>(view: &GraphFile<B>, params: &Params, threads: usize) -> Self {
        let t0 = std::time::Instant::now();
        let foreign = foreign_index(view, threads);
        let foreign_ms = t0.elapsed().as_millis() as u64;
        let t1 = std::time::Instant::now();
        let cap_dm = cap_dm_of(params);
        let (short_comp, lmax_dm) = short_components(view, cap_dm);
        let scc_ms = t1.elapsed().as_millis() as u64;
        Self {
            zoom: view.header().zoom,
            tables: Tables::Computed { foreign, short_comp },
            cap_dm,
            lmax_dm,
            foreign_ms,
            scc_ms,
        }
    }

    /// Les tables de la vue `vi` lues dans un fichier d'index de service, au seuil des réglages (`cap_dm`, au plus
    /// celui du fichier : vérifié par l'appelant).
    pub fn from_file(zoom: u8, file: Arc<dyn IndexTables>, vi: usize, cap_dm: u64, lmax_dm: u32) -> Self {
        Self { zoom, tables: Tables::File { file, vi }, cap_dm, lmax_dm, foreign_ms: 0, scc_ms: 0 }
    }

    /// Vrai si les tables viennent du fichier d'index de service.
    pub fn is_from_index_file(&self) -> bool {
        matches!(self.tables, Tables::File { .. })
    }

    /// Toutes les chaînes étrangères, `(clé de tuile, id de chaîne)` croissants.
    pub fn foreign_all(&self) -> ForeignSlice<'_> {
        match &self.tables {
            Tables::Computed { foreign, .. } => ForeignSlice::Computed(foreign),
            Tables::File { file, vi } => ForeignSlice::File(file.foreign(*vi)),
        }
    }

    /// Chaînes étrangères de la tuile `key`.
    pub fn foreign_of(&self, key: u32) -> ForeignSlice<'_> {
        match &self.tables {
            Tables::Computed { foreign, .. } => {
                let lo = foreign.partition_point(|x| x.0 < key);
                let hi = foreign.partition_point(|x| x.0 <= key);
                ForeignSlice::Computed(&foreign[lo..hi])
            }
            Tables::File { file, vi } => {
                let f = file.foreign(*vi);
                let lo = f.partition_point(|x| x[0].get() < key);
                let hi = f.partition_point(|x| x[0].get() <= key);
                ForeignSlice::File(&f[lo..hi])
            }
        }
    }

    /// Longueur (dm) de la composante fortement connexe de la chaîne `c` ; `cap_dm` (« au moins ») si elle est plus
    /// longue : la comparaison à un seuil d'au plus `cap_dm` reste exacte. Le fichier peut avoir un seuil plus haut :
    /// une longueur au-delà de `cap_dm` y vaut `cap_dm`, comme dans les tables calculées.
    pub fn comp_len_dm(&self, c: u32) -> u64 {
        match &self.tables {
            Tables::Computed { short_comp, .. } => match short_comp.binary_search_by_key(&c, |x| x.0) {
                Ok(i) => short_comp[i].1,
                Err(_) => self.cap_dm,
            },
            Tables::File { file, vi } => {
                let s = file.short(*vi);
                match s.binary_search_by_key(&c, |x| x[0].get()) {
                    Ok(i) => u64::from(s[i][1].get()).min(self.cap_dm),
                    Err(_) => self.cap_dm,
                }
            }
        }
    }

    /// Octets de mémoire anonyme tenus par l'index (0 pour les tables du fichier : des pages de fichier).
    pub fn bytes(&self) -> usize {
        match &self.tables {
            Tables::Computed { foreign, short_comp } => foreign.capacity() * 8 + short_comp.capacity() * 16,
            Tables::File { .. } => 0,
        }
    }
}
