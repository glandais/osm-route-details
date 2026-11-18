//! Le profil de goût d'une équipe au dépôt (lot 4, 3b, §14.7) : le joueur du service et la phase de profil.
//!
//! Après l'appariement, la place de dépôt est rendue ; la phase qui suit prend une place de profil (une seule pour tous
//! les environnements), construit les données de la **nouvelle** surcouche, joue le moteur sur les boucles retenues
//! trois fois et écrit `<team>.ordp.enc` avec la surcouche. Un échec ou un dépassement du profil ne fait jamais échouer
//! un dépôt. **Rien de privé n'est journalisé** : ni départ, ni empreinte de boucle, ni nom d'équipe.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ord_graph::overlay::Overlay;
use ord_graph_match::profile::collect::LearnSet;
use ord_graph_match::profile::format::{encode, hex16, ProfileMeta, FORMAT};
use ord_graph_match::profile::learn::{learn, Learned};
use ord_graph_match::profile::{first_kept_class_meters, Player, Stopped};
use ord_loops::{generate_in, Failure, Mode, Request, Taste, TeamCtx, TeamData, Workspace, TASTE_CLASSES};
use ord_loops_check::{Checker, Config as CheckConfig, TeamCheck};
use serde::Serialize;
use utoipa::ToSchema;

use super::cursor;
use super::store::{build_team_pair, next_serial, ReadyError};
use super::windows::{AcquireError, TeamRef, WindowManager};
use super::Ready;

/// Pas de réveil des attentes de place (le même que celui des fenêtres).
const POLL: Duration = Duration::from_millis(50);
/// Marge gardée sous l'échéance du dépôt pour écrire et répondre.
const DEPOSIT_MARGIN: Duration = Duration::from_secs(30);

/// How the taste profile of a deposit ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfileOutcome {
    /// Every step finished: the profile is written.
    Learned,
    /// One or two steps finished (budget or shutdown): the table of the last finished step is written.
    Partial,
    /// No loop retained (team outside the region, one-way tracks only): the common table is written as the profile.
    Empty,
    /// No step finished (no slot, budget, shutdown): no profile written, the previous one is kept.
    Skipped,
}

impl ProfileOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Learned => "learned",
            Self::Partial => "partial",
            Self::Empty => "empty",
            Self::Skipped => "skipped",
        }
    }
}

/// What the taste profile phase of a deposit did. Absent from the report when profiles are disabled.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DepositProfileV1 {
    pub outcome: ProfileOutcome,
    /// Steps finished (0 to 3).
    pub steps: u8,
    /// Loops the profile was learned from, all ride types.
    pub loops: u32,
    /// Generations played to learn it.
    pub plays: u64,
    /// Duration of the phase, slot wait included, in seconds.
    pub seconds: f64,
}

/// Ce que la phase rend : le rapport et, s'il y a lieu, les octets en clair à écrire.
pub(crate) struct Phase {
    pub report: DepositProfileV1,
    pub bytes: Option<Vec<u8>>,
}

/// Le joueur du service : les fenêtres du service, le moteur du service, le contrôleur indépendant ; la table passée est
/// celle du pas, au crochet de `WindowTeam`.
pub(crate) struct ServicePlayer<'a> {
    ready: &'a Ready,
    team: &'a TeamData,
    check: &'a TeamCheck,
    overlay: &'a Overlay,
    threads: usize,
    deadline: Instant,
}

impl std::fmt::Debug for ServicePlayer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServicePlayer").field("threads", &self.threads).finish_non_exhaustive()
    }
}

