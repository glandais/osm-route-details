//! Statistiques : relevés de construction (écrits dans la section `STATS`), lecture de `/proc`,
//! et comptes du graphe écrit (`GraphStats`, §5 de la note) pour la commande `measure`.

use std::collections::BTreeMap;
use std::time::Instant;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::attrs::{Access, HighwayClass};
use crate::format::GraphFile;
use crate::grid;

/// Ce que `/proc/self/status` dit de la mémoire, en Ko (0 hors Linux ou en cas d'erreur).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ProcStatus {
    pub vm_rss_kb: u64,
    pub vm_hwm_kb: u64,
    pub rss_anon_kb: u64,
    pub rss_file_kb: u64,
}

pub fn proc_status() -> ProcStatus {
    let mut p = ProcStatus::default();
    let Ok(text) = std::fs::read_to_string("/proc/self/status") else { return p };
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(key), Some(value)) = (it.next(), it.next()) else { continue };
        let Ok(v) = value.parse::<u64>() else { continue };
        match key {
            "VmRSS:" => p.vm_rss_kb = v,
            "VmHWM:" => p.vm_hwm_kb = v,
            "RssAnon:" => p.rss_anon_kb = v,
            "RssFile:" => p.rss_file_kb = v,
            _ => {}
        }
    }
    p
}

/// Défauts de page `(mineurs, majeurs)` du processus, par `/proc/self/stat`.
pub fn page_faults() -> (u64, u64) {
    let Ok(text) = std::fs::read_to_string("/proc/self/stat") else { return (0, 0) };
    // Le champ 2 (`comm`) peut contenir des espaces : on repart après la dernière parenthèse.
    let Some(rest) = text.rsplit_once(')').map(|(_, r)| r) else { return (0, 0) };
    let f: Vec<&str> = rest.split_whitespace().collect();
    // `rest` commence au champ 3 : minflt = champ 10, majflt = champ 12.
    let get = |n: usize| f.get(n - 3).and_then(|v| v.parse().ok()).unwrap_or(0);
    (get(10), get(12))
}

/// Temps CPU du processus (utilisateur + système), en secondes. Suppose `CLK_TCK` = 100, valeur de Linux.
pub fn cpu_seconds() -> f64 {
    let Ok(text) = std::fs::read_to_string("/proc/self/stat") else { return 0.0 };
    let Some(rest) = text.rsplit_once(')').map(|(_, r)| r) else { return 0.0 };
    let f: Vec<&str> = rest.split_whitespace().collect();
    let get = |n: usize| f.get(n - 3).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    (get(14) + get(15)) / 100.0
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Phase {
    pub name: String,
    pub wall_s: f64,
    pub cpu_s: f64,
    pub rss_kb: u64,
    pub hwm_kb: u64,
}

/// Chronomètre par phase : `lap` clôt la phase en cours.
#[derive(Debug)]
pub struct PhaseClock {
    last_wall: Instant,
    last_cpu: f64,
}

impl Default for PhaseClock {
    fn default() -> Self {
        Self::new()
    }
}

impl PhaseClock {
    pub fn new() -> Self {
        Self { last_wall: Instant::now(), last_cpu: cpu_seconds() }
    }

    pub fn lap(&mut self, name: &str) -> Phase {
        let (wall, cpu) = (Instant::now(), cpu_seconds());
        let st = proc_status();
        let p = Phase {
            name: name.to_string(),
            wall_s: wall.duration_since(self.last_wall).as_secs_f64(),
            cpu_s: cpu - self.last_cpu,
            rss_kb: st.vm_rss_kb,
            hwm_kb: st.vm_hwm_kb,
        };
        (self.last_wall, self.last_cpu) = (wall, cpu);
        p
    }
}

/// Comptes de construction, sérialisés dans la section `STATS` du fichier.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BuildStats {
    pub region: String,
    pub threads: usize,
    pub ways_total: u64,
    pub ways_kept: u64,
    pub rejects: BTreeMap<String, u64>,
    pub ways_cut_by_missing_nodes: u64,
    pub nodes_referenced_missing: u64,
    pub relations_total: u64,
    pub relations_routes: u64,
    pub relation_way_members_matched: u64,
    pub relation_way_members_not_in_graph: u64,
    pub oneway_reversible: u64,
    pub ignored_conditional: u64,
    pub ignored_bicycle_directional: u64,
    pub ignored_motor_vehicle: u64,
    pub loops_cut: u64,
    pub duplicate_identities: u64,
    pub used_nodes: u64,
    pub vertices: u64,
    pub edges: u64,
    pub phases: Vec<Phase>,
}

