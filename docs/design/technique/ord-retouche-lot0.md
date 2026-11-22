# Retouche d'un parcours — lot 0 : une mesure, conception

> Statut : **conçu le 22 novembre 2026, rien de codé, rien de mesuré.** Depuis `develop` à `c6113eb` (1.14.0). Plan
> appelant : Pédalons `docs/plans/2026-11-22-retouche-parcours.md` (ledger `API-212`), §3 à §6, §9 (décisions D1 à D5),
> §10 (lots et critères), §11 (risques). Ce lot est le **lot 0** de ce plan : une mesure, **pas une fonctionnalité**.
> Rien dans `ord-server`, rien dans le contrat, aucune API publique stable : le code vit dans le banc
> (`tools/ord-loops-bench`) et, pour le seul crochet qui touche au moteur, derrière la fonctionnalité `probe` de
> `ord-loops`, comme la sonde du 4c ([`ord-loops-lot4c-sonde.md`](ord-loops-lot4c-sonde.md)).
>
> Marques : **[lu]** relu dans le code de `c6113eb` pour cette note ; **[mesuré]** renvoie à un relevé existant, cité ;
> **[estimé]** est un ordre de grandeur à confirmer à l'essai ; **[proposé]** est un réglage que le relevé confirme ou
> change. Ni départ, ni coordonnée, ni identifiant d'équipe ou de trace ici ; équipes lettrées A à I comme aux relevés
> précédents.

## 0. En bref

- **Deux livraisons.** **A** mesure la continuité de l'appariement et l'emplacement des coutures sur **toutes** les
  traces de l'export, sans toucher au moteur. **B** prototype les deux opérations (`move_end`, `resize`), les contrôle
  et les chronomètre. Chacune a son commit, son essai et sa campagne.
- **Le seul changement du moteur est un masque d'arêtes interdites pour toute la requête**, posé par le banc. Il est
  lu par `Weighter::chain`, la porte unique des deux arbres, des A* et de l'accroche, et il est compilé **seulement**
  avec `probe`. `Ban` n'est pas touché. Sans masque, ou sans `probe`, une génération rend les mêmes parcours au bit
  près (§5.1).
- **`resize` passe par l'API publique** : `generate_with_figures_in` avec `start` = couture `i`, `end` = couture `j`,
  `distance_m` = `ℓ + Δ`, et le masque des arêtes gardées. C'est la lentille d'aller simple de production, sans copie.
  **`move_end` demande une fonction de sonde**, `ord_loops::probe_graft::move_end_in`, parce qu'aucune API publique ne
  rend l'arbre retour (§6).
- **Le recollé est jugé par le contrôleur indépendant**, d'abord sur le tronçon neuf seul (C1), puis par morceaux
  continus autour des coutures, **par différence** avec les mêmes morceaux de l'original (C2), parce que les traces
  réelles violent déjà des règles (relevé du lot 4, §6 : `DEAD_END` et `NOT_ADMITTED` sur les trois marches
  continues). La réutilisation des arêtes gardées est comptée par le banc (C3).
- **Le critère « une couture à moins de 1 km de tout point »** se lit sur l'abscisse **de la trace d'origine**. Les
  coutures y sont posées par une projection monotone de leurs nœuds sur la polyligne du document. Cette projection
  vérifie aussi l'ordre des jambes, et c'est elle qui découpera la géométrie d'origine « au point près » au lot 1.
- **Durée [estimé]** : environ 5 min pour la continuité, environ 20 min pour une campagne complète des prototypes sur
  16 fils, avec une marge de 2× sous l'heure (§7.4).

---

## 1. Ce que le code fait aujourd'hui (relu)

### 1.1 De l'export à la marche

- **Le document `network_edges`** d'une trace est une liste d'arêtes de route-details **dans l'ordre de la trace**. Chaque
  arête a sa géométrie `g` (polyligne 6), `trv` (mètres parcourus), `wk`, `fn`/`tn`, `fo`/`to` [lu, `ord-graph-match/src/doc.rs`].
  Au lot 1, `to − fo` ne s'écarte de `trv` sur aucune des 2 171 054 arêtes [mesuré, `ord-graph-lot1-mesures.md`]. Une
  entrée est donc **un passage monotone** : un demi-tour au milieu d'une arête fait deux entrées.
- **`export::ingest`** garde, pour chaque trace, la suite de ses `TeamEdge` dans l'ordre du document. La
  déduplication porte sur les **clés** `(g, trv)`, pas sur les occurrences : deux passages sur la même arête restent
  deux entrées [lu].
- **`export::track_match`** parcourt les arêtes dans l'ordre. `Counted` ajoute les morceaux de sa chaîne (dans l'ordre
  de `g`). `Outside` et `Fail` n'ajoutent rien, ce qui fait un trou. La fonction compte aussi `total_m`, `matched_m` et
  `outside_m` en mètres du document (`trv`, avec un repli sur `glen`) [lu].
- **`Walk::from_pieces`** (`ord-graph-match/src/walk.rs`) [lu] :
  - il jette les morceaux de moins de 1 m ;
  - il fusionne deux morceaux contigus d'une même arête **et d'un même sens** (à 2 m près) ;
  - il ramène à leur nœud les bouts à 2 m ou moins d'un nœud ;
  - il raccorde à leur nœud commun deux appariements voisins qui s'arrêtent à 25 m ou moins de lui (`bridge_at_nodes`,
    depuis le 3b) ;
  - il classe enfin les jonctions : `Node(n)` (même nœud de base), `Touch` (5 m ou moins), `Hole(d)` (`d` est la
    **corde plane** entre les deux bouts, pas la longueur non appariée le long de la trace).
- **`judge_pieces`** (`profile/collect.rs`) fait la retenue du banc, dans cet ordre : trace exclue ; illisible ; type
  au-delà de MTB ; part appariée < `min_matched` (0,95) ; suite vide ; plus long trou > `max_hole_m` (1 000 m, sur la
  corde) ; D < 5 km ; pas une boucle (écart départ–arrivée ≥ 5 % de la polyligne) ; D > `max_km` [lu].
- **Au lot 4**, sur 3 377 lignes, la retenue a écarté 59 traces pour la part appariée, 126 pour un trou de plus de
  1 km et 331 qui n'étaient pas des boucles. Il restait 2 855 boucles, dont **3 seulement continues par nœuds**
  [mesuré, `ord-loops-lot4-mesures.md` §2.1 et §6]. C'était **avant** `bridge_at_nodes` : aucun relevé depuis ne compte
  les marches continues.

### 1.2 Le moteur

