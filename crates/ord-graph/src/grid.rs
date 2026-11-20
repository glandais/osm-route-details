//! Grille fixe de tuiles Web Mercator (« slippy ») : la cellule d'un point, en arithmétique pure.

/// Latitude maximale de Web Mercator.
const MAX_LAT: f64 = 85.051_128_779_806_59;

/// Tuile `(x, y)` au zoom `zoom` qui contient un point en degrés E7.
pub fn tile_of_e7(lat_e7: i32, lon_e7: i32, zoom: u8) -> (u32, u32) {
    tile_of_deg(f64::from(lat_e7) * 1e-7, f64::from(lon_e7) * 1e-7, zoom)
}

pub fn tile_of_deg(lat: f64, lon: f64, zoom: u8) -> (u32, u32) {
    let n = f64::from(1u32 << zoom);
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    let x = (lon + 180.0) / 360.0 * n;
    let y = (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n;
    let max = (1u32 << zoom) - 1;
    ((x.floor().max(0.0) as u32).min(max), (y.floor().max(0.0) as u32).min(max))
}

/// Coin nord-ouest d'une tuile, en degrés `(lat, lon)`.
pub fn tile_origin_deg(x: u32, y: u32, zoom: u8) -> (f64, f64) {
    let n = f64::from(1u32 << zoom);
    let lon = f64::from(x) / n * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * f64::from(y) / n)).sinh().atan().to_degrees();
    (lat, lon)
}

/// Plus petite dimension (m) des tuiles des rangées `y − 1` à `y + 1` : leur hauteur, ou leur largeur à leur latitude
/// la plus haute. Une polyligne partie d'une tuile de la rangée `y` ne sort de ses huit voisines qu'en traversant
/// l'une de ces tuiles : plus courte que cela, elle y reste (note `ord-loops-planete.md` §4). Seuil des arêtes plus
/// larges qu'une cellule, propre à la latitude : le plus petit de tout le graphe s'effondre près des pôles.
pub fn neighbourhood_min_tile_m(y: u32, zoom: u8) -> f64 {
    const M_PER_DEG: f64 = 6_371_008.8 * std::f64::consts::PI / 180.0;
    let last = (1u32 << zoom) - 1;
    let mut min = f64::INFINITY;
    for row in y.saturating_sub(1)..=(y + 1).min(last) {
        let (lat_hi, lon_lo) = tile_origin_deg(0, row, zoom);
        let (lat_lo, lon_hi) = tile_origin_deg(1, row + 1, zoom);
        let height = (lat_hi - lat_lo) * M_PER_DEG;
        let width = (lon_hi - lon_lo) * M_PER_DEG * lat_hi.abs().max(lat_lo.abs()).to_radians().cos();
        min = min.min(height.min(width));
    }
    min
}

/// Longueur (dm) au-delà de laquelle une arête possédée par une cellule de la rangée `y` peut sortir de ses voisines :
/// la moitié de [`neighbourhood_min_tile_m`] (marge prise).
pub fn wide_threshold_dm(y: u32, zoom: u8) -> u32 {
    (neighbourhood_min_tile_m(y, zoom) * 0.5 * 10.0).min(f64::from(u32::MAX)) as u32
}

/// Clé d'une tuile pour les index triés : `y << 16 | x` (x et y tiennent sur 16 bits jusqu'à z16).
pub fn tile_key(x: u32, y: u32) -> u32 {
    (y << 16) | x
}

/// Tuile `(x, y)` d'une clé de [`tile_key`].
pub fn tile_of_key(key: u32) -> (u32, u32) {
    (key & 0xffff, key >> 16)
}

