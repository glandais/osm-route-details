//! Décodage MVT générique (vector_tile.proto 2.1) : couche choisie, propriétés, lignes en unités
//! globales découpées au carré exact de la tuile (T02 §8, T03 §3.2).
//!
//! Repris librement d'osm-surface (`src/mvt.rs`, Apache-2.0, © Gabriel Landais) : messages prost
//! écrits à la main, commandes zigzag et `ClosePath`, découpe avec un point de coupe canonique pour
//! que deux tuiles voisines calculent le même point. Ajout : chaque extrémité dit si elle est un
//! point de coupe (`cut`) ou une vraie fin de ligne.

use std::io::Read;

use ord_domain::geo::Grid;
use ord_domain::tiles::{DecodeError, TileKey};
use prost::Message;

#[derive(Clone, PartialEq, Message)]
struct RawTile {
    /// Couches gardées en octets : seule la couche demandée est décodée en entier.
    #[prost(bytes = "vec", repeated, tag = "3")]
    layers: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
struct LayerName {
    #[prost(string, tag = "1")]
    name: String,
}

#[derive(Clone, PartialEq, Message)]
struct RawLayer {
    #[prost(uint32, tag = "15")]
    version: u32,
    #[prost(string, tag = "1")]
    name: String,
    #[prost(message, repeated, tag = "2")]
    features: Vec<RawFeature>,
    #[prost(string, repeated, tag = "3")]
    keys: Vec<String>,
    #[prost(message, repeated, tag = "4")]
    values: Vec<RawValue>,
    #[prost(uint32, optional, tag = "5")]
    extent: Option<u32>,
}

#[derive(Clone, PartialEq, Message)]
struct RawFeature {
    #[prost(uint64, optional, tag = "1")]
    id: Option<u64>,
    #[prost(uint32, repeated, tag = "2")]
    tags: Vec<u32>,
    #[prost(int32, optional, tag = "3")]
    geom_type: Option<i32>,
    #[prost(uint32, repeated, tag = "4")]
    geometry: Vec<u32>,
}

#[derive(Clone, PartialEq, Message)]
struct RawValue {
    #[prost(string, optional, tag = "1")]
    string_value: Option<String>,
    #[prost(float, optional, tag = "2")]
    float_value: Option<f32>,
    #[prost(double, optional, tag = "3")]
    double_value: Option<f64>,
    #[prost(int64, optional, tag = "4")]
    int_value: Option<i64>,
    #[prost(uint64, optional, tag = "5")]
    uint_value: Option<u64>,
    #[prost(sint64, optional, tag = "6")]
    sint_value: Option<i64>,
    #[prost(bool, optional, tag = "7")]
    bool_value: Option<bool>,
}

/// Valeur d'une propriété MVT.
#[derive(Clone, Debug, PartialEq)]
pub enum MvtValue {
    String(String),
    Bool(bool),
    Int(i64),
    Float(f64),
}

impl MvtValue {
    fn from_raw(v: &RawValue) -> Option<Self> {
        if let Some(s) = &v.string_value {
            return Some(Self::String(s.clone()));
        }
        if let Some(b) = v.bool_value {
            return Some(Self::Bool(b));
        }
        if let Some(i) = v.int_value.or(v.sint_value) {
            return Some(Self::Int(i));
        }
        if let Some(u) = v.uint_value {
            return Some(Self::Int(i64::try_from(u).unwrap_or(i64::MAX)));
        }
        v.double_value.or(v.float_value.map(f64::from)).map(Self::Float)
    }

    /// Forme texte (booléens et nombres compris).
    pub fn as_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
        }
    }

    /// Vrai pour `true`, `"true"`, `"yes"`, `"1"` et tout nombre non nul.
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::String(s) => matches!(s.as_str(), "true" | "yes" | "1"),
            Self::Int(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
        }
    }
}

const GEOM_LINESTRING: i32 = 2;

/// Ligne découpée au carré de sa tuile, en unités globales.
#[derive(Clone, PartialEq)]
pub struct ClippedLine {
    pub points: Vec<[f64; 2]>,
    /// Extrémité coupée au bord de la tuile (et non vraie fin de ligne).
    pub cut: [bool; 2],
}

