//! Les recherches (note lot 2, §4 et §5.1) : arbres aller et retour sur les arêtes de vue orientées avec la règle
//! de virage, puis A* ciblés d'heuristique « arbre retour ». Les états sont des **arêtes de vue orientées**
//! (`champ = arête << 1 | sens`, sens 0 = de `a` vers `b`), étiquetés d'un coût entier et de mètres exacts.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use ord_graph::format::DIR_BIT;

use crate::cost::{self, RideType, TASTE_CLASSES};
use crate::engine::{Engine, Snap, ViewData};
use crate::geo::{heading_deg, polyline_m, Local};
use crate::team::{ChainTeam, TeamCtx, TypeTeam, BRIDGED, TEAM};
use crate::turns::{Lead, OrgSearch};
use crate::verify::slice;
use crate::Mode;
use ord_graph::view::{heading_to_u16, polyline_heading};

/// Transitions `(état d'arrivée, état de départ)` que le contrôle a refusées : interdites aux recherches suivantes.
pub(crate) type Forbidden = rustc_hash::FxHashSet<(u32, u32)>;

pub(crate) const NONE: u32 = u32::MAX;
pub(crate) const INF: u32 = u32::MAX;

/// Coupé par l'appelant.
#[derive(Debug)]
pub(crate) struct Stopped;

#[derive(Clone, Copy, Default, Debug)]
struct Slot {
    stamp: u32,
    d: u32,
    pred: u32,
    m: u32,
}

/// Un arbre de plus courts chemins sur les états orientés, remis à zéro par estampille.
#[derive(Default, Debug)]
pub(crate) struct Tree {
    slots: Vec<Slot>,
    cur: u32,
    nstamp: Vec<u32>,
    nbest: Vec<u32>,
    /// Nœuds atteints, dans l'ordre de première fixation.
    pub reached: Vec<u32>,
    /// États fixés, dans l'ordre.
    pub settled: Vec<u32>,
}

impl Tree {
    /// Prépare l'arbre pour `n_fields` états et `n_nodes` nœuds. Les tableaux d'un usage plus grand sont gardés tant
    /// qu'ils ne dépassent pas le double (lot 6, §9) : l'estampille croît strictement, une case d'un usage précédent
    /// n'est jamais lue comme fixée, et seul le préfixe sert.
    pub fn begin(&mut self, n_fields: usize, n_nodes: usize) {
        let fits = |have: usize, need: usize| have >= need && have <= need.saturating_mul(2).max(64);
        if !fits(self.slots.len(), n_fields) || !fits(self.nstamp.len(), n_nodes) {
            self.slots = vec![Slot::default(); n_fields];
            self.nstamp = vec![0; n_nodes];
            self.nbest = vec![NONE; n_nodes];
            self.cur = 0;
        }
        self.cur += 1;
        self.reached.clear();
        self.settled.clear();
    }

    /// Octets tenus (capacités réelles).
    pub fn bytes(&self) -> usize {
        self.slots.capacity() * std::mem::size_of::<Slot>()
            + (self.nstamp.capacity()
                + self.nbest.capacity()
                + self.reached.capacity()
                + self.settled.capacity())
                * 4
    }

    #[inline]
    pub fn get(&self, f: u32) -> u32 {
        let s = &self.slots[f as usize];
        if s.stamp == self.cur {
            s.d
        } else {
            INF
        }
    }

    #[inline]
    pub fn pred(&self, f: u32) -> u32 {
        self.slots[f as usize].pred
    }

    #[inline]
    pub fn meters(&self, f: u32) -> u32 {
        self.slots[f as usize].m
    }

    #[inline]
    pub(crate) fn set(&mut self, f: u32, d: u32, pred: u32, m: u32) {
        self.slots[f as usize] = Slot { stamp: self.cur, d, pred, m };
    }

    #[inline]
    fn note_node(&mut self, v: u32, f: u32) {
        if self.nstamp[v as usize] != self.cur {
            self.nstamp[v as usize] = self.cur;
            self.nbest[v as usize] = f;
            self.reached.push(v);
        }
    }

    /// Meilleur état qui arrive (aller) ou part (retour) du nœud.
    #[inline]
    pub fn node_best(&self, v: u32) -> Option<u32> {
        (self.nstamp[v as usize] == self.cur).then(|| self.nbest[v as usize])
    }

    /// Chemin de départ à `f` : les états dans l'ordre du parcours (suit les prédécesseurs).
    pub fn path_to(&self, f: u32) -> Vec<u32> {
        let mut out = vec![f];
        let mut cur = f;
        while self.pred(cur) != NONE {
            cur = self.pred(cur);
            out.push(cur);
        }
        out.reverse();
        out
    }
}

/// Espace de travail d'un fil : états des recherches par vue, alloués au premier usage.
#[derive(Default, Debug)]
pub struct Workspace {
    pub(crate) views: [ViewWs; 2],
    /// Retouche, lot 0 : chaînes interdites pour le prochain appel de `generate_inner`, qui le prend (`take`).
    #[cfg(feature = "probe")]
    pub(crate) keep_out: Option<crate::probe_graft::KeepOut>,
    /// Sonde μ (lot 5, §3.4) : multiplicateur du non-revêtu des prochaines générations (en 256es), conservé d'un appel à
    /// l'autre : le banc le pose avant chaque jeu, `None` pour le neutre.
    #[cfg(feature = "probe")]
    pub(crate) mu_q8: Option<u32>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retouche, lot 0 (sonde) : interdit ces chaînes à **toute** la prochaine génération, arbres, A* et accroche compris.
    /// L'appel de `generate_inner` prend le masque : il faut le poser avant chaque appel.
    #[cfg(feature = "probe")]
    pub fn set_keep_out(&mut self, ko: Option<crate::probe_graft::KeepOut>) {
        self.keep_out = ko;
    }

    /// Sonde μ (lot 5, §3.4) : le multiplicateur de la prochaine génération et des suivantes, jusqu'au prochain appel.
    #[cfg(feature = "probe")]
    pub fn set_mu_q8(&mut self, mu_q8: Option<u32>) {
        self.mu_q8 = mu_q8;
    }

    /// Octets tenus par les trois arbres, le `ban` et la trame du couloir de chaque vue (capacités réelles).
    pub fn bytes(&self) -> usize {
        self.views
            .iter()
            .map(|v| v.t1.bytes() + v.t2.bytes() + v.ta.bytes() + v.ban.capacity() * 4 + v.cor.bytes())
            .sum()
    }

    /// Libère les tableaux si l'espace de travail tient plus de `keep_bytes` (§9) : le prochain usage les réalloue.
    pub fn shrink_to(&mut self, keep_bytes: usize) {
        if self.bytes() > keep_bytes {
            *self = Self::default();
        }
    }
}

