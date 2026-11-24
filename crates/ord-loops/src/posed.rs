//! Le parcours posé : l'entrée de la retouche (note `ord-retouche-lot1.md` §2).
//!
//! Un parcours enregistré est une **marche** (jambes d'arêtes de base, jonctions) et la polyligne d'origine du document.
//! Une **couture** est une jonction `Node(n)` où un tronçon neuf peut partir ou arriver exactement. Elle est valide si :
//! 1. `n` est un nœud de la vue du type (seul un nœud de vue est un bout de chaîne) ;
//! 2. les arêtes des deux jambes sont dans la vue et dans la fenêtre : cette règle se juge **dans la fenêtre** (`graft.rs`) ;
//! 3. ce n'est pas un demi-tour : deux arêtes distinctes, et un angle d'au plus 150° entre la sortie de la première et
//!    l'entrée de la seconde ;
//! 4. `n` est dans une plage continue à `seam_margin_m` ou plus de chaque bout (`Touch` coupe la plage, sauf
//!    `touch_continues`). Pour `move_end`, seule la marge **avant** compte, la suite étant abandonnée.
//!
//! La projection monotone sur la polyligne d'origine place chaque couture à son abscisse `s_m` ; une couture projetée hors
//! de l'ordre de la marche (5,0 % des traces au lot 0) fait **refuser** la trace : couper la géométrie d'origine à une
//! couture mal placée rendrait un recollé faux. C'est le portage, sans changement de règle, de `posed.rs` et `project.rs`
//! du banc du lot 0 : `ord-graph-match` dépend d'`ord-loops`, le moteur ne peut pas prendre un `Walk` et a son entrée à lui.

use crate::cost::RideType;
use crate::engine::Engine;
use crate::geo::{LatLon, R_EARTH_M};

/// Mètres par degré de latitude (sphère de `ord-loops-check`, approximation locale pour les projections).
const M_LAT: f64 = 111_194.926_6;

/// Une arête de base parcourue sur `[lo, hi]` ⊂ `[0, 1]` de sa longueur (paramètre a → b), dans le sens b → a si `reverse`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PosedLeg {
    pub edge: u32,
    pub reverse: bool,
    pub lo: f64,
    pub hi: f64,
}

/// Jonction entre deux jambes consécutives (celle d'`ord-graph-match`, recopiée : pas de dépendance inverse).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PosedJoin {
    /// Sortie de l'une et entrée de l'autre au même nœud de base.
    Node(u32),
    /// Les deux points sont à 5 m ou moins sans être au même nœud.
    Touch,
    /// Trou : longueur de la corde, m.
    Hole { chord_m: f64 },
}

/// Ce que l'appelant donne : la marche et la polyligne d'origine (celle du document, chaque premier point répété retiré).
#[derive(Clone, Copy, Debug)]
pub struct PosedInput<'a> {
    pub ride_type: RideType,
    pub legs: &'a [PosedLeg],
    /// `legs.len() - 1` jonctions.
    pub joins: &'a [PosedJoin],
    pub line: &'a [LatLon],
}

/// Réglages de la pose ; `Default` = les valeurs du moteur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseTune {
    /// Marge continue exigée de chaque côté d'une couture, m (lot 0 : 200 → 100, +12 points de couvert, rien perdu ailleurs).
    pub seam_margin_m: f64,
    /// Distance maximale d'un nœud à la polyligne pour la projection d'une couture, m.
    pub proj_tol_m: f64,
    /// `Touch` ne coupe pas une plage (variante ; la référence est stricte : le recollé y serait discontinu).
    pub touch_continues: bool,
    /// Écart relatif maximal de `Σ jambes + Σ trous` à la longueur de la polyligne : au-delà, des jambes en double ou
    /// manquantes.
    pub max_drift: f64,
}

impl Default for PoseTune {
    fn default() -> Self {
        Self { seam_margin_m: 100.0, proj_tol_m: 15.0, touch_continues: false, max_drift: 0.02 }
    }
}