/// Percentile (plus proche rang) d'un tableau trié.
pub fn percentile<T: Copy + Default>(sorted: &[T], p: f64) -> T {
    if sorted.is_empty() {
        return T::default();
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Quantiles {
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
}

impl Quantiles {
    pub fn of(mut v: Vec<f64>) -> Self {
        v.sort_by(f64::total_cmp);
        Self {
            p50: percentile(&v, 0.5),
            p90: percentile(&v, 0.9),
            p99: percentile(&v, 0.99),
            max: percentile(&v, 1.0),
        }
    }
}

/// Comptes et tailles du fichier de graphe (calculés par `measure`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GraphStats {
    pub file_bytes: u64,
    pub sections: BTreeMap<String, u64>,
    pub bytes_nodes: u64,
    pub bytes_csr: u64,
    pub bytes_directed: u64,
    pub bytes_owned: u64,
    pub bytes_border: u64,
    pub bytes_block_padding: u64,
    pub bytes_geometry: u64,
    pub bytes_ids: u64,
    pub nodes: u64,
    pub edges: u64,
    pub directed: u64,
    pub cells: u64,
    pub oneway_edges: u64,
    pub contraflow_edges: u64,
    pub closed_edges: u64,
    pub border_edges: u64,
    pub border_table_entries: u64,
    pub edges_with_point_outside_ends_cells: u64,
    pub nodes_degree_1: u64,
    pub nodes_degree_2: u64,
    pub nodes_degree_2_same_attrs: u64,
    pub nodes_degree_3_plus: u64,
    pub edges_per_cell: Quantiles,
    pub edge_length_m: Quantiles,
    pub km_total: f64,
    pub km_by_class: BTreeMap<String, f64>,
    pub km_by_access: BTreeMap<String, f64>,
    pub km_by_service: BTreeMap<String, f64>,
    pub km_by_route: BTreeMap<String, f64>,
    pub km_by_infra: BTreeMap<String, f64>,
    /// Nombre d'arêtes (non orientées) par classe et par `service=*` : leviers de réduction du graphe, en arêtes.
    pub edges_by_class: BTreeMap<String, u64>,
    pub edges_by_service: BTreeMap<String, u64>,
    pub distinct_attrs: u64,
    pub largest_component_node_share: f64,
    pub largest_component_nodes: u64,
    pub bytes_per_node: f64,
    pub bytes_per_edge: f64,
    pub bytes_per_directed: f64,
    pub bytes_per_km: f64,
    /// Octets de topologie seule (nœuds, CSR, orientées, possédées, bord) par arête.
    pub topology_bytes_per_edge: f64,
}

struct Dsu {
    parent: Vec<u32>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self { parent: (0..n as u32).collect() }
    }
    fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            let p = self.parent[x as usize];
            self.parent[x as usize] = self.parent[p as usize];
            x = self.parent[x as usize];
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra as usize] = rb;
        }
    }
}

/// Composante la plus grande du graphe (arêtes d'accès non `No`, non orientées) : étiquette par nœud.
pub fn largest_component<B: AsRef<[u8]>>(g: &GraphFile<B>) -> (Vec<bool>, u64) {
    let n = g.header().nodes as usize;
    let mut dsu = Dsu::new(n);
    for e in 0..g.header().edges as u32 {
        if g.edge(e).attrs.access() == Access::No {
            continue;
        }
        let (a, b) = g.edge_ends(e);
        dsu.union(a, b);
    }
    let mut size: FxHashMap<u32, u64> = FxHashMap::default();
    for v in 0..n as u32 {
        *size.entry(dsu.find(v)).or_default() += 1;
    }
    let (best, count) =
        size.iter().max_by_key(|(r, c)| (**c, std::cmp::Reverse(**r))).map_or((0, 0), |(r, c)| (*r, *c));
    let member = (0..n as u32).map(|v| dsu.find(v) == best).collect();
    (member, count)
}