#[derive(Default, Debug)]
pub(crate) struct ViewWs {
    pub t1: Tree,
    pub t2: Tree,
    pub ta: Tree,
    /// Interdits, queues et couloir du retour, par arête de vue : `génération << 3 | bits`.
    pub ban: Vec<u32>,
    pub ban_gen: u32,
    /// Trame du couloir (lot 4, 4c, §15.6), réutilisée d'une recherche à l'autre ; vide en `Classic`.
    pub cor: CorGrid,
}

pub(crate) const BAN: u32 = 1;
pub(crate) const TAIL: u32 = 2;

// --- Poids ---------------------------------------------------------------------------------------------------

/// Poids des arêtes pour une requête : type, mode, familiarité, équipe.
pub(crate) struct Weighter<'a> {
    ride: RideType,
    mode: Mode,
    w_osm: &'a [u32],
    /// Goût (sonde du lot 4, §12) : multiplicateurs par classe d'une arête de base, pour `edge`.
    taste: Option<&'a [u16; TASTE_CLASSES]>,
    ct: Option<&'a [ChainTeam]>,
    tt: Option<&'a TypeTeam>,
    /// λ en 256es.
    lambda_q8: u64,
    mt: [u32; 256],
    /// Bornes de roulabilité (lot 5, §6.3) : octet par chaîne (`ViewData::unrideable`), **vide** sans bornes, en strict
    /// et hors vue gravel ; décalage du type dans l'octet ; chaînes que l'équipe libère ; arêtes non roulables exemptées.
    unr: &'a [u8],
    unr_shift: u32,
    freed: &'a [u32],
    exempt: Option<&'a TypeTeam>,
    ride_ok: bool,
    /// Retouche, lot 0 (sonde) : bitset des chaînes interdites à toute la requête.
    #[cfg(feature = "probe")]
    keep_out: Option<&'a [u64]>,
    /// Consigne de part (lot 5, §4.2) : le multiplicateur μ du non-revêtu, en 256es (256 : neutre), et la part `offroad` de
    /// chaque chaîne de la vue (`ViewData::off_q8`). `None` : 1.14.
    mu_q8: Option<u32>,
    off_q8: &'a [u8],
}

impl<'a> Weighter<'a> {
    pub fn new(
        v: &'a ViewData,
        team: Option<TeamCtx<'a>>,
        ride: RideType,
        mode: Mode,
        familiarity: f32,
        off_team_cost_q8: u32,
    ) -> Self {
        let tt = match (mode, team) {
            (Mode::Discovery, _) | (_, None) => None,
            (_, Some(t)) => Some((t.data.of(ride), t.win.chains_of(ride))),
        };
        let fam = (familiarity.clamp(0.0, 1.0) * 1000.0).round() as u32;
        // Indépendant du mode : le goût s'applique aussi en découverte dès qu'il y a une surcouche (décision 1 du §9.1).
        let taste = team.and_then(|t| t.win.taste_of(ride));
        Self {
            ride,
            mode,
            w_osm: taste.map_or(&v.w_osm[ride.index()][..], |(w, _)| w),
            taste: taste.map(|(_, q)| q),
            ct: tt.map(|t| t.1),
            tt: tt.map(|t| t.0),
            lambda_q8: u64::from(off_team_cost_q8),
            mt: cost::multiplier_table(fam),
            // Aucun effet en strict (§6.3) : il ne prend que des arêtes d'équipe et reste celui de 1.14.
            unr: if mode == Mode::Strict { &[] } else { &v.unrideable },
            unr_shift: crate::engine::ride_shift(ride),
            freed: team.map_or(&[][..], |t| t.win.freed_of(ride)),
            // L'exemption suit la surcouche, quel que soit le mode (découverte comprise) ; sans surcouche, aucune.
            exempt: team.map(|t| t.data.of(ride)),
            ride_ok: mode != Mode::Strict && !v.unrideable.is_empty(),
            #[cfg(feature = "probe")]
            keep_out: None,
            mu_q8: None,
            off_q8: &v.off_q8,
        }
    }

    /// Pose le multiplicateur μ (lot 5, §4.2) ; `None` ou 256 : aucun effet.
    pub fn with_mu_q8(mut self, mu_q8: Option<u32>) -> Self {
        self.mu_q8 = mu_q8;
        self
    }

    /// `⌈w × (256 + (μ − 256) × f / 256) / 256⌉` (§4.2), en entiers : même poids en natif et en WASM. `INF` reste `INF`.
    #[inline]
    pub(crate) fn mu_of(&self, w: u32, f: u8) -> u32 {
        let Some(mu) = self.mu_q8 else { return w };
        if w == INF || mu == 256 {
            return w;
        }
        let t = 65_536 + (i64::from(mu) - 256) * i64::from(f);
        debug_assert!(t > 0, "μ ≥ 1/4 : le facteur reste positif");
        ((u64::from(w) * t as u64 + 65_535) >> 16).min(u64::from(u32::MAX / 2)) as u32
    }

    /// Pose le masque des chaînes interdites (sonde). Une chaîne masquée pèse `INF` : absente du graphe de la requête.
    #[cfg(feature = "probe")]
    pub fn with_keep_out(mut self, ko: Option<&'a [u64]>) -> Self {
        self.keep_out = ko;
        self
    }

    #[inline]
    fn mul(&self, a: u32, z: u8) -> u64 {
        (u64::from(a) * u64::from(self.mt[z as usize]) + 128) >> 8
    }

    /// Coût d'un poids hors équipe : λ × `w`, arrondi (exact pour les entiers de λ).
    #[inline]
    fn off(&self, w: u64) -> u64 {
        (self.lambda_q8 * w + 128) >> 8
    }

    /// Poids d'une arête de vue, `INF` si elle est interdite (strict, hors réseau d'équipe).
    #[inline]
    pub fn chain(&self, c: u32) -> u32 {
        let w = self.chain_plain(c);
        // Toutes les branches de `chain_plain` y passent, le `return w` sans surcouche compris (§4.2).
        let w = self.mu_of(w, self.off_q8.get(c as usize).copied().unwrap_or(0));
        w
    }

