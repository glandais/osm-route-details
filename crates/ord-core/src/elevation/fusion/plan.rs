//! Groupes d'ouvrages et plan de lecture (T04 §3.1) : pour chaque ouvrage, ses stations d'axe
//! (sur le parcours, ou sur la géométrie OSM hors du parcours), ses sondes de culée ou de portail,
//! ses éléments, et la correspondance distance → chaînage de chacun de ses passages.
//!
//! Choix simples, notés : un ouvrage est lu une fois, sur le passage qui couvre la plus grande part
//! de son axe (inv. 8), et sur la plus longue suite monotone de ce passage (demi-tour) ; deux
//! ouvrages à un seul passage séparés de moins de `2 × sonde` sur le parcours sont fusionnés en un
//! groupe lu sur le parcours ; un ouvrage à plusieurs passages n'est jamais fusionné.

use ord_domain::elevation::{TerrainOrigin, TerrainSample};
use ord_domain::route::PreparedRoute;
use ord_domain::LatLon;

use crate::rules::Rules;
use crate::typology::{
    EndKind, PassageDoubt, Structure, StructureId, StructureKind, StructurePassage, TypologyOutput,
};

/// Où se lit une station d'axe ou une sonde.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisPoint {
    /// Station du parcours.
    Route(usize),
    /// Point hors du parcours (`StructurePlan::off_route`).
    Off(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisStation {
    /// Chaînage de l'axe du groupe (mètres depuis A, ou distance de parcours si le groupe est lu
    /// sur le parcours).
    pub c: f64,
    pub at: AxisPoint,
}

/// Comment les passages d'un groupe lisent le profil.
#[derive(Clone, Debug, PartialEq)]
pub enum Mapping {
    /// Chaînage de l'ouvrage : profil calculé une fois et rendu à chaque passage.
    Chainage,
    /// Groupe de plusieurs ouvrages fusionnés, lu sur le parcours : chaînage = distance.
    Route,
}

/// Raison d'une absence de correction (`structure_uncorrected`, compteur `uncorrected`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UncorrectedReason {
    /// Option `structure_correction: false`.
    Disabled,
    /// Doute de la typologie sur un passage (appariement incertain, `unknown` adjacent…).
    Doubt,
    /// Extrémité hors des tuiles ou sans voie au-delà : pas de sonde.
    EndUnresolved,
    /// Sonde sans altitude.
    NoAltitude,
    /// Le groupe touche une plage sans typologie.
    TypologyGap,
}

/// Un groupe d'ouvrages corrigé d'un seul tenant.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupPlan {
    pub structures: Vec<StructureId>,
    /// Indices dans `TypologyOutput::passages`.
    pub passages: Vec<usize>,
    pub mapping: Mapping,
    /// Stations d'axe, chaînage croissant.
    pub axis: Vec<AxisStation>,
    /// Sondes juste au-delà de chaque extrémité (`None` : pas de sonde possible).
    pub probes: [Option<AxisStation>; 2],
    /// Suites d'éléments de même nature : `(c_from, c_to, nature)`, contiguës, croissantes.
    pub runs: Vec<(f64, f64, StructureKind)>,
    /// Chaînage des extrémités de l'ouvrage (bornes de `runs`).
    pub ends: [f64; 2],
    pub uncorrected: Option<UncorrectedReason>,
}

impl GroupPlan {
    pub fn has_bridge(&self) -> bool {
        self.runs.iter().any(|r| r.2 == StructureKind::Bridge)
    }

    /// Chaînage d'une distance de parcours pour le passage `p` de ce groupe.
    pub fn chainage(&self, p: &StructurePassage, d: f64) -> f64 {
        match self.mapping {
            Mapping::Chainage => p.chainage_at(d),
            Mapping::Route => d,
        }
    }

    pub fn kind_at(&self, c: f64) -> StructureKind {
        self.runs.iter().find(|r| c >= r.0 - 1e-9 && c <= r.1 + 1e-9).map_or_else(
            || if c < self.ends[0] { self.runs[0].2 } else { self.runs[self.runs.len() - 1].2 },
            |r| r.2,
        )
    }
}

