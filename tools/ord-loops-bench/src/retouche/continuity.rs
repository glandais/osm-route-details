//! Mesures de continuité d'une trace et de l'emplacement de ses coutures (note : `ord-retouche-lot0.md` §2.2 à §2.5).
//!
//! La marche est celle d'aujourd'hui : `track_match`, puis `Walk::from_pieces`, puis `Walk::trace`. **Aucun id, aucune
//! coordonnée** dans ce qui sort : la lettre de l'équipe, le rang de la trace dans l'ordre de l'export et des nombres.

use ord_graph::mmap::MappedGraph;
use ord_graph_match::doc::TYPES;
use ord_graph_match::geo::decode_polyline6;
use ord_graph_match::job::Job;
use ord_graph_match::walk::{Join, Walk};
use serde::Serialize;

use super::posed::{self, SeamOpts, ViewLookup};
use super::project::{self, DocLine, SeamPoint};
use crate::likeness::export::{self, ExportTrack, Ingest, Refusal, RetainParams};
use crate::likeness::shape::{quantile_sorted, sorted};

/// Quantiles d'un échantillon (type 7, comme les autres relevés).
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
pub struct Q {
    pub n: usize,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
    pub max: f64,
}

impl Q {
    pub fn of(v: &[f64]) -> Option<Self> {
        let s = sorted(v);
        Some(Self {
            n: s.len(),
            p10: quantile_sorted(&s, 0.1)?,
            p50: quantile_sorted(&s, 0.5)?,
            p90: quantile_sorted(&s, 0.9)?,
            max: *s.last()?,
        })
    }
}

/// Forme d'une trace : boucle si l'écart départ–arrivée de la marche est inférieur à `loop_gap` de sa polyligne (la
/// règle de `judge_pieces`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Loop,
    OneWay,
}

/// Une trace mesurée. `#[serde(skip)]` : listes gardées pour les agrégats, pas écrites.
#[derive(Clone, Debug, Serialize)]
pub struct TrackRec {
    pub team: String,
    /// Rang de la trace dans l'ordre de l'export, pour son équipe.
    pub rank: u32,
    pub ty: &'static str,
    pub shape: Shape,
    pub excluded: bool,
    /// Retenue par le banc actuel (boucle, aucune cause de refus).
    pub retained: bool,
    /// Retenue pour une boucle **ou** un aller simple (la même règle sans le test de boucle).
    pub retained_any: bool,
    /// Première cause de refus du banc (exclusion comprise) ; `None` si retenue.
    pub refusal: Option<&'static str>,
    pub doc_m: f64,
    pub matched_share: f64,
    pub unmatched_doc_m: f64,
    pub outside_m: f64,
    pub legs: usize,
    pub runs: usize,
    pub holes: usize,
    pub touches: usize,
    pub hole_chord_m: f64,
    pub max_hole_m: f64,
    pub holes_per_100km: f64,
    pub run_m: Option<Q>,
    pub continuous: bool,
    /// Coutures valides (marge des deux côtés) et coutures valides pour `move_end` (marge avant seulement).
    pub seams: usize,
    pub seams_before: usize,
    pub seams_per_km: f64,
    pub order_anomalies: usize,
    pub proj_p50_m: Option<f64>,
    pub proj_max_m: Option<f64>,
    pub drift: f64,
    /// Longueur de la polyligne du document, m.
    pub doc_len_m: f64,
    /// `None` : aucune couture (distance infinie).
    pub max_dist_to_seam_m: Option<f64>,
    pub max_dist_inner_m: Option<f64>,
    pub last_seam_from_end_m: Option<f64>,
    pub max_gap_m: f64,
    /// La même mesure sur l'abscisse de marche (la corde des trous en moins).
    pub max_dist_to_seam_walk_m: Option<f64>,
    #[serde(skip)]
    pub run_lens: Vec<f64>,
    #[serde(skip)]
    pub hole_chords: Vec<f64>,
}

/// Ce que devient une ligne de l'export.
#[derive(Clone, Debug)]
pub enum Outcome {
    Unreadable {
        ty: &'static str,
    },
    /// Document lu mais aucun morceau apparié (ou marche vide).
    Empty {
        ty: &'static str,
        doc_m: f64,
    },
    Measured(Box<TrackRec>),
}

pub fn type_name(ty: u8) -> &'static str {
    TYPES[usize::from(ty).min(TYPES.len() - 1)]
}

pub fn refusal_name(r: Refusal) -> &'static str {
    match r {
        Refusal::Excluded => "Excluded",
        Refusal::Unreadable => "Unreadable",
        Refusal::Type => "Type",
        Refusal::Matched => "Matched",
        Refusal::Empty => "Empty",
        Refusal::Hole => "Hole",
        Refusal::Short => "Short",
        Refusal::NotLoop => "NotLoop",
        Refusal::Long => "Long",
    }
}

