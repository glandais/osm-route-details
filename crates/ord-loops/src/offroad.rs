//! La consigne de part de non-revêtu (lot 5, §4, 5d) : au plus quelques générations complètes à des multiplicateurs μ
//! différents du non-revêtu, leurs candidats mis en commun, puis **un seul** choix ordinaire dont la note porte l'écart à la
//! cible. Note de conception : `docs/design/technique/ord-loops-lot5.md`, avec l'encadré des décisions du propriétaire du
//! 23 novembre 2026 (grille élargie, au plus 4 pas, temps environ ×3 accepté).
//!
//! La consigne passe par [`Steer`] et `generate_steered_in`, pas par [`crate::Request`] ni par [`crate::Generation`] : les
//! références figées en hachent le `Debug` complet. Sans consigne ([`Steer::NONE`]), c'est le moteur de 1.14, au bit près.

use std::time::{Duration, Instant};

use crate::engine::Engine;
use crate::generate::{finish, offroad_share, run_step, Scored};
use crate::offroad_prior::{grid_index, GRID_Q8, NEUTRAL};
use crate::team::TeamCtx;
use crate::window::Window;
use crate::{Diagnostics, Failure, FigureDiagnostics, Generation, Mode, NoRouteDetail, Request, RideType};

/// Poids du terme de part dans la note d'un parcours : cinq points d'écart valent 1 (§4.5), comme 45° d'écart de cap.
pub(crate) const SCORE_WEIGHT: f64 = 20.0;

/// La part de non-revêtu visée (route-details : tag `surface`, puis `tracktype`, puis classe), gravel et VTT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffroadTarget {
    /// Part visée, en millièmes de la longueur.
    pub target_milli: u16,
    /// Écart toléré, en millièmes (0,05 : 50).
    pub tolerance_milli: u16,
    /// Générations complètes au plus, pas 0 compris (4 : §4.3) ; 0 vaut 1.
    pub max_steps: u8,
    /// Aucun pas k ≥ 1 n'est lancé, et celui en cours est coupé et jeté, à ce délai (ms) après le début de la recherche. Le
    /// service y met `min(échéance − 1 s, 10 s)` ; 0 : un seul pas.
    pub steps_until_ms: u32,
}

/// Ce que l'appelant demande en plus de la requête.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Steer {
    pub offroad: Option<OffroadTarget>,
}

impl Steer {
    /// Aucune consigne : le moteur de 1.14.
    pub const NONE: Self = Self { offroad: None };
}

/// Ce que la recherche a fait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OffroadOutcome {
    /// La première proposition est dans la tolérance (au sens du vérificateur interne : le service le recalcule sur ce que
    /// le contrôleur indépendant a gardé).
    pub reached: bool,
    /// Générations jouées, pas 0 compris, pas coupés non comptés.
    pub steps: u8,
    /// Le μ (256es) du pas qui a donné la première proposition.
    pub mu_q8: u32,
}

#[derive(Clone, Debug)]
pub struct SteeredGeneration {
    pub generation: Generation,
    /// `None` sans consigne.
    pub offroad: Option<OffroadOutcome>,
}

struct Rec {
    idx: usize,
    diag: Diagnostics,
    fig: FigureDiagnostics,
}

/// Le prochain point de grille à jouer : à partir de `anchor` (le point qui a donné la première proposition), dans le sens
/// de l'écart (`dir` = −1 : la part est trop basse, μ plus petit), le premier non joué ; sinon l'autre sens ; sans sens, le
/// plus proche non joué.
fn next_index(played: &[bool; 7], anchor: usize, dir: Option<i32>) -> Option<usize> {
    let scan = |d: i32| {
        let mut i = anchor as i32 + d;
        while (0..7).contains(&i) {
            if !played[i as usize] {
                return Some(i as usize);
            }
            i += d;
        }
        None
    };
    match dir {
        Some(d) => scan(d).or_else(|| scan(-d)),
        None => (0..7usize).filter(|&i| !played[i]).min_by_key(|&i| ((i as i32 - anchor as i32).abs(), i)),
    }
}

