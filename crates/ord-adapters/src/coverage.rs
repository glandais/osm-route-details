//! Polygones de couverture de Mapterhorn (`coverage.pmtiles`, servi en MVT, couche `coverage`,
//! attribut `source`), exploration 05 §1.1. Décodeur protobuf écrit à la main, limité à ce qu'il
//! faut ici : nom de couche, attribut `source`, géométrie des polygones (MVT 2.1).
//!
//! Les sommets restent en unités de la tuile (`[0, extent]`, y vers le sud) ; le référentiel de
//! couverture (`ord-data::coverage`) fait la conversion.

use std::time::Duration;

use ord_domain::coverage::{CoveragePolygon, CoverageTile};
use ord_domain::tiles::{DecodeError, TileCodec, TileKey, TtlClass};

/// Décodeur de la couche `coverage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoverageCodec {
    /// TTL des polygones (7 j, T02 §5.3).
    pub ttl: Duration,
}

impl Default for CoverageCodec {
    fn default() -> Self {
        Self { ttl: Duration::from_secs(7 * 86_400) }
    }
}

fn err(msg: &str) -> DecodeError {
    DecodeError::Format(format!("mvt: {msg}"))
}

/// Lecteur protobuf minimal (varints et champs délimités).
struct Pb<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Pb<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn done(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn varint(&mut self) -> Result<u64, DecodeError> {
        let mut out = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.buf.get(self.pos).ok_or_else(|| err("truncated varint"))?;
            self.pos += 1;
            out |= u64::from(b & 0x7f) << shift;
            if b < 0x80 {
                return Ok(out);
            }
        }
        Err(err("varint too long"))
    }

    fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let len = usize::try_from(self.varint()?).map_err(|_| err("length"))?;
        let end = self
            .pos
            .checked_add(len)
            .filter(|e| *e <= self.buf.len())
            .ok_or_else(|| err("truncated field"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    /// Champ suivant : `(numéro, type de fil)`.
    fn key(&mut self) -> Result<(u64, u8), DecodeError> {
        let k = self.varint()?;
        Ok((k >> 3, (k & 7) as u8))
    }

    fn skip(&mut self, wire: u8) -> Result<(), DecodeError> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => self.pos += 8,
            2 => {
                self.bytes()?;
            }
            5 => self.pos += 4,
            _ => return Err(err("unsupported wire type")),
        }
        if self.pos > self.buf.len() {
            return Err(err("truncated field"));
        }
        Ok(())
    }

    fn packed(&mut self) -> Result<Vec<u32>, DecodeError> {
        let mut inner = Pb::new(self.bytes()?);
        let mut out = Vec::new();
        while !inner.done() {
            out.push(u32::try_from(inner.varint()?).map_err(|_| err("packed value"))?);
        }
        Ok(out)
    }
}

fn zigzag(v: u32) -> i64 {
    i64::from(v >> 1) ^ -i64::from(v & 1)
}

/// Anneaux d'une géométrie de polygone (commandes MoveTo, LineTo, ClosePath).
fn rings(geometry: &[u32]) -> Result<Vec<Vec<[f64; 2]>>, DecodeError> {
    let (mut x, mut y) = (0i64, 0i64);
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut ring: Vec<[f64; 2]> = Vec::new();
    let mut i = 0;
    while i < geometry.len() {
        let cmd = geometry[i];
        i += 1;
        let (id, count) = (cmd & 7, (cmd >> 3) as usize);
        match id {
            1 | 2 => {
                if id == 1 && !ring.is_empty() {
                    out.push(std::mem::take(&mut ring));
                }
                for _ in 0..count {
                    let (dx, dy) = (
                        *geometry.get(i).ok_or_else(|| err("truncated geometry"))?,
                        *geometry.get(i + 1).ok_or_else(|| err("truncated geometry"))?,
                    );
                    i += 2;
                    x += zigzag(dx);
                    y += zigzag(dy);
                    ring.push([x as f64, y as f64]);
                }
            }
            7 => {
                if ring.len() >= 3 {
                    out.push(std::mem::take(&mut ring));
                } else {
                    ring.clear();
                }
            }
            _ => return Err(err("unknown geometry command")),
        }
    }
    if ring.len() >= 3 {
        out.push(ring);
    }
    Ok(out)
}

struct RawFeature {
    tags: Vec<u32>,
    kind: u64,
    geometry: Vec<u32>,
}