    #[inline]
    fn chain_plain(&self, c: u32) -> u32 {
        if !self.unr.is_empty() {
            let lv = self.unr[c as usize] >> self.unr_shift & 3;
            if lv & 2 != 0 || (lv == 1 && self.freed.binary_search(&c).is_err()) {
                return INF;
            }
        }
        #[cfg(feature = "probe")]
        if let Some(k) = self.keep_out {
            if k[(c >> 6) as usize] >> (c & 63) & 1 != 0 {
                return INF;
            }
        }
        let w = self.w_osm[c as usize];
        let Some(ct) = self.ct else { return w };
        let t = ct[c as usize];
        let rest = u64::from(w.saturating_sub(t.a).saturating_sub(t.b));
        match self.mode {
            Mode::Strict => {
                if t.strict {
                    (self.mul(t.a, t.z) + u64::from(t.b)).min(u64::from(u32::MAX / 2)) as u32
                } else {
                    INF
                }
            }
            _ => (self.off(rest) + self.mul(t.a, t.z) + u64::from(t.b)).min(u64::from(u32::MAX / 2)) as u32,
        }
    }

    /// Poids d'une arête de base (pour les morceaux partiels du départ et de l'arrivée).
    pub fn edge<B: AsRef<[u8]>>(&self, eng: &Engine<B>, e: u32) -> u32 {
        let w = self.edge_plain(eng, e);
        // Même crochet que `chain` ; une arête entière `offroad` pèse 255/256 comme une chaîne qui l'est tout entière.
        // Comme pour `chain`, `f` n'existe que là où la vue a son tableau (`off_q8`, vue gravel) : sur la vue route,
        // `f = 0` pour les arêtes partielles aussi, sinon μ y agirait sur le départ et l'arrivée seulement.
        if self.mu_q8.is_some() && !self.off_q8.is_empty() && eng.attrs_len(e).0.offroad() {
            self.mu_of(w, 255)
        } else {
            w
        }
    }

    fn edge_plain<B: AsRef<[u8]>>(&self, eng: &Engine<B>, e: u32) -> u32 {
        let (attrs, len) = eng.attrs_len(e);
        if self.ride_ok {
            match crate::engine::ride_level(self.ride, attrs) {
                crate::engine::RIDE_OK => {}
                crate::engine::RIDE_SOFT if self.exempt.is_some_and(|t| t.exempts(e)) => {}
                _ => return INF,
            }
        }
        let w = match self.taste {
            None => eng.base_weight(self.ride, attrs, len),
            Some(q) => cost::taste_weight(self.ride, attrs, len, q),
        };
        let Some(tt) = self.tt else { return w };
        let (f, z) = tt.flags(e);
        if f & TEAM != 0 {
            self.mul(w, z).min(u64::from(u32::MAX / 2)) as u32
        } else if f & BRIDGED != 0 {
            w
        } else if self.mode == Mode::Strict {
            INF
        } else {
            self.off(u64::from(w)).min(u64::from(u32::MAX / 2)) as u32
        }
    }

    pub fn strict(&self) -> bool {
        self.mode == Mode::Strict
    }

    pub fn tt(&self) -> Option<&TypeTeam> {
        self.tt
    }
}

// --- Règle de virage -------------------------------------------------------------------------------------------

pub(crate) const TURN_MAX16: u32 = 27_307; // ceil(150 / 180 * 32768)
pub(crate) const TURN_FREE16: u32 = 8_192; // 45°
pub(crate) const TURN_COST_MAX: u32 = 2_000; // 200 m équivalents (en décimètres pondérés), à 150°

#[inline]
pub(crate) fn in_head(v: &ViewData, f: u32) -> u16 {
    let h = v.head[(f >> 1) as usize];
    if f & 1 == 0 {
        h[1]
    } else {
        h[0].wrapping_add(32_768)
    }
}

#[inline]
pub(crate) fn out_head(v: &ViewData, f: u32) -> u16 {
    let h = v.head[(f >> 1) as usize];
    if f & 1 == 0 {
        h[0]
    } else {
        h[1].wrapping_add(32_768)
    }
}

#[inline]
pub(crate) fn angle16(a: u16, b: u16) -> u32 {
    (i32::from(b.wrapping_sub(a) as i16)).unsigned_abs()
}

fn unit(h: u16) -> (f32, f32) {
    let r = f32::from(h) * (std::f32::consts::TAU / 65_536.0);
    (r.sin(), r.cos())
}

/// Cap d'arrivée de `f`, prolongé par le prédécesseur si la chaîne fait moins de 50 m (note §4).
#[inline]
pub(crate) fn in_head_ext(v: &ViewData, f: u32, pred: u32) -> u16 {
    let own = in_head(v, f);
    let l = v.len_dm[(f >> 1) as usize];
    if l >= 500 || pred == NONE {
        return own;
    }
    let (so, co) = unit(own);
    let (sp, cp) = unit(in_head(v, pred));
    let w = l as f32 / 500.0;
    let (x, y) = (w * so + (1.0 - w) * sp, w * co + (1.0 - w) * cp);
    let deg = x.atan2(y).to_degrees();
    ((deg.rem_euclid(360.0) / 360.0 * 65_536.0).round() as u32 & 0xffff) as u16
}

/// Contrôle **rétroactif** du virage qui précède une chaîne courte `f` (< 50 m), une fois son successeur `fo`
/// connu : la fenêtre de cap de départ de `f` déborde sur `fo`, que `turn_cost` ne peut pas voir.
#[inline]
pub(crate) fn retro_ok(v: &ViewData, pe: u32, f: u32, fo: u32) -> bool {
    if pe == NONE || v.len_dm[(f >> 1) as usize] >= 500 {
        return true;
    }
    let own = out_head(v, f);
    let (so, co) = unit(own);
    let (sn, cn) = unit(out_head(v, fo));
    let w = v.len_dm[(f >> 1) as usize] as f32 / 500.0;
    let (x, y) = (w * so + (1.0 - w) * sn, w * co + (1.0 - w) * cn);
    let deg = x.atan2(y).to_degrees();
    let ext = ((deg.rem_euclid(360.0) / 360.0 * 65_536.0).round() as u32 & 0xffff) as u16;
    !(angle16(in_head(v, pe), ext) >= TURN_MAX16 && v.deg[v.tgt(pe) as usize] >= 3)
}

/// Coût de virage de `fe` vers `fo`, `None` si interdit (demi-tour, ≥ 150° à un nœud de degré ≥ 3).
#[inline]
pub(crate) fn turn_cost(
    v: &ViewData,
    forb: &Forbidden,
    fe: u32,
    pred: u32,
    fo: u32,
    hin_over: Option<u16>,
    hout_over: Option<u16>,
) -> Option<u32> {
    if fo == fe ^ 1 || (!forb.is_empty() && forb.contains(&(fe, fo))) {
        return None;
    }
    let hin = hin_over.unwrap_or_else(|| in_head_ext(v, fe, pred));
    let hout = hout_over.unwrap_or_else(|| out_head(v, fo));
    let a = angle16(hin, hout);
    if a >= TURN_MAX16 && v.deg[v.tgt(fe) as usize] >= 3 {
        return None;
    }
    Some(if a > TURN_FREE16 {
        (((a - TURN_FREE16) as u64 * u64::from(TURN_COST_MAX) / u64::from(TURN_MAX16 - TURN_FREE16)) as u32)
            .min(TURN_COST_MAX + 500)
    } else {
        0
    })
}

