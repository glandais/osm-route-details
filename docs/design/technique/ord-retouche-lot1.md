# Retouche d'un parcours — lot 7a : la retouche dans le moteur, conception

> Statut : **conçu le 24 novembre 2026, rien de codé, rien de mesuré.** Depuis `develop` à `c99a02b` (1.17.1). Plans
> appelants (Pédalons) : `docs/plans/2026-11-22-retouche-parcours.md` (ledger `API-212`, §3 à §6, décisions D1 à D5 du §9,
> critères du §10, résultats du lot 0 au §10 bis) et `docs/plans/2026-11-23-itineraire-velo-osm.md` (ledger `API-217`, §8 :
> ce que l'itinéraire de A à B change). Ce lot est le « 7a » : **le moteur, ses tests et le banc**. Pas de service, pas
> d'endpoint, pas de contrat ; le service est le 7b.
>
> S'appuie sur [`ord-retouche-lot0.md`](ord-retouche-lot0.md) (le prototype, sous `probe`),
> [`ord-retouche-lot0-mesures.md`](ord-retouche-lot0-mesures.md) (le relevé, causes d'échec chiffrées) et
> [`ord-route-lot1.md`](ord-route-lot1.md) (l'itinéraire de A à B).
>
> Marques : **[lu]** relu dans le code de `c99a02b` pour cette note ; **[mesuré]** renvoie au relevé du lot 0, cité ;
> **[supposé]** est une explication non vérifiée ; **[proposé]** est un réglage que la campagne du §11 confirme ou change.
> Ni départ, ni coordonnée, ni identifiant d'équipe ou de trace ici ; équipes lettrées comme aux relevés précédents.

## 0. En bref

- **Trois opérations publiques** dans `ord-loops`, au-dessus d'une entrée commune, le **parcours posé**
  ([`PosedRoute`](#2-lentrée--le-parcours-posé)) : `move_end_in` (déplacer l'arrivée), `resize_in` (viser une distance :
  raccourcir **par arbre d'itinéraire**, allonger **par la lentille**). Chacune suit le patron de `route_in` : un plan de
  fenêtre (`Engine::plan_move_end`, `Engine::plan_resize`), un appel dans une fenêtre, une variante qui bâtit ses fenêtres
  pour les tests et les outils.
- **La cause des 89 % de refus de `move_end` est la règle des grappes du contrôle, pas le virage simple** [lu, et mesuré :
  93 % des refusés ont un angle de couture sous 120°]. Le contrôle interne du prototype juge le raccord **seul**, et la
  règle des grappes y est levée sur ses 200 premiers mètres (`CLUSTER_FREE_ENDS_M`) : précisément là où est la couture.
  Le moteur a déjà ce qu'il faut pour voir la jambe gardée : le modèle de virages de l'organique (`TurnModel`, `Lead`,
  `OrgSearch::junction_ok`, `turns.rs`), qui rejoue la règle des grappes sur un chemin **posé après des jambes fixées**.
  Le cap de la jambe gardée est donné de trois façons, toutes sans nouveau calcul de géométrie : l'**état d'arrivée** de la
  jambe gardée et son prédécesseur pour le virage simple (`turn_cost`, `retro_ok`), la **suite des états gardés** comme
  `Lead` pour les grappes, et, pour une recherche qui **part** de la couture, une **accroche de couture** dont le cap
  d'entrée est celui de la jambe gardée et dont le demi-tour est fermé (§4).
- **Raccourcir = un arbre aller depuis chaque couture `i` candidate**, sur tout le parcours masqué : pour chaque couture
  `j` en aval, l'économie `ℓ(i, j) − m(i, j)` se lit dans l'arbre, et le chemin de l'arbre **est** l'itinéraire de `i` à
  `j` aux poids du générateur, avec la règle de virage du générateur. Plus de lentille, plus de borne à 1,05 × le plus court
  chemin, plus de fenêtre tirée au vol d'oiseau (§6).
- **Allonger reste la lentille** de production, appelée par un paramètre interne de `collect` (masque et accroches de
  couture), `None` pour toute génération ordinaire ; le temps est tenu par un **arrêt anticipé** et une **échéance** (§7).
- **L'interdiction durable sort de `probe`** : `KeepOut` devient public (`graft.rs`), `Weighter` porte son masque dans
  toutes les compilations, et le masque passe **par paramètre**, plus par l'état du `Workspace`. Une génération et un
  itinéraire ordinaires ne le posent jamais : mêmes résultats au bit près (§5).
- **Le recollé est contrôlé dans le moteur** par `verify::check_spliced_in`, le contrôle interne sur 400 m de contexte gardé
  de part et d'autre, par différence avec le contexte seul (§8). Le contrôleur indépendant reste au banc (C1 à C3 du lot 0).
- **Le banc** `retouche-play` joue les mêmes tirages que le lot 0 avec l'API du moteur (`--impl engine`, défaut) ou avec
  les prototypes (`--impl lot0`), pour comparer à tirage égal ; sorties sous `~/.cache/osm-route-details/graph/retouche-bench/`
  (§11).

---

## 1. Ce que le code fait aujourd'hui (relu)

### 1.1 Le prototype `move_end_in` et sa cause d'échec

- `probe_graft::move_end_in` bâtit un arbre retour depuis `E′` sur le parcours masqué, puis, pour chaque couture `k`, retient
  la sortie `fo` de plus petit `t2.get(fo)` telle que `turn_cost(v, forb, f_in, NONE, fo, None, None)` soit `Some` [lu].
  Avec `pred = NONE`, le cap d'arrivée est celui de la seule chaîne `f_in` (`in_head_ext` ne le prolonge pas quand la chaîne
  fait moins de 50 m), et `retro_ok` n'est pas appelé [lu].
- Le contrôle interne est ensuite `check_route_in` sur **le raccord seul**, de la couture à `E′` [lu]. Ce contrôle lève la
  règle des grappes quand la grappe tient dans les 200 premiers mètres (`verify.rs`, `CLUSTER_FREE_ENDS_M`) [lu] ; le
  contrôleur indépendant aussi (`cluster_free_ends_m`, `ord-loops-check/src/check.rs`) [lu]. C2 du banc, lui, juge le
  morceau continu **qui contient la couture** : la couture y est au milieu, la levée ne joue plus, et une grappe « fin de la
  jambe gardée → début du raccord » à moins de 100 m devient un `SHARP_TURN` [lu].
- Lecture : c'est le mécanisme qui produit « `SHARP_TURN` de C2 seul, angle de couture sous 120° » [mesuré, 89 % des refus,
  93 % sous 120°]. Que ce soit **toute** la cause n'est pas démontré [supposé] ; le test 8 du §10 le reproduit sur une
  grille, la campagne du §11 le chiffre.

### 1.2 Ce que le moteur sait déjà faire de la jambe gardée

- **`turns.rs`** [lu] : `TurnModel` refait le contrôle des virages (virage simple et grappes de 100 m, bouts libres de
  200 m) **sur des états de chaîne**, à la relaxation ; `Lead { path, dm, aux }` décrit les états **déjà posés** avant le
  départ d'une recherche (ou après l'arrivée d'une recherche à rebours) et les mètres du parcours jusque-là ;
  `OrgSearch::junction_ok(cx, ctx, base_dm, ret, aux)` rejoue le modèle sur les 200 premiers mètres d'un chemin d'arbre posé
  après des jambes fixées. C'est le contrôle de couture dont la retouche a besoin, écrit pour les jonctions de la
  génération organique.
- **`astar` et `astar_p2p`** partent déjà « du nœud atteint par l'état `last`, de prédécesseur `last_pred` » avec un `Lead`
  [lu] ; les deux arbres, eux, partent d'une accroche (`Anchor`) et lisent `Lead::NONE` [lu].
- **`Weighter::chain`** est la porte unique des arbres, des A\* et de l'accroche ; le masque `keep_out` y est lu sous `probe`
  seulement, et le `Workspace` le passe à `collect` qui le prend (`take`) [lu].
- **`route_in`** [lu] : A\* d'accroche à accroche, heuristique vol d'oiseau, virages **relâchés** (le virage serré coûte au
  lieu d'être refusé depuis 1.17.1). La retouche ne peut pas prendre ces virages tels quels : le contrôleur refuse
  `SHARP_TURN` partout, et C1 le compterait. Elle reprend de l'itinéraire son **principe** (le chemin de moindre coût aux
  poids du générateur, sans lentille), sa mesure (`route::measure`) et son rendu (`Route`), mais pas son A\* : un seul arbre
  depuis `i` sert toutes les coutures `j` (§6), là où un A\* par paire coûterait autant d'appels.

### 1.3 L'entrée du lot 0, dans le banc

`retouche::posed::Posed` (marche d'`ord-graph-match`, polyligne du document, coutures valides des règles 1 à 4, projection
monotone de `project.rs`, compteur d'anomalies d'ordre) vit dans le banc [lu]. `ord-graph-match` dépend d'`ord-loops` [lu,
`Cargo.toml`] : le moteur ne peut pas prendre un `Walk`. Il prend donc une entrée à lui, que le banc (et au 7b le service)
remplit depuis la marche.

---

## 2. L'entrée : le parcours posé

### 2.1 Types (`crates/ord-loops/src/posed.rs`, nouveau)

```rust
/// Une arête de base parcourue sur `[lo, hi]` ⊂ `[0, 1]` de sa longueur (paramètre a → b), dans le sens b → a si `reverse`.
pub struct PosedLeg { pub edge: u32, pub reverse: bool, pub lo: f64, pub hi: f64 }

/// Jonction entre deux jambes consécutives (celle d'`ord-graph-match`, recopiée : pas de dépendance inverse).
pub enum PosedJoin { Node(u32), Touch, Hole { chord_m: f64 } }

/// Ce que l'appelant donne : la marche et la polyligne d'origine (celle du document, chaque premier point répété retiré).
pub struct PosedInput<'a> {
    pub ride_type: RideType,
    pub legs: &'a [PosedLeg],
    pub joins: &'a [PosedJoin],      // legs.len() - 1
    pub line: &'a [LatLon],
}

pub struct PoseTune {
    pub seam_margin_m: f64,          // 100 (lot 0 : 200 → 100, K1 +12 points, rien perdu ailleurs [mesuré])
    pub proj_tol_m: f64,             // 15
    pub touch_continues: bool,       // false : `Touch` coupe une plage (le recollé y serait discontinu)
    pub max_drift: f64,              // 0,02
}

pub struct PosedSeam {
    pub join: usize,                 // entre legs[join] et legs[join + 1]
    pub node: u32,                   // nœud de base
    pub s_m: f64,                    // abscisse sur la polyligne d'origine
    pub a_m: f64,                    // abscisse de marche (graphe)
    pub before_m: f64, pub after_m: f64, // marge continue de chaque côté
}

pub struct PosedRoute { /* champs privés : ride_type, legs, joins, line (e7), s cumulée, l_m, cum_leg_end_m,
                           seams, runs, stats */ }

pub enum PoseError {
    Empty, NotOnGraph, WrongType,
    /// Des coutures projetées hors de l'ordre de la marche : 5,0 % des traces [mesuré] ; refusées à l'entrée.
    OrderAnomaly { seams: u32 },
    /// Jambes en double ou manquantes : `|Σ leg_m + Σ trous − L| / L` au-delà de `max_drift`.
    Drift,
}

pub struct PoseStats {
    pub runs: u32, pub holes: u32, pub touches: u32,
    /// Jonctions `Node` refusées comme couture, par règle (lot 0, §2.3) : 1 nœud hors vue, 3 demi-tour ou virage ≥ 150°,
    /// 4 marge ; la règle 2 (arêtes dans la vue) se juge dans la fenêtre (§3.1) et a son compteur dans les diagnostics.
    pub refused_by_rule: [u32; 3],
}

impl PosedRoute {
    pub fn pose<B: AsRef<[u8]>>(e: &Engine<B>, input: &PosedInput<'_>, tune: &PoseTune) -> Result<Self, PoseError>;
    pub fn length_m(&self) -> f64;
    pub fn seams(&self) -> &[PosedSeam];
    pub fn stats(&self) -> &PoseStats;
    /// Géométrie d'origine entre deux abscisses, au point près (découpe interpolée aux bouts).
    pub fn line_between(&self, s0: f64, s1: f64) -> Vec<LatLon>;
}
```

### 2.2 Ce que fait `pose`

C'est le portage, sans changement de règle, de `posed.rs` et `project.rs` du banc [lu] :

1. **Structure** : jambes non vides, arêtes connues du graphe de base (`NotOnGraph`), type ROAD, GRAVEL ou MTB (`WrongType`) ;
   longueur graphe de chaque jambe `(hi − lo) × len_dm / 10`.
2. **Plages** : suites maximales de jambes jointes par `Node` (`Touch` et `Hole` coupent, sauf `touch_continues`).
3. **Coutures candidates** : règle 1 (`view_node_of_base(n)` dans la vue du type, lu sur `Engine::views_files` sans fenêtre),
   règle 3 (arêtes distinctes, virage de 150° au plus sur les géométries d'arête), règle 4 (marge `seam_margin_m` de chaque
   côté ; l'opération choisit ensuite si elle exige les deux côtés ou seulement l'avant).
4. **Projection monotone** sur la polyligne d'origine (lot 0, §2.4, complément du codage compris : fenêtre bornée par
   `3 ×` les cordes des trous) : `s_m` de chaque couture, anomalies d'ordre, dérive. **Une anomalie suffit à refuser**
   (`OrderAnomaly`) : couper la géométrie d'origine à une couture mal placée rendrait un recollé faux, et le relevé n'a pas
   trouvé de correction simple (les traces concernées ont deux fois plus de trous [mesuré]).

Le portage est jugé au banc (§11.3) : `retouche-continuity` sur la pose du moteur doit redonner les agrégats de continuité
du lot 0.

---

## 3. Les coutures dans une fenêtre

### 3.1 La table des coutures (`graft.rs`)

Une opération se joue dans une fenêtre. Une seule passe sur les liens de ses chaînes (comme `move_end_in` [lu]) donne, pour
chaque arête de base de la marche, la chaîne locale qui la porte. Chaque couture valide devient :

```rust
pub(crate) struct SeamState {
    pub seam: usize,          // indice dans PosedRoute::seams
    pub node: u32,            // nœud local (complet dans la fenêtre, sinon la couture est écartée)
    pub f_in: u32,            // état orienté de la chaîne gardée qui arrive au nœud (dernier lien = la jambe d'arrivée)
    pub pred_in: u32,         // état gardé qui précède f_in, ou NONE
    pub f_out: u32,           // état de la chaîne gardée qui repart (NONE si l'opération ne garde pas la suite)
    pub succ_out: u32,        // état gardé qui suit f_out, ou NONE
    pub lead_before: Vec<u32>,// états gardés jusqu'à 200 m avant le nœud (au plus 48, `MAX_STATES`), dans l'ordre
    pub lead_after: Vec<u32>, // états gardés jusqu'à 200 m après le nœud
    pub dm_from_start: u32,   // mètres de marche (dm) du départ au nœud, saturés
}
```

Une couture est écartée, et comptée par cause dans les diagnostics, si son nœud n'est pas un nœud complet de la fenêtre, si
`f_in` (ou `f_out` quand il le faut) n'existe pas avec la bonne orientation (**règle 2** du lot 0 : arête hors vue), ou si
la chaîne de `f_in` n'est pas entière dans la marche sur ses 50 derniers mètres (le cap serait faux). Un `lead` s'arrête au
premier `Touch`, au premier trou ou à la première arête hors fenêtre ; un `lead` de moins de 200 m est compté
(`short_lead`), il peut laisser passer une grappe.

### 3.2 Le masque

**Toutes les chaînes qui portent une arête de la marche**, sans zone libre autour des coutures (`KeepOut::build(win,
legs, [])`). Le lot 0 libérait la zone des coutures pour qu'une recherche puisse partir d'une accroche posée au nœud [lu,
§12 de la note du lot 0] ; ici les recherches partent d'un **état** (§4), elles n'ont plus besoin de traverser une chaîne
gardée, et la zone libre ne servait plus qu'à reprendre le parcours : c'est elle qui donnait les `U_TURN` et
`REUSE_OUTSIDE_TAIL` de C2 sur `resize` [mesuré : 9 184 et 5 275 codes de refus]. La partie remplacée `[i, j]` est
masquée elle aussi pour `move_end`. **Pour `resize`, la partie remplacée est libre par défaut** (`RetouchTune::free_section` et
`free_shorten`, § 14.1) : l'argument « un chemin qui en reprendrait un morceau équivaut à une autre paire » ne vaut que pour un préfixe ou
un suffixe de `[i, j]`, pas pour un morceau du milieu, et le masque total coûtait cher (allonger 75,0 → 96,4 %, raccourcir 41,8 → 57,4 % à
la première mesure, § 14.1). C3 ne l'interdit pas : seules les jambes hors de `[i, j]` sont gardées.

Le masque coûte une passe sur les liens de la fenêtre, comme la table : les deux se font dans la même passe.

---

## 4. Le cap de la jambe gardée, au départ et à l'arrivée d'une recherche

Trois mécanismes, chacun à sa place ; aucun ne change la règle de virage du générateur.

**4.1 Virage simple à la couture, quand la couture est le bout d'un arbre déjà bâti** (`move_end` : l'arbre retour finit à
la couture ; raccourcir : l'arbre aller arrive à la couture `j`). La transition se juge après coup, état par état :

- départ du raccord en `k` : `turn_cost(v, forb, f_in, pred_in, fo, None, None)` et `retro_ok(v, pred_in, f_in, fo)` — le
  prédécesseur gardé prolonge le cap d'une chaîne courte (`in_head_ext`), ce que le prototype perdait avec `NONE` ;
- arrivée du tronçon neuf en `j` : `turn_cost(v, forb, fe, t1.pred(fe), f_out, None, None)` et `retro_ok(v, fe, f_out,
  succ_out)`.

**4.2 Grappes de 100 m de part et d'autre de la couture.** `OrgSearch::junction_ok` avec le modèle
`TurnModel::new(e.params.max_turn_deg, margin)` :

- en `i` (ou `k`) : `ctx = lead_before`, `ret` = les états du tronçon neuf, `base_dm = dm_from_start` (la couture est loin du
  départ : la levée de début ne joue pas, à juste titre), `aux` = l'arbre de l'autre bout quand il existe ;
- en `j` : `ctx` = la fin du tronçon neuf, `ret = [f_out] + lead_after`.

Une sortie refusée est écartée et la suivante est prise (§6.3, §5 de `move_end`). `margin` vaut **1°** [proposé] : le
modèle et le contrôleur prennent les caps dans des repères qui diffèrent d'au plus 1,34° sur 80 km [lu, doc de
`check_route_aligned_in`] ; une marge trop grande refuse des sorties que le contrôleur accepte, ce que le banc compte
(`junction_cut`).

