//! Mesures pures sur polylignes planes (mètres locaux) et sur échantillons : forme, sommets, virages, quantiles,
//! Wasserstein. **Aucune dépendance au graphe** ; tout est testé sur des figures synthétiques (§11.5 et §11.7 de
//! `docs/design/technique/ord-loops-lot4.md`).

pub use ord_graph_match::plane::{
    dist, douglas_peucker, far_of, heading_deg, length, point_at, vertex_indices, vertices, Pt,
};

/// Forme d'une trace ouverte au départ, `pts[0]` étant le départ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Longueur de la polyligne, m (la D des mesures de forme).
    pub l_poly: f64,
    /// Distance maximale au départ, sur les sommets, divisée par D.
    pub r_over_d: f64,
    /// Abscisse du premier sommet de distance maximale, divisée par D.
    pub far_pos: f64,
    /// `4π A / D²`, `A` l'aire de Gauss du polygone fermé.
    pub compactness: f64,
    pub vertices: u32,
    pub vertices_10: u32,
    pub vertices_legacy: u32,
    /// Détour jusqu'au point le plus éloigné : `far_pos / r_over_d` (abscisse de ce point sur sa distance à vol
    /// d'oiseau du départ) ; `None` si `r_over_d` vaut 0. Sonde du goût, §12.4.
    pub detour_far: Option<f64>,
    /// `l_poly / P`, `P` le périmètre du polygone **fermé** des points gardés par la règle de `vertices_10` (départ,
    /// sommets, arrivée, puis retour au départ) ; `None` si `P` vaut 0. Sonde du goût, §12.4.
    pub sinuosity_10: Option<f64>,
}

/// Mesure la forme de `pts` (au moins deux points ; sinon tout vaut 0).
pub fn shape(pts: &[Pt]) -> Shape {
    let l = length(pts);
    if pts.len() < 2 || l <= 0.0 {
        return Shape {
            l_poly: l,
            r_over_d: 0.0,
            far_pos: 0.0,
            compactness: 0.0,
            vertices: 0,
            vertices_10: 0,
            vertices_legacy: 0,
            detour_far: None,
            sinuosity_10: None,
        };
    }
    let (r_over_d, far_pos) = far_of(pts);
    let a = gauss_area(pts).abs();
    let kept = vertex_indices(pts, 0.10);
    let perimeter = if kept.len() >= 2 {
        let ring: Vec<Pt> = kept.iter().map(|&i| pts[i]).collect();
        length(&ring) + dist(ring[ring.len() - 1], ring[0])
    } else {
        // Aucun sommet au-delà de la tolérance : le polygone se réduit au segment départ-arrivée, aller et retour.
        2.0 * dist(pts[0], pts[pts.len() - 1])
    };
    Shape {
        l_poly: l,
        r_over_d,
        far_pos,
        compactness: 4.0 * std::f64::consts::PI * a / (l * l),
        vertices: vertices(pts, 0.05),
        vertices_10: vertices(pts, 0.10),
        vertices_legacy: vertices_legacy(pts, 0.05),
        detour_far: (r_over_d > 0.0).then(|| far_pos / r_over_d),
        sinuosity_10: (perimeter > 0.0).then(|| l / perimeter),
    }
}

/// Aire signée (lacet) du polygone `pts` **fermé** par le segment dernier → premier.
pub fn gauss_area(pts: &[Pt]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        s += a[0] * b[1] - b[0] * a[1];
    }
    s / 2.0
}

/// Distance d'un point à la **droite** `a b` ; droite dégénérée : distance au point (celle de `shapes.py`).
fn dist_to_line(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dy);
    if l > 0.0 {
        ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / l
    } else {
        dist(p, a)
    }
}

/// La règle de `shapes.py` (`dp`) portée exactement : corde `p[0] → p[n − 1]`, distance à la **droite** (au point si
/// la corde est nulle), `len(keep) − 2`. Sur une boucle ouverte d'un écart non nul, la première coupe prend le point
/// le plus loin de la droite départ–arrivée, pas du départ.
pub fn vertices_legacy(pts: &[Pt], tol_share: f64) -> u32 {
    let n = pts.len();
    if n < 2 {
        return 0;
    }
    let tol = tol_share * length(pts);
    let keep = douglas_peucker(pts, &[0, n - 1], tol, dist_to_line);
    (keep.len() as u32).saturating_sub(2)
}

