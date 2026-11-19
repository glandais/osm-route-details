//! Le moteur : le graphe de base et ses deux vues (`road` corrigée, `gravel`) **lus dans les fichiers** (`mmap`), plus
//! les index globaux de chaque vue (`index.rs`). La recherche ne lit pas les fichiers : elle lit une [`Window`]
//! (`window.rs`), les cellules d'une zone relues à plat et renumérotées (note du lot 6).
//!
//! Avant le lot 6, le moteur recopiait tout le graphe à plat (2,9 Go sur la France) ; il n'en garde plus que les index
//! (quelques Mo) et lit le graphe de base dans le fichier projeté, aux mêmes octets.

use std::fmt;

use ord_graph::attract::{AttrError, AttrFile};
use ord_graph::attrs::Attrs;
use ord_graph::format::{GraphFile, DIR_BIT};
use ord_graph::view::ViewKind;

use crate::cost::{self, RideType, Taste};
use crate::geo::polyline_m;
use crate::index::ViewIndex;

/// Constantes Java reprises telles quelles (note §6) et réglages du moteur.
#[derive(Clone, Debug)]
pub struct Params {
    /// Virage refusé à partir de cet angle, aux nœuds de degré ≥ 3.
    pub max_turn_deg: f64,
    /// Queue maximale (part de D) et part recouverte maximale.
    pub tail_share: f64,
    pub max_crossings: u32,
    pub tolerance: f64,
    /// Accroche : distance maximale du point à l'arête.
    pub snap_m: f64,
    /// Trou comblé en strict : longueur maximale (m).
    pub hole_m: f64,
    /// Jonction courte comblée en strict : chemin non roulé de cette longueur au plus entre deux nœuds d'équipe (m).
    pub connector_m: f64,
    /// Rapport retour / aller initial des demi-tours.
    pub rho0: f64,
    /// Budget de recherches ciblées, en multiple des états fixés de l'arbre aller.
    pub astar_budget: u32,
    /// Queue permise au retour : part de D de l'aller que le retour peut reprendre (au coût × 2).
    pub tail_reuse_share: f64,
    /// Composante minimale d'accroche : part de D.
    pub min_component_share: f64,
    /// Points de demi-tour essayés au plus par secteur de 30° (le ρ du candidat corrige la cible à chaque essai).
    pub tries_per_sector: usize,
    /// Plus grande distance D acceptée (m) : au-delà, la composante minimale d'accroche (`0,9 D`) ne se lit plus dans
    /// l'index creux des composantes et le moteur refuse la demande (lot 6, §6).
    pub max_distance_m: f64,
    /// Marge d'arrondi (m) ajoutée aux zones d'une fenêtre (lot 6, §5.3).
    pub window_margin_m: f64,
    /// Fils du calcul des index à l'ouverture (une vue par fil, au plus).
    pub index_threads: usize,
    /// Goût de base (lot 4, 3b) : le poids de **toute** arête de la vue, aux deux curseurs, avec ou sans équipe. Défaut :
    /// la table commune figée ([`Taste::common`]). [`Taste::neutral`] redonne exactement les poids de 1.9.2
    /// (`cost::edge_weight`). Un goût passé au crochet d'une `WindowTeam` **remplace** ce poids, il ne s'y multiplie pas.
    pub base_taste: Taste,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            max_turn_deg: 150.0,
            tail_share: 0.15,
            max_crossings: 2,
            tolerance: 0.10,
            snap_m: 2000.0,
            hole_m: 100.0,
            connector_m: 30.0,
            rho0: 1.09,
            astar_budget: 6,
            tail_reuse_share: 0.15,
            min_component_share: 0.9,
            tries_per_sector: 6,
            max_distance_m: 400_000.0,
            window_margin_m: 1_000.0,
            index_threads: 2,
            base_taste: Taste::common(),
        }
    }
}