pub struct Ctx<'a> {
    pub gf: &'a MappedGraph,
    pub ing: &'a Ingest,
    pub jobs: &'a [Job],
    /// Vues `[road, gravel]`.
    pub views: [&'a dyn ViewLookup; 2],
    pub opts: SeamOpts,
    pub proj_tol_m: f64,
    pub retain: RetainParams,
}

/// La vue d'un type : ROAD et MIXED → `road` ; GRAVEL et MTB → `gravel`.
fn view_index(ty: u8) -> usize {
    usize::from(ty == 1 || ty == 2)
}

pub fn measure(ctx: &Ctx<'_>, idx: usize, rank: u32, letter: &str) -> Outcome {
    let t: &ExportTrack = &ctx.ing.tracks[idx];
    let ty_name = type_name(t.ty);
    if !t.readable {
        return Outcome::Unreadable { ty: ty_name };
    }
    let m = export::track_match(ctx.jobs, t);
    if m.pieces.is_empty() {
        return Outcome::Empty { ty: ty_name, doc_m: m.total_m };
    }
    let walk = Walk::from_pieces(ctx.gf, &m.pieces);
    if walk.legs.is_empty() {
        return Outcome::Empty { ty: ty_name, doc_m: m.total_m };
    }
    let trace = walk.trace(ctx.gf);
    // Forme et cause de refus : la règle du banc, exclusion mise de côté pour lire les autres causes.
    let gap = {
        let (a, b) = (trace.pts[0], trace.pts[trace.pts.len() - 1]);
        (a[0] - b[0]).hypot(a[1] - b[1])
    };
    let shape = if gap < ctx.retain.loop_gap * trace.l_poly { Shape::Loop } else { Shape::OneWay };
    let cause =
        export::judge_pieces(ctx.gf, &ctx.retain, t.ty, false, true, m.total_m, m.matched_m, &m.pieces).err();
    let refusal = if t.excluded { Some(Refusal::Excluded) } else { cause };
    let retained = refusal.is_none();
    let retained_any = !t.excluded && matches!(cause, None | Some(Refusal::NotLoop));

    // Jonctions.
    let holes: Vec<f64> =
        walk.joins.iter().filter_map(|j| if let Join::Hole(d) = j { Some(*d) } else { None }).collect();
    let touches = walk.joins.iter().filter(|j| matches!(j, Join::Touch)).count();
    let runs = posed::runs(&walk, false);
    let run_lens: Vec<f64> = runs.iter().map(|r| trace.leg_m[r.clone()].iter().sum()).collect();

    // Polyligne du document et coutures.
    let line = DocLine::from_parts(
        t.edges.iter().map(|e| decode_polyline6(&ctx.ing.keys[e.key as usize].0).unwrap_or_default()),
    );
    let doc_len = line.len_m();
    let cands =
        posed::candidates(ctx.gf, &walk, &trace, ctx.views[view_index(t.ty)], ctx.opts.touch_continues);
    let mut prev_join = 0usize;
    let points: Vec<SeamPoint> = cands
        .iter()
        .map(|c| {
            let p = ctx.gf.node_pos(c.node);
            let chords: f64 = walk.joins[prev_join..c.join]
                .iter()
                .filter_map(|j| if let Join::Hole(d) = j { Some(*d) } else { None })
                .sum();
            prev_join = c.join;
            SeamPoint {
                latlon: (f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7),
                a_m: c.a_m,
                slack_m: 3.0 * chords,
            }
        })
        .collect();
    let proj = project::project(&line, &points, ctx.proj_tol_m);
    let pick = |valid: &dyn Fn(&posed::Seam) -> bool| -> Vec<usize> {
        (0..cands.len()).filter(|&i| valid(&cands[i])).collect()
    };
    let strict = pick(&|s| s.valid(ctx.opts.margin_m));
    let before = pick(&|s| s.valid_before(ctx.opts.margin_m));
    let s_strict: Vec<f64> = strict.iter().map(|&i| proj.s[i]).collect();
    let s_before: Vec<f64> = before.iter().map(|&i| proj.s[i]).collect();
    let a_strict: Vec<f64> = strict.iter().map(|&i| cands[i].a_m).collect();

    let unmatched = m.total_m - m.matched_m - m.outside_m;
    let leg_sum: f64 = trace.leg_m.iter().sum();
    let drift = if m.total_m > 0.0 { (leg_sum + unmatched - m.total_m).abs() / m.total_m } else { 0.0 };
    let proj_q = Q::of(&proj.dist_m);
    Outcome::Measured(Box::new(TrackRec {
        team: letter.to_string(),
        rank,
        ty: ty_name,
        shape,
        excluded: t.excluded,
        retained: retained && shape == Shape::Loop,
        retained_any,
        refusal: refusal.map(refusal_name),
        doc_m: m.total_m,
        matched_share: m.matched_share(),
        unmatched_doc_m: unmatched,
        outside_m: m.outside_m,
        legs: walk.legs.len(),
        runs: runs.len(),
        holes: holes.len(),
        touches,
        hole_chord_m: holes.iter().sum::<f64>() + 0.0,
        max_hole_m: trace.max_hole_m,
        holes_per_100km: if m.total_m > 0.0 { holes.len() as f64 / m.total_m * 1e5 } else { 0.0 },
        run_m: Q::of(&run_lens),
        continuous: walk.is_continuous(),
        seams: strict.len(),
        seams_before: before.len(),
        seams_per_km: if doc_len > 0.0 { strict.len() as f64 / (doc_len / 1000.0) } else { 0.0 },
        order_anomalies: proj.anomalies,
        proj_p50_m: proj_q.map(|q| q.p50),
        proj_max_m: proj_q.map(|q| q.max),
        drift,
        doc_len_m: doc_len,
        max_dist_to_seam_m: project::max_dist_to_seam(&s_strict, 0.0, doc_len),
        max_dist_inner_m: project::max_dist_to_seam(&s_strict, 0.05 * doc_len, 0.95 * doc_len),
        last_seam_from_end_m: s_before.last().map(|s| (doc_len - s).max(0.0)),
        max_gap_m: project::max_gap(&s_strict),
        max_dist_to_seam_walk_m: project::max_dist_to_seam(&a_strict, 0.0, trace.l_poly),
        run_lens,
        hole_chords: holes,
    }))
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn rec(dist: Option<f64>) -> TrackRec {
        TrackRec {
            team: "A".into(),
            rank: 0,
            ty: "ROAD",
            shape: Shape::Loop,
            excluded: false,
            retained: true,
            retained_any: true,
            refusal: None,
            doc_m: 10_000.0,
            matched_share: 1.0,
            unmatched_doc_m: 0.0,
            outside_m: 0.0,
            legs: 10,
            runs: 1,
            holes: 0,
            touches: 0,
            hole_chord_m: 0.0,
            max_hole_m: 0.0,
            holes_per_100km: 0.0,
            run_m: None,
            continuous: true,
            seams: 3,
            seams_before: 3,
            seams_per_km: 0.3,
            order_anomalies: 0,
            proj_p50_m: Some(2.0),
            proj_max_m: Some(5.0),
            drift: 0.0,
            doc_len_m: 10_000.0,
            max_dist_to_seam_m: dist,
            max_dist_inner_m: dist,
            last_seam_from_end_m: dist,
            max_gap_m: 0.0,
            max_dist_to_seam_walk_m: dist,
            run_lens: vec![10_000.0],
            hole_chords: vec![],
        }
    }

    #[test]
    fn a_track_without_seam_is_counted_and_never_within_a_distance() {
        let (a, b, c) = (rec(Some(400.0)), rec(Some(1500.0)), rec(None));
        let agg = super::super::out::aggregate(&[&a, &b, &c]);
        assert_eq!((agg.n, agg.no_seam.map(|s| s.k)), (3, Some(1)));
        assert_eq!(agg.within_500.map(|s| s.k), Some(1));
        assert_eq!(agg.within_1000.map(|s| s.k), Some(1));
        assert_eq!(agg.within_2000.map(|s| s.k), Some(2));
        // Les quantiles ne portent que sur les traces qui ont une couture.
        assert_eq!(agg.max_dist_to_seam_m.map(|q| q.n), Some(2));
    }

    #[test]
    fn ingest_keeps_every_occurrence_of_an_edge_walked_twice() {
        // Un document dont la même arête est parcourue deux fois : deux `TeamEdge`, une seule clé.
        let doc = r#"{"edges":[{"g":"_p~iF~ps|U_ulLnnqC","trv":100.0,"wk":"road"},{"g":"_p~iF~ps|U_ulLnnqC","trv":100.0,"wk":"road"}]}"#;
        let csv = format!(
            "team_id,track_id,surface_type,excluded_from_generator,network_edges\n7,t1,ROAD,f,\"{}\"\n",
            doc.replace('"', "\"\"")
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.csv.gz");
        let mut gz =
            flate2::write::GzEncoder::new(std::fs::File::create(&path).unwrap(), flate2::Compression::fast());
        gz.write_all(csv.as_bytes()).unwrap();
        gz.finish().unwrap();
        let ing = export::ingest(&path, &[]).unwrap();
        assert_eq!(ing.tracks.len(), 1);
        assert_eq!(ing.tracks[0].edges.len(), 2);
        assert_eq!(ing.keys.len(), 1);
        assert_eq!(ing.tracks[0].edges[0].key, ing.tracks[0].edges[1].key);
    }
}