impl ServicePlayer<'_> {
    fn stopped(&self) -> bool {
        self.ready.stopping.load(Ordering::Relaxed) || Instant::now() >= self.deadline
    }

    /// Une requête : `Err(())` si le joueur doit s'arrêter, `Ok(None)` si le moteur ne rend rien de gardé.
    fn play_one(
        &self,
        ws: &mut Workspace,
        serial: u64,
        req: &Request,
        taste: &Taste,
        checker: &Checker<'_, memmap2::Mmap>,
    ) -> Result<Option<[f64; TASTE_CLASSES]>, ()> {
        let r = self.ready;
        let vi = req.ride_type.view_index();
        let mut extra_m = 0.0;
        for _ in 0..2 {
            let plan = r.engine.plan_with(req, r.cfg.window_radius_step_m, extra_m);
            let team = Some(TeamRef { serial, data: self.team, taste: Some(taste) });
            // Une attente de place qui dépasse l'échéance de la phase est un arrêt : le profil n'est pas une requête
            // d'utilisateur, il attend le budget, pas moins.
            let mut stop = || self.stopped();
            let held = match r.windows.acquire(&r.engine, &plan, team, self.deadline, &mut stop) {
                Ok(h) => h,
                Err(AcquireError::Stopped | AcquireError::Overloaded) => return Err(()),
            };
            let ctx = held.wt.as_deref().map(|wt| TeamCtx { data: self.team, win: wt });
            let res = catch_unwind(AssertUnwindSafe(|| {
                generate_in(&r.engine, &held.win, ctx, req, ws, &mut || self.stopped())
            }));
            r.windows.release(held);
            match res {
                Err(_) => {
                    metrics::counter!("ord_profile_play_panics_total").increment(1);
                    return Ok(None);
                }
                Ok(Err(Failure::Stopped)) => return Err(()),
                Ok(Err(Failure::WindowEscape)) => {
                    extra_m = r.engine.longest_chain_m(vi) + 10_000.0;
                }
                Ok(Err(_)) => return Ok(None),
                Ok(Ok(g)) => {
                    // Le contrôleur et la marche du parcours gardé tournent aussi sous `catch_unwind` : une panique ici
                    // ferait tomber tout le dépôt (le fil du joueur est joint par `expect`), surcouche comprise.
                    let kept = catch_unwind(AssertUnwindSafe(|| {
                        first_kept_class_meters(r.engine.base(), checker, req, &g)
                    }));
                    return Ok(kept.unwrap_or_else(|_| {
                        metrics::counter!("ord_profile_play_panics_total").increment(1);
                        None
                    }));
                }
            }
        }
        Ok(None)
    }
}

impl Player for ServicePlayer<'_> {
    fn play_all(
        &self,
        reqs: &[Request],
        taste: &Taste,
    ) -> Result<Vec<Option<[f64; TASTE_CLASSES]>>, Stopped> {
        let r = self.ready;
        // Un numéro neuf pour la table jouée : ses agrégats ne servent qu'à ce pas.
        let serial = next_serial();
        let next = AtomicUsize::new(0);
        let halt = std::sync::atomic::AtomicBool::new(false);
        let mut out: Vec<(usize, Option<[f64; TASTE_CLASSES]>)> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..self.threads.max(1))
                .map(|_| {
                    let (next, halt) = (&next, &halt);
                    s.spawn(move || {
                        let checker = Checker::with_tables(
                            r.engine.base(),
                            Some(self.overlay),
                            CheckConfig::default(),
                            &r.graph_index,
                            Some(self.check),
                        );
                        let mut ws = r.take_workspace();
                        let mut mine = Vec::new();
                        loop {
                            if halt.load(Ordering::Relaxed) || self.stopped() {
                                halt.store(true, Ordering::Relaxed);
                                break;
                            }
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            let Some(req) = reqs.get(i) else { break };
                            match self.play_one(&mut ws, serial, req, taste, &checker) {
                                Ok(m) => mine.push((i, m)),
                                Err(()) => {
                                    halt.store(true, Ordering::Relaxed);
                                    break;
                                }
                            }
                        }
                        r.give_workspace(ws);
                        mine
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().expect("fil du joueur")).collect()
        });
        r.windows.drop_team(serial);
        metrics::counter!("ord_profile_plays_total").increment(out.len() as u64);
        if halt.load(Ordering::Relaxed) || out.len() != reqs.len() {
            return Err(Stopped);
        }
        out.sort_by_key(|(i, _)| *i);
        Ok(out.into_iter().map(|(_, m)| m).collect())
    }
}

