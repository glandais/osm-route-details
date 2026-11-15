//! Rapport JSON : par équipe et au total. Ni coordonnée, ni id de parcours, de trace ou d'arête (note §8).

use std::collections::BTreeMap;

use ord_graph::attrs::Access;
use ord_graph::mmap::MappedGraph;
use serde_json::{json, Map, Value};

use crate::aggregate::{EdgeAcc, TeamAcc, CAUSES, N_TYPES, STRIDE};
use crate::checks::Verification;
use crate::export::{OW_NAMES, TYPES, WK_NAMES};

pub fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Rang le plus proche : `q` dans 0..=1.
pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let k = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[k - 1]
}

/// `{n, p50, p90, p99, max}` d'une liste de valeurs (nulls si vide).
pub fn distribution(values: &[f64]) -> Value {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return json!({"n": 0, "p50": null, "p90": null, "p99": null, "max": null});
    }
    json!({
        "n": v.len(),
        "p50": r3(percentile(&v, 0.5)),
        "p90": r3(percentile(&v, 0.9)),
        "p99": r3(percentile(&v, 0.99)),
        "max": r3(*v.last().expect("non vide")),
    })
}

fn share(num: f64, den: f64) -> Value {
    if den > 0.0 {
        json!(r3(num / den))
    } else {
        Value::Null
    }
}

fn named<const N: usize>(names: &[&str; N], vals: &[f64; N]) -> Value {
    let m: BTreeMap<&str, f64> = names.iter().copied().zip(vals.iter().map(|v| r3(*v))).collect();
    json!(m)
}

const CLASSES: [(&str, u32, u32); 8] = [
    ("1", 1, 1),
    ("2", 2, 2),
    ("3-4", 3, 4),
    ("5-9", 5, 9),
    ("10-19", 10, 19),
    ("20-49", 20, 49),
    ("50-99", 50, 99),
    ("100+", 100, u32::MAX),
];

/// Statistiques de la surcouche : arêtes de base, km de réseau (chaque arête une fois), répartitions.
pub fn overlay_stats(
    gf: &MappedGraph,
    counts: &rustc_hash::FxHashMap<u32, EdgeAcc>,
    bytes: Option<u64>,
) -> Value {
    #[derive(Default, Clone, Copy)]
    struct Bin {
        edges: u64,
        km: f64,
    }
    let add = |b: &mut Bin, km: f64| {
        b.edges += 1;
        b.km += km;
    };
    let (mut all, mut dirs, mut types) = (Bin::default(), [Bin::default(); 2], [Bin::default(); N_TYPES]);
    let mut by_class: BTreeMap<&'static str, Bin> = BTreeMap::new();
    let mut by_access: BTreeMap<&'static str, Bin> = BTreeMap::new();
    let mut histogram = [0u64; CLASSES.len()];
    let mut routes: Vec<f64> = Vec::with_capacity(counts.len());
    let mut sorted: Vec<(&u32, &EdgeAcc)> = counts.iter().collect();
    sorted.sort_by_key(|(e, _)| **e);
    for (&edge, acc) in sorted {
        let ev = gf.edge(edge);
        let km = f64::from(ev.len_dm) / 10_000.0;
        add(&mut all, km);
        for (d, bin) in dirs.iter_mut().enumerate() {
            if (0..N_TYPES).any(|t| acc.counts[t * 2 + d] > 0) {
                add(bin, km);
            }
        }
        for (t, bin) in types.iter_mut().enumerate() {
            if acc.counts[t * 2] > 0 || acc.counts[t * 2 + 1] > 0 {
                add(bin, km);
            }
        }
        add(by_class.entry(ev.attrs.class().map_or("?", |c| c.name())).or_default(), km);
        let access: Access = ev.attrs.access();
        add(by_access.entry(access.name()).or_default(), km);
        routes.push(f64::from(acc.any));
        if let Some(k) = CLASSES.iter().position(|(_, lo, hi)| (*lo..=*hi).contains(&acc.any)) {
            histogram[k] += 1;
        }
    }
    let bin = |b: &Bin| json!({"edges": b.edges, "km": r3(b.km)});
    let map = |m: &BTreeMap<&'static str, Bin>| {
        Value::Object(m.iter().map(|(k, b)| ((*k).to_string(), bin(b))).collect::<Map<_, _>>())
    };
    let hist: Map<String, Value> =
        CLASSES.iter().zip(histogram).map(|((name, _, _), n)| ((*name).to_string(), json!(n))).collect();
    json!({
        "base_edges": all.edges,
        "network_km": r3(all.km),
        "by_direction": {"forward_a_to_b": bin(&dirs[0]), "backward_b_to_a": bin(&dirs[1])},
        "by_type": Value::Object(TYPES.iter().zip(types).map(|(t, b)| ((*t).to_string(), bin(&b))).collect()),
        "by_osm_class": map(&by_class),
        "by_access": map(&by_access),
        "routes_per_edge": {"distribution": distribution(&routes), "classes": hist},
        "file_bytes": bytes,
    })
}

