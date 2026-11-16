//! Surcouche de fréquentation d'une équipe (lot 1) : « arête de base → parcours distincts, par type de
//! parcours et par sens », rattachée à un graphe précis par son identité.
//!
//! Note de conception : `docs/design/technique/ord-graph-lot1.md` §6. Fichier `.ordo`, petit-boutiste :
//!
//! | Partie | Contenu |
//! |---|---|
//! | En-tête (256 o) | magie `ORDOVRL1`, version, identité du graphe, équipe, provenance de l'export, paramètres, table des types |
//! | Corps | par arête, triée : delta d'id (varint), masque `u16` des couples (type, sens) non nuls, un varint par bit |
//! | Pied | xxh3 de tout ce qui précède |
//!
//! Le bit `2 * type + sens` du masque dit que le compte du couple n'est pas nul ; sens 0 = a→b, 1 = b→a.

use std::io::Write;
use std::path::Path;

use xxhash_rust::xxh3::xxh3_64;

use crate::format::GraphIdentity;

pub const MAGIC: [u8; 8] = *b"ORDOVRL1";
pub const FORMAT_MAJOR: u16 = 1;
pub const FORMAT_MINOR: u16 = 0;
/// Types de parcours distincts d'une surcouche (le masque tient sur 16 bits : 8 types × 2 sens).
pub const MAX_TYPES: usize = 8;
/// Octets d'un nom de type dans l'en-tête.
const TYPE_NAME_LEN: usize = 12;
const HEADER_LEN: usize = 256;
const FOOTER_LEN: usize = 8;
const TOOL_LEN: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    #[error("E/S : {0}")]
    Io(#[from] std::io::Error),
    #[error("ce n'est pas un fichier de surcouche (magie)")]
    BadMagic,
    #[error("format_major {found} non pris en charge (attendu {expected})")]
    UnsupportedMajor { found: u16, expected: u16 },
    #[error("fichier tronqué ou incohérent : {0}")]
    Truncated(String),
    #[error("somme de contrôle fausse")]
    Checksum,
    #[error("surcouche d'un autre graphe (identité différente : ids d'arête non comparables)")]
    GraphMismatch,
    #[error("écriture : {0}")]
    Invalid(String),
}

/// Ce que l'en-tête porte en plus des données.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayMeta {
    pub graph: GraphIdentity,
    pub team_id: u64,
    /// xxh3 des octets de l'export lu, et nombre de lignes de données.
    pub export_xxh3: u64,
    pub export_rows: u64,
    /// Empreinte des paramètres d'appariement.
    pub params_hash: u64,
    /// Version de l'outil, 16 octets au plus.
    pub tool_version: String,
    pub build_epoch: i64,
    /// Noms des types de parcours, dans l'ordre des indices (au plus [`MAX_TYPES`], 12 octets chacun).
    pub types: Vec<String>,
}

/// Surcouche lue et vérifiée : arêtes triées et comptes à plat (`2 × types` par arête).
#[derive(Clone, Debug)]
pub struct Overlay {
    pub meta: OverlayMeta,
    edges: Vec<u32>,
    counts: Vec<u16>,
}

fn put_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn get_varint(buf: &[u8], pos: &mut usize) -> Option<u32> {
    let (mut v, mut shift) = (0u32, 0u32);
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        v |= u32::from(b & 0x7f).checked_shl(shift)?;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
}

fn truncated(what: &str) -> OverlayError {
    OverlayError::Truncated(what.into())
}