- **`Weighter::chain(c)`** [lu, `search.rs`] est lu par :
  - `tree_forward` et `tree_backward` (une arête `INF` n'est pas relâchée) ;
  - `astar` et `astar_p2p` ;
  - `reverse::astar_back` ;
  - le filtre `ok` de l'accroche (`e.snap`).

  **`Ban`** n'est lu que par les A* : les arbres l'ignorent. On ne peut donc pas obtenir une interdiction qui dure en
  posant les arêtes gardées dans le tampon `ban` à chaque génération, comme le fait `probe_triangle_in` pour une
  jambe : l'arbre aller partirait encore le long du parcours gardé.
- **L'aller simple de `generate_inner`** se déroule ainsi [lu] :
  1. il accroche `start` et `end` sur le graphe ;
  2. il construit les deux arbres sur l'ellipse de foyers départ et arrivée, de somme D ;
  3. il vérifie la faisabilité : `DistanceTooShort` si D < 1,05 × le plus court chemin, `NetworkTooSmall` si le réseau
     atteint fait moins de 0,9 D ;
  4. la lentille cherche `n` qui minimise `|T1m(n) + ρ·T2m(n) − D|` : l'aller vient de l'arbre, le retour d'un A* sur le
     graphe privé de l'aller (queue tolérée sur 0,15 D) ;
  5. il garde les candidats à ±10 % de D (`Params::tolerance`), contrôle chacun (`verify.rs`) et en choisit au plus
     trois.

  Les figures organiques ne valent que pour une boucle : un aller simple suit toujours le chemin `Classic`.
- **Les ids de chaîne et de nœud d'une `Window` sont locaux.** Une chaîne porte ses arêtes de base, ids globaux, avec
  le bit 31 pour le sens (`ViewData::links`). La vue sur disque sait dire si un nœud de base est un nœud de vue
  (`GraphFile::view_node_of_base`) et donne les arêtes de base de chaque arête de vue (`view_chain`) [lu,
  `ord-graph/src/view.rs`].
- **`ord_loops_check::Checker::check(req, route)`** rend un **ensemble** de violations, sans position [lu]. Il juge une
  `Route` continue, qui se fabrique depuis une marche continue (`Walk::to_route`).

---

## 2. Livraison A : continuité et coutures

Aucune ligne du moteur ni d'`ord-graph-match` ne change : tout est dans le banc.

### 2.1 Population

- **Toutes les lignes de l'export, sans filtre de retenue.** On compte d'abord, sans les mesurer, les traces illisibles
  et les traces vides (aucun morceau apparié). Les traces **exclues du générateur** sont mesurées et marquées : D1 dit
  « tout parcours d'équipe enregistré », exclusion comprise.
- **Strates** : type (`ROAD`, `GRAVEL`, `MTB`, et `MIXED` à part, qui n'a pas de type de génération) × forme. Une trace
  est une **boucle** si l'écart entre son premier et son dernier point est inférieur à 5 % de sa polyligne (la règle
  de `judge_pieces`) ; sinon c'est un **aller simple**. Chaque agrégat est donné par strate, par équipe pour A et B (les
  autres réunies) et au total.
- Les traces retenues par le banc actuel (`judge_pieces` à ses défauts, `max_km` absent), et pour une boucle ou un
  aller simple la même règle sans le test de boucle, sont marquées `retained`. **Ce que les filtres écartent** se lit
  alors directement : compte et km par cause (`Matched`, `Hole`, `Short`), et les mêmes mesures de couture sur les
  seules traces écartées.

### 2.2 Mesures de continuité, par trace

La marche est celle d'aujourd'hui : `track_match`, puis `Walk::from_pieces`, puis `Walk::trace`.

