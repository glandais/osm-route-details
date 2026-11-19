//! La fenêtre (note lot 6, §4.2, §4.3 et §5) : les chaînes d'un ensemble de tuiles z11 d'une vue, relues à plat et
//! **renumérotées en gardant l'ordre des ids globaux** ; ce que la recherche lit, et rien d'autre.
//!
//! Une fenêtre est un ensemble de tuiles, pas une requête : toute fenêtre qui contient les tuiles nécessaires à une
//! requête (§5.1) rend le même parcours, la fenêtre de toutes les tuiles étant le moteur à plat (§5.4).

use std::sync::OnceLock;

use ord_graph::format::DIR_BIT;
use ord_graph::grid::{tile_key, tile_of_e7, tile_of_key, tile_rect_e7};
use ord_graph::view::{heading_to_u16, polyline_heading};

use crate::engine::{rides_of_view, Engine, Snap, ViewData};
use crate::geo::{haversine_m, point_segment, LatLon, Local, M_PER_DEG};
use crate::{Request, RideType};

/// Octets par chaîne d'une fenêtre : tableaux (≈ 69), grilles d'accroche et marge.
const WINDOW_BYTES_PER_CHAIN: usize = 84;

const CELL_E7: i32 = 100_000;

fn cell_key(lat: i32, lon: i32) -> u32 {
    let (ilat, ilon) = (lat.div_euclid(CELL_E7) + 9_000, lon.div_euclid(CELL_E7) + 18_000);
    (ilat as u32) * 36_000 + ilon as u32
}

/// Une fenêtre : une vue, un ensemble trié de tuiles, et ce que la recherche lit.
#[derive(Debug)]
pub struct Window {
    pub(crate) vi: usize,
    pub(crate) v: ViewData,
    /// Clés (`y << 16 | x`) triées des tuiles de la fenêtre.
    tiles: Vec<u32>,
    /// Grille d'accroche de chaque tuile (même ordre que `tiles`), construite à la première accroche (§4.3).
    grids: Vec<OnceLock<Vec<(u32, u32)>>>,
}

impl Window {
    /// Indice de la vue : 0 `road`, 1 `gravel`.
    pub fn view_index(&self) -> usize {
        self.vi
    }

    pub fn n_chains(&self) -> usize {
        self.v.n_chains
    }

    pub fn tiles(&self) -> &[u32] {
        &self.tiles
    }

    /// Vrai si la fenêtre contient toutes les tuiles de `need` (deux listes triées).
    pub fn covers(&self, need: &[u32]) -> bool {
        tiles_cover(&self.tiles, need)
    }

    /// Octets tenus : tableaux de la vue, tuiles, grilles construites (capacités réelles).
    pub fn bytes(&self) -> usize {
        self.v.bytes()
            + self.tiles.capacity() * 4
            + self.grids.capacity() * std::mem::size_of::<OnceLock<Vec<(u32, u32)>>>()
            + self.grids.iter().filter_map(OnceLock::get).map(|g| g.capacity() * 8).sum::<usize>()
    }
}

/// Vrai si `have` contient toutes les clés de `need` (deux listes triées, sans doublon).
pub fn tiles_cover(have: &[u32], need: &[u32]) -> bool {
    let mut i = 0;
    for &n in need {
        while i < have.len() && have[i] < n {
            i += 1;
        }
        if i == have.len() || have[i] != n {
            return false;
        }
    }
    true
}

/// Réunion de deux listes triées.
pub fn tiles_union(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    out.sort_unstable();
    out.dedup();
    out
}

/// Ce que le moteur dit d'une requête : la vue qui la sert, les tuiles dont elle a **besoin** (§5.1) et la **classe**
/// de fenêtre qui les contient (§5.5), que le cache peut réemployer.
#[derive(Clone, Debug)]
pub struct WindowPlan {
    pub view: usize,
    pub need: Vec<u32>,
    pub class: Vec<u32>,
}