// --- Accroche, disque -------------------------------------------------------------------------------------------

/// Position de départ ou d'arrivée sur une arête de vue, avec les coûts des morceaux partiels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub snap: Snap,
    /// Départ : de la position vers `b` (sens 0) ou vers `a` (sens 1).
    pub out_cost: [u32; 2],
    pub out_len: [u32; 2],
    /// Arrivée : de `a` vers la position (sens 0) ou de `b` vers la position (sens 1).
    pub in_cost: [u32; 2],
    pub in_len: [u32; 2],
    /// Cap à l'arrivée du morceau partiel de départ (au bout `b` pour le sens 0, au bout `a` pour le sens 1).
    pub out_head: [u16; 2],
    /// Cap au départ du morceau partiel d'arrivée (depuis `a` pour le sens 0, depuis `b` pour le sens 1).
    pub in_head: [u16; 2],
}

type Points = Vec<(i32, i32)>;

fn extend_dedupe(acc: &mut Points, pts: Points) {
    let skip = usize::from(acc.last().is_some() && acc.last() == pts.first());
    acc.extend(pts.into_iter().skip(skip));
}

pub(crate) fn make_anchor<B: AsRef<[u8]>>(
    e: &Engine<B>,
    v: &ViewData,
    w: &Weighter<'_>,
    snap: Snap,
) -> Anchor {
    let links = v.links(snap.chain);
    let g = e.geom_len_m(snap.edge).max(1e-6);
    let frac = (snap.offset_m / g).clamp(0.0, 1.0);
    let fb = if snap.rev { 1.0 - frac } else { frac };
    let (mut b_cost, mut b_len, mut a_cost, mut a_len) = (0u64, 0u64, 0u64, 0u64);
    let (mut b_inf, mut a_inf) = (false, false);
    for (li, &l) in links.iter().enumerate() {
        let ed = l & !DIR_BIT;
        let ew = w.edge(e, ed);
        let len = u64::from(e.len_dm(ed));
        let (fb_i, fa_i) = if li < snap.link {
            (1.0, 0.0)
        } else if li > snap.link {
            (0.0, 1.0)
        } else {
            (fb, 1.0 - fb)
        };
        let part = |f: f64| (f * f64::from(if ew == INF { 0 } else { ew })).round() as u64;
        if fb_i > 0.0 {
            b_inf |= ew == INF;
            b_cost += part(fb_i);
            b_len += (fb_i * len as f64).round() as u64;
        }
        if fa_i > 0.0 {
            a_inf |= ew == INF;
            a_cost += part(fa_i);
            a_len += (fa_i * len as f64).round() as u64;
        }
    }
    let cost = |x: u64, inf: bool, open: bool| {
        if inf || !open {
            INF
        } else {
            x.min(u64::from(u32::MAX / 2)) as u32
        }
    };
    let (o0, o1) = (v.open[snap.chain as usize] & 1 != 0, v.open[snap.chain as usize] & 2 != 0);
    // Caps des morceaux partiels, sur leur géométrie réelle.
    let (mut before, mut after) = (Points::new(), Points::new());
    for (li, &l) in links.iter().enumerate() {
        let (ed, rev) = (l & !DIR_BIT, l & DIR_BIT != 0);
        let mut nat = e.edge_polyline(ed);
        if li == snap.link {
            let total = polyline_m(&nat);
            let p0 = slice(&nat, 0.0, snap.offset_m);
            let p1 = slice(&nat, snap.offset_m, total);
            let (mut b_part, mut a_part) = (p0, p1);
            if rev {
                (b_part, a_part) = (a_part, b_part);
                b_part.reverse();
                a_part.reverse();
            }
            extend_dedupe(&mut before, b_part);
            extend_dedupe(&mut after, a_part);
        } else {
            if rev {
                nat.reverse();
            }
            if li < snap.link {
                extend_dedupe(&mut before, nat);
            } else {
                extend_dedupe(&mut after, nat);
            }
        }
    }
    let rev_of = |p: &Vec<(i32, i32)>| -> Vec<(i32, i32)> { p.iter().rev().copied().collect() };
    let hd = |p: &[(i32, i32)], start: bool| heading_to_u16(polyline_heading(p, start));
    let out_head = [hd(&after, false), hd(&rev_of(&before), false)];
    let in_head = [hd(&before, true), hd(&rev_of(&after), true)];
    Anchor {
        snap,
        out_cost: [cost(a_cost, a_inf, o0), cost(b_cost, b_inf, o1)],
        out_len: [a_len as u32, b_len as u32],
        in_cost: [cost(b_cost, b_inf, o0), cost(a_cost, a_inf, o1)],
        in_len: [b_len as u32, a_len as u32],
        out_head,
        in_head,
    }
}

/// Zone de recherche : disque de rayon D/2 autour du départ (boucle), ellipse de foyers départ et arrivée de
/// somme D (aller simple).
pub(crate) struct Disc {
    loc: Local,
    s: (f64, f64),
    e: (f64, f64),
    limit: f64,
    is_loop: bool,
}

impl Disc {
    pub fn new(s: (i32, i32), e: (i32, i32), d_m: f64, is_loop: bool) -> Self {
        let loc = Local::new(s);
        Self { loc, s: loc.xy(s), e: loc.xy(e), limit: if is_loop { d_m / 2.0 } else { d_m }, is_loop }
    }

    /// Position E7 en mètres locaux, repère du départ.
    #[inline]
    pub fn xy_of(&self, p: (i32, i32)) -> (f64, f64) {
        self.loc.xy(p)
    }

    #[inline]
    pub fn inside(&self, p: (i32, i32)) -> bool {
        let (x, y) = self.loc.xy(p);
        let ds = (x - self.s.0).hypot(y - self.s.1);
        if self.is_loop {
            ds <= self.limit
        } else {
            ds + (x - self.e.0).hypot(y - self.e.1) <= self.limit
        }
    }

    /// Cap depuis le point de référence (départ, ou milieu départ-arrivée pour un aller simple), en degrés.
    pub fn bearing_of(&self, p: (i32, i32)) -> f64 {
        let (x, y) = self.loc.xy(p);
        let r =
            if self.is_loop { self.s } else { ((self.s.0 + self.e.0) / 2.0, (self.s.1 + self.e.1) / 2.0) };
        heading_deg(x - r.0, y - r.1)
    }
}