#[derive(Debug)]
pub enum EngineError {
    /// Le fichier n'est pas une vue, ou pas du bon genre.
    BadView(String),
    /// La vue ne vient pas de ce graphe de base.
    ViewMismatch,
    /// La surcouche vient d'un autre graphe.
    OverlayMismatch,
    /// La construction creuse des données d'équipe a quitté les cases chaudes, même élargies à la boîte de la zone
    /// (lot 6, finition 2, §20.2) : jamais un résultat faux, une erreur.
    TeamZoneEscape,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadView(s) => write!(f, "vue inutilisable : {s}"),
            Self::ViewMismatch => write!(f, "la vue ne correspond pas à ce graphe de base"),
            Self::OverlayMismatch => write!(f, "la surcouche vient d'un autre graphe"),
            Self::TeamZoneEscape => write!(f, "la construction de l'équipe a quitté son voisinage"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Une vue relue à plat : celle d'une [`Window`], numérotée localement. Les ids locaux de chaînes et de nœuds
/// suivent l'ordre des ids globaux (`gid`, `ngid`) : tous les départages du moteur sont ceux du moteur à plat.
#[derive(Debug)]
pub(crate) struct ViewData {
    pub n_nodes: usize,
    pub n_chains: usize,
    pub pos: Vec<(i32, i32)>,
    pub ends: Vec<(u32, u32)>,
    pub len_dm: Vec<u32>,
    /// Bit 0 : sens `a → b` ouvert ; bit 1 : sens `b → a`.
    pub open: Vec<u8>,
    pub out_off: Vec<u32>,
    pub out_f: Vec<u32>,
    pub in_off: Vec<u32>,
    pub in_f: Vec<u32>,
    pub chain_off: Vec<u32>,
    /// Arêtes de base de chaque chaîne, **ids globaux** (bit 31 : parcourue de `b` vers `a`).
    pub chain_links: Vec<u32>,
    pub head: Vec<[u16; 2]>,
    pub deg: Vec<u8>,
    /// Poids sans équipe par type servi par la vue, indexé par `RideType::index` (vide si non servi).
    pub w_osm: [Vec<u32>; 3],
    /// Id global de chaque chaîne et de chaque nœud de vue (croissants).
    pub gid: Vec<u32>,
    pub ngid: Vec<u32>,
    /// Un nœud est complet quand sa tuile est dans la fenêtre : toutes ses chaînes sont chargées (note §5.2).
    pub complete: Vec<bool>,
    /// Masque d'attracteurs de chaque nœud local (lot 4, 4d) : **vide** si la vue n'a pas de fichier, sinon `n_nodes` octets.
    /// Un nœud a le même masque dans toute fenêtre qui le contient.
    pub attr: Vec<u8>,
}

impl ViewData {
    #[inline]
    pub fn tgt(&self, f: u32) -> u32 {
        let e = self.ends[(f >> 1) as usize];
        if f & 1 == 0 {
            e.1
        } else {
            e.0
        }
    }

    #[inline]
    pub fn src(&self, f: u32) -> u32 {
        let e = self.ends[(f >> 1) as usize];
        if f & 1 == 0 {
            e.0
        } else {
            e.1
        }
    }

    #[inline]
    pub fn out(&self, v: u32) -> &[u32] {
        &self.out_f[self.out_off[v as usize] as usize..self.out_off[v as usize + 1] as usize]
    }

    #[inline]
    pub fn inn(&self, v: u32) -> &[u32] {
        &self.in_f[self.in_off[v as usize] as usize..self.in_off[v as usize + 1] as usize]
    }

    /// Arêtes de base de la chaîne, de `a` vers `b` ; bit 31 : parcourue de `b` vers `a`.
    #[inline]
    pub fn links(&self, c: u32) -> &[u32] {
        &self.chain_links[self.chain_off[c as usize] as usize..self.chain_off[c as usize + 1] as usize]
    }

    /// Liens d'un sens de parcours : `(arête de base, parcourue à l'envers)` dans l'ordre du parcours.
    pub fn oriented_links(&self, f: u32) -> Vec<(u32, bool)> {
        let l = self.links(f >> 1);
        if f & 1 == 0 {
            l.iter().map(|&x| (x & !DIR_BIT, x & DIR_BIT != 0)).collect()
        } else {
            l.iter().rev().map(|&x| (x & !DIR_BIT, x & DIR_BIT == 0)).collect()
        }
    }

    /// Octets tenus (capacités réelles des tableaux).
    pub fn bytes(&self) -> usize {
        use std::mem::size_of;
        self.pos.capacity() * size_of::<(i32, i32)>()
            + self.ends.capacity() * size_of::<(u32, u32)>()
            + (self.len_dm.capacity()
                + self.out_off.capacity()
                + self.out_f.capacity()
                + self.in_off.capacity()
                + self.in_f.capacity()
                + self.chain_off.capacity()
                + self.chain_links.capacity()
                + self.gid.capacity()
                + self.ngid.capacity()
                + self.w_osm.iter().map(Vec::capacity).sum::<usize>())
                * 4
            + self.open.capacity()
            + self.head.capacity() * 4
            + self.deg.capacity()
            + self.complete.capacity()
            + self.attr.capacity()
    }
}

/// Composantes fortement connexes (Tarjan itératif) d'un graphe de `n` nœuds dont `succ(x, buf)` **ajoute** les
/// successeurs de `x` à `buf` ; rend l'indice de composante de chaque nœud. Sens respectés, virages ignorés : une
/// accroche dans une poche orientée (rue à sens unique roulée une fois, qui ne mène nulle part) n'est pas dans la
/// grande composante, alors qu'elle y est sens ignorés (campagne du lot 2, `ord-loops-lot2-mesures.md`).
///
/// Les successeurs des nœuds de la pile d'appels vivent bout à bout dans **un seul** tampon : pas d'allocation par
/// nœud (lot 6, sur 10 M de nœuds).
pub(crate) fn strong_components(n: usize, succ: &dyn Fn(u32, &mut Vec<u32>)) -> Vec<u32> {
    const UNSEEN: u32 = u32::MAX;
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![UNSEEN; n];
    let mut stack: Vec<u32> = Vec::new();
    // pile d'appels : (nœud, début de ses successeurs dans `buf`, position courante)
    let mut call: Vec<(u32, usize, usize)> = Vec::new();
    let mut buf: Vec<u32> = Vec::new();
    let (mut next_index, mut next_comp) = (0u32, 0u32);
    for root in 0..n as u32 {
        if index[root as usize] != UNSEEN {
            continue;
        }
        buf.clear();
        succ(root, &mut buf);
        index[root as usize] = next_index;
        low[root as usize] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root as usize] = true;
        call.push((root, 0, 0));
        while let Some(top) = call.last_mut() {
            let x = top.0;
            if top.2 < buf.len() {
                let y = buf[top.2];
                top.2 += 1;
                if index[y as usize] == UNSEEN {
                    let start = buf.len();
                    succ(y, &mut buf);
                    index[y as usize] = next_index;
                    low[y as usize] = next_index;
                    next_index += 1;
                    stack.push(y);
                    on_stack[y as usize] = true;
                    call.push((y, start, start));
                } else if on_stack[y as usize] {
                    low[x as usize] = low[x as usize].min(index[y as usize]);
                }
                continue;
            }
            let (_, start, _) = *top;
            call.pop();
            buf.truncate(start);
            if let Some(parent) = call.last() {
                let p = parent.0 as usize;
                low[p] = low[p].min(low[x as usize]);
            }
            if low[x as usize] == index[x as usize] {
                loop {
                    let y = stack.pop().expect("pile de Tarjan non vide");
                    on_stack[y as usize] = false;
                    comp[y as usize] = next_comp;
                    if y == x {
                        break;
                    }
                }
                next_comp += 1;
            }
        }
    }
    comp
}

/// Longueur (dm) de chaque composante fortement connexe : les chaînes gardées par `keep` dont les deux bouts y sont.
pub(crate) fn strong_lengths(
    comp: &[u32],
    ends: &[(u32, u32)],
    len_dm: &[u32],
    keep: &dyn Fn(usize) -> bool,
) -> Vec<u64> {
    let mut out = vec![0u64; comp.len()];
    for (c, &(a, b)) in ends.iter().enumerate() {
        if keep(c) && comp[a as usize] == comp[b as usize] {
            out[comp[a as usize] as usize] += u64::from(len_dm[c]);
        }
    }
    out
}

/// Longueur (dm) de la composante d'une chaîne, 0 si ses deux bouts ne sont pas dans la même.
pub(crate) fn chain_strong_len(comp: &[u32], len: &[u64], ends: (u32, u32)) -> u64 {
    let (ca, cb) = (comp[ends.0 as usize], comp[ends.1 as usize]);
    if ca == cb {
        len[ca as usize]
    } else {
        0
    }
}

/// Résultat d'une accroche : arête de base la plus proche, à `offset_m` de son extrémité `a`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Snap {
    pub chain: u32,
    pub link: usize,
    pub edge: u32,
    pub rev: bool,
    pub offset_m: f64,
    pub dist_m: f64,
}

