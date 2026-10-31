//! Lecteur PMTiles v3 minimal, en mémoire (T02 §3 : `coverage-index.pmtiles`, 1,5 Mo, chargé
//! entier). En-tête de 127 octets, répertoires compressés (gzip ou sans compression), entrées
//! indexées par l'identifiant de Hilbert de la tuile. Les tuiles compressées ne sont pas prises en
//! charge : l'index de couverture de Mapterhorn est servi sans compression de tuile.

use std::io::Read;

use ord_domain::tiles::TileKey;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PmtilesError {
    #[error("not a PMTiles v3 archive")]
    Magic,
    #[error("truncated archive")]
    Truncated,
    #[error("unsupported compression {0}")]
    Compression(u8),
    #[error("invalid directory: {0}")]
    Directory(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    tile_id: u64,
    offset: u64,
    length: u32,
    run_length: u32,
}

/// Archive PMTiles en mémoire ; toutes les entrées de tuiles sont indexées au chargement.
#[derive(Clone)]
pub struct Pmtiles {
    bytes: Vec<u8>,
    tile_data_offset: u64,
    entries: Vec<Entry>,
    pub min_zoom: u8,
    pub max_zoom: u8,
}

impl std::fmt::Debug for Pmtiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pmtiles")
            .field("bytes", &self.bytes.len())
            .field("entries", &self.entries.len())
            .field("min_zoom", &self.min_zoom)
            .field("max_zoom", &self.max_zoom)
            .finish()
    }
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, PmtilesError> {
    b.get(at..at + 8).and_then(|s| s.try_into().ok()).map(u64::from_le_bytes).ok_or(PmtilesError::Truncated)
}

fn varint(b: &[u8], pos: &mut usize) -> Result<u64, PmtilesError> {
    let mut out = 0u64;
    for shift in (0..64).step_by(7) {
        let x = *b.get(*pos).ok_or_else(|| PmtilesError::Directory("truncated varint".into()))?;
        *pos += 1;
        out |= u64::from(x & 0x7f) << shift;
        if x < 0x80 {
            return Ok(out);
        }
    }
    Err(PmtilesError::Directory("varint too long".into()))
}

fn decompress(b: &[u8], compression: u8) -> Result<Vec<u8>, PmtilesError> {
    match compression {
        0 | 1 => Ok(b.to_vec()),
        2 => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(b)
                .read_to_end(&mut out)
                .map_err(|e| PmtilesError::Directory(format!("gzip: {e}")))?;
            Ok(out)
        }
        other => Err(PmtilesError::Compression(other)),
    }
}

fn directory(b: &[u8]) -> Result<Vec<Entry>, PmtilesError> {
    let mut pos = 0;
    let n = usize::try_from(varint(b, &mut pos)?).map_err(|_| PmtilesError::Directory("size".into()))?;
    if n > b.len() {
        return Err(PmtilesError::Directory("entry count".into()));
    }
    let mut entries = vec![Entry { tile_id: 0, offset: 0, length: 0, run_length: 0 }; n];
    let mut last = 0u64;
    for e in &mut entries {
        last += varint(b, &mut pos)?;
        e.tile_id = last;
    }
    for e in &mut entries {
        e.run_length =
            u32::try_from(varint(b, &mut pos)?).map_err(|_| PmtilesError::Directory("run".into()))?;
    }
    for e in &mut entries {
        e.length =
            u32::try_from(varint(b, &mut pos)?).map_err(|_| PmtilesError::Directory("length".into()))?;
    }
    for i in 0..n {
        let v = varint(b, &mut pos)?;
        entries[i].offset = if v == 0 && i > 0 {
            entries[i - 1].offset + u64::from(entries[i - 1].length)
        } else {
            v.checked_sub(1).ok_or_else(|| PmtilesError::Directory("offset".into()))?
        };
    }
    Ok(entries)
}

/// Identifiant de Hilbert d'une tuile (spécification PMTiles v3).
pub fn tile_id(key: TileKey) -> u64 {
    let z = u32::from(key.z);
    let base: u64 = (0..z).map(|i| 1u64 << (2 * i)).sum();
    let n = 1u64 << z;
    let (mut x, mut y) = (u64::from(key.x), u64::from(key.y));
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                // Arithmétique modulaire : seuls les bits sous `s` comptent ensuite.
                x = s.wrapping_sub(1).wrapping_sub(x);
                y = s.wrapping_sub(1).wrapping_sub(y);
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    base + d
}