impl GraphStats {
    pub fn compute<B: AsRef<[u8]>>(g: &GraphFile<B>) -> Self {
        let h = g.header();
        let mut s = GraphStats {
            file_bytes: g.len(),
            nodes: h.nodes,
            edges: h.edges,
            directed: h.directed,
            cells: h.cells,
            ..Default::default()
        };
        for (name, len) in g.section_lengths() {
            s.sections.insert(name.to_string(), len);
        }
        s.bytes_geometry = s.sections.get("GEOMETRY").copied().unwrap_or(0);
        s.bytes_ids = s.sections.get("ID_NODES").copied().unwrap_or(0)
            + s.sections.get("ID_EDGES").copied().unwrap_or(0);

        let mut per_cell_edges = Vec::new();
        let mut lengths = Vec::new();
        let mut deg = vec![0u8; h.nodes as usize];
        let mut first_attrs = vec![0u64; h.nodes as usize];
        let mut mixed = vec![false; h.nodes as usize];
        let mut distinct = rustc_hash::FxHashSet::default();
        for ci in 0..g.n_cells() {
            let cell = g.cell(ci);
            let info = cell.info();
            let (nn, nd, ne, nb) = (
                u64::from(info.n_nodes),
                u64::from(info.n_directed),
                u64::from(info.n_edges),
                u64::from(info.n_border),
            );
            s.bytes_nodes += nn * 8;
            s.bytes_csr += (nn + 1) * 4;
            s.bytes_directed += nd * 8;
            s.bytes_owned += ne * 16;
            s.bytes_border += nb * 8;
            s.bytes_block_padding += info.padded_block_len() - u64::from(info.block_len);
            s.border_table_entries += nb;
            per_cell_edges.push(ne as f64);
            for i in 0..info.n_edges {
                let e = info.first_edge + i;
                let rec = cell.owned(i as usize);
                let a = rec.attrs;
                distinct.insert(a.0);
                let km = f64::from(rec.len_dm) / 10_000.0;
                lengths.push(f64::from(rec.len_dm) / 10.0);
                s.km_total += km;
                let class = a.class().map_or("?", HighwayClass::name);
                *s.km_by_class
                    .entry(format!("{class}{}", if a.link() { "_link" } else { "" }))
                    .or_default() += km;
                *s.km_by_access.entry(a.access().name().to_string()).or_default() += km;
                *s.km_by_service.entry(a.service().to_string()).or_default() += km;
                *s.edges_by_service.entry(a.service().to_string()).or_default() += 1;
                *s.edges_by_class
                    .entry(format!("{class}{}", if a.link() { "_link" } else { "" }))
                    .or_default() += 1;
                *s.km_by_infra.entry(a.infra().to_string()).or_default() += km;
                for (bit, name) in [(1u8, "local"), (2, "regional"), (4, "national"), (8, "mtb")] {
                    if a.routes() & bit != 0 {
                        *s.km_by_route.entry(name.to_string()).or_default() += km;
                    }
                }
                if a.fwd_ok() != a.bwd_ok() {
                    s.oneway_edges += 1;
                }
                if !a.fwd_ok() && !a.bwd_ok() {
                    s.closed_edges += 1;
                }
                if a.contraflow() {
                    s.contraflow_edges += 1;
                }
                let (na, nb_) = g.edge_ends(e);
                for v in [na, nb_] {
                    let d = &mut deg[v as usize];
                    if *d == 0 {
                        first_attrs[v as usize] = a.0;
                    } else if first_attrs[v as usize] != a.0 {
                        mixed[v as usize] = true;
                    }
                    *d = d.saturating_add(1);
                }
                let (ca, cb) = (g.node_cell(na), g.node_cell(nb_));
                if ca != cb {
                    s.border_edges += 1;
                }
                // Points intérieurs hors des tuiles des deux extrémités.
                let (ta, tb) = (g.cell_xy(ca), g.cell_xy(cb));
                if g.geometry(e).iter().any(|&(lat, lon)| {
                    let t = grid::tile_of_e7(lat, lon, h.zoom);
                    t != ta && t != tb
                }) {
                    s.edges_with_point_outside_ends_cells += 1;
                }
            }
        }
        for v in 0..h.nodes as usize {
            match deg[v] {
                0 | 1 => s.nodes_degree_1 += 1,
                2 => {
                    s.nodes_degree_2 += 1;
                    if !mixed[v] {
                        s.nodes_degree_2_same_attrs += 1;
                    }
                }
                _ => s.nodes_degree_3_plus += 1,
            }
        }
        s.distinct_attrs = distinct.len() as u64;
        s.edges_per_cell = Quantiles::of(per_cell_edges);
        s.edge_length_m = Quantiles::of(lengths);
        let (_, big) = largest_component(g);
        s.largest_component_nodes = big;
        s.largest_component_node_share = if h.nodes > 0 { big as f64 / h.nodes as f64 } else { 0.0 };
        let fb = s.file_bytes as f64;
        s.bytes_per_node = fb / h.nodes.max(1) as f64;
        s.bytes_per_edge = fb / h.edges.max(1) as f64;
        s.bytes_per_directed = fb / h.directed.max(1) as f64;
        s.bytes_per_km = fb / s.km_total.max(1e-9);
        let topo = s.bytes_nodes + s.bytes_csr + s.bytes_directed + s.bytes_owned + s.bytes_border;
        s.topology_bytes_per_edge = topo as f64 / h.edges.max(1) as f64;
        s
    }
}
