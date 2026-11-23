//! Retouche d'un parcours, lot 0 (note `ord-retouche-lot0.md` §4 et §5) : **code de mesure**, compilé seulement avec la
//! fonctionnalité `probe` (le banc). Rien de ce module n'est appelé par la génération.
//!
//! - [`KeepOut`] : des chaînes de la fenêtre interdites à **toute** une génération. Le banc le pose par
//!   [`crate::Workspace::set_keep_out`] avant l'appel ; `Weighter::chain` le lit, donc les deux arbres, les A* et
//!   l'accroche (`Ban` n'est lu que par les A*).
//! - [`move_end_in`] : déplacer l'arrivée. Un arbre retour depuis la nouvelle arrivée `E′` sur le parcours masqué, et pour
//!   chaque couture la sortie qui minimise « longueur abandonnée + raccord ».

use std::collections::HashMap;
use std::time::Instant;

use ord_graph::format::DIR_BIT;
use rustc_hash::FxHashSet;

use crate::engine::{Engine, Snap};
use crate::generate::steps_of;
use crate::search::{
    make_anchor, tree_backward, turn_cost, Ctx, Disc, Forbidden, Weighter, Workspace, INF, NONE,
};
use crate::team::TeamCtx;
use crate::verify::check_route_in;
use crate::window::Window;
use crate::{Family, LatLon, Mode, Position, Request, Route};

/// Passes : la première, puis des reprises après avoir interdit les virages que le contrôle interne a refusés.
const PASSES: usize = 4;