fn share_over(v: &[f64], threshold: f64) -> Value {
    if v.is_empty() {
        Value::Null
    } else {
        json!(r3(v.iter().filter(|x| **x > threshold).count() as f64 / v.len() as f64))
    }
}

pub fn verification_json(ver: &[Verification], sample_requested: usize, matched: usize) -> Value {
    let h: Vec<f64> = ver.iter().map(|v| v.hausdorff_m).collect();
    let f: Vec<f64> = ver.iter().map(|v| v.frechet_m).collect();
    let l: Vec<f64> = ver.iter().map(|v| v.len_ratio).collect();
    let mut hs = h.clone();
    hs.sort_by(f64::total_cmp);
    let mut ls: Vec<f64> = l.iter().copied().filter(|x| x.is_finite()).collect();
    ls.sort_by(f64::total_cmp);
    json!({
        "matched_population": matched,
        "sample_requested": sample_requested,
        "sample": ver.len(),
        "hausdorff_m": distribution(&h),
        "hausdorff_over_1m": share_over(&h, 1.0),
        "hausdorff_over_3m": share_over(&h, 3.0),
        "frechet_m": distribution(&f),
        "frechet_over_1m": share_over(&f, 1.0),
        "frechet_over_3m": share_over(&f, 3.0),
        "length_ratio": {
            "n": ls.len(),
            "p1": if ls.is_empty() { Value::Null } else { json!(r3(percentile(&ls, 0.01))) },
            "p50": if ls.is_empty() { Value::Null } else { json!(r3(percentile(&ls, 0.5))) },
            "p99": if ls.is_empty() { Value::Null } else { json!(r3(percentile(&ls, 0.99))) },
        },
        "alert_p99_over_1m": !hs.is_empty() && percentile(&hs, 0.99) > 1.0,
    })
}

/// Rapport d'une équipe (ou du total : `acc` agrégé).
pub fn team_json(gf: &MappedGraph, acc: &TeamAcc, overlay_bytes: Option<u64>, ver: &Value) -> Value {
    let in_region = acc.km_total - acc.km_outside;
    let unmatched_total: f64 = acc.km_unmatched.iter().sum();
    let by_cause = |a: &[f64; CAUSES.len()]| named(&CAUSES, a);
    let mut cause_wk = Map::new();
    for (c, name) in CAUSES.iter().enumerate() {
        cause_wk.insert((*name).to_string(), named(&WK_NAMES, &acc.km_unmatched_wk[c]));
    }
    let mut wk_cause = Map::new();
    for (w, name) in WK_NAMES.iter().enumerate() {
        let m: BTreeMap<&str, f64> =
            CAUSES.iter().enumerate().map(|(c, cn)| (*cn, r3(acc.km_unmatched_wk[c][w]))).collect();
        wk_cause.insert((*name).to_string(), json!(m));
    }
    let edges_by_cause: BTreeMap<&str, u64> = CAUSES.iter().copied().zip(acc.edges_by_cause).collect();
    let geoms_by_cause: BTreeMap<&str, usize> =
        CAUSES.iter().copied().zip(acc.geoms_by_cause.iter().map(|s| s.len())).collect();
    let c = &acc.continuity;
    let gaps: Vec<f64> = c.gaps_m.iter().map(|g| f64::from(*g)).collect();
    let hs: Vec<f64> = acc.matcher_hausdorff.iter().map(|h| f64::from(*h)).collect();
    json!({
        "team_id": acc.id.to_string(),
        "rows": {"total": acc.rows, "excluded": acc.excluded, "unreadable": acc.unreadable},
        "tracks": {
            "in_region": acc.tracks_in,
            "partly_in_region": acc.tracks_partial,
            "out_of_region": acc.tracks_out,
            "without_edges": acc.tracks_empty,
        },
        "team_edges": acc.team_edges,
        "km": {
            "ridden_total": r3(acc.km_total),
            "ridden_by_type": named(&TYPES, &acc.km_by_type),
            "ridden_by_way_kind": named(&WK_NAMES, &acc.km_total_wk),
            "outside_region": r3(acc.km_outside),
            "in_region": r3(in_region),
            "matched": r3(acc.km_matched),
            "matched_share_of_in_region": share(acc.km_matched, in_region),
            "counted_in_overlay": r3(acc.km_counted),
            "below_threshold": r3(acc.km_below_threshold),
            "closed_direction_pieces_skipped": r3(acc.km_closed_piece_skipped),
            "unmatched_total": r3(unmatched_total),
            "unmatched_by_cause": by_cause(&acc.km_unmatched),
            "unmatched_by_way_kind": Value::Object(wk_cause),
            "unmatched_by_cause_and_way_kind": Value::Object(cause_wk),
            "direction_closed_by_oneway": named(&OW_NAMES, &acc.km_closed_ow),
        },
        "edges": {
            "outside_region": acc.edges_outside,
            "unmatched_by_cause": edges_by_cause,
        },
        "geometries": {
            "distinct_matched": acc.geoms_matched.len(),
            "distinct_outside_region": acc.geoms_outside.len(),
            "unmatched_by_cause": geoms_by_cause,
        },
        "overlay": overlay_stats(gf, &acc.counts, overlay_bytes),
        "checks": {
            "continuity": {
                "pairs": c.pairs,
                "adjacent": c.adjacent,
                "adjacent_share": share(c.adjacent as f64, c.pairs as f64),
                "same_edge_share": share(c.same_edge as f64, c.pairs as f64),
                "skipped_unmatched": c.skipped_unmatched,
                "skipped_outside_region": c.skipped_outside,
                "gaps_m": distribution(&gaps),
                "internal_chains": c.internal_chains,
                "internal_gaps": c.internal_gaps,
            },
            "partial_fields": {
                "checked": acc.partial.checked,
                "offsets_vs_trv_mismatch": acc.partial.offsets_vs_trv,
                "start_expected_node": acc.partial.start_expected_node,
                "start_not_at_node": acc.partial.start_not_at_node,
                "end_expected_node": acc.partial.end_expected_node,
                "end_not_at_node": acc.partial.end_not_at_node,
            },
            "verification": ver,
            "matcher_hausdorff_m": distribution(&hs),
        },
    })
}