/// Sérialise une surcouche. `edges` strictement croissant ; `counts` à plat, `2 × types` par arête, aucune
/// arête entièrement nulle.
pub fn encode(meta: &OverlayMeta, edges: &[u32], counts: &[u16]) -> Result<Vec<u8>, OverlayError> {
    let stride = 2 * meta.types.len();
    if meta.types.len() > MAX_TYPES {
        return Err(OverlayError::Invalid(format!("plus de {MAX_TYPES} types")));
    }
    if meta.types.iter().any(|t| t.len() > TYPE_NAME_LEN) || meta.tool_version.len() > TOOL_LEN {
        return Err(OverlayError::Invalid("nom de type ou version trop long".into()));
    }
    if counts.len() != edges.len() * stride {
        return Err(OverlayError::Invalid("comptes et arêtes de longueurs incohérentes".into()));
    }
    if edges.windows(2).any(|w| w[0] >= w[1]) {
        return Err(OverlayError::Invalid("arêtes non strictement croissantes".into()));
    }
    let mut out = Vec::with_capacity(HEADER_LEN + edges.len() * 6 + FOOTER_LEN);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
    out.extend_from_slice(&FORMAT_MINOR.to_le_bytes());
    let g = &meta.graph;
    out.extend_from_slice(&g.format_major.to_le_bytes());
    out.extend_from_slice(&g.attr_layout.to_le_bytes());
    out.push(g.zoom);
    out.extend_from_slice(&[0; 3]);
    for v in [g.nodes, g.edges, g.directed, g.id_edges_xxh3, g.id_nodes_xxh3, g.pbf_xxh3] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&g.replication_timestamp.to_le_bytes());
    for v in [meta.team_id, meta.export_xxh3, meta.export_rows, meta.params_hash] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let mut tool = [0u8; TOOL_LEN];
    tool[..meta.tool_version.len()].copy_from_slice(meta.tool_version.as_bytes());
    out.extend_from_slice(&tool);
    out.extend_from_slice(&meta.build_epoch.to_le_bytes());
    out.extend_from_slice(&(edges.len() as u64).to_le_bytes());
    out.push(meta.types.len() as u8);
    for i in 0..MAX_TYPES {
        let mut slot = [0u8; TYPE_NAME_LEN];
        if let Some(t) = meta.types.get(i) {
            slot[..t.len()].copy_from_slice(t.as_bytes());
        }
        out.extend_from_slice(&slot);
    }
    if out.len() > HEADER_LEN {
        return Err(OverlayError::Invalid("en-tête trop long".into()));
    }
    out.resize(HEADER_LEN, 0);

    let mut prev = 0u32;
    for (i, &e) in edges.iter().enumerate() {
        let row = &counts[i * stride..(i + 1) * stride];
        let mask = row.iter().enumerate().filter(|(_, &c)| c > 0).fold(0u16, |m, (b, _)| m | (1 << b));
        if mask == 0 {
            return Err(OverlayError::Invalid("arête sans aucun compte".into()));
        }
        put_varint(&mut out, e - prev);
        prev = e;
        out.extend_from_slice(&mask.to_le_bytes());
        for &c in row.iter().filter(|&&c| c > 0) {
            put_varint(&mut out, u32::from(c));
        }
    }
    let sum = xxh3_64(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    Ok(out)
}

fn rd<const N: usize>(b: &[u8], at: &mut usize) -> [u8; N] {
    let mut a = [0u8; N];
    a.copy_from_slice(&b[*at..*at + N]);
    *at += N;
    a
}

impl Overlay {
    /// Décode et vérifie : magie, version, somme du pied, cohérence du corps. N'examine pas le graphe.
    pub fn decode(bytes: &[u8]) -> Result<Overlay, OverlayError> {
        if bytes.len() < HEADER_LEN + FOOTER_LEN {
            return Err(truncated("en-tête"));
        }
        if bytes[..8] != MAGIC {
            return Err(OverlayError::BadMagic);
        }
        let mut at = 8;
        let major = u16::from_le_bytes(rd(bytes, &mut at));
        if major != FORMAT_MAJOR {
            return Err(OverlayError::UnsupportedMajor { found: major, expected: FORMAT_MAJOR });
        }
        let (body, foot) = bytes.split_at(bytes.len() - FOOTER_LEN);
        if xxh3_64(body) != u64::from_le_bytes(foot.try_into().expect("8 octets")) {
            return Err(OverlayError::Checksum);
        }
        let _minor = u16::from_le_bytes(rd(bytes, &mut at));
        let format_major = u16::from_le_bytes(rd(bytes, &mut at));
        let attr_layout = u16::from_le_bytes(rd(bytes, &mut at));
        let zoom = bytes[at];
        at += 4;
        let mut u = [0u64; 6];
        for v in &mut u {
            *v = u64::from_le_bytes(rd(bytes, &mut at));
        }
        let replication_timestamp = i64::from_le_bytes(rd(bytes, &mut at));
        let graph = GraphIdentity {
            format_major,
            attr_layout,
            zoom,
            nodes: u[0],
            edges: u[1],
            directed: u[2],
            id_edges_xxh3: u[3],
            id_nodes_xxh3: u[4],
            pbf_xxh3: u[5],
            replication_timestamp,
        };
        let team_id = u64::from_le_bytes(rd(bytes, &mut at));
        let export_xxh3 = u64::from_le_bytes(rd(bytes, &mut at));
        let export_rows = u64::from_le_bytes(rd(bytes, &mut at));
        let params_hash = u64::from_le_bytes(rd(bytes, &mut at));
        let tool: [u8; TOOL_LEN] = rd(bytes, &mut at);
        let build_epoch = i64::from_le_bytes(rd(bytes, &mut at));
        let n_entries = u64::from_le_bytes(rd(bytes, &mut at));
        let n_types = usize::from(bytes[at]);
        at += 1;
        if n_types > MAX_TYPES {
            return Err(truncated("table des types"));
        }
        let mut types = Vec::with_capacity(n_types);
        for i in 0..MAX_TYPES {
            let slot: [u8; TYPE_NAME_LEN] = rd(bytes, &mut at);
            if i < n_types {
                types.push(String::from_utf8_lossy(&slot).trim_end_matches('\0').to_string());
            }
        }
        let meta = OverlayMeta {
            graph,
            team_id,
            export_xxh3,
            export_rows,
            params_hash,
            tool_version: String::from_utf8_lossy(&tool).trim_end_matches('\0').to_string(),
            build_epoch,
            types,
        };
        let stride = 2 * n_types;
        let n = usize::try_from(n_entries).map_err(|_| truncated("nombre d'entrées"))?;
        // Au moins 3 octets par entrée : borne le `with_capacity` d'un en-tête mensonger.
        if n > body.len() / 3 {
            return Err(truncated("nombre d'entrées"));
        }
        let mut edges = Vec::with_capacity(n);
        let mut counts = Vec::with_capacity(n * stride);
        let mut pos = HEADER_LEN;
        let mut prev = 0u64;
        for i in 0..n {
            let delta = get_varint(body, &mut pos).ok_or_else(|| truncated("corps"))?;
            if i > 0 && delta == 0 {
                return Err(truncated("arêtes non croissantes"));
            }
            prev += u64::from(delta);
            let edge = u32::try_from(prev).map_err(|_| truncated("id d'arête"))?;
            let m = body.get(pos..pos + 2).ok_or_else(|| truncated("corps"))?;
            let mask = u16::from_le_bytes([m[0], m[1]]);
            pos += 2;
            if mask == 0 || (stride < 16 && u32::from(mask) >> stride != 0) {
                return Err(truncated("masque"));
            }
            for b in 0..stride {
                if mask & (1 << b) != 0 {
                    let c = get_varint(body, &mut pos).ok_or_else(|| truncated("corps"))?;
                    let c = u16::try_from(c).ok().filter(|&c| c > 0).ok_or_else(|| truncated("compte"))?;
                    counts.push(c);
                } else {
                    counts.push(0);
                }
            }
            edges.push(edge);
        }
        if pos != body.len() {
            return Err(truncated("octets en trop après le corps"));
        }
        Ok(Overlay { meta, edges, counts })
    }