/// Empreinte de la liste des tuiles d'une fenêtre (FNV-1a).
fn tiles_hash(tiles: &[u32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &t in tiles {
        h = (h ^ u64::from(t)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Chaînes locales d'une fenêtre interdites pour toute une requête (bitset de `n_chains` bits).
#[derive(Clone, Debug)]
pub struct KeepOut {
    bits: Vec<u64>,
    n_chains: usize,
    tiles_hash: u64,
    masked: usize,
}

impl KeepOut {
    /// Masque les chaînes de `win` qui portent au moins une arête de `mask_edges` et aucune arête de `free_edges`. Les
    /// `free_edges` sont la zone des coutures : une chaîne qui en porte une reste libre, pour que la recherche puisse
    /// partir ou arriver exactement à la couture.
    pub fn build(
        win: &Window,
        mask_edges: impl IntoIterator<Item = u32>,
        free_edges: impl IntoIterator<Item = u32>,
    ) -> Self {
        let mask: FxHashSet<u32> = mask_edges.into_iter().collect();
        let free: FxHashSet<u32> = free_edges.into_iter().collect();
        let n = win.v.n_chains;
        let mut bits = vec![0u64; n.div_ceil(64).max(1)];
        let mut masked = 0;
        if !mask.is_empty() {
            for c in 0..n {
                let (mut m, mut f) = (false, false);
                for &l in win.v.links(c as u32) {
                    let e = l & !DIR_BIT;
                    m |= mask.contains(&e);
                    f |= free.contains(&e);
                }
                if m && !f {
                    bits[c >> 6] |= 1 << (c & 63);
                    masked += 1;
                }
            }
        }
        Self { bits, n_chains: n, tiles_hash: tiles_hash(win.tiles()), masked }
    }

    pub fn masked_chains(&self) -> usize {
        self.masked
    }

    /// Mètres de vue masqués : à rapporter, pour voir ce que la chaîne entière ajoute à l'arête gardée.
    pub fn masked_m(&self, win: &Window) -> f64 {
        self.assert_window(win);
        let v = &win.v;
        (0..v.n_chains).filter(|&c| self.is_masked(c as u32)).map(|c| f64::from(v.len_dm[c])).sum::<f64>()
            / 10.0
    }

    pub fn is_masked(&self, c: u32) -> bool {
        self.bits[(c >> 6) as usize] >> (c & 63) & 1 != 0
    }

    pub(crate) fn bits(&self) -> &[u64] {
        &self.bits
    }

    /// Refuse un masque bâti pour une autre fenêtre : ses numéros de chaînes ne diraient rien ici.
    pub(crate) fn assert_window(&self, win: &Window) {
        assert!(
            self.n_chains == win.v.n_chains && self.tiles_hash == tiles_hash(win.tiles()),
            "le masque de chaînes interdites a été bâti pour une autre fenêtre"
        );
    }
}

/// Une couture vue de la sonde : le nœud de base, l'arête (et son sens) qui y arrive le long du parcours, et les mètres
/// qu'on abandonne si on quitte le parcours ici.
#[derive(Clone, Copy, Debug)]
pub struct SeamIn {
    pub node: u32,
    pub in_edge: u32,
    pub in_reverse: bool,
    pub abandon_m: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct MoveEndParams {
    /// Rayon du disque de l'arbre retour autour de `E′`, m ; élargi une fois (×2) si aucune couture n'est atteinte.
    pub radius_m: f64,
    pub max_radius_m: f64,
    pub min_radius_m: f64,
}

/// Une sortie choisie : la couture, le raccord (de la couture à `E′`) et les mètres abandonnés.
#[derive(Clone, Debug)]
pub struct MoveEndPick {
    /// Indice dans `seams`.
    pub seam: usize,
    pub raccord: Route,
    pub raccord_m: f64,
    pub abandon_m: f64,
}

#[derive(Clone, Debug)]
pub struct MoveEndOut {
    /// Minimise `abandon_m + raccord_m` (à égalité, la couture la plus tardive).
    pub best: MoveEndPick,
    /// La couture atteinte la plus tardive.
    pub keep_all: Option<MoveEndPick>,
    /// Le plus petit raccord.
    pub nearest: Option<MoveEndPick>,
    pub radius_m: f64,
    pub widened: bool,
    /// Passes jouées (1 : le premier arbre suffisait).
    pub passes: u32,
    pub t2_settled: u64,
    pub seams_reached: u32,
    pub us: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveEndFail {
    EndTooFar,
    /// `E′` s'accroche sur une chaîne masquée : il est sur le parcours (ou sur sa partie abandonnée). L'arête de base
    /// accrochée, pour que le banc situe la couture qui suit.
    EndOnRoute {
        edge: u32,
    },
    NoSeamReached,
    WindowEscape,
    NoTeamData,
    Stopped,
}

/// Déplacer l'arrivée : `r.start` est `E′` (`r.end` vide), `r.ride_type`, `r.mode`, la familiarité et λ sont ceux du
/// curseur ; `r.distance_m` ne sert qu'à la fenêtre du banc. `keep_out` doit avoir été bâti sur `win`.
#[allow(clippy::too_many_arguments)]
pub fn move_end_in<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<TeamCtx<'_>>,
    r: &Request,
    seams: &[SeamIn],
    keep_out: &KeepOut,
    ws: &mut Workspace,
    p: &MoveEndParams,
) -> Result<MoveEndOut, MoveEndFail> {
    keep_out.assert_window(win);
    if r.mode != Mode::Discovery && team.is_none() {
        return Err(MoveEndFail::NoTeamData);
    }
    let ride = r.ride_type;
    let vi = ride.view_index();
    assert_eq!(win.vi, vi, "la fenêtre n'est pas celle de la vue du type demandé");
    let v = &win.v;
    let t0 = Instant::now();
    let free = Weighter::new(v, team, ride, r.mode, r.familiarity, r.off_team_cost_q8);
    let w = Weighter::new(v, team, ride, r.mode, r.familiarity, r.off_team_cost_q8)
        .with_keep_out(Some(keep_out.bits()));
    let min_dm = (e.params.min_component_share * p.min_radius_m * 10.0) as u64;
    let strict_tt = if free.strict() { free.tt() } else { None };
    let ok = |c: u32| -> bool {
        free.chain(c) != INF && strict_tt.is_none_or(|tt| tt.strict_len_of(v.gid[c as usize]) >= min_dm)
    };
    let s_snap = e
        .snap(win, r.start, &ok, min_dm)
        .map_err(|()| MoveEndFail::WindowEscape)?
        .ok_or(MoveEndFail::EndTooFar)?;
    if keep_out.is_masked(s_snap.chain) {
        return Err(MoveEndFail::EndOnRoute { edge: s_snap.edge });
    }
    let ea = make_anchor(e, v, &w, s_snap);

    // Où sont les chaînes d'arrivée des coutures : une passe sur les liens de la fenêtre.
    let wanted: FxHashSet<u32> = seams.iter().map(|s| s.in_edge).collect();
    let mut chain_of: HashMap<u32, u32> = HashMap::with_capacity(wanted.len());
    for c in 0..v.n_chains {
        for &l in v.links(c as u32) {
            let ed = l & !DIR_BIT;
            if wanted.contains(&ed) {
                chain_of.insert(ed, c as u32);
            }
        }
    }
    // État d'arrivée de chaque couture : (nœud local, état orienté de la chaîne qui y arrive).
    let arrivals: Vec<Option<(u32, u32)>> = seams
        .iter()
        .map(|s| {
            let gn = e.views_files[vi].view_node_of_base(s.node)?;
            let nl = v.ngid.binary_search(&gn).ok()? as u32;
            if !v.complete[nl as usize] {
                return None;
            }
            let c = *chain_of.get(&s.in_edge)?;
            (0..2u32)
                .map(|dir| (c << 1) | dir)
                .find(|&f| v.tgt(f) == nl && v.oriented_links(f).last() == Some(&(s.in_edge, s.in_reverse)))
                .map(|f| (nl, f))
        })
        .collect();

    let s_e7 = r.start.to_e7();
    let mut forb = Forbidden::default();
    let esc = std::cell::Cell::new(false);
    let rho0 = p.radius_m.clamp(p.min_radius_m, p.max_radius_m);
    // Comme la génération : un raccord que le contrôle interne refuse pour un virage trop serré est repris avec les
    // transitions que le refus interdit (au plus `PASSES - 1` reprises).
    for pass in 0..PASSES {
        let mut rho = rho0;
        let mut widened = false;
        // (couture, état de sortie, coût, mètres du raccord)
        let mut reached: Vec<(usize, u32, u64, f64)> = Vec::new();
        let mut t2_settled: u64;
        loop {
            let disc = Disc::new(s_e7, s_e7, 2.0 * rho, true);
            let cx = Ctx { v, w: &w, disc: &disc, s: &ea, e: &ea, forb: &forb, esc: &esc, org: None };
            let t2 = &mut ws.views[vi].t2;
            let halted = |_| if esc.get() { MoveEndFail::WindowEscape } else { MoveEndFail::Stopped };
            tree_backward(&cx, t2, &mut || false).map_err(halted)?;
            t2_settled = t2.settled.len() as u64;
            reached.clear();
            for (k, a) in arrivals.iter().enumerate() {
                let Some((nl, f_in)) = *a else { continue };
                let mut best: Option<(u64, u32)> = None;
                for &fo in v.out(nl) {
                    let g = t2.get(fo);
                    if g == INF {
                        continue;
                    }
                    // Le virage du moteur à la couture : pas de demi-tour, 150° au plus.
                    let Some(tc) = turn_cost(v, &forb, f_in, NONE, fo, None, None) else { continue };
                    let total = u64::from(g) + u64::from(tc);
                    if best.is_none_or(|(b, bf)| total < b || (total == b && fo < bf)) {
                        best = Some((total, fo));
                    }
                }
                if let Some((cost, fo)) = best {
                    reached.push((k, fo, cost, f64::from(t2.meters(fo)) / 10.0));
                }
            }
            if !reached.is_empty() || rho >= p.max_radius_m {
                break;
            }
            rho = (2.0 * rho).min(p.max_radius_m);
            widened = true;
        }
        if reached.is_empty() {
            return Err(MoveEndFail::NoSeamReached);
        }
        let t2 = &ws.views[vi].t2;
        let pick_of = |&(k, fo, _, raccord_m): &(usize, u32, u64, f64)| -> (MoveEndPick, Vec<u32>) {
            let mut fields = t2.path_to(fo);
            fields.reverse();
            let links = v.oriented_links(fo);
            let (e0, rev0) = links[0];
            let len0 = v.links(fo >> 1).len();
            let start_snap = Snap {
                chain: fo >> 1,
                link: if fo & 1 == 0 { 0 } else { len0 - 1 },
                edge: e0,
                rev: rev0,
                offset_m: if rev0 { e.geom_len_m(e0) } else { 0.0 },
                dist_m: 0.0,
            };
            let (steps, step_field) = steps_of(v, &fields, &start_snap, &s_snap);
            (
                MoveEndPick {
                    seam: k,
                    raccord: Route {
                        start: Position { edge: start_snap.edge, offset_m: start_snap.offset_m },
                        end: Position { edge: s_snap.edge, offset_m: s_snap.offset_m },
                        steps,
                        length_m: 0.0,
                        team_m: 0.0,
                        bridged_m: 0.0,
                        shared_m: 0.0,
                        crossings: 0,
                        unpaved_m: 0.0,
                        offroad_m: 0.0,
                        presumed_surface_m: 0.0,
                        bearing_deviation_deg: None,
                        familiarity: r.familiarity,
                        family: Family::Turnaround,
                    },
                    raccord_m,
                    abandon_m: seams[k].abandon_m,
                },
                step_field,
            )
        };
        let best = reached
            .iter()
            .min_by(|a, b| {
                let (ta, tb) = (seams[a.0].abandon_m + a.3, seams[b.0].abandon_m + b.3);
                ta.total_cmp(&tb).then(b.0.cmp(&a.0))
            })
            .expect("non vide");
        let (best_pick, step_field) = pick_of(best);
        // Le contrôle interne de la génération sur le raccord de la meilleure sortie.
        let node_pos = v.pos[arrivals[best.0].expect("atteinte").0 as usize];
        let rq = Request {
            start: LatLon::from_e7(node_pos),
            end: Some(r.start),
            distance_m: best_pick.raccord_m,
            ..r.clone()
        };
        let chk = check_route_in(e, win, team.map(|t| t.data), &rq, &best_pick.raccord);
        if !chk.sharp_at.is_empty() && pass + 1 < PASSES {
            let mut grown = false;
            for &i in &chk.sharp_at {
                if i + 1 < step_field.len() && step_field[i] != step_field[i + 1] {
                    grown |= forb.insert((step_field[i], step_field[i + 1]));
                }
            }
            if grown {
                continue;
            }
        }
        let last = reached.iter().max_by_key(|x| x.0).expect("non vide");
        let near = reached.iter().min_by(|a, b| a.3.total_cmp(&b.3).then(b.0.cmp(&a.0))).expect("non vide");
        return Ok(MoveEndOut {
            best: best_pick,
            keep_all: (last.0 != best.0).then(|| pick_of(last).0),
            nearest: (near.0 != best.0).then(|| pick_of(near).0),
            radius_m: rho,
            widened,
            passes: pass as u32 + 1,
            t2_settled,
            seams_reached: reached.len() as u32,
            us: t0.elapsed().as_secs_f64() * 1e6,
        });
    }
    unreachable!("la dernière passe rend toujours")
}