impl TeamAcc {
    /// Somme de deux équipes (pour le total). Les ids de géométrie sont communs à tout l'export.
    pub fn absorb(&mut self, o: &TeamAcc) {
        self.rows += o.rows;
        self.excluded += o.excluded;
        self.unreadable += o.unreadable;
        self.tracks_in += o.tracks_in;
        self.tracks_partial += o.tracks_partial;
        self.tracks_out += o.tracks_out;
        self.tracks_empty += o.tracks_empty;
        self.team_edges += o.team_edges;
        self.km_total += o.km_total;
        self.km_outside += o.km_outside;
        self.km_matched += o.km_matched;
        self.km_counted += o.km_counted;
        self.km_below_threshold += o.km_below_threshold;
        self.km_closed_piece_skipped += o.km_closed_piece_skipped;
        self.edges_outside += o.edges_outside;
        for i in 0..N_TYPES {
            self.km_by_type[i] += o.km_by_type[i];
        }
        for (a, b) in self.km_total_wk.iter_mut().zip(o.km_total_wk) {
            *a += b;
        }
        for (a, b) in self.km_closed_ow.iter_mut().zip(o.km_closed_ow) {
            *a += b;
        }
        for c in 0..CAUSES.len() {
            self.km_unmatched[c] += o.km_unmatched[c];
            self.edges_by_cause[c] += o.edges_by_cause[c];
            for w in 0..WK_NAMES.len() {
                self.km_unmatched_wk[c][w] += o.km_unmatched_wk[c][w];
            }
            self.geoms_by_cause[c].extend(o.geoms_by_cause[c].iter().copied());
        }
        self.geoms_outside.extend(o.geoms_outside.iter().copied());
        self.geoms_matched.extend(o.geoms_matched.iter().copied());
        for (e, a) in &o.counts {
            let t = self.counts.entry(*e).or_default();
            for i in 0..STRIDE {
                t.counts[i] += a.counts[i];
            }
            t.any += a.any;
        }
        let (c, d) = (&mut self.continuity, &o.continuity);
        c.pairs += d.pairs;
        c.adjacent += d.adjacent;
        c.same_edge += d.same_edge;
        c.skipped_unmatched += d.skipped_unmatched;
        c.skipped_outside += d.skipped_outside;
        let (p, q) = (&mut self.partial, &o.partial);
        p.checked += q.checked;
        p.offsets_vs_trv += q.offsets_vs_trv;
        p.start_expected_node += q.start_expected_node;
        p.start_not_at_node += q.start_not_at_node;
        p.end_expected_node += q.end_expected_node;
        p.end_not_at_node += q.end_not_at_node;
        c.gaps_m.extend_from_slice(&d.gaps_m);
        c.internal_chains += d.internal_chains;
        c.internal_gaps += d.internal_gaps;
        self.matcher_hausdorff.extend_from_slice(&o.matcher_hausdorff);
    }
}
