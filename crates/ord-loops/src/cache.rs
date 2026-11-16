//! Cache de fenêtres sous un budget d'octets unique (note lot 6, §5.5 et §10.1).
//!
//! Pur et sans fil : l'appelant le protège (le service par un `Mutex`) et attend lui-même quand la place manque.
//! Il compte les fenêtres, leurs agrégats d'équipe ([`WindowTeam`]) et les **réservations** de l'appelant (espaces
//! de travail, constructions en cours), au plus récemment servi près. Il n'évince que ce que personne ne tient
//! (`Arc` seul) : un résultat ne dépend donc jamais de l'état du cache (une fenêtre qui contient les tuiles
//! nécessaires rend le même parcours, §5.4).

use std::sync::Arc;

use crate::team::WindowTeam;
use crate::window::{tiles_cover, Window};

/// Compteurs d'usage, relus par les métriques et le banc.
#[derive(Clone, Copy, Debug, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub inserted: u64,
    pub evicted_windows: u64,
    pub evicted_teams: u64,
}

#[derive(Debug)]
struct Entry {
    win: Arc<Window>,
    teams: Vec<(u64, Arc<WindowTeam>)>,
    used: u64,
}

impl Entry {
    fn bytes(&self) -> usize {
        self.win.bytes() + self.teams.iter().map(|t| t.1.bytes()).sum::<usize>()
    }

    fn idle(&self) -> bool {
        Arc::strong_count(&self.win) == 1 && self.teams.iter().all(|t| Arc::strong_count(&t.1) == 1)
    }
}

/// Fenêtres en cache, sous un budget.
#[derive(Debug)]
pub struct WindowCache {
    budget: usize,
    reserved: usize,
    tick: u64,
    entries: Vec<Entry>,
    stats: CacheStats,
}