/// La date `complete_at` du profil qu'on écrit : maintenant s'il est complet (tous les pas, ou rien à rejouer) ; sinon
/// celle du profil qu'il remplace, ou, faute de profil, maintenant (début de la série de profils partiels).
fn complete_at(steps_done: u8, loops: [u32; 3], steps: u8, now: i64, previous: Option<i64>) -> i64 {
    if steps_done >= steps || loops == [0; 3] {
        now
    } else {
        previous.unwrap_or(now)
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Ready {
    /// Prend une place de profil depuis un fil bloquant : attente bornée par l'échéance, coupée par l'arrêt du service.
    fn take_profile_slot(&self, deadline: Instant) -> Option<tokio::sync::OwnedSemaphorePermit> {
        loop {
            if let Ok(p) = std::sync::Arc::clone(&self.profile_slots).try_acquire_owned() {
                return Some(p);
            }
            if self.stopping.load(Ordering::Relaxed) || Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(POLL);
        }
    }

    /// La phase de profil d'un dépôt (§14.7.1, point 4). `overlay_deposited_at` : la date de dépôt écrite dans la
    /// surcouche, recopiée dans l'en-tête du profil ; `previous_complete_since` : la date `complete_since` du profil
    /// que celui-ci remplace (lisible et appris avec les mêmes réglages), recopiée par un profil partiel ;
    /// `deposit_deadline` : l'échéance du dépôt. `None` quand les profils
    /// sont désactivés.
    pub(crate) fn profile_phase(
        &self,
        set: Option<LearnSet>,
        overlay_bytes: &[u8],
        overlay_deposited_at: i64,
        previous_complete_since: Option<i64>,
        deposit_deadline: Instant,
    ) -> Option<Phase> {
        let settings = self.profile.as_ref()?;
        let started = Instant::now();
        let set = set.unwrap_or_default();
        let finish = |outcome: ProfileOutcome, learned: Option<&Learned>, bytes: Option<Vec<u8>>| -> Phase {
            let seconds = started.elapsed().as_secs_f64();
            metrics::counter!("ord_profile_builds_total", "outcome" => outcome.label()).increment(1);
            metrics::histogram!("ord_profile_build_seconds").record(seconds);
            Phase {
                report: DepositProfileV1 {
                    outcome,
                    steps: learned.map_or(0, |l| l.steps_done),
                    loops: learned.map_or(0, |l| l.loops.iter().sum()),
                    plays: learned.map_or(0, |l| l.plays),
                    seconds,
                },
                bytes,
            }
        };
        let meta_of = |l: &Learned, now: i64| ProfileMeta {
            // Complet (ou ensemble vide) : c'est maintenant. Partiel : la date du profil complet précédent, ou, faute
            // de profil, le début de la série de profils partiels.
            complete_at: Some(complete_at(
                l.steps_done,
                l.loops,
                settings.learn.steps,
                now,
                previous_complete_since,
            )),
            format: FORMAT,
            learned_at: now,
            overlay_deposited_at,
            common: hex16(settings.common.hash_for(settings.learn.mix)),
            learn: hex16(settings.learn.hash()),
            engine: env!("CARGO_PKG_VERSION").to_string(),
            steps: l.steps_done,
            loops: l.loops,
            pairs: l.pairs,
            normalized: settings.learn.normalize,
            shapes: (!set.shapes.is_empty()).then(|| set.shapes.clone()),
        };
        // Aucune boucle : le profil est la table commune, sans aucun jeu ; l'état devient `ready`.
        if set.loops.is_empty() {
            let learned = Learned {
                taste: settings.common.taste(),
                steps_done: 0,
                loops: [0; 3],
                pairs: [0; 3],
                plays: 0,
                diag: Vec::new(),
            };
            let bytes = encode(&meta_of(&learned, unix_now()), &learned.taste);
            return Some(finish(ProfileOutcome::Empty, Some(&learned), Some(bytes)));
        }
        let deadline = (started + Duration::from_secs(self.cfg.profile.budget_s))
            .min(deposit_deadline.checked_sub(DEPOSIT_MARGIN).unwrap_or(started));
        let waited = Instant::now();
        let Some(permit) = self.take_profile_slot(deadline) else {
            metrics::histogram!("ord_profile_slot_wait_seconds").record(waited.elapsed().as_secs_f64());
            return Some(finish(ProfileOutcome::Skipped, None, None));
        };
        metrics::histogram!("ord_profile_slot_wait_seconds").record(waited.elapsed().as_secs_f64());
        while self.hooks.profile_hold.load(Ordering::Relaxed) && !self.stopping.load(Ordering::Relaxed) {
            std::thread::sleep(POLL);
        }
        // Un échec du profil ne fait jamais échouer un dépôt : une panique de la phase (construction des données de
        // l'équipe, joueur) devient `skipped`, la surcouche est écrite quand même.
        let result =
            catch_unwind(AssertUnwindSafe(|| self.learn_in_slot(&set, overlay_bytes, settings, deadline)))
                .unwrap_or_else(|_| {
                    metrics::counter!("ord_profile_play_panics_total").increment(1);
                    tracing::error!("taste profile phase panicked: no profile written");
                    None
                });
        drop(permit);
        let Some(learned) = result else { return Some(finish(ProfileOutcome::Skipped, None, None)) };
        if learned.steps_done == 0 {
            return Some(finish(ProfileOutcome::Skipped, Some(&learned), None));
        }
        let outcome = if learned.steps_done >= settings.learn.steps {
            ProfileOutcome::Learned
        } else {
            ProfileOutcome::Partial
        };
        let bytes = encode(&meta_of(&learned, unix_now()), &learned.taste);
        tracing::info!(
            outcome = outcome.label(),
            steps = learned.steps_done,
            loops = ?learned.loops,
            plays = learned.plays,
            "taste profile learned"
        );
        for d in &learned.diag {
            tracing::debug!(
                step = d.step,
                ty = d.ty,
                l1_gap = d.l1_gap,
                mean_mult = d.mean_mult,
                "taste profile step"
            );
        }
        Some(finish(outcome, Some(&learned), Some(bytes)))
    }

    /// Les données de la nouvelle surcouche, comptées dans le budget, puis l'apprentissage.
    fn learn_in_slot(
        &self,
        set: &LearnSet,
        overlay_bytes: &[u8],
        settings: &super::store::ProfileSettings,
        deadline: Instant,
    ) -> Option<Learned> {
        let overlay = Overlay::decode_for(overlay_bytes, &self.engine.base().graph_identity()).ok()?;
        let stopping = || self.stopping.load(Ordering::Relaxed) || Instant::now() >= deadline;
        let reserve = |bytes: usize| -> Result<Box<dyn Send + '_>, ReadyError> {
            self.windows
                .reserve_build(bytes, deadline, &stopping)
                .map(|g| Box::new(g) as Box<dyn Send + '_>)
                .map_err(|_| ReadyError::Overloaded)
        };
        let (team, check) = build_team_pair(&self.engine, &self.graph_index, &overlay, &reserve).ok()?;
        // Résidente le temps de la phase : comptée dans le budget, rendue à la fin avec ses agrégats.
        let held_bytes = overlay.bytes() + team.bytes() + check.bytes();
        let held_serial = next_serial();
        self.windows.hold_team(held_bytes);
        // Rendue à la chute de la garde, même sur une panique du joueur : sinon le budget de mémoire perdrait ces octets
        // pour toute la vie du service.
        let _held = HeldTeam { windows: &self.windows, bytes: held_bytes, serial: held_serial };
        if self.hooks.profile_panic.load(Ordering::Relaxed) {
            panic!("panique de la phase de profil (crochet de test)");
        }
        let player = ServicePlayer {
            ready: self,
            team: &team,
            check: &check,
            overlay: &overlay,
            threads: self.cfg.profile.threads,
            deadline,
        };
        let cursors: Vec<(Mode, f32, u32)> = settings
            .learn
            .cursors
            .iter()
            .map(|&h| {
                let a = cursor::apply(h);
                (a.mode, a.familiarity, a.off_team_cost_q8)
            })
            .collect();
        Some(learn(set, &cursors, &player, &settings.common, &settings.learn))
    }
}

/// Les octets de la nouvelle surcouche tenus le temps de la phase de profil, rendus au budget à la chute.
struct HeldTeam<'a> {
    windows: &'a WindowManager,
    bytes: usize,
    serial: u64,
}

impl Drop for HeldTeam<'_> {
    fn drop(&mut self) {
        self.windows.release_team(self.bytes, self.serial);
    }
}

#[cfg(test)]
mod tests {
    use super::complete_at;

    #[test]
    fn a_complete_profile_dates_itself_and_a_partial_one_carries_the_last_complete_date() {
        let (now, before) = (2_000_000, 1_000_000);
        // Trois pas sur trois : maintenant, quel que soit le profil remplacé.
        assert_eq!(complete_at(3, [4, 2, 0], 3, now, Some(before)), now);
        assert_eq!(complete_at(3, [4, 2, 0], 3, now, None), now);
        // Ensemble vide (aucun pas, aucune boucle) : complet.
        assert_eq!(complete_at(0, [0; 3], 3, now, Some(before)), now);
        // Partiel : la date du complet précédent, recopiée de nuit en nuit...
        assert_eq!(complete_at(2, [4, 2, 0], 3, now, Some(before)), before);
        assert_eq!(complete_at(1, [4, 2, 0], 3, now + 86_400, Some(before)), before);
        // ... ou, sans profil, le début de la série.
        assert_eq!(complete_at(1, [4, 2, 0], 3, now, None), now);
    }
}
