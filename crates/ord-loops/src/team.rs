//! Attributs d'équipe (note lot 2, §2) : arêtes d'équipe par type, trous comblés (règle « cul-de-sac fictif, 100 m »),
//! agrégats par arête de vue (chaîne), composantes du réseau strict.
//!
//! Écart à la note (§3) : les listes par vue sont **denses** (une `ChainTeam` de 12 octets par arête de vue) et non
//! creuses ; 7 à 11 Mo par type et par équipe au lieu de 0,7 Mo, pour un accès direct dans la boucle de recherche.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use ord_graph::attrs::HighwayClass;
use ord_graph::format::DIR_BIT;
use ord_graph::overlay::Overlay;
use rustc_hash::FxHashMap;

use crate::cost::{self, is_greenway, RideType};
use crate::engine::{
    chain_strong_len, strong_components, strong_lengths, Engine, EngineError, ViewData, NO_CHAIN,
};

pub(crate) const TEAM: u8 = 1;
pub(crate) const BRIDGED: u8 = 2;

/// Agrégat d'équipe d'une arête de vue pour un type.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ChainTeam {
    /// Somme des poids sans équipe des arêtes de base d'équipe de la chaîne.
    pub a: u32,
    /// Somme de celles qui sont comblées.
    pub b: u32,
    /// Fréquence moyenne (pondérée par `a`) des arêtes d'équipe, 0 à 255.
    pub z: u8,
    /// Toutes les arêtes de base sont d'équipe ou comblées.
    pub strict: bool,
}

#[derive(Debug)]
pub(crate) struct TypeTeam {
    pub chains: Vec<ChainTeam>,
    /// Arêtes de base d'équipe ou comblées : `(arête, drapeaux, z)`, triées.
    pub edges: Vec<(u32, u8, u8)>,
    pub strict_comp: Vec<u32>,
    pub strict_comp_len_dm: Vec<u64>,
    pub team_edges: u64,
    pub team_m: f64,
    pub bridged_m: f64,
    /// Arêtes roulées par l'équipe (type demandé ou tous types) mais hors de la vue du type : (m du type, m tous types).
    pub outside_m: (f64, f64),
}

impl TypeTeam {
    pub fn flags(&self, e: u32) -> (u8, u8) {
        match self.edges.binary_search_by_key(&e, |x| x.0) {
            Ok(i) => (self.edges[i].1, self.edges[i].2),
            Err(_) => (0, 0),
        }
    }
}

/// Ce que le moteur sait d'une équipe, pour un graphe : immuable, partageable.
#[derive(Debug)]
pub struct TeamData {
    pub(crate) per: [Option<TypeTeam>; 3],
}

/// Résumé pour le rapport et les tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct TeamStats {
    /// Arêtes de base d'équipe pour le type.
    pub team_edges: u64,
    pub team_km: f64,
    /// Longueur comblée par la règle des trous.
    pub bridged_km: f64,
    /// Km roulés par l'équipe hors de la vue du type, par un parcours du type (ou de n'importe quel type).
    pub outside_typed_km: f64,
    pub outside_any_km: f64,
}

impl TeamData {
    /// Construit les attributs d'équipe pour les trois types ; refuse une surcouche d'un autre graphe.
    pub fn build<B: AsRef<[u8]>>(e: &Engine<B>, overlay: &Overlay) -> Result<Self, EngineError> {
        overlay.check_graph(&e.base.graph_identity()).map_err(|_| EngineError::OverlayMismatch)?;
        let per = RideType::ALL.map(|r| Some(build_type(e, overlay, r)));
        Ok(Self { per })
    }

    pub fn stats(&self, ride: RideType) -> TeamStats {
        self.per[ride.index()].as_ref().map_or_else(TeamStats::default, |t| TeamStats {
            team_edges: t.team_edges,
            team_km: t.team_m / 1000.0,
            bridged_km: t.bridged_m / 1000.0,
            outside_typed_km: t.outside_m.0 / 1000.0,
            outside_any_km: t.outside_m.1 / 1000.0,
        })
    }

