//! Couloir (T03 §4.1, T02 §8) : tuiles et cellules à moins d'un rayon de la trace.
//!
//! Repris d'osm-surface (`src/geo.rs` `corridor_tiles`, `src/network.rs` `Corridor`, Apache-2.0,
//! © Gabriel Landais).

use ord_domain::geo::{dist_units, haversine_m, lerp, Grid};
use ord_domain::tiles::TileKey;
use ord_domain::LatLon;
use rustc_hash::FxHashSet;

/// Tuiles à moins de `radius_m` de la ligne `pts`, échantillonnée tous les 100 m avec un décalage
/// 3 × 3 de ± rayon ; triées, sans doublon.
pub fn corridor_tiles(grid: Grid, pts: &[LatLon], radius_m: f64) -> Vec<TileKey> {
    let mut set = std::collections::BTreeSet::new();
    let mut add = |p: LatLon| {
        let u = grid.to_units(p);
        let r = radius_m / grid.metres_per_unit(p.lat);
        for dx in [-r, 0.0, r] {
            for dy in [-r, 0.0, r] {
                set.insert(grid.tile_of([u[0] + dx, u[1] + dy]));
            }
        }
    };
    for (i, &p) in pts.iter().enumerate() {
        add(p);
        if let Some(&q) = pts.get(i + 1) {
            let steps = (haversine_m(p, q) / 100.0).ceil() as usize;
            for k in 1..steps {
                let t = k as f64 / steps as f64;
                add(LatLon::new(p.lat + (q.lat - p.lat) * t, p.lon + (q.lon - p.lon) * t));
            }
        }
    }
    set.into_iter().collect()
}

/// Cellules de 128 unités à moins d'un rayon de la trace : ce qui est dehors ne sert jamais à
/// l'apparier.
#[derive(Debug, Default)]
pub struct Corridor {
    cells: FxHashSet<(i64, i64)>,
}

impl Corridor {
    pub const CELL_UNITS: f64 = 128.0;

    /// `points` en unités globales, avec leur latitude (échelle métrique).
    pub fn new(grid: Grid, points: &[([f64; 2], f64)], radius_m: f64) -> Self {
        let mut cells = FxHashSet::default();
        let mut mark = |p: [f64; 2], lat: f64| {
            let r = ((radius_m / grid.metres_per_unit(lat)) / Self::CELL_UNITS).ceil() as i64;
            let (cx, cy) = Self::cell(p);
            for dx in -r..=r {
                for dy in -r..=r {
                    cells.insert((cx + dx, cy + dy));
                }
            }
        };
        for (i, &(p, lat)) in points.iter().enumerate() {
            mark(p, lat);
            if let Some(&(q, lat2)) = points.get(i + 1) {
                let steps = (dist_units(p, q) / (Self::CELL_UNITS / 2.0)).ceil() as usize;
                for k in 1..steps {
                    let f = k as f64 / steps as f64;
                    mark(lerp(p, q, f), lat + (lat2 - lat) * f);
                }
            }
        }
        Self { cells }
    }

    fn cell(p: [f64; 2]) -> (i64, i64) {
        ((p[0] / Self::CELL_UNITS).floor() as i64, (p[1] / Self::CELL_UNITS).floor() as i64)
    }

    pub fn contains(&self, p: [f64; 2]) -> bool {
        self.cells.contains(&Self::cell(p))
    }

    /// Le segment touche une cellule du couloir.
    pub fn touches(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        let steps = (dist_units(a, b) / (Self::CELL_UNITS / 2.0)).ceil().max(1.0) as usize;
        (0..=steps).any(|k| self.contains(lerp(a, b, k as f64 / steps as f64)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corridor_covers_the_line_and_its_margin() {
        let grid = Grid::new(14, 4096);
        let a = LatLon::new(47.2, -1.55);
        let b = LatLon::new(47.2, -1.45);
        let tiles = corridor_tiles(grid, &[a, b], 100.0);
        assert!(tiles.windows(2).all(|w| w[0] < w[1]), "triées sans doublon");
        let ta = grid.tile_of(grid.to_units(a));
        let tb = grid.tile_of(grid.to_units(b));
        assert!(tiles.contains(&ta) && tiles.contains(&tb));
        for x in ta.x..=tb.x {
            assert!(tiles.iter().any(|t| t.x == x && t.y == ta.y), "tuile {x} traversée");
        }
        let pts: Vec<([f64; 2], f64)> = [a, b].iter().map(|p| (grid.to_units(*p), p.lat)).collect();
        let c = Corridor::new(grid, &pts, 100.0);
        let mid = lerp(pts[0].0, pts[1].0, 0.5);
        assert!(c.contains(mid));
        assert!(!c.contains([mid[0], mid[1] + 2000.0]), "à 800 m : hors du couloir");
        assert!(c.touches([mid[0], mid[1] + 2000.0], [mid[0], mid[1] - 2000.0]));
    }
}
