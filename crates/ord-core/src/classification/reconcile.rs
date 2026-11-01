//! Réconciliation OSM / heuristiques (T03 §7.1, T04 §4.3). Sans détection (MVP, ou heuristiques
//! inactives), chaque passage OSM reçoit un verdict `osm`. En V1, les détections des heuristiques
//! (terrain mesuré) confirment un passage OSM de même nature (`osm_heuristic`), sont écartées sur
//! un ouvrage OSM de nature opposée, et deviennent des ouvrages `heuristic` hors de tout ouvrage OSM
//! (ou pour leur part qui dépasse un ouvrage OSM de plus de `UNTAGGED_MIN_M`).

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

/// Part d'une détection qui dépasse un ouvrage OSM au-delà de laquelle elle devient un ouvrage
/// `heuristic` contigu (`untagged_min_m`, `deckMinRunM` de vcyclist).
pub const UNTAGGED_MIN_M: f64 = 40.0;

/// Un verdict par passage OSM (nature de l'élément au milieu du passage) : `osm_heuristic` si une
/// détection de même nature le recouvre à `CONFIRM_OVERLAP` de la plus courte des deux plages,
/// `osm` sinon. Puis un verdict `heuristic` (sans passage) par détection qui ne touche aucun
/// passage OSM, ou par part d'une détection qui dépasse les passages OSM de même nature qu'elle
/// touche de plus de `UNTAGGED_MIN_M` ; une détection qui touche un passage OSM de nature opposée
/// est écartée (OSM prime). Triés par distance.
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
    let osm: Vec<(f64, f64, StructureKind)> = out.iter().map(|v| (v.from_m, v.to_m, v.kind)).collect();
    for h in detected {
        let touching: Vec<&(f64, f64, StructureKind)> =
            osm.iter().filter(|o| o.1 > h.from_m && o.0 < h.to_m).collect();
        if touching.iter().any(|o| o.2 != h.kind) {
            continue;
        }
        // Parts de la détection hors des passages OSM qu'elle touche.
        let mut pieces: Vec<(f64, f64)> = vec![(h.from_m, h.to_m)];
        for o in &touching {
            pieces = pieces
                .into_iter()
                .flat_map(|(a, b)| {
                    let mut v = Vec::new();
                    if o.0 > a {
                        v.push((a, o.0.min(b)));
                    }
                    if o.1 < b {
                        v.push((o.1.max(a), b));
                    }
                    v
                })
                .filter(|(a, b)| b > a)
                .collect();
        }
        for (a, b) in pieces {
            if !touching.is_empty() && b - a <= UNTAGGED_MIN_M {
                continue;
            }
            out.push(StructureVerdict {
                target: GroupRef::Heuristic(h.id),
                kind: h.kind,
                from_m: a,
                to_m: b,
                passage: None,
                source: StructureSource::Heuristic,
            });
        }
    }
    out.sort_by(|a, b| a.from_m.total_cmp(&b.from_m).then(a.to_m.total_cmp(&b.to_m)));
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
        let v = reconcile(&[passage(0, 100.0, 200.0)], &[], &[t]);
        assert_eq!(v.len(), 1, "nature opposée : écartée, OSM prime");
        assert_eq!(v[0].source, StructureSource::Osm);
        // Dépassement de 60 m (> 40 m) : la part au-delà devient un ouvrage heuristique contigu.
        let v = reconcile(&[passage(0, 100.0, 200.0)], &[], &[h]);
        assert_eq!(v.len(), 2, "{v:?}");
        assert_eq!((v[1].from_m, v[1].to_m, v[1].source), (200.0, 260.0, StructureSource::Heuristic));
        assert_eq!(v[1].passage, None);
        // Dépassement de 20 m : ignoré.
        let short = DetectedStructure { to_m: 220.0, ..h };
        assert_eq!(reconcile(&[passage(0, 100.0, 200.0)], &[], &[short]).len(), 1);
    }

    #[test]
    fn a_detection_off_any_osm_structure_is_heuristic() {
        let h = DetectedStructure { id: 3, kind: StructureKind::Bridge, from_m: 500.0, to_m: 530.0 };
        let v = reconcile(&[passage(0, 100.0, 200.0)], &[], &[h]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].target, GroupRef::Heuristic(3));
        assert_eq!(v[1].source, StructureSource::Heuristic);
    }
}