| Champ | Définition |
|---|---|
| `doc_m` | `total_m` (Σ `trv`) |
| `matched_share` | `matched_m / total_m` (la notion du dépôt) |
| `unmatched_doc_m` | `total_m − matched_m − outside_m` ; `outside_m` à part |
| `legs`, `runs` | nombre de jambes ; nombre de **plages continues** (suites maximales de jambes jointes par `Node`) |
| `holes`, `touches` | nombre de jonctions `Hole` et `Touch` |
| `hole_chord_m` | Σ et maximum des cordes des trous (`Trace::holes_m`, `max_hole_m`) |
| `holes_per_100km` | `holes / doc_m × 1e5` |
| `run_m` | longueur graphe de chaque plage (P10, P50, P90 et maximum par trace ; la distribution des plages entre aussi dans l'agrégat) |
| `continuous` | `Walk::is_continuous()` |

Les quantiles sont ceux des autres relevés (type 7, `likeness::shape::quantile_sorted`), sur les traces et, pour les
trous et les plages, sur les objets eux-mêmes.

### 2.3 Couture valide

Une jonction `Join::Node(n)` entre les jambes `x` et `y` est une **couture valide** si :

1. **`n` est un nœud de la vue du type** (`view_node_of_base(n).is_some()` ; ROAD → `road`, GRAVEL et MTB →
   `gravel`, MIXED → `road`, pour la mesure seulement). Seul un nœud de vue est un bout de chaîne, donc un point où une
   recherche peut partir ou arriver exactement ;
2. **les arêtes de `x` et `y` sont dans cette vue.** Un masque de bits des arêtes de base présentes est bâti une fois en
   parcourant `view_chain` de toutes les arêtes de vue (environ 8 Mo [estimé]). Sans ces arêtes, le virage à la couture
   ne se jugerait pas sur des chaînes que le moteur connaît ;
3. **ce n'est pas un demi-tour** : `x.edge ≠ y.edge`, et l'angle entre la sortie de `x` et l'entrée de `y` est d'au
   plus 150° (le `max_turn_deg` du contrôleur), mesuré sur les géométries d'arête comme `measure.rs` mesure les
   virages ;
4. **`n` est dans une plage continue à au moins `seam_margin_m` de chacun de ses deux bouts**, mesuré sur `leg_m`.
   Par défaut **200 m** [proposé, plan §3] ; option `--seam-margin-m`. Pour `move_end`, seule la marge **avant** compte
   (la suite est abandonnée) : le relevé donne les deux comptes.

`Touch` coupe une plage, au sens strict. Une variante `--touch-continues` la traite comme continue, pour voir si le choix
pèse ; le résultat de référence reste le strict, parce que le recollé serait discontinu en ce point.

### 2.4 Abscisse le long de la trace d'origine : projection monotone

Le critère parle de distance **le long de la trace**. Sur la marche, un trou ne vaut que sa corde, et une corde
sous-estime un passage non apparié sinueux. On mesure donc sur la **polyligne du document** : les géométries `g`
décodées et mises bout à bout dans l'ordre, chaque premier point répété retiré (comme `raw_legacy`), avec l'abscisse
haversine `S(x)` et la longueur `L`.

Chaque couture valide `k` (dans l'ordre de la marche, à l'abscisse de marche `a_k`, tirée de `Trace::join_arc`) reçoit
une abscisse de trace `s_k` :

- la recherche part de `s_{k−1}` (0 pour la première) et ne lit que la fenêtre `[s_{k−1} + 0,8 (a_k − a_{k−1}) − 50 m,
  s_{k−1} + 1,5 (a_k − a_{k−1}) + 200 m]` ;
- dans cette fenêtre, on prend **le premier minimum local** de la distance du nœud à la polyligne qui tombe à
  `proj_tol_m` (**15 m** [proposé]) ou moins ;
- s'il n'y en a pas, la couture est une **anomalie d'ordre**. Elle est comptée, puis projetée sur le plus proche point
  global, et `s_k` reste croissante (`max(s_{k−1}, …)`).

**Complément du codage (livraison A).** Un trou ne vaut que sa corde sur la marche, alors que le passage non apparié qu'il
recouvre est plus long : la borne haute de la fenêtre gagne `3 ×` la somme des cordes des trous entre la couture
précédente et celle-ci. Sans lui, toute couture qui suit un trou de quelques centaines de mètres tombait en « anomalie
d'ordre » à l'essai.

Le départ de la recherche avance avec la marche, si bien qu'un aller-retour (le même nœud passé deux fois), une boucle
en 8 ou une trace qui se recoupe trouvent chacun **le bon passage**, au lieu du premier venu.

**Indicateurs d'ordre par trace** :

- `order_anomalies` : nombre de coutures non trouvées dans leur fenêtre ;
- `proj_dist_m` : P50 et maximum de la distance de projection ;
- `drift` : `|Σ leg_m + unmatched_doc_m − doc_m| / doc_m`. Au-delà de 2 %, des jambes sont en double ou manquent.

**Mesures de couture**, avec les coutures `s_1 < … < s_m` sur `[0, L]` :

- **`max_dist_to_seam_m`** = `max(s_1, L − s_m, max_k (s_{k+1} − s_k) / 2)` : la plus grande distance d'un point de la
  trace à la couture valide la plus proche. Elle est infinie sans couture (`m = 0`). **C'est le critère du plan.**
- `max_dist_inner_m` : la même grandeur restreinte à `[0,05 L ; 0,95 L]` (là où `resize` coud), et
  `last_seam_from_end_m = L − s_m` (ce qui compte pour `move_end`) ;
- `max_gap_m = max_k (s_{k+1} − s_k)` et `seams_per_km`.

Pour savoir ce que coûte la corde, la même mesure est aussi faite sur l'abscisse de marche (`a_k`, `Trace::l_poly`) :
le relevé donne l'écart.

### 2.5 L'ordre des jambes est-il fiable pour une greffe ?

**Par construction, oui** [lu], pour les raisons du §1.1 : document ordonné, passages monotones, occurrences gardées,
morceaux dans l'ordre de `g`. Restent quatre risques, chacun couvert par un test :

| Cas | Risque | Vérification |
|---|---|---|
| Aller-retour sur une même arête | deux jambes de sens opposés ; la fusion ne joint que des morceaux de même sens, `bridge_at_nodes` saute `x.edge == y.edge` | test de fixture : aller-retour sur une impasse → deux jambes, jonction `Node` au bout ; demi-tour à mi-arête → `Touch` |
| Boucle en 8, trace qui se recoupe | même nœud à deux positions de la marche | test : un 8 sur la grille → nœud central présent deux fois, dans l'ordre ; projection : deux `s` distinctes et croissantes |
| Tours répétés (circuit) | deux passages de même sens sur la même arête | test : deux tours → aucune fusion entre la fin du tour 1 et le début du tour 2 (bouts éloignés de plus de 2 m) |
| Clé dédupliquée | une géométrie répétée ne doit pas perdre une occurrence | test d'`ingest` sur un CSV de deux lignes, dont une avec la même arête deux fois → deux `TeamEdge` |

Sur les données, le relevé donne `order_anomalies`, `drift` et `proj_dist_m`. **Critère proposé** : moins de 1 % des
traces avec une anomalie. Au-delà, l'ordre n'est pas assez sûr pour couper la géométrie d'origine à une couture, et le
lot 1 doit prévoir le recalage géométrique direct (risque §11 du plan).

### 2.6 Sorties de la livraison A

Elles vont sous `~/.cache/osm-route-details/graph/retouche/` (défaut de `--out`) :

- `<label>-continuity.jsonl` : une ligne par trace lisible. On y trouve la lettre de l'équipe, le rang de la trace
  dans l'ordre de l'export pour son équipe, la strate, `retained` et la cause du refus, puis tous les champs du §2.2 et
  du §2.4. **Ni id, ni empreinte d'id, ni coordonnée** ;
- `<label>-continuity.json` : les agrégats par strate, par équipe et au total (comptes, parts, quantiles P10, P50 et
  P90, maximum), la **part des traces avec `max_dist_to_seam_m` ≤ 1 km** (et ≤ 500 m, ≤ 2 km), et ce que les filtres
  écartent ;
- `<label>-continuity.md` : le résumé de ces agrégats, **seule partie recopiable** dans le relevé du dépôt, avec les
  empreintes xxh3 de l'export et du `.jsonl`.

---

## 3. Le parcours posé (entrée commune de B)

`retouche::posed::Posed` est construit dans le banc depuis une marche et la polyligne du document :

```rust
pub struct Posed {
    pub walk: Walk,                // la marche d'aujourd'hui, inchangée
    pub trace: Trace,              // Walk::trace
    pub poly: Vec<(f64, f64)>,     // polyligne du document (lat, lon), abscisse haversine `s`
    pub s: Vec<f64>,
    pub l_m: f64,                  // L
    pub seams: Vec<Seam>,          // coutures valides, dans l'ordre de la marche
    pub runs: Vec<Range<usize>>,   // plages continues (indices de jambes)
}
pub struct Seam {
    pub join: usize,               // indice de la jonction : entre legs[join] et legs[join + 1]
    pub node: u32,                 // nœud de base
    pub s_m: f64,                  // abscisse de trace (§2.4)
    pub a_m: f64,                  // abscisse de marche
    pub before_m: f64, pub after_m: f64, // marge continue de chaque côté
}
```

Le recollé se représente en deux couches :

- **la couche graphe**, pour les contrôles : préfixe = `legs[..=i.join]`, tronçon neuf = `Walk::from_route(P)`, suffixe =
  `legs[j.join + 1..]`, avec `Join::Node` aux coutures ;
- **la couche géométrie**, pour les longueurs : `L′ = s_i + longueur(P) + (L − s_j)`. `longueur(P)` est celle que
  rend le contrôleur (`Report::length_m`, haversine), comme pour la trace.

Pour `move_end`, le suffixe est vide et `L′ = s_k + longueur(P)`.

---

## 4. Livraison B : `move_end`

### 4.1 Méthode (plan §5)

1. **Masque** = toutes les chaînes de la fenêtre qui portent une arête de la marche, **préfixe et partie abandonnée
   réunis**. Sans masque, la partie abandonnée resterait libre, mais :
   - la suivre dans son sens revient à choisir un `k` plus tard, ce que le critère essaie déjà ;
   - la prendre à rebours depuis `k` est un demi-tour sur le parcours.

   Le masque entier est donc presque exact, et il évite de bâtir un arbre par `k`. Le seul cas perdu est un raccord
   qui quitte le parcours puis le rejoint plus loin pour en remonter un morceau. Il est rare et compté : quand aucun `k`
   n'est atteint, on rejoue une fois avec le masque du seul préfixe de la couture la plus tardive.
2. **Accroche de `E′`** avec le poids sans masque :
   - si la chaîne trouvée est masquée, `E′` est **sur le parcours**. L'issue est `truncated` : `k` = la première
     couture valide après la projection de `E′`, rien d'abandonné hors de la fin, aucun raccord. C'est un succès, compté
     à part ;
   - sinon, `E′` est accroché avec le poids masqué (le filtre `ok` de `snap`). Au-delà de `snap_m`, l'issue est
     `EndTooFar`.
3. **Arbre retour** `tree_backward` depuis `E′`, sur le disque de centre `E′` et de rayon
   `ρ = clamp(1,6 × vol d'oiseau(E, E′) + 3 km ; 5 km ; 40 km)` [proposé] (`Disc::new(E′, E′, 2ρ, true)`). Le critère
   borne le raccord utile : au mieux, on garde tout et on raccorde depuis `E`, soit environ 1,3 à 1,5 × le vol
   d'oiseau. Si aucune couture n'est atteinte et que `ρ < 40 km`, on rejoue une fois avec `2ρ`.
4. **Coût en `k`**, pour chaque couture valide `k` (marge avant ≥ 200 m) dont le nœud de vue est atteint :
   - `f_in` est l'état orienté de la chaîne qui porte la jambe arrivant en `k` (masquée, donc absente de l'arbre) ;
   - on retient l'état sortant `fo ∈ out(k)` de plus petit `t2.get(fo)` tel que `turn_cost(v, forb, f_in, NONE, fo,
     None, None)` soit `Some` et que `retro_ok` passe. C'est le virage du moteur, sans demi-tour et à 150° au plus ;
   - le **raccord** vaut `t2.meters(fo)`.
5. **Choix** : `k*` minimise `(L − s_k) + raccord(k)`, en mètres. À égalité, la couture la plus tardive gagne (on garde
   plus). On donne aussi deux variantes, quand elles diffèrent de `k*` :
   - **garder tout** : la dernière couture atteinte ;
   - **quitter au plus près** : le plus petit raccord.

   Le chemin est `t2.path_to(fo)`, mis en étapes par `steps_of`, avec une accroche fabriquée au bout de chaîne `k` (un
   morceau de départ entier) et l'accroche de `E′` à l'arrivée.

### 4.2 API de sonde

C'est un nouveau module `crates/ord-loops/src/probe_graft.rs`, déclaré `#[cfg(feature = "probe")] pub mod probe_graft;`
dans `lib.rs`. Rien n'y est appelé par la génération.