/// Une couture candidate (passe les règles 1, 3 et la projection) et ses marges continues.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PosedSeam {
    /// La jonction est entre `legs[join]` et `legs[join + 1]`.
    pub join: usize,
    /// Nœud de base.
    pub node: u32,
    /// Abscisse sur la polyligne d'origine, m.
    pub s_m: f64,
    /// Abscisse de marche (graphe, trous compris), m.
    pub a_m: f64,
    /// Longueur graphe continue avant et après la couture, dans sa plage.
    pub before_m: f64,
    pub after_m: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PoseStats {
    /// Plages continues, trous et contacts de la marche.
    pub runs: u32,
    pub holes: u32,
    pub touches: u32,
    /// Jonctions `Node` écartées comme couture, par règle : 1 (nœud hors vue), 3 (demi-tour ou virage de plus de 150°), 4
    /// (marge, des deux côtés). La règle 2 se juge dans la fenêtre et a son compteur dans les diagnostics.
    pub refused_by_rule: [u32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoseError {
    Empty,
    NotOnGraph,
    /// Un type que le moteur ne sert pas (ni route, ni gravel, ni VTT) : jamais, `RideType` n'a que ces trois valeurs.
    WrongType,
    /// Des coutures projetées hors de l'ordre de la marche : 5,0 % des traces au lot 0 ; refusées à l'entrée.
    OrderAnomaly {
        seams: u32,
    },
    /// Jambes en double ou manquantes : `|Σ jambes + Σ trous − L| / L` au-delà de `max_drift`.
    Drift,
}

/// Un parcours posé : la marche, la polyligne d'origine et les coutures candidates.
#[derive(Clone, Debug)]
pub struct PosedRoute {
    ride_type: RideType,
    legs: Vec<PosedLeg>,
    joins: Vec<PosedJoin>,
    /// Polyligne d'origine, E7, et abscisses haversine cumulées.
    line: Vec<(i32, i32)>,
    s: Vec<f64>,
    l_m: f64,
    /// Longueur graphe de chaque jambe et cumulée à sa fin, m.
    leg_m: Vec<f64>,
    cum_leg_end_m: Vec<f64>,
    seams: Vec<PosedSeam>,
    stats: PoseStats,
    margin_m: f64,
    proj_tol_m: f64,
}

// --- Géométrie -----------------------------------------------------------------------------------------------------

fn hav(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let dlat = la2 - la1;
    let dlon = (b.1 - a.1).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R_EARTH_M * h.sqrt().min(1.0).asin()
}

fn e7_deg(p: (i32, i32)) -> (f64, f64) {
    (f64::from(p.0) * 1e-7, f64::from(p.1) * 1e-7)
}

fn bearing(a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let dy = b.0 - a.0;
    let dx = (b.1 - a.1) * a.0.to_radians().cos();
    (dx != 0.0 || dy != 0.0).then(|| (dx.atan2(dy).to_degrees() + 360.0) % 360.0)
}

/// Points `(lat, lon)` d'une arête dans le sens de la jambe.
fn walking_pts<B: AsRef<[u8]>>(e: &Engine<B>, leg: &PosedLeg) -> Vec<(f64, f64)> {
    let mut pts: Vec<(f64, f64)> = e.edge_polyline(leg.edge).into_iter().map(e7_deg).collect();
    if leg.reverse {
        pts.reverse();
    }
    pts
}

/// Angle de virage (degrés, 0 = tout droit) entre la sortie de `x` et l'entrée de `y` à leur nœud commun. `None` si une
/// géométrie est dégénérée.
fn turn_deg<B: AsRef<[u8]>>(e: &Engine<B>, x: &PosedLeg, y: &PosedLeg) -> Option<f64> {
    let px = walking_pts(e, x);
    let py = walking_pts(e, y);
    let out_x = px.windows(2).rev().find_map(|w| bearing(w[0], w[1]))?;
    let in_y = py.windows(2).find_map(|w| bearing(w[0], w[1]))?;
    let mut d = (in_y - out_x).abs() % 360.0;
    if d > 180.0 {
        d = 360.0 - d;
    }
    Some(d)
}

/// Distance du point `p` au segment `i` de la polyligne et abscisse de la projection.
fn on_segment(line: &[(f64, f64)], s: &[f64], i: usize, p: (f64, f64)) -> (f64, f64) {
    let (a, b) = (line[i], line[i + 1]);
    let kx = M_LAT * p.0.to_radians().cos();
    let (ax, ay) = ((a.1 - p.1) * kx, (a.0 - p.0) * M_LAT);
    let (bx, by) = ((b.1 - p.1) * kx, (b.0 - p.0) * M_LAT);
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (-(ax * dx + ay * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (ax + t * dx, ay + t * dy);
    (qx.hypot(qy), s[i] + t * (s[i + 1] - s[i]))
}

/// Une couture à projeter.
struct SeamPoint {
    latlon: (f64, f64),
    a_m: f64,
    /// Marge ajoutée à la fenêtre : un trou ne vaut que sa corde sur la marche, et le passage non apparié qu'il recouvre
    /// est plus long (trois fois la somme des cordes des trous depuis la couture précédente).
    slack_m: f64,
}

/// Projette les coutures (dans l'ordre de la marche) sur la polyligne : chaque recherche part de l'abscisse de la couture
/// précédente et ne lit que la fenêtre `[s₋ + 0,8 Δa − 50 ; s₋ + 1,5 Δa + 200 + marge]` ; elle y prend le premier
/// minimum local de la distance qui tombe à `tol_m` ou moins. Rend les abscisses (croissantes) et le nombre d'anomalies.
fn project(line: &[(f64, f64)], s: &[f64], seams: &[SeamPoint], tol_m: f64) -> (Vec<f64>, u32) {
    let (mut out, mut anomalies) = (Vec::with_capacity(seams.len()), 0u32);
    let (mut prev_s, mut prev_a) = (0.0f64, 0.0f64);
    let nseg = line.len().saturating_sub(1);
    for sp in seams {
        let da = (sp.a_m - prev_a).max(0.0);
        let lo = (prev_s + 0.8 * da - 50.0).max(prev_s);
        let hi = prev_s + 1.5 * da + 200.0 + sp.slack_m;
        let i0 = s.partition_point(|&x| x < lo).saturating_sub(1).min(nseg);
        let i1 = s.partition_point(|&x| x <= hi).min(nseg);
        let mut found = None;
        let mut i = i0;
        while i < i1 {
            let (d, sx) = on_segment(line, s, i, sp.latlon);
            if d <= tol_m {
                let (mut bd, mut bs) = (d, sx);
                let mut j = i + 1;
                while j < i1 {
                    let (d2, s2) = on_segment(line, s, j, sp.latlon);
                    if d2 < bd {
                        (bd, bs) = (d2, s2);
                        j += 1;
                    } else {
                        break;
                    }
                }
                found = Some((bd, bs));
                break;
            }
            i += 1;
        }
        let sx = match found {
            Some((_, sx)) => sx,
            None => {
                anomalies += 1;
                (0..nseg)
                    .map(|k| on_segment(line, s, k, sp.latlon))
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .map_or(prev_s, |x| x.1)
            }
        };
        let sx = sx.max(prev_s);
        out.push(sx);
        prev_s = sx;
        prev_a = sp.a_m;
    }
    (out, anomalies)
}

impl PosedRoute {
    /// Pose un parcours : structure, plages, coutures candidates (règles 1, 3 et 4), projection monotone.
    pub fn pose<B: AsRef<[u8]>>(
        e: &Engine<B>,
        input: &PosedInput<'_>,
        tune: &PoseTune,
    ) -> Result<Self, PoseError> {
        let legs = input.legs;
        if legs.is_empty() || input.line.len() < 2 {
            return Err(PoseError::Empty);
        }
        if input.joins.len() + 1 != legs.len() {
            return Err(PoseError::NotOnGraph);
        }
        let n_edges = e.n_edges();
        if legs
            .iter()
            .any(|l| l.edge >= n_edges || !(0.0..=1.0).contains(&l.lo) || !(0.0..=1.0).contains(&l.hi))
        {
            return Err(PoseError::NotOnGraph);
        }
        let vi = input.ride_type.view_index();
        let view = &e.views_files[vi];
        let leg_m: Vec<f64> =
            legs.iter().map(|l| (l.hi - l.lo).abs() * f64::from(e.len_dm(l.edge)) / 10.0).collect();
        let mut acc = 0.0;
        let cum_leg_end_m: Vec<f64> = leg_m
            .iter()
            .map(|m| {
                acc += m;
                acc
            })
            .collect();
        let holes_m: f64 = input
            .joins
            .iter()
            .map(|j| if let PosedJoin::Hole { chord_m } = j { *chord_m } else { 0.0 })
            .sum();
        let line_e7: Vec<(i32, i32)> = input.line.iter().map(|p| p.to_e7()).collect();
        let line: Vec<(f64, f64)> = line_e7.iter().copied().map(e7_deg).collect();
        let mut s = Vec::with_capacity(line.len());
        let mut sacc = 0.0;
        for (i, p) in line.iter().enumerate() {
            if i > 0 {
                sacc += hav(line[i - 1], *p);
            }
            s.push(sacc);
        }
        let l_m = sacc;
        if l_m <= 0.0 || ((acc + holes_m) - l_m).abs() / l_m > tune.max_drift {
            return Err(PoseError::Drift);
        }
        // Plages continues.
        let mut stats = PoseStats::default();
        let mut run_of = vec![0usize; legs.len()];
        let mut run_ends: Vec<usize> = Vec::new();
        let mut run = 0usize;
        for (j, join) in input.joins.iter().enumerate() {
            run_of[j] = run;
            let joined = matches!(join, PosedJoin::Node(_))
                || (tune.touch_continues && matches!(join, PosedJoin::Touch));
            match join {
                PosedJoin::Hole { .. } => stats.holes += 1,
                PosedJoin::Touch => stats.touches += 1,
                PosedJoin::Node(_) => {}
            }
            if !joined {
                run_ends.push(j);
                run += 1;
            }
        }
        run_of[legs.len() - 1] = run;
        run_ends.push(legs.len() - 1);
        stats.runs = run_ends.len() as u32;
        let run_start = |r: usize| if r == 0 { 0 } else { run_ends[r - 1] + 1 };
        // Coutures candidates : règles 1 et 3, marges de la plage.
        struct Cand {
            join: usize,
            node: u32,
            a_m: f64,
            before_m: f64,
            after_m: f64,
            slack_m: f64,
        }
        let mut cands: Vec<Cand> = Vec::new();
        let mut prev_join = 0usize;
        for (j, join) in input.joins.iter().enumerate() {
            let PosedJoin::Node(n) = *join else { continue };
            let r = run_of[j];
            if run_of[j + 1] != r {
                continue;
            }
            let (x, y) = (&legs[j], &legs[j + 1]);
            if view.view_node_of_base(n).is_none() {
                stats.refused_by_rule[0] += 1;
                continue;
            }
            // Un angle indéterminé (géométrie dégénérée) n'est pas un demi-tour avéré : la couture est refusée par
            // prudence, ce qui ne peut que sous-estimer la couverture.
            let ok_turn = x.edge != y.edge && turn_deg(e, x, y).is_some_and(|t| t <= e.params.max_turn_deg);
            if !ok_turn {
                stats.refused_by_rule[1] += 1;
                continue;
            }
            let before =
                cum_leg_end_m[j] - if run_start(r) == 0 { 0.0 } else { cum_leg_end_m[run_start(r) - 1] };
            let total = cum_leg_end_m[run_ends[r]]
                - if run_start(r) == 0 { 0.0 } else { cum_leg_end_m[run_start(r) - 1] };
            let chords: f64 = input.joins[prev_join..j]
                .iter()
                .map(|jn| if let PosedJoin::Hole { chord_m } = jn { *chord_m } else { 0.0 })
                .sum();
            prev_join = j;
            // Abscisse de marche : jambes et cordes des trous jusqu'à la jonction.
            let holes_before: f64 = input.joins[..j]
                .iter()
                .map(|jn| if let PosedJoin::Hole { chord_m } = jn { *chord_m } else { 0.0 })
                .sum();
            cands.push(Cand {
                join: j,
                node: n,
                a_m: cum_leg_end_m[j] + holes_before,
                before_m: before,
                after_m: total - before,
                slack_m: 3.0 * chords,
            });
        }
        let points: Vec<SeamPoint> = cands
            .iter()
            .map(|c| SeamPoint { latlon: e7_deg(e.pos(c.node)), a_m: c.a_m, slack_m: c.slack_m })
            .collect();
        let (proj, anomalies) = project(&line, &s, &points, tune.proj_tol_m);
        if anomalies > 0 {
            return Err(PoseError::OrderAnomaly { seams: anomalies });
        }
        let seams: Vec<PosedSeam> = cands
            .iter()
            .zip(&proj)
            .map(|(c, &sx)| PosedSeam {
                join: c.join,
                node: c.node,
                s_m: sx,
                a_m: c.a_m,
                before_m: c.before_m,
                after_m: c.after_m,
            })
            .collect();
        stats.refused_by_rule[2] = seams
            .iter()
            .filter(|x| !(x.before_m >= tune.seam_margin_m && x.after_m >= tune.seam_margin_m))
            .count() as u32;
        Ok(Self {
            ride_type: input.ride_type,
            legs: legs.to_vec(),
            joins: input.joins.to_vec(),
            line: line_e7,
            s,
            l_m,
            leg_m,
            cum_leg_end_m,
            seams,
            stats,
            margin_m: tune.seam_margin_m,
            proj_tol_m: tune.proj_tol_m,
        })
    }

    pub fn ride_type(&self) -> RideType {
        self.ride_type
    }

    /// Longueur de la polyligne d'origine, m.
    pub fn length_m(&self) -> f64 {
        self.l_m
    }

    /// Les coutures candidates (règles 1 et 3), dans l'ordre de la marche ; les marges sont à lire (`valid_both`,
    /// `valid_before`).
    pub fn seams(&self) -> &[PosedSeam] {
        &self.seams
    }

    pub fn stats(&self) -> &PoseStats {
        &self.stats
    }

    pub fn legs(&self) -> &[PosedLeg] {
        &self.legs
    }

    pub fn joins(&self) -> &[PosedJoin] {
        &self.joins
    }

    /// Longueur graphe de chaque jambe, m.
    pub fn leg_m(&self) -> &[f64] {
        &self.leg_m
    }

    /// Longueur graphe cumulée à la fin de la jambe `j`, m.
    pub fn leg_end_m(&self, j: usize) -> f64 {
        self.cum_leg_end_m[j]
    }

    /// Valide pour raccourcir ou allonger : marge des deux côtés.
    pub fn valid_both(&self, k: usize) -> bool {
        let s = &self.seams[k];
        s.before_m >= self.margin_m && s.after_m >= self.margin_m
    }

    /// Valide pour déplacer l'arrivée : seule la marge avant compte.
    pub fn valid_before(&self, k: usize) -> bool {
        self.seams[k].before_m >= self.margin_m
    }

    pub fn proj_tol_m(&self) -> f64 {
        self.proj_tol_m
    }

    /// Dernier point de la polyligne d'origine.
    pub fn end_point(&self) -> LatLon {
        LatLon::from_e7(*self.line.last().expect("polyligne non vide"))
    }

    /// Empreinte de la marche (arêtes et sens), pour tirer les cases : stable d'un appel à l'autre.
    pub(crate) fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for l in &self.legs {
            h = (h ^ (u64::from(l.edge) << 1 | u64::from(l.reverse))).wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^ self.l_m.to_bits()
    }

    /// Géométrie d'origine entre deux abscisses, au point près (découpe interpolée aux bouts).
    pub fn line_between(&self, s0: f64, s1: f64) -> Vec<LatLon> {
        let (s0, s1) = (s0.clamp(0.0, self.l_m), s1.clamp(0.0, self.l_m));
        if s1 < s0 {
            return Vec::new();
        }
        let at = |x: f64| -> (i32, i32) {
            let i = self.s.partition_point(|&v| v <= x).saturating_sub(1).min(self.line.len() - 2);
            let (a, b) = (self.line[i], self.line[i + 1]);
            let len = self.s[i + 1] - self.s[i];
            let t = if len > 0.0 { ((x - self.s[i]) / len).clamp(0.0, 1.0) } else { 0.0 };
            (a.0 + (f64::from(b.0 - a.0) * t).round() as i32, a.1 + (f64::from(b.1 - a.1) * t).round() as i32)
        };
        let mut out: Vec<(i32, i32)> = vec![at(s0)];
        for (k, &p) in self.line.iter().enumerate() {
            if self.s[k] > s0 && self.s[k] < s1 && out.last() != Some(&p) {
                out.push(p);
            }
        }
        let end = at(s1);
        if out.last() != Some(&end) {
            out.push(end);
        }
        out.into_iter().map(LatLon::from_e7).collect()
    }

    /// Projette un point sur la polyligne, dans l'intervalle d'abscisses `[lo, hi]` : `(distance m, abscisse)` du plus proche
    /// segment ; le **dernier** des segments à `tol_m` ou moins s'il y en a, sinon le plus proche.
    pub(crate) fn project_last(&self, p: (i32, i32), lo: f64, hi: f64, tol_m: f64) -> Option<(f64, f64)> {
        let line: Vec<(f64, f64)> = self.line.iter().copied().map(e7_deg).collect();
        let q = e7_deg(p);
        let nseg = line.len() - 1;
        let i0 = self.s.partition_point(|&x| x < lo).saturating_sub(1).min(nseg);
        let i1 = self.s.partition_point(|&x| x <= hi).min(nseg);
        let mut best: Option<(f64, f64)> = None;
        let mut last_ok: Option<(f64, f64)> = None;
        for i in i0..i1 {
            let (d, sx) = on_segment(&line, &self.s, i, q);
            if d <= tol_m {
                last_ok = Some((d, sx));
            }
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, sx));
            }
        }
        last_ok.or(best)
    }

    /// Les étapes (jambes entières) qui précèdent la jonction `join` jusqu'à `m` mètres de graphe, coupées au premier trou ou
    /// contact : le contexte gardé d'un tronçon neuf qui part de cette jonction. Dans l'ordre de la marche.
    pub(crate) fn context_before(&self, join: usize, m: f64) -> Vec<crate::Step> {
        let mut k = join;
        let mut acc = 0.0;
        let mut out = Vec::new();
        loop {
            out.push(crate::Step { base_edge: self.legs[k].edge, reverse: self.legs[k].reverse });
            acc += self.leg_m[k];
            if acc >= m || k == 0 || !matches!(self.joins[k - 1], PosedJoin::Node(_)) {
                break;
            }
            k -= 1;
        }
        out.reverse();
        out
    }

    /// Les étapes qui suivent la jonction `join` jusqu'à `m` mètres de graphe, coupées au premier trou ou contact.
    pub(crate) fn context_after(&self, join: usize, m: f64) -> Vec<crate::Step> {
        let mut k = join + 1;
        let mut acc = 0.0;
        let mut out = Vec::new();
        while k < self.legs.len() {
            out.push(crate::Step { base_edge: self.legs[k].edge, reverse: self.legs[k].reverse });
            acc += self.leg_m[k];
            if acc >= m || k + 1 >= self.legs.len() || !matches!(self.joins[k], PosedJoin::Node(_)) {
                break;
            }
            k += 1;
        }
        out
    }
}
