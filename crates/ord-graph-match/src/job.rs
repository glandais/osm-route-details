//! L'appariement d'une géométrie distincte : région, longueur, résultat. Commun à l'outil (en parallèle, sur tout
//! l'export) et au service (une à une, mémoïsé).

use ord_graph::mmap::MappedGraph;
use ord_graph::util::Poly;

use crate::geo::{decode_polyline6, polyline_len, Frame};
use crate::index::Near;
use crate::matcher::{match_geometry, Chain, Fail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Inside,
    Outside,
    /// Une partie des sommets dans le polygone, une partie dehors.
    Border,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    /// Polyligne illisible ou de moins de deux points.
    Invalid,
    /// Non tentée : hors de la région ou à cheval sur sa limite.
    NotTried,
    Unmatched(Fail),
    Matched(Chain),
}

#[derive(Clone, Debug)]
pub struct Job {
    pub region: Region,
    /// Longueur de `g` en mètres (repli quand `trv` manque).
    pub glen: f32,
    pub outcome: Outcome,
}

/// Apparie la polyligne `g` (précision 6) ; `trv` : mètres parcourus annoncés (`NaN` si absent).
pub fn match_job(gf: &MappedGraph, idx: &dyn Near, poly: &Poly, g: &str, trv: f32) -> Job {
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
    let outcome = match match_geometry(gf, idx, &pts, f64::from(trv)) {
        Ok(c) => Outcome::Matched(c),
        Err(f) => Outcome::Unmatched(f),
    };
    Job { region, glen: glen as f32, outcome }
}
