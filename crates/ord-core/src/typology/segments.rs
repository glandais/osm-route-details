//! Segments homogènes du contrat (T03 §8). Les verdicts de `reconcile` fixent la source d'un
//! ouvrage : au MVP, tout ouvrage est `osm` ; un verdict `osm_heuristic` (V1) change seulement la
//! source sur sa plage.

use ord_domain::route::PreparedRoute;

use super::graph::{ClassId, EdgeClass, LineId};
use super::{RunMatch, TypedRun};
use crate::classification::{
    Classified, StructureClass, StructureSource, StructureVerdict, SurfaceClass, SurfaceSource, WayKind,
};

#[derive(Clone, Debug, PartialEq)]
pub struct TypedSegment {
    pub from_m: f64,
    pub to_m: f64,
    pub from_index: u32,
    pub to_index: u32,
    pub structure: StructureClass,
    pub structure_source: Option<StructureSource>,
    pub surface_class: SurfaceClass,
    pub surface_source: SurfaceSource,
    pub way_kind: WayKind,
    pub matched: bool,
    /// Route fermée au vélo suivie (état « hors graphe », T03 §5.3) : `matched = false`, mais
    /// `structure`, revêtement et `way_kind` de cette route ; avertissement `bicycle_forbidden`.
    pub off_graph: bool,
    /// Élément d'ouvrage (deux ponts contigus restent deux segments).
    pub element: Option<LineId>,
    /// Classe du premier tronçon (attributs bruts de `debug`).
    pub class: Option<ClassId>,
}

impl TypedSegment {
    pub fn len(&self) -> f64 {
        self.to_m - self.from_m
    }

    pub fn is_empty(&self) -> bool {
        self.len() <= 0.0
    }

    fn same_key(&self, o: &Self) -> bool {
        (
            self.structure,
            self.structure_source,
            self.surface_class,
            self.surface_source,
            self.way_kind,
            self.matched,
            self.off_graph,
            self.element,
        ) == (
            o.structure,
            o.structure_source,
            o.surface_class,
            o.surface_source,
            o.way_kind,
            o.matched,
            o.off_graph,
            o.element,
        )
    }

    /// Tronçon au sol apparié : seul cas absorbable, et seul voisin qui absorbe.
    fn ground_matched(&self) -> bool {
        self.matched && self.structure == StructureClass::Ground
    }
}

fn merge(segs: Vec<TypedSegment>) -> Vec<TypedSegment> {
    let mut out: Vec<TypedSegment> = Vec::with_capacity(segs.len());
    for s in segs {
        match out.last_mut() {
            Some(last) if last.same_key(&s) => last.to_m = s.to_m,
            _ => out.push(s),
        }
    }
    out
}

/// Segments contigus couvrant `[0, L]`, coupés sur chaque attribut du contrat et sur l'élément
/// d'ouvrage ; un tronçon au sol apparié de moins de `min_segment_m` est absorbé par son voisin au
/// sol apparié le plus long (jamais un ouvrage, jamais un changement de `matched`).
pub fn build_segments(
    runs: &[TypedRun],
    classes: &[EdgeClass],
    verdicts: &[StructureVerdict],
    route: &PreparedRoute,
    min_segment_m: f64,
) -> Vec<TypedSegment> {
    let raw: Vec<TypedSegment> = runs
        .iter()
        .map(|r| {
            let (c, matched, element, class) = match r.matched {
                RunMatch::Matched { class, element } => {
                    (classes[class as usize].classified, true, element, Some(class))
                }
                // Route fermée au vélo : sa typologie, sans appariement au graphe vélo.
                RunMatch::OffGraph { class } => {
                    (classes[class as usize].classified, false, None, Some(class))
                }
                RunMatch::Unmatched | RunMatch::NoData { .. } => (Classified::UNMATCHED, false, None, None),
            };
            let off_graph = matches!(r.matched, RunMatch::OffGraph { .. });
            let mid = 0.5 * (r.from_m + r.to_m);
            let source = verdicts
                .iter()
                .find(|v| v.from_m <= mid && mid <= v.to_m)
                .map_or(StructureSource::Osm, |v| v.source);
            let structure_source =
                matches!(c.structure, StructureClass::Bridge | StructureClass::Tunnel).then_some(source);
            TypedSegment {
                from_m: r.from_m,
                to_m: r.to_m,
                from_index: 0,
                to_index: 0,
                structure: if matched || off_graph { c.structure } else { StructureClass::Unknown },
                structure_source,
                surface_class: c.surface_class,
                surface_source: c.surface_source,
                way_kind: c.way_kind,
                matched,
                off_graph,
                element,
                class,
            }
        })
        .collect();
    let mut segs = merge(raw);
    loop {
        let short = (0..segs.len())
            .filter(|&i| segs[i].ground_matched() && segs[i].len() < min_segment_m)
            .filter(|&i| {
                (i > 0 && segs[i - 1].ground_matched())
                    || (i + 1 < segs.len() && segs[i + 1].ground_matched())
            })
            .min_by(|&a, &b| segs[a].len().total_cmp(&segs[b].len()).then(a.cmp(&b)));
        let Some(i) = short else { break };
        let prev = (i > 0 && segs[i - 1].ground_matched()).then(|| i - 1);
        let next = (i + 1 < segs.len() && segs[i + 1].ground_matched()).then_some(i + 1);
        let host = match (prev, next) {
            (Some(p), Some(n)) => {
                if segs[n].len() > segs[p].len() {
                    n
                } else {
                    p
                }
            }
            (Some(p), None) => p,
            (None, Some(n)) => n,
            (None, None) => break,
        };
        let (from, to) = (segs[i].from_m, segs[i].to_m);
        let mut absorbed = segs[host].clone();
        absorbed.from_m = from;
        absorbed.to_m = to;
        segs[i] = absorbed;
        segs = merge(segs);
    }
    // Indices client des points d'entrée qui encadrent chaque segment.
    let inputs: Vec<(f64, u32)> = route
        .input_pos
        .iter()
        .filter_map(|&i| route.samples.get(i as usize))
        .map(|s| (s.d_m, s.input_index.unwrap_or(s.prev_input)))
        .collect();
    for s in segs.iter_mut().filter(|_| !inputs.is_empty()) {
        let k = inputs.partition_point(|(d, _)| *d <= s.from_m + 1e-6);
        s.from_index = inputs[k.saturating_sub(1).min(inputs.len().saturating_sub(1))].1;
        let k = inputs.partition_point(|(d, _)| *d < s.to_m - 1e-6);
        s.to_index = inputs[k.min(inputs.len().saturating_sub(1))].1;
    }
    segs
}