/// Point lu hors du parcours : MNT (avec repli, pour connaître son origine) et MNS au besoin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffPoint {
    pub at: LatLon,
    pub dsm: bool,
}

/// Plan de lecture et groupes ; calculé après la typologie, avant la lecture du MNS (étape 4c).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructurePlan {
    pub groups: Vec<GroupPlan>,
    /// Stations du parcours où lire le MNS (triées, sans doublon).
    pub route_dsm: Vec<usize>,
    pub off_route: Vec<OffPoint>,
}

/// Sonde de culée : `max(min, pixels × pixel lu)`.
fn probe_m(rules: &Rules, pixel: f64) -> f64 {
    let r = &rules.elevation.structures;
    r.abutment_probe_min_m.max(r.abutment_probe_pixels * pixel)
}

/// Station du parcours la plus proche de `d`.
fn nearest_station(route: &PreparedRoute, d: f64) -> usize {
    let s = &route.samples;
    let i = s.partition_point(|x| x.d_m < d);
    if i == 0 {
        0
    } else if i >= s.len() {
        s.len() - 1
    } else if (s[i].d_m - d) < (d - s[i - 1].d_m) {
        i
    } else {
        i - 1
    }
}

/// Point à la distance `m` le long d'une polyligne (`None` si elle est plus courte).
fn along(line: &[LatLon], m: f64) -> Option<LatLon> {
    let mut acc = 0.0;
    for w in line.windows(2) {
        let l = ord_domain::geo::haversine_m(w[0], w[1]);
        if acc + l >= m && l > 0.0 {
            let t = (m - acc) / l;
            return Some(LatLon::new(
                w[0].lat + (w[1].lat - w[0].lat) * t,
                w[0].lon + (w[1].lon - w[0].lon) * t,
            ));
        }
        acc += l;
    }
    None
}

/// Plus longue suite de stations dont le chaînage est monotone (à `tol` près).
fn longest_monotone(cs: &[(usize, f64)], tol: f64) -> &[(usize, f64)] {
    if cs.len() < 2 {
        return cs;
    }
    let (mut best, mut start, mut dir) = ((0usize, 1usize), 0usize, 0i8);
    for k in 1..cs.len() {
        let step = cs[k].1 - cs[k - 1].1;
        let s = if step > tol {
            1
        } else if step < -tol {
            -1
        } else {
            0
        };
        if s != 0 && dir != 0 && s != dir {
            start = k - 1;
        }
        if s != 0 {
            dir = s;
        }
        if k + 1 - start > best.1 - best.0 {
            best = (start, k + 1);
        }
    }
    &cs[best.0..best.1]
}

struct Builder<'a> {
    route: &'a PreparedRoute,
    terrain: &'a [TerrainSample],
    rules: &'a Rules,
    pixel: &'a dyn Fn(LatLon, TerrainOrigin) -> f64,
    off: Vec<OffPoint>,
}