/// Écart entre deux caps, ramené à `[0, 180]`.
pub fn angle_between(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// **Virages** : pour chaque jonction `(abscisse, degré)` de degré au moins `min_deg`, cap d'entrée = cap du vecteur
/// `P(s − window) → P(s)`, cap de sortie = `P(s) → P(s + window)` (`P` interpolé, abscisse bornée à `[0, L]`) ;
/// compté si l'angle entre les deux dépasse `min_angle` degrés. Le départ et l'arrivée (abscisse en dehors de
/// `]0, L[`) ne comptent pas.
pub fn turns(
    pts: &[Pt],
    cum: &[f64],
    junctions: &[(f64, u8)],
    min_deg: u8,
    min_angle: f64,
    window_m: f64,
) -> u32 {
    let total = *cum.last().unwrap_or(&0.0);
    let mut n = 0;
    for &(s, deg) in junctions {
        if deg < min_deg || s <= 1e-6 || s >= total - 1e-6 {
            continue;
        }
        let p = point_at(pts, cum, s);
        let before = point_at(pts, cum, s - window_m);
        let after = point_at(pts, cum, s + window_m);
        if dist(before, p) < 1e-6 || dist(p, after) < 1e-6 {
            continue;
        }
        if angle_between(heading_deg(before, p), heading_deg(p, after)) > min_angle {
            n += 1;
        }
    }
    n
}

// --- Étalonnage : `shapes.py` en entier -------------------------------------------------------------------------

/// Une trace vue comme `shapes.py` la voit (`raw_legacy`), sans graphe : la concaténation des géométries brutes de
/// l'export.
#[derive(Clone, Copy, Debug)]
pub struct RawRow {
    pub d: f64,
    pub gap: f64,
    pub rmax: f64,
    /// Abscisse du point le plus loin, divisée par D, depuis le point 1.
    pub fr: f64,
    pub area: f64,
    pub corners: i64,
}

/// `shapes.py`, une trace : `pts` = géométries brutes concaténées (`pts += g if not pts else g[1:]`, `(lat, lon)` en
/// degrés), `tot` = somme des `trv` (absent : 0). Rejet si `len(pts) < 50` ou `tot < 5000` ; projection
/// `((lon − lon0) × 111320 × cos lat0, (lat − lat0) × 110540)`, un point sur trois (`[::3]`) ; l'aire est celle de
/// `shapes.py` (sans terme de fermeture : le départ est l'origine).
pub fn raw_legacy_row(pts: &[[f64; 2]], tot: f64) -> Option<RawRow> {
    if pts.len() < 50 || tot < 5000.0 {
        return None;
    }
    let (la0, lo0) = (pts[0][0], pts[0][1]);
    let c = la0.to_radians().cos();
    let p: Vec<Pt> =
        pts.iter().map(|q| [(q[1] - lo0) * 111_320.0 * c, (q[0] - la0) * 110_540.0]).step_by(3).collect();
    if p.len() < 2 {
        return None;
    }
    let d: f64 = p.windows(2).map(|w| dist(w[0], w[1])).sum();
    let gap = dist(p[0], p[p.len() - 1]);
    let (mut cum, mut rmax, mut fr) = (0.0, 0.0, 0.0);
    for i in 1..p.len() {
        cum += dist(p[i - 1], p[i]);
        let r = p[i][0].hypot(p[i][1]);
        if r > rmax {
            rmax = r;
            fr = cum / d;
        }
    }
    let area =
        (0..p.len() - 1).map(|i| p[i][0] * p[i + 1][1] - p[i + 1][0] * p[i][1]).sum::<f64>().abs() / 2.0;
    let corners = vertices_legacy(&p, 0.05) as i64;
    Some(RawRow { d, gap, rmax, fr, area, corners })
}

/// Quantile de `shapes.py` : `v[min(n − 1, int(n × p / 100))]` sur l'échantillon trié, au millième.
fn legacy_q(v: &[f64]) -> String {
    let s = sorted(v);
    if s.is_empty() {
        return "-".into();
    }
    [10usize, 25, 50, 75, 90]
        .iter()
        .map(|p| format!("{:.3}", s[(s.len() - 1).min(s.len() * p / 100)]))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Les lignes de `shapes.py`, au même texte et aux mêmes arrondis (`diff` avec sa sortie), moins la ligne
/// « unpaved share » (elle lit `sc`, absent de `ParsedEdge`).
pub fn legacy_lines(rows: &[RawRow]) -> Vec<String> {
    let loops: Vec<&RawRow> = rows.iter().filter(|r| r.gap < 0.05 * r.d).collect();
    let col = |f: &dyn Fn(&RawRow) -> f64| loops.iter().map(|r| f(r)).collect::<Vec<_>>();
    let mut out = vec![
        format!("tracks {} loops {}", rows.len(), loops.len()),
        format!("rmax/D p10..p90 {}", legacy_q(&col(&|r| r.rmax / r.d))),
        format!("farthest position along route {}", legacy_q(&col(&|r| r.fr))),
        format!(
            "isoperimetric 4piA/D^2 {}",
            legacy_q(&col(&|r| 4.0 * std::f64::consts::PI * r.area / (r.d * r.d)))
        ),
    ];
    let mut hist: std::collections::BTreeMap<i64, usize> = std::collections::BTreeMap::new();
    for r in &loops {
        *hist.entry(r.corners.min(6)).or_default() += 1;
    }
    out.push(format!(
        "corners (DP 5% D) histogram {{{}}}",
        hist.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ")
    ));
    for (lo, hi) in [(0.0, 40e3), (40e3, 70e3), (70e3, 100e3), (100e3, 150e3), (150e3, 1e9)] {
        let s: Vec<&&RawRow> = loops.iter().filter(|r| lo <= r.d && r.d < hi).collect();
        if s.is_empty() {
            continue;
        }
        let med =
            |f: &dyn Fn(&RawRow) -> f64| median(&s.iter().map(|r| f(r)).collect::<Vec<_>>()).unwrap_or(0.0);
        out.push(format!(
            "D {:.0}-{:.0} km n={} rmax/D med {:.3} iso med {:.3}",
            lo / 1e3,
            hi / 1e3,
            s.len(),
            med(&|r| r.rmax / r.d),
            med(&|r| 4.0 * std::f64::consts::PI * r.area / (r.d * r.d)),
        ));
    }
    out
}

// --- Distributions ---------------------------------------------------------------------------------------------

/// Quantile `p` ∈ [0, 1] par interpolation linéaire (`h = (n − 1) p`, type 7) d'un échantillon **trié**.
pub fn quantile_sorted(v: &[f64], p: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let h = (v.len() - 1) as f64 * p;
    let (lo, hi) = (h.floor() as usize, h.ceil() as usize);
    Some(v[lo] + (h - lo as f64) * (v[hi] - v[lo]))
}

/// Les valeurs de `v` triées (sans NaN).
pub fn sorted(v: &[f64]) -> Vec<f64> {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| !x.is_nan()).collect();
    s.sort_by(f64::total_cmp);
    s
}

/// P10, P25, P50, P75, P90.
pub const QUANTILES: [f64; 5] = [0.10, 0.25, 0.50, 0.75, 0.90];

pub fn quantiles5(v: &[f64]) -> Option<[f64; 5]> {
    let s = sorted(v);
    let mut out = [0.0; 5];
    for (o, p) in out.iter_mut().zip(QUANTILES) {
        *o = quantile_sorted(&s, p)?;
    }
    Some(out)
}

pub fn median(v: &[f64]) -> Option<f64> {
    quantile_sorted(&sorted(v), 0.5)
}

/// Écart interquartile (P75 − P25).
pub fn iqr(v: &[f64]) -> Option<f64> {
    let s = sorted(v);
    Some(quantile_sorted(&s, 0.75)? - quantile_sorted(&s, 0.25)?)
}

/// **Wasserstein-1** entre deux échantillons à une dimension de tailles quelconques :
/// `W1 = ∫ |F_a(x) − F_b(x)| dx`, exacte, par parcours fusionné des points de rupture sur les deux échantillons
/// triés. `None` si l'un est vide.
pub fn wasserstein1(a: &[f64], b: &[f64]) -> Option<f64> {
    let (a, b) = (sorted(a), sorted(b));
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let (na, nb) = (a.len() as f64, b.len() as f64);
    let (mut i, mut j) = (0usize, 0usize);
    let mut x = a[0].min(b[0]);
    let mut w = 0.0;
    while i < a.len() || j < b.len() {
        // Avance sur tous les points égaux à `x`.
        while i < a.len() && a[i] <= x {
            i += 1;
        }
        while j < b.len() && b[j] <= x {
            j += 1;
        }
        let next = match (a.get(i), b.get(j)) {
            (Some(&p), Some(&q)) => p.min(q),
            (Some(&p), None) => p,
            (None, Some(&q)) => q,
            (None, None) => break,
        };
        w += (i as f64 / na - j as f64 / nb).abs() * (next - x);
        x = next;
    }
    Some(w)
}

/// W1 divisée par l'écart interquartile du réel ; `None` si cet écart est nul (la W1 brute reste donnée à côté).
pub fn wasserstein1_normalised(real: &[f64], generated: &[f64]) -> Option<f64> {
    let (w, q) = (wasserstein1(real, generated)?, iqr(real)?);
    (q > 0.0).then_some(w / q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ord_graph_match::plane::cumulative;

    /// Polyligne densifiée : un point tous les 10 m au plus, les sommets gardés.
    fn dense(corners: &[Pt]) -> Vec<Pt> {
        let mut out = vec![corners[0]];
        for w in corners.windows(2) {
            let n = (dist(w[0], w[1]) / 10.0).ceil().max(1.0) as usize;
            for k in 1..=n {
                let t = k as f64 / n as f64;
                out.push([w[0][0] + t * (w[1][0] - w[0][0]), w[0][1] + t * (w[1][1] - w[0][1])]);
            }
        }
        out
    }

    /// Cercle de rayon `r` passant par l'origine (centre en `(r, 0)`), parcouru une fois.
    fn circle(r: f64) -> Vec<Pt> {
        let n = ((2.0 * std::f64::consts::PI * r) / 10.0).ceil() as usize;
        (0..=n)
            .map(|k| {
                let a = std::f64::consts::PI + 2.0 * std::f64::consts::PI * k as f64 / n as f64;
                [r + r * a.cos(), r * a.sin()]
            })
            .collect()
    }

    /// Lentille de corde `c` et de flèche `s` : deux arcs paraboliques entre `(0, 0)` et `(c, 0)`.
    fn lens(c: f64, s: f64) -> Vec<Pt> {
        let arc = |sign: f64, rev: bool| {
            let n = 400;
            let mut v: Vec<Pt> = (0..=n)
                .map(|k| {
                    let t = k as f64 / n as f64;
                    [c * t, sign * 4.0 * s * t * (1.0 - t)]
                })
                .collect();
            if rev {
                v.reverse();
            }
            v
        };
        let mut out = arc(1.0, false);
        out.extend(arc(-1.0, true).into_iter().skip(1));
        out
    }

    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} != {b} (±{tol})");
    }

    #[test]
    fn a_circle_has_r_over_d_one_over_pi_compactness_one_and_three_vertices() {
        let s = shape(&circle(5_000.0));
        close(s.r_over_d, 1.0 / std::f64::consts::PI, 2e-3);
        close(s.compactness, 1.0, 5e-3);
        close(s.far_pos, 0.5, 5e-3);
        assert_eq!((s.vertices, s.vertices_10), (3, 3));
    }

    #[test]
    fn detour_far_and_sinuosity_on_the_reference_shapes() {
        let ab = shape(&dense(&[[0.0, 0.0], [0.0, 8_000.0], [0.0, 0.0]]));
        close(ab.detour_far.unwrap(), 1.0, 1e-9);
        close(ab.sinuosity_10.unwrap(), 1.0, 1e-9);
        // Cercle : far_pos 1/2 sur r/D 1/π ; l'aire inscrite est un carré (3 sommets) de périmètre 4 √2 r.
        let c = shape(&circle(5_000.0));
        close(c.detour_far.unwrap(), std::f64::consts::PI / 2.0, 1e-2);
        assert_eq!(c.vertices_10, 3);
        close(c.sinuosity_10.unwrap(), 2.0 * std::f64::consts::PI / (4.0 * 2f64.sqrt()), 1e-2);
        let tri = shape(&dense(&[[0.0, 0.0], [8_000.0, 0.0], [4_000.0, 6_000.0], [0.0, 0.0]]));
        close(tri.detour_far.unwrap(), 1.0, 1e-9);
        close(tri.sinuosity_10.unwrap(), 1.0, 1e-9);
        // Dégénéré : rien ne bouge, rien à rendre.
        let dot = shape(&[[0.0, 0.0], [0.0, 0.0]]);
        assert_eq!((dot.detour_far, dot.sinuosity_10), (None, None));
    }

    #[test]
    fn the_kept_indices_are_the_vertices_plus_two() {
        let a = 5_000.0;
        let shapes = [
            circle(5_000.0),
            dense(&[[0.0, 0.0], [0.0, 8_000.0], [0.0, 0.0]]),
            dense(&[[0.0, 0.0], [8_000.0, 0.0], [4_000.0, 6_000.0], [0.0, 0.0]]),
            dense(&[[0.0, 0.0], [a, 0.0], [a, a], [0.0, a], [0.0, 0.0]]),
            lens(10_000.0, 300.0),
            lens(10_000.0, 1_500.0),
            dense(&[[0.0, 0.0], [0.0, 8_000.0], [0.0, -1.0]]),
            vec![[0.0, 0.0], [1.0, 0.0]],
        ];
        for p in &shapes {
            for tol in [0.05, 0.10] {
                let idx = vertex_indices(p, tol);
                assert!(idx.windows(2).all(|w| w[0] < w[1]));
                assert_eq!(idx.len().saturating_sub(2) as u32, vertices(p, tol));
            }
        }
    }

    #[test]
    fn an_out_and_back_has_half_radius_no_area_and_one_vertex() {
        let s = shape(&dense(&[[0.0, 0.0], [0.0, 8_000.0], [0.0, 0.0]]));
        close(s.r_over_d, 0.5, 1e-9);
        close(s.compactness, 0.0, 1e-9);
        close(s.far_pos, 0.5, 1e-9);
        assert_eq!(s.vertices, 1);
    }

    #[test]
    fn a_triangle_has_two_vertices_and_the_expected_area() {
        let pts = dense(&[[0.0, 0.0], [8_000.0, 0.0], [4_000.0, 6_000.0], [0.0, 0.0]]);
        let s = shape(&pts);
        let d = 8_000.0 + 2.0 * 7_211.102_550_927_978;
        close(s.l_poly, d, 1e-6);
        close(s.r_over_d, 8_000.0 / d, 1e-9);
        close(s.far_pos, 8_000.0 / d, 1e-9);
        close(s.compactness, 4.0 * std::f64::consts::PI * 24.0e6 / (d * d), 1e-9);
        assert_eq!(s.vertices, 2);
    }

    #[test]
    fn a_square_from_a_corner_has_three_vertices() {
        let a = 5_000.0;
        let pts = dense(&[[0.0, 0.0], [a, 0.0], [a, a], [0.0, a], [0.0, 0.0]]);
        assert_eq!(shape(&pts).vertices, 3);
    }

    #[test]
    fn a_lens_counts_one_vertex_plus_one_per_side_above_the_tolerance() {
        let thin = lens(10_000.0, 300.0);
        assert_eq!((vertices(&thin, 0.05), vertices(&thin, 0.10)), (1, 1));
        let fat = lens(10_000.0, 1_500.0);
        assert_eq!((vertices(&fat, 0.05), vertices(&fat, 0.10)), (3, 1));
    }

    #[test]
    fn the_legacy_rule_cuts_on_the_start_to_end_chord() {
        // Aller-retour vers le nord, terminé 1 m au sud du départ : la corde est la droite du trajet.
        let pts = dense(&[[0.0, 0.0], [0.0, 8_000.0], [0.0, -1.0]]);
        assert_eq!(vertices_legacy(&pts, 0.05), 0);
        assert_eq!(vertices(&pts, 0.05), 1);
    }

    fn straight(len: f64) -> (Vec<Pt>, Vec<f64>) {
        let pts = dense(&[[0.0, 0.0], [len, 0.0]]);
        let cum = cumulative(&pts);
        (pts, cum)
    }

    #[test]
    fn turns_count_a_right_angle_at_a_junction_of_degree_three_only() {
        // Est 500 m puis nord 500 m : angle droit à l'abscisse 500.
        let pts = dense(&[[0.0, 0.0], [500.0, 0.0], [500.0, 500.0]]);
        let cum = cumulative(&pts);
        assert_eq!(turns(&pts, &cum, &[(500.0, 3)], 3, 45.0, 25.0), 1);
        assert_eq!(turns(&pts, &cum, &[(500.0, 4)], 3, 45.0, 25.0), 1);
        assert_eq!(
            turns(&pts, &cum, &[(500.0, 2)], 3, 45.0, 25.0),
            0,
            "degré 2 : un coude, pas un carrefour"
        );
        // Un coude de 30° ne compte pas.
        let a = 30.0f64.to_radians();
        let bend = dense(&[[0.0, 0.0], [500.0, 0.0], [500.0 + 500.0 * a.cos(), 500.0 * a.sin()]]);
        let cum = cumulative(&bend);
        assert_eq!(turns(&bend, &cum, &[(500.0, 3)], 3, 45.0, 25.0), 0);
        // Le départ et l'arrivée ne comptent pas, même à un nœud de degré 3.
        let (line, cum) = straight(1_000.0);
        assert_eq!(turns(&line, &cum, &[(0.0, 3), (1_000.0, 3)], 3, 45.0, 25.0), 0);
        let cum = cumulative(&pts);
        assert_eq!(turns(&pts, &cum, &[(0.0, 3), (1_000.0, 3)], 3, 45.0, 25.0), 0);
    }

    #[test]
    fn quantiles_interpolate_linearly() {
        let q = quantiles5(&[4.0, 1.0, 3.0, 2.0]).unwrap();
        close(q[2], 2.5, 1e-12);
        close(q[0], 1.3, 1e-12);
        close(q[1], 1.75, 1e-12);
        assert!(quantiles5(&[]).is_none());
        assert_eq!(median(&[5.0]), Some(5.0));
    }

    #[test]
    fn wasserstein_matches_the_known_cases() {
        let a = [0.0, 1.0, 2.0, 3.0];
        close(wasserstein1(&a, &a).unwrap(), 0.0, 1e-12);
        let shifted: Vec<f64> = a.iter().map(|x| x + 0.7).collect();
        close(wasserstein1(&a, &shifted).unwrap(), 0.7, 1e-12);
        close(wasserstein1(&[0.0, 1.0], &[0.0, 0.0, 1.0, 1.0]).unwrap(), 0.0, 1e-12);
        close(wasserstein1(&[0.0], &[1.0, 2.0]).unwrap(), 1.5, 1e-12);
        assert!(wasserstein1(&[], &[1.0]).is_none());
        // Normalisée par l'écart interquartile du réel (ici P75 − P25 = 1,5), nul : `None`.
        let real = [1.0, 2.0, 3.0, 4.0];
        let gen = [2.0, 3.0, 4.0, 5.0];
        close(wasserstein1_normalised(&real, &gen).unwrap(), 1.0 / 1.5, 1e-12);
        assert!(wasserstein1_normalised(&[2.0, 2.0, 2.0], &[1.0, 5.0]).is_none());
    }

    #[test]
    fn the_legacy_calibration_rejects_short_traces_and_prints_shapes_py_lines() {
        // 200 points d'un aller-retour de 10 km au nord, en degrés.
        let lat = |m: f64| 47.0 + m / 110_540.0;
        let mut pts: Vec<[f64; 2]> = (0..100).map(|k| [lat(100.0 * f64::from(k)), -1.5]).collect();
        pts.extend((0..100).rev().map(|k| [lat(100.0 * f64::from(k)), -1.5]));
        assert!(raw_legacy_row(&pts, 4_999.0).is_none());
        assert!(raw_legacy_row(&pts[..40], 20_000.0).is_none());
        let r = raw_legacy_row(&pts, 19_800.0).unwrap();
        close(r.rmax / r.d, 0.5, 2e-2);
        let lines = legacy_lines(&[r]);
        assert_eq!(lines[0], "tracks 1 loops 1");
        assert!(lines[1].starts_with("rmax/D p10..p90 0.5"), "{}", lines[1]);
        assert!(lines[4].starts_with("corners (DP 5% D) histogram {"), "{}", lines[4]);
    }
}