pub(crate) struct Ctx<'a> {
    pub v: &'a ViewData,
    pub w: &'a Weighter<'a>,
    pub disc: &'a Disc,
    pub s: &'a Anchor,
    pub e: &'a Anchor,
    pub forb: &'a Forbidden,
    /// Levé quand la recherche touche un nœud incomplet de la fenêtre : le résultat est jeté (lot 6, §5.2).
    pub esc: &'a std::cell::Cell<bool>,
    /// Modèle des virages et interdiction de l'aller-retour (lot 4, 4c, §16) : `None` en `Classic`.
    pub org: Option<&'a OrgSearch>,
}

pub(crate) const POLL: u32 = 2048;

/// Arbre aller depuis le départ : états atteints, coût et mètres (dm) du meilleur chemin à chacun.
pub(crate) fn tree_forward(
    cx: &Ctx<'_>,
    t: &mut Tree,
    stop: &mut dyn FnMut() -> bool,
) -> Result<(), Stopped> {
    let v = cx.v;
    t.begin(2 * v.n_chains, v.n_nodes);
    let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    for dir in 0..2u32 {
        let c = cx.s.out_cost[dir as usize];
        if c != INF {
            let f = (cx.s.snap.chain << 1) | dir;
            t.set(f, c, NONE, cx.s.out_len[dir as usize]);
            heap.push(Reverse((c, f)));
        }
    }
    let mut polls = 0u32;
    while let Some(Reverse((d, f))) = heap.pop() {
        if d > t.get(f) {
            continue;
        }
        t.settled.push(f);
        polls += 1;
        if polls.is_multiple_of(POLL) && stop() {
            return Err(Stopped);
        }
        let node = v.tgt(f);
        if !v.complete[node as usize] {
            cx.esc.set(true);
            return Err(Stopped);
        }
        t.note_node(node, f);
        let (pred, m) = (t.pred(f), t.meters(f));
        for &fo in v.out(node) {
            let c = fo >> 1;
            let w = cx.w.chain(c);
            if w == INF || !cx.disc.inside(v.pos[v.tgt(fo) as usize]) {
                continue;
            }
            let hin_over = (pred == NONE).then(|| cx.s.out_head[(f & 1) as usize]);
            let Some(tc) = turn_cost(v, cx.forb, f, pred, fo, hin_over, None) else { continue };
            if !retro_ok(v, pred, f, fo) {
                continue;
            }
            if let Some(o) = cx.org {
                if o.refuses_fwd(cx, t, &Lead::NONE, f, fo, m, d, true) {
                    continue;
                }
            }
            let nd = d.saturating_add(w).saturating_add(tc);
            if nd < t.get(fo) {
                t.set(fo, nd, f, m + v.len_dm[c as usize]);
                heap.push(Reverse((nd, fo)));
            }
        }
    }
    Ok(())
}

/// Arbre retour vers l'arrivée : le coût du meilleur chemin **qui commence** par chaque état et finit à l'arrivée.
pub(crate) fn tree_backward(
    cx: &Ctx<'_>,
    t: &mut Tree,
    stop: &mut dyn FnMut() -> bool,
) -> Result<(), Stopped> {
    let v = cx.v;
    t.begin(2 * v.n_chains, v.n_nodes);
    let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    for dir in 0..2u32 {
        let c = cx.e.in_cost[dir as usize];
        if c != INF {
            let f = (cx.e.snap.chain << 1) | dir;
            t.set(f, c, NONE, cx.e.in_len[dir as usize]);
            heap.push(Reverse((c, f)));
        }
    }
    let mut polls = 0u32;
    while let Some(Reverse((d, f))) = heap.pop() {
        if d > t.get(f) {
            continue;
        }
        t.settled.push(f);
        polls += 1;
        if polls.is_multiple_of(POLL) && stop() {
            return Err(Stopped);
        }
        let node = v.src(f);
        if !v.complete[node as usize] {
            cx.esc.set(true);
            return Err(Stopped);
        }
        t.note_node(node, f);
        let m = t.meters(f);
        for &fg in v.inn(node) {
            let c = fg >> 1;
            let w = cx.w.chain(c);
            if w == INF || !cx.disc.inside(v.pos[v.src(fg) as usize]) {
                continue;
            }
            let hout_over = (t.pred(f) == NONE).then(|| cx.e.in_head[(f & 1) as usize]);
            let Some(tc) = turn_cost(v, cx.forb, fg, NONE, f, None, hout_over) else { continue };
            if let Some(o) = cx.org {
                // Aucune exemption au départ : un chemin de T2 est toujours la fin du parcours, à des kilomètres du départ
                // le long du parcours, même quand il en passe près (boucle) ; lue sur T1, l'exemption laisserait passer
                // des grappes que le contrôle refuse à l'approche de l'arrivée. Le symétrique de T1, qui n'a pas
                // d'exemption à l'arrivée (§16.13 de la note).
                if o.refuses_bwd(cx, t, &Lead::NONE, fg, f, m, d, true) {
                    continue;
                }
            }
            let nd = d.saturating_add(w).saturating_add(tc);
            if nd < t.get(fg) {
                t.set(fg, nd, f, m + v.len_dm[c as usize]);
                heap.push(Reverse((nd, fg)));
            }
        }
    }
    Ok(())
}

/// Ce que l'A* a trouvé : les états depuis le point de demi-tour jusqu'à l'arrivée (le dernier est partiel).
#[derive(Debug)]
pub(crate) struct Found {
    pub fields: Vec<u32>,
    pub meters: u32,
}

/// Interdits et pénalités du retour, et couloir (lot 4, 4c, §15.6).
pub(crate) struct Ban<'a> {
    pub stamp: &'a [u32],
    pub gen: u32,
    /// Trame du couloir : une arête dont le nœud d'arrivée y tombe coûte `kappa_q8 / 256` fois plus. `None` : sans
    /// couloir (toujours en `Classic`).
    pub cor: Option<&'a CorGrid>,
    pub kappa_q8: u32,
}

impl<'a> Ban<'a> {
    /// Sans couloir : les interdits et les queues seulement.
    pub(crate) fn plain(stamp: &'a [u32], gen: u32) -> Self {
        Self { stamp, gen, cor: None, kappa_q8: 256 }
    }

    /// Poids après les pénalités du couloir : `w · κ / 256` si le nœud tombe dans une case marquée.
    #[inline]
    pub(crate) fn corridor(&self, w: u32, cx: &Ctx<'_>, node: u32) -> u32 {
        match self.cor {
            Some(g) if g.hit(cx.disc.xy_of(cx.v.pos[node as usize])) => {
                ((u64::from(w) * u64::from(self.kappa_q8) + 128) >> 8).min(u64::from(u32::MAX / 2)) as u32
            }
            _ => w,
        }
    }

