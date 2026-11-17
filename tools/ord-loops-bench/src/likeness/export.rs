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
/// lisibles, exclues comprises : `shapes.py` ne filtre pas). Aligné sur `Ingest::tracks`.
pub fn raw_legacy(ing: &Ingest) -> Vec<Option<super::shape::RawRow>> {
    use ord_graph_match::geo::decode_polyline6;
    ing.tracks
        .par_iter()
        .map(|t| {
            if !t.readable {
                return None;
            }
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

// --- Appariement et retenue -------------------------------------------------------------------------------------

use ord_graph::mmap::MappedGraph;
use ord_graph::util::Poly;
use ord_graph_match::agg::{verdict, Verdict};
use ord_graph_match::index::SegIndex;
use ord_graph_match::job::{match_job, Job};
use ord_graph_match::matcher::Piece;
use serde::Serialize;

use super::walk::{Trace, Walk};

/// Apparie chaque clé distincte, en parallèle (`match_job` est pure : même résultat que la mémoïsation du dépôt).
pub fn match_keys(gf: &MappedGraph, seg: &SegIndex, poly: &Poly, ing: &Ingest) -> Vec<Job> {
    ing.keys.par_iter().map(|(g, trv)| match_job(gf, seg, poly, g, *trv)).collect()
}

/// Ce que le dépôt voit d'une trace : mètres annoncés, retenus, hors région, et la suite appariée dans l'ordre.
#[derive(Debug, Default)]
pub struct TrackMatch {
    pub total_m: f64,
    pub matched_m: f64,
    pub outside_m: f64,
    pub pieces: Vec<Piece>,
}

impl TrackMatch {
    /// Part appariée, au sens du dépôt, trace par trace.
    pub fn matched_share(&self) -> f64 {
        if self.total_m > 0.0 {
            self.matched_m / self.total_m
        } else {
            0.0
        }
    }
}

/// La suite appariée d'une trace : pour chaque arête, `trv` annoncé (repli sur la longueur de la géométrie) ;
/// `Counted` ajoute ses morceaux dans l'ordre, `Outside` et `Fail` n'ajoutent rien (un trou).
pub fn track_match(jobs: &[Job], t: &ExportTrack) -> TrackMatch {
    let mut m = TrackMatch::default();
    for e in &t.edges {
        let job = &jobs[e.key as usize];
        let trv = f64::from(if e.trv.is_nan() { job.glen } else { e.trv });
        m.total_m += trv;
        match verdict(e.wk, job) {
            Verdict::Counted(chain) => {
                m.matched_m += trv;
                m.pieces.extend_from_slice(&chain.pieces);
            }
            Verdict::Outside => m.outside_m += trv,
            Verdict::Fail { .. } => {}
        }
    }
    m
}

/// Pourquoi une trace n'est pas retenue, dans l'ordre où c'est jugé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Excluded,
    Unreadable,
    Type,
    Matched,
    Empty,
    Hole,
    Short,
    NotLoop,
    Long,
}

/// Les décomptes d'une équipe (aucun id).
#[derive(Clone, Debug, Default, Serialize)]
pub struct RetentionCounts {
    pub rows: usize,
    pub excluded: usize,
    pub unreadable: usize,
    pub type_refused: usize,
    pub matched_share_below: usize,
    pub empty: usize,
    pub hole_too_long: usize,
    pub below_min_km: usize,
    pub not_a_loop: usize,
    pub above_max_km: usize,
    pub retained: usize,
    /// Après `--limit`.
    pub kept: usize,
    /// Boucles retenues dont la marche est continue (toutes jonctions au même nœud).
    pub contiguous: usize,
    pub holdout: usize,
}

/// Une boucle réelle retenue.
#[derive(Debug)]
pub struct Loop {
    pub team: usize,
    /// Rang parmi les boucles retenues de l'équipe, dans l'ordre de l'export.
    pub i: u32,
    /// Indice dans `Ingest::tracks`.
    pub track: usize,
    pub id_hash: u64,
    pub ty: u8,
    pub walk: Walk,
    pub trace: Trace,
    pub matched_share: f64,
}

/// Paramètres de retenue.
#[derive(Clone, Copy, Debug)]
pub struct RetainParams {
    pub min_matched: f64,
    pub max_hole_m: f64,
    pub min_km: f64,
    pub max_km: Option<f64>,
    pub loop_gap: f64,
}

/// Juge une trace (§11.3) : la boucle retenue (sans rang) ou la cause du refus.
pub fn judge(
    gf: &MappedGraph,
    ing: &Ingest,
    jobs: &[Job],
    idx: usize,
    p: &RetainParams,
) -> Result<Loop, Refusal> {
    let t = &ing.tracks[idx];
    if t.excluded {
        return Err(Refusal::Excluded);
    }
    if !t.readable {
        return Err(Refusal::Unreadable);
    }
    if t.ty > 2 {
        return Err(Refusal::Type);
    }
    let m = track_match(jobs, t);
    if m.matched_share() < p.min_matched {
        return Err(Refusal::Matched);
    }
    if m.pieces.is_empty() {
        return Err(Refusal::Empty);
    }
    let walk = Walk::from_pieces(gf, &m.pieces);
    if walk.legs.is_empty() {
        return Err(Refusal::Empty);
    }
    let trace = walk.trace(gf);
    if trace.max_hole_m > p.max_hole_m {
        return Err(Refusal::Hole);
    }
    if trace.d_graph < p.min_km * 1000.0 {
        return Err(Refusal::Short);
    }
    let gap = super::shape::dist(trace.pts[0], trace.pts[trace.pts.len() - 1]);
    if gap >= p.loop_gap * trace.l_poly {
        return Err(Refusal::NotLoop);
    }
    if p.max_km.is_some_and(|k| trace.d_graph > k * 1000.0) {
        return Err(Refusal::Long);
    }
    Ok(Loop {
        team: t.team,
        i: 0,
        track: idx,
        id_hash: t.id_hash,
        ty: t.ty,
        walk,
        trace,
        matched_share: m.matched_share(),
    })
}

/// Retient les boucles de toutes les traces, en parallèle, puis range par équipe dans l'ordre de l'export et applique
/// `--limit`. Rend les boucles et, par équipe (indice de `Ingest::teams`), leurs décomptes.
pub fn retain(
    gf: &MappedGraph,
    ing: &Ingest,
    jobs: &[Job],
    p: &RetainParams,
    limit: Option<usize>,
) -> (Vec<Loop>, Vec<RetentionCounts>) {
    let judged: Vec<Result<Loop, Refusal>> =
        (0..ing.tracks.len()).into_par_iter().map(|i| judge(gf, ing, jobs, i, p)).collect();
    let mut counts = vec![RetentionCounts::default(); ing.teams.len()];
    let mut loops: Vec<Loop> = Vec::new();
    let mut next = vec![0u32; ing.teams.len()];
    for (idx, j) in judged.into_iter().enumerate() {
        let team = ing.tracks[idx].team;
        let c = &mut counts[team];
        c.rows += 1;
        match j {
            Err(Refusal::Excluded) => c.excluded += 1,
            Err(Refusal::Unreadable) => c.unreadable += 1,
            Err(Refusal::Type) => c.type_refused += 1,
            Err(Refusal::Matched) => c.matched_share_below += 1,
            Err(Refusal::Empty) => c.empty += 1,
            Err(Refusal::Hole) => c.hole_too_long += 1,
            Err(Refusal::Short) => c.below_min_km += 1,
            Err(Refusal::NotLoop) => c.not_a_loop += 1,
            Err(Refusal::Long) => c.above_max_km += 1,
            Ok(mut l) => {
                c.retained += 1;
                if limit.is_some_and(|n| next[team] as usize >= n) {
                    continue;
                }
                l.i = next[team];
                next[team] += 1;
                c.kept += 1;
                c.contiguous += usize::from(l.walk.is_continuous());
                loops.push(l);
            }
        }
    }
    (loops, counts)
}
