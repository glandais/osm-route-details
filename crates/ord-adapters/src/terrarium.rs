//! Tuiles Terrarium en WebP de Mapterhorn (T02 §6.1) : `h = R·256 + G + B/256 − 32768`.
//!
//! Le format n'a pas de no-data : RGB `(0, 0, 0)` et toute valeur hors de
//! [`MIN_VALID_M`, `MAX_VALID_M`] sont du no-data (`NaN`).

use std::io::Cursor;

use ord_domain::tiles::{DecodeError, ElevationTile, TileCodec, TileKey, TtlClass};

/// Bornes de plausibilité [à calibrer] (T02 §6.1).
pub const MIN_VALID_M: f32 = -500.0;
pub const MAX_VALID_M: f32 = 9_000.0;

/// Décodeur Terrarium pour des tuiles carrées de `size` pixels (512 chez Mapterhorn).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrariumCodec {
    pub size: u16,
}

impl Default for TerrariumCodec {
    fn default() -> Self {
        Self { size: 512 }
    }
}

/// Altitude d'un pixel RGB ; `NaN` pour le no-data.
pub fn terrarium_value(r: u8, g: u8, b: u8) -> f32 {
    if (r, g, b) == (0, 0, 0) {
        return f32::NAN;
    }
    let h = f32::from(r) * 256.0 + f32::from(g) + f32::from(b) / 256.0 - 32_768.0;
    if (MIN_VALID_M..=MAX_VALID_M).contains(&h) {
        h
    } else {
        f32::NAN
    }
}

/// Encodage inverse (tests et terrains synthétiques) : `h` arrondi au 1/256 m.
pub fn terrarium_rgb(h: f64) -> [u8; 3] {
    let v = ((h + 32_768.0) * 256.0).round().clamp(0.0, f64::from(u32::MAX >> 8));
    let v = v as u32;
    [((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8]
}

impl TileCodec for TerrariumCodec {
    type Tile = ElevationTile;

    fn decode(&self, _id: TileKey, bytes: &[u8]) -> Result<ElevationTile, DecodeError> {
        let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))
            .map_err(|e| DecodeError::Format(format!("webp: {e}")))?;
        let (w, h) = decoder.dimensions();
        let size = u32::from(self.size);
        if (w, h) != (size, size) {
            return Err(DecodeError::Format(format!("expected {size}x{size} pixels, got {w}x{h}")));
        }
        let len = decoder
            .output_buffer_size()
            .ok_or_else(|| DecodeError::Format("webp: image too large".into()))?;
        let mut buf = vec![0u8; len];
        decoder.read_image(&mut buf).map_err(|e| DecodeError::Format(format!("webp: {e}")))?;
        let stride = if decoder.has_alpha() { 4 } else { 3 };
        let pixels = (size * size) as usize;
        if buf.len() != pixels * stride {
            return Err(DecodeError::Size(buf.len()));
        }
        let values: Box<[f32]> =
            buf.chunks_exact(stride).map(|p| terrarium_value(p[0], p[1], p[2])).collect();
        let all_valid = values.iter().all(|v| !v.is_nan());
        if values.iter().all(|v| v.is_nan()) {
            return Ok(ElevationTile::NoData);
        }
        Ok(ElevationTile::Grid { size: self.size, values, all_valid })
    }

    fn empty(&self, _id: TileKey) -> ElevationTile {
        ElevationTile::NoData
    }

    fn weight(&self, tile: &ElevationTile) -> u32 {
        match tile {
            ElevationTile::NoData => 64,
            ElevationTile::Grid { values, .. } => (values.len() * 4 + 64).min(u32::MAX as usize) as u32,
        }
    }

    fn ttl_class(&self, _tile: &ElevationTile) -> TtlClass {
        // Mapterhorn est immuable dans un espace de noms versionné (T02 §5.3).
        TtlClass::Immutable
    }
}

/// Encode une grille d'altitudes (lignes nord → sud) en WebP Terrarium sans perte.
pub fn encode_terrarium_webp(size: u16, value: impl Fn(u16, u16) -> f64) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(usize::from(size) * usize::from(size) * 3);
    for py in 0..size {
        for px in 0..size {
            rgb.extend_from_slice(&terrarium_rgb(value(px, py)));
        }
    }
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out)
        .encode(&rgb, u32::from(size), u32::from(size), image_webp::ColorType::Rgb8)
        .map(|()| out)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: TileKey = TileKey { z: 12, x: 1, y: 2 };

    #[test]
    fn formula_and_nodata() {
        assert_eq!(terrarium_value(128, 0, 0), 0.0);
        assert_eq!(terrarium_value(128, 100, 128), 100.5);
        assert!(terrarium_value(0, 0, 0).is_nan());
        assert!(terrarium_value(255, 255, 255).is_nan(), "hors bornes");
        for h in [-430.0, 0.0, 4805.25, 123.456] {
            let [r, g, b] = terrarium_rgb(h);
            assert!((f64::from(terrarium_value(r, g, b)) - h).abs() <= 1.0 / 512.0, "{h}");
        }
    }

    #[test]
    fn webp_round_trip() {
        let codec = TerrariumCodec { size: 16 };
        let bytes = encode_terrarium_webp(16, |x, y| 100.0 + f64::from(x) + 0.5 * f64::from(y));
        let tile = codec.decode(K, &bytes).expect("décodage");
        let ElevationTile::Grid { size, values, all_valid } = tile else { panic!("grille attendue") };
        assert_eq!(size, 16);
        assert!(all_valid);
        assert_eq!(values[0], 100.0);
        assert_eq!(values[16 * 3 + 2], 103.5, "ligne 3, colonne 2");
    }

    #[test]
    fn rejects_garbage_truncated_and_wrong_size() {
        let codec = TerrariumCodec { size: 16 };
        assert!(codec.decode(K, b"<html>error</html>").is_err());
        let bytes = encode_terrarium_webp(16, |_, _| 10.0);
        assert!(codec.decode(K, &bytes[..bytes.len() / 2]).is_err());
        assert!(TerrariumCodec { size: 32 }.decode(K, &bytes).is_err());
        let sea = encode_terrarium_webp(16, |_, _| -40_000.0);
        assert_eq!(codec.decode(K, &sea), Ok(ElevationTile::NoData));
    }
}