/// Le moteur : immuable, partageable entre fils (un `Workspace` par fil).
#[derive(Debug)]
pub struct Engine<B> {
    pub(crate) base: GraphFile<B>,
    pub(crate) views_files: [GraphFile<B>; 2],
    pub(crate) kinds: [ViewKind; 2],
    pub(crate) idx: [ViewIndex; 2],
    pub(crate) params: Params,
    /// Fichiers d'attracteurs des deux vues (lot 4, 4d), s'ils sont posés.
    pub(crate) attr: [Option<AttrFile<B>>; 2],
}

impl<B: AsRef<[u8]> + Sync> Engine<B> {
    /// Ouvre le graphe de base et ses deux vues non élaguées : `road` (corrigée, lot 2) et `gravel`. Contrôle leur
    /// identité, puis calcule les index globaux de chaque vue (§4.1 du lot 6) : `index_threads` fils au plus.
    pub fn new(
        base: GraphFile<B>,
        road: GraphFile<B>,
        gravel: GraphFile<B>,
        params: Params,
    ) -> Result<Self, EngineError> {
        if base.is_view() {
            return Err(EngineError::BadView("le graphe de base est une vue".into()));
        }
        let kinds = [ViewKind::Road, ViewKind::Gravel];
        for (view, kind) in [(&road, kinds[0]), (&gravel, kinds[1])] {
            check_view(&base, view, kind)?;
        }
        // Index globaux : une vue par fil quand `index_threads` le permet.
        let per_view = (params.index_threads / 2).max(1);
        let idx = if params.index_threads >= 2 {
            std::thread::scope(|s| {
                let h = s.spawn(|| ViewIndex::build(&road, &params, per_view));
                let g = ViewIndex::build(&gravel, &params, per_view);
                [h.join().expect("fil d'index"), g]
            })
        } else {
            [ViewIndex::build(&road, &params, 1), ViewIndex::build(&gravel, &params, 1)]
        };
        Ok(Self { base, views_files: [road, gravel], kinds, idx, params, attr: [None, None] })
    }
}