**4.3 Une recherche qui part de la couture** (l'arbre aller de raccourcissement, la lentille d'allongement). Elle part
d'une **accroche de couture** :

```rust
pub(crate) fn seam_anchor_start<B>(e: &Engine<B>, v: &ViewData, w: &Weighter<'_>, s: &SeamState) -> (Snap, Anchor);
pub(crate) fn seam_anchor_end<B>(e: &Engine<B>, v: &ViewData, w: &Weighter<'_>, s: &SeamState) -> (Snap, Anchor);
```

- départ : le `Snap` est posé sur la chaîne de `f_in`, au nœud (`offset` au bout, `dist_m` 0), puis `make_anchor` et deux
  corrections : le sens qui repart le long de `f_in` (le demi-tour sur la jambe gardée) passe à `INF` ; le sens de longueur
  nulle qui finit au nœud garde un coût nul et reçoit pour **cap d'arrivée** `out_head = in_head_ext(v, f_in, pred_in)`. La
  première relaxation des arbres lit justement ce cap (`hin_over = out_head` quand `pred == NONE` [lu, `tree_forward`]) : le
  virage simple au départ est celui de la jambe gardée ;
- arrivée : le symétrique sur la chaîne de `f_out` ; `in_head = out_head(v, f_out)` (le cap de la jambe gardée qui repart),
  et le sens qui arriverait en remontant `f_out` passe à `INF`.

Le morceau de longueur nulle produit une étape vide en tête (et en queue) du parcours rendu par `steps_of` [lu]. Le contrôle
la lirait avec un cap dégénéré. Toute route rendue par une recherche de couture passe donc par
`graft::normalize_between(route, node_i, node_j)` (le portage de `control::normalize_between` du banc [lu]) **avant tout
contrôle**, celui de la lentille compris (§7.1).

Les arbres ne prennent pas de `Lead` et restent inchangés : la grappe au départ est rejouée après coup par 4.2. Dans l'arbre
aller de raccourcissement, le modèle de virage est actif (`cx.org`), ce qui juge l'intérieur du tronçon neuf comme le
contrôle ; sa levée de début (200 m après `i`) y est fausse, et c'est `junction_ok`, qui rejoue exactement ces 200 m
(`JUNCTION_DM`) avec le `lead`, qui la corrige.

---

## 5. L'interdiction durable, hors de `probe`

- **`KeepOut`** quitte `probe_graft.rs` pour `graft.rs`, public, inchangé (bitset de chaînes, empreinte des tuiles, `assert`
  de fenêtre) ; `build(win, mask_edges, free_edges)` garde sa signature. `probe_graft` le réexporte
  (`pub use crate::graft::KeepOut;`) : le prototype et `tests_graft.rs` compilent et passent **sans modification**.
- **`Weighter`** : le champ `keep_out: Option<&[u64]>` et `with_keep_out` perdent leur `#[cfg(feature = "probe")]`. Le test
  en tête de `chain_plain` aussi.
- **Le passage** se fait par paramètre : les opérations bâtissent leur `Weighter` avec le masque ; la lentille le reçoit par
  le paramètre interne de `collect` (§7.1). `Workspace::keep_out` et `set_keep_out` **restent sous `probe`**, réservés au
  chemin `--impl lot0` du banc ; ils partiront au 7b avec `probe_graft::move_end_in`, une fois la campagne de comparaison
  faite.
- **Pourquoi les générations ne changent pas.** `Weighter::new` pose `keep_out: None` ; aucune génération et aucun
  itinéraire ne l'appellent avec un masque. Le code compilé sans `probe` gagne une branche jamais prise dans `chain_plain`,
  aucun calcul flottant ni aucun ordre de tas ne change. Les références figées (`tests_reference.rs`), les tests de
  l'itinéraire (`tests_route.rs`) et l'empreinte I5 du banc (`50adca52ef713d18`, commande du §12 de la note du lot 0) le
  vérifient ; le temps de génération de l'essai I5 est relevé avant et après, pour une branche de plus dans la boucle chaude
  **[proposé : écart attendu sous 1 %]**.

---

## 6. Raccourcir par arbre d'itinéraire

`Δ = D′ − L < 0`. On cherche une paire de coutures `(i, j)` dont l'économie `ℓ(i, j) − m(i, j)` approche `|Δ|`, où
`ℓ = s_j − s_i` (abscisses d'origine) et `m(i, j)` la longueur du meilleur chemin de `i` à `j` aux poids de la requête.

### 6.1 Coutures `i` candidates

- coutures valides des deux côtés, `0,05 L ≤ s_i ≤ 0,95 L − |Δ|` (au moins `|Δ|` de parcours après `i`) ;
- **6 cases** égales sur cet intervalle, une couture tirée par case par la graine (`SplitMix64(seed ^ empreinte)`) [proposé :
  le nombre d'arbres du lot 0, 6 `i`] ;
- les cases sont jouées dans un ordre tiré lui aussi, pour que l'arrêt anticipé (§6.4) ne favorise pas le début du
  parcours.

### 6.2 Un arbre par `i`

- `tree_forward` depuis `seam_anchor_start(i)`, poids masqué (§3.2), `cx.org` = le modèle de virages, `Forbidden` de la passe ;
- **zone** : un disque de centre `i` et de rayon `R_i = clamp(max_j (ℓ(i, j) − (1 − τ) |Δ|), 2 km, 20 km)`, où le maximum
  porte sur les `j` admissibles (§6.3) et `τ = pick_tol`. Un chemin de longueur `R` reste à moins de `R` de son départ :
  le disque contient **tous** les chemins utiles, l'arbre est exact dans sa zone. 20 km **[proposé]** borne le temps : un
  raccourcissement qui demande un tronçon neuf plus long est rendu par une autre paire, ou échoue (`no_economy`) ;
- coût **[estimé]** : un disque de 20 km fixe de l'ordre de 10⁵ à 4 × 10⁵ états, soit 50 à 150 ms à chaud (l'itinéraire
  Nantes → Angers fixe 252 355 états en 77 ms [mesuré, `ord-route-lot1.md` §5]) ; six arbres tiennent dans la seconde.

### 6.3 Lecture des `j` et choix

Pour chaque couture `j` valide des deux côtés, après `i`, avec `s_j ≤ 0,95 L` et `ℓ(i, j) ≥ |Δ| + 1 km` (borne basse du
tronçon neuf, plan §4.3) :

1. **arrivée** : parmi les états `fe` d'arrivée au nœud de `j` atteints par l'arbre, celui de plus petit
   `t1.get(fe) + turn_cost(fe → f_out)` avec `retro_ok` (§4.1) ; `m(i, j) = t1.meters(fe) / 10` ;
2. **économie** `econ = ℓ − m` ; écart prévu `err₀ = |L − econ − D′| / D′` ;
3. **admissible** si `err₀ ≤ RetouchTune::shorten_read_tol`, **3 %** (`tolerance`) depuis la seconde campagne (§ 14.1) ; c'était
   `pick_tol` = 1 % [proposé] : la longueur lue est celle du tronçon rendu, il n'y a pas d'A\* de confirmation, et une lecture entre 1 et 3 %
   est un succès jeté. `pick_tol` ne règle plus que l'arrêt anticipé (§ 6.4) ; le tri des lectures est celui du tri final (écart arrondi
   à 0,5 %, puis part gardée) ;