    /// Décode, puis refuse une surcouche dont l'identité de graphe n'est pas celle de `graph`.
    pub fn decode_for(bytes: &[u8], graph: &GraphIdentity) -> Result<Overlay, OverlayError> {
        let o = Self::decode(bytes)?;
        o.check_graph(graph)?;
        Ok(o)
    }

    pub fn read(path: &Path) -> Result<Overlay, OverlayError> {
        Self::decode(&std::fs::read(path)?)
    }

    pub fn check_graph(&self, graph: &GraphIdentity) -> Result<(), OverlayError> {
        if self.meta.graph.same_ids(graph) {
            Ok(())
        } else {
            Err(OverlayError::GraphMismatch)
        }
    }

    /// Octets tenus en mémoire (capacités réelles des tableaux, et les noms de types) : ce que la surcouche déchiffrée
    /// coûte une fois décodée, sans le texte clair qui l'a produite.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.edges.capacity() * 4
            + self.counts.capacity() * 2
            + self.meta.types.iter().map(|t| t.capacity() + std::mem::size_of::<String>()).sum::<usize>()
    }

    /// Nombre d'arêtes de base à compte non nul.
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Valeurs par couple : `counts[2 * type + sens]`, sens 0 = a→b, 1 = b→a.
    pub fn stride(&self) -> usize {
        2 * self.meta.types.len()
    }

    /// Comptes d'une arête de base (dichotomie), `None` si aucun parcours ne la prend.
    pub fn get(&self, edge: u32) -> Option<&[u16]> {
        let i = self.edges.binary_search(&edge).ok()?;
        let s = self.stride();
        Some(&self.counts[i * s..(i + 1) * s])
    }

    /// Arêtes et comptes dans l'ordre du fichier.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &[u16])> + '_ {
        let s = self.stride().max(1);
        self.edges.iter().copied().zip(self.counts.chunks(s))
    }
}