fn feature(buf: &[u8]) -> Result<RawFeature, DecodeError> {
    let mut pb = Pb::new(buf);
    let mut f = RawFeature { tags: Vec::new(), kind: 0, geometry: Vec::new() };
    while !pb.done() {
        match pb.key()? {
            (2, 2) => f.tags = pb.packed()?,
            (3, 0) => f.kind = pb.varint()?,
            (4, 2) => f.geometry = pb.packed()?,
            (_, w) => pb.skip(w)?,
        }
    }
    Ok(f)
}

/// Valeur chaîne d'un `Value` MVT (les autres types sont ignorés).
fn string_value(buf: &[u8]) -> Result<Option<String>, DecodeError> {
    let mut pb = Pb::new(buf);
    let mut out = None;
    while !pb.done() {
        match pb.key()? {
            (1, 2) => out = Some(String::from_utf8_lossy(pb.bytes()?).into_owned()),
            (_, w) => pb.skip(w)?,
        }
    }
    Ok(out)
}

fn layer(buf: &[u8]) -> Result<Option<CoverageTile>, DecodeError> {
    let mut pb = Pb::new(buf);
    let (mut name, mut extent) = (String::new(), 4096u32);
    let (mut keys, mut values, mut features) = (Vec::new(), Vec::new(), Vec::new());
    while !pb.done() {
        match pb.key()? {
            (1, 2) => name = String::from_utf8_lossy(pb.bytes()?).into_owned(),
            (2, 2) => features.push(feature(pb.bytes()?)?),
            (3, 2) => keys.push(String::from_utf8_lossy(pb.bytes()?).into_owned()),
            (4, 2) => values.push(string_value(pb.bytes()?)?),
            (5, 0) => extent = u32::try_from(pb.varint()?).map_err(|_| err("extent"))?,
            (_, w) => pb.skip(w)?,
        }
    }
    if name != "coverage" {
        return Ok(None);
    }
    let mut polygons = Vec::new();
    for f in features.into_iter().filter(|f| f.kind == 3) {
        let source = f
            .tags
            .as_chunks::<2>()
            .0
            .iter()
            .find(|kv| keys.get(kv[0] as usize).is_some_and(|k| k == "source"))
            .and_then(|kv| values.get(kv[1] as usize).cloned().flatten());
        let Some(source) = source else { continue };
        let rings = rings(&f.geometry)?;
        if !rings.is_empty() {
            polygons.push(CoveragePolygon { source, rings });
        }
    }
    Ok(Some(CoverageTile { extent, polygons }))
}

/// Décode une tuile MVT ; sans couche `coverage`, la tuile est vide (aucune source).
pub fn decode_coverage_mvt(bytes: &[u8]) -> Result<CoverageTile, DecodeError> {
    let mut pb = Pb::new(bytes);
    let mut out = CoverageTile { extent: 4096, polygons: Vec::new() };
    while !pb.done() {
        match pb.key()? {
            (3, 2) => {
                if let Some(t) = layer(pb.bytes()?)? {
                    out = t;
                }
            }
            (_, w) => pb.skip(w)?,
        }
    }
    Ok(out)
}

impl TileCodec for CoverageCodec {
    type Tile = CoverageTile;

    fn decode(&self, _id: TileKey, bytes: &[u8]) -> Result<CoverageTile, DecodeError> {
        if bytes.first() == Some(&b'<') {
            return Err(err("HTML or XML body"));
        }
        decode_coverage_mvt(bytes)
    }

    fn empty(&self, _id: TileKey) -> CoverageTile {
        CoverageTile { extent: 4096, polygons: Vec::new() }
    }

    fn weight(&self, tile: &CoverageTile) -> u32 {
        let vertices: usize = tile.polygons.iter().flat_map(|p| &p.rings).map(Vec::len).sum();
        (vertices * 16 + tile.polygons.len() * 64 + 64).min(u32::MAX as usize) as u32
    }

    fn ttl_class(&self, _tile: &CoverageTile) -> TtlClass {
        TtlClass::Expires(self.ttl)
    }
}

