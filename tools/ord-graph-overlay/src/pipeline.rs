//! Chargement de l'export (lecture, dédoublonnage des géométries) et appariement des géométries distinctes.

use std::io::{BufReader, Read};
use std::path::Path;

use anyhow::Context;
use ord_graph::mmap::MappedGraph;
use ord_graph::util::Poly;
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use xxhash_rust::xxh3::xxh3_64;

use crate::export::{parse_row, Columns, CsvReader, ParsedRow};
use crate::geo::{decode_polyline6, polyline_len, Frame};
use crate::index::SegIndex;
use crate::matcher::{match_geometry, Chain, Fail};

/// Une arête d'équipe, dépouillée de sa polyligne (renvoyée par `geom` vers une géométrie distincte).
#[derive(Clone, Copy, Debug)]
pub struct EdgeRec {
    pub geom: u32,
    pub trv: f32,
    pub len: f32,
    pub fm: f32,
    pub tm: f32,
    pub from_off: f32,
    pub to_off: f32,
    pub from_node: i32,
    pub to_node: i32,
    pub wk: u8,
    pub ow: u8,
}

/// Une trace : ses arêtes sont `edges[lo..hi]`.
#[derive(Clone, Copy, Debug)]
pub struct TrackRec {
    pub team: usize,
    pub ty: u8,
    pub lo: usize,
    pub hi: usize,
}

#[derive(Clone, Debug, Default)]
pub struct TeamRows {
    pub id: u64,
    pub rows: u64,
    pub excluded: u64,
    /// Document absent ou illisible.
    pub unreadable: u64,
}

#[derive(Debug, Default)]
pub struct Ingest {
    pub teams: Vec<TeamRows>,
    pub tracks: Vec<TrackRec>,
    pub edges: Vec<EdgeRec>,
    pub geoms: Vec<Box<str>>,
    pub rows: u64,
    /// Lignes ignorées (champ d'équipe illisible ou équipe non demandée).
    pub skipped_rows: u64,
    pub export_xxh3: u64,
}

const BATCH: usize = 48;

/// Lit l'export (gzip, CSV) en lots analysés en parallèle ; ordre des lignes conservé.
pub fn ingest(path: &Path, wanted: &[u64]) -> anyhow::Result<Ingest> {
    let file = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    let gz = flate2::bufread::MultiGzDecoder::new(BufReader::with_capacity(1 << 20, file));
    let mut csv = CsvReader::new(BufReader::with_capacity(1 << 20, gz));
    let mut fields: Vec<Vec<u8>> = Vec::new();
    anyhow::ensure!(csv.read_record(&mut fields)?, "export vide");
    let cols = Columns::from_header(&fields)?;

    let mut ing = Ingest::default();
    let mut team_index: FxHashMap<u64, usize> = FxHashMap::default();
    let mut dedup: FxHashMap<(u64, u32), u32> = FxHashMap::default();
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
            let ti = *team_index.entry(row.team_id).or_insert_with(|| {
                ing.teams.push(TeamRows { id: row.team_id, ..Default::default() });
                ing.teams.len() - 1
            });
            ing.teams[ti].rows += 1;
            if row.excluded {
                ing.teams[ti].excluded += 1;
                continue;
            }
            let Some(edges) = row.edges else {
                ing.teams[ti].unreadable += 1;
                continue;
            };
            let lo = ing.edges.len();
            for e in edges {
                let key = (xxh3_64(e.g.as_bytes()), e.g.len() as u32);
                let geom = *dedup.entry(key).or_insert_with(|| {
                    ing.geoms.push(e.g.into_boxed_str());
                    (ing.geoms.len() - 1) as u32
                });
                ing.edges.push(EdgeRec {
                    geom,
                    trv: e.trv,
                    len: e.len,
                    fm: e.fm,
                    tm: e.tm,
                    from_off: e.from_off,
                    to_off: e.to_off,
                    from_node: e.from_node,
                    to_node: e.to_node,
                    wk: e.wk,
                    ow: e.ow,
                });
            }
            ing.tracks.push(TrackRec { team: ti, ty: row.ty, lo, hi: ing.edges.len() });
        }
    }
    drop(csv);
    // Les octets du fichier lu (compressé) : empreinte de l'export.
    ing.export_xxh3 = hash_file(path)?;
    Ok(ing)
}

fn hash_file(path: &Path) -> anyhow::Result<u64> {
    let mut h = xxhash_rust::xxh3::Xxh3::new();
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Inside,
    Outside,
    /// Une partie des sommets dans le polygone, une partie dehors.
    Border,
}

#[derive(Debug)]
pub enum Outcome {
    /// Polyligne illisible ou de moins de deux points.
    Invalid,
    /// Non tentée : hors de la région ou à cheval sur sa limite.
    NotTried,
    Unmatched(Fail),
    Matched(Chain),
}

#[derive(Debug)]
pub struct Job {
    pub region: Region,
    /// Longueur de `g` en mètres (repli quand `trv` manque).
    pub glen: f32,
    pub outcome: Outcome,
}

/// Apparie chaque géométrie distincte, en parallèle.
pub fn run_jobs(
    gf: &MappedGraph,
    idx: &SegIndex,
    poly: &Poly,
    geoms: &[Box<str>],
    trv_of: &[f32],
) -> Vec<Job> {
    geoms
        .par_iter()
        .enumerate()
        .map(|(i, g)| {
            let Some(pts) = decode_polyline6(g).filter(|p| p.len() >= 2) else {
                return Job { region: Region::Inside, glen: 0.0, outcome: Outcome::Invalid };
            };
            let frame = Frame::new(pts[0][0], pts[0][1]);
            let glen = polyline_len(&pts.iter().map(|p| frame.xy(p[0], p[1])).collect::<Vec<_>>());
            let inside = pts.iter().filter(|p| poly.contains(p[1], p[0])).count();
            let region = if inside == pts.len() {
                Region::Inside
            } else if inside == 0 {
                Region::Outside
            } else {
                Region::Border
            };
            if region != Region::Inside {
                return Job { region, glen: glen as f32, outcome: Outcome::NotTried };
            }
            let trv = f64::from(trv_of[i]);
            let outcome = match match_geometry(gf, idx, &pts, trv) {
                Ok(c) => Outcome::Matched(c),
                Err(f) => Outcome::Unmatched(f),
            };
            Job { region, glen: glen as f32, outcome }
        })
        .collect()
}

/// `trv` annoncé pour chaque géométrie (celui de sa première apparition ; `NaN` si absent).
pub fn trv_per_geom(ing: &Ingest) -> Vec<f32> {
    let mut v = vec![f32::NAN; ing.geoms.len()];
    let mut set = vec![false; ing.geoms.len()];
    for e in &ing.edges {
        let g = e.geom as usize;
        if !set[g] {
            set[g] = true;
            v[g] = e.trv;
        }
    }
    v
}