impl std::fmt::Debug for ClippedLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClippedLine").field("points", &self.points.len()).field("cut", &self.cut).finish()
    }
}

/// Entité linéaire d'une couche : propriétés (index dans `keys`/`values` résolus) et lignes.
#[derive(Clone, Debug, PartialEq)]
pub struct LineFeature {
    pub id: Option<u64>,
    pub properties: Vec<(String, MvtValue)>,
    pub lines: Vec<ClippedLine>,
}

fn invalid(e: impl std::fmt::Display) -> DecodeError {
    DecodeError::Format(format!("mvt: {e}"))
}

/// Corps décompressé : gzip accepté (`1f 8b`), sinon tel quel.
fn inflate(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, DecodeError> {
    if bytes.len() > 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut out).map_err(invalid)?;
        Ok(std::borrow::Cow::Owned(out))
    } else {
        Ok(std::borrow::Cow::Borrowed(bytes))
    }
}

/// Entités linéaires de la couche `layer` de la tuile `key`, en unités globales de `grid`,
/// découpées au carré de la tuile. Corps vide : aucune entité.
pub fn decode_line_layer(
    bytes: &[u8],
    layer: &str,
    key: TileKey,
    grid: Grid,
) -> Result<Vec<LineFeature>, DecodeError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = inflate(bytes)?;
    let raw = RawTile::decode(&bytes[..]).map_err(invalid)?;
    let mut out = Vec::new();
    for layer_bytes in &raw.layers {
        if LayerName::decode(&layer_bytes[..]).map_err(invalid)?.name != layer {
            continue;
        }
        let l = RawLayer::decode(&layer_bytes[..]).map_err(invalid)?;
        let scale = f64::from(grid.extent) / f64::from(l.extent.unwrap_or(4096).max(1));
        let [x0, y0] = grid.tile_origin(key);
        let e = f64::from(grid.extent);
        let bbox = [x0, y0, x0 + e, y0 + e];
        for f in &l.features {
            if f.geom_type != Some(GEOM_LINESTRING) {
                continue;
            }
            let mut properties = Vec::new();
            for pair in f.tags.as_chunks::<2>().0 {
                let (Some(k), Some(v)) = (l.keys.get(pair[0] as usize), l.values.get(pair[1] as usize))
                else {
                    continue;
                };
                if let Some(v) = MvtValue::from_raw(v) {
                    properties.push((k.clone(), v));
                }
            }
            let mut lines = Vec::new();
            for line in decode_lines(&f.geometry) {
                let global: Vec<[f64; 2]> = line
                    .iter()
                    .map(|&(px, py)| [x0 + f64::from(px) * scale, y0 + f64::from(py) * scale])
                    .collect();
                lines.extend(clip_polyline(&global, bbox).into_iter().filter(|c| c.points.len() >= 2));
            }
            if !lines.is_empty() {
                out.push(LineFeature { id: f.id, properties, lines });
            }
        }
    }
    Ok(out)
}

const GEOM_POLYGON: i32 = 3;

/// Anneaux des polygones de la couche `layer` de la tuile `key` (une entrée par entité, ses
/// anneaux extérieurs et intérieurs), en unités globales de `grid`, non découpés (tampon de tuile
/// compris). Sert au banc (couche `bridges`, emprise des ponts) ; le service ne la lit pas.
pub fn decode_polygon_layer(
    bytes: &[u8],
    layer: &str,
    key: TileKey,
    grid: Grid,
) -> Result<Vec<Vec<Vec<[f64; 2]>>>, DecodeError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = inflate(bytes)?;
    let raw = RawTile::decode(&bytes[..]).map_err(invalid)?;
    let mut out = Vec::new();
    for layer_bytes in &raw.layers {
        if LayerName::decode(&layer_bytes[..]).map_err(invalid)?.name != layer {
            continue;
        }
        let l = RawLayer::decode(&layer_bytes[..]).map_err(invalid)?;
        let scale = f64::from(grid.extent) / f64::from(l.extent.unwrap_or(4096).max(1));
        let [x0, y0] = grid.tile_origin(key);
        for f in l.features.iter().filter(|f| f.geom_type == Some(GEOM_POLYGON)) {
            let rings: Vec<Vec<[f64; 2]>> = decode_lines(&f.geometry)
                .into_iter()
                .filter(|r| r.len() >= 3)
                .map(|r| {
                    r.iter()
                        .map(|&(px, py)| [x0 + f64::from(px) * scale, y0 + f64::from(py) * scale])
                        .collect()
                })
                .collect();
            if !rings.is_empty() {
                out.push(rings);
            }
        }
    }
    Ok(out)
}