    /// Plafond d'une recherche de retour : `2 h0 + 10 000` ; avec couloir, `h0 (256 + κ) / 256 + 10 000` (κ = 1 redonne
    /// `2 h0 + 10 000`, le couloir ne fait que relever des coûts).
    #[inline]
    pub(crate) fn cap(&self, h0: u32) -> u32 {
        if self.cor.is_some() {
            let c = u64::from(h0) * (256 + u64::from(self.kappa_q8)) / 256 + 10_000;
            c.min(u64::from(u32::MAX)) as u32
        } else {
            h0.saturating_mul(2).saturating_add(10_000)
        }
    }

    #[inline]
    pub(crate) fn bits(&self, c: u32) -> u32 {
        let s = self.stamp[c as usize];
        if s >> 3 == self.gen {
            s & 7
        } else {
            0
        }
    }
}

/// A* depuis le nœud `from_node` (atteint par l'état `last`, de prédécesseur `last_pred`) jusqu'à l'arrivée,
/// d'heuristique « arbre retour » `t2`. `budget` : états fixés restants, décomptés ; `None` si rien n'est trouvé ou
/// si le budget est épuisé (`exhausted` l'indique).
#[allow(clippy::too_many_arguments)]
pub(crate) fn astar(
    cx: &Ctx<'_>,
    ta: &mut Tree,
    t2: &Tree,
    last: u32,
    last_pred: u32,
    ban: &Ban<'_>,
    lead: &Lead<'_>,
    budget: &mut u64,
    exhausted: &mut bool,
    stop: &mut dyn FnMut() -> bool,
) -> Result<Option<Found>, Stopped> {
    let v = cx.v;
    ta.begin(2 * v.n_chains, v.n_nodes);
    let from_node = v.tgt(last);
    let Some(h0) = t2.node_best(from_node).map(|b| t2.get(b)) else { return Ok(None) };
    let cap = ban.cap(h0);
    let end_chain = cx.e.snap.chain;
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let expand = |ta: &mut Tree,
                  heap: &mut BinaryHeap<Reverse<(u32, u32, u32)>>,
                  node: u32,
                  f_prev: u32,
                  pred_prev: u32,
                  hin_over: Option<u16>,
                  store: u32,
                  g: u32,
                  m: u32| {
        if !v.complete[node as usize] {
            cx.esc.set(true);
            return;
        }
        for &fo in v.out(node) {
            let c = fo >> 1;
            let bits = ban.bits(c);
            let terminal = c == end_chain;
            if !terminal && bits & BAN != 0 {
                continue;
            }
            let hout_over = terminal.then(|| cx.e.in_head[(fo & 1) as usize]);
            let Some(tc) = turn_cost(v, cx.forb, f_prev, pred_prev, fo, hin_over, hout_over) else {
                continue;
            };
            if !retro_ok(v, pred_prev, f_prev, fo) {
                continue;
            }
            if let Some(o) = cx.org {
                if o.refuses_fwd(cx, ta, lead, f_prev, fo, m, g, store != NONE) {
                    continue;
                }
            }
            let (w, len, h) = if terminal {
                let dir = (fo & 1) as usize;
                let w = cx.e.in_cost[dir];
                if w == INF {
                    continue;
                }
                (w, cx.e.in_len[dir], 0)
            } else {
                let mut w = cx.w.chain(c);
                if w == INF {
                    continue;
                }
                let tn = v.tgt(fo);
                if !cx.disc.inside(v.pos[tn as usize]) {
                    continue;
                }
                let Some(hb) = t2.node_best(tn) else { continue };
                if bits & TAIL != 0 {
                    w = w.saturating_mul(2);
                }
                w = ban.corridor(w, cx, tn);
                (w, v.len_dm[c as usize], t2.get(hb))
            };
            let ng = g.saturating_add(w).saturating_add(tc);
            if ng < ta.get(fo) {
                ta.set(fo, ng, store, m + len);
                heap.push(Reverse((ng.saturating_add(h), ng, fo)));
            }
        }
    };
    // Le premier pas part du nœud du demi-tour, avec le virage depuis le dernier état de l'aller.
    let first_over = (last_pred == NONE).then(|| cx.s.out_head[(last & 1) as usize]);
    expand(ta, &mut heap, from_node, last, last_pred, first_over, NONE, 0, 0);
    if cx.esc.get() {
        return Err(Stopped);
    }
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
        if (f >> 1) == end_chain {
            let mut fields = vec![f];
            let mut cur = f;
            while ta.pred(cur) != NONE {
                cur = ta.pred(cur);
                fields.push(cur);
            }
            fields.reverse();
            return Ok(Some(Found { fields, meters: ta.meters(f) }));
        }
        let node = v.tgt(f);
        let (pred, m) = (ta.pred(f), ta.meters(f));
        expand(ta, &mut heap, node, f, pred, None, f, g, m);
        if cx.esc.get() {
            return Err(Stopped);
        }
    }
    Ok(None)
}

// --- Couloir (lot 4, 4c, §15.6) ----------------------------------------------------------------------------------

/// Trame carrée du couloir : un bit par case, centrée au départ (repère `Local` du disque de la requête), les cases des
/// jambes déjà posées dilatées de `w`. Lue par position (`hit`) à la relaxation : poser un bit par chaîne demanderait un
/// index case → chaînes. Réutilisée d'une recherche à l'autre (`begin` la remet à zéro).
#[derive(Default, Debug)]
pub(crate) struct CorGrid {
    side: usize,
    cell: f64,
    half: f64,
    bits: Vec<u64>,
    /// Gabarit de la dilatation : les décalages `(dx, dy)` en cases d'un disque de rayon `radius = ⌈w / cell⌉`.
    stencil: Vec<(i32, i32)>,
    radius: i32,
}

impl CorGrid {
    /// Octets tenus (capacités réelles).
    pub fn bytes(&self) -> usize {
        self.bits.capacity() * 8 + self.stencil.capacity() * 8
    }

    /// Case de la trame : `clamp(w / 3, 100 m, 500 m)`.
    fn cell_of(w_m: f64) -> f64 {
        (w_m / 3.0).clamp(100.0, 500.0)
    }