/// Écrit une surcouche comme le graphe : fichier `.tmp` à côté, `fsync`, relecture vérifiée, `0444`, `rename`.
/// Renvoie la taille du fichier.
pub fn write_overlay(
    path: &Path,
    meta: &OverlayMeta,
    edges: &[u32],
    counts: &[u16],
) -> Result<u64, OverlayError> {
    let bytes = encode(meta, edges, counts)?;
    let tmp = path.with_extension("ordo.tmp");
    let _ = std::fs::remove_file(&tmp);
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().create_new(true).write(true).open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        let back = Overlay::decode(&std::fs::read(&tmp)?)?;
        if back.meta != *meta || back.len() != edges.len() {
            return Err(OverlayError::Invalid("relecture différente de l'écriture".into()));
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o444))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(bytes.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> GraphIdentity {
        GraphIdentity {
            format_major: 1,
            attr_layout: 1,
            zoom: 11,
            nodes: 10,
            edges: 20,
            directed: 40,
            id_edges_xxh3: 111,
            id_nodes_xxh3: 222,
            pbf_xxh3: 333,
            replication_timestamp: 5,
        }
    }

    fn meta() -> OverlayMeta {
        OverlayMeta {
            graph: identity(),
            team_id: 898_683_759_673_449_207,
            export_xxh3: 9,
            export_rows: 3,
            params_hash: 77,
            tool_version: "1.8.0".into(),
            build_epoch: 1_794_700_000,
            types: vec!["ROAD".into(), "GRAVEL".into()],
        }
    }

    fn sample() -> (Vec<u32>, Vec<u16>) {
        (vec![0, 3, 1000, 70_000], vec![1, 0, 0, 0, 0, 2, 0, 65_535, 4, 4, 4, 4, 0, 0, 7, 0])
    }

    #[test]
    fn round_trip() {
        let (e, c) = sample();
        let bytes = encode(&meta(), &e, &c).unwrap();
        let o = Overlay::decode(&bytes).unwrap();
        assert_eq!(o.meta, meta());
        assert_eq!(o.len(), 4);
        assert_eq!(o.get(3), Some(&[0u16, 2, 0, 65_535][..]));
        assert_eq!(o.get(1000), Some(&[4u16, 4, 4, 4][..]));
        assert_eq!(o.get(70_000), Some(&[0u16, 0, 7, 0][..]));
        assert_eq!(o.get(4), None);
        assert_eq!(o.iter().count(), 4);
        o.check_graph(&identity()).unwrap();
    }

    #[test]
    fn other_graph_identity_is_refused() {
        let (e, c) = sample();
        let bytes = encode(&meta(), &e, &c).unwrap();
        for tweak in [
            |g: &mut GraphIdentity| g.id_edges_xxh3 += 1,
            |g: &mut GraphIdentity| g.id_nodes_xxh3 += 1,
            |g: &mut GraphIdentity| g.edges += 1,
            |g: &mut GraphIdentity| g.zoom = 12,
        ] {
            let mut g = identity();
            tweak(&mut g);
            assert!(matches!(Overlay::decode_for(&bytes, &g), Err(OverlayError::GraphMismatch)));
        }
        // La provenance ne compte pas : un PBF re-téléchargé qui donne les mêmes ids reste valide.
        let mut g = identity();
        g.pbf_xxh3 += 1;
        g.replication_timestamp += 1;
        assert!(Overlay::decode_for(&bytes, &g).is_ok());
    }

    #[test]
    fn corruption_truncation_and_unknown_major_are_refused() {
        let (e, c) = sample();
        let bytes = encode(&meta(), &e, &c).unwrap();
        let mut flipped = bytes.clone();
        flipped[HEADER_LEN + 1] ^= 1;
        assert!(matches!(Overlay::decode(&flipped), Err(OverlayError::Checksum)));
        assert!(Overlay::decode(&bytes[..bytes.len() - 3]).is_err());
        assert!(Overlay::decode(&bytes[..100]).is_err());
        let mut major = bytes.clone();
        major[8] = 9;
        assert!(matches!(Overlay::decode(&major), Err(OverlayError::UnsupportedMajor { found: 9, .. })));
        let mut magic = bytes.clone();
        magic[0] = b'X';
        assert!(matches!(Overlay::decode(&magic), Err(OverlayError::BadMagic)));
    }

    #[test]
    fn writer_rejects_malformed_input() {
        let m = meta();
        assert!(encode(&m, &[3, 3], &[1, 0, 0, 0, 1, 0, 0, 0]).is_err());
        assert!(encode(&m, &[3], &[0, 0, 0, 0]).is_err());
        assert!(encode(&m, &[3], &[1, 0]).is_err());
        let mut many = meta();
        many.types = (0..9).map(|i| format!("T{i}")).collect();
        assert!(encode(&many, &[], &[]).is_err());
    }

    #[test]
    fn file_round_trip_is_read_only_and_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.ordo");
        let (e, c) = sample();
        let size = write_overlay(&path, &meta(), &e, &c).unwrap();
        assert_eq!(size, std::fs::metadata(&path).unwrap().len());
        assert!(!dir.path().join("t.ordo.tmp").exists());
        assert_eq!(Overlay::read(&path).unwrap().len(), 4);
        // Réécrire par-dessus un fichier en lecture seule remplace par `rename`.
        write_overlay(&path, &meta(), &e[..2], &c[..8]).unwrap();
        assert_eq!(Overlay::read(&path).unwrap().len(), 2);
    }

    #[test]
    fn empty_overlay_is_valid() {
        let bytes = encode(&meta(), &[], &[]).unwrap();
        assert!(Overlay::decode(&bytes).unwrap().is_empty());
    }
}
