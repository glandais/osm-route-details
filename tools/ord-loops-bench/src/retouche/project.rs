//! Polyligne du document d'une trace et projection monotone des coutures (note : `ord-retouche-lot0.md` §2.4).
//!
//! Le critère « une couture à moins de 1 km de tout point » parle de la distance **le long de la trace**. Sur la marche,
//! un trou ne vaut que sa corde ; on mesure donc sur la polyligne du **document** (les géométries `g` mises bout à
//! bout dans l'ordre), où les coutures sont posées par une projection qui avance avec la marche : un aller-retour, une
//! boucle en 8 ou des tours répétés trouvent chacun le bon passage, et non le premier venu.

use ord_loops_check::haversine_m;

/// Mètres par degré de latitude (sphère de `ord-loops-check`, approximation locale pour les projections).
const M_LAT: f64 = 111_194.926_6;

/// La polyligne du document : points `(lat, lon)` et abscisses haversine cumulées.
#[derive(Clone, Debug)]
pub struct DocLine {
    pub pts: Vec<(f64, f64)>,
    pub s: Vec<f64>,
}

impl DocLine {
    /// Met bout à bout les géométries dans l'ordre, le premier point de chacune (sauf la première) retiré, comme
    /// `raw_legacy`.
    pub fn from_parts<I: IntoIterator<Item = Vec<[f64; 2]>>>(parts: I) -> Self {
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for g in parts {
            let skip = usize::from(!pts.is_empty());
            pts.extend(g.into_iter().skip(skip).map(|p| (p[0], p[1])));
        }
        Self::from_points(pts)
    }