/// Facteur de la durée estimée d'un pas k ≥ 1 sur le plus long pas joué : un pas à μ < 1 coûte jusqu'à × 2,7 un pas à
/// μ = 1 en agglomération (relevé §10.5) ; le pas lancé à tort est coupé à `until` et jeté, l'estimation n'évite que le
/// calcul perdu.
const STEP_ESTIMATE: f64 = 1.5;

/// Un pas de durée estimée `STEP_ESTIMATE × longest` (au moins le plus long pas joué), lancé à `elapsed`, finit-il avant
/// `until` ?
pub(crate) fn fits(elapsed: Duration, longest: Duration, until: Duration) -> bool {
    elapsed + longest.mul_f64(STEP_ESTIMATE) <= until
}

/// La recherche de μ (§4.3). `ride` : gravel ou VTT, vérifié par l'appelant.
pub(crate) fn search<B: AsRef<[u8]>>(
    e: &Engine<B>,
    win: &Window,
    team: Option<TeamCtx<'_>>,
    r: &Request,
    t: &OffroadTarget,
    ws: &mut crate::Workspace,
    stop: &mut dyn FnMut() -> bool,
) -> (Result<SteeredGeneration, Failure>, Diagnostics, FigureDiagnostics) {
    let start = Instant::now();
    let target = f64::from(t.target_milli) / 1000.0;
    let tol = f64::from(t.tolerance_milli) / 1000.0;
    let until = Duration::from_millis(u64::from(t.steps_until_ms));
    let max_steps = usize::from(t.max_steps.max(1));
    let mut played = [false; 7];
    let mut recs: Vec<Rec> = Vec::new();
    let mut pool: Vec<(usize, Scored)> = Vec::new();
    let mut organic: Option<(bool, f64)> = None;
    let mut first_err: Option<Failure> = None;
    let mut max_dur = Duration::ZERO;
    let prior = grid_index(u32::from(t.target_milli));
    // Le pas 0 n'est jamais joué à μ < 1 : il y coûte plus cher que la génération sans cible dans les agglomérations (relevé
    // du 23 novembre, `ord-loops-lot5-mesures.md` §10.5 : × 1,7 à × 3,6 à μ ≤ 0,35, jusqu'à × 2,2 à 0,7), et c'est lui seul
    // qui garde le budget entier du service. À μ = 1, le pas 0 est la génération sans cible au bit près : une consigne ne
    // coûte ni 504 ni 422 que la requête sans cible n'aurait pas eu. Le prior y est joué au pas 1 (`jump`). Au-dessus de 1
    // (cibles basses), le pas 0 reste au prior (× 0,8 à × 1,9 au même relevé, pas de 504 de plus).
    let mut idx = prior.max(NEUTRAL);
    let first_idx = idx;
    // Anchor and direction of the next step: the grid point behind the first proposal and the sign of its gap.
    let mut next: Option<(usize, Option<i32>)>;

    loop {
        let k = recs.len();
        played[idx] = true;
        let t_step = Instant::now();
        // Seul le pas 0 garde le stop du service tel quel (1.14) ; un pas k ≥ 1 est aussi coupé à `until`, et jeté.
        let (res, diag, fig) = if k == 0 {
            run_step(e, win, team, r, ws, stop, Some(GRID_Q8[idx]))
        } else {
            let mut cut = || stop() || start.elapsed() >= until;
            run_step(e, win, team, r, ws, &mut cut, Some(GRID_Q8[idx]))
        };
        let last_dur = t_step.elapsed();
        match res {
            Ok(c) => {
                max_dur = max_dur.max(last_dur);
                organic = organic.or(c.organic);
                let rec = recs.len();
                for s in c.scored {
                    if !pool.iter().any(|(_, x)| x.key == s.key) {
                        pool.push((rec, s));
                    }
                }
                recs.push(Rec { idx, diag, fig });
            }
            // Un secteur sans candidat à ce μ peut en avoir un à un autre, et le plus court chemin d'un aller simple (que
            // `DistanceTooShort` compare à la distance) suit μ : le pas compte, la recherche continue.
            Err(f @ (Failure::NoRoute(_) | Failure::DistanceTooShort)) => {
                max_dur = max_dur.max(last_dur);
                first_err.get_or_insert(f);
                recs.push(Rec { idx, diag, fig });
            }
            // Le reste ne dépend pas de μ (ou, pour un pas coupé, l'a rendu invalide) : au pas 0 c'est la réponse, plus
            // loin le pas est jeté.
            Err(f) => {
                if k == 0 {
                    return (Err(f), diag, fig);
                }
                break;
            }
        }
        // Mesure : la première proposition du choix commun.
        let all: Vec<Scored> = pool.iter().map(|(_, s)| s.clone()).collect();
        let first = (!all.is_empty())
            .then(|| finish(e, r, all, organic, Some(target)))
            .and_then(|v| v.into_iter().next());
        next = None;
        if let Some(f) = &first {
            let share = offroad_share(f);
            if (share - target).abs() <= tol + 1e-9 {
                break;
            }
            let rec = pool.iter().find(|(_, s)| s.key == f.steps).map_or(recs.len() - 1, |p| p.0);
            next = Some((recs[rec].idx, Some(if share < target { -1 } else { 1 })));
        }
        // Arrêts avant de lancer le pas k + 1 (§4.3, point 4) : aucun pas que le temps écoulé plus sa durée estimée, au
        // moins celle du plus long pas joué (le pas 0 compris), ferait finir après `until`.
        if recs.len() >= max_steps || !fits(start.elapsed(), max_dur, until) || stop() {
            break;
        }
        // Après un pas 0 joué à μ = 1 à la place d'un prior plus bas : le prior, si la part est trop basse ou s'il n'y a
        // aucune proposition ; trop haute, la recherche ordinaire (vers les μ plus chers).
        let dir = next.and_then(|n| n.1);
        let jump = k == 0 && prior < first_idx && !played[prior] && dir != Some(1);
        let (anchor, dir) = next.unwrap_or((idx, None));
        match if jump { Some(prior) } else { next_index(&played, anchor, dir) } {
            Some(i) => idx = i,
            None => break,
        }
    }

    // Aucun pas n'a de candidat et μ = 1 n'a pas été joué (pas 0 au-dessus de 1, pas coupés, pas comptés) : la génération
    // sans cible, dans le budget du service seul. Avec une consigne, la réponse n'est jamais pire que sans.
    if pool.is_empty() && !played[NEUTRAL] && !stop() {
        let (res, diag, fig) = run_step(e, win, team, r, ws, stop, Some(GRID_Q8[NEUTRAL]));
        match res {
            Ok(c) => {
                organic = organic.or(c.organic);
                let rec = recs.len();
                pool.extend(c.scored.into_iter().map(|s| (rec, s)));
                recs.push(Rec { idx: NEUTRAL, diag, fig });
            }
            Err(f @ (Failure::NoRoute(_) | Failure::DistanceTooShort)) => {
                first_err.get_or_insert(f);
                recs.push(Rec { idx: NEUTRAL, diag, fig });
            }
            // Coupé par le service : le verdict des pas joués tient.
            Err(_) => {}
        }
    }

    let n_steps = recs.len().min(255) as u8;
    if pool.is_empty() {
        let f = first_err.unwrap_or(Failure::NoRoute(NoRouteDetail::NoCandidate));
        let rec = recs.swap_remove(0);
        return (Err(f), rec.diag, rec.fig);
    }
    let all: Vec<Scored> = pool.iter().map(|(_, s)| s.clone()).collect();
    let routes = finish(e, r, all, organic, Some(target));
    let owner = routes.first().and_then(|f| pool.iter().find(|(_, s)| s.key == f.steps)).map_or(0, |p| p.0);
    let share = routes.first().map_or(0.0, offroad_share);
    let rec = recs.swap_remove(owner);
    let out = OffroadOutcome {
        reached: !routes.is_empty() && (share - target).abs() <= tol + 1e-9,
        steps: n_steps,
        mu_q8: GRID_Q8[rec.idx],
    };
    let g = Generation {
        routes,
        network_m: rec.diag.reached_m,
        familiarity_fallback: false,
        diagnostics: rec.diag.clone(),
    };
    (Ok(SteeredGeneration { generation: g, offroad: Some(out) }), rec.diag, rec.fig)
}

/// Le type de la requête accepte-t-il une cible de part ? Gravel et VTT : la vue route n'a pas de non-revêtu à viser.
pub(crate) fn steerable(ride: RideType, mode: Mode) -> bool {
    ride != RideType::Road && mode != Mode::Strict
}
