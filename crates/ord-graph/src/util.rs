//! Petits outils partagés : aléa SplitMix64 (tirages reproductibles) et polygone `.poly` de Geofabrik.

use std::path::Path;

/// SplitMix64, écrit ici pour qu'aucun algorithme tiers ne puisse changer les tirages.
#[derive(Clone, Debug)]
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Entier dans `0..n` (biais négligeable pour `n` ≪ 2^64).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// `k` éléments distincts de `0..n`, tirés sans remise (mélange partiel de Fisher-Yates).
pub fn sample_without_replacement(rng: &mut SplitMix64, n: usize, k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..n).collect();
    let k = k.min(n);
    for i in 0..k {
        let j = i + rng.below(n - i);
        idx.swap(i, j);
    }
    idx.truncate(k);
    idx
}

/// Erreur de lecture d'un `.poly`.
#[derive(Debug, thiserror::Error)]
pub enum PolyError {
    #[error("{0} : {1}")]
    Io(String, std::io::Error),
    #[error("{0}")]
    Syntax(String),
}

/// Polygone d'un fichier `.poly` : anneaux extérieurs et trous, en `(lon, lat)`.
#[derive(Clone, Debug, Default)]
pub struct Poly {
    outer: Vec<Vec<(f64, f64)>>,
    holes: Vec<Vec<(f64, f64)>>,
}

fn in_ring(ring: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let ((xi, yi), (xj, yj)) = (ring[i], ring[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl Poly {
    pub fn parse(text: &str) -> Result<Poly, PolyError> {
        let mut poly = Poly::default();
        let mut lines = text.lines();
        lines.next().ok_or_else(|| PolyError::Syntax("fichier .poly vide".into()))?;
        let mut current: Option<(bool, Vec<(f64, f64)>)> = None;
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "END" {
                match current.take() {
                    Some((hole, ring)) if hole => poly.holes.push(ring),
                    Some((_, ring)) => poly.outer.push(ring),
                    None => {} // « END » final du fichier
                }
            } else if current.is_none() {
                current = Some((line.starts_with('!'), Vec::new()));
            } else {
                let mut it = line.split_whitespace();
                let (Some(x), Some(y)) = (it.next(), it.next()) else {
                    return Err(PolyError::Syntax(format!("ligne .poly illisible : {line}")));
                };
                let num = |s: &str| {
                    s.parse::<f64>().map_err(|_| PolyError::Syntax(format!("nombre illisible : {s}")))
                };
                current.as_mut().expect("anneau ouvert").1.push((num(x)?, num(y)?));
            }
        }
        if poly.outer.is_empty() {
            return Err(PolyError::Syntax("aucun anneau extérieur dans le .poly".into()));
        }
        Ok(poly)
    }

    pub fn read(path: &Path) -> Result<Poly, PolyError> {
        let text = std::fs::read_to_string(path).map_err(|e| PolyError::Io(path.display().to_string(), e))?;
        Self::parse(&text)
    }

    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        self.outer.iter().any(|r| in_ring(r, lon, lat)) && !self.holes.iter().any(|r| in_ring(r, lon, lat))
    }

    /// Vrai si 32 points du cercle de rayon `radius_m` autour de `(lat, lon)` sont tous dans le polygone.
    pub fn contains_disc(&self, lat: f64, lon: f64, radius_m: f64) -> bool {
        const R: f64 = 6_371_008.8;
        (0..32).all(|k| {
            let t = f64::from(k) / 32.0 * std::f64::consts::TAU;
            let dlat = (radius_m * t.sin() / R).to_degrees();
            let dlon = (radius_m * t.cos() / (R * lat.to_radians().cos())).to_degrees();
            self.contains(lon + dlon, lat + dlat)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_is_the_reference_sequence() {
        // Vecteurs de référence de SplitMix64 pour la graine 0.
        let mut r = SplitMix64(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    }

    #[test]
    fn same_seed_same_starts_and_no_repeats() {
        let pick = |seed| sample_without_replacement(&mut SplitMix64(seed), 1000, 50);
        assert_eq!(pick(20_261_115), pick(20_261_115));
        assert_ne!(pick(20_261_115), pick(20_261_116));
        let mut p = pick(1);
        p.sort_unstable();
        p.dedup();
        assert_eq!(p.len(), 50);
        assert_eq!(sample_without_replacement(&mut SplitMix64(1), 3, 10).len(), 3);
    }

    #[test]
    fn poly_with_hole() {
        let text = "pays\n1\n   0.0 0.0\n   10.0 0.0\n   10.0 10.0\n   0.0 10.0\n   0.0 0.0\nEND\n!2\n   4.0 4.0\n   6.0 4.0\n   6.0 6.0\n   4.0 6.0\n   4.0 4.0\nEND\nEND\n";
        let p = Poly::parse(text).unwrap();
        assert!(p.contains(1.0, 1.0));
        assert!(!p.contains(5.0, 5.0));
        assert!(!p.contains(11.0, 1.0));
        assert!(Poly::parse("x\nEND\n").is_err());
    }

    #[test]
    fn disc_interior_needs_the_whole_circle() {
        let text = "t\n1\n   -2.0 47.0\n   -1.0 47.0\n   -1.0 48.0\n   -2.0 48.0\n   -2.0 47.0\nEND\nEND\n";
        let p = Poly::parse(text).unwrap();
        assert!(p.contains_disc(47.5, -1.5, 10_000.0));
        assert!(!p.contains_disc(47.5, -1.5, 80_000.0));
        assert!(!p.contains_disc(47.5, -1.02, 10_000.0));
    }
}
