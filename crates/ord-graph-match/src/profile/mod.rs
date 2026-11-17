//! Profil de goût d'une équipe, appris au dépôt par regénération (lot 4, 3b, note `ord-loops-lot4.md` §14) : les
//! boucles retenues des traces appariées, le jeu du moteur sur leurs requêtes, le recalé itéré et le format du fichier.
//!
//! **Privé** : départs, caps et empreintes des boucles ne sont jamais journalisés ni écrits ; seules des tables de 72
//! entiers par type en sortent.

pub mod collect;
pub mod format;
pub mod learn;

use ord_loops::{
    generate_in, Engine, Failure, Generation, Request, Taste, TeamCtx, TeamData, WindowTeam, Workspace,
    TASTE_CLASSES,
};
use ord_loops_check::Checker;

use crate::walk::{class_lengths, Walk};
use ord_graph::mmap::MappedGraph;

/// Le joueur s'est arrêté (échéance ou arrêt du service) : l'apprentissage rend la table du dernier pas fini.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stopped;

/// Joue des requêtes avec une table de goût (§14.5.3).
pub trait Player: Sync {
    /// Joue chaque requête avec `taste` passé au crochet (`WindowTeam::build_with_taste(…, Some(taste))`) sur la
    /// surcouche du dépôt ; une `WindowEscape` est rejouée une fois en fenêtre élargie ; les parcours rendus passent au
    /// contrôleur indépendant dans l'ordre du moteur ; rend les mètres par classe du premier gardé, `None` sinon. Pas
    /// de budget de temps par requête. Le résultat `i` est celui de `reqs[i]`, quel que soit le nombre de fils.
    fn play_all(&self, reqs: &[Request], taste: &Taste)
        -> Result<Vec<Option<[f64; TASTE_CLASSES]>>, Stopped>;
}

/// Le premier parcours gardé par le contrôleur indépendant : `class_lengths` de `Walk::from_route`. Partagé par tous
/// les joueurs, c'est la règle du « premier parcours » du banc (§14.0, point 7).
pub fn first_kept_class_meters<B: AsRef<[u8]>>(
    gf: &MappedGraph,
    checker: &Checker<'_, B>,
    req: &Request,
    g: &Generation,
) -> Option<[f64; TASTE_CLASSES]> {
    g.routes.iter().find(|r| checker.check(req, r).ok()).map(|route| {
        let walk = Walk::from_route(gf, route);
        let trace = walk.trace(gf);
        class_lengths(gf, &walk, &trace)
    })
}

/// Joueur de référence : un fil, la fenêtre exacte de chaque requête, sans cache. Pour les tests du service et du banc,
/// qui doivent rendre les mêmes résultats par leurs propres joueurs.
pub struct FlatPlayer<'a, B: AsRef<[u8]>> {
    pub engine: &'a Engine<B>,
    pub team: &'a TeamData,
    pub checker: &'a Checker<'a, B>,
    pub gf: &'a MappedGraph,
}

impl<B: AsRef<[u8]>> std::fmt::Debug for FlatPlayer<'_, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlatPlayer").finish_non_exhaustive()
    }
}

impl<B: AsRef<[u8]> + Sync> Player for FlatPlayer<'_, B>
where
    B: AsRef<[u8]>,
{
    fn play_all(
        &self,
        reqs: &[Request],
        taste: &Taste,
    ) -> Result<Vec<Option<[f64; TASTE_CLASSES]>>, Stopped> {
        let mut ws = Workspace::new();
        let mut out = Vec::with_capacity(reqs.len());
        for req in reqs {
            let run = |ws: &mut Workspace, extra: f64| -> Result<Generation, Failure> {
                let plan = self.engine.plan_with(req, 0.0, extra);
                let win =
                    self.engine.build_window(plan.view, &plan.need, &mut || false).ok_or(Failure::Stopped)?;
                let wt = WindowTeam::build_with_taste(self.engine, &win, self.team, Some(taste));
                generate_in(
                    self.engine,
                    &win,
                    Some(TeamCtx { data: self.team, win: &wt }),
                    req,
                    ws,
                    &mut || false,
                )
            };
            let mut res = run(&mut ws, 0.0);
            if matches!(res, Err(Failure::WindowEscape)) {
                let extra = self.engine.longest_chain_m(req.ride_type.view_index()) + 10_000.0;
                res = run(&mut ws, extra);
            }
            out.push(res.ok().and_then(|g| first_kept_class_meters(self.gf, self.checker, req, &g)));
        }
        Ok(out)
    }
}
