//! Les formes de l'équipe au dépôt (lot 4, 4c, §15.3 et §15.9 de `ord-loops-lot4.md`) : sur chaque boucle retenue, le
//! nombre de sommets à 10 % de D (la définition figée du banc, `plane::vertices`) et, pour les triangles, le r / D.
//! Comptées par le collecteur **avant** le plafond de 500 boucles, donc sur toutes les boucles retenues.
//!
//! Elles vont dans l'en-tête JSON du profil (champ facultatif du format 1, hors de ses empreintes) : un agrégat par type,
//! jamais une boucle. Elles servent le tirage des figures du moteur (`ord_loops::FigureMix`).

use ord_loops::{FigureMix, RideType};
use serde::{Deserialize, Serialize};

use crate::plane::{far_of, vertices, Pt};

/// Une équipe pèse sur le tirage à partir de ce nombre de boucles retenues du type.
pub const MIN_LOOPS: u32 = 50;
/// ... et sa loi du r / D des triangles à partir de ce nombre de boucles à deux sommets.
pub const MIN_TRIANGLES: u32 = 20;

/// Les comptes d'un type : boucles retenues et leur répartition par nombre de sommets à 10 % de D.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeShape {
    pub loops: u32,
    /// Au plus un sommet (aller-retour, lentille), exactement deux (triangle), trois ou plus.
    pub k1: u32,
    pub k2: u32,
    pub k3: u32,
    /// P10, P25, P50, P75, P90 (type 7) du r / D des boucles à deux sommets, en millièmes ; `None` sous
    /// [`MIN_TRIANGLES`] boucles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rd_tri_milli: Option<[u16; 5]>,
}

/// Les formes d'une équipe, par type (`RideType::index()` : ROAD, GRAVEL, MTB).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeProfile {
    pub types: [TypeShape; 3],
}

impl ShapeProfile {
    /// Aucune boucle retenue.
    pub fn is_empty(&self) -> bool {
        self.types.iter().all(|t| t.loops == 0)
    }

    /// Le mélange de l'équipe pour un type : `Some` si elle a au moins [`MIN_LOOPS`] boucles du type, qu'elle en a de
    /// moins de trois sommets ou de deux (le tirage a besoin des deux), et que le type n'est pas le MTB (quatre boucles
    /// réelles : jamais de triangle). La loi du r / D est la sienne si elle a [`MIN_TRIANGLES`] triangles, sinon celle des
    /// constantes générales.
    pub fn mix(&self, ride: RideType) -> Option<FigureMix> {
        if ride == RideType::Mtb {
            return None;
        }
        let t = &self.types[ride.index()];
        if t.loops < MIN_LOOPS || t.k1 + t.k2 == 0 {
            return None;
        }
        let tri_q8 = (256.0 * f64::from(t.k2) / f64::from(t.k1 + t.k2)).round() as u16;
        let rd = t.rd_tri_milli.unwrap_or_else(|| FigureMix::general(ride).rd_tri_milli);
        Some(FigureMix { tri_q8, rd_tri_milli: rd })
    }
}

/// Le mélange du service et du banc : celui de l'équipe s'il existe, sinon le général. Sans équipe, sans profil, avec un
/// profil sans formes (écrit par 1.10) ou une équipe sous le seuil : le général.
pub fn mix_for(shapes: Option<&ShapeProfile>, ride: RideType) -> FigureMix {
    shapes.and_then(|s| s.mix(ride)).unwrap_or_else(|| FigureMix::general(ride))
}

/// L'accumulateur du collecteur.
#[derive(Debug, Default)]
pub struct ShapeAcc {
    counts: [[u32; 3]; 3],
    rd: [Vec<f64>; 3],
}

impl ShapeAcc {
    /// Compte une boucle retenue : `ty` (0 à 2) et sa polyligne plane ouverte au départ.
    pub fn add(&mut self, ty: u8, pts: &[Pt]) {
        let t = usize::from(ty).min(2);
        let k = vertices(pts, 0.10);
        self.counts[t][(k.max(1) as usize - 1).min(2)] += 1;
        if k == 2 {
            self.rd[t].push(far_of(pts).0);
        }
    }