/// Rectangle d'une tuile en degrés E7, **élargi d'une unité** vers l'extérieur : `(lat_min, lat_max, lon_min,
/// lon_max)`. Tout point E7 dont `tile_of_e7` rend `(x, y)` y est compris ; l'inverse n'est pas vrai (le bord
/// appartient à la tuile voisine), ce qui convient à un test « la tuile peut couper cette zone ».
pub fn tile_rect_e7(x: u32, y: u32, zoom: u8) -> (i32, i32, i32, i32) {
    let (lat_hi, lon_lo) = tile_origin_deg(x, y, zoom);
    let (lat_lo, lon_hi) = tile_origin_deg(x + 1, y + 1, zoom);
    let e7 = |d: f64| d * 1e7;
    (
        e7(lat_lo).floor() as i32 - 1,
        e7(lat_hi).ceil() as i32 + 1,
        e7(lon_lo).floor() as i32 - 1,
        e7(lon_hi).ceil() as i32 + 1,
    )
}

fn spread(v: u32) -> u64 {
    let mut x = u64::from(v);
    x = (x | (x << 16)) & 0x0000_ffff_0000_ffff;
    x = (x | (x << 8)) & 0x00ff_00ff_00ff_00ff;
    x = (x | (x << 4)) & 0x0f0f_0f0f_0f0f_0f0f;
    x = (x | (x << 2)) & 0x3333_3333_3333_3333;
    (x | (x << 1)) & 0x5555_5555_5555_5555
}

/// Ordre de Morton (Z) de `(x, y)` : voisins proches sur disque.
pub fn morton(x: u32, y: u32) -> u64 {
    spread(x) | (spread(y) << 1)
}

/// Morton d'une position E7, décalée en non signé.
pub fn morton_e7(lat_e7: i32, lon_e7: i32) -> u64 {
    morton((i64::from(lat_e7) + 900_000_000) as u32, (i64::from(lon_e7) + 1_800_000_000) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morton_interleaves_bits() {
        assert_eq!(morton(0, 0), 0);
        assert_eq!(morton(1, 0), 1);
        assert_eq!(morton(0, 1), 2);
        assert_eq!(morton(3, 3), 15);
        assert_eq!(morton(u32::MAX, u32::MAX), u64::MAX);
    }

    #[test]
    fn nantes_is_in_the_expected_z11_tile() {
        // Nantes ≈ 47.218 N, 1.554 W.
        assert_eq!(tile_of_deg(47.218, -1.554, 11), (1015, 718));
    }

    #[test]
    fn tile_rect_contains_its_points_and_keys_roundtrip() {
        let (x, y) = (1015, 718);
        let (la0, la1, lo0, lo1) = tile_rect_e7(x, y, 11);
        for (la, lo) in [(la0, lo0), (la1, lo1), ((la0 + la1) / 2, (lo0 + lo1) / 2), (la0 + 5, lo1 - 5)] {
            let t = tile_of_e7(la, lo, 11);
            // Les coins élargis peuvent tomber dans la tuile voisine ; le centre et l'intérieur non.
            if la != la0 && la != la1 && lo != lo0 && lo != lo1 {
                assert_eq!(t, (x, y));
            }
        }
        assert_eq!(tile_of_key(tile_key(x, y)), (x, y));
        assert!(tile_key(1, 0) < tile_key(0, 1), "tri par y puis x");
    }

    #[test]
    fn origin_is_inside_its_own_tile() {
        let (lat, lon) = tile_origin_deg(1015, 718, 11);
        assert_eq!(tile_of_deg(lat - 1e-9, lon + 1e-9, 11), (1015, 718));
    }

    #[test]
    fn tile_formula_matches_ord_domain_grid() {
        let g = ord_domain::geo::Grid::new(11, 1);
        for (lat, lon) in
            [(47.218, -1.554), (48.117, -1.677), (47.0, 0.0), (47.5, -0.5), (46.3, -2.2), (85.0, 179.9)]
        {
            let t = g.tile_of(g.to_units(ord_domain::request::LatLon::new(lat, lon)));
            assert_eq!(tile_of_deg(lat, lon, 11), (t.x, t.y), "{lat},{lon}");
        }
    }
}