4. au plus **2 `j` par `i`** [proposé], les plus petits `err₀`, puis la plus grande part gardée ;
5. pour chacun : chemin `t1.path_to(fe)`, étapes, `normalize_between`, puis `junction_ok` en `i` et en `j` (§4.2), puis, la partie
   remplacée étant libre (§3.2), un refus si le chemin reprend une jambe gardée **d'après `j`** (C3 local : l'arbre de la couture `i` ne
   masque que les jambes d'avant `i` et d'après `j_max`), puis `check_spliced_in` (§8). Refusé : le suivant. Si tous les `j` d'un `i` sont refusés pour un virage, **une** passe de plus
   sur ce `i` avec les transitions refusées dans `Forbidden` [proposé].

Il n'y a **pas** d'A\* de confirmation : le masque de l'arbre est celui du recollé, le chemin de l'arbre est donc le chemin
final.

### 6.4 Note, arrêt et budget

- note des propositions : celle du lot 0 (§5.4 de sa note) — `err ≤ 3 %` d'abord, puis `err` arrondi à 0,5 %, puis la part
  gardée, puis l'ordre de tirage ; trois propositions au plus, **de cases `i` distinctes** ;
- **arrêt anticipé** : dès que trois propositions contrôlées à 1 % ou moins sont tenues, les cases restantes ne sont pas
  jouées ;
- **échéance** : `soft_ms` (1 000 ms [proposé]) passé avec au moins une proposition à 3 % : on s'arrête ; `budget_ms`
  (1 800 ms [proposé]) : la closure `stop` de l'appelant est combinée à l'échéance, et ce qui est tenu est rendu ; rien de
  tenu → `Timeout`.

### 6.5 Ce que la méthode ne sait pas faire

