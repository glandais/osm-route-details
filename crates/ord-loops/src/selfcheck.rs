//! Auto-contrôle des index globaux contre la force brute (note lot 6, §14, points 3 et 4), sur un vrai graphe : le
//! banc l'appelle avec `--self-check`. Chaque contrôle recalcule la chose **autrement** que le moteur (sans les
//! index, sans le CSR des cellules, sans fenêtre) et compare.

use std::collections::BTreeSet;

use ord_graph::format::{GraphFile, DIR_BIT};
use ord_graph::grid::tile_key;
use ord_graph::view::BaseEdges;

use crate::engine::{chain_strong_len, strong_components, strong_lengths, Engine};
use crate::index::tiles_of_polyline;

/// Ce qui a été comparé.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelfCheck {
    /// Entrées de l'index des chaînes étrangères, par vue.
    pub foreign: [usize; 2],
    /// Chaînes dont la longueur de composante a été comparée (sous le plafond), par vue.
    pub components: [usize; 2],
    /// Nœuds de base dont le degré admis a été comparé, par vue.
    pub degrees: [usize; 2],
    pub base_edges: usize,
}

/// Chaînes qui touchent une tuile sans la posséder, par force brute : les boîtes de tous les segments de toutes les
/// chaînes.
fn check_foreign<B: AsRef<[u8]>>(e: &Engine<B>, vi: usize) -> Result<usize, String> {
    let view = &e.views_files[vi];
    let zoom = view.header().zoom;
    let mut want: BTreeSet<(u32, u32)> = BTreeSet::new();
    for c in 0..view.header().edges as u32 {
        let (a, b) = view.edge_ends(c);
        let mut pts = vec![view.node_pos(a)];
        pts.extend(view.geometry(c));
        pts.push(view.node_pos(b));
        let (x, y) = view.cell_xy(view.edge_cell(c));
        let own = tile_key(x, y);
        tiles_of_polyline(&pts, zoom, |k| {
            if k != own {
                want.insert((k, c));
            }
        });
    }
    let got = &e.idx[vi].foreign;
    if got.len() != want.len() || got.iter().zip(&want).any(|(g, w)| g != w) {
        return Err(format!(
            "index des chaînes étrangères de la vue {vi} : {} entrées, {} attendues",
            got.len(),
            want.len()
        ));
    }
    Ok(got.len())
}

/// Longueur de la composante de chaque chaîne, à plat depuis les extrémités et les sens ouverts.
fn flat_comp_lengths<B: AsRef<[u8]>>(view: &GraphFile<B>) -> Vec<u64> {
    let be = BaseEdges::load(view);
    let n = view.header().nodes as usize;
    let mut off = vec![0u32; n + 1];
    for (c, &(a, b)) in be.ends.iter().enumerate() {
        off[a as usize + 1] += u32::from(be.attrs[c].fwd_ok());
        off[b as usize + 1] += u32::from(be.attrs[c].bwd_ok());
    }
    for i in 0..n {
        off[i + 1] += off[i];
    }
    let mut fill = off.clone();
    let mut succ = vec![0u32; off[n] as usize];
    for (c, &(a, b)) in be.ends.iter().enumerate() {
        if be.attrs[c].fwd_ok() {
            succ[fill[a as usize] as usize] = b;
            fill[a as usize] += 1;
        }
        if be.attrs[c].bwd_ok() {
            succ[fill[b as usize] as usize] = a;
            fill[b as usize] += 1;
        }
    }
    let comp = strong_components(n, &|x, s| {
        s.extend_from_slice(&succ[off[x as usize] as usize..off[x as usize + 1] as usize]);
    });
    let lens = strong_lengths(&comp, &be.ends, &be.len_dm, &|_| true);
    be.ends.iter().map(|&e| chain_strong_len(&comp, &lens, e)).collect()
}

fn check_components<B: AsRef<[u8]>>(e: &Engine<B>, vi: usize) -> Result<usize, String> {
    let idx = &e.idx[vi];
    let flat = flat_comp_lengths(&e.views_files[vi]);
    let mut below = 0;
    for (c, &l) in flat.iter().enumerate() {
        let want = l.min(idx.cap_dm);
        if idx.comp_len_dm(c as u32) != want {
            return Err(format!(
                "composante de la chaîne {c} de la vue {vi} : {} au lieu de {want}",
                idx.comp_len_dm(c as u32)
            ));
        }
        below += usize::from(l < idx.cap_dm);
    }
    let lmax = BaseEdges::load(&e.views_files[vi]).len_dm.into_iter().max().unwrap_or(0);
    if idx.lmax_dm != lmax {
        return Err(format!("plus longue chaîne de la vue {vi} : {} au lieu de {lmax}", idx.lmax_dm));
    }
    Ok(below)
}

/// Degré admis d'un nœud de base (arêtes de base vivantes autour de lui) contre la règle du §4.5 : le degré du nœud
/// de vue, ou 2 pour un nœud intérieur à une chaîne.
fn check_degrees<B: AsRef<[u8]>>(e: &Engine<B>, vi: usize) -> Result<usize, String> {
    let view = &e.views_files[vi];
    let n_base = e.base.header().nodes as usize;
    let mut alive = vec![0u8; n_base];
    let mut chain_deg = vec![0u8; view.header().nodes as usize];
    for c in 0..view.header().edges as u32 {
        let (a, b) = view.edge_ends(c);
        for x in [a, b] {
            chain_deg[x as usize] = chain_deg[x as usize].saturating_add(1);
        }
        for l in view.view_chain(c).ok_or("chaînes absentes")? {
            let (ba, bb) = e.base.edge_ends(l.get() & !DIR_BIT);
            for x in [ba, bb] {
                alive[x as usize] = alive[x as usize].saturating_add(1);
            }
        }
    }
    let mut checked = 0;
    for (node, &want) in alive.iter().enumerate() {
        if want == 0 {
            continue;
        }
        let got = match view.view_node_of_base(node as u32) {
            None => 2,
            Some(g) => chain_deg[g as usize],
        };
        if got != want {
            return Err(format!(
                "degré admis du nœud de base {node} dans la vue {vi} : {got} au lieu de {want}"
            ));
        }
        checked += 1;
    }
    Ok(checked)
}

impl<B: AsRef<[u8]>> Engine<B> {
    /// Compare les index globaux et la règle du degré admis à la force brute ; la première différence est rendue en
    /// erreur. Lourd (relit les deux vues entières, plusieurs fois) : outil de banc, pas de service.
    pub fn self_check(&self) -> Result<SelfCheck, String> {
        let mut out = SelfCheck::default();
        for vi in 0..2 {
            out.foreign[vi] = check_foreign(self, vi)?;
            out.components[vi] = check_components(self, vi)?;
            out.degrees[vi] = check_degrees(self, vi)?;
        }
        for ed in 0..self.n_edges() {
            let (a, b) = self.ends(ed);
            if a == b {
                return Err(format!("l'arête de base {ed} a ses deux bouts au même nœud"));
            }
            out.base_edges += 1;
        }
        Ok(out)
    }
}