```rust
/// Chaînes locales d'une fenêtre interdites pour toute une requête (bitset de `n_chains` bits).
#[derive(Clone, Debug)]
pub struct KeepOut { bits: Vec<u64>, n_chains: usize, tiles_xxh3: u64 }
impl KeepOut {
    /// Masque les chaînes de `win` dont au moins un lien est dans `base_edges` (une passe sur `chain_links`).
    pub fn build<B: AsRef<[u8]>>(e: &Engine<B>, win: &Window, base_edges: impl IntoIterator<Item = u32>) -> Self;
    pub fn masked_chains(&self) -> usize;
    /// Mètres de vue masqués : à rapporter, pour voir ce que la chaîne entière ajoute à l'arête gardée.
    pub fn masked_m(&self, win: &Window) -> f64;
}

pub struct SeamIn { pub node: u32, pub in_edge: u32, pub in_reverse: bool, pub abandon_m: f64 }

#[derive(Clone, Copy, Debug)]
pub struct MoveEndParams { pub radius_m: f64, pub max_radius_m: f64, pub min_radius_m: f64 }

pub struct MoveEndPick { pub seam: usize, pub raccord: Route, pub raccord_m: f64, pub abandon_m: f64 }

pub struct MoveEndOut {
    pub best: MoveEndPick,
    pub keep_all: Option<MoveEndPick>,
    pub nearest: Option<MoveEndPick>,
    pub radius_m: f64, pub widened: bool, pub prefix_retry: bool,
    pub t2_settled: u64, pub seams_reached: u32, pub us: f64,
}

pub enum MoveEndFail {
    EndTooFar, EndOnRoute { after_s_m: f64 }, NoSeamReached, WindowEscape, NoTeamData, Stopped,
}

pub fn move_end_in<B: AsRef<[u8]>>(
    e: &Engine<B>, win: &Window, team: Option<TeamCtx<'_>>,
    r: &Request,             // start = E′, end = None ; ride, mode, familiarité, off_team et seed du curseur
    seams: &[SeamIn], keep_out: &KeepOut, ws: &mut Workspace, p: &MoveEndParams,
) -> Result<MoveEndOut, MoveEndFail>;
```

**L'entrée est déjà continue par morceaux.** La sonde ne voit que les coutures, chacune avec son arête d'arrivée, et le
masque. La marche, ses trous et la géométrie restent dans le banc. La fenêtre est celle de `e.plan(&Request { start: E′,
end: None, distance_m: 2ρ_max, .. }, step)` : elle couvre le disque élargi, pour que le second essai ne change pas de
fenêtre. `KeepOut::build` est appelé sur **cette** fenêtre. Le masque garde l'empreinte des tuiles de sa fenêtre, et un
`assert` au début de `move_end_in` et de `generate_inner` (sous `probe`) refuse un masque construit pour une autre
fenêtre.

### 4.3 Tirage de `E′` et essais

Il se fait par trace et par curseur, avec une graine `id_hash ^ seed ^ tag` passée dans `SplitMix64`. Les positions ne
sortent jamais.

- **Points autour de l'arrivée d'origine `E`** : `r ∈ {2 ; 5 ; 15} km` × **4 caps** tirés uniformément dans `[0, 360)`,
  soit 12 essais. Le point est le point géodésique à `r` de `E` (formule directe sur la sphère). Un point à plus de
  `snap_m` (2 km) du graphe donne `EndTooFar` : il est compté et **sorti du dénominateur**.
- **Un lieu réel de l'équipe** : l'arrivée d'une autre trace de la même équipe, à 30 km ou moins de `E`, tirée par la
  graine. L'arrivée d'une boucle est son départ, c'est-à-dire un lieu de rendez-vous, ce qui est le cas D5 « un lieu de
  l'équipe ». Cela fait 1 essai, sauté s'il n'y a pas de candidat.
