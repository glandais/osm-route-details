//! Géométrie plane en mètres locaux : les quelques fonctions sur polylignes que la marche et la retenue des boucles
//! partagent avec le banc (`shape.rs` du banc les réexporte). Aucune dépendance au graphe.

/// Un point en mètres locaux.
pub type Pt = [f64; 2];

pub fn dist(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Longueur d'une polyligne.
pub fn length(pts: &[Pt]) -> f64 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Abscisses cumulées (la première vaut 0).
pub fn cumulative(pts: &[Pt]) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len());
    let mut acc = 0.0;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            acc += dist(pts[i - 1], *p);
        }
        out.push(acc);
    }
    out
}

/// Point d'une polyligne à l'abscisse `s` (bornée à `[0, L]`).
pub fn point_at(pts: &[Pt], cum: &[f64], s: f64) -> Pt {
    let total = *cum.last().unwrap_or(&0.0);
    let s = s.clamp(0.0, total);
    let i = cum.partition_point(|&c| c < s).clamp(1, pts.len().saturating_sub(1).max(1));
    if pts.len() < 2 {
        return pts.first().copied().unwrap_or([0.0, 0.0]);
    }
    let (c0, c1) = (cum[i - 1], cum[i]);
    let t = if c1 > c0 { (s - c0) / (c1 - c0) } else { 0.0 };
    [pts[i - 1][0] + t * (pts[i][0] - pts[i - 1][0]), pts[i - 1][1] + t * (pts[i][1] - pts[i - 1][1])]
}

/// Centre des segments pondéré par leur longueur.
pub fn barycentre(pts: &[Pt]) -> Pt {
    let (mut sx, mut sy, mut sl) = (0.0, 0.0, 0.0);
    for w in pts.windows(2) {
        let l = dist(w[0], w[1]);
        sx += l * (w[0][0] + w[1][0]) / 2.0;
        sy += l * (w[0][1] + w[1][1]) / 2.0;
        sl += l;
    }
    if sl > 0.0 {
        [sx / sl, sy / sl]
    } else {
        pts.first().copied().unwrap_or([0.0, 0.0])
    }
}

/// Cap (degrés depuis le nord, dans le sens des aiguilles d'une montre) du vecteur `a → b`.
pub fn heading_deg(a: Pt, b: Pt) -> f64 {
    let h = (b[0] - a[0]).atan2(b[1] - a[1]).to_degrees();
    h.rem_euclid(360.0)
}

/// Distance d'un point au segment `a b` (projection bornée) ; segment dégénéré : distance au point.
pub fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    if l2 <= 0.0 {
        return dist(p, a);
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
    dist(p, [a[0] + t * dx, a[1] + t * dy])
}

/// Douglas-Peucker itératif (pile) sur les indices `a..b` : rend les indices gardés, triés. La première coupe de
/// chaque morceau est le point le plus loin ; le premier maximum strict gagne.
pub fn douglas_peucker(
    pts: &[Pt],
    seeds: &[usize],
    tol: f64,
    far: impl Fn(Pt, Pt, Pt) -> f64,
) -> std::collections::BTreeSet<usize> {
    let mut keep: std::collections::BTreeSet<usize> = seeds.iter().copied().collect();
    let mut stack: Vec<(usize, usize)> = seeds.windows(2).map(|w| (w[0], w[1])).collect();
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (mut best, mut bi) = (-1.0f64, a);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = far(*p, pts[a], pts[b]);
            if d > best {
                best = d;
                bi = i;
            }
        }
        if best > tol {
            keep.insert(bi);
            stack.push((a, bi));
            stack.push((bi, b));
        }
    }
    keep
}