    pub fn from_points(pts: Vec<(f64, f64)>) -> Self {
        let mut s = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                acc += haversine_m(pts[i - 1], *p);
            }
            s.push(acc);
        }
        Self { pts, s }
    }

    /// Longueur `L`, m.
    pub fn len_m(&self) -> f64 {
        self.s.last().copied().unwrap_or(0.0)
    }

    /// Distance du point à la polyligne sur le segment `i` (entre `pts[i]` et `pts[i + 1]`) et abscisse de la projection.
    fn on_segment(&self, i: usize, p: (f64, f64)) -> (f64, f64) {
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        let kx = M_LAT * p.0.to_radians().cos();
        let (ax, ay) = ((a.1 - p.1) * kx, (a.0 - p.0) * M_LAT);
        let (bx, by) = ((b.1 - p.1) * kx, (b.0 - p.0) * M_LAT);
        let (dx, dy) = (bx - ax, by - ay);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 { (-(ax * dx + ay * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let (qx, qy) = (ax + t * dx, ay + t * dy);
        (qx.hypot(qy), self.s[i] + t * (self.s[i + 1] - self.s[i]))
    }

    /// Le plus proche point de toute la polyligne : (distance, abscisse).
    fn nearest(&self, p: (f64, f64)) -> Option<(f64, f64)> {
        (0..self.pts.len().saturating_sub(1))
            .map(|i| self.on_segment(i, p))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .or_else(|| self.pts.first().map(|q| (haversine_m(*q, p), 0.0)))
    }
}

/// Une couture à projeter : son nœud et son abscisse sur la marche.
#[derive(Clone, Copy, Debug)]
pub struct SeamPoint {
    pub latlon: (f64, f64),
    /// Abscisse sur la polyligne de la marche (`Trace::join_arc`).
    pub a_m: f64,
    /// Marge ajoutée à la fenêtre : un trou ne vaut que sa corde sur la marche, et le passage non apparié qu'il
    /// recouvre est plus long (trois fois la somme des cordes des trous depuis la couture précédente). Un complément à
    /// la fenêtre du §2.4, qui sinon déclarait « anomalie » toute couture qui suit un trou de plus de quelques centaines
    /// de mètres.
    pub slack_m: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Projection {
    /// Abscisse de trace de chaque couture, croissante.
    pub s: Vec<f64>,
    /// Distance du nœud à la polyligne, par couture.
    pub dist_m: Vec<f64>,
    /// Coutures introuvables dans leur fenêtre (projetées sur le plus proche point global).
    pub anomalies: usize,
}

/// Projette les coutures (dans l'ordre de la marche) sur la polyligne : chaque recherche part de l'abscisse de la
/// couture précédente et ne lit que la fenêtre `[s₋ + 0,8 Δa − 50 ; s₋ + 1,5 Δa + 200]` ; elle y prend le premier
/// minimum local de la distance qui tombe à `tol_m` ou moins.
pub fn project(line: &DocLine, seams: &[SeamPoint], tol_m: f64) -> Projection {
    let mut out = Projection::default();
    let (mut prev_s, mut prev_a) = (0.0f64, 0.0f64);
    let nseg = line.pts.len().saturating_sub(1);
    for sp in seams {
        let da = (sp.a_m - prev_a).max(0.0);
        let lo = (prev_s + 0.8 * da - 50.0).max(prev_s);
        let hi = prev_s + 1.5 * da + 200.0 + sp.slack_m;
        // Segments dont l'abscisse de fin dépasse `lo` jusqu'à ceux dont le début dépasse `hi`.
        let i0 = line.s.partition_point(|&x| x < lo).saturating_sub(1).min(nseg);
        let i1 = line.s.partition_point(|&x| x <= hi).min(nseg);
        let mut found = None;
        let mut i = i0;
        while i < i1 {
            let (d, s) = line.on_segment(i, sp.latlon);
            if d <= tol_m {
                let (mut bd, mut bs) = (d, s);
                let mut j = i + 1;
                while j < i1 {
                    let (d2, s2) = line.on_segment(j, sp.latlon);
                    if d2 < bd {
                        (bd, bs) = (d2, s2);
                        j += 1;
                    } else {
                        break;
                    }
                }
                found = Some((bd, bs));
                break;
            }
            i += 1;
        }
        let (d, s) = match found {
            Some(x) => x,
            None => {
                out.anomalies += 1;
                line.nearest(sp.latlon).unwrap_or((f64::INFINITY, prev_s))
            }
        };
        let s = s.max(prev_s);
        out.s.push(s);
        out.dist_m.push(d);
        prev_s = s;
        prev_a = sp.a_m;
    }
    out
}

/// La plus grande distance, sur `[lo, hi]`, d'un point à la couture la plus proche (`seams` triées) ; `None` sans
/// couture. Les extrémités et les milieux de deux coutures consécutives suffisent : la distance est linéaire par
/// morceaux.
pub fn max_dist_to_seam(seams: &[f64], lo: f64, hi: f64) -> Option<f64> {
    if seams.is_empty() || hi < lo {
        return None;
    }
    let near = |x: f64| seams.iter().map(|s| (s - x).abs()).fold(f64::INFINITY, f64::min);
    let mut cands = vec![lo, hi];
    for w in seams.windows(2) {
        let mid = (w[0] + w[1]) / 2.0;
        if mid > lo && mid < hi {
            cands.push(mid);
        }
    }
    Some(cands.into_iter().map(near).fold(0.0, f64::max))
}

/// Le plus grand intervalle entre deux coutures consécutives (bouts exclus) ; `0` pour moins de deux coutures.
pub fn max_gap(seams: &[f64]) -> f64 {
    seams.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une ligne le long d'un parallèle : `x` mètres à l'est de l'origine, `y` au nord.
    fn pt(x: f64, y: f64) -> (f64, f64) {
        let lat0 = 47.0f64;
        (lat0 + y / M_LAT, -1.8 + x / (M_LAT * lat0.to_radians().cos()))
    }

    fn line(p: &[(f64, f64)]) -> DocLine {
        DocLine::from_points(p.iter().map(|&(x, y)| pt(x, y)).collect())
    }

    fn sp(x: f64, y: f64, a: f64) -> SeamPoint {
        SeamPoint { latlon: pt(x, y), a_m: a, slack_m: 0.0 }
    }

    #[test]
    fn out_and_back_finds_each_pass_of_the_same_node() {
        // Aller de 0 à 2000 m puis retour : le nœud à 1000 m est passé deux fois (abscisses 1000 et 3000).
        let l = line(&[(0.0, 0.0), (1000.0, 0.0), (2000.0, 0.0), (1000.0, 0.0), (0.0, 0.0)]);
        // Mais le polyligne revient exactement sur lui-même : les points d'aller et de retour coïncident.
        let seams = [sp(1000.0, 0.0, 1000.0), sp(2000.0, 0.0, 2000.0), sp(1000.0, 0.0, 3000.0)];
        let p = project(&l, &seams, 15.0);
        assert_eq!(p.anomalies, 0);
        assert!((p.s[0] - 1000.0).abs() < 2.0, "{:?}", p.s);
        assert!((p.s[1] - 2000.0).abs() < 2.0);
        assert!((p.s[2] - 3000.0).abs() < 2.0, "le retour doit être trouvé au second passage : {:?}", p.s);
    }

    #[test]
    fn figure_eight_central_node_has_two_increasing_abscissas() {
        // Un 8 : le nœud central (0, 0) est à l'abscisse 0+ et à mi-parcours.
        let l = line(&[
            (-500.0, -500.0),
            (0.0, 0.0),
            (500.0, 500.0),
            (500.0, -500.0),
            (0.0, 0.0),
            (-500.0, 500.0),
            (-500.0, -500.0),
        ]);
        let half = haversine_m(pt(-500.0, -500.0), pt(0.0, 0.0))
            + haversine_m(pt(0.0, 0.0), pt(500.0, 500.0))
            + haversine_m(pt(500.0, 500.0), pt(500.0, -500.0))
            + haversine_m(pt(500.0, -500.0), pt(0.0, 0.0));
        let near = haversine_m(pt(-500.0, -500.0), pt(0.0, 0.0));
        let seams = [sp(0.0, 0.0, near), sp(0.0, 0.0, half)];
        let p = project(&l, &seams, 15.0);
        assert_eq!(p.anomalies, 0);
        assert!((p.s[0] - near).abs() < 2.0 && (p.s[1] - half).abs() < 2.0, "{:?}", p.s);
        assert!(p.s[1] > p.s[0] + 1000.0);
    }

    #[test]
    fn repeated_laps_pick_the_pass_matching_the_walk() {
        // Deux tours d'un carré de 4 × 500 m.
        let sq = [(0.0, 0.0), (500.0, 0.0), (500.0, 500.0), (0.0, 500.0), (0.0, 0.0)];
        let mut pts = sq.to_vec();
        pts.extend_from_slice(&sq[1..]);
        let l = line(&pts);
        let seams = [sp(500.0, 0.0, 500.0), sp(500.0, 0.0, 2500.0)];
        let p = project(&l, &seams, 15.0);
        assert_eq!(p.anomalies, 0);
        assert!((p.s[0] - 500.0).abs() < 2.0);
        assert!((p.s[1] - 2500.0).abs() < 2.0, "{:?}", p.s);
    }

    #[test]
    fn a_node_off_the_line_is_an_anomaly_and_s_stays_increasing() {
        let l = line(&[(0.0, 0.0), (2000.0, 0.0)]);
        let seams = [sp(1000.0, 0.0, 1000.0), sp(500.0, 400.0, 1100.0)];
        let p = project(&l, &seams, 15.0);
        assert_eq!(p.anomalies, 1);
        assert!(p.s[1] >= p.s[0]);
        assert!(p.dist_m[1] > 100.0);
    }

    #[test]
    fn a_hole_before_the_seam_widens_the_window() {
        // Le document fait 2 400 m entre deux nœuds que la marche croit distants de 600 m (un trou de 600 m de corde).
        let l = line(&[(0.0, 0.0), (2400.0, 0.0), (2400.0, 1000.0)]);
        let mut seams = [sp(0.0, 0.0, 0.0), sp(2400.0, 500.0, 600.0)];
        assert_eq!(project(&l, &seams, 15.0).anomalies, 1);
        seams[1].slack_m = 3.0 * 600.0;
        let p = project(&l, &seams, 15.0);
        assert_eq!(p.anomalies, 0);
        assert!((p.s[1] - 2900.0).abs() < 2.0, "{:?}", p.s);
    }

    #[test]
    fn max_dist_to_seam_by_hand() {
        // Coutures à 1000, 3000 et 3500 sur [0, 10000] : bouts 1000 et 6500, milieu 1000.
        let s = [1000.0, 3000.0, 3500.0];
        assert_eq!(max_dist_to_seam(&s, 0.0, 10_000.0), Some(6500.0));
        // Restreint à [500, 9500] : 6000 au bout droit.
        assert_eq!(max_dist_to_seam(&s, 500.0, 9500.0), Some(6000.0));
        // Entre deux coutures éloignées.
        assert_eq!(max_dist_to_seam(&[0.0, 4000.0], 0.0, 4000.0), Some(2000.0));
        assert_eq!(max_dist_to_seam(&[], 0.0, 1.0), None);
        assert_eq!(max_gap(&s), 2000.0);
        assert_eq!(max_gap(&[5.0]), 0.0);
    }

    #[test]
    fn doc_line_joins_parts_without_repeating_first_points() {
        let a = vec![[47.0, -1.8], [47.001, -1.8]];
        let b = vec![[47.001, -1.8], [47.002, -1.8]];
        let l = DocLine::from_parts([a, b]);
        assert_eq!(l.pts.len(), 3);
        assert!((l.len_m() - 222.4).abs() < 1.0);
    }
}