fn check_view<B: AsRef<[u8]>>(
    base: &GraphFile<B>,
    view: &GraphFile<B>,
    kind: ViewKind,
) -> Result<(), EngineError> {
    let meta = view.view_meta().ok_or_else(|| EngineError::BadView("pas une vue".into()))?;
    let h = base.header();
    if (meta.base_nodes, meta.base_edges, meta.base_pbf_xxh3, meta.base_build_epoch)
        != (h.nodes, h.edges, h.pbf_xxh3, h.build_epoch)
    {
        return Err(EngineError::ViewMismatch);
    }
    if meta.kind != kind || meta.prune_dead_ends {
        return Err(EngineError::BadView(format!(
            "attendu une vue `{}` non élaguée, reçu `{}`",
            kind.name(),
            meta.name
        )));
    }
    if view.header().zoom != h.zoom {
        return Err(EngineError::BadView("la vue n'a pas le zoom de la base".into()));
    }
    let n = view.header().edges;
    if n > 0 && (view.view_chain(0).is_none() || view.view_base_nodes().is_none()) {
        return Err(EngineError::BadView("chaînes absentes".into()));
    }
    Ok(())
}

impl<B: AsRef<[u8]>> Engine<B> {
    /// Range le fichier d'attracteurs de la vue `vi` (0 `road`, 1 `gravel`) après l'avoir confronté au graphe de base et à la
    /// vue (`AttrFile::check_for`) ; en cas d'erreur rien n'est rangé. À poser avant de construire des fenêtres : celles qui
    /// existent déjà n'ont pas de masque (les attracteurs y sont inactifs).
    pub fn set_attractors(&mut self, vi: usize, f: AttrFile<B>) -> Result<(), AttrError> {
        f.check_for(self.base.header(), &self.views_files[vi])?;
        self.attr[vi] = Some(f);
        Ok(())
    }

