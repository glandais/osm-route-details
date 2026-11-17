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