/// Encodeur MVT minimal de la couche `coverage` (tests et serveurs scriptés) : un polygone par
/// source, un seul anneau, sommets en unités de tuile.
pub fn encode_coverage_mvt(extent: u32, polygons: &[(&str, Vec<[i64; 2]>)]) -> Vec<u8> {
    fn varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }
    fn field(out: &mut Vec<u8>, num: u64, bytes: &[u8]) {
        varint(out, (num << 3) | 2);
        varint(out, bytes.len() as u64);
        out.extend_from_slice(bytes);
    }
    fn packed(values: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        for v in values {
            varint(&mut out, u64::from(*v));
        }
        out
    }
    let zz = |v: i64| ((v << 1) ^ (v >> 63)) as u32;
    let mut layer = Vec::new();
    field(&mut layer, 1, b"coverage");
    for (i, (_, ring)) in polygons.iter().enumerate() {
        let mut geom = vec![(1 << 3) | 1];
        let (mut x, mut y) = (0i64, 0i64);
        for (k, p) in ring.iter().enumerate() {
            if k == 1 {
                geom.push((((ring.len() - 1) as u32) << 3) | 2);
            }
            geom.push(zz(p[0] - x));
            geom.push(zz(p[1] - y));
            (x, y) = (p[0], p[1]);
        }
        geom.push((1 << 3) | 7);
        let mut f = Vec::new();
        field(&mut f, 2, &packed(&[0, i as u32]));
        varint(&mut f, 3 << 3);
        varint(&mut f, 3);
        field(&mut f, 4, &packed(&geom));
        field(&mut layer, 2, &f);
    }
    field(&mut layer, 3, b"source");
    for (name, _) in polygons {
        let mut v = Vec::new();
        field(&mut v, 1, name.as_bytes());
        field(&mut layer, 4, &v);
    }
    varint(&mut layer, 5 << 3);
    varint(&mut layer, u64::from(extent));
    varint(&mut layer, 15 << 3);
    varint(&mut layer, 2);
    let mut tile = Vec::new();
    field(&mut tile, 3, &layer);
    tile
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tuile réelle de 60 octets (Mapterhorn, une tuile z12 « GLO-30 seul ») : un carré `glo30`.
    const GLO30_ONLY: [u8; 60] = [
        0x1a, 0x3a, 0x0a, 0x08, 0x63, 0x6f, 0x76, 0x65, 0x72, 0x61, 0x67, 0x65, 0x12, 0x18, 0x08, 0x0a, 0x12,
        0x02, 0x00, 0x00, 0x18, 0x03, 0x22, 0x0e, 0x09, 0x7f, 0x7f, 0x1a, 0x80, 0x42, 0x00, 0x00, 0x80, 0x42,
        0xff, 0x41, 0x00, 0x0f, 0x1a, 0x06, 0x73, 0x6f, 0x75, 0x72, 0x63, 0x65, 0x22, 0x07, 0x0a, 0x05, 0x67,
        0x6c, 0x6f, 0x33, 0x30, 0x28, 0x80, 0x20, 0x78, 0x02,
    ];

    #[test]
    fn decodes_a_real_glo30_tile() {
        let t = decode_coverage_mvt(&GLO30_ONLY).unwrap();
        assert_eq!(t.extent, 4096);
        assert_eq!(t.polygons.len(), 1);
        assert_eq!(t.polygons[0].source, "glo30");
        let ring = &t.polygons[0].rings[0];
        assert_eq!(ring.len(), 4);
        assert_eq!(ring[0], [-64.0, -64.0]);
        assert_eq!(ring[2], [4160.0, 4160.0]);
    }

    #[test]
    fn round_trip_of_the_test_encoder() {
        let bytes = encode_coverage_mvt(
            4096,
            &[
                ("glo30", vec![[0, 0], [4096, 0], [4096, 4096], [0, 4096]]),
                ("frrgealti1metro", vec![[0, 0], [2048, 0], [2048, 4096]]),
            ],
        );
        let t = CoverageCodec::default().decode(TileKey { z: 12, x: 0, y: 0 }, &bytes).unwrap();
        assert_eq!(
            t.polygons.iter().map(|p| p.source.as_str()).collect::<Vec<_>>(),
            ["glo30", "frrgealti1metro"]
        );
        assert_eq!(t.polygons[1].rings[0], vec![[0.0, 0.0], [2048.0, 0.0], [2048.0, 4096.0]]);
    }

    #[test]
    fn rejects_truncated_and_html() {
        assert!(decode_coverage_mvt(&GLO30_ONLY[..30]).is_err());
        assert!(CoverageCodec::default().decode(TileKey { z: 1, x: 0, y: 0 }, b"<html>").is_err());
        assert_eq!(decode_coverage_mvt(b"").unwrap().polygons.len(), 0);
    }
}