    /// Remet la trame à zéro pour une requête de distance `d_m` et un couloir de demi-largeur `w_m` (demi-côté
    /// `d_m / 2 + w_m`).
    pub fn begin(&mut self, d_m: f64, w_m: f64) {
        let cell = Self::cell_of(w_m);
        let half = d_m / 2.0 + w_m;
        let side = ((2.0 * half / cell).ceil() as usize).max(1);
        let words = (side * side).div_ceil(64);
        if self.side == side && self.cell == cell && self.bits.len() == words {
            self.bits.iter_mut().for_each(|x| *x = 0);
        } else {
            self.bits.clear();
            self.bits.resize(words, 0);
        }
        // Le gabarit suit le rayon, pas la trame : deux requêtes à la même trame (case bornée à 500 m, côté égal) peuvent
        // demander deux rayons ; le garder d'une requête à l'autre ferait dépendre la sortie de la précédente.
        let r = (w_m / cell).ceil() as i32;
        if self.stencil.is_empty() || self.radius != r {
            self.stencil.clear();
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy <= r * r {
                        self.stencil.push((dx, dy));
                    }
                }
            }
            self.radius = r;
        }
        self.side = side;
        self.cell = cell;
        self.half = half;
    }

    #[inline]
    fn cell_xy(&self, p: (f64, f64)) -> Option<(i32, i32)> {
        let (cx, cy) = (((p.0 + self.half) / self.cell).floor(), ((p.1 + self.half) / self.cell).floor());
        let n = self.side as f64;
        (cx >= 0.0 && cy >= 0.0 && cx < n && cy < n).then_some((cx as i32, cy as i32))
    }

    /// La case de `p` (mètres locaux) est marquée.
    #[inline]
    pub fn hit(&self, p: (f64, f64)) -> bool {
        match self.cell_xy(p) {
            Some((cx, cy)) => {
                let i = cy as usize * self.side + cx as usize;
                self.bits[i >> 6] >> (i & 63) & 1 != 0
            }
            None => false,
        }
    }

    /// Pose la case de `p` et celles de son voisinage (dilatation).
    fn mark_point(&mut self, p: (f64, f64)) {
        let Some((cx, cy)) = self.cell_xy(p) else { return };
        let n = self.side as i32;
        for &(dx, dy) in &self.stencil {
            let (x, y) = (cx + dx, cy + dy);
            if x >= 0 && y >= 0 && x < n && y < n {
                let i = y as usize * self.side + x as usize;
                self.bits[i >> 6] |= 1 << (i & 63);
            }
        }
    }

    /// Marque une jambe (états `fields` dans l'ordre du parcours) : le segment entre les nœuds de chaque état,
    /// échantillonné tous les `cell / 2`, **sauf** les points dont l'abscisse le long de la jambe est à moins de
    /// `head_m` de son début ou de `tail_m` de sa fin (la queue du départ ou de l'arrivée, et la zone d'un sommet).
    pub fn mark_leg(&mut self, v: &ViewData, disc: &Disc, fields: &[u32], head_m: f64, tail_m: f64) {
        let total: f64 = fields.iter().map(|&f| f64::from(v.len_dm[(f >> 1) as usize])).sum::<f64>() / 10.0;
        let (lo, hi) = (head_m, total - tail_m);
        let mut acc = 0.0f64;
        for &f in fields {
            let l = f64::from(v.len_dm[(f >> 1) as usize]) / 10.0;
            let (a0, a1) = (acc, acc + l);
            acc = a1;
            if a1 < lo || a0 > hi {
                continue;
            }
            let p0 = disc.xy_of(v.pos[v.src(f) as usize]);
            let p1 = disc.xy_of(v.pos[v.tgt(f) as usize]);
            let seg = (p1.0 - p0.0).hypot(p1.1 - p0.1);
            let n = ((seg / (self.cell / 2.0)).ceil() as usize).max(1);
            for k in 0..=n {
                let t = k as f64 / n as f64;
                let a = a0 + t * l;
                if a < lo || a > hi {
                    continue;
                }
                self.mark_point((p0.0 + t * (p1.0 - p0.0), p0.1 + t * (p1.1 - p0.1)));
            }
        }
    }
}

/// Facteur minimal de coût par décimètre de la recherche point à point (lot 4, 4c, §15.0 point 11, §15.5) : 0,999 fois le
/// plus petit `chain(c) / len_dm(c)` des états fixés par l'arbre aller `t1` (chaînes de longueur non nulle, permises),
/// 0 s'il n'y en a pas.
///
/// Écart au §15.5 (§15.19 de la note) : le minimum est lu sur T1 et non sur la fenêtre. T1 fixe tout état atteignable du
/// départ dans le disque, donc (au prédécesseur près de la règle de rétro-virage) toute chaîne qu'une jambe 2 peut
/// relâcher : la borne vaut pour elles **sans** marge d'arrondi (`chain` est le poids exact, équipe et goût compris), et
/// elle ne dépend pas de la fenêtre servie (une fenêtre plus grande du cache rend la même sortie, I4). Le 0,999 absorbe
/// l'écart entre le vol d'oiseau dans le plan local et les longueurs arrondies au décimètre.
pub(crate) fn t1_min_factor(cx: &Ctx<'_>, t1: &Tree) -> f64 {
    let v = cx.v;
    let mut m = f64::INFINITY;
    for &f in &t1.settled {
        let c = f >> 1;
        let l = v.len_dm[c as usize];
        if l == 0 {
            continue;
        }
        let w = cx.w.chain(c);
        if w != INF {
            m = m.min(f64::from(w) / f64::from(l));
        }
    }
    if m.is_finite() {
        0.999 * m
    } else {
        0.0
    }
}

// --- Recherche point à point (lot 4, 4c, §15.5) -----------------------------------------------------------------

/// La cible d'une recherche P1 → P2 et ses repères.
pub(crate) struct P2pTarget {
    pub node: u32,
    /// Position du nœud cible, mètres locaux.
    pub xy: (f64, f64),
    /// Coût du meilleur état du nœud dans chaque arbre (0 si non atteint).
    pub t1: u32,
    pub t2: u32,
    /// Plus petit coût par décimètre (vol d'oiseau).
    pub f_min: f64,
    /// Marge retranchée aux deux bornes triangulaires : le coût d'un virage sépare deux états du même nœud
    /// (`TURN_COST_MAX + 500`, 2 500) ; 0 rejoue la sonde.
    pub slack: u32,
}

/// Marge de l'heuristique : `TURN_COST_MAX + 500`.
pub(crate) const P2P_SLACK: u32 = TURN_COST_MAX + 500;