Retirer moins que le détour qu'un tronçon contient. Un aller simple presque droit n'a pas d'économie à rendre : pour tout
`(i, j)`, `ℓ ≈ m`. C'est l'issue `NoEconomy`, nommée, au lieu du `no_window` du lot 0 (650 des 676 `no_window` étaient des
raccourcissements d'allers simples [mesuré]). Raccourcir un aller simple exige de déplacer une extrémité : hors du lot
(D5 a écarté le déplacement du départ ; l'arrivée a son opération, `move_end`).

---

## 7. Allonger : la lentille, ce qui change

### 7.1 L'appel

`collect` (dans `generate.rs`, crate) gagne un dernier paramètre `graft: Option<&LensGraft<'_>>` ; **tous** les appels
existants passent `None`, et avec `None` le corps est celui d'aujourd'hui. Avec `Some(g)` :

```rust
pub(crate) struct LensGraft<'a> {
    pub keep_out: &'a KeepOut,
    pub start: &'a SeamState,
    pub end: &'a SeamState,
    pub node_i: u32, pub node_j: u32, // nœuds de base, pour `normalize_between`
}
```

- le `Weighter` reçoit `g.keep_out` ;
- l'accroche `e.snap` des deux bouts est remplacée par `seam_anchor_start(g.start)` et `seam_anchor_end(g.end)` (§4.3) ;
- chaque candidat est passé par `normalize_between` avant `check_route_in` ;
- sous `probe`, un masque posé par `ws.set_keep_out` en même temps qu'un `graft` est une erreur (`assert`).

Une fonction crate `generate_graft_in(e, win, team, req, graft, ws, stop)` enveloppe `run_step` comme `generate_with_figures_in`
le fait, et rend les candidats contrôlés et notés. `Request` y est un aller simple (`start` = nœud `i`, `end = Some(` nœud
`j )`, `distance_m = ℓ + Δ`, `bearing: None`, `Figures::Classic`, curseur de la requête).

### 7.2 Fenêtres `(i, j)`

Celles du lot 0 (§5.3 de sa note) : `ℓ* ∈ {1,5 ; 3} × |Δ|`, 6 cases, `j` dont `s_j − s_i` est la plus proche de `ℓ*` dans
`[0,8 ; 1,25] × ℓ*`, coutures valides des deux côtés ; 12 au plus. Deux changements :

- **ordre** : toute la famille `1,5 |Δ|` d'abord (elle a donné 76,3 % des meilleures propositions [mesuré]), cases dans un
  ordre tiré ;
- **arrêt** : celui du §6.4 (trois propositions contrôlées à 1 %, `soft_ms`, `budget_ms`). Le temps jusqu'à la première
  proposition à 3 % avait un P90 de 442 ms quand la somme de toutes les fenêtres en avait un de 4 060 ms [mesuré] : c'est le
  levier principal sur K4.

### 7.3 Ce qui ne change pas

Les poids, la lentille, les secteurs, la tolérance de ±10 % au tronçon (`Params::tolerance`) et le contrôle interne : le
moteur de production. Le seuil de ±10 % au tronçon fait au plus ±5 % du total à `Δ = +25 %` et `ℓ = 1,5 Δ` [calculé au lot
0] ; le choix par `err` sur toutes les fenêtres a suffi (écart P90 0,61 % [mesuré]).

---

## 8. Contrôle du parcours recollé, dans le moteur

```rust
/// Le contrôle interne d'un tronçon neuf posé entre deux contextes gardés, par différence.
pub fn check_spliced_in<B: AsRef<[u8]>>(
    e: &Engine<B>, win: &Window, team: Option<&TeamData>, req: &Request,
    before: &[Step], new: &Route, after: &[Step],
) -> SplicedCheck;

pub struct SplicedCheck {
    pub violations: Vec<Violation>,   // celles du recollé absentes des deux contextes contrôlés seuls
    pub sharp_at_new: Vec<usize>,     // virages refusés, en indices d'étapes de `new` (−1 : la couture d'entrée)
    pub seam_u_turn: bool,
}
```

- `before` et `after` : les étapes entières des jambes gardées sur **400 m** de part et d'autre [proposé : 100 m de grappe,
  200 m de bout libre, 50 m de fenêtre de cap, et une marge], coupés au premier trou ou `Touch` ;
- le recollé `before + new + after` est contrôlé par `check_route_with` (inchangé, repère du départ), les deux contextes
  seuls aussi ; la différence d'ensembles est rendue, sans `WrongStart`, `WrongEnd`, `BadOffset` et
  `DistanceOutOfTolerance` (artefacts de découpe, comme C2 au lot 0) ;
- les codes ne sont pas tous jugés par présence : `SharpTurn` et `UTurn` le sont **par indices** (celui que le contexte seul avait
  déjà est écarté, celui que le tronçon apporte reste vu, même si le code figure déjà dans le contexte), `TooManyCrossings` par
  **différence de comptes** (le recollé n'a pas plus de croisements que la somme des deux contextes), `CorridorDoubled` par
  comparaison de la part doublée au plus grand des deux contextes ; les autres le sont par présence (§ 14.1) ;
- `sharp_at` est ramené aux étapes de `new` : un indice dans `before` ou à la couture désigne la transition de départ (la
  sortie est écartée, §4.2), un indice dans `after` la transition d'arrivée ;
- pour `move_end`, `after` est vide et la levée de fin de la règle des grappes joue à `E′`, qui est la vraie arrivée.

C'est l'équivalent interne de C2, sur des fenêtres courtes : quelques millisecondes par proposition [estimé]. Le contrôleur
indépendant (`ord-loops-check`) n'est pas appelé par le moteur ; il reste le juge du banc, et du service au 7b.

---

## 9. API publique

### 9.1 Requête et réglages

```rust
pub struct RetouchRequest {
    pub ride_type: RideType, pub mode: Mode, pub familiarity: f32, pub off_team_cost_q8: u32, pub seed: u64,
}

pub struct RetouchTune {
    pub end_snap_m: f64,              // 500 : un point posé sur la carte, comme l'itinéraire [proposé]
    pub move_end_radius: (f64, f64, f64, f64), // (facteur, marge, min, max) : clamp(1,6 × vol d'oiseau + 3 km ; 5 ; 40 km)
    pub tolerance: f64,               // 0,03 (plan §4.3 ; gardé, ±2 % ou ±1 % ne gagnent rien [mesuré])
    pub pick_tol: f64,                // 0,01 (arrêt anticipé seulement)
    pub shorten_read_tol: f64,        // 0,03 : une couture j est lue dans l'arbre si l'écart prévu est de 3 % au plus (§6.3, §14.1)
    pub max_change: f64,              // 0,30 (D3) ; le banc joue 0,40 pour voir au-delà
    pub min_change_m: f64,            // 1 000
    pub min_segment_m: f64,           // 1 000
    pub ends_share: f64,              // 0,05
    pub shorten_trees: usize,         // 6
    pub shorten_radius_max_m: f64,    // 20 000
    pub shorten_j_per_i: usize,       // 2
    pub lengthen_windows: usize,      // 12
    pub free_section: bool,           // true : la partie remplacée est libre pour l'allongement (§3.2, §14.1)
    pub free_shorten: bool,           // true : et pour le raccourcissement (§6.2, §14.1)
    pub proposals: usize,             // 3
    pub passes: usize,                // 4 (move_end, comme le prototype)
    pub turn_margin_deg: u8,          // 1
    pub soft_ms: u32, pub budget_ms: u32, // 1 000, 1 800 (resize) ; move_end n'a que `stop`
}
```

### 9.2 Fonctions

```rust
impl<B: AsRef<[u8]>> Engine<B> {
    /// Fenêtre de `move_end` : le disque de rayon maximal autour de `E′` (celle du lot 0).
    pub fn plan_move_end(&self, posed: &PosedRoute, end: LatLon, tune: &RetouchTune, step_m: f64, extra_m: f64) -> WindowPlan;
    /// Fenêtre de `resize` : l'union (`tiles_union`) des zones de ses recherches — disques de raccourcissement, ellipses des
    /// fenêtres d'allongement. Le choix des coutures se fait sans fenêtre (règles 1, 3, 4) ; la règle 2 se juge ensuite.
    pub fn plan_resize(&self, posed: &PosedRoute, target_m: f64, req: &RetouchRequest, tune: &RetouchTune,
                       step_m: f64, extra_m: f64) -> Result<ResizePlan, RetouchFailure>;
}

pub fn move_end_in<B>(e: &Engine<B>, win: &Window, team: Option<TeamCtx<'_>>, posed: &PosedRoute, end: LatLon,
                      req: &RetouchRequest, tune: &RetouchTune, ws: &mut Workspace, stop: &mut dyn FnMut() -> bool)
                      -> Result<Retouch, Box<Refused>>;
pub fn resize_in<B>(e: &Engine<B>, win: &Window, team: Option<TeamCtx<'_>>, posed: &PosedRoute, plan: &ResizePlan,
                    req: &RetouchRequest, tune: &RetouchTune, ws: &mut Workspace, stop: &mut dyn FnMut() -> bool)
                    -> Result<Retouch, Box<Refused>>;
/// `Refused { failure: RetouchFailure, diagnostics: RetouchDiagnostics }` : un refus rend ses compteurs (arbres joués, lectures,
/// coupes de la règle des grappes), au lieu de les perdre.
/// Les mêmes en bâtissant les fenêtres (et les agrégats d'équipe), rejouées une fois élargies sur `WindowEscape` :
/// pour les tests et les outils, comme `route`.
pub fn move_end<B>(…) -> Result<Retouch, RetouchFailure>;   // sans les diagnostics d'un refus
pub fn resize<B>(…) -> Result<Retouch, RetouchFailure>;
```

`ResizePlan` porte la fenêtre (`WindowPlan`), le sens (raccourcir ou allonger) et la liste ordonnée des coutures `i` ou des
fenêtres `(i, j)` tirées : `resize_in` joue exactement ce que le plan a dimensionné.

**Une fenêtre par opération** [proposé]. Le lot 0 prenait une fenêtre par paire `(i, j)` au cache du banc [lu] ; une seule
fenêtre suit le patron de `route_in` et laisse le cache au service. Le risque est la taille de l'union pour un parcours de
plus de 200 km : le banc relève les octets et le temps de construction à froid (§11.4) ; au-delà d'un seuil à fixer sur ces
chiffres, `plan_resize` rendra plusieurs plans.

### 9.3 Résultat

```rust
pub struct Retouch { pub proposals: Vec<Retouched>, pub diagnostics: RetouchDiagnostics }

pub struct Retouched {
    pub kind: RetouchKind,            // Shorten, Lengthen, MoveEnd(Best | KeepAll | Nearest), Truncate
    pub from_seam: usize,             // indice dans PosedRoute::seams
    pub to_seam: Option<usize>,       // None pour move_end et Truncate
    pub from_s_m: f64, pub to_s_m: f64, // abscisses d'origine de la partie remplacée (to = L pour move_end)
    pub new: Route,                   // le tronçon neuf, mesuré (`route::measure`) ; vide pour Truncate
    pub length_m: f64,                // L′ = s_i + longueur(new) + (L − s_j)
    pub kept_m: f64,
    pub err: Option<f64>,             // |L′ − D′| / D′ (resize)
    pub abandon_m: Option<f64>,       // move_end
}

impl Retouched {
    /// La géométrie complète : `line_between(0, s_i)`, le tronçon neuf (`route_geometry`), `line_between(s_j, L)`. La
    /// couture est le nœud ; le point de coupe de l'origine est à `proj_tol_m` au plus de lui (15 m).
    pub fn geometry<B>(&self, e: &Engine<B>, posed: &PosedRoute) -> Vec<LatLon>;
}

pub struct RetouchDiagnostics {
    pub seams_offered: u32, pub seams_refused_window: [u32; 3], // règle 2, nœud incomplet, chaîne d'arrivée partielle
    pub short_leads: u32, pub trees: u32, pub settled: u64, pub windows_tried: u32,
    pub junction_cut: u32, pub spliced_refused: BTreeMap<Violation, u32>, pub passes: u32,
    pub stopped_early: bool, pub deadline_hit: bool, pub radius_m: Option<f64>, pub widened: bool, pub elapsed_us: u64,
    // raccourcir : j lus, j refusés au virage d'entrée, chemins à tête inattendue, chemins reprenant une jambe d'après j,
    // meilleur écart err₀ lu par arbre
    pub shorten_reached: u32, pub shorten_turn_refused: u32, pub shorten_head_mismatch: u32, pub shorten_reuse_refused: u32,
    pub tree_best_err0: Vec<Option<f64>>,
}
```

### 9.4 Échecs nommés

| `RetouchFailure` | Quand | Lot 0 |
|---|---|---|
| `Pose(PoseError)` | l'entrée est refusée (`OrderAnomaly` en tête) | traces gardées, 5 % d'anomalies [mesuré] |
| `NoSeam` | aucune couture valide pour l'opération (marges, règle 2) | `no_seam` |
| `EndTooFar` | `E′` à plus de `end_snap_m` d'une voie du type | `end_too_far` |
| `NoSeamReached` | `move_end` : aucune sortie de couture dans l'arbre, rayon maximal compris (§4) | `no_seam_reached` |
| `JunctionCut` | des sorties atteintes (`move_end`) ou des chemins lus (raccourcir), tous coupés par la règle des grappes ; rien n'a été proposé au contrôle du recollé | — (compté dans `no_seam_reached` et `engine_fail` au premier relevé) |
| `ChangeTooSmall` | `|Δ| < min_change_m` | — |
| `ChangeTooLarge` | `|δ| > max_change` (D3, renvoi au générateur) | marque `beyond_limit` |
| `NoEconomy` | raccourcir : aucune paire n'offre l'économie à `pick_tol` près dans les arbres joués | `no_window` (650 des 676) et `DISTANCE_TOO_SHORT` |
| `NoWindow` | allonger : aucune paire `(i, j)` à l'échelle de `ℓ*` | `no_window` (26) |
| `NoProposal { dominant: Option<Failure> }` | toutes les recherches ont échoué (échec dominant du moteur pour la lentille) | `engine_fail` |
| `AllRefused { dominant: Violation }` | des chemins, tous refusés par §4.2 ou §8 | `all_refused` |
| `OffTarget { best_err }` | des propositions contrôlées, aucune à `tolerance` | `off_target` |
| `Timeout` | `budget_ms` passé sans proposition | — |
| `NoTeamData`, `WindowEscape`, `Stopped`, `TooLong` | comme la génération et l'itinéraire | |

`E′` sur le parcours n'est pas un échec : si l'accroche **sans masque** tombe sur une arête de la marche, l'issue est
`Truncate`, coupée au **dernier** passage de cette arête (on garde le plus), à la projection exacte du point accroché sur la
polyligne d'origine, sans couture. Si elle tombe sur une chaîne masquée dont l'arête n'est pas dans la marche (le cas
`end_on_masked_chain` du lot 0, 166 essais [mesuré]), `E′` est réaccroché avec le poids masqué, et l'opération suit son
cours.

### 9.5 `move_end`, l'algorithme

1. Pose déjà faite ; requête : `NoTeamData` comme la génération.
2. Accroche de `E′` (§9.4 pour le cas « sur le parcours »).
3. Table des coutures valides **avant** (marge de 100 m avant), masque de toute la marche.
4. Arbre retour depuis `E′` sur le disque `ρ` (rejoué une fois à `2ρ` si aucune couture n'est atteinte), `cx.org` = le modèle
   de virages (le raccord est jugé en lui-même à la relaxation, sa levée de fin est la vraie, à `E′`).
5. Pour chaque couture `k` atteinte : les sorties `fo` triées par `t2.get(fo) + turn_cost(f_in, pred_in → fo)` (§4.1), la
   première qui passe `junction_ok(lead_before, chemin de fo, aux = t2)` (§4.2).
6. Choix du lot 0 : `k*` minimise `abandon + raccord`, à égalité la plus tardive ; variantes « tout garder » et « quitter au
   plus près ».
7. `check_spliced_in` sur les trois variantes ; une variante refusée pour un virage du raccord ajoute ses transitions à
   `Forbidden` et l'arbre est rejoué (au plus `passes`, comme le prototype) ; refusée pour la couture, sa sortie est écartée
   et la sortie ou la couture suivante est prise, sans rejouer l'arbre.

---

## 10. Tests unitaires (grilles faites à la main, comme `tests_route.rs`)

`crates/ord-loops/src/tests_posed.rs` :

1. Une marche continue en L sur une grille : coutures aux seuls nœuds de vue, marges de 100 m, `s_m` égales à l'abscisse de
   marche quand la polyligne d'origine est faite des arêtes (distance de projection nulle).
2. Une jonction en demi-tour et un virage de 160° ne sont pas des coutures ; `Touch` et `Hole` coupent les plages ; les
   compteurs par règle le disent.
3. Une polyligne d'origine dont deux tronçons sont permutés : `OrderAnomaly`. Une jambe en double : `Drift`.
4. Un 8 : le nœud central donne deux coutures, d'abscisses distinctes et croissantes ; deux tours d'un circuit : aucune
   fusion entre les tours.
5. `line_between` : découpe exacte aux bouts, interpolée au milieu d'un segment.

`crates/ord-loops/src/tests_retouche.rs` :

6. **Masque** : vide, une génération rend les mêmes parcours que sans masque (repris du test du prototype) ; une chaîne
   masquée n'apparaît dans aucune proposition de `move_end` ni de `resize`.
7. **`move_end`, trois cas du plan §5** : `E′` voisin de l'arrivée → la dernière couture, court raccord ; `E′` au-delà du
   point lointain d'une boucle → le retour abandonné ; `E′` sur le parcours → `Truncate` au dernier passage.
8. **La grappe à la couture** : une jambe gardée qui finit par une chaîne de 40 m après un carrefour, la sortie la moins
   chère fait avec elle un virage de plus de 150° sur 80 m. Le moteur prend une autre sortie ou une autre couture,
   `check_spliced_in` ne relève rien. Sous `probe`, le même cas joué par `probe_graft::move_end_in` rend la sortie refusée :
   le test montre la cause et la correction.
9. **Le cap gardé au départ** : une couture où la sortie « tout droit » selon la seule chaîne `f_in` est un virage de plus
   de 150° selon `in_head_ext(f_in, pred_in)` : refusée.
10. **Raccourcir une boucle à lobe** : un lobe de longueur connue sur la boucle ; `Δ` = l'excès du lobe → la proposition
    retire exactement le lobe, `err < 0,5 %` ; `m(i, j)` égale la distance d'un Dijkstra de référence (celui de
    `tests_route.rs`) sur le graphe masqué.
11. **Raccourcir un aller simple droit** : `NoEconomy`.
12. **Allonger une boucle de 20 %** : `err ≤ 3 %`, aucune arête gardée reprise, pas de demi-tour aux coutures, le tronçon
    commence et finit aux nœuds des coutures (`normalize_between`).
13. **Accroches de couture** : le sens qui remonte la jambe gardée est fermé ; la première relaxation lit le cap gardé ;
    l'étape vide de tête disparaît après `normalize_between`.
14. **Bornes et refus** : `ChangeTooSmall`, `ChangeTooLarge`, `NoSeam`, `EndTooFar`, `NoTeamData`, `Timeout` (une échéance
    nulle), `Stopped`.
15. **Arrêt anticipé et déterminisme** : mêmes entrée et graine → mêmes propositions, dans le même ordre ; trois propositions
    à 1 % arrêtent le jeu des cases (`stopped_early`).
16. **`check_spliced_in`** : un défaut présent dans le contexte seul n'est pas rendu ; une grappe à cheval sur la couture
    l'est, avec l'indice de couture.
17. **Géométrie** : `Retouched::geometry` commence par la polyligne d'origine jusqu'à `s_i` et finit par elle depuis `s_j`,
    au point près.

**Inchangés** : `tests.rs`, `tests_reference.rs`, `tests_route.rs`, `tests_graft.rs` et les autres. Les générations et les
itinéraires gardent leurs références sans modification, dans les deux jeux de fonctionnalités.

---

## 11. Le banc : `retouche-play` sur l'API du moteur

### 11.1 Ce qui change

- `--impl engine|lot0` (défaut `engine`). `lot0` est le code d'aujourd'hui, intact, pour rejouer les mêmes tirages. `engine` :
  - **pose** par `ord_loops::PosedRoute::pose`, depuis la marche (`Walk` → `PosedLeg`/`PosedJoin`, une conversion de dix
    lignes dans le banc) et la polyligne du document ; `Pose(_)` est compté par cause, **hors** des dénominateurs, et la part
    des traces refusées à l'entrée est donnée à côté de chaque critère ;
  - **`move_end`** : les mêmes `E′` (`draw.rs` inchangé : trois rayons × quatre caps, un lieu d'équipe, la fermeture de
    boucle pour information), un appel `move_end_in` dans la fenêtre de `plan_move_end` prise au cache du banc ;
  - **`resize`** : les mêmes δ et le même échantillon de traces ; le banc ne tire plus de fenêtres, `plan_resize` et
    `resize_in` font le choix ;
  - **contrôles** : C1, C2 (plages entières) et C3 du lot 0, inchangés, sur `Retouched` (le recollé se fabrique depuis
    `from_seam`, `to_seam` et `new`).
- Sorties sous `~/.cache/osm-route-details/graph/retouche-bench/` (défaut de `--out`), privées ; seul le résumé `.md`
  (agrégats, empreintes xxh3) se recopie dans le relevé `ord-retouche-lot1-mesures.md`. Ni position, ni id, ni empreinte d'id.
- Champs nouveaux des lignes : issue du moteur, `stopped_early`, `deadline_hit`, `trees`, `windows_tried`, `junction_cut`,
  refus de `check_spliced_in` par code, coutures écartées par règle (pose et fenêtre), `short_leads`, octets et temps à froid
  de la fenêtre.

### 11.2 Critères de sortie (plan §10)

| # | Critère | Définition | Dénominateur |
|---|---|---|---|
| K2 | arrivée déplacée sans refus du contrôleur, **95 %** | variante `best` (ou `Truncate`) acceptée par C1, C2 et C3 | essais dont `E′` s'accroche (500 m), sur des traces posées sans erreur ayant au moins une couture ; `close_loop` exclu ; `NoSeamReached` compte comme échec |
| K3 | distance à ±3 %, **90 %**, pour \|δ\| ≤ 25 % | `Ok` avec une proposition à 3 % acceptée par C1 à C3 | demandes δ ∈ {±10 ; ±25} % ; verdict **pondéré par strate** comme au lot 0, brut à côté, et raccourcissement seul à côté |
| K4 | P90 **sous 2 s** | temps de l'appel du moteur (`move_end_in`, `resize_in`), contrôles internes compris, fenêtre prise au cache | toutes les demandes jouées ; à 16 fils **et** à 1 fil sur un tirage de toutes les strates (la limite 3 du relevé du lot 0) ; le temps à froid de la fenêtre et celui des contrôles indépendants sont donnés à part |

Chaque critère est jugé sur l'intervalle de Wilson à 90 %, comme au lot 0. **Remplace K1** (plan §10 bis) : la part des
demandes qui ont une paire de coutures à l'échelle de δ (`NoEconomy` et `NoWindow` comptés), par strate, sans seuil.

### 11.3 Invariants vérifiés au banc

- **Générations** : l'essai I5 redonne `50adca52ef713d18`, avec et sans `probe`.
- **Pose** : `retouche-continuity` sur la pose du moteur, aux marges de 200 et 100 m, redonne les agrégats du lot 0 (K1,
  ordre, trous, plages) ; la ligne par trace garde ses champs du lot 0 dans le même ordre, et l'empreinte de son `.jsonl`
  **sur ces seuls champs** doit être celle du lot 0 (`2120e0c1463a86a7`, `4cf8199be7b3e318`) — à condition que l'export
  soit le même (`ca53159cc90498d6`) [supposé : l'export n'a pas changé depuis].
- **Déterminisme** : `--threads 1` et `--threads 16` rendent la même empreinte de recollés.

### 11.4 Campagnes

1. Essai `--teams B,C --limit 20` : sorties, invariants, débit.
2. Complète `--impl engine` (les 3 355 traces pour `move_end`, l'échantillon de 587 traces pour `resize`) ; la marge de 100 m
   est celle du moteur.
3. Complète `--impl lot0 --seam-margin-m 100` : la référence à tirage égal (le relevé du lot 0 a cette variante, `fullm100`,
   on la rejoue sur le même binaire).
4. `--free-section` (allongement), une fois.
5. Recalage à 1 fil sur 10 traces par strate.

Durée [estimé] : celle du lot 0 (29 min la complète [mesuré]) ou moins, l'arrêt anticipé coupant l'allongement.

---

## 12. Fichiers

| Fichier | Quoi |
|---|---|
| `crates/ord-loops/src/posed.rs` (nouveau) | `PosedRoute`, `PosedLeg`, `PosedJoin`, `PosedInput`, `PoseTune`, `PoseError`, `PoseStats`, projection monotone (portage de `project.rs`), règles 1, 3 et 4 |
| `crates/ord-loops/src/graft.rs` (nouveau) | `KeepOut` (déplacé), `SeamState` et sa table, masque, accroches de couture, `normalize_between`, types publics (`RetouchRequest`, `RetouchTune`, `Retouch`, `Retouched`, `RetouchFailure`, `RetouchDiagnostics`), `plan_move_end`, `plan_resize`, `ResizePlan` |
| `crates/ord-loops/src/graft_move_end.rs` (nouveau) | `move_end_in`, `move_end` (§9.5) |
| `crates/ord-loops/src/graft_resize.rs` (nouveau) | `resize_in`, `resize` : arbres de raccourcissement (§6), fenêtres et arrêt de l'allongement (§7), note et choix |
| `crates/ord-loops/src/verify.rs` | `check_spliced_in`, `SplicedCheck` (ajout ; `check_route_with` inchangé) |
| `crates/ord-loops/src/search.rs` | `Weighter::keep_out` et `with_keep_out` hors `probe` ; le type de `Workspace::keep_out` (toujours sous `probe`) devient `crate::graft::KeepOut` |
| `crates/ord-loops/src/generate.rs` | `collect` : paramètre `graft: Option<&LensGraft>` (`None` partout ailleurs) ; `generate_graft_in` (crate) ; l'`assert` « pas les deux masques » sous `probe` |
| `crates/ord-loops/src/route.rs` | `measure` passe `pub(crate)` ; rien d'autre |
| `crates/ord-loops/src/turns.rs` | au plus des visibilités `pub(crate)` (`MAX_STATES`, `JUNCTION_DM`) |
| `crates/ord-loops/src/probe_graft.rs` | `KeepOut` remplacé par `pub use crate::graft::KeepOut;` ; le reste inchangé |
| `crates/ord-loops/src/lib.rs` | `mod posed; mod graft; mod graft_move_end; mod graft_resize;`, les `pub use`, les modules de test |
| `crates/ord-loops/src/tests_posed.rs`, `tests_retouche.rs` (nouveaux) | §10 |
| `crates/ord-loops/Cargo.toml` | commentaire de `probe` : le masque n'en dépend plus, seul le prototype du lot 0 y reste |
| `tools/ord-loops-bench/src/retouche/engine.rs` (nouveau) | conversion `Walk` → `PosedInput`, essais `move_end` et demandes `resize` par l'API, recollé depuis `Retouched` |
| `tools/ord-loops-bench/src/retouche/play.rs`, `mod.rs`, `out_play.rs` | `--impl`, `--free-section`, `--end-snap-m`, sortie par défaut `…/retouche-bench/`, champs et critères du §11 |
| `tools/ord-loops-bench/src/retouche/continuity.rs`, `posed.rs` | pose du moteur pour `retouche-continuity` (`--impl lot0` y garde l'ancienne) |
| `docs/design/technique/ord-retouche-lot1.md` | cette note ; le relevé `ord-retouche-lot1-mesures.md` suivra la campagne |

**Ne changent pas** : `ord-server`, le contrat, `ord-loops-check` (il reste indépendant), `ord-graph-match`, les poids, la
lentille, les arbres (`tree_forward`, `tree_backward`), les A\*, `route_in`.

Avant de rendre la main : `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, la même chose avec
`--features probe` sur `ord-loops` et le banc, `cargo test --workspace`, et `cargo test -p ord-loops --features probe`.

---

## 13. Ce que ce lot laisse ouvert

- **Le service** (7b) : endpoint, contrat, `capabilities`, la pose depuis `network_edges` dans `ord-server` (par
  `ord-graph-match`, qui peut fabriquer un `PosedInput`), la file et le budget, et le retrait de `probe_graft::move_end_in`
  et de `Workspace::set_keep_out`.
- **Raccourcir un aller simple droit** (`NoEconomy`) : seulement en déplaçant une extrémité, écarté par D5 pour le départ.
- **Une fenêtre par opération** pour les très longs parcours (§9.2) : à trancher sur les octets mesurés.
- **Le tronçon désigné à la main** (D4) : une seule paire `(i, j)`, l'arbre ou la lentille sans tirage ; rien ne l'empêche,
  rien ne le demande encore.
- **Si K2 reste sous 95 %** avec la grappe corrigée : relire les refus restants par code (le relevé du lot 0 en laissait
  11 %, C1 et C2 ensemble, croisements, réutilisation) avant tout nouveau mécanisme.

---

## 14. Écarts à la conception (relevés au codage, 24 novembre 2026)

Statut : le code du lot 7a est écrit (`ord-loops` : `posed.rs`, `graft.rs`, `graft_move_end.rs`, `graft_resize.rs`,
`verify::check_spliced_in`, tests `tests_posed.rs` et `tests_retouche.rs` ; banc : `retouche/engine.rs`, `--impl`). **La
campagne du §11.4 est jouée**, deux fois : le premier relevé, puis le second après la relecture (§14.1) ; les chiffres sont dans
[`ord-retouche-lot1-mesures.md`](ord-retouche-lot1-mesures.md). L'essai de fumée du premier jour (équipes B et C, 6 traces chacune) ne se lit pas comme une mesure.

Là où le code s'écarte de la note, et pourquoi :

- **`Retouched::from_seam` est un `Option<usize>`** (`None` pour `Truncate`, qui n'a pas de couture), non un `usize`.
  `SplicedCheck` rend `sharp_entry` et `sharp_exit` (deux booléens) plus `sharp_at_new` (indices d'étapes du tronçon) au lieu
  d'un indice `−1` pour la couture d'entrée.
- **`plan_move_end` ne prend pas `RetouchRequest`** : le type vient de `PosedRoute::ride_type()`, la fenêtre ne dépend ni du
  mode ni de la familiarité.
- **L'abscisse de marche `a_m` d'une couture** (entrée de la projection monotone) est la longueur graphe cumulée des jambes
  plus les cordes des trous, non l'abscisse sur la polyligne plane de la marche (`Trace::join_arc`, que le moteur ne peut pas
  recalculer sans `ord-graph-match`). L'écart est de l'ordre de l'écart entre longueur graphe et longueur plane d'une jambe ;
  **l'empreinte de continuité du lot 0 (§11.3) n'est pas vérifiée** et `retouche-continuity` n'a pas été basculé sur la pose du
  moteur (seul `retouche-play --impl engine` la lit). À mesurer avant de se fier à `OrderAnomaly` : la dérive `max_drift`
  (0,02) a refusé 1 trace sur 11 à l'essai de fumée.
- **Le masque et la table des coutures se font en deux passes** sur les liens de la fenêtre (`KeepOut::build`, puis
  `seam_table`), non en une ; un passage de plus sur les chaînes, sans effet sur les résultats.
- **`junction_ok` ne juge que les chemins d'arbre** (`move_end`, raccourcir), pas les candidats de la lentille : un candidat
  de la lentille est un itinéraire en états de chaînes qu'il faudrait reconstruire depuis ses pas (une table arête → chaîne de
  toute la fenêtre) ; `check_spliced_in` le juge sur 400 m de contexte, comme prévu au §8. Un virage de couture refusé par le
  modèle mais accepté par le contrôle passe donc dans la lentille.
- **`move_end` : au plus 4 sorties par couture** (`EXITS_PER_SEAM`) et 16 essais de choix par passe ; les trois variantes
  (`Best`, `KeepAll`, `Nearest`) sont toutes contrôlées par `check_spliced_in`, la refusée est omise (la `Best` seule peut
  faire échouer l'appel).
- **`Truncate`** : l'abscisse d'origine de la coupe est estimée par la part de la marche (jambe d'arrivée / longueur graphe ×
  `L`), puis le point accroché est projeté, dans `± (2 km + 2 % de L)`, sur le **dernier** segment de la polyligne à
  `proj_tol_m` ou moins (`PosedRoute::project_last`). Une trace qui repasse longtemps au même endroit peut s'y tromper.
- **Raccourcir** : pas d'`A*` de confirmation (comme prévu) ; les `j` sont lus par l'état d'arrivée le moins cher
  (`t1.get(fe) + turn_cost`), avec `retro_ok` sur `f_out` et son successeur ; un `i` dont tous les `j` sont refusés pour un
  virage est rejoué une fois avec `Forbidden`. L'arrêt anticipé et les échéances sont vérifiés **entre deux arbres ou deux
  fenêtres**, pas dans un arbre (l'échéance dure passe, elle, par la fermeture `stop`).
- **Allonger** : `LensGraft` porte les accroches déjà bâties (`Anchor`) et le masque, non les `SeamState` ; `generate_graft_in`
  rend les candidats notés de `collect`, la table de la fenêtre étant tirée une fois par `resize_in`. Les propositions de la
  lentille sont notées sur la distance mesurée du tronçon (`Check::length_m`).
- **`RetouchTune::max_change` vaut 0,30** par défaut (D3) ; le banc joue 0,40 (`--max-change`), donc `ChangeTooLarge` n'y sort
  pas pour ±35 %.
- **Tests** : les 17 de la note n'ont pas tous une assertion propre. Écrits : 1 à 5 (`tests_posed.rs`, 6 tests), 6 (aucune
  arête gardée reprise ; l'égalité masque vide / sans masque reste celle de `tests_graft.rs` sous `probe`), 7, 8 (et sa
  contrepartie sous `probe` : le prototype prend la sortie que la grappe refuse), 10 (le lobe retiré exactement ; **pas
  de comparaison à un Dijkstra de référence**), 11, 12, 13, 14, 15 (déterminisme et arrêt anticipé, faibles), 16, 17. **Le 9**
  (un virage de plus de 150° selon `in_head_ext(f_in, pred_in)` seul) n'a pas son test : le test 13 vérifie que le cap de
  l'accroche est `in_head_ext(f_in, pred_in)`, pas le refus qui en découle. Les tests existants ne sont pas modifiés.
- **Banc** : `--seam-margin-m` passe de 200 à 100 par défaut (la marge du moteur) ; `--impl lot0` demande donc
  `--seam-margin-m 200` pour rejouer le relevé du lot 0 tel quel (ou 100 pour la variante `fullm100`). Pour `--impl engine`,
  K4 est le temps de l'appel du moteur seul (`total_ms`, `call_ms`) ; les contrôles indépendants sont dans `indep_ms`.
  `first_ok_ms` y est le temps de l'appel entier quand la proposition est à `tolerance`. Les fenêtres de lentille ne sont plus
  détaillées (une ligne `WinLine` par demande). C3 est plus strict qu'au lot 0 : aucune arête gardée n'est libre, la zone des
  coutures non plus. Les invariants I5 (`50adca52ef713d18`) et de pose (§11.3) ne sont **pas** rejoués dans ce lot.

### 14.1 Après la relecture et le second relevé (24 novembre 2026)

La relecture a rendu quatre défauts du moteur et une conclusion fausse du premier relevé ; le moteur a changé, la campagne complète a été rejouée.

- **Un refus rend ses diagnostics.** `resize_in` et `move_end_in` rendent `Err(Box<Refused>)` (`failure` et `diagnostics`), `resize` et `move_end` gardent `Err(RetouchFailure)`. Le premier relevé lisait `trees: 0` sur
  tous les `no_economy` : un artefact, les arbres avaient tourné (§9.3, relevé §3.3).
- **Le contrôle du recollé juge par position et par comptes** (§8) : un défaut du même code que celui du contexte, apporté en plus par le tronçon, n'est plus effacé. K2 n'a pas bougé (75 866 / 77 114).
- **La règle des grappes a son nom** : `JunctionCut` (§9.4). Elle valait 42 essais de `move_end` sur 585 `no_seam_reached`, et 114 des `engine_fail` de raccourcissement du premier relevé.
- **Trois réglages pour K3, d'un bloc** : `free_section` et `free_shorten` vrais par défaut (la partie remplacée n'est masquée ni pour allonger ni dans l'arbre de raccourcissement ; le chemin lu ne doit pas reprendre une jambe
  d'après `j`), et `shorten_read_tol` = `tolerance` (3 %). K3 pondéré 64,0 → 89,0 %. **Décision : gardés**, ne pas les défaire sans remesure. **Non tenu** : 90 % à la lettre ; raccourcir un aller simple reste à 38,9 %.
- **À ne pas défaire** : la lecture des `j` par écart arrondi à 0,5 % puis part gardée (le tri par écart exact préférait une paire qui garde moins) ; le `Box` du refus (clippy, `result_large_err`) ; le banc lit les compteurs sur tout résultat.
- **Banc** : `--masked-section`, `--masked-shorten`, `--read-tol`, `--shorten-trees` (les anciennes options `--free-section` ne sont plus : le moteur libère par défaut).
- **Pistes ouvertes** : lire les `j` à 3 % de la section (74 demandes refusées par le seul `C1:DISTANCE_OUT_OF_TOLERANCE`) ; 12 ou 18 arbres ; masque par `j` pour les 272 refus « jambe d'après `j` reprise ».

---

## 15. Service (lot 7b) : conception

Statut : **conçu le 24 novembre 2026, codé le même jour (relevé au §16).** Plan appelant : `docs/plans/2026-11-22-retouche-parcours.md` (Pédalons),
§3, §4.3, §5, §6.3, §7 et §9. Modèle : l'itinéraire de A à B (`generator/router.rs`, [`ord-route-lot1.md`](ord-route-lot1.md) §7).
Marques comme plus haut ; **[tranché]** est un choix de cette conception, à ne pas rouvrir sans raison écrite.

Version : **1.18.0, API 1.17**, ajout pur au contrat. Générations, itinéraires et autres routes inchangés ; le moteur
`ord-loops` n'est **pas** modifié (§15.9).

### 15.1 Requête : `POST /v1/generator/routes/edit`

```json
{
  "polyline": "…",
  "network_edges": { "edges": [ … ] },
  "ride_type": "road",
  "cursor": 0.25,
  "seed": 0,
  "operation": { "resize": { "distance_m": 95000 } }
}
```

ou `"operation": { "move_end": { "point": { "lat": …, "lon": … } } }`.

| Champ | Type | Règle | Erreur (pointeur) |
|---|---|---|---|
| `polyline` | chaîne | polyligne Google, **précision 6**, le parcours enregistré ; décodée, les points consécutifs égaux en E7 retirés ; au moins 2 points distincts, chaque point dans [−90 ; 90] × [−180 ; 180] | 400 `/polyline` |
| | | au plus `edit.points_max` = **50 000** points après décodage | 413 `route_too_large`, `limit: points` |
| | | longueur `L` (haversine sur les points E7, la formule de `PosedRoute::pose`) au plus `limits.distance_m_max` | 413 `route_too_large`, `limit: length_m` |
| `network_edges` | objet (`serde_json::Value`, `#[schema(value_type = Object)]`) | le document de la trace tel que le backend le garde (§15.3) ; lu par `ord_graph_match::doc::Document` ; ses clés inconnues sont tolérées, comme au dépôt [lu, `deposit.rs`] | 400 `/network_edges` (« unreadable network_edges document ») si ce n'est pas un objet ou si `edges` n'est pas un tableau |
| `ride_type` | `road` \| `gravel` \| `mtb` | `RideTypeV1` | 400 `/ride_type` |
| `cursor` | nombre, facultatif | [0 ; 1], arrondi au centième ; **sémantique de l'itinéraire** (`cursor::apply_route`, pas de strict) ; absent : 1 (découverte) ; ignoré sans en-têtes d'équipe, mais validé | 400 `/cursor` |
| `seed` | entier ≥ 0, facultatif | défaut 0 ; « autres propositions » = autre graine | 400 `/seed` |
| `operation` | objet à **une** clé | `resize` ou `move_end` (enum externe de serde, `deny_unknown_fields`) ; zéro ou deux clés | 400 `/operation` |
| `operation.resize.distance_m` | nombre | fini, > 0, ≤ `limits.distance_m_max` ; c'est la **distance totale visée** `D′`, pas l'écart | 400 `/operation/resize/distance_m` |
| `operation.move_end.point` | `GeoPointV1` | `routes::point` | 400 `/operation/move_end/point/lat` (`/lon`) |

- **Corps** : `EDIT_BODY_MAX` = **4 Mio** (`generator/mod.rs`, publié en `edit.request_body_bytes`) : la génération garde ses
  64 Kio. Un document de 300 km pèse de l'ordre du mégaoctet [estimé : ~3 000 arêtes de 200 à 400 octets]. Au-delà : 413
  `limit: body_bytes` (`read_json`, inchangé).
- **JSON strict** : `read_json` / `parse_strict` ; toute clé inconnue hors de `network_edges` est listée (`unknown_keys`).
  `network_edges` est un `Value` et non un `RawValue` : `serde_ignored` et `serde_path_to_error` enveloppent le désérialiseur, le
  jeton magique de `RawValue` n'y passerait pas [supposé, non essayé] ; un `Value` ne signale aucune clé ignorée.
- **Pas de champ `window`** (désignation du tronçon à la main, D4) : l'ajouter plus tard est un ajout pur [tranché].
- **En-têtes** : `Ord-Namespace-Key` et `Ord-Team` **facultatifs**, validés dès que l'un est là (`credentials(&headers, true)`),
  comme l'itinéraire. Avec eux et une surcouche servable : le mode du curseur, le goût de l'équipe ; surcouche absente ou
  périmée : découverte, `overlay: null`. Le mode strict n'est pas offert : le banc n'a joué que les curseurs 0,25 et 1
  [mesuré], et un recollé strict échouerait dès qu'une couture tombe hors des routes de l'équipe [supposé] [tranché].

### 15.2 Réponse 200

```rust
pub struct EditResponseV1 {
    pub graph_id: String,
    pub mode: ModeV1,                   // jamais `strict`
    pub overlay: Option<OverlayInfoV1>,
    pub taste: TasteV1,
    pub operation: EditOperationKindV1, // resize | move_end
    pub original_m: f64,                // L, haversine de la polyligne reçue
    pub target_m: Option<f64>,          // D′ (resize), null sinon
    pub dropped: u32,                   // propositions refusées par le contrôleur indépendant (§15.6)
    pub routes: Vec<EditedRouteV1>,     // 1 à 3, la meilleure d'abord (ordre du moteur)
}

pub struct EditedRouteV1 {
    pub kind: EditKindV1,               // shorten | lengthen | move_end | truncate
    pub variant: Option<MoveEndVariantV1>, // best | keep_all | nearest (move_end), null sinon
    pub polyline: String,               // la polyligne complète recollée, précision 6
    pub distance_m: f64,                // Retouched::length_m (gardé sur la polyligne + tronçon neuf sur le graphe)
    pub error_share: Option<f64>,       // Retouched::err, au millième (resize)
    pub kept_m: f64,
    pub kept_share: f64,                // kept_m / original_m, au millième
    pub abandoned_m: Option<f64>,       // Retouched::abandon_m (move_end, truncate)
    pub end_snap_m: Option<f64>,        // move_end : du point demandé au dernier point rendu
    pub unmatched_m: f64,               // §15.4
    pub changed: ChangedV1,
}

pub struct ChangedV1 {
    pub from_m: f64,                    // Retouched::from_s_m, abscisse sur la polyligne d'ORIGINE
    pub to_m: f64,                      // Retouched::to_s_m (= original_m pour move_end et truncate)
    pub new_m: f64,                     // new.length_m (0 pour truncate)
    pub points: [u32; 2],               // indices, dans `polyline` rendue, du premier et du dernier point du tronçon neuf
    // Les champs utiles de RouteV1, mesurés sur le tronçon neuf seul (rapport du contrôleur, §15.6) :
    pub team_m: f64, pub bridged_m: f64, pub team_share: f64,
    pub offroad_m: f64, pub offroad_share: f64,
    pub unpaved: Vec<[u32; 2]>,         // plages d'indices de `polyline` rendue (route_geometry_spans, décalées)
    pub crossings: u32,
}
```

- **Pourquoi les mesures de RouteV1 sont celles du tronçon** [tranché] : le parcours gardé n'est pas un parcours du graphe (trous,
  `Touch`), ses `team_m` ou `offroad_m` seraient faux ; le client a déjà ceux du parcours d'origine. `ascent_m`, `family`,
  `tail_m`, `bearing_deviation_deg` n'ont pas de sens ici et ne sont pas repris.
- **Géométrie** : le service assemble lui-même, au lieu de `Retouched::geometry`, pour connaître les indices :
  `line_between(0, from_m)`, puis `route_geometry_spans(base, &new)` (vide pour `truncate`), puis, pour `shorten` et
  `lengthen`, `line_between(to_m, L)` ; un point égal en E7 au précédent est retiré, et les indices de `points` et `unpaved`
  sont ceux d'après ce retrait. Entre la coupe de l'origine et le nœud de couture, il reste un saut de 15 m au plus
  (`proj_tol_m`) [lu, doc de `Retouched::geometry`]. `truncate` : `points = [n − 1, n − 1]`.
- **Arrondis** : mètres au dixième (`r1`), parts au millième, comme `RouteV1`.

### 15.3 Du document `network_edges` à `PosedInput`

C'est le chemin du banc (`likeness::export::track_match` puis `Walk::from_pieces`, `retouche::engine::pose_of`) [lu], porté
dans **`ord-graph-match`** (qui dépend d'`ord-loops` et peut donc fabriquer un `PosedInput` [lu, `Cargo.toml`]), nouveau
module `crates/ord-graph-match/src/posed.rs` :

```rust
/// Une trace posée depuis son document : la marche, et les étendues non appariées sur l'axe du document.
pub struct DocWalk {
    pub walk: Walk,
    /// Étendues `[fm, tm]` de l'axe du document sans arête retenue (arêtes non comptées, écarts entre arêtes, bouts).
    /// `None` si une arête n'a pas `fm`/`tm` : rien n'est localisable.
    pub unmatched: Option<Vec<(f64, f64)>>,
    /// Somme des `trv` des arêtes non comptées (repli quand `unmatched` est `None`).
    pub unmatched_trv_m: f64,
    /// Longueur de l'axe du document : `len` du document s'il est là, sinon le plus grand `tm`.
    pub axis_m: f64,
}

pub fn doc_walk(gf: &MappedGraph, idx: &dyn Near, poly: &Poly, edges: &[ParsedEdge],
                stop: &dyn Fn() -> bool) -> Option<DocWalk>; // None : arrêté
pub fn posed_parts(walk: &Walk) -> (Vec<PosedLeg>, Vec<PosedJoin>); // la conversion de dix lignes du banc
```

1. Chaque arête du document : `match_job(gf, idx, poly, g, trv)` puis `agg::verdict(wk, job)` ; `Counted(chain)` ajoute
   `chain.pieces` dans l'ordre, tout le reste fait un trou — **exactement** la règle du dépôt (`Deposit::add_edge`) [lu], donc la
   marche que la surcouche a comptée. Pas de mémoïsation (une trace, peu de géométries répétées). `stop` est lu entre deux arêtes.
2. `Walk::from_pieces(gf, &pieces)` ; aucune pièce : `route_not_matched` (§15.5).
3. `posed_parts` ; `PosedInput { ride_type, legs, joins, line }` où **`line` est la polyligne de la requête** ; puis
   `PosedRoute::pose(engine, &input, &PoseTune::default())`.
4. `Document` lit en plus le champ racine `len` (`Option<f64>`, ignoré du dépôt) : c'est la longueur de l'axe de `fm`/`tm` dans
   le document que garde Pédalons [lu, `NetworkEdgesDocument.java`].

**L'index** : `CellIndex::with_wide(gf, edit_index_cache_mb, Arc::clone(wide))`, bâti par requête, comme un dépôt [lu,
`Worker::matching`] ; `edit_index_cache_mb` = **64** (nouvelle clé de configuration `generator.edit_index_cache_mb`, > 0). Sans
`cell_wide` (plus de 100 000 arêtes larges) le service refuse la retouche plutôt que de bâtir l'index entier à chaque requête :
`capabilities.generator.edit` absent, 503 `data_unavailable` (`source: graph`) [tranché].

**La polyligne de la requête, pas celle du document** [tranché]. Le banc posait sur la concaténation des `g` du document
[lu, `retouche::posed::pose`] : elle est recalée sur la rue et **joint les trous de route-details par une corde**. Rendre cette
ligne comme partie « gardée » changerait le parcours de la personne là où elle ne l'a pas demandé, ce que le produit promet de
ne pas faire (plan §3 : « rien n'est écrasé »). Conséquence **non mesurée** : la projection monotone (tolérance 15 m) et la
dérive (2 %) jugent maintenant la trace d'origine, plus bruitée qu'une ligne recalée pour un GPX enregistré par un compteur.
Les 5,0 % d'anomalies d'ordre et 0,5 % de dérive du banc [mesuré] sont donc une **borne basse** [supposé]. L'export du banc n'a
pas les polylignes : la mesure se fera à la recette, par la raison `order_anomaly` / `route_mismatch` de
`ord_generator_edit_requests_total` ; si elle dépasse 10 %, poser sur la ligne du document **pour les coutures seulement** est la
piste (une seconde projection vers la polyligne d'origine pour la découpe), pas un repli silencieux.

**Pas de cache des poses** [tranché] : une requête refait appariement et pose. Le temps de l'étape est publié
(`ord_generator_edit_stage_seconds{stage="match"|"pose"}`) ; un petit cache LRU en mémoire (clé xxh3 du corps et du graphe,
jamais journalisée) ne s'ajoute que si le P90 de `match + pose` dépasse 200 ms à la recette. Les fenêtres, elles, sont celles du
cache partagé des générations (§15.7).

### 15.4 `unmatched_m`

Ce qu'un trou gardé laisse sans garantie « pas de route prise deux fois » (plan §3). Calculé **par le service**, sur l'axe du
document, sans rien demander au moteur :

- étendues non appariées de `DocWalk::unmatched`, ramenées à la polyligne par `k = L / axis_m` (route-details mesure le long des
  points qu'on lui a donnés, ceux de la polyligne : `k ≈ 1` [supposé]) ;
- `unmatched_m` = longueur de leur intersection avec les parties gardées : `[0, from_m] ∪ [to_m, L]` (`shorten`, `lengthen`),
  `[0, from_m]` (`move_end`, `truncate`) ;
- document sans `fm`/`tm` : `unmatched_trv_m` entier, **borne haute**, et la description du contrat le dit.

Les trous propres à `Walk::from_pieces` (deux chaînes comptées qui ne se touchent pas) n'y sont pas : ils sont courts [supposé].

### 15.5 Échecs : statut et raison

Les raisons nouvelles s'ajoutent à `GenerationReason` (`problem.rs`), comme `no_route` à l'API 1.15 [lu] : **onze valeurs**
`change_too_small`, `change_too_large`, `route_off_coverage`, `route_not_matched`, `order_anomaly`, `route_mismatch`, `no_seam`,
`junction_cut`, `no_economy`, `no_window`, `off_target`. Les autres sont reprises.

**Avant la file** (aucun calcul de moteur, aucune place prise) :

| Cas | HTTP | Code | `details` |
|---|---|---|---|
| champ invalide (§15.1) | 400 | `invalid_input` | pointeurs |
| corps > 4 Mio ; plus de 50 000 points ; `L` > `distance_m_max` | 413 | `route_too_large` | `limit` : `body_bytes`, `points`, `length_m` |
| générateur éteint, ou pas d'index par cellule | 503 | `data_unavailable` | `source: graph`, `Retry-After` |
| `move_end.point` hors du polygone | 422 | `out_of_coverage` | `coverage: graph` |
| un point de la polyligne hors du polygone | 422 | `generation_failed` | `reason: route_off_coverage` |
| `resize` : `|D′ − L| < min_change_m` (1 000 m) | 422 | `generation_failed` | `change_too_small` |
| `resize` : `|D′ − L| > max_change × L` (30 %) | 422 | `generation_failed` | `change_too_large` (renvoi au générateur, D3) |
| file pleine ; budget passé en attente | 503 ; 504 | `overloaded` ; `timeout` | |

Les bornes de distance sont rejugées par `plan_resize` [lu] : au bord exact, le moteur rend la même raison.
`route_off_coverage` est strict (un seul point dehors suffit) [tranché] : la couverture est la planète depuis 1.14 [lu, CLAUDE.md
de Pédalons], et une coupe au bord fausserait la pose sans le dire.

**Pendant le calcul** :

| Issue | HTTP | Raison | Lecture pour le client |
|---|---|---|---|
| aucune pièce comptée (document vide, tout non apparié) | 422 | `route_not_matched` | parcours non retouchable, régénérer |
| `Pose(Empty)` | 422 | `route_not_matched` | idem |
| `Pose(OrderAnomaly)` — **5,0 % des traces au banc** | 422 | `order_anomaly` | parcours non retouchable (appariement dans le désordre), régénérer |
| `Pose(Drift)` — 0,5 % | 422 | `route_mismatch` | le document ne décrit pas cette polyligne : à recalculer côté backend |
| `Pose(NotOnGraph \| WrongType)` | 500 | `internal` | jamais : jambes tirées du même graphe, trois types |
| `NoSeam` | 422 | `no_seam` | aucune couture possible |
| `EndTooFar` (> 500 m d'une voie) | 422 | `end_too_far` | rapprocher le point d'une voie |
| `NoSeamReached` | 422 | `end_unreachable` | arrivée injoignable depuis le parcours |
| `JunctionCut` | 422 | `junction_cut` | pas de raccord sans virage serré à la couture |
| `ChangeTooSmall` / `ChangeTooLarge` | 422 | `change_too_small` / `change_too_large` | (bord exact, ci-dessus) |
| `NoEconomy` — **cas normal** : raccourcir réussit 57,2 %, 38,9 % sur un aller simple [mesuré] | 422 | `no_economy` | rien à retirer à ce parcours ; plus petit écart, ou déplacer l'arrivée |
| `NoWindow` | 422 | `no_window` | pas de tronçon à l'échelle de l'écart |
| `NoProposal { .. }` | 422 | `no_candidate` | |
| `AllRefused { .. }` ; ou toutes les propositions refusées par le contrôleur indépendant (§15.6) | 422 | `checker_rejected` | (la description de la valeur s'élargit à la retouche) |
| `OffTarget { .. }` | 422 | `off_target` | propositions hors de ±3 % |
| `Timeout` (échéance du moteur, 1,8 s) | 422 | `budget` | comme `NoRoute(Budget)` d'une génération |
| `Stopped` (client parti, budget du service) | 504 | `timeout` | |
| `WindowEscape` une seconde fois | 500 | `internal` | compté `ord_generator_edit_window_escapes_total` |
| `NoTeamData`, `TooLong` | 500 | `internal` | jamais : mode d'équipe seulement avec l'entrée ; `L` borné avant |
| mémoire de l'équipe : `Overloaded` / `Stopped` / `Failed` / `State(_)` | 503 / 504 / 500 / — | | `State(_)` : découverte, `overlay: null` (comme l'itinéraire) |

`problem_of(RetouchFailure) -> Problem` est un `match` exhaustif, sans `_` : un échec ajouté au moteur casse la compilation du
service [tranché]. Le texte `detail` est en anglais, sans valeur reçue (« no edit could be made for this route »).

### 15.6 Calcul, sur le fil bloquant

1. **Équipe** : `team_entry` comme `compute_route` [lu] ; `Applied` = `cursor::apply_route(h)` avec l'entrée, `cursor::discovery()`
   sans ; `RetouchRequest { ride_type, mode, familiarity, off_team_cost_q8, seed }`.
2. **Appariement et pose** (§15.3).
3. **Fenêtre** : `plan_move_end(&posed, end, &tune, window_radius_step_m, extra_m)` ou
   `plan_resize(&posed, D′, &rreq, &tune, window_radius_step_m, extra_m)?.window()` ; `self.windows.acquire(…)` avec le `TeamRef`
   (surcouche et goût) ; `take_workspace` / `give_workspace`.
4. **Moteur** : `move_end_in` ou `resize_in`, `stop` = drapeau du client ou échéance. `Err(Box<Refused>)` : la `failure` donne le
   problème (§15.5), les `diagnostics` vont aux métriques. `WindowEscape` : un second essai avec
   `extra_m = snap_lmax_m + 10 000`, comme `resize` et `move_end` [lu] et l'itinéraire.
5. **Contrôleur indépendant** sur le **tronçon neuf seul**, comme le C1 du banc [lu, `retouche/engine.rs`] et comme une
   génération contrôle chacun de ses parcours : `Checker::with_tables(base, overlay, check_config, graph_index, check)`, requête
   `Request { start: nœud de la couture i, end: Some(nœud j | point demandé), distance_m: section, ..}` avec `section` =
   `(to_m − from_m) + (D′ − L)` pour `resize` et `new.length_m` pour `move_end` — les définitions du banc, pour que les taux
   mesurés s'appliquent. Refusée : retirée, `dropped += 1`, `ord_generator_edit_routes_rejected_total{violation}`. `truncate`
   n'a pas de tronçon, rien à contrôler. Toutes retirées : `checker_rejected`. Les C2 et C3 du banc restent au banc : le moteur
   les tient déjà (`check_spliced_in`, masque) [lu, §8 et §3.2].
6. **Rendu** (§15.2, §15.4).

**Ce que le service ne règle pas** : `RetouchTune::default()` et `PoseTune::default()` tels quels (D3 à 30 %, accroche à 500 m,
±3 %, échéances 1 s / 1,8 s) [tranché] ; aucune clé de configuration pour eux, le banc reste le lieu où on les change.

### 15.7 File, budget, fenêtres

- **Même file et même budget que la génération** : `Ready::slot` (`gen_slots`, `max_queued` = 8), échéance
  `cfg.budget_ms` (30 s, attente comprise). Pas de file à part comme l'itinéraire : une retouche coûte comme une petite
  génération (P90 de `resize` **1,1 s**, `move_end` 139 ms [mesuré, appel du moteur seul]) plus l'appariement [non mesuré].
- **Fenêtres** : le cache partagé (`WindowManager`), une fenêtre par opération (§9.2). Les fenêtres de `plan_resize` sont
  l'union des zones de recherche : pour un long parcours elles peuvent être grosses ; le budget de mémoire du gestionnaire les
  refuse alors en 503 `overloaded` [supposé : jamais vu au banc, dont le cache n'avait pas de plafond].
- **Index d'appariement** : 64 Mo au plus par requête en calcul, hors du budget des fenêtres (comme l'index d'un dépôt [lu]) ;
  deux calculs simultanés au plus (`max_concurrent`).

### 15.8 `capabilities.generator.edit`

Présent quand le générateur sert **et** que l'index par cellule existe ; absent sinon (un service plus ancien ne l'a pas non
plus : ne pas appeler).

```rust
pub struct EditCapabilities {
    pub operations: Vec<&'static str>, // ["resize", "move_end"]
    pub ride_types: Vec<&'static str>, // ["road", "gravel", "mtb"]
    pub change_share_max: f64,         // 0.30 : |D′ − L| / L au plus (sinon change_too_large)
    pub change_m_min: f64,             // 1000 : |D′ − L| au moins (sinon change_too_small)
    pub tolerance: f64,                // 0.03 : |distance − D′| / D′ d'une proposition rendue
    pub end_snap_m: f64,               // 500 : le point de move_end à une voie du type (sinon end_too_far)
    pub proposals_max: u32,            // 3
    pub points_max: usize,             // 50 000
    pub request_body_bytes: usize,     // 4 Mio
}
```

Les valeurs viennent de `RetouchTune::default()` et des constantes du service, jamais recopiées à la main. La longueur maximale
du parcours et de `D′` est `generator.limits.distance_m_max`, déjà publiée.

### 15.9 Ajouts au moteur : aucun

Relu dans l'arbre de travail du lot 7a : tout ce que le service rend se lit dans l'API publique d'`ord-loops`.

| Le service rend | Source [lu] |
|---|---|
| `changed.from_m`, `to_m` | `Retouched::from_s_m`, `to_s_m` |
| `changed.new_m`, mesures du tronçon | `Retouched::new` (`Route`), rapport du `Checker` |
| `changed.points`, `unpaved` | `PosedRoute::line_between`, `route_geometry_spans` |
| `kept_m`, `error_share`, `abandoned_m`, `distance_m` | `Retouched::kept_m`, `err`, `abandon_m`, `length_m` |
| `original_m` | `PosedRoute::length_m` (et la même formule avant la file) |
| nœuds de couture pour le contrôleur | `PosedRoute::seams()[k].node`, `engine.base().node_pos` |
| `unmatched_m` | le document (§15.4), pas le moteur |
| bornes des capacités | `RetouchTune::default()` |

`unmatched_m` au mètre près sur la **marche** demanderait `PosedRoute::holes() -> &[(f64, f64)]` (abscisses d'origine des
trous, une projection de plus dans `pose`) : **non retenu**, la lecture par le document suffit. Le retrait de
`probe_graft::move_end_in` et de `Workspace::set_keep_out` (§13) n'est pas fait au 7b : `--impl lot0` du banc s'en sert encore.

### 15.10 Journal et métriques

Une ligne `route edit` par requête : `operation`, `ride_type`, `mode`, `distance_band` (bande de `L`, celle de la génération),
`change_band` (`resize` : `-30..-10%`, `-10..0%`, `0..10%`, `10..30%` ; `move_end` : vide), `outcome`, `reason`, `routes`,
`dropped`, `seams`, `holes` (comptes de `PoseStats`), `match_ms`, `pose_ms`, `engine_ms`, `duration_ms`, `ns8`. **Jamais** de
coordonnée, de polyligne, de document, de distance exacte, de clé ni d'équipe.

| Métrique | Étiquettes |
|---|---|
| `ord_generator_edit_requests_total` | `operation`, `outcome`, `reason` |
| `ord_generator_edit_duration_seconds` (histogramme) | `operation`, `ride_type` |
| `ord_generator_edit_stage_seconds` (histogramme) | `stage` = `match`, `pose`, `engine`, `check` |
| `ord_generator_edit_routes_total` | `kind` = `shorten`, `lengthen`, `move_end_best`, `move_end_keep_all`, `move_end_nearest`, `truncate` |
| `ord_generator_edit_routes_rejected_total` | `violation` |
| `ord_generator_edit_window_escapes_total` | — |

Une retouche n'incrémente pas `ord_generator_requests_total` : les tableaux des générations ne bougent pas.

### 15.11 Fichiers

| Fichier | Quoi |
|---|---|
| `crates/ord-server/src/generator/edit.rs` (nouveau) | DTO (§15.1, §15.2), `EditCapabilities`, validation, `Ready::edit` (file, journal, métriques), calcul (§15.6), `problem_of`, rendu, handler `post_edit` et son `#[utoipa::path]` (tag `generator`, `operationId` `postGeneratorRoutesEdit`), tests unitaires |
| `crates/ord-server/src/generator/mod.rs` | `pub mod edit;`, `EDIT_BODY_MAX`, champ `edit: Option<EditCapabilities>` de `GeneratorCapabilities` et son remplissage |
| `crates/ord-server/src/problem.rs` | les onze `GenerationReason`, la description de `checker_rejected` élargie |
| `crates/ord-server/src/app.rs`, `openapi.rs` | la route `/v1/generator/routes/edit` (POST), le chemin et les schémas |
| `crates/ord-server/src/config.rs` | `generator.edit_index_cache_mb` (64, > 0 validé) |
| `crates/ord-server/src/lib.rs` | `API_VERSION` = `"1.17"` |
| `crates/ord-graph-match/src/posed.rs` (nouveau), `lib.rs`, `doc.rs` | `DocWalk`, `doc_walk`, `posed_parts` ; `Document` lit `len` |
| `contract/openapi.json` | régénéré par `cargo run -p ord-server -- openapi` |
| `Cargo.toml`, `Cargo.lock` | 1.18.0 |
| `CHANGELOG.md` | `## 1.18.0 (API 1.17)` : ajouté (endpoint, capacités, raisons), réserves (pose sur la polyligne non mesurée, raccourcir 57,2 % / 38,9 %, 5 % d'anomalies d'ordre) |
| `crates/ord-server/tests/edit.rs`, `edit_sentinel.rs` (nouveaux) | §15.12 |
| `docs/design/technique/ord-retouche-lot1.md` | cette section ; le relevé du 7b s'y ajoutera |

**Ne changent pas** : `ord-loops` (aucune ligne), `routes.rs`, `router.rs`, `generate.rs`, `deposit.rs` (au-delà de l'emploi de
`Document::len`), les tests existants (sauf `contract.rs`, qui suit le fichier régénéré).

### 15.12 Tests

Grille synthétique des tests du générateur (`tests/common/generator.rs` : `write_files`, `node`, `document`), **au moins 12 × 12**
(boucle de ~13 km, pour que coutures, marges de 100 m, `min_segment_m` et `ends_share` laissent des fenêtres) [supposé, à
ajuster]. Une aide de test `edit_body(path, op)` fabrique la polyligne d'un chemin de carrefours et son document (une arête par
tronçon de 3 rues, `fm`/`tm` cumulés, `len`). Aucune coordonnée réelle.

`tests/edit.rs` :

1. **Capacités** : `generator.edit` présent, valeurs de `RetouchTune::default()` ; absent sans graphe.
2. **`move_end` près de l'arrivée** : 200, 1 à 3 routes, `kind = move_end`, `variant` rempli, la polyligne commence par celle
   d'origine **au point près** jusqu'à `changed.points[0]`, `changed.to_m = original_m`, `end_snap_m` ≤ 500.
3. **`move_end` sur le parcours** : `truncate`, `new_m = 0`, `points[0] = points[1] =` dernier indice.
4. **Allonger de 20 % une boucle** : `|distance_m − D′| / D′ ≤ 0,03`, polyligne qui commence et finit par l'origine,
   `points` encadrent le tronçon neuf, `kept_share` < 1.
5. **Raccourcir un aller simple droit** : 422 `no_economy` (le cas normal a son nom).
6. **Bornes avant la file** : +40 % → `change_too_large`, +500 m → `change_too_small` ; aucune étape `engine` dans les
   métriques.
7. **400 par pointeur** : polyligne illisible (`/polyline`), un seul point, `operation` vide ou à deux clés (`/operation`),
   `distance_m` négatif ou infini, point invalide (`/operation/move_end/point/lat`), `network_edges` non objet, `cursor` hors
   bornes, clé inconnue (`unknown_keys`), clé sans équipe (`/headers/…`).
8. **413** : corps au-delà de 4 Mio, plus de 50 000 points, `L` au-delà de `distance_m_max`.
9. **Couverture** : point d'arrivée hors du polygone → `out_of_coverage` ; polyligne qui sort → `route_off_coverage`.
10. **Pose** : document permuté → `order_anomaly` ; document d'un autre chemin → `route_mismatch` ; `edges: []` →
    `route_not_matched`.
11. **Équipe** : surcouche déposée → `mode` du curseur, `overlay` rempli, `changed.team_m` > 0 sur un tronçon pris sur les
    routes de l'équipe ; équipe sans surcouche → `discovery`, `overlay: null`.
12. **`unmatched_m`** : une arête du document à géométrie illisible dans la partie gardée → `unmatched_m` ≈ sa longueur ; la
    même dans la partie remplacée → 0 ; document sans `fm` → borne haute.
13. **Déterminisme** : le même corps deux fois → la même réponse, octet pour octet.
14. **File partagée** : les places de génération prises (crochet de test existant s'il y en a un, sinon `max_concurrent` 1 et une
    génération longue) → la retouche attend ou rend 503 `overloaded`.
15. **Contrôleur** : avec `hooks.corrupt_routes` (crochet de la génération, s'il s'applique aux routes du moteur ; sinon un
    crochet `corrupt_edits` dans `Hooks`) → `dropped` > 0, puis `checker_rejected`.

`tests/edit_sentinel.rs` (modèle `router_sentinel.rs`) : une retouche réussie et une refusée, avec équipe ; ni chiffre de
coordonnée (cinq décimales des carrefours), ni fragment de polyligne, ni clé, ni équipe dans les journaux et les métriques.

Tests unitaires (`edit.rs`) : `problem_of` pour **chaque** `RetouchFailure` (et `PoseError`) ; intersection des étendues de
`unmatched_m` ; `change_band` ; assemblage de la géométrie (point répété retiré, indices décalés). `ord-graph-match/posed.rs` :
`doc_walk` sur un graphe fait à la main (une arête illisible fait un trou et une étendue ; `fm` absent rend `None`), et
`posed_parts`.

Avant de rendre la main : `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
puis `cargo run -p ord-server -- openapi` et le test de contrat.

### 15.13 Ce que ce lot laisse ouvert

- **La pose sur la polyligne d'origine** n'est pas mesurée (§15.3) : la recette lira la part de `order_anomaly` et
  `route_mismatch`.
- **Le temps de l'appariement** n'est pas mesuré : `stage="match"` à la recette ; cache des poses seulement si besoin.
- **Raccourcir** reste à 57,2 % (38,9 % sur un aller simple) : le client doit présenter `no_economy` comme une réponse, pas
  comme une panne ; les pistes du §14.1 (lecture à 3 % de la section, plus d'arbres) sont du moteur, pas du service.
- **La mise en service** dans la pile partagée (entrée `OPS` côté Pédalons) et le lot 4 du plan (backend).

---

## 16. Relevé du 7b : ce qui a été codé, et les écarts à §15 (24 novembre 2026)

Codé, non commité : 1.18.0, API 1.17. `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` et `cargo test
--workspace --no-fail-fast` verts. Fichiers : §15.11, plus `tests/common/generator.rs` (aides `path_document`, `edit_body`, `edit`,
`square`, `row`). `ord-loops` n'a pas changé d'une ligne.

**Écarts** (le plus simple qui respecte les invariants) :

- **`operation` est une structure à deux clés facultatives** (`resize`, `move_end`), non un `enum` externe de serde : l'extracteur
  strict laisse tomber les segments de variante dans ses pointeurs (`/operation/distance_m` au lieu de
  `/operation/resize/distance_m`). Zéro ou deux clés : 400 au pointeur `/operation`, par la validation du service. Même JSON.
- **`doc_walk` prend le `len` du document** (`doc_len: Option<f64>`) et `Document::len_m()` le donne : la signature du §15.3 n'avait
  pas de quoi le passer.
- **`unmatched_m` sans positions** (une arête du document sans `fm` ou `tm`) : `min(somme des trv non comptés, kept_m)`, non la somme
  entière du §15.4. Toujours une borne haute, jamais plus que ce qui est gardé.
- **Aucun crochet nouveau** : `corrupt_routes` et `corrupt_first_route` (ceux de la génération) altèrent le tronçon neuf avant le
  contrôleur ; les deux sont testés (`dropped`, `checker_rejected`).
- **Une métrique de plus** : `ord_generator_edit_spliced_refused_total{violation}` (les `diagnostics` du moteur, §15.6 point 4). La
  bande de changement a deux étiquettes de plus, `<-30%` et `>30%` (jamais journalisées en pratique : jugées avant la file).
- **`end_snap_m` aussi pour `truncate`** (la distance du point demandé au dernier point rendu).
- **Ordre des refus d'entrée** : tous les 400 d'abord (rendus ensemble), puis les 413 `points` et `length_m`, puis, avant la file,
  422 (`out_of_coverage`, `route_off_coverage`, `change_too_small`, `change_too_large`). Le 413 `body_bytes` est celui de
  `read_json`, avant tout.
- **Deux tests existants suivent la version** : `tests/router.rs` et `tests/generator.rs` lisaient `api_version` / `info.version` =
  `"1.16"` (trois chaînes, pas une ligne de logique).

**Vérifié par les tests** (grille de 21 × 21 rues de 300 m ; un carré de 6 rues, un parcours à lobe, un aller simple en L) : allonger
de 20 % à 3 % près, polyligne qui commence et finit par l'origine, `points` qui encadrent le tronçon neuf ; raccourcir un lobe
exactement de ce lobe (`from_m`, `to_m`, `new_m` à 5 m) ; `no_economy` sur une ligne droite ; `move_end` et `truncate` (point **au
milieu d'une rue** : sur un carrefour, le moteur rend un `move_end` de 1 m) ; bornes avant la file (file pleine, 422 quand même) ;
tous les 400 par pointeur, JSON strict, en-têtes ; 413 (corps, points, longueur) ; couverture (`out_of_coverage`,
`route_off_coverage`, `end_too_far`) ; `route_not_matched` (`edges: []`), `route_mismatch` (arêtes inversées ou en double),
`order_anomaly` (le document d'un autre carré de même longueur), `no_seam` (une rue) ; `checker_rejected` et `dropped` ; équipe
(mode, `overlay`, `team_m` du tronçon, équipe sans surcouche) ; `unmatched_m` (trou gardé compté, trou remplacé non, borne haute sans
`fm`) ; déterminisme octet pour octet ; capacités. `edit_sentinel.rs` : rien de la requête (sommets, point, distance visée,
fragments de la polyligne et du document, clés, équipe) dans les journaux ni les métriques.

**Vérifié seulement par le tableau unitaire de `problem_of`** (statut et raison de chaque `RetouchFailure` et `PoseError`, sans
requête) : `junction_cut`, `no_window`, `no_candidate`, `off_target`, `end_unreachable`, `budget`, `timeout` (`Stopped`), les 500.
**Non vérifié** : le second essai sur `WindowEscape` (jamais atteint sur la grille), la retouche éteinte faute d'index par cellule
(plus de 100 000 arêtes larges), la mémoire des fenêtres d'un long parcours, le temps d'appariement (`stage="match"`), la pose sur
la polyligne reçue d'un vrai GPX (§15.3) : tout cela est à la recette.