impl Builder<'_> {
    fn off(&mut self, at: LatLon, dsm: bool) -> AxisPoint {
        self.off.push(OffPoint { at, dsm });
        AxisPoint::Off(self.off.len() - 1)
    }

    fn probe_at(&self, station: usize) -> f64 {
        let s = &self.route.samples[station];
        probe_m(self.rules, (self.pixel)(s.at, self.terrain[station].origin))
    }

    /// Groupe d'un ouvrage et de ses passages, lu une fois en chaînage.
    fn structure_group(&mut self, s: &Structure, passages: &[(usize, &StructurePassage)]) -> GroupPlan {
        let route = self.route;
        let step = self.rules.elevation.station_step_m;
        // Passage principal : la plus grande part de l'axe, le premier à égalité.
        let (_, primary) = passages
            .iter()
            .copied()
            .max_by(|a, b| {
                let (la, ha) = a.1.chain_extent();
                let (lb, hb) = b.1.chain_extent();
                (ha - la).total_cmp(&(hb - lb)).then(b.0.cmp(&a.0))
            })
            .unwrap_or(passages[0]);
        let in_range: Vec<(usize, f64)> = route
            .samples
            .iter()
            .enumerate()
            .filter(|(_, x)| x.d_m >= primary.from_m - 1e-6 && x.d_m <= primary.to_m + 1e-6)
            .map(|(i, x)| (i, primary.chainage_at(x.d_m).clamp(0.0, s.length_m)))
            .collect();
        let mut on: Vec<(usize, f64)> = longest_monotone(&in_range, 0.05).to_vec();
        on.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        on.dedup_by(|a, b| (a.1 - b.1).abs() < 0.05);
        let has_bridge = s.elements.iter().any(|e| e.kind == StructureKind::Bridge);
        let (lo, hi) = match (on.first(), on.last()) {
            (Some(a), Some(b)) => (a.1, b.1),
            _ => (s.length_m, s.length_m),
        };
        let reach = self.rules.typology.structures.end_reached_m;
        let mut axis: Vec<AxisStation> = Vec::new();
        // Extension amont, sur la géométrie OSM (inv. 2).
        if lo > reach {
            let mut c = 0.0;
            while c < lo - 0.5 * step {
                let at = self.off(s.point_at(c), has_bridge);
                axis.push(AxisStation { c, at });
                c += step;
            }
        }
        axis.extend(on.iter().map(|&(i, c)| AxisStation { c, at: AxisPoint::Route(i) }));
        if s.length_m - hi > reach {
            let mut c = hi + step;
            while c < s.length_m {
                let at = self.off(s.point_at(c), has_bridge);
                axis.push(AxisStation { c, at });
                c += step;
            }
            let at = self.off(s.point_at(s.length_m), has_bridge);
            axis.push(AxisStation { c: s.length_m, at });
        }
        // Sondes : sur le parcours si le passage principal atteint l'extrémité et que le parcours
        // continue au-delà, sinon sur la voie au sol au-delà de la culée.
        let mut probes = [None, None];
        let mut unresolved = false;
        for (side, slot) in probes.iter_mut().enumerate() {
            let end_c = if side == 0 { 0.0 } else { s.length_m };
            let near = on.iter().min_by(|a, b| (a.1 - end_c).abs().total_cmp(&(b.1 - end_c).abs()));
            let reached = near.is_some_and(|n| (n.1 - end_c).abs() <= reach);
            let ref_station = near.map_or(0, |n| n.0);
            let probe = self.probe_at(ref_station);
            let outward = if side == 0 { -probe } else { s.length_m + probe };
            let mut found = None;
            if reached {
                // Distance de parcours où le chaînage vaut `outward`, dans le sens du passage.
                let d_end = route.samples[ref_station].d_m;
                let dir = {
                    let c0 = primary.chainage_at(d_end);
                    let c1 = primary.chainage_at(d_end + 1.0);
                    if c1 >= c0 {
                        1.0
                    } else {
                        -1.0
                    }
                };
                let d_probe = d_end + dir * (outward - primary.chainage_at(d_end));
                if d_probe >= 0.0 && d_probe <= route.length_m {
                    let i = nearest_station(route, d_probe);
                    found = Some(AxisStation {
                        c: primary.chainage_at(route.samples[i].d_m),
                        at: AxisPoint::Route(i),
                    });
                }
            }
            if found.is_none() {
                let end = &s.ends[side];
                match end.kind {
                    EndKind::Abutment => {
                        if let Some(p) = along(&end.approach, probe) {
                            let at = self.off(p, has_bridge);
                            found = Some(AxisStation { c: outward, at });
                        } else {
                            unresolved = true;
                        }
                    }
                    _ => unresolved = true,
                }
            }
            *slot = found;
        }
        let mut runs: Vec<(f64, f64, StructureKind)> = Vec::new();
        for e in &s.elements {
            match runs.last_mut() {
                Some(r) if r.2 == e.kind => r.1 = e.chain_to,
                _ => runs.push((e.chain_from, e.chain_to, e.kind)),
            }
        }
        let doubtful = passages.iter().any(|(_, p)| !p.doubts.is_empty());
        let uncorrected = if passages.iter().any(|(_, p)| p.doubts.contains(&PassageDoubt::EndUnresolved)) {
            Some(UncorrectedReason::EndUnresolved)
        } else if doubtful {
            Some(UncorrectedReason::Doubt)
        } else if unresolved {
            Some(UncorrectedReason::EndUnresolved)
        } else {
            None
        };
        GroupPlan {
            structures: vec![s.id],
            passages: passages.iter().map(|x| x.0).collect(),
            mapping: Mapping::Chainage,
            axis,
            probes,
            runs,
            ends: [0.0, s.length_m],
            uncorrected,
        }
    }

    /// Groupe de passages simples voisins sur le parcours, lu sur le parcours.
    fn route_group(&mut self, members: &[(usize, &StructurePassage, &Structure)]) -> GroupPlan {
        let route = self.route;
        let (from, to) = (members[0].1.from_m, members[members.len() - 1].1.to_m);
        let axis: Vec<AxisStation> = route
            .samples
            .iter()
            .enumerate()
            .filter(|(_, x)| x.d_m >= from - 1e-6 && x.d_m <= to + 1e-6)
            .map(|(i, x)| AxisStation { c: x.d_m, at: AxisPoint::Route(i) })
            .collect();
        // Nature le long du parcours : celle de l'élément de chaque passage ; un intervalle entre
        // deux ouvrages prend la nature du précédent.
        let mut runs: Vec<(f64, f64, StructureKind)> = Vec::new();
        for (k, (_, p, s)) in members.iter().enumerate() {
            let end = members.get(k + 1).map_or(p.to_m, |n| n.1.from_m);
            let mut d = p.from_m;
            let step = 1.0;
            while d < end {
                let kind = s.kind_at(p.chainage_at(d.min(p.to_m)));
                let next = (d + step).min(end);
                match runs.last_mut() {
                    Some(r) if r.2 == kind => r.1 = next,
                    _ => runs.push((d, next, kind)),
                }
                d = next;
            }
        }
        let mut probes = [None, None];
        let mut unresolved = false;
        for (side, d_end) in [(0, from), (1, to)] {
            let i = nearest_station(route, d_end);
            let probe = self.probe_at(i);
            let d_probe = if side == 0 { from - probe } else { to + probe };
            if d_probe >= 0.0 && d_probe <= route.length_m {
                let j = nearest_station(route, d_probe);
                probes[side] = Some(AxisStation { c: route.samples[j].d_m, at: AxisPoint::Route(j) });
            } else {
                // Parcours qui commence ou finit sur l'ouvrage : la voie au-delà de la culée.
                let (_, p, s) = if side == 0 { members[0] } else { members[members.len() - 1] };
                let c = p.chainage_at(d_end);
                let end = if c < 0.5 * s.length_m { &s.ends[0] } else { &s.ends[1] };
                match (end.kind, along(&end.approach, probe)) {
                    (EndKind::Abutment, Some(q)) => {
                        let has_bridge = s.elements.iter().any(|e| e.kind == StructureKind::Bridge);
                        let at = self.off(q, has_bridge);
                        probes[side] = Some(AxisStation { c: d_probe, at });
                    }
                    _ => unresolved = true,
                }
            }
        }
        let doubtful = members.iter().any(|(_, p, _)| !p.doubts.is_empty());
        GroupPlan {
            structures: members.iter().map(|m| m.2.id).collect(),
            passages: members.iter().map(|m| m.0).collect(),
            mapping: Mapping::Route,
            axis,
            probes,
            runs,
            ends: [from, to],
            uncorrected: if members.iter().any(|(_, p, _)| p.doubts.contains(&PassageDoubt::EndUnresolved))
                || unresolved
            {
                Some(UncorrectedReason::EndUnresolved)
            } else if doubtful {
                Some(UncorrectedReason::Doubt)
            } else {
                None
            },
        }
    }
}