/// Cases d'accroche (0,01°) que `snap` balaie pour un point : `(ilat0, ilat1, ilon0, ilon1)`.
fn snap_cells(snap_m: f64, pe: (i32, i32)) -> (i32, i32, i32, i32) {
    let dlat = (snap_m / (M_PER_DEG * 1e-7)) as i32 + 1;
    let dlon = (f64::from(dlat) / (f64::from(pe.0) * 1e-7).to_radians().cos().max(0.05)) as i32 + 1;
    (
        (pe.0 - dlat).div_euclid(CELL_E7),
        (pe.0 + dlat).div_euclid(CELL_E7),
        (pe.1 - dlon).div_euclid(CELL_E7),
        (pe.1 + dlon).div_euclid(CELL_E7),
    )
}

/// Toutes les tuiles d'un rectangle E7 `(lat_min, lat_max, lon_min, lon_max)`.
pub(crate) fn tiles_of_rect(zoom: u8, lat: (i32, i32), lon: (i32, i32), out: &mut Vec<u32>) {
    let (x0, y0) = tile_of_e7(lat.1, lon.0, zoom);
    let (x1, y1) = tile_of_e7(lat.0, lon.1, zoom);
    for y in y0..=y1 {
        for x in x0..=x1 {
            out.push(tile_key(x, y));
        }
    }
}

/// Tuiles dont le rectangle coupe le disque de centre `center` (E7) et de rayon `radius_m`, dans le repère local
/// de `origin` (celui de `Disc` : affine en latitude et longitude, donc un rectangle y reste un rectangle).
fn tiles_of_disc(zoom: u8, origin: (i32, i32), center: (i32, i32), radius_m: f64, out: &mut Vec<u32>) {
    let loc = Local::new(origin);
    let ky = M_PER_DEG * 1e-7;
    let kx = loc.xy((origin.0, origin.1.saturating_add(1_000_000))).0 / 1e6;
    let (dlat, dlon) = ((radius_m / ky) as i32 + 2, (radius_m / kx.abs().max(1e-9)) as i32 + 2);
    let (cx, cy) = loc.xy(center);
    let (x0, y0) = tile_of_e7(center.0.saturating_add(dlat), center.1.saturating_sub(dlon), zoom);
    let (x1, y1) = tile_of_e7(center.0.saturating_sub(dlat), center.1.saturating_add(dlon), zoom);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (la0, la1, lo0, lo1) = tile_rect_e7(x, y, zoom);
            let (xa, ya) = loc.xy((la0, lo0));
            let (xb, yb) = loc.xy((la1, lo1));
            let (nx, ny) = (cx.clamp(xa.min(xb), xa.max(xb)), cy.clamp(ya.min(yb), ya.max(yb)));
            if (nx - cx).hypot(ny - cy) <= radius_m {
                out.push(tile_key(x, y));
            }
        }
    }
}

impl<B: AsRef<[u8]>> Engine<B> {
    /// Plus longue chaîne de la vue `vi`, en mètres.
    pub fn longest_chain_m(&self, vi: usize) -> f64 {
        f64::from(self.idx[vi].lmax_dm) / 10.0
    }

    /// Indice de la vue qui sert un type.
    fn vi_of(ride: RideType) -> usize {
        ride.view_index()
    }

    /// Zoom des cellules (celui du graphe de base et des vues).
    pub fn zoom(&self) -> u8 {
        self.base.header().zoom
    }

    /// Les tuiles dont la requête a besoin, et la classe de fenêtre qui les contient (§5.1 et §5.5).
    /// `radius_step_m` : pas d'arrondi du rayon de classe (0 : pas de classe, `class == need`).
    pub fn plan(&self, r: &Request, radius_step_m: f64) -> WindowPlan {
        self.plan_with(r, radius_step_m, 0.0)
    }