- **Fermer en boucle** (`E′` = le départ d'un aller simple) : 1 essai, pour information. Le cas est écarté par D5, il
  ne compte pas dans les critères.

Cela fait **13 essais par trace et par curseur, plus 1 pour un aller simple.** Population : toutes les traces de type
ROAD, GRAVEL ou MTB qui ont au moins une couture valide (marge avant), lisibles, sans autre filtre.

### 4.4 Contrôle du recollé

Il est commun aux deux opérations et se fait en trois étages. Un essai est **accepté par le contrôleur** si les trois
passent.

- **C1, le tronçon neuf seul.** `Checker::check(req_seg, P)` avec le contrôleur de `play_one` : tables d'équipe
  comprises, `Config::default()`. `req_seg` a :
  - `start` = la position de la couture `i` (ou `k`) ;
  - `end` = `Some(j)` pour `resize`, `Some(E′)` pour `move_end` ;
  - `distance_m` = `ℓ + Δ` pour `resize`, `longueur(P)` pour `move_end` ;
  - le type, le mode, la familiarité, `off_team` et la graine du curseur.

  **Toute violation est un refus.** C'est le critère du lot 1 : « 0 refus du contrôleur indépendant ».
- **C2, les coutures, par morceaux continus, par différence.** On découpe le recollé (couche graphe) en morceaux
  continus : suites maximales de jambes jointes par `Node`. Pour chaque morceau qui contient une couture ou une jambe
  neuve :
  - on le contrôle avec `req = { start: son premier point, end: Some(son dernier point), distance_m: sa longueur
    graphe }` ;
  - on fait la même chose sur **les plages de l'original qu'il recouvre** (celle de la couture `i`, celle de `j`),
    contrôlées une à une ;
  - le **refus** est l'ensemble des codes du recollé **absents** de l'union des codes de l'original. `WRONG_START`,
    `WRONG_END`, `BAD_OFFSET` et `DISTANCE_OUT_OF_TOLERANCE` sont ignorés : ce sont des artefacts du découpage.

  Les trous de l'original restent des trous. On ne comble rien pour le contrôleur : le plan l'avait laissé ouvert, et
  ce lot ne le tranche pas. Ce qu'on perd est compté : `unchecked_m` = mètres du recollé dans des morceaux qu'aucun
  contrôle n'a lus.
- **C3, la réutilisation** (dans le banc). On compte les mètres de base des arêtes de `P` qui sont aussi des arêtes du
  préfixe ou du suffixe gardés.
  - Pour `resize`, la zone des coutures (`seam_margin_m` de part et d'autre de `i` et de `j`) est tolérée, à l'image
    de la queue du moteur. Elle est comptée à part, dans `reuse_seam_m`.
  - Le **refus** est `reuse_outside_m > 0`. Avec le masque, il ne peut venir que d'une jambe gardée hors de la vue
    (impossible à reprendre) ou d'un défaut du masque : c'est un garde-fou.

Le relevé donne, par opération et par strate :

- la part d'essais acceptés et la répartition des codes de C1 et de C2 ;
- l'angle aux coutures (sortie de la jambe gardée → première étape de `P`, dernière étape de `P` → entrée de la jambe
  gardée) ;
- `unchecked_m`.

Un taux élevé de `SHARP_TURN` ou `U_TURN` en C2 aux coutures dira au lot 1 qu'il faut donner à la recherche le cap
d'arrivée de la jambe gardée. C'est possible pour `move_end` (déjà fait au §4.1, étape 4). Pour `resize`, ce n'est pas
possible par l'API publique, voir le §5.3.

---

## 5. Livraison B : `resize`

### 5.1 Le crochet d'interdiction (seul changement du moteur)

Tout est sous `#[cfg(feature = "probe")]`, et rien d'autre ne change.

- **`search.rs`** :
  - `Workspace` gagne `pub(crate) keep_out: Option<KeepOut>` et une méthode publique `set_keep_out(&mut self, ko:
    Option<KeepOut>)` ;
  - `Weighter` gagne `keep_out: Option<&'a [u64]>` et `fn with_keep_out(self, ko: Option<&'a [u64]>) -> Self` ;
  - en tête de `Weighter::chain`, `if let Some(k) = self.keep_out { if k[c >> 6] >> (c & 63) & 1 != 0 { return INF } }`.
- **`generate.rs`**, dans `generate_inner`, après `Weighter::new` :
  `#[cfg(feature = "probe")] let ko = ws.keep_out.take();` puis `let w = w.with_keep_out(ko.as_ref().map(..))`, avec
  l'`assert` d'empreinte de fenêtre (§4.2). `take()` consomme le masque : le banc le pose avant **chaque** appel, et
  aucun appel suivant ne peut l'hériter par mégarde. `Workspace::shrink_to` le jette avec le reste.
- **`probe_graft.rs`** : `KeepOut` (§4.2).

**Pourquoi là et pas dans `Ban`** : les arbres ne lisent pas `Ban` (§1.2), et `Weighter::chain` est la seule porte que
lisent les arbres, les A*, `astar_back` et l'accroche. Un masque à `INF` produit exactement « arête absente du graphe
pour cette requête ».

**Invariant.** Sans `probe`, le code compilé est identique. Avec `probe` et `keep_out = None`, `chain` ne gagne qu'une
branche qui n'est jamais prise, et les résultats sont les mêmes au bit près. Le banc tourne toujours avec `probe` : les
empreintes de `likeness` le vérifient (§8).

### 5.2 Méthode

La recherche passe par l'API publique, `generate_with_figures_in(e, win, Some(team), &req, ws, stop)`, avec :

- `req.start` = la position du nœud `i` ;
- `req.end = Some(` la position du nœud `j` `)` ;
- `distance_m = ℓ + Δ`, où `ℓ = s_j − s_i` ;
- `bearing: None` et `figures: Classic` (un aller simple joue `Classic` de toute façon) ;
- le mode, la familiarité, `off_team` et la graine du curseur ;
- le masque des arêtes gardées posé par `ws.set_keep_out` juste avant l'appel.

**Masque d'une fenêtre (i, j)** : les arêtes des jambes `legs[..=i.join]` et `legs[j.join + 1..]`, **moins** celles qui
touchent la zone des coutures (`seam_margin_m` de marche de part et d'autre de `i` et de `j`). L'aller peut ainsi
reprendre quelques dizaines de mètres de parcours au départ, comme la queue du moteur. Les arêtes du tronçon remplacé
`[i, j]` restent libres : la greffe peut en reprendre une partie, et `changed_m` le mesure.

**Accroche aux coutures.** Le point donné est la position exacte du nœud. L'accroche du moteur tombe à 0 m, sur une
chaîne non masquée qui touche le nœud. On vérifie `diagnostics.start_snap_m ≤ 0,5 m` et `end_snap_m ≤ 0,5 m` ; sinon,
l'issue est `seam_snap_off`, comptée par fenêtre.

**Fenêtre du moteur** : celle de `SharedWindows::acquire` (le cache du banc), puis `acquire_wide` après un
`WindowEscape`, exactement comme `play_one`. Le masque se construit sur la fenêtre rendue.

**Tolérance.** Le moteur garde ses candidats à ±10 % de `ℓ + Δ`. Pour un `|Δ|` important, cela peut dépasser 3 % du
total : pour `Δ = +25 %` et `ℓ = 1,5 Δ`, ±10 % du tronçon font ±5 % de `D′` [calculé]. On ne change pas `Params` : la
proposition est **choisie** parmi les candidats de toutes les fenêtres (§5.4), et le relevé dit si c'est assez. Sinon,
le lot 2 posera une tolérance par requête.

### 5.3 Fenêtres (i, j) (plan §4.2)

Les coutures candidates sont les coutures valides (marges des deux côtés) avec `0,05 L ≤ s ≤ 0,95 L`. `Δ = D′ − L`,
`D′ = L (1 + δ)`.

- **Rallonger** (`Δ > 0`) : pour `ℓ* ∈ {1,5 ; 3} × |Δ|`, on découpe `[0,05 L ; 0,95 L − ℓ*]` en **6 cases** égales.
  Dans chaque case, on tire `i` par la graine parmi ses coutures. `j` est la couture après `i` dont `s_j − s_i` est la
  plus proche de `ℓ*` dans `[0,8 ; 1,25] × ℓ*`. Une case sans couple est sautée. Cela fait **12 fenêtres au plus**.
- **Raccourcir** (`Δ < 0`) : il faut `ℓ + Δ ≥ 1,05 × plus court chemin(i, j)`. Le banc n'a pas l'arbre, alors il
  borne avec le vol d'oiseau `c(i, j)`. On tire 6 coutures `i` (une par case de `[0,05 L ; 0,95 L]`). Pour chacune, on
  garde les 2 `j > i` avec `s_j − s_i ≥ 1,5 |Δ|`, `(ℓ + Δ) / c(i, j) ≥ 1,3`, et le rapport `(ℓ + Δ) / c(i, j)` le plus
  proche de **1,6** [proposé]. Cela fait **12 fenêtres au plus**. Sur une boucle, cela place `i` à l'aller et `j` au
  retour sans règle de plus. La part de `DistanceTooShort` mesure ce que coûte l'approximation. L'option
  `--shorten-tree`, qui fait un arbre aller par `i` comme le plan, est laissée au lot 2 si cette part dépasse 20 %.
- Toujours : `ℓ + Δ ≥ 1 km` (la borne basse de la greffe, plan §4.3) et `|Δ| ≥ 1 km`. Une demande sans fenêtre a
  l'issue `no_window`.

Les fenêtres d'une demande sont jouées **l'une après l'autre sur un fil**, comme le service le ferait. Le temps de la
demande est la somme de leurs temps de génération (fenêtres prises au cache, temps de construction compté à part). Le
relevé donne aussi le temps jusqu'à la première proposition à 3 %, pour juger un arrêt anticipé.

### 5.4 Note et choix des propositions

Les candidats sont **toutes** les routes rendues, sur toutes les fenêtres (trois au plus par fenêtre). Chacune donne
`L′ = s_i + longueur(P) + (L − s_j)` et `err = |L′ − D′| / D′`. Le tri se fait :

1. d'abord sur `err ≤ 3 %` ;
2. puis sur `err` arrondi à 0,5 % ;
3. puis sur la part gardée `kept_share = (s_i + L − s_j) / L′`, la plus grande d'abord ;
4. enfin sur le rang du moteur dans sa fenêtre.

On garde jusqu'à **trois propositions** de fenêtres distinctes, après les contrôles C1 à C3. Une proposition refusée
par un contrôle est remplacée par la suivante, mais le refus est compté.

**Issues d'une demande** :

- `ok`, si la meilleure proposition contrôlée est à 3 % ou moins ;
- `off_target`, si des propositions contrôlées existent mais aucune n'est à 3 % ;
- `no_window` ;
- `all_refused`, si des routes ont été rendues mais toutes refusées par C1 à C3 ;
- `engine_fail`, quand aucune fenêtre ne rend de route, avec l'échec dominant : `DistanceTooShort`,
  `NoRoute(détail)`, `NetworkTooSmall`, `StartTooFar` ou `EndTooFar`, ou `seam_snap_off`.

`|δ| > 30 %` porte la marque `beyond_limit` (le refus `change_too_large` de D3), mais la demande est **jouée quand
même**, pour voir où se trouve le seuil.

### 5.5 Jeux

- `δ ∈ {−35 ; −25 ; −10 ; +10 ; +25 ; +35} %` × curseur `{1 (découverte) ; 0,25 (équipe d'abord)}`, par
  `ord_server::generator::cursor::apply` comme `request_of` [lu : 1 → `Discovery`, 0,25 → `TeamFirst`, λ = 2,5] ;
- l'équipe est la surcouche **complète** du lot 6 (`G/lot6/overlays`, lue par `Overlay::read` comme `teams.rs`), sans
  goût appris (`Taste` de `Params` : neutre, comme `likeness`). La surcouche contient le parcours lui-même, ce qui est
  le cas réel : un parcours enregistré est dans la surcouche de son équipe ;
- `--no-keep-out` rejoue sans masque, **à l'essai seulement**, pour mesurer ce que le masque apporte (`reuse_outside_m`
  sans masque) ;
- population : les traces ROAD, GRAVEL et MTB, boucles et allers simples, avec `L ≥ 5 km` et au moins deux coutures
  candidates, **échantillonnées** (§7.3).

---

## 6. Pourquoi pas tout par l'API publique, ni tout par la sonde

- **`resize` par l'API publique seule (sans le crochet)** est faisable dès aujourd'hui, mais l'arbre aller partirait
  le long du parcours gardé. La lentille prendrait volontiers son point de demi-tour sur `R[0..i]` ou `R[j..]`, et
  C3 refuserait une grande part des propositions. On joue cette variante à l'essai (`--no-keep-out`) pour la chiffrer,
  pas comme prototype.
- **`resize` par une copie de la lentille dans la sonde** aurait permis un poids masqué sans toucher `Weighter`, au
  prix d'environ 300 lignes recopiées de `generate_inner` (secteurs, ρ tiré, ordre inverse, contrôle, choix). Le
  prototype jugerait alors une lentille qui n'est pas celle de production. Le crochet de six lignes sous `probe` est
  plus court, et il mesure **le vrai moteur**.
- **`move_end`** a besoin de l'arbre retour et de son état en chaque nœud. Aucune API publique ne les rend, d'où
  `move_end_in`. Il réutilise `tree_backward`, `turn_cost`, `retro_ok`, `steps_of` et `make_anchor`, tous
  `pub(crate)`.

---

## 7. Le banc

### 7.1 Sous-commandes

C'est un nouveau module `tools/ord-loops-bench/src/retouche/`, avec deux sous-commandes ajoutées à `Command` dans
`main.rs`.

**`retouche-continuity`** (livraison A).

| Option | Défaut | Sens |
|---|---|---|
| `--graph`, `--road`, `--gravel`, `--poly`, `--export`, `--letters` | ceux de `likeness` (lot 6, export privé) | mêmes chemins, même résolution que `LikenessArgs::paths` |
| `--out` | `G/retouche/` | dossier des sorties |
| `--label` | `continuity` | préfixe des fichiers |
| `--teams`, `--limit` | toutes, aucune | lettres ; au plus N traces par équipe, dans l'ordre de l'export |
| `--seam-margin-m` | 200 | §2.3 |
| `--proj-tol-m` | 15 | §2.4 |
| `--touch-continues` | non | §2.3 |
| `--threads` | `available_parallelism` | |

**`retouche-play`** (livraison B) prend les mêmes chemins, plus `--overlays` (défaut `G/lot6/overlays`).

| Option | Défaut | Sens |
|---|---|---|
| `--op` | `both` | `move-end`, `resize`, `both` |
| `--cursors` | `0.25,1` | |
| `--deltas` | `-35,-25,-10,10,25,35` | δ en % |
| `--windows` | 12 | fenêtres au plus par demande |
| `--radii-km` | `2,5,15` | §4.3 |
| `--bearings` | 4 | caps par rayon |
| `--no-team-ends`, `--no-close-loop` | non | sautent ces essais |
| `--sample-per-stratum` | 60 | traces de `resize` par équipe × type × forme (§7.3) |
| `--move-end-all` | oui | `move_end` sur toutes les traces éligibles (sinon : le même échantillon) |
| `--no-keep-out` | non | §5.5 |
| `--seed` | `20261122` | |
| `--budget-mb` | 6000 | cache de fenêtres partagé |
| `--teams`, `--limit`, `--threads`, `--label` (`play`) | | comme ci-dessus |

Le code se répartit ainsi :

- `retouche/mod.rs` : arguments, chemins, phases, avec les mêmes `Phases` et le même `AnonSampler` que `likeness`. Si
  la résolution des chemins est factorisée avec `likeness`, `likeness` ne doit pas changer de comportement ;
- `posed.rs` : §3 et §2.3, la couture ;
- `project.rs` : §2.4 ;
- `continuity.rs` : §2.2 ;
- `draw.rs` : tirages des §4.3 et §5.3 ;
- `move_end.rs`, `resize.rs` ;
- `control.rs` : C1, C2 et C3 ;
- `out.rs` : lignes et agrégats.

L'appariement des clés, la lecture de l'export et les lettres viennent de `likeness::export` et de
`likeness::read_letters`. On les réutilise, on ne les recopie pas.

### 7.2 Mode essai

Il s'agit de `--teams B,C --limit 20`, le même essai que les relevés du lot 4. En moins de 3 minutes [estimé], il sert
à :

- vérifier les sorties et les invariants (§8) ;
- chronométrer le débit, en essais et en demandes par seconde, à extrapoler pour la campagne ;
- vérifier le déterminisme : `--threads 1` et `--threads 16` rendent la même empreinte (xxh3 des empreintes de route
  `play::route_hash` des recollés, dans l'ordre des essais, avec le recollé haché comme préfixe, tronçon et suffixe) ;
- jouer `--no-keep-out` (§5.5).

### 7.3 Tirage de la campagne

- **Continuité** : toutes les traces, sans tirage.
- **`move_end`** : toutes les traces éligibles, environ 3 000 [estimé]. Cela fait 13 ou 14 essais × 2 curseurs, soit
  environ 80 000 essais.
- **`resize`** : par équipe × type × forme, jusqu'à **60 traces** tirées au **rang de `xxh3(id_hash ^ seed)`**, la
  règle stable de `holdout::select`. A et B donnent 4 strates ou plus chacune. Les petites équipes entrent en entier.
  Cela fait environ **300 traces** [estimé], × 6 δ × 2 curseurs = environ **3 600 demandes**, soit **43 000 générations
  au plus**.

Chaque ligne de sortie (essai ou demande) porte sa strate : les agrégats se lisent par strate, puis au total **pondéré
par strate** (poids = population de la strate / échantillon), pour que A ne fasse pas à elle seule le total.

### 7.4 Durée et mémoire [estimé]

| Phase | Estimation | Base |
|---|---|---|
| moteur, index du contrôleur, export, appariement des clés | 1 à 2 min | phases de `likeness` : 21 s hors jeux au lot 4, appariement compris [calculé sur `ord-loops-lot4-mesures.md`] |
| continuité (projection, quantiles) | moins de 1 min | quelques millisecondes par trace sur 3 400 traces |
| données d'équipe et tables du contrôleur depuis les surcouches | moins de 1 min | `--dump-teams` du lot 6 |
| `move_end` | environ 3 min | environ 80 000 arbres retour de rayon 5 à 40 km, environ 30 ms en moyenne, 16 fils |
| `resize` | environ 7 min | environ 43 000 allers simples de 1 à 100 km, environ 150 ms en moyenne (une boucle de 1.10 : 290 ms au P50 [mesuré, `ord-loops-lot4c-sonde.md`]), 16 fils |
| contrôles C1 à C3 | environ 2 min | 4 à 6 contrôles par recollé |

Cela fait **environ 15 à 20 min** pour une campagne complète, avec une marge de 2× sous l'heure. La mémoire est celle
de `likeness` (pic anonyme 14,2 Go au lot 4 [mesuré]), plus le masque des arêtes de vue (environ 8 Mo) : elle tient
dans les 30 Go.

**Garde-fou** : après 2 % des demandes, le banc écrit le temps restant estimé sur `stderr`. `--max-minutes` (défaut
60) arrête proprement : les lignes déjà écrites restent, et le résumé porte la mention « campagne coupée ». Les temps
pris sur 16 fils simultanés (8 cœurs physiques) sont **pessimistes** pour le service. L'essai les recale par un passage
`--threads 1` sur les mêmes demandes, et le relevé donne les deux.

### 7.5 Sorties de la livraison B

Elles vont sous `G/retouche/` :

- `<label>-move-end.jsonl` : une ligne par essai. Champs :
  - lettre, rang, strate, curseur, `r` et numéro du cap (ou `team_end`, `close_loop`) ;
  - issue, `ρ`, `widened`, `prefix_retry`, `t2_settled`, `us` ;
  - pour chaque variante : `abandon_m`, `raccord_m`, `raccord / vol d'oiseau(k, E′)`, `L′ / L`, part d'équipe du
    raccord (`team_m / length_m`), angle à la couture ;
  - C1, C2 (codes), C3 (mètres), `unchecked_m` ;
  - empreinte du recollé.
- `<label>-resize.jsonl` : une ligne par demande. Champs :
  - lettre, rang, strate, curseur, δ, `beyond_limit` ;
  - fenêtres jouées et, par fenêtre, `ℓ`, issue moteur, routes rendues, ms ;
  - issue de la demande ; pour la meilleure proposition : `err`, `kept_share`, `changed_m` (mètres de `P` hors des
    arêtes de l'original), longueur de `P`, part d'équipe de `P`, angles aux deux coutures, C1, C2, C3 ;
  - temps total et temps jusqu'à la première proposition à 3 %.
- `<label>-play.json` : agrégats par opération, strate, curseur et δ (comptes, parts, quantiles P10, P50 et P90).
  Pour chaque critère du §9 : numérateur, dénominateur et intervalle de Wilson à 90 %. Il donne aussi les empreintes.
- `<label>-play.md` : le résumé recopiable (agrégats seulement).

Aucune position, aucun id et aucune empreinte d'id n'est écrit, même dans les sorties privées. Le rang dans l'ordre de
l'export et la lettre suffisent à rejouer une trace.

---

## 8. Invariants et tests

- **La génération ordinaire est inchangée.** Il faut faire passer, **sans modification** :
  - `cargo test -p ord-loops` (fonctionnalités par défaut) ;
  - `cargo test -p ord-loops --features probe` (les instantanés de `tests_reference.rs` passent ainsi avec `probe`
    compilé et sans masque) ;
  - `cargo test -p ord-server` ;
  - `cargo test -p ord-loops-check` ;
  - `cargo test -p ord-graph-match`.

  Sur le banc, il faut rejouer l'essai I5 (`likeness --variants svc+org+attr:0 --holdout-share 0.25 --teams B,C --limit 20`, l'organique sans attracteurs actifs,
  §17.8 de `ord-loops-lot4.md`) :
  il doit redonner `I5_TRIAL_FINGERPRINT`, ce qui vérifie le crochet `Weighter` avec `probe` compilé.
- **Nouveaux tests** (grilles synthétiques : `likeness/fixture.rs` dans le banc, les aides de `tests.rs` dans
  `ord-loops` ; le `fixture` d'`ord-graph-match` est privé à ses tests) :
  - dans `ord-loops`, `tests_graft.rs` sous `#[cfg(all(test, feature = "probe"))]` :
    - un masque vide donne les mêmes parcours que sans masque ;
    - une chaîne masquée n'apparaît dans aucun parcours, ni dans aucun arbre ;
    - `move_end_in` sur une grille donne le `k` attendu dans trois cas : `E′` voisin de l'arrivée, `E′` au-delà du
      point lointain, `E′` sur le parcours (`EndOnRoute`) ;
    - un masque d'une autre fenêtre fait paniquer l'`assert` ;
  - dans le banc :
    - la couture valide : nœud hors vue, demi-tour et marge courte sont refusés ;
    - la projection monotone : aller-retour, 8 et tours répétés, avec les cas du §2.5 ;
    - `max_dist_to_seam_m` sur des abscisses à la main ;
    - les tirages des fenêtres : 12 fenêtres au plus, rien dans les 5 % des bouts, graine stable ;
    - la différence de C2 : un code présent dans l'original n'est pas un refus.
- **Avant chaque commit** : `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  puis les tests des crates touchées : `ord-loops-bench` pour A, et en plus `ord-loops` (les deux jeux de
  fonctionnalités) et `ord-server` pour B.

---

## 9. Critères

Ce sont ceux du plan §10. Chacun est jugé sur l'intervalle de Wilson à 90 % : « tenu » si la borne basse atteint le
seuil, « non tenu » si la borne haute est en dessous, « à la limite » entre les deux.

| # | Critère | Définition | Population (dénominateur) |
|---|---|---|---|
| K1 | **Couture à ≤ 1 km pour 95 % des parcours** | `max_dist_to_seam_m ≤ 1 000` (§2.4, couture stricte, `Touch` coupant) | toutes les traces lisibles et non vides de type ROAD, GRAVEL ou MTB, exclues comprises (D1). Donné aussi par strate et sur les seules traces retenues par le banc actuel |
| K2 | **`move_end` sans refus du contrôleur dans 95 % des essais** | issue `ok` ou `truncated`, et C1, C2 et C3 acceptés, sur la variante `best` | essais dont `E′` s'accroche (hors `EndTooFar`), sur des traces qui ont au moins une couture ; `close_loop` exclu |
| K3 | **Distance à ±3 % dans 90 % des essais pour \|δ\| ≤ 25 %** | issue `ok` de la demande (§5.4) | demandes de δ ∈ {±10 ; ±25} % sur l'échantillon, pondérées par strate |
| K4 | **P90 du temps sous 2 s** | `resize` : somme des générations de la demande ; `move_end` : temps de `move_end_in` plus les contrôles | toutes les demandes et tous les essais joués ; le P90 est donné sur 16 fils et recalé à 1 fil (§7.4) |

**K1 est repris tel quel.** Le relevé y ajoute la variante `[5 %, 95 %]` (`max_dist_inner_m`), qui est la vraie
contrainte de `resize`, et la distance de la dernière couture à l'arrivée (`move_end`). Si K1 échoue sur les bouts
seulement, ce n'est pas un échec pour `resize`. **K2 est durci** : un `EndOnRoute` sans couture après `E′` compte comme
un échec. **K3** compte une demande sans fenêtre comme un échec. **K4** est jugé sur la somme séquentielle, parce que
c'est le temps qu'attend la personne.

**Mesuré en plus, sans seuil** (à lire pour régler les seuils **[proposé]** du plan) :

- `kept_share` : P10 et P50 par opération et par δ ;
- `changed_m` / `D′` ;
- part d'équipe du tronçon neuf, par curseur ;
- angle aux coutures et part de `SHARP_TURN` et `U_TURN` en C2 ;
- part de `DistanceTooShort` en raccourcissement ;
- rendement des fenêtres (part des fenêtres qui rendent une route, part des demandes dont la meilleure vient de
  `ℓ = 1,5 |Δ|` contre `3 |Δ|`) ;
- courbe du succès de K3 en fonction de δ, de −35 à +35 %, pour confirmer ou déplacer le seuil de ±30 % de D3 ;
- ce que le masque apporte (`--no-keep-out`, à l'essai) ;
- pour `move_end`, `raccord / vol d'oiseau` et `L′ / L` par rayon.

**Décision.** K1 et K2 tenus ouvrent le lot 1 (arrivée). K3 et K4 tenus ouvrent le lot 2 (distance). Si K1 n'est pas
tenu à cause des trous (et non de la marge de 200 m), c'est le risque §11 du plan : l'entrée doit être complétée par un
recalage géométrique, qui est un autre chantier. Le relevé dit laquelle des deux causes domine, en rejouant K1 avec
`--seam-margin-m 100`.

---

## 10. Livraisons et fichiers

### A. Continuité (aucun changement du moteur)

| Fichier | Quoi |
|---|---|
| `tools/ord-loops-bench/src/retouche/mod.rs` (nouveau) | `RetoucheContinuityArgs`, `run_continuity`, phases |
| `tools/ord-loops-bench/src/retouche/posed.rs` (nouveau) | `Posed`, `Seam`, couture valide (§2.3), plages, masque des arêtes de base de la vue |
| `tools/ord-loops-bench/src/retouche/project.rs` (nouveau) | polyligne du document, projection monotone (§2.4) |
| `tools/ord-loops-bench/src/retouche/continuity.rs` (nouveau) | mesures par trace (§2.2), causes de retenue, strates |
| `tools/ord-loops-bench/src/retouche/out.rs` (nouveau) | `.jsonl`, agrégats, résumé `.md` |
| `tools/ord-loops-bench/src/main.rs` | sous-commande `retouche-continuity`, `mod retouche` |
| `tools/ord-loops-bench/src/likeness/mod.rs` | au plus : rendre publique la résolution des chemins, sans changer `likeness` |

Commit : `feat(bench): mesure de continuité et des coutures pour la retouche (lot 0 A)`. La campagne et le relevé
`ord-retouche-lot0-mesures.md` (partie A) font l'objet d'un commit `docs(loops): …` séparé, lors du relevé.

### B. Prototypes

| Fichier | Quoi |
|---|---|
| `crates/ord-loops/src/search.rs` | sous `probe` : `Workspace::keep_out`, `set_keep_out`, `Weighter::keep_out`, `with_keep_out`, test dans `chain` |
| `crates/ord-loops/src/generate.rs` | sous `probe` : prise du masque après `Weighter::new`, `assert` d'empreinte |
| `crates/ord-loops/src/probe_graft.rs` (nouveau) | `KeepOut`, `SeamIn`, `move_end_in` et ses types |
| `crates/ord-loops/src/lib.rs` | `#[cfg(feature = "probe")] pub mod probe_graft;` |
| `crates/ord-loops/src/tests_graft.rs` (nouveau) | tests du §8, sous `probe` |
| `crates/ord-loops/Cargo.toml` | commentaire de la fonctionnalité `probe` complété (retouche, lot 0) |
| `tools/ord-loops-bench/src/retouche/draw.rs` (nouveau) | tirages de `E′` (§4.3), des fenêtres (§5.3), de l'échantillon (§7.3) |
| `tools/ord-loops-bench/src/retouche/move_end.rs` (nouveau) | essais `move_end`, recollé |
| `tools/ord-loops-bench/src/retouche/resize.rs` (nouveau) | demandes `resize` par `generate_with_figures_in` et le masque, note (§5.4) |
| `tools/ord-loops-bench/src/retouche/control.rs` (nouveau) | C1, C2 par différence, C3 (§4.4) |
| `tools/ord-loops-bench/src/retouche/mod.rs`, `out.rs`, `main.rs` | `retouche-play`, agrégats, critères K1 à K4 |

Commit : `feat(bench): prototypes move_end et resize de la retouche, crochet d'interdiction sous probe (lot 0 B)`. Le
commit message rappelle que la génération ordinaire est inchangée et cite l'empreinte I5 de l'essai.

**Ne change pas** : `ord-server`, le contrat, `ord-loops-check` (il reste indépendant : C2 l'appelle tel quel),
`ord-graph-match` (la marche reste celle du service).

---

## 11. Ce que ce lot ne tranche pas

- **Le comblement des trous pour le contrôleur** : C2 ne contrôle que des morceaux continus et rapporte `unchecked_m`.
- **Le cap à la couture dans la recherche de `resize`** : l'API publique n'en a pas. Si C2 le réclame, le lot 2 ajoute
  une entrée « cap d'arrivée » à l'accroche.
- **Une tolérance par requête** (§5.2), **un arbre par `i` pour raccourcir** (§5.3) et **le tronçon désigné à la main**
  (D4, après la première version).
- **Le goût appris** : les jeux tournent sur le poids neutre. Le service passera le profil de l'équipe, comme pour une
  génération, sans effet attendu sur la continuité ni sur les coutures.

---

## 12. Écarts du codage de la livraison B

- **`KeepOut::build(win, mask_edges, free_edges)`** : une chaîne est masquée si elle porte une arête à masquer **et aucune**
  arête libre (la zone des coutures). Sans cela, une longue chaîne qui contient la jambe de la couture serait masquée entière
  et la recherche ne pourrait plus partir du nœud. Le paramètre `e` du §4.2 est supprimé (inutile).
- **`move_end_in` rejoue après un virage serré** (jusqu'à 4 passes, comme `generate_inner`) : le contrôle interne du raccord
  de la meilleure sortie interdit les transitions refusées et relance l'arbre. Sans cela, 23 % des raccords avaient un
  `SHARP_TURN` (essai : K2 à 68 %, 84 % avec les reprises). `MoveEndOut` gagne `passes`.
- **`EndOnRoute { edge }`** rend l'arête accrochée, pas `after_s_m` : le banc situe lui-même la couture qui suit. Le rejeu avec
  le masque du seul préfixe (`prefix_retry`) est fait par le banc, en rappelant `move_end_in`, pas par le moteur.
- **C2 est borné à `±c2_window_m` (5 km) de graphe autour des coutures** (`--c2-window-m`), original compris, pour que le coût
  des contrôles reste borné sur des traces de 400 km ; `unchecked_m` compte ce que les trous laissent sans lecture dans ce voisinage.
- Seule la variante `best` de `move_end` est contrôlée ; les deux autres sont mesurées (abandon, raccord, `L′/L`).
- Le sous-commandes sont `retouche-play` (+ `--op`), les fichiers `out_play.rs`, `play.rs` ; `Posed` ne garde ni `trace` ni `runs`.
- La commande I5 du §8 est incomplète : `--variants base,svc,svc+org,svc+org+attr:0` (empreinte `50adca52ef713d18` retrouvée).