impl WindowCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self { budget: budget_bytes, reserved: 0, tick: 0, entries: Vec::new(), stats: CacheStats::default() }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Octets comptés : fenêtres, agrégats d'équipe, réservations.
    pub fn total(&self) -> usize {
        self.reserved + self.entries.iter().map(Entry::bytes).sum::<usize>()
    }

    /// Octets des seules fenêtres et agrégats en cache.
    pub fn cached_bytes(&self) -> usize {
        self.entries.iter().map(Entry::bytes).sum()
    }

    /// Octets des fenêtres seules et des agrégats d'équipe seuls.
    pub fn split_bytes(&self) -> (usize, usize) {
        (
            self.entries.iter().map(|e| e.win.bytes()).sum(),
            self.entries.iter().map(|e| e.teams.iter().map(|t| t.1.bytes()).sum::<usize>()).sum(),
        )
    }

    /// Réserve `bytes` pour l'appelant (espace de travail pris, construction en cours) : vrai si le budget le
    /// permet, après éviction des fenêtres que personne ne tient. Rien n'est réservé sinon.
    pub fn reserve(&mut self, bytes: usize) -> bool {
        if !self.make_room(bytes) {
            return false;
        }
        self.reserved += bytes;
        true
    }

    /// Réserve `bytes` sans condition (octets déjà tenus par l'appelant, par exemple des espaces de travail en
    /// réserve) : ils comptent dans le budget sans pouvoir être refusés.
    pub fn force_reserve(&mut self, bytes: usize) {
        self.reserved += bytes;
    }

    /// Rend une réservation (`bytes` = ce qui avait été réservé).
    pub fn release(&mut self, bytes: usize) {
        self.reserved = self.reserved.saturating_sub(bytes);
    }

    /// Évince, de la moins récemment servie à la plus récente, les entrées que personne ne tient, jusqu'à ce que
    /// `extra` octets de plus tiennent dans le budget. Faux si ce n'est pas possible.
    pub fn make_room(&mut self, extra: usize) -> bool {
        while self.total() + extra > self.budget {
            let Some((i, _)) =
                self.entries.iter().enumerate().filter(|(_, e)| e.idle()).min_by_key(|(_, e)| e.used)
            else {
                return false;
            };
            let e = self.entries.swap_remove(i);
            self.stats.evicted_windows += 1;
            self.stats.evicted_teams += e.teams.len() as u64;
        }
        true
    }

    /// La plus petite fenêtre de la vue `vi` qui contient `need`.
    pub fn lookup(&mut self, vi: usize, need: &[u32]) -> Option<Arc<Window>> {
        self.tick += 1;
        let tick = self.tick;
        let best = self
            .entries
            .iter_mut()
            .filter(|e| e.win.view_index() == vi && tiles_cover(e.win.tiles(), need))
            .min_by_key(|e| e.win.n_chains())?;
        best.used = tick;
        self.stats.hits += 1;
        Some(Arc::clone(&best.win))
    }

    /// Met une fenêtre construite en cache. L'appelant a fait de la place avant (`make_room`) ; le cache ne rejette
    /// jamais une fenêtre construite : le budget peut être dépassé de la correction entre estimation et réalité.
    pub fn insert(&mut self, win: Window) -> Arc<Window> {
        self.tick += 1;
        let win = Arc::new(win);
        self.entries.push(Entry { win: Arc::clone(&win), teams: Vec::new(), used: self.tick });
        self.stats.inserted += 1;
        win
    }

    /// Agrégats d'équipe `team` (clé choisie par l'appelant) de cette fenêtre, s'ils sont en cache.
    pub fn team(&mut self, win: &Arc<Window>, team: u64) -> Option<Arc<WindowTeam>> {
        let e = self.entries.iter().find(|e| Arc::ptr_eq(&e.win, win))?;
        e.teams.iter().find(|t| t.0 == team).map(|t| Arc::clone(&t.1))
    }

    /// Met les agrégats d'une équipe en cache avec leur fenêtre ; sans effet si la fenêtre a été évincée.
    pub fn insert_team(&mut self, win: &Arc<Window>, team: u64, wt: WindowTeam) -> Arc<WindowTeam> {
        let wt = Arc::new(wt);
        if let Some(e) = self.entries.iter_mut().find(|e| Arc::ptr_eq(&e.win, win)) {
            e.teams.retain(|t| t.0 != team);
            e.teams.push((team, Arc::clone(&wt)));
        }
        wt
    }

    /// Retire de toutes les fenêtres les agrégats d'une équipe (surcouche remplacée, équipe évincée).
    pub fn drop_team(&mut self, team: u64) {
        for e in &mut self.entries {
            let n = e.teams.len();
            e.teams.retain(|t| t.0 != team);
            self.stats.evicted_teams += (n - e.teams.len()) as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::engine_zoomed;
    use crate::tests_reference::{scenario_graph_dense, scenario_params};

    #[test]
    fn it_evicts_only_what_nobody_holds_and_the_least_recent_first() {
        let e = engine_zoomed(&mut scenario_graph_dense(5), 16, scenario_params());
        let all = e.full_window(0);
        let tiles = all.tiles();
        let parts: Vec<Vec<u32>> =
            vec![tiles[..40].to_vec(), tiles[60..100].to_vec(), tiles[120..160].to_vec()];
        let wins: Vec<Window> = parts.iter().map(|t| e.build_window(0, t, &mut || false).unwrap()).collect();
        let one = wins[0].bytes().max(wins[1].bytes()).max(wins[2].bytes());
        let mut c = WindowCache::new(one * 5 / 2);
        let a = c.insert(wins.into_iter().next().unwrap());
        assert_eq!(c.len(), 1);
        for p in &parts[1..] {
            assert!(c.make_room(one));
            c.insert(e.build_window(0, p, &mut || false).unwrap());
        }
        // `a` est tenue : à la troisième, c'est la plus ancienne **inutilisée** qui part.
        assert_eq!(c.len(), 2, "une évincée");
        assert!(c.lookup(0, &parts[0][..5]).is_some(), "la fenêtre tenue est restée");
        assert!(c.lookup(0, &parts[1][..5]).is_none(), "la plus ancienne inutilisée est partie");
        assert!(c.lookup(0, &parts[2][..5]).is_some());
        // Tout est tenu : plus de place, la réservation échoue et rien n'est réservé.
        let b = c.lookup(0, &parts[2][..5]).unwrap();
        let before = c.total();
        assert!(!c.reserve(c.budget()));
        assert_eq!(c.total(), before);
        drop((a, b));
        assert!(c.reserve(one), "tout est libre : les fenêtres inutilisées partent");
        c.release(one);
        assert_eq!(c.total(), c.cached_bytes());
        // La plus petite fenêtre qui convient sert.
        let mut c = WindowCache::new(usize::MAX / 2);
        let big = c.insert(e.build_window(0, tiles, &mut || false).unwrap());
        let small = c.insert(e.build_window(0, &parts[2], &mut || false).unwrap());
        let got = c.lookup(0, &parts[2][..5]).unwrap();
        assert!(Arc::ptr_eq(&got, &small) && !Arc::ptr_eq(&got, &big));
    }
}