    /// Comme [`Engine::plan`], avec `extra_m` mètres de marge de plus autour des deux zones : la fenêtre élargie qu'on
    /// bâtit après un échappement (§5.2).
    pub fn plan_with(&self, r: &Request, radius_step_m: f64, extra_m: f64) -> WindowPlan {
        let vi = Self::vi_of(r.ride_type);
        let zoom = self.zoom();
        let s7 = r.start.to_e7();
        let e7 = r.end.unwrap_or(r.start).to_e7();
        let margin = self.params.window_margin_m + extra_m;
        // Zone 1 : le disque de recherche (boucle : autour du départ ; aller simple : autour du milieu).
        let mid = (
            ((i64::from(s7.0) + i64::from(e7.0)) / 2) as i32,
            ((i64::from(s7.1) + i64::from(e7.1)) / 2) as i32,
        );
        let center = if r.end.is_some() { mid } else { s7 };
        let radius = r.distance_m / 2.0 + margin;
        let mut need: Vec<u32> = Vec::new();
        tiles_of_disc(zoom, s7, center, radius, &mut need);
        // Zone 2 : les cases d'accroche, élargies de la plus longue chaîne (§5.1).
        let lmax_m = f64::from(self.idx[vi].lmax_dm) / 10.0 + margin;
        let ky = M_PER_DEG * 1e-7;
        for p in std::iter::once(s7).chain(r.end.map(LatLon::to_e7)) {
            let (ilat0, ilat1, ilon0, ilon1) = snap_cells(self.params.snap_m, p);
            let (la0, la1) = (ilat0 * CELL_E7, (ilat1 + 1) * CELL_E7 - 1);
            let (lo0, lo1) = (ilon0 * CELL_E7, (ilon1 + 1) * CELL_E7 - 1);
            let worst_lat = f64::from(la0.abs().max(la1.abs())) * 1e-7;
            let dlat = (lmax_m / ky) as i32 + 2;
            let dlon = (lmax_m / (ky * worst_lat.to_radians().cos().max(0.05))) as i32 + 2;
            tiles_of_rect(
                zoom,
                (la0.saturating_sub(dlat), la1.saturating_add(dlat)),
                (lo0.saturating_sub(dlon), lo1.saturating_add(dlon)),
                &mut need,
            );
        }
        need.sort_unstable();
        need.dedup();
        let class = if radius_step_m > 0.0 {
            // Classe : le disque centré sur le centre de la tuile du centre, de rayon arrondi au pas au-dessus.
            let (cx, cy) = tile_of_e7(center.0, center.1, zoom);
            let (la0, la1, lo0, lo1) = tile_rect_e7(cx, cy, zoom);
            let tc = ((la0 + la1) / 2, (lo0 + lo1) / 2);
            let off = haversine_m(tc, center);
            let steps = ((radius + off) / radius_step_m).ceil().max(1.0);
            let mut class = Vec::new();
            tiles_of_disc(zoom, tc, tc, steps * radius_step_m, &mut class);
            class.sort_unstable();
            class.dedup();
            tiles_union(&class, &need)
        } else {
            need.clone()
        };
        WindowPlan { view: vi, need, class }
    }

    /// Estimation (octets) de ce que tiendra la fenêtre de `tiles` de la vue `vi`, avec les agrégats d'une équipe :
    /// le nombre de chaînes (celles que possèdent les cellules, plus les étrangères : un majorant) fois le coût
    /// par chaîne mesuré à la construction (§4.2 : 66 à 69 o, plus les grilles d'accroche et 12 o par chaîne et par
    /// type pour l'équipe). Sert à réserver la place avant de construire.
    pub fn estimate_window_bytes(&self, vi: usize, tiles: &[u32]) -> usize {
        // Un octet de plus par chaîne avec un fichier d'attracteurs (le masque d'un nœud ; au plus un nœud par chaîne).
        self.window_chain_count(vi, tiles) * (WINDOW_BYTES_PER_CHAIN + usize::from(self.attr[vi].is_some()))
    }

    /// Nombre de chaînes de la fenêtre de `tiles` (celles que possèdent les cellules, plus les étrangères : un
    /// majorant, une chaîne étrangère à deux tuiles étant comptée deux fois).
    pub fn window_chain_count(&self, vi: usize, tiles: &[u32]) -> usize {
        let view = &self.views_files[vi];
        let mut chains = 0usize;
        for &key in tiles {
            let (x, y) = tile_of_key(key);
            if let Some(ci) = view.find_cell(x, y) {
                chains += view.cell_info(ci).n_edges as usize;
            }
            chains += self.idx[vi].foreign_of(key).len();
        }
        chains
    }

