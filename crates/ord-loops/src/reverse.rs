//! Ordre inverse (note §5.2) : le retour est le chemin de l'arbre retour depuis le point de demi-tour, l'aller se
//! cherche **à rebours** du demi-tour jusqu'au départ, sur le graphe privé du retour, avec l'arbre aller pour
//! heuristique. Sert quand l'ordre de base (aller de l'arbre aller, retour par un A*) ne trouve pas de retour disjoint.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::search::{turn_cost, Ban, Ctx, Found, Stopped, Tree, BAN, COR, INF, NONE, POLL, TAIL};

/// Rend les états de l'aller, du départ au dernier état avant le demi-tour `turn_node`. `ret0` : premier état du
/// retour (il fixe le virage au demi-tour).
#[allow(clippy::too_many_arguments)]
pub(crate) fn astar_back(
    cx: &Ctx<'_>,
    ta: &mut Tree,
    t1: &Tree,
    turn_node: u32,
    ret0: u32,
    ban: &Ban<'_>,
    budget: &mut u64,
    exhausted: &mut bool,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Option<Found>, Stopped> {
    let v = cx.v;
    ta.begin(2 * v.n_chains, v.n_nodes);
    let Some(h0) = t1.node_best(turn_node).map(|b| t1.get(b)) else { return Ok(None) };
    let cap = h0.saturating_mul(2).saturating_add(10_000);
    let start_chain = cx.s.snap.chain;
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let expand = |ta: &mut Tree,
                  heap: &mut BinaryHeap<Reverse<(u32, u32, u32)>>,
                  node: u32,
                  f_next: u32,
                  store: u32,
                  g: u32,
                  m: u32| {
        for &fg in v.inn(node) {
            let c = fg >> 1;
            let bits = ban.bits(c);
            let terminal = c == start_chain;
            if !terminal && bits & BAN != 0 {
                continue;
            }
            let hin_over = terminal.then(|| cx.s.out_head[(fg & 1) as usize]);
            let Some(tc) = turn_cost(v, cx.forb, fg, NONE, f_next, hin_over, None) else { continue };
            let (w, len, h) = if terminal {
                let dir = (fg & 1) as usize;
                let w = cx.s.out_cost[dir];
                if w == INF {
                    continue;
                }
                (w, cx.s.out_len[dir], 0)
            } else {
                let mut w = cx.w.chain(c);
                if w == INF {
                    continue;
                }
                let sn = v.src(fg);
                if !cx.disc.inside(v.pos[sn as usize]) {
                    continue;
                }
                let Some(hb) = t1.node_best(sn) else { continue };
                if bits & TAIL != 0 {
                    w = w.saturating_mul(2);
                }
                if bits & COR != 0 {
                    w = w.saturating_mul(3);
                }
                (w, v.len_dm[c as usize], t1.get(hb))
            };
            let ng = g.saturating_add(w).saturating_add(tc);
            if ng < ta.get(fg) {
                ta.set(fg, ng, store, m + len);
                heap.push(Reverse((ng.saturating_add(h), ng, fg)));
            }
        }
    };
    expand(ta, &mut heap, turn_node, ret0, NONE, 0, 0);
    let mut polls = 0u32;
    while let Some(Reverse((key, g, f))) = heap.pop() {
        if key > cap {
            break;
        }
        if g > ta.get(f) {
            continue;
        }
        if *budget == 0 {
            *exhausted = true;
            return Ok(None);
        }
        *budget -= 1;
        ta.settled.push(f);
        polls += 1;
        if polls.is_multiple_of(POLL) && stop() {
            return Err(Stopped);
        }
        if (f >> 1) == start_chain {
            // `pred` est le successeur : on le suit jusqu'au premier état avant le demi-tour (`NONE`).
            let mut fields = vec![f];
            let mut cur = f;
            while ta.pred(cur) != NONE {
                cur = ta.pred(cur);
                fields.push(cur);
            }
            return Ok(Some(Found { fields, meters: ta.meters(f) }));
        }
        let node = v.src(f);
        let m = ta.meters(f);
        expand(ta, &mut heap, node, f, f, g, m);
    }
    Ok(None)
}