impl P2pTarget {
    /// Heuristique au nœud `n` : `max(vol d'oiseau × f_min, T1(P2) − T1(n) − slack, T2(n) − T2(P2) − slack)`, les
    /// deux bornes triangulaires seulement avec `tri`.
    #[inline]
    pub(crate) fn h(&self, cx: &Ctx<'_>, t1: &Tree, t2: &Tree, n: u32, tri: bool) -> u32 {
        let (x, y) = cx.disc.xy_of(cx.v.pos[n as usize]);
        let crow_dm = (x - self.xy.0).hypot(y - self.xy.1) * 10.0;
        let mut h = (crow_dm * self.f_min).floor().min(f64::from(u32::MAX / 4)) as u32;
        if tri {
            if let Some(b) = t1.node_best(n) {
                h = h.max(self.t1.saturating_sub(t1.get(b)).saturating_sub(self.slack));
            }
            if let Some(b) = t2.node_best(n) {
                h = h.max(t2.get(b).saturating_sub(self.t2).saturating_sub(self.slack));
            }
        }
        h
    }
}

/// Fin d'une recherche point à point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum P2pEnd {
    Found,
    NotFound,
    /// Le plafond de la recherche (états fixés) est atteint : abandonnée.
    Capped,
    /// Le budget de la passe est à 0.
    BudgetOut,
}

#[derive(Debug)]
pub(crate) struct P2pOut {
    /// Les états du dernier état de la jambe 1 (exclu) jusqu'au nœud cible (état d'arrivée compris).
    pub fields: Option<Vec<u32>>,
    pub meters: u32,
    pub cost: u32,
    pub settled: u64,
    pub end: P2pEnd,
}

/// A* de P1 (atteint par l'état `last`, de prédécesseur `last_pred`) au **nœud** `tgt.node`, atteint par n'importe quel
/// état, sur les arêtes que `ban` n'interdit pas, dans le disque, avec la règle de virage. `cap_states` : plafond de la
/// recherche ; `budget` : états restants de la passe, décomptés.
#[allow(clippy::too_many_arguments)]
pub(crate) fn astar_p2p(
    cx: &Ctx<'_>,
    ta: &mut Tree,
    t1: &Tree,
    t2: &Tree,
    last: u32,
    last_pred: u32,
    tgt: &P2pTarget,
    ban: &Ban<'_>,
    lead: &Lead<'_>,
    tri: bool,
    cap_states: u64,
    budget: &mut u64,
    stop: &mut dyn FnMut() -> bool,
) -> Result<P2pOut, Stopped> {
    let v = cx.v;
    ta.begin(2 * v.n_chains, v.n_nodes);
    let mut out = P2pOut { fields: None, meters: 0, cost: 0, settled: 0, end: P2pEnd::NotFound };
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let expand = |ta: &mut Tree,
                  heap: &mut BinaryHeap<Reverse<(u32, u32, u32)>>,
                  node: u32,
                  f_prev: u32,
                  pred_prev: u32,
                  hin_over: Option<u16>,
                  store: u32,
                  g: u32,
                  m: u32| {
        if !v.complete[node as usize] {
            cx.esc.set(true);
            return;
        }
        for &fo in v.out(node) {
            let c = fo >> 1;
            let bits = ban.bits(c);
            if bits & BAN != 0 {
                continue;
            }
            let Some(tc) = turn_cost(v, cx.forb, f_prev, pred_prev, fo, hin_over, None) else { continue };
            if !retro_ok(v, pred_prev, f_prev, fo) {
                continue;
            }
            if let Some(o) = cx.org {
                if o.refuses_fwd(cx, ta, lead, f_prev, fo, m, g, store != NONE) {
                    continue;
                }
            }
            let mut w = cx.w.chain(c);
            if w == INF {
                continue;
            }
            let tn = v.tgt(fo);
            if !cx.disc.inside(v.pos[tn as usize]) {
                continue;
            }
            if bits & TAIL != 0 {
                w = w.saturating_mul(2);
            }
            w = ban.corridor(w, cx, tn);
            let ng = g.saturating_add(w).saturating_add(tc);
            if ng < ta.get(fo) {
                ta.set(fo, ng, store, m + v.len_dm[c as usize]);
                let h = tgt.h(cx, t1, t2, tn, tri);
                heap.push(Reverse((ng.saturating_add(h), ng, fo)));
            }
        }
    };
    let first_over = (last_pred == NONE).then(|| cx.s.out_head[(last & 1) as usize]);
    expand(ta, &mut heap, v.tgt(last), last, last_pred, first_over, NONE, 0, 0);
    let mut polls = 0u32;
    while let Some(Reverse((_, g, f))) = heap.pop() {
        if cx.esc.get() {
            return Err(Stopped);
        }
        if g > ta.get(f) {
            continue;
        }
        if out.settled >= cap_states {
            out.end = P2pEnd::Capped;
            return Ok(out);
        }
        if *budget == 0 {
            out.end = P2pEnd::BudgetOut;
            return Ok(out);
        }
        *budget -= 1;
        out.settled += 1;
        polls += 1;
        if polls.is_multiple_of(POLL) && stop() {
            return Err(Stopped);
        }
        if v.tgt(f) == tgt.node {
            let mut fields = vec![f];
            let mut cur = f;
            while ta.pred(cur) != NONE {
                cur = ta.pred(cur);
                fields.push(cur);
            }
            fields.reverse();
            out.fields = Some(fields);
            out.meters = ta.meters(f);
            out.cost = g;
            out.end = P2pEnd::Found;
            return Ok(out);
        }
        let node = v.tgt(f);
        let (pred, m) = (ta.pred(f), ta.meters(f));
        expand(ta, &mut heap, node, f, pred, None, f, g, m);
    }
    if cx.esc.get() {
        return Err(Stopped);
    }
    Ok(out)
}

#[cfg(test)]
mod cor_tests {
    use super::CorGrid;

    /// Une trame réutilisée rend ce que rend une trame neuve, même quand la case (bornée à 500 m) et le côté ne changent
    /// pas d'une requête à l'autre et que le rayon de la dilatation change (D = 66,6 km puis 66,7 km, w = 0,03 D : 142
    /// cases de côté, rayon 4 puis 5).
    #[test]
    fn a_reused_grid_dilates_with_the_radius_of_its_request() {
        let marks = |g: &mut CorGrid| {
            g.mark_point((0.0, 0.0));
            (0..80).map(|k| g.hit((f64::from(k) * 50.0, 0.0))).collect::<Vec<bool>>()
        };
        let mut fresh = CorGrid::default();
        fresh.begin(66_700.0, 2_001.0);
        let want = marks(&mut fresh);
        let mut reused = CorGrid::default();
        reused.begin(66_600.0, 1_998.0);
        let first = marks(&mut reused);
        assert_eq!((reused.side, reused.cell), (fresh.side, fresh.cell), "même trame");
        reused.begin(66_700.0, 2_001.0);
        assert_eq!(marks(&mut reused), want);
        assert_ne!(first, want, "les deux rayons diffèrent : le test ne prouve rien");
    }
}
