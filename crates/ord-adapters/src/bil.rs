//! Tuiles BIL32 du WMS-R de la Géoplateforme (IGN LiDAR HD, T02 §6.1) : `f32` petit-boutiste,
//! lignes nord → sud, sans en-tête, exactement `size² × 4` octets.
//!
//! Lecture stricte de la longueur (vcyclist, `Bil32.kt`) : un WMS signale ses erreurs par un 200
//! au corps XML, et accepter un corps court changerait une page d'erreur en tuile de zéros
//! plausibles. Une valeur ≤ −9000 ou `NaN` est du no-data (`−9999` hors levé, en mer).

use std::time::Duration;

use ord_domain::tiles::{DecodeError, ElevationTile, TileCodec, TileKey, TtlClass};

/// Seuil du no-data IGN (`−9999`), lâche plutôt qu'égal, comme vcyclist et `leaflet-relief`.
pub const BIL_NO_DATA_THRESHOLD: f32 = -9000.0;

/// Décodeur BIL32 de tuiles carrées de `size` pixels (256 à l'IGN).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BilCodec {
    pub size: u16,
    /// Durée de vie d'une tuile qui contient du no-data (30 j [à calibrer], T02 §5.3) : un levé
    /// peut la compléter ; une tuile entièrement valide est immuable.
    pub nodata_ttl: Duration,
}

impl Default for BilCodec {
    fn default() -> Self {
        Self { size: 256, nodata_ttl: Duration::from_secs(30 * 86_400) }
    }
}

/// Valeur d'un échantillon ; `NaN` pour le no-data.
pub fn bil_value(v: f32) -> f32 {
    if v.is_nan() || v <= BIL_NO_DATA_THRESHOLD {
        f32::NAN
    } else {
        v
    }
}

impl TileCodec for BilCodec {
    type Tile = ElevationTile;

    fn decode(&self, _id: TileKey, bytes: &[u8]) -> Result<ElevationTile, DecodeError> {
        let pixels = usize::from(self.size) * usize::from(self.size);
        if bytes.len() != pixels * 4 {
            if bytes.first() == Some(&b'<') {
                return Err(DecodeError::Format(format!(
                    "{} bytes, the body looks like an XML error page",
                    bytes.len()
                )));
            }
            return Err(DecodeError::Size(bytes.len()));
        }
        let values: Box<[f32]> =
            bytes.as_chunks::<4>().0.iter().map(|b| bil_value(f32::from_le_bytes(*b))).collect();
        if values.iter().all(|v| v.is_nan()) {
            return Ok(ElevationTile::NoData);
        }
        let all_valid = values.iter().all(|v| !v.is_nan());
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

    fn ttl_class(&self, tile: &ElevationTile) -> TtlClass {
        match tile {
            ElevationTile::Grid { all_valid: true, .. } => TtlClass::Immutable,
            _ => TtlClass::Expires(self.nodata_ttl),
        }
    }
}

/// Encode une grille (lignes nord → sud) en BIL32 ; `NaN` devient `−9999` (tests, serveurs scriptés).
pub fn encode_bil(size: u16, value: impl Fn(u16, u16) -> f64) -> Vec<u8> {
    let mut out = Vec::with_capacity(usize::from(size) * usize::from(size) * 4);
    for py in 0..size {
        for px in 0..size {
            let v = value(px, py);
            let v = if v.is_nan() { -9999.0f32 } else { v as f32 };
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: TileKey = TileKey { z: 15, x: 16_000, y: 11_000 };

    #[test]
    fn decodes_rows_north_to_south_and_nodata() {
        let codec = BilCodec { size: 4, ..BilCodec::default() };
        let bytes =
            encode_bil(
                4,
                |x, y| if (x, y) == (3, 3) { f64::NAN } else { f64::from(x) + 10.0 * f64::from(y) },
            );
        let tile = codec.decode(K, &bytes).unwrap();
        let ElevationTile::Grid { size, values, all_valid } = &tile else { panic!("grille attendue") };
        assert_eq!(*size, 4);
        assert!(!all_valid);
        assert_eq!(values[4 * 2 + 1], 21.0, "ligne 2, colonne 1");
        assert!(values[15].is_nan());
        assert_eq!(codec.ttl_class(&tile), TtlClass::Expires(codec.nodata_ttl));
        let full = codec.decode(K, &encode_bil(4, |_, _| 5.0)).unwrap();
        assert_eq!(codec.ttl_class(&full), TtlClass::Immutable);
    }

    #[test]
    fn all_nodata_is_the_nodata_variant() {
        let codec = BilCodec { size: 4, ..BilCodec::default() };
        let sea = codec.decode(K, &encode_bil(4, |_, _| f64::NAN)).unwrap();
        assert_eq!(sea, ElevationTile::NoData);
        assert_eq!(codec.ttl_class(&sea), TtlClass::Expires(codec.nodata_ttl));
        assert!(bil_value(-99_999.0).is_nan());
        assert_eq!(bil_value(-8_999.0), -8_999.0);
    }

    #[test]
    fn strict_length_and_xml_error_page() {
        let codec = BilCodec { size: 4, ..BilCodec::default() };
        let bytes = encode_bil(4, |_, _| 1.0);
        assert_eq!(codec.decode(K, &bytes[..60]), Err(DecodeError::Size(60)));
        let mut long = bytes.clone();
        long.push(0);
        assert!(codec.decode(K, &long).is_err());
        let err = codec.decode(K, b"<?xml version=\"1.0\"?><ServiceExceptionReport/>").unwrap_err();
        assert!(err.to_string().contains("XML"), "{err}");
    }
}