    pub(crate) fn of(&self, ride: RideType) -> &TypeTeam {
        self.per[ride.index()].as_ref().expect("les trois types sont construits")
    }

    /// Vrai si l'arête de base est d'équipe pour le type (sans les comblements).
    pub fn is_team_edge(&self, ride: RideType, edge: u32) -> bool {
        self.of(ride).flags(edge).0 & TEAM != 0
    }

    /// Vrai si l'arête de base est comblée par la règle des trous.
    pub fn is_bridged_edge(&self, ride: RideType, edge: u32) -> bool {
        self.of(ride).flags(edge).0 & BRIDGED != 0
    }
}

/// Surface rugueuse (note §2.1, Java `EdgeWeights.roughUnpaved`) : seule celle-là n'est présumée d'équipe en gravel
/// et VTT que si un parcours du bon type l'a prise. `compacted`, `fine_gravel`, `gravel`, `unpaved`, les pavés
/// n'en sont pas. Réécrite à part dans le contrôleur, qui n'importe pas le moteur.
fn rough_surface(a: ord_graph::attrs::Attrs) -> bool {
    use ord_graph::attrs::SURFACES;
    if a.tracktype() >= 4 {
        return true;
    }
    let code = usize::from(a.surface());
    if code == 0 {
        return false;
    }
    matches!(
        SURFACES.get(code - 1).copied(),
        Some(
            "ground"
                | "dirt"
                | "earth"
                | "grass"
                | "sand"
                | "mud"
                | "rock"
                | "wood"
                | "clay"
                | "woodchips"
                | "salt"
                | "ice"
        )
    )
}

fn type_index(o: &Overlay, name: &str) -> Option<usize> {
    o.meta.types.iter().position(|t| t == name)
}