impl Pmtiles {
    /// Ouvre une archive entière en mémoire et lit tous ses répertoires.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, PmtilesError> {
        if bytes.len() < 127 || &bytes[..7] != b"PMTiles" || bytes[7] != 3 {
            return Err(PmtilesError::Magic);
        }
        let (root_off, root_len) = (u64_at(&bytes, 8)?, u64_at(&bytes, 16)?);
        let (leaf_off, _leaf_len) = (u64_at(&bytes, 40)?, u64_at(&bytes, 48)?);
        let tile_data_offset = u64_at(&bytes, 56)?;
        let internal = bytes[97];
        let tile_compression = bytes[98];
        if tile_compression > 1 {
            return Err(PmtilesError::Compression(tile_compression));
        }
        let (min_zoom, max_zoom) = (bytes[100], bytes[101]);
        let slice = |off: u64, len: u64| -> Result<&[u8], PmtilesError> {
            let (a, l) = (usize::try_from(off).ok(), usize::try_from(len).ok());
            let (Some(a), Some(l)) = (a, l) else { return Err(PmtilesError::Truncated) };
            bytes.get(a..a.checked_add(l).ok_or(PmtilesError::Truncated)?).ok_or(PmtilesError::Truncated)
        };
        let mut entries = Vec::new();
        let mut pending = vec![directory(&decompress(slice(root_off, root_len)?, internal)?)?];
        let mut depth = 0;
        while let Some(dir) = pending.pop() {
            for e in dir {
                if e.run_length == 0 {
                    // Répertoire feuille.
                    depth += 1;
                    if depth > 100_000 {
                        return Err(PmtilesError::Directory("too many leaves".into()));
                    }
                    let raw = slice(leaf_off + e.offset, u64::from(e.length))?;
                    pending.push(directory(&decompress(raw, internal)?)?);
                } else {
                    entries.push(e);
                }
            }
        }
        entries.sort_by_key(|e| e.tile_id);
        Ok(Self { bytes, tile_data_offset, entries, min_zoom, max_zoom })
    }

    /// Octets d'une tuile, `None` si absente de l'archive.
    pub fn tile(&self, key: TileKey) -> Option<&[u8]> {
        let id = tile_id(key);
        let i = self.entries.partition_point(|e| e.tile_id <= id).checked_sub(1)?;
        let e = self.entries[i];
        if id >= e.tile_id + u64::from(e.run_length) {
            return None;
        }
        let start = usize::try_from(self.tile_data_offset + e.offset).ok()?;
        self.bytes.get(start..start + e.length as usize)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    /// Archive de test, sans compression, un répertoire racine : `(tuile, octets)` triés par id.
    pub(crate) fn build(tiles: &[(TileKey, Vec<u8>)]) -> Vec<u8> {
        let mut sorted: Vec<(u64, &Vec<u8>)> = tiles.iter().map(|(k, b)| (tile_id(*k), b)).collect();
        sorted.sort_by_key(|t| t.0);
        let mut dir = Vec::new();
        put_varint(&mut dir, sorted.len() as u64);
        let mut last = 0;
        for (id, _) in &sorted {
            put_varint(&mut dir, id - last);
            last = *id;
        }
        for _ in &sorted {
            put_varint(&mut dir, 1);
        }
        for (_, b) in &sorted {
            put_varint(&mut dir, b.len() as u64);
        }
        let mut off = 0u64;
        for (_, b) in &sorted {
            put_varint(&mut dir, off + 1);
            off += b.len() as u64;
        }
        let mut header = vec![0u8; 127];
        header[..7].copy_from_slice(b"PMTiles");
        header[7] = 3;
        let root_off = 127u64;
        let data_off = root_off + dir.len() as u64;
        header[8..16].copy_from_slice(&root_off.to_le_bytes());
        header[16..24].copy_from_slice(&(dir.len() as u64).to_le_bytes());
        header[40..48].copy_from_slice(&data_off.to_le_bytes());
        header[56..64].copy_from_slice(&data_off.to_le_bytes());
        header[97] = 1;
        header[98] = 1;
        header[100] = 0;
        header[101] = 12;
        let mut out = header;
        out.extend_from_slice(&dir);
        for (_, b) in &sorted {
            out.extend_from_slice(b);
        }
        out
    }

    #[test]
    fn hilbert_ids_of_the_specification() {
        assert_eq!(tile_id(TileKey { z: 0, x: 0, y: 0 }), 0);
        assert_eq!(tile_id(TileKey { z: 1, x: 0, y: 0 }), 1);
        assert_eq!(tile_id(TileKey { z: 1, x: 0, y: 1 }), 2);
        assert_eq!(tile_id(TileKey { z: 1, x: 1, y: 1 }), 3);
        assert_eq!(tile_id(TileKey { z: 1, x: 1, y: 0 }), 4);
        assert_eq!(tile_id(TileKey { z: 2, x: 0, y: 0 }), 5);
    }

    #[test]
    fn round_trip_and_missing_tiles() {
        let a = TileKey { z: 12, x: 2074, y: 1409 };
        let b = TileKey { z: 12, x: 2213, y: 1468 };
        let pm = Pmtiles::parse(build(&[(a, b"{\"glo30\":{}}".to_vec()), (b, b"x".to_vec())])).unwrap();
        assert_eq!(pm.tile(a), Some(&b"{\"glo30\":{}}"[..]));
        assert_eq!(pm.tile(b), Some(&b"x"[..]));
        assert_eq!(pm.tile(TileKey { z: 12, x: 0, y: 0 }), None);
        assert_eq!(Pmtiles::parse(b"nope".to_vec()).unwrap_err(), PmtilesError::Magic);
    }
}