/// **Sommets d'une trace, la définition figée** (§11.5.3). Sur la polyligne **ouverte au départ** `p[0..n]` (arrivée
/// non refermée) et `tol = tol_share × D` (`D` = longueur de la polyligne ; 0,05 pour `vertices`, 0,10 pour
/// `vertices_10`) :
/// 1. `f` = indice du sommet le plus éloigné du départ (premier maximum strict) ; si `|p[f]| < tol`, 0 sommet ;
/// 2. garder `{0, f, n − 1}`, puis Douglas-Peucker récursif sur `[0, f]` et `[f, n − 1]`, avec la distance d'un point
///    **au segment** (projection bornée ; segment dégénéré : distance au point), coupe si `distance > tol` ;
/// 3. `vertices = |gardés| − 2` (le départ et son double ne comptent pas).
///
/// Aller-retour = 1, triangle = 2, quadrilatère partant d'un coin = 3 ; cercle = 3 ; lentille = 1 + un par côté
/// dont la flèche dépasse `tol`.
pub fn vertices(pts: &[Pt], tol_share: f64) -> u32 {
    (vertex_indices(pts, tol_share).len() as u32).saturating_sub(2)
}

/// Les indices gardés par la règle de [`vertices`] (le départ, les sommets, l'arrivée), triés ; vide si la règle ne
/// garde rien (moins de trois points, ou point le plus éloigné sous la tolérance). `vertices = len − 2`.
pub fn vertex_indices(pts: &[Pt], tol_share: f64) -> Vec<usize> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let tol = tol_share * length(pts);
    let o = pts[0];
    let (mut rmax, mut f) = (0.0f64, 0usize);
    for (i, p) in pts.iter().enumerate().skip(1) {
        let d = dist(*p, o);
        if d > rmax {
            rmax = d;
            f = i;
        }
    }
    if rmax < tol || f == 0 {
        return Vec::new();
    }
    let mut seeds = vec![0, f, n - 1];
    seeds.dedup();
    douglas_peucker(pts, &seeds, tol, dist_to_segment).into_iter().collect()
}

/// Éloignement maximal d'une trace ouverte au départ : `(r / D, abscisse du premier sommet le plus éloigné / D)`, `D`
/// la longueur de la polyligne ; `(0, 0)` si elle a moins de deux points ou une longueur nulle. Le premier maximum
/// strict gagne (celui de `Shape::r_over_d` et `Shape::far_pos` du banc).
pub fn far_of(pts: &[Pt]) -> (f64, f64) {
    let l = length(pts);
    if pts.len() < 2 || l <= 0.0 {
        return (0.0, 0.0);
    }
    let cum = cumulative(pts);
    let o = pts[0];
    let (mut rmax, mut at) = (0.0f64, 0usize);
    for (i, p) in pts.iter().enumerate().skip(1) {
        let d = dist(*p, o);
        if d > rmax {
            rmax = d;
            at = i;
        }
    }
    (rmax / l, cum[at] / l)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn an_out_and_back_has_one_vertex_and_a_triangle_two() {
        let ab = dense(&[[0.0, 0.0], [5000.0, 0.0], [0.0, 0.0]]);
        assert_eq!(vertices(&ab, 0.10), 1);
        let tri = dense(&[[0.0, 0.0], [4000.0, 1500.0], [4000.0, -1500.0], [0.0, 0.0]]);
        assert_eq!(vertices(&tri, 0.10), 2);
        assert_eq!(vertex_indices(&tri, 0.10).len(), 4);
    }

    #[test]
    fn a_square_from_a_corner_has_three_vertices() {
        let sq = dense(&[[0.0, 0.0], [3000.0, 0.0], [3000.0, 3000.0], [0.0, 3000.0], [0.0, 0.0]]);
        assert_eq!(vertices(&sq, 0.10), 3);
    }

    #[test]
    fn far_of_reads_the_farthest_point_and_its_abscissa() {
        let ab = dense(&[[0.0, 0.0], [5000.0, 0.0], [0.0, 0.0]]);
        let (r, pos) = far_of(&ab);
        assert!((r - 0.5).abs() < 1e-9 && (pos - 0.5).abs() < 1e-9, "{r} {pos}");
        assert_eq!(far_of(&[[0.0, 0.0]]), (0.0, 0.0));
    }
}