fn build_type<B: AsRef<[u8]>>(e: &Engine<B>, ov: &Overlay, ride: RideType) -> TypeTeam {
    let v = e.view(ride);
    let n_edges = e.bd.ends.len();
    let (i_road, i_gravel, i_mtb, i_mixed) =
        (type_index(ov, "ROAD"), type_index(ov, "GRAVEL"), type_index(ov, "MTB"), type_index(ov, "MIXED"));
    let cnt = |c: &[u16], idx: Option<usize>, dir: usize| idx.map_or(0u32, |i| u32::from(c[2 * i + dir]));
    let mut fl = vec![0u8; n_edges];
    let mut raw_n = vec![0u32; n_edges];
    let mut max_n = 1u32;
    let mut outside_dm = (0u64, 0u64);
    for (edge, c) in ov.iter() {
        let any = c.iter().any(|&x| x > 0);
        if !any {
            continue;
        }
        if v.edge_chain[edge as usize] == NO_CHAIN {
            let typed = match ride {
                RideType::Road => {
                    cnt(c, i_road, 0).max(cnt(c, i_road, 1)).max(cnt(c, i_mixed, 0)).max(cnt(c, i_mixed, 1))
                }
                RideType::Gravel => cnt(c, i_gravel, 0)
                    .max(cnt(c, i_gravel, 1))
                    .max(cnt(c, i_mixed, 0))
                    .max(cnt(c, i_mixed, 1)),
                RideType::Mtb => {
                    cnt(c, i_mtb, 0).max(cnt(c, i_mtb, 1)).max(cnt(c, i_mixed, 0)).max(cnt(c, i_mixed, 1))
                }
            };
            let l = u64::from(e.bd.len_dm[edge as usize]);
            outside_dm.1 += l;
            if typed > 0 {
                outside_dm.0 += l;
            }
            continue;
        }
        let dirs = |f: &dyn Fn(usize) -> u32| f(0).max(f(1));
        let mut typed = match ride {
            RideType::Road => dirs(&|d| cnt(c, i_road, d) + cnt(c, i_mixed, d)),
            RideType::Gravel => dirs(&|d| cnt(c, i_gravel, d) + cnt(c, i_mixed, d)),
            RideType::Mtb => dirs(&|d| cnt(c, i_mtb, d) + cnt(c, i_mixed, d)),
        };
        if ride == RideType::Mtb && typed == 0 {
            typed = dirs(&|d| cnt(c, i_gravel, d));
        }
        let a = e.bd.attrs[edge as usize];
        // Exceptions Java (plan §5.5) : une surface présumée n'est acceptée que d'un parcours du bon type.
        let exception = match ride {
            RideType::Road => (is_greenway(a) || a.class() == Some(HighwayClass::Track)) && a.surface() == 0,
            RideType::Gravel | RideType::Mtb => {
                matches!(a.class(), Some(HighwayClass::Path | HighwayClass::Footway)) || rough_surface(a)
            }
        };
        let team = if exception { typed > 0 } else { true };
        if !team {
            continue;
        }
        fl[edge as usize] = TEAM;
        let n = if typed > 0 {
            typed
        } else {
            dirs(&|d| (0..ov.meta.types.len()).map(|i| u32::from(c[2 * i + d])).sum())
        };
        raw_n[edge as usize] = n;
        max_n = max_n.max(n);
    }
    let lm = f64::from(max_n).ln_1p();
    let mut z = vec![0u8; n_edges];
    for e_i in 0..n_edges {
        if fl[e_i] & TEAM != 0 {
            z[e_i] = (255.0 * f64::from(raw_n[e_i]).ln_1p() / lm).round().clamp(0.0, 255.0) as u8;
        }
    }

    // Trous : culs-de-sac fictifs et plus court chemin non roulé de 100 m au plus vers un autre nœud d'équipe.
    let n_nodes = e.bd.pos.len();
    let mut tdeg = vec![0u8; n_nodes];
    for (i, &(a, b)) in e.bd.ends.iter().enumerate() {
        if fl[i] & TEAM != 0 {
            tdeg[a as usize] = tdeg[a as usize].saturating_add(1);
            tdeg[b as usize] = tdeg[b as usize].saturating_add(1);
        }
    }
    let hole_dm = (e.params.hole_m * 10.0) as u32;
    let mut starts: Vec<u32> = Vec::new();
    for (i, &(a, b)) in e.bd.ends.iter().enumerate() {
        if fl[i] & TEAM != 0 {
            for x in [a, b] {
                if tdeg[x as usize] == 1 && v.base_inc(x).len() >= 2 {
                    starts.push(x);
                }
            }
        }
    }
    starts.sort_unstable();
    starts.dedup();
    let mut to_bridge: Vec<u32> = Vec::new();
    for &s in &starts {
        let mut dist: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg[x as usize] >= 1 {
                let mut cur = x;
                while cur != s {
                    let (_, pe) = dist[&cur];
                    to_bridge.push(pe);
                    let (a, b) = e.bd.ends[pe as usize];
                    cur = if a == cur { b } else { a };
                }
                break;
            }
            for &be in v.base_inc(x) {
                if fl[be as usize] & TEAM != 0 {
                    continue;
                }
                let nd = d + e.bd.len_dm[be as usize];
                if nd > hole_dm {
                    continue;
                }
                let (a, b) = e.bd.ends[be as usize];
                let y = if a == x { b } else { a };
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, be));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
    }
    // Jonctions courtes : un chemin non roulé de `connector_m` au plus entre deux nœuds d'équipe (seconde chaussée
    // d'une 2 × 2, raccord d'un carrefour que l'appariement a manqué) ; sans elle une chaussée à sens unique
    // roulée dans un sens ne se rejoint pas dans l'autre.
    let conn_dm = (e.params.connector_m * 10.0) as u32;
    for s in 0..n_nodes as u32 {
        if tdeg[s as usize] == 0
            || !v
                .base_inc(s)
                .iter()
                .any(|&be| fl[be as usize] & TEAM == 0 && e.bd.len_dm[be as usize] <= conn_dm)
        {
            continue;
        }
        let mut dist: FxHashMap<u32, (u32, u32)> = FxHashMap::default();
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        dist.insert(s, (0, u32::MAX));
        heap.push(Reverse((0, s)));
        while let Some(Reverse((d, x))) = heap.pop() {
            if dist[&x].0 < d {
                continue;
            }
            if x != s && tdeg[x as usize] >= 1 {
                let mut cur = x;
                while cur != s {
                    let (_, pe) = dist[&cur];
                    to_bridge.push(pe);
                    let (a, b) = e.bd.ends[pe as usize];
                    cur = if a == cur { b } else { a };
                }
                continue;
            }
            for &be in v.base_inc(x) {
                if fl[be as usize] & TEAM != 0 {
                    continue;
                }
                let nd = d + e.bd.len_dm[be as usize];
                if nd > conn_dm {
                    continue;
                }
                let (a, b) = e.bd.ends[be as usize];
                let y = if a == x { b } else { a };
                if dist.get(&y).is_none_or(|&(od, _)| nd < od) {
                    dist.insert(y, (nd, be));
                    heap.push(Reverse((nd, y)));
                }
            }
        }
    }
    for be in to_bridge {
        fl[be as usize] |= BRIDGED;
    }

    // Agrégats par arête de vue.
    let mut chains = vec![ChainTeam::default(); v.n_chains];
    let w_osm = |ed: usize| cost::edge_weight(ride, e.bd.attrs[ed], e.bd.len_dm[ed]);
    for (c, ct) in chains.iter_mut().enumerate() {
        let (mut a, mut b, mut zs, mut strict) = (0u64, 0u64, 0u64, true);
        for l in v.links(c as u32) {
            let ed = (l & !DIR_BIT) as usize;
            let w = u64::from(w_osm(ed));
            if fl[ed] & TEAM != 0 {
                a += w;
                zs += w * u64::from(z[ed]);
            } else if fl[ed] & BRIDGED != 0 {
                b += w;
            } else {
                strict = false;
            }
        }
        *ct = ChainTeam {
            a: a.min(u64::from(u32::MAX / 8)) as u32,
            b: b.min(u64::from(u32::MAX / 8)) as u32,
            z: (zs / a.max(1)).min(255) as u8,
            strict,
        };
    }

    let mut edges = Vec::new();
    let (mut team_dm, mut bridged_dm, mut team_edges) = (0u64, 0u64, 0u64);
    for ed in 0..n_edges {
        if fl[ed] != 0 {
            edges.push((ed as u32, fl[ed], z[ed]));
            if fl[ed] & TEAM != 0 {
                team_dm += u64::from(e.bd.len_dm[ed]);
                team_edges += 1;
            } else {
                bridged_dm += u64::from(e.bd.len_dm[ed]);
            }
        }
    }

    // Composantes fortement connexes du réseau strict (sens respectés, virages ignorés).
    let strict_comp = strong_components(v.n_nodes, &|x, s: &mut Vec<u32>| {
        s.extend(v.out(x).iter().filter(|&&f| chains[(f >> 1) as usize].strict).map(|&f| v.tgt(f)));
    });
    let strict_comp_len_dm = strong_lengths(&strict_comp, &v.ends, &v.len_dm, &|c| chains[c].strict);
    TypeTeam {
        chains,
        edges,
        strict_comp,
        strict_comp_len_dm,
        team_edges,
        team_m: team_dm as f64 / 10.0,
        bridged_m: bridged_dm as f64 / 10.0,
        outside_m: (outside_dm.0 as f64 / 10.0, outside_dm.1 as f64 / 10.0),
    }
}

impl TypeTeam {
    /// Longueur de la composante stricte fortement connexe de la chaîne (0 si ses bouts n'en partagent aucune).
    pub fn strict_comp_len_of_chain(&self, v: &ViewData, c: u32) -> u64 {
        chain_strong_len(&self.strict_comp, &self.strict_comp_len_dm, v.ends[c as usize])
    }
}