    /// Le fichier va avec ce graphe de base et la vue `vi` (`AttrFile::check_for`), sans le ranger : le service contrôle les deux
    /// fichiers avant d'en poser un.
    pub fn check_attractors(&self, vi: usize, f: &AttrFile<B>) -> Result<(), AttrError> {
        f.check_for(self.base.header(), &self.views_files[vi])
    }

    /// La vue `vi` a un fichier d'attracteurs.
    pub fn has_attractors(&self, vi: usize) -> bool {
        self.attr[vi].is_some()
    }

    /// Poids d'une arête de base pour un type : `⌈len × K × q8 / (64 × 256)⌉` avec la table du goût de base
    /// (`Params::base_taste`). Avec [`Taste::neutral`], exactement [`cost::edge_weight`].
    #[inline]
    pub(crate) fn base_weight(&self, ride: RideType, a: Attrs, len_dm: u32) -> u32 {
        cost::taste_weight(ride, a, len_dm, &self.params.base_taste.q8[ride.index()])
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn base(&self) -> &GraphFile<B> {
        &self.base
    }

    /// Durées de construction des index (ms) : `[(chaînes étrangères, composantes)]` par vue.
    pub fn index_timings_ms(&self) -> [(u64, u64); 2] {
        [(self.idx[0].foreign_ms, self.idx[0].scc_ms), (self.idx[1].foreign_ms, self.idx[1].scc_ms)]
    }

    /// Octets tenus par les index globaux des deux vues.
    pub fn index_bytes(&self) -> usize {
        self.idx[0].bytes() + self.idx[1].bytes()
    }

    // --- Le graphe de base, lu dans le fichier ---------------------------------------------------------------------

    pub(crate) fn n_edges(&self) -> u32 {
        self.base.header().edges as u32
    }

    #[inline]
    pub(crate) fn ends(&self, e: u32) -> (u32, u32) {
        self.base.edge_ends(e)
    }

    #[inline]
    pub(crate) fn pos(&self, n: u32) -> (i32, i32) {
        self.base.node_pos(n)
    }

    #[inline]
    pub(crate) fn way(&self, e: u32) -> u64 {
        self.base.edge_way(e)
    }

    #[inline]
    pub(crate) fn attrs(&self, e: u32) -> Attrs {
        self.base.edge(e).attrs
    }

    #[inline]
    pub(crate) fn len_dm(&self, e: u32) -> u32 {
        self.base.edge(e).len_dm
    }

    /// Attributs et longueur (dm) en une lecture.
    #[inline]
    pub(crate) fn attrs_len(&self, e: u32) -> (Attrs, u32) {
        let v = self.base.edge(e);
        (v.attrs, v.len_dm)
    }

    /// Longueur de la géométrie d'une arête de base, en mètres (haversine).
    pub(crate) fn geom_len_m(&self, e: u32) -> f64 {
        polyline_m(&self.edge_polyline(e))
    }

    /// Polyligne complète d'une arête de base, de `a` vers `b`.
    pub(crate) fn edge_polyline(&self, e: u32) -> Vec<(i32, i32)> {
        let (a, b) = self.base.edge_ends(e);
        let mut pts = vec![self.base.node_pos(a)];
        pts.extend(self.base.geometry(e));
        pts.push(self.base.node_pos(b));
        pts
    }

    /// Vues servies à un type : l'indice de la vue.
    pub(crate) fn kind_of(&self, vi: usize) -> ViewKind {
        self.kinds[vi]
    }
}

/// Types servis par la vue d'indice `vi`.
pub(crate) fn rides_of_view(vi: usize) -> &'static [RideType] {
    if vi == 0 {
        &[RideType::Road]
    } else {
        &[RideType::Gravel, RideType::Mtb]
    }
}
