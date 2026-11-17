//! Lecture de l'export privé : gzip en flux, `CsvReader` partagé avec l'outil, colonnes repérées par leur nom
//! d'en-tête, lots analysés en parallèle (ordre conservé). L'id d'une trace n'est jamais gardé : seule son
//! empreinte xxh3 l'est. **Privé** : rien d'ici n'est journalisé ni écrit.

use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::Path;

use anyhow::{anyhow, Context as _, Result};
use ord_graph_match::csv::CsvReader;
use ord_graph_match::doc::{parse_document, type_code, ParsedEdge};
use rayon::prelude::*;
use xxhash_rust::xxh3::{xxh3_64, Xxh3};

/// Une arête d'équipe d'une trace : sa géométrie est `Ingest::keys[key]`.
#[derive(Clone, Copy, Debug)]
pub struct TeamEdge {
    /// Indice dans `Ingest::keys`.
    pub key: u32,
    /// `trv` annoncé (`NaN` : absent).
    pub trv: f32,
    pub wk: u8,
}

#[derive(Clone, Debug)]
pub struct ExportTrack {
    /// Indice dans `Ingest::teams` (ids d'équipe, ordre d'apparition).
    pub team: usize,
    /// `xxh3_64(track_id)` ; l'id lui-même n'est pas gardé.
    pub id_hash: u64,
    /// `doc::type_code(surface_type)`.
    pub ty: u8,
    pub excluded: bool,
    /// Document lu.
    pub readable: bool,
    /// Dans l'ordre du document.
    pub edges: Vec<TeamEdge>,
}

#[derive(Debug, Default)]
pub struct Ingest {
    pub teams: Vec<u64>,
    pub tracks: Vec<ExportTrack>,
    /// Clés distinctes `(g, trv)`, dédoublonnées par `(xxh3_64(g), g.len(), trv.to_bits())` : exactement la clé de la
    /// mémoïsation du dépôt.
    pub keys: Vec<(Box<str>, f32)>,
    pub rows: u64,
    /// Lignes ignorées (identifiant d'équipe illisible ou équipe non demandée).
    pub skipped_rows: u64,
    /// Empreinte du fichier lu (octets compressés), comme l'outil.
    pub export_xxh3: u64,
}

#[derive(Clone, Copy, Debug)]
struct Columns {
    team: usize,
    track: usize,
    surface: usize,
    excluded: usize,
    doc: usize,
}

impl Columns {
    fn from_header(h: &[Vec<u8>]) -> Result<Self> {
        let find = |name: &str| {
            h.iter()
                .position(|c| c.as_slice() == name.as_bytes())
                .ok_or_else(|| anyhow!("colonne {name} absente de l'en-tête de l'export"))
        };
        Ok(Self {
            team: find("team_id")?,
            track: find("track_id")?,
            surface: find("surface_type")?,
            excluded: find("excluded_from_generator")?,
            doc: find("network_edges")?,
        })
    }

    fn needed(&self) -> usize {
        self.team.max(self.track).max(self.surface).max(self.excluded).max(self.doc) + 1
    }
}

struct ParsedRow {
    team_id: u64,
    id_hash: u64,
    ty: u8,
    excluded: bool,
    edges: Option<Vec<ParsedEdge>>,
}

fn parse_row(cols: &Columns, fields: &[Vec<u8>], wanted: &[u64]) -> Option<ParsedRow> {
    if fields.len() < cols.needed() {
        return None;
    }
    let team_id: u64 = std::str::from_utf8(&fields[cols.team]).ok()?.trim().parse().ok()?;
    if !wanted.is_empty() && !wanted.contains(&team_id) {
        return None;
    }
    Some(ParsedRow {
        team_id,
        id_hash: xxh3_64(&fields[cols.track]),
        ty: type_code(std::str::from_utf8(&fields[cols.surface]).unwrap_or("")),
        excluded: matches!(fields[cols.excluded].as_slice(), b"t" | b"true" | b"True" | b"TRUE" | b"1"),
        edges: parse_document(&fields[cols.doc]),
    })
}

const BATCH: usize = 48;

/// Lit l'export (gzip, CSV) ; `wanted` : ids d'équipe retenus (vide : toutes). Les équipes sont indicées dans
/// l'ordre d'apparition, les traces dans l'ordre des lignes.
pub fn ingest(path: &Path, wanted: &[u64]) -> Result<Ingest> {
    let file = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    let gz = flate2::bufread::MultiGzDecoder::new(BufReader::with_capacity(1 << 20, file));
    let mut csv = CsvReader::new(BufReader::with_capacity(1 << 20, gz));
    let mut fields: Vec<Vec<u8>> = Vec::new();
    anyhow::ensure!(csv.read_record(&mut fields)?, "export vide");
    let cols = Columns::from_header(&fields)?;

    let mut ing = Ingest::default();
    let mut team_index: HashMap<u64, usize> = HashMap::new();
    let mut dedup: HashMap<(u64, u32, u32), u32> = HashMap::new();
    let mut eof = false;
    while !eof {
        let mut batch: Vec<Vec<Vec<u8>>> = Vec::with_capacity(BATCH);
        while batch.len() < BATCH {
            if !csv.read_record(&mut fields)? {
                eof = true;
                break;
            }
            batch.push(std::mem::take(&mut fields));
        }
        let parsed: Vec<Option<ParsedRow>> = batch.par_iter().map(|f| parse_row(&cols, f, wanted)).collect();
        for row in parsed {
            ing.rows += 1;
            let Some(row) = row else {
                ing.skipped_rows += 1;
                continue;
            };
            let team = *team_index.entry(row.team_id).or_insert_with(|| {
                ing.teams.push(row.team_id);
                ing.teams.len() - 1
            });
            let readable = row.edges.is_some();
            let mut edges = Vec::new();
            for e in row.edges.unwrap_or_default() {
                let key = (xxh3_64(e.g.as_bytes()), e.g.len() as u32, e.trv.to_bits());
                let k = *dedup.entry(key).or_insert_with(|| {
                    ing.keys.push((e.g.clone().into_boxed_str(), e.trv));
                    (ing.keys.len() - 1) as u32
                });
                edges.push(TeamEdge { key: k, trv: e.trv, wk: e.wk });
            }
            ing.tracks.push(ExportTrack {
                team,
                id_hash: row.id_hash,
                ty: row.ty,
                excluded: row.excluded,
                readable,
                edges,
            });
        }
    }
    drop(csv);
    ing.export_xxh3 = hash_file(path)?;
    Ok(ing)
}

fn hash_file(path: &Path) -> Result<u64> {
    let mut h = Xxh3::new();
    let mut r = BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.digest())
}

/// `raw_legacy` : `shapes.py` en entier, trace par trace, sur les géométries brutes (tous types, toutes traces
/// lisibles, exclues comprises : `shapes.py` ne filtre pas).
pub fn raw_legacy(ing: &Ingest) -> Vec<super::shape::RawRow> {
    use ord_graph_match::geo::decode_polyline6;
    ing.tracks
        .par_iter()
        .filter(|t| t.readable)
        .filter_map(|t| {
            let mut pts: Vec<[f64; 2]> = Vec::new();
            let mut tot = 0.0f64;
            for e in &t.edges {
                let g = decode_polyline6(&ing.keys[e.key as usize].0).unwrap_or_default();
                if pts.is_empty() {
                    pts.extend(g);
                } else {
                    pts.extend(g.into_iter().skip(1));
                }
                if !e.trv.is_nan() {
                    tot += f64::from(e.trv);
                }
            }
            super::shape::raw_legacy_row(&pts, tot)
        })
        .collect()
}
