//! Tirage des tronçons d'une trace posée (note `ord-route-lot1bis-mesures.md` §1) : fonctions pures, sans graphe.
//!
//! Un tronçon va d'une jonction `Node` de la marche à une autre, plus loin dans le sens de la marche. La **référence**
//! est la sous-marche entre les deux : les jambes `j_start + 1 ..= j_end`.

use xxhash_rust::xxh3::xxh3_64;

/// Les bandes de vol d'oiseau, m.
pub const BANDS: [(f64, f64); 3] = [(2_000.0, 5_000.0), (5_000.0, 15_000.0), (15_000.0, 40_000.0)];

/// Nom d'une bande (pour les sorties).
pub fn band_name(b: usize) -> &'static str {
    ["2-5km", "5-15km", "15-40km"].get(b).copied().unwrap_or("all")
}

/// Ce que le tirage sait d'une jonction de la marche.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JoinKind {
    /// Nœud commun, et les deux jambes voisines sont dans la vue du type : un bout de tronçon possible.
    Node,
    /// Nœud commun, mais une jambe voisine est hors de la vue : continuité sans bout possible.
    NodeOffView,
    Touch,
    /// Trou non apparié, longueur planaire (m).
    Hole(f64),
}

/// Ce que le tirage lit d'une marche.
#[derive(Debug)]
pub struct WalkView<'a> {
    /// Longueur graphe de chaque jambe, m.
    pub leg_m: &'a [f64],
    /// `joins[j]` est entre les jambes `j` et `j + 1`.
    pub joins: &'a [JoinKind],
}

/// Règles d'un tirage.
#[derive(Clone, Copy, Debug)]
pub struct DrawRules {
    /// Part maximale des trous dans la longueur de la référence.
    pub max_hole_share: f64,
    /// `L_ref / vol d'oiseau` maximal.
    pub max_detour: f64,
    /// Départs tirés au plus par bande.
    pub tries: usize,
}

impl Default for DrawRules {
    fn default() -> Self {
        Self { max_hole_share: 0.05, max_detour: 3.0, tries: 64 }
    }
}

/// Un tronçon tiré.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pick {
    pub band: usize,
    /// Jonctions de départ et d'arrivée (`j_start < j_end`).
    pub j_start: usize,
    pub j_end: usize,
    pub crow_m: f64,
    /// Longueur graphe de la sous-marche, m.
    pub l_ref_m: f64,
    /// Somme des trous de la sous-marche, m.
    pub hole_m: f64,
}

/// Un entier pseudo-aléatoire reproductible.
pub fn mix(seed: u64, a: u64, b: u64, c: u64) -> u64 {
    let mut buf = [0u8; 32];
    buf[..8].copy_from_slice(&seed.to_le_bytes());
    buf[8..16].copy_from_slice(&a.to_le_bytes());
    buf[16..24].copy_from_slice(&b.to_le_bytes());
    buf[24..].copy_from_slice(&c.to_le_bytes());
    xxh3_64(&buf)
}

/// Tire au plus un tronçon par bande de `bands`, dans l'ordre des bandes. `crow(i, k)` : vol d'oiseau (m) entre les
/// nœuds des jonctions `i` et `k`. Pour chaque bande, jusqu'à `rules.tries` départs tirés parmi les jonctions `Node` ;
/// pour un départ, l'arrivée est tirée uniformément parmi les jonctions `Node` plus loin qui tiennent les trois règles
/// (bande, détour, trous). Rien si aucun départ n'en a.
pub fn draw(
    w: &WalkView<'_>,
    crow: &dyn Fn(usize, usize) -> f64,
    bands: &[(f64, f64)],
    rules: &DrawRules,
    seed: u64,
    track_hash: u64,
) -> Vec<Pick> {
    let nodes: Vec<usize> = (0..w.joins.len()).filter(|&j| w.joins[j] == JoinKind::Node).collect();
    if nodes.len() < 2 {
        return Vec::new();
    }
    // Abscisse graphe de chaque jonction (fin de la jambe `j`) et trous cumulés jusqu'à elle (jonction comprise).
    let mut at = Vec::with_capacity(w.joins.len());
    let mut holes = Vec::with_capacity(w.joins.len());
    let (mut acc, mut hacc) = (0.0, 0.0);
    for (j, k) in w.joins.iter().enumerate() {
        acc += w.leg_m[j];
        if let JoinKind::Hole(d) = k {
            hacc += d;
        }
        at.push(acc);
        holes.push(hacc);
    }
    let mut out = Vec::new();
    for (b, &(lo, hi)) in bands.iter().enumerate() {
        for t in 0..rules.tries {
            let r = mix(seed, track_hash, b as u64, t as u64);
            let si = (r % nodes.len() as u64) as usize;
            let i = nodes[si];
            let mut cands: Vec<Pick> = Vec::new();
            for &k in &nodes[si + 1..] {
                let l_ref = at[k] - at[i];
                // Au-delà, aucun point plus loin ne tient le détour dans la bande.
                if l_ref > rules.max_detour * hi {
                    break;
                }
                let c = crow(i, k);
                if c < lo || c > hi || l_ref > rules.max_detour * c {
                    continue;
                }
                // Trous strictement à l'intérieur de la sous-marche : jonctions `i + 1 .. k`.
                let hole_m = holes[k - 1] - holes[i];
                if hole_m > rules.max_hole_share * l_ref {
                    continue;
                }
                cands.push(Pick { band: b, j_start: i, j_end: k, crow_m: c, l_ref_m: l_ref, hole_m });
            }
            if !cands.is_empty() {
                let pick = (mix(seed ^ 0x5eed, track_hash, b as u64, t as u64) % cands.len() as u64) as usize;
                out.push(cands[pick]);
                break;
            }
        }
    }
    out
}