/// Groupes d'ouvrages du parcours et ce qu'il faut lire pour les corriger.
pub fn plan(
    route: &PreparedRoute,
    terrain: &[TerrainSample],
    typo: &TypologyOutput,
    rules: &Rules,
    pixel: &dyn Fn(LatLon, TerrainOrigin) -> f64,
) -> StructurePlan {
    let mut b = Builder { route, terrain, rules, pixel, off: Vec::new() };
    let mut groups: Vec<GroupPlan> = Vec::new();
    // Passages par ouvrage ; passage sans ouvrage retrouvé : groupe non corrigé, lu sur le parcours.
    let by_id = |id: StructureId| typo.structures.iter().find(|s| s.id == id);
    let mut units: Vec<(f64, Vec<usize>)> = Vec::new();
    for (k, p) in typo.passages.iter().enumerate() {
        match units
            .iter_mut()
            .find(|u| typo.passages[u.1[0]].structure == p.structure && by_id(p.structure).is_some())
        {
            Some(u) => u.1.push(k),
            None => units.push((p.from_m, vec![k])),
        }
    }
    units.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Fusion sur le parcours des ouvrages à un seul passage séparés de moins de deux sondes.
    let mut pending: Vec<(usize, &StructurePassage, &Structure)> = Vec::new();
    let flush = |b: &mut Builder<'_>,
                 pending: &mut Vec<(usize, &StructurePassage, &Structure)>,
                 groups: &mut Vec<GroupPlan>| {
        match pending.len() {
            0 => {}
            1 => {
                let (k, p, s) = pending[0];
                groups.push(b.structure_group(s, &[(k, p)]));
            }
            _ => groups.push(b.route_group(pending)),
        }
        pending.clear();
    };
    for (_, members) in &units {
        let p0 = &typo.passages[members[0]];
        let Some(s) = by_id(p0.structure) else {
            flush(&mut b, &mut pending, &mut groups);
            groups.push(GroupPlan {
                structures: Vec::new(),
                passages: members.clone(),
                mapping: Mapping::Route,
                axis: Vec::new(),
                probes: [None, None],
                runs: vec![(p0.from_m, p0.to_m, StructureKind::Bridge)],
                ends: [p0.from_m, p0.to_m],
                uncorrected: Some(UncorrectedReason::Doubt),
            });
            continue;
        };
        if members.len() > 1 {
            flush(&mut b, &mut pending, &mut groups);
            let ps: Vec<(usize, &StructurePassage)> =
                members.iter().map(|&k| (k, &typo.passages[k])).collect();
            groups.push(b.structure_group(s, &ps));
            continue;
        }
        if let Some(&(_, last, _)) = pending.last() {
            let i = nearest_station(route, last.to_m);
            let gap_max = 2.0 * b.probe_at(i);
            if p0.from_m - last.to_m >= gap_max {
                flush(&mut b, &mut pending, &mut groups);
            }
        }
        pending.push((members[0], p0, s));
    }
    flush(&mut b, &mut pending, &mut groups);
    // Groupe qui touche une plage sans typologie : non corrigé.
    let gaps: Vec<&std::ops::Range<f64>> =
        typo.coverage.missing.iter().chain(&typo.coverage.unavailable).collect();
    for g in &mut groups {
        let (lo, hi) = g.passages.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &k| {
            (lo.min(typo.passages[k].from_m), hi.max(typo.passages[k].to_m))
        });
        if g.uncorrected.is_none() && gaps.iter().any(|r| r.start < hi && r.end > lo) {
            g.uncorrected = Some(UncorrectedReason::TypologyGap);
        }
    }
    let mut route_dsm: Vec<usize> = groups
        .iter()
        .filter(|g| g.uncorrected.is_none() && g.has_bridge())
        .flat_map(|g| {
            // Stations d'axe, et sondes (MNS lu pour juger la rampe, jamais écrit).
            g.axis.iter().chain(g.probes.iter().flatten()).filter_map(|a| match a.at {
                AxisPoint::Route(i) => Some(i),
                AxisPoint::Off(_) => None,
            })
        })
        .collect();
    route_dsm.sort_unstable();
    route_dsm.dedup();
    StructurePlan { groups, route_dsm, off_route: b.off }
}