fn zigzag(v: u32) -> i32 {
    ((v >> 1) as i32) ^ -((v & 1) as i32)
}

/// Lignes d'un flux de commandes, en pixels de tuile.
fn decode_lines(geometry: &[u32]) -> Vec<Vec<(i32, i32)>> {
    let mut lines: Vec<Vec<(i32, i32)>> = Vec::new();
    let (mut cx, mut cy) = (0i32, 0i32);
    let mut i = 0;
    while i < geometry.len() {
        let cmd = geometry[i] & 0x7;
        let count = (geometry[i] >> 3) as usize;
        i += 1;
        match cmd {
            1 | 2 => {
                for _ in 0..count {
                    let (Some(&dx), Some(&dy)) = (geometry.get(i), geometry.get(i + 1)) else {
                        return lines;
                    };
                    i += 2;
                    cx = cx.wrapping_add(zigzag(dx));
                    cy = cy.wrapping_add(zigzag(dy));
                    if cmd == 1 {
                        lines.push(vec![(cx, cy)]);
                    } else if let Some(last) = lines.last_mut() {
                        if last.last() != Some(&(cx, cy)) {
                            last.push((cx, cy));
                        }
                    }
                }
            }
            7 => {
                // ClosePath : une rue fermée (giratoire dessiné en ligne) retrouve son premier point.
                if let Some(last) = lines.last_mut() {
                    if let Some(&first) = last.first() {
                        last.push(first);
                    }
                }
            }
            _ => return lines,
        }
    }
    lines
}

/// Découpe une polyligne à un rectangle `[x0, y0, x1, y1]` : les morceaux intérieurs, dans l'ordre,
/// chacun avec ses extrémités coupées. Le point de coupe ne dépend que de la paire non ordonnée
/// des extrémités du segment : les deux tuiles d'une frontière calculent le même point.
pub fn clip_polyline(points: &[[f64; 2]], bbox: [f64; 4]) -> Vec<ClippedLine> {
    let mut out: Vec<ClippedLine> = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();
    let mut start_cut = false;
    let mut end_cut = false;
    let flush = |current: &mut Vec<[f64; 2]>, start_cut: bool, end_cut: bool, out: &mut Vec<ClippedLine>| {
        if current.len() >= 2 {
            out.push(ClippedLine { points: std::mem::take(current), cut: [start_cut, end_cut] });
        } else {
            current.clear();
        }
    };
    for w in points.windows(2) {
        match clip_segment(w[0], w[1], bbox) {
            Some((a, b)) => {
                if current.last().is_none_or(|l| *l != a) {
                    flush(&mut current, start_cut, end_cut, &mut out);
                    current.push(a);
                    start_cut = a != w[0];
                }
                if a != b {
                    current.push(b);
                }
                end_cut = b != w[1];
            }
            None => flush(&mut current, start_cut, end_cut, &mut out),
        }
    }
    flush(&mut current, start_cut, end_cut, &mut out);
    out
}

