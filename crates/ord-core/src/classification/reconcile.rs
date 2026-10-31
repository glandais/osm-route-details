//! Réconciliation OSM / heuristiques (T03 §7.1). Au MVP, `detected` est vide : chaque passage OSM
//! reçoit un verdict `osm`. Le chemin de code est celui de la V1, qui y passera les détections des
//! heuristiques (terrain mesuré).

use crate::typology::{PassageId, Structure, StructureId, StructureKind, StructurePassage};

use super::StructureSource;

/// Ouvrage détecté par une heuristique (V1), en distance le long du parcours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectedStructure {
    pub id: u32,
    pub kind: StructureKind,
    pub from_m: f64,
    pub to_m: f64,
}

/// Ouvrage visé par un verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupRef {
    Osm(StructureId),
    Heuristic(u32),
}

/// Décision par passage ou par détection : nature, plage et source de l'ouvrage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StructureVerdict {
    pub target: GroupRef,
    pub kind: StructureKind,
    pub from_m: f64,
    pub to_m: f64,
    pub passage: Option<PassageId>,
    pub source: StructureSource,
}

/// Recouvrement minimal d'une détection de même nature pour confirmer un passage OSM
/// (`confirm_overlap`, V1) [à calibrer].
pub const CONFIRM_OVERLAP: f64 = 0.5;

/// Un verdict par passage OSM (nature de l'élément au milieu du passage) : `osm_heuristic` si une
/// détection de même nature le recouvre à `CONFIRM_OVERLAP` de la plus courte des deux plages,
/// `osm` sinon. Les détections hors de tout passage (V1, `heuristic`) ne sont pas encore produites.
pub fn reconcile(
    passages: &[StructurePassage],
    structures: &[Structure],
    detected: &[DetectedStructure],
) -> Vec<StructureVerdict> {
    let mut out: Vec<StructureVerdict> = passages
        .iter()
        .map(|p| {
            let kind = structures
                .iter()
                .find(|s| s.id == p.structure)
                .map_or(StructureKind::Bridge, |s| s.kind_at(p.chainage_at(0.5 * (p.from_m + p.to_m))));
            let confirmed = detected.iter().any(|h| {
                let inter = h.to_m.min(p.to_m) - h.from_m.max(p.from_m);
                let shortest = (h.to_m - h.from_m).min(p.to_m - p.from_m);
                h.kind == kind && shortest > 0.0 && inter / shortest >= CONFIRM_OVERLAP
            });
            StructureVerdict {
                target: GroupRef::Osm(p.structure),
                kind,
                from_m: p.from_m,
                to_m: p.to_m,
                passage: Some(p.id),
                source: if confirmed { StructureSource::OsmHeuristic } else { StructureSource::Osm },
            }
        })
        .collect();
    out.sort_by(|a, b| a.from_m.total_cmp(&b.from_m));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passage(id: u32, from_m: f64, to_m: f64) -> StructurePassage {
        StructurePassage {
            id,
            structure: 7,
            from_m,
            to_m,
            chainage: vec![(from_m, 0.0), (to_m, to_m - from_m)],
            doubts: Vec::new(),
        }
    }

    #[test]
    fn without_detection_every_passage_is_osm() {
        let v = reconcile(&[passage(1, 100.0, 200.0), passage(0, 10.0, 20.0)], &[], &[]);
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|v| v.source == StructureSource::Osm && v.target == GroupRef::Osm(7)));
        assert_eq!(v[0].passage, Some(0), "triés par distance");
    }

    #[test]
    fn overlapping_detection_of_same_kind_confirms() {
        let h = DetectedStructure { id: 0, kind: StructureKind::Bridge, from_m: 120.0, to_m: 260.0 };
        let v = reconcile(&[passage(0, 100.0, 200.0)], &[], &[h]);
        assert_eq!(v[0].source, StructureSource::OsmHeuristic);
        let t = DetectedStructure { kind: StructureKind::Tunnel, ..h };
        assert_eq!(reconcile(&[passage(0, 100.0, 200.0)], &[], &[t])[0].source, StructureSource::Osm);
    }
}