    pub fn finish(self) -> ShapeProfile {
        let mut out = ShapeProfile::default();
        for (t, mut rd) in self.rd.into_iter().enumerate() {
            let c = self.counts[t];
            let shape = &mut out.types[t];
            (shape.k1, shape.k2, shape.k3) = (c[0], c[1], c[2]);
            shape.loops = c[0] + c[1] + c[2];
            if rd.len() as u32 >= MIN_TRIANGLES {
                rd.sort_by(f64::total_cmp);
                let q = |p: f64| {
                    let h = (rd.len() - 1) as f64 * p;
                    let (lo, hi) = (h.floor() as usize, h.ceil() as usize);
                    ((rd[lo] + (h - lo as f64) * (rd[hi] - rd[lo])) * 1000.0).round() as u16
                };
                shape.rd_tri_milli = Some([q(0.10), q(0.25), q(0.50), q(0.75), q(0.90)]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Polyligne densifiée : un point tous les 10 m au plus.
    fn dense(corners: &[Pt]) -> Vec<Pt> {
        let mut out = vec![corners[0]];
        for w in corners.windows(2) {
            let n = (crate::plane::dist(w[0], w[1]) / 10.0).ceil().max(1.0) as usize;
            for k in 1..=n {
                let t = k as f64 / n as f64;
                out.push([w[0][0] + t * (w[1][0] - w[0][0]), w[0][1] + t * (w[1][1] - w[0][1])]);
            }
        }
        out
    }

    fn triangle(r: f64) -> Vec<Pt> {
        dense(&[[0.0, 0.0], [r, 0.4 * r], [r, -0.4 * r], [0.0, 0.0]])
    }

    fn out_and_back() -> Vec<Pt> {
        dense(&[[0.0, 0.0], [5000.0, 0.0], [0.0, 0.0]])
    }

    #[test]
    fn a_triangle_counts_two_vertices_an_out_and_back_one() {
        let mut a = ShapeAcc::default();
        a.add(0, &triangle(4000.0));
        a.add(0, &out_and_back());
        a.add(1, &dense(&[[0.0, 0.0], [3000.0, 0.0], [3000.0, 3000.0], [0.0, 3000.0], [0.0, 0.0]]));
        let s = a.finish();
        assert_eq!((s.types[0].k1, s.types[0].k2, s.types[0].k3, s.types[0].loops), (1, 1, 0, 2));
        assert_eq!((s.types[1].k1, s.types[1].k2, s.types[1].k3), (0, 0, 1), "un carré : trois sommets");
        assert_eq!(s.types[2], TypeShape::default());
        assert!(s.types[0].rd_tri_milli.is_none(), "sous 20 triangles : pas de loi");
    }

    #[test]
    fn counts_do_not_depend_on_the_order_and_quantiles_come_from_enough_triangles() {
        let loops: Vec<Vec<Pt>> = (0..30)
            .map(|i| triangle(3000.0 + 100.0 * f64::from(i)))
            .chain((0..30).map(|i| dense(&[[0.0, 0.0], [4000.0 + 50.0 * f64::from(i), 0.0], [0.0, 0.0]])))
            .collect();
        let run = |order: &[usize]| {
            let mut a = ShapeAcc::default();
            for &i in order {
                a.add(0, &loops[i]);
            }
            a.finish()
        };
        let fwd: Vec<usize> = (0..loops.len()).collect();
        let rev: Vec<usize> = fwd.iter().rev().copied().collect();
        let (x, y) = (run(&fwd), run(&rev));
        assert_eq!(x, y);
        assert_eq!((x.types[0].k1, x.types[0].k2), (30, 30));
        let q = x.types[0].rd_tri_milli.expect("30 triangles");
        assert!(q.windows(2).all(|w| w[0] <= w[1]), "{q:?}");
        // r / D d'un triangle : le sommet le plus loin à r, D = périmètre.
        assert!((200..=450).contains(&q[2]), "{q:?}");
    }

    #[test]
    fn the_team_mix_needs_fifty_loops_and_falls_back_to_the_general_one() {
        let team = |loops: u32, k1: u32, k2: u32, rd: Option<[u16; 5]>| {
            let mut s = ShapeProfile::default();
            s.types[0] = TypeShape { loops, k1, k2, k3: loops - k1 - k2, rd_tri_milli: rd };
            s
        };
        let road = RideType::Road;
        // Sous le seuil, sans profil, sans formes : le général.
        assert_eq!(mix_for(None, road), FigureMix::general(road));
        assert_eq!(mix_for(Some(&ShapeProfile::default()), road), FigureMix::general(road));
        assert_eq!(mix_for(Some(&team(49, 20, 25, None)), road), FigureMix::general(road));
        // Au seuil : sa part de triangles `round(256 · k2 / (k1 + k2))`, la loi générale sans assez de triangles.
        let m = mix_for(Some(&team(50, 20, 25, None)), road);
        assert_eq!(m.tri_q8, (256.0f64 * 25.0 / 45.0).round() as u16);
        assert_eq!(m.rd_tri_milli, FigureMix::general(road).rd_tri_milli);
        // Avec sa loi.
        let own = [200, 240, 300, 340, 380];
        assert_eq!(mix_for(Some(&team(80, 30, 40, Some(own))), road).rd_tri_milli, own);
        // Le MTB, jamais.
        let mut s = team(80, 30, 40, Some(own));
        s.types[2] = s.types[0].clone();
        assert_eq!(mix_for(Some(&s), RideType::Mtb), FigureMix::general(RideType::Mtb));
        // Le type sans boucle de moins de trois sommets : le général.
        assert_eq!(mix_for(Some(&team(60, 0, 0, None)), road), FigureMix::general(road));
    }
}