fn clip_segment(p: [f64; 2], q: [f64; 2], bbox: [f64; 4]) -> Option<([f64; 2], [f64; 2])> {
    let inside = |r: [f64; 2]| r[0] >= bbox[0] && r[0] <= bbox[2] && r[1] >= bbox[1] && r[1] <= bbox[3];
    if inside(p) && inside(q) {
        return Some((p, q));
    }
    // Ordre canonique : la coupe ne dépend que de la paire non ordonnée.
    let swap = (q[0], q[1]) < (p[0], p[1]);
    let (a, b) = if swap { (q, p) } else { (p, q) };
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (pk, qk) in [(-dx, a[0] - bbox[0]), (dx, bbox[2] - a[0]), (-dy, a[1] - bbox[1]), (dy, bbox[3] - a[1])]
    {
        if pk == 0.0 {
            if qk < 0.0 {
                return None;
            }
        } else {
            let r = qk / pk;
            if pk < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    let at = |t: f64| -> [f64; 2] {
        if t <= 0.0 {
            a
        } else if t >= 1.0 {
            b
        } else {
            let mut r = [a[0] + t * dx, a[1] + t * dy];
            // La coordonnée coupée est posée exactement sur le bord.
            for (k, edge) in [(0, bbox[0]), (0, bbox[2]), (1, bbox[1]), (1, bbox[3])] {
                if (r[k] - edge).abs() < 1e-6 {
                    r[k] = edge;
                }
            }
            r
        }
    };
    let (ca, cb) = (at(t0), at(t1));
    if ca == cb {
        return None;
    }
    Some(if swap { (cb, ca) } else { (ca, cb) })
}

// ---------------------------------------------------------------------------------------------
// Encodage (tests et tuiles synthétiques seulement)
// ---------------------------------------------------------------------------------------------

/// Entité à encoder : propriétés et lignes en pixels de tuile (hors de `[0, extent]` permis).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EncodeFeature {
    pub properties: Vec<(String, MvtValue)>,
    pub lines: Vec<Vec<[i64; 2]>>,
}

fn zigzag_encode(v: i64) -> u32 {
    ((v << 1) ^ (v >> 63)) as u32
}

/// Tuile MVT d'une seule couche de lignes (encodeur minimal, pour les tests).
pub fn encode_line_layer(layer: &str, extent: u32, features: &[EncodeFeature]) -> Vec<u8> {
    let mut keys: Vec<String> = Vec::new();
    let mut values: Vec<RawValue> = Vec::new();
    let mut raw_features = Vec::new();
    for f in features {
        let mut tags = Vec::new();
        for (k, v) in &f.properties {
            let ki = keys.iter().position(|x| x == k).unwrap_or_else(|| {
                keys.push(k.clone());
                keys.len() - 1
            });
            let rv = match v {
                MvtValue::String(s) => RawValue { string_value: Some(s.clone()), ..RawValue::default() },
                MvtValue::Bool(b) => RawValue { bool_value: Some(*b), ..RawValue::default() },
                MvtValue::Int(i) => RawValue { int_value: Some(*i), ..RawValue::default() },
                MvtValue::Float(x) => RawValue { double_value: Some(*x), ..RawValue::default() },
            };
            let vi = values.iter().position(|x| *x == rv).unwrap_or_else(|| {
                values.push(rv);
                values.len() - 1
            });
            tags.extend([ki as u32, vi as u32]);
        }
        let mut geometry = Vec::new();
        let (mut cx, mut cy) = (0i64, 0i64);
        for line in f.lines.iter().filter(|l| l.len() >= 2) {
            geometry.push((1 << 3) | 1);
            geometry.extend([zigzag_encode(line[0][0] - cx), zigzag_encode(line[0][1] - cy)]);
            (cx, cy) = (line[0][0], line[0][1]);
            geometry.push(((line.len() as u32 - 1) << 3) | 2);
            for p in &line[1..] {
                geometry.extend([zigzag_encode(p[0] - cx), zigzag_encode(p[1] - cy)]);
                (cx, cy) = (p[0], p[1]);
            }
        }
        raw_features.push(RawFeature { id: None, tags, geom_type: Some(GEOM_LINESTRING), geometry });
    }
    let l = RawLayer {
        version: 2,
        name: layer.into(),
        features: raw_features,
        keys,
        values,
        extent: Some(extent),
    };
    RawTile { layers: vec![l.encode_to_vec()] }.encode_to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_round_trip() {
        for v in [0i64, 1, -1, 2, -2, 4095, -4096] {
            assert_eq!(i64::from(zigzag(zigzag_encode(v))), v);
        }
    }

    #[test]
    fn commands_decode_into_lines() {
        // MoveTo(2,2) LineTo(+0,+8) (+8,+0) ; MoveTo(+1,+1) LineTo(+2,+0) ; puis ClosePath.
        let g = [9, 4, 4, 18, 0, 16, 16, 0, 9, 2, 2, 10, 4, 0];
        assert_eq!(decode_lines(&g), vec![vec![(2, 2), (2, 10), (10, 10)], vec![(11, 11), (13, 11)]]);
        let closed = [9, 0, 0, 18, 8, 0, 0, 8, 15];
        assert_eq!(decode_lines(&closed), vec![vec![(0, 0), (4, 0), (4, 4), (0, 0)]]);
    }

    #[test]
    fn clipping_cuts_at_the_border_symmetrically_and_flags_cuts() {
        let a = [10.0, 10.0];
        let b = [30.0, 20.0];
        let left = clip_polyline(&[a, b], [0.0, 0.0, 20.0, 40.0]);
        let right = clip_polyline(&[b, a], [20.0, 0.0, 40.0, 40.0]);
        assert_eq!(left[0].points, vec![a, [20.0, 15.0]]);
        assert_eq!(left[0].cut, [false, true]);
        assert_eq!(right[0].points, vec![b, [20.0, 15.0]]);
        assert_eq!(right[0].cut, [false, true]);
        assert!(clip_polyline(&[[50.0, 50.0], [60.0, 60.0]], [0.0, 0.0, 20.0, 20.0]).is_empty());
        let pieces =
            clip_polyline(&[[5.0, 5.0], [30.0, 5.0], [30.0, 10.0], [5.0, 10.0]], [0.0, 0.0, 20.0, 20.0]);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].cut, [false, true]);
        assert_eq!(pieces[1].cut, [true, false]);
        let inside = clip_polyline(&[[1.0, 1.0], [2.0, 2.0]], [0.0, 0.0, 20.0, 20.0]);
        assert_eq!(inside[0].cut, [false, false]);
    }

    #[test]
    fn encoded_layer_decodes_back_gzip_or_not() {
        let grid = Grid::new(14, 4096);
        let key = TileKey { z: 14, x: 3, y: 5 };
        let f = EncodeFeature {
            properties: vec![
                ("kind".into(), MvtValue::String("track".into())),
                ("bridge".into(), MvtValue::Bool(true)),
            ],
            lines: vec![vec![[100, 100], [5000, 100]]],
        };
        let bytes = encode_line_layer("streets", 4096, &[f]);
        let decoded = decode_line_layer(&bytes, "streets", key, grid).unwrap();
        assert_eq!(decoded.len(), 1);
        let line = &decoded[0].lines[0];
        assert_eq!(line.cut, [false, true], "coupée au bord est");
        assert_eq!(line.points[1][0], 4.0 * 4096.0);
        assert!(decode_line_layer(&bytes, "other", key, grid).unwrap().is_empty());
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &bytes).unwrap();
        assert_eq!(decode_line_layer(&gz.finish().unwrap(), "streets", key, grid).unwrap(), decoded);
        assert!(decode_line_layer(&[], "streets", key, grid).unwrap().is_empty());
        assert!(decode_line_layer(b"<html>", "streets", key, grid).is_err());
    }

    #[test]
    fn polygon_layer_decodes_its_rings() {
        let grid = crate::streets::Shortbread::GRID;
        let key = TileKey { z: 14, x: 8000, y: 5600 };
        // Carré de 100 × 100 pixels fermé.
        let geometry = vec![
            (1 << 3) | 1,
            zigzag_encode(10),
            zigzag_encode(10),
            (3 << 3) | 2,
            zigzag_encode(100),
            zigzag_encode(0),
            zigzag_encode(0),
            zigzag_encode(100),
            zigzag_encode(-100),
            zigzag_encode(0),
            7 | (1 << 3),
        ];
        let l = RawLayer {
            version: 2,
            name: "bridges".into(),
            features: vec![RawFeature {
                id: None,
                tags: Vec::new(),
                geom_type: Some(GEOM_POLYGON),
                geometry,
            }],
            keys: Vec::new(),
            values: Vec::new(),
            extent: Some(4096),
        };
        let bytes = RawTile { layers: vec![l.encode_to_vec()] }.encode_to_vec();
        let p = decode_polygon_layer(&bytes, "bridges", key, grid).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0][0].len(), 5, "anneau fermé");
        assert!(decode_line_layer(&bytes, "bridges", key, grid).unwrap().is_empty());
    }
}