/// Moitié d'une trace (calage sur A, jugement sur B) : parité du hachage de son identifiant.
pub fn half_of(id_hash: u64) -> char {
    if xxh3_64(&id_hash.to_le_bytes()) & 1 == 0 {
        'A'
    } else {
        'B'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une marche droite de `n` jambes de 500 m, points sur une ligne : vol d'oiseau = abscisse.
    fn straight(n: usize) -> (Vec<f64>, Vec<JoinKind>) {
        (vec![500.0; n], vec![JoinKind::Node; n - 1])
    }

    fn line_crow(i: usize, k: usize) -> f64 {
        (k as f64 - i as f64).abs() * 500.0
    }

    #[test]
    fn one_pick_per_band_inside_its_band_and_reproducible() {
        let (legs, joins) = straight(200); // 100 km
        let w = WalkView { leg_m: &legs, joins: &joins };
        let p = draw(&w, &line_crow, &BANDS, &DrawRules::default(), 7, 42);
        assert_eq!(p.len(), 3);
        for (b, pk) in p.iter().enumerate() {
            assert_eq!(pk.band, b);
            assert!(pk.j_start < pk.j_end);
            assert!(pk.crow_m >= BANDS[b].0 && pk.crow_m <= BANDS[b].1, "{pk:?}");
            assert!((pk.l_ref_m - (pk.j_end - pk.j_start) as f64 * 500.0).abs() < 1e-6);
        }
        assert_eq!(p, draw(&w, &line_crow, &BANDS, &DrawRules::default(), 7, 42));
        assert_ne!(p, draw(&w, &line_crow, &BANDS, &DrawRules::default(), 8, 42));
    }

    #[test]
    fn a_short_walk_gives_only_the_bands_it_can() {
        let (legs, joins) = straight(16); // 8 km
        let w = WalkView { leg_m: &legs, joins: &joins };
        let p = draw(&w, &line_crow, &BANDS, &DrawRules::default(), 1, 1);
        assert_eq!(p.iter().map(|p| p.band).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn holes_over_five_percent_and_round_trips_are_refused() {
        // Un trou de 400 m au milieu de chaque paire possible d'une marche de 6 km : 400 / 2 500 > 5 %.
        let mut legs = vec![500.0; 12];
        legs[0] = 500.0;
        let mut joins = vec![JoinKind::Node; 11];
        joins[5] = JoinKind::Hole(400.0);
        let w = WalkView { leg_m: &legs, joins: &joins };
        let p = draw(&w, &line_crow, &BANDS[..1], &DrawRules { tries: 500, ..DrawRules::default() }, 3, 3);
        for pk in &p {
            // Le trou n'est jamais à l'intérieur.
            assert!(!(pk.j_start < 5 && pk.j_end > 5), "{pk:?}");
            assert_eq!(pk.hole_m, 0.0);
        }
        // Un aller-retour : le vol d'oiseau revient vers 0, le détour dépasse 3.
        let legs = vec![500.0; 40];
        let joins = vec![JoinKind::Node; 39];
        let w = WalkView { leg_m: &legs, joins: &joins };
        let fold = |i: usize, k: usize| {
            let x = |j: usize| if j < 20 { j as f64 } else { 39.0 - j as f64 };
            (x(k) - x(i)).abs() * 500.0
        };
        for pk in draw(&w, &fold, &BANDS[..1], &DrawRules { tries: 500, ..DrawRules::default() }, 5, 5) {
            assert!(pk.l_ref_m <= 3.0 * pk.crow_m, "{pk:?}");
        }
    }

    #[test]
    fn only_node_joins_in_view_are_ends() {
        let legs = vec![500.0; 20];
        let mut joins = vec![JoinKind::Touch; 19];
        joins[2] = JoinKind::Node;
        joins[14] = JoinKind::NodeOffView;
        joins[11] = JoinKind::Node;
        let w = WalkView { leg_m: &legs, joins: &joins };
        let p = draw(&w, &line_crow, &BANDS, &DrawRules::default(), 9, 9);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].j_start, p[0].j_end), (2, 11));
    }
}