    /// Fenêtre de toutes les tuiles de la vue `vi` : le moteur à plat (référence des tests et du banc).
    pub fn full_window(&self, vi: usize) -> Window {
        let view = &self.views_files[vi];
        let mut tiles: Vec<u32> = (0..view.n_cells())
            .map(|ci| {
                let (x, y) = view.cell_xy(ci);
                tile_key(x, y)
            })
            .collect();
        // Les tuiles traversées par une chaîne sans y avoir de nœud comptent aussi : l'accroche y cherche des candidats.
        tiles.extend(self.idx[vi].foreign.iter().map(|f| f.0));
        tiles.sort_unstable();
        tiles.dedup();
        self.build_window(vi, &tiles, &mut || false).expect("jamais coupée")
    }

    /// Construit la fenêtre des tuiles `tiles` (triées, sans doublon) de la vue `vi` ; `None` si `stop` coupe.
    pub fn build_window(&self, vi: usize, tiles: &[u32], stop: &mut dyn FnMut() -> bool) -> Option<Window> {
        self.build_window_opts(vi, tiles, true, stop)
    }

    /// Comme [`Engine::build_window`] ; sans `weights`, les poids sans équipe ne sont pas calculés (la construction
    /// des attributs d'équipe n'en lit pas).
    pub(crate) fn build_window_opts(
        &self,
        vi: usize,
        tiles: &[u32],
        weights: bool,
        stop: &mut dyn FnMut() -> bool,
    ) -> Option<Window> {
        debug_assert!(tiles.windows(2).all(|w| w[0] < w[1]));
        let view = &self.views_files[vi];
        let idx = &self.idx[vi];
        let zoom = idx.zoom;
        // 1. Chaînes : celles que possèdent les cellules des tuiles, et les étrangères qui les touchent.
        let mut gid: Vec<u32> = Vec::new();
        for (k, &key) in tiles.iter().enumerate() {
            if k % 16 == 0 && stop() {
                return None;
            }
            let (x, y) = tile_of_key(key);
            if let Some(ci) = view.find_cell(x, y) {
                let i = view.cell_info(ci);
                gid.extend(i.first_edge..i.first_edge + i.n_edges);
            }
            gid.extend(idx.foreign_of(key).iter().map(|f| f.1));
        }
        gid.sort_unstable();
        gid.dedup();
        gid.shrink_to_fit();
        let n = gid.len();

        // 2. Une lecture par chaîne, dans l'ordre des ids : un curseur de cellule suffit.
        let mut ends_g: Vec<(u32, u32)> = Vec::with_capacity(n);
        let mut len_dm: Vec<u32> = Vec::with_capacity(n);
        let mut open: Vec<u8> = Vec::with_capacity(n);
        let (mut first, mut end) = (1u32, 0u32);
        let mut cv = None;
        for (k, &c) in gid.iter().enumerate() {
            if k % 65_536 == 0 && stop() {
                return None;
            }
            if c < first || c >= end {
                let ci = view.edge_cell(c);
                let info = *view.cell_info(ci);
                (first, end) = (info.first_edge, info.first_edge + info.n_edges);
                cv = Some(view.cell(ci));
            }
            let o = cv.as_ref().expect("cellule courante").owned((c - first) as usize);
            ends_g.push(view.edge_ends(c));
            len_dm.push(o.len_dm);
            open.push(u8::from(o.attrs.fwd_ok()) | (u8::from(o.attrs.bwd_ok()) << 1));
        }

        // 3. Nœuds : les bouts des chaînes, triés ; position par un curseur de cellule.
        let mut ngid: Vec<u32> = Vec::with_capacity(2 * n);
        for &(a, b) in &ends_g {
            ngid.push(a);
            ngid.push(b);
        }
        ngid.sort_unstable();
        ngid.dedup();
        ngid.shrink_to_fit();
        let n_nodes = ngid.len();
        let mut pos: Vec<(i32, i32)> = Vec::with_capacity(n_nodes);
        let (mut nfirst, mut nend) = (1u32, 0u32);
        let mut ncv = None;
        for &g in &ngid {
            if g < nfirst || g >= nend {
                let ci = view.node_cell(g);
                let info = *view.cell_info(ci);
                (nfirst, nend) = (info.first_node, info.first_node + info.n_nodes);
                ncv = Some(view.cell(ci));
            }
            pos.push(ncv.as_ref().expect("cellule courante").node((g - nfirst) as usize));
        }
        let local = |g: u32| ngid.binary_search(&g).expect("bout d'une chaîne chargée") as u32;
        let ends: Vec<(u32, u32)> = ends_g.iter().map(|&(a, b)| (local(a), local(b))).collect();
        drop(ends_g);

        // 4. CSR sortant et entrant, degrés.
        let (mut out_off, mut in_off) = (vec![0u32; n_nodes + 1], vec![0u32; n_nodes + 1]);
        let mut deg = vec![0u8; n_nodes];
        for (c, &(a, b)) in ends.iter().enumerate() {
            deg[a as usize] = deg[a as usize].saturating_add(1);
            deg[b as usize] = deg[b as usize].saturating_add(1);
            if open[c] & 1 != 0 {
                out_off[a as usize + 1] += 1;
                in_off[b as usize + 1] += 1;
            }
            if open[c] & 2 != 0 {
                out_off[b as usize + 1] += 1;
                in_off[a as usize + 1] += 1;
            }
        }
        for v in 0..n_nodes {
            out_off[v + 1] += out_off[v];
            in_off[v + 1] += in_off[v];
        }
        let (mut out_f, mut in_f) =
            (vec![0u32; out_off[n_nodes] as usize], vec![0u32; in_off[n_nodes] as usize]);
        let (mut of, mut inf) = (out_off.clone(), in_off.clone());
        for (c, &(a, b)) in ends.iter().enumerate() {
            let c2 = (c as u32) << 1;
            if open[c] & 1 != 0 {
                out_f[of[a as usize] as usize] = c2;
                of[a as usize] += 1;
                in_f[inf[b as usize] as usize] = c2;
                inf[b as usize] += 1;
            }
            if open[c] & 2 != 0 {
                out_f[of[b as usize] as usize] = c2 | 1;
                of[b as usize] += 1;
                in_f[inf[a as usize] as usize] = c2 | 1;
                inf[a as usize] += 1;
            }
        }
        drop((of, inf));

        // 5. Liens (ids globaux d'arêtes de base), caps, poids sans équipe.
        let mut chain_off: Vec<u32> = Vec::with_capacity(n + 1);
        let mut chain_links: Vec<u32> = Vec::new();
        chain_off.push(0);
        let stored_heads = view.view_heading_count() == view.header().edges as usize;
        let mut head: Vec<[u16; 2]> = Vec::with_capacity(n);
        let rides = rides_of_view(vi);
        let mut w_osm: [Vec<u32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        if weights {
            for &r in rides {
                w_osm[r.index()] = Vec::with_capacity(n);
            }
        }
        for (k, &c) in gid.iter().enumerate() {
            if k % 65_536 == 0 && stop() {
                return None;
            }
            let links = view.view_chain(c).expect("chaînes validées à l'ouverture");
            chain_links.extend(links.iter().map(|x| x.get()));
            chain_off.push(chain_links.len() as u32);
            head.push(match stored_heads.then(|| view.view_heading(c)).flatten() {
                Some(h) => h,
                None => {
                    // Vue de format minor 1 : caps recalculés depuis la géométrie.
                    let mut pts = vec![pos[ends[k].0 as usize]];
                    pts.extend(view.geometry(c));
                    pts.push(pos[ends[k].1 as usize]);
                    [
                        heading_to_u16(polyline_heading(&pts, true)),
                        heading_to_u16(polyline_heading(&pts, false)),
                    ]
                }
            });
            if weights {
                let mut sums = [0u64; 3];
                for l in links {
                    let (a, len) = self.attrs_len(l.get() & !DIR_BIT);
                    for &r in rides {
                        sums[r.index()] += u64::from(self.base_weight(r, a, len));
                    }
                }
                for &r in rides {
                    w_osm[r.index()].push(sums[r.index()].min(u64::from(u32::MAX / 8)) as u32);
                }
            }
        }
        chain_links.shrink_to_fit();

        // 6. Complétude : un nœud dont la tuile est dans la fenêtre a toutes ses chaînes (§5.2).
        let complete: Vec<bool> =
            pos.iter().map(|p| tiles.binary_search(&tile_key_of(p, zoom)).is_ok()).collect();

        // 7. Attracteurs (lot 4, 4d) : le masque de chaque nœud, lu dans la cellule du nœud (jamais « dans les tuiles de la
        // fenêtre » : un nœud qui sort des tuiles garde le même masque, I4). Les `ngid` d'une même cellule se suivent.
        let attr: Vec<u8> = match &self.attr[vi] {
            None => Vec::new(),
            Some(file) => {
                let mut masks = vec![0u8; n_nodes];
                let (mut nfirst, mut nend) = (1u32, 0u32);
                let mut zone: (&[ord_graph::attract::U32], &[u8]) = (&[], &[]);
                let mut zi = 0usize;
                for (k, &g) in ngid.iter().enumerate() {
                    if g < nfirst || g >= nend {
                        let ci = view.node_cell(g);
                        let info = view.cell_info(ci);
                        (nfirst, nend) = (info.first_node, info.first_node + info.n_nodes);
                        zone = file.zone_of_cell(ci);
                        zi = zone.0.partition_point(|x| x.get() < g);
                    }
                    while zi < zone.0.len() && zone.0[zi].get() < g {
                        zi += 1;
                    }
                    if zi < zone.0.len() && zone.0[zi].get() == g {
                        masks[k] = zone.1[zi];
                    }
                }
                masks
            }
        };

        let v = ViewData {
            n_nodes,
            n_chains: n,
            pos,
            ends,
            len_dm,
            open,
            out_off,
            out_f,
            in_off,
            in_f,
            chain_off,
            chain_links,
            head,
            deg,
            w_osm,
            gid,
            ngid,
            complete,
            attr,
        };
        let grids = (0..tiles.len()).map(|_| OnceLock::new()).collect();
        Some(Window { vi, v, tiles: tiles.to_vec(), grids })
    }

    /// Grille d'accroche de la tuile d'indice `ti` de la fenêtre : `(case de 0,01°, chaîne locale)` des points
    /// échantillonnés (un tous les 400 m) qui tombent dans la tuile, triés (§4.3).
    fn tile_grid<'a>(&self, win: &'a Window, ti: usize) -> &'a [(u32, u32)] {
        win.grids[ti].get_or_init(|| {
            let key = win.tiles[ti];
            let (tx, ty) = tile_of_key(key);
            let view = &self.views_files[win.vi];
            let zoom = self.zoom();
            let mut globals: Vec<u32> = Vec::new();
            if let Some(ci) = view.find_cell(tx, ty) {
                let i = view.cell_info(ci);
                globals.extend(i.first_edge..i.first_edge + i.n_edges);
            }
            globals.extend(self.idx[win.vi].foreign_of(key).iter().map(|f| f.1));
            let v = &win.v;
            let mut entries: Vec<(u32, u32)> = Vec::new();
            for g in globals {
                let lc = v.gid.binary_search(&g).expect("chaîne de la tuile chargée");
                let mut pts = vec![v.pos[v.ends[lc].0 as usize]];
                pts.extend(view.geometry(g));
                pts.push(v.pos[v.ends[lc].1 as usize]);
                for w in pts.windows(2) {
                    let n = (haversine_m(w[0], w[1]) / 400.0).ceil().max(1.0) as i32;
                    for k in 0..=n {
                        let t = f64::from(k) / f64::from(n);
                        let lat = w[0].0 + (f64::from(w[1].0 - w[0].0) * t) as i32;
                        let lon = w[0].1 + (f64::from(w[1].1 - w[0].1) * t) as i32;
                        if tile_of_e7(lat, lon, zoom) == (tx, ty) {
                            entries.push((cell_key(lat, lon), lc as u32));
                        }
                    }
                }
            }
            entries.sort_unstable();
            entries.dedup();
            entries.shrink_to_fit();
            entries
        })
    }

    /// Accroche `p` sur l'arête de base la plus proche (≤ `snap_m`) parmi celles dont la chaîne est acceptée par
    /// `ok` (id **local**) et dont la composante fait au moins `min_comp_dm` ; égalité : plus petit id de chaîne.
    /// `Err(())` si la boîte d'accroche sort de la fenêtre (garde-fou : jamais avec les tuiles nécessaires).
    pub(crate) fn snap(
        &self,
        win: &Window,
        p: LatLon,
        ok: &dyn Fn(u32) -> bool,
        min_comp_dm: u64,
    ) -> Result<Option<Snap>, ()> {
        let v = &win.v;
        let pe = p.to_e7();
        let loc = Local::new(pe);
        let (ilat0, ilat1, ilon0, ilon1) = snap_cells(self.params.snap_m, pe);
        let zoom = self.zoom();
        // Tuiles qui recouvrent les cases de la boîte.
        let mut boxed: Vec<u32> = Vec::new();
        tiles_of_rect(
            zoom,
            (ilat0 * CELL_E7, (ilat1 + 1) * CELL_E7 - 1),
            (ilon0 * CELL_E7, (ilon1 + 1) * CELL_E7 - 1),
            &mut boxed,
        );
        let mut cand: Vec<u32> = Vec::new();
        for key in boxed {
            match win.tiles.binary_search(&key) {
                Ok(ti) => {
                    let grid = self.tile_grid(win, ti);
                    for ilat in ilat0..=ilat1 {
                        for ilon in ilon0..=ilon1 {
                            let k = cell_key(ilat * CELL_E7, ilon * CELL_E7);
                            let lo = grid.partition_point(|e| e.0 < k);
                            let hi = grid.partition_point(|e| e.0 <= k);
                            cand.extend(grid[lo..hi].iter().map(|e| e.1));
                        }
                    }
                }
                Err(_) => {
                    let (x, y) = tile_of_key(key);
                    if self.views_files[win.vi].find_cell(x, y).is_some()
                        || !self.idx[win.vi].foreign_of(key).is_empty()
                    {
                        return Err(());
                    }
                }
            }
        }
        cand.sort_unstable();
        cand.dedup();
        let pxy = (0.0, 0.0);
        // Jamais à moins d'un mètre d'un bout d'arête : pas de morceau de longueur nulle au départ ni à l'arrivée.
        let margin: f64 = 1.0;
        let mut best: Option<Snap> = None;
        let idx = &self.idx[win.vi];
        for c in cand {
            if !ok(c) || idx.comp_len_dm(v.gid[c as usize]) < min_comp_dm {
                continue;
            }
            for (li, &l) in v.links(c).iter().enumerate() {
                let (e, rev) = (l & !DIR_BIT, l & DIR_BIT != 0);
                let pts = self.edge_polyline(e);
                let xy: Vec<(f64, f64)> = pts.iter().map(|&q| loc.xy(q)).collect();
                let mut acc = 0.0;
                let total: f64 = pts.windows(2).map(|w| haversine_m(w[0], w[1])).sum();
                let mut seg_best: Option<(f64, f64)> = None; // (distance, abscisse naturelle)
                for (i, w) in xy.windows(2).enumerate() {
                    let seg_len = haversine_m(pts[i], pts[i + 1]);
                    let (d, t) = point_segment(pxy, w[0], w[1]);
                    if seg_best.is_none_or(|(bd, _)| d < bd) {
                        seg_best = Some((d, acc + t * seg_len));
                    }
                    acc += seg_len;
                }
                let Some((d, off)) = seg_best else { continue };
                if d > self.params.snap_m {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some(b) => d < b.dist_m - 1e-9 || ((d - b.dist_m).abs() <= 1e-9 && c < b.chain),
                };
                if better {
                    best = Some(Snap {
                        chain: c,
                        link: li,
                        edge: e,
                        rev,
                        offset_m: off.clamp(margin.min(total / 2.0), total - margin.min(total / 2.0)),
                        dist_m: d,
                    });
                }
            }
        }
        Ok(best)
    }
}

pub(crate) fn tile_key_of(p: &(i32, i32), zoom: u8) -> u32 {
    let (x, y) = tile_of_e7(p.0, p.1, zoom);
    tile_key(x, y)
}
