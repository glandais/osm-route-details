# Générateur de parcours — la planète dans le service : conception

> Statut : conception technique, validée et **implémentée** le 20 novembre 2026 (§11 : écarts et relevés). Suite de
> [`ord-graph-views-hors-memoire.md`](ord-graph-views-hors-memoire.md) §10.4 (la planète chargée par le service sur le
> poste) et du lot 6 ([`ord-loops-lot6.md`](ord-loops-lot6.md) : moteur fenêtré, index globaux du démarrage, arêtes
> larges). Motif : les fichiers de la planète sont justes, mais le service ne les sert pas dans le conteneur de 3 Go.
> **[mesuré]** : relevé de ce document ou déjà publié ; **[calculé]** : dérivé de mesures ; **[estimé]** : à confirmer.

## 0. Périmètre

- **Dans la note** : la fenêtre d'une requête (zone d'accroche), les arêtes plus larges qu'une cellule (seuil, plafond,
  relecture à chaque dépôt), et un **fichier d'index de service** `.ordi` calculé hors ligne et projeté au démarrage.
- **Hors de la note** : les formats `.ordg`, `.ordv` et `.orda` (inchangés : rien à reconstruire), les parcours qui
  passent l'antiméridien (le moteur ne raccorde pas, déjà documenté), l'altitude.
- **Contrainte** : un graphe sans fichier `.ordi` (la France aujourd'hui) se charge exactement comme avant ; les
  générations rendent les mêmes parcours avec ou sans fichier.

## 1. Constat [mesuré, sauf mention]

Planète du 19 novembre 2026 : base 431,5 M nœuds, 555,2 M arêtes ; `road` 195,1 M nœuds, 247,9 M chaînes ; `gravel`
272,4 M nœuds, 353,3 M chaînes. Service de `6e26240`, réglages par défaut, cache de pages froid.

| Constat | Planète | France (lot 6) |
|---|---|---|
| Démarrage, 3 Go | tué (`oom-kill`) à 2 min 53 s | 8,5 s |
| Démarrage, 12 Go | 36,7 min, pic anonyme 5,0 Gio | — |
| Index des vues gardé en mémoire (`index_mb`) | 646 Mo | ≈ 8 Mo |
| Plus longue chaîne `Lmax` | 566 km `road`, 1 406 km `gravel` | 26,5 et 15,3 km |
| Arêtes plus larges qu'une cellule | 16 448 (plafond 1 000) | 1 |
| Générations (56, 14 lieux) | 6 × 200, 10 × 422, **39 × 503**, 1 × 504 | — |

Les causes, dans le code :

1. **Fenêtre** (`Engine::plan_with`) : la zone d'accroche est élargie de `Lmax` **de toute la vue** (lot 6 §5.1).
   Sur la planète, chaque fenêtre couvre des centaines de kilomètres autour du départ ; son estimation dépasse le
   budget (800 Mo) et la réservation attend l'échéance : 503 `overloaded`.
2. **Composantes fortement connexes** (`ViewIndex::build`) : Tarjan à 13 o par nœud, les deux vues en parallèle.
   ≈ 6 Go transitoires **[calculé]**, d'où le pic et la mort à 3 Go. 9,3 min (`scc_ms`).
3. **Chaînes étrangères** (`ViewIndex::build`) : un balayage de toutes les chaînes, 2,7 min (`foreign_ms`), et la
   table reste en mémoire anonyme (l'essentiel des 646 Mo).
4. **Arêtes larges** (`CellIndex::exceptions`, `GraphIndex::build`) : le seuil de décodage est la moitié de la **plus
   petite** tuile du graphe. Près des pôles, une tuile z11 fait quelques kilomètres de large : le seuil s'effondre
   et le balayage décode beaucoup d'arêtes (14,4 min, puis 8,2 min pour `GraphIndex`). Le plafond de 1 000 est
   dépassé : les dépôts se replient sur l'index entier (`SegIndex::build`), impossible sur la planète.
5. **Relecture à chaque dépôt** (constat de lecture du code) : `CellIndex::new` relit la liste des arêtes larges
   **à chaque dépôt** (`deposit.rs`, `Worker::matching`). Sur la planète, ce serait 14 min par dépôt.
6. **Étendue des nœuds** (`node_extent`, contrôle du polygone) : un balayage de tous les nœuds de la base.

## 2. Principe

- **Corriger ce qui est faux à toute échelle** : la zone d'accroche (§3) et le seuil des arêtes larges (§4). Ces
  corrections valent aussi sans fichier d'index.
- **Calculer hors ligne ce qui ne dépend que des fichiers** (§5) : chaînes étrangères, composantes courtes, arêtes
  larges, étendue des nœuds. Une commande `ord-graph-build index` écrit un fichier `.ordi` à côté de la base. Le
  service le **projette** (`mmap`) : des pages de fichier, que le noyau reprend, au lieu de mémoire anonyme.
- **Les mêmes fonctions** calculent le fichier hors ligne et l'index au démarrage quand le fichier est absent : le
  fichier ne peut rendre que ce que le démarrage aurait calculé (§7).

## 3. La zone d'accroche par les bouts des chaînes

Ce que la zone d'accroche doit garantir (lot 6 §5.2, point 2) : **les deux bouts de toute chaîne candidate à
l'accroche sont dans des tuiles de la fenêtre** (ils sont alors complets). Aujourd'hui, cette garantie vient d'un
élargissement de la boîte d'accroche par `Lmax`. On la donne exactement :

- `B` : les tuiles de la boîte d'accroche (les cases de 0,01° que `snap` balaie, mêmes formules), **sans
  élargissement** ;
- `C(B)` : les chaînes qui touchent une tuile de `B`, possédées (`cell_info(ci).n_edges`) ou étrangères
  (`foreign_of`) ;
- zone d'accroche = `B` ∪ { tuile de `a`, tuile de `b` : chaîne `(a, b)` de `C(B)` }.

**Preuve.** Une chaîne candidate a un point échantillonné dans la boîte d'accroche, donc dans une tuile de `B` ; elle
touche cette tuile (son polyligne y passe), donc elle est dans `C(B)` (index des étrangères, lot 6 §4.1) ; les tuiles de
ses deux bouts sont ajoutées. Les points 1, 3 et 4 du §5.2 ne changent pas. La nouvelle zone est **incluse** dans
l'ancienne (un bout est à moins de `Lmax` de tout point de sa chaîne) : par le §5.4 du lot 6 (toute fenêtre qui
contient `N(req)` rend le même parcours), les parcours ne changent pas.

**Coût.** Les chaînes des tuiles de `B` (1 à 4 tuiles) : quelques dizaines de milliers de lectures `edge_ends` et
`node_cell` par requête, au plus quelques centaines de milliers dans une tuile de grande ville **[estimé : 1 à
40 ms]**, pour des générations de l'ordre de la seconde.

**Repli après échappement** (`generate_windowed`, jamais attendu) : aujourd'hui `N(req)` élargi de `Lmax` + 10 km. Il
devient `N(req)` élargi de `Lmax(B)` + 10 km, `Lmax(B)` étant la plus longue chaîne de `C(B)` (lue dans le même
balayage). `Engine::longest_chain_m` reste, pour les tests et le banc.

## 4. Les arêtes plus larges qu'une cellule

- **Seuil par cellule** : une arête possédée par la cellule `(x, y)` ne peut sortir des 3 × 3 cellules voisines qu'en
  traversant une tuile entière d'une rangée `y − 1` à `y + 1`. Le seuil devient la moitié de la plus petite dimension
  de ces trois rangées (largeur prise à leur latitude la plus haute), au lieu de celle du graphe entier. Même facteur ½,
  même liste résultante (le seuil ne fait que trier ce qu'on décode) : c'est une économie, pas un changement de
  résultat. Vaut pour `CellIndex::wide_edges` et `GraphIndex::build` (`min_tile_m` dans les deux crates).
- **Plafond** : `MAX_WIDE_EDGES` passe de 1 000 à **100 000**. La liste coûte ses points : une arête large a en
  moyenne quelques centaines de points **[estimé]**, 16 448 arêtes tiennent en quelques dizaines de Mo. Au-delà du
  plafond, le repli reste celui d'aujourd'hui.
- **Une seule lecture** : la liste des arêtes larges (avec leurs points) est lue au démarrage, gardée dans `Ready`
  (`Arc`) et passée à `CellIndex` (`CellIndex::with_wide`). Elle n'est plus relue à chaque dépôt.

## 5. Le fichier d'index de service `.ordi`

### 5.1 Contenu

Un fichier par graphe : `<nom>.ordi`, à côté de `<nom>.ordg` (comme `.orda` à côté des vues).

| Partie | Contenu | Remplace | Planète |
|---|---|---|---|
| En-tête | magic `ORDINDX\0`, version, identité de la base (nœuds, arêtes, taille et `xxh3` du PBF, époque de construction), identité de chaque vue (genre, nœuds, chaînes, cellules, époque), `CAP` des composantes, `Lmax` par vue, boîte des nœuds | — | 256 o |
| `FOREIGN` (par vue) | `(clé de tuile, id de chaîne)`, triés, 8 o | `ViewIndex::foreign` | ≈ 0,5 Go **[estimé]** |
| `SHORT` (par vue) | `(id de chaîne, longueur dm de sa composante)` pour les composantes de moins de `CAP`, triés, 8 o | `ViewIndex::short_comp` | ≈ 0,1 Go **[estimé]** |
| `WIDE_SEG` | arêtes de base dont un segment sort des voisines de leur cellule (u32, croissants) | `CellIndex::exceptions` | 16 448 × 4 o |
| `WIDE_END` | arêtes de base dont un bout n'est ni dans leur cellule ni dans une voisine (u32) | `GraphIndex::build` | ≤ 16 448 × 4 o |
| `NODE_SAMPLE` | un nœud de base sur 16, positions E7, dans l'ordre des ids | le balayage de `node_extent` | 27 M × 8 o ≈ 0,2 Go |

Chaque partie a son décalage, sa longueur et son `xxh3` dans une table après l'en-tête ; `open` contrôle les
longueurs, l'alignement et l'ordre (tri strict) sans tout relire ; `verify` relit les `xxh3` (commande
`ord-graph-build index --check`, et les tests).

`CAP` : les longueurs de composante sont exactes sous `CAP` et valent « au moins `CAP` » au-delà (lot 6 §6). Le service
calcule le sien (`min_component_share × max_distance_m`, 360 km par défaut) et **refuse un fichier dont le `CAP` est
plus petit** ; un `CAP` plus grand convient (on tronque à la lecture). La commande écrit `CAP` = 400 km par défaut
(`--cap-km`).

### 5.2 Construction : `ord-graph-build index --graph <nom>.ordg [--views road,gravel] [--check]`

- Ouvre la base et les vues par `mmap`, contrôle leur identité (comme `Engine::new`).
- Par vue, **l'une après l'autre** : chaînes étrangères (la fonction de `ViewIndex::build`, en parallèle par paquets de
  cellules), puis composantes (`strong_components`, la même fonction). Pic attendu : 13 o par nœud de la plus grande
  vue, ≈ 3,5 Go pour `gravel` **[calculé]**, au-dessus du budget de 1 Gio des constructions hors mémoire (§8).
- Arêtes larges avec le seuil par cellule (§4), échantillon des nœuds.
- Écriture dans un temporaire du même dossier, `fsync`, renommage, mode 0444 (comme les autres fichiers).
- Rapport JSON : durées, pic anonyme, tailles par partie, comptes.

Dépendances : l'outil dépend en plus d'`ord-loops`, `ord-loops-check` et `ord-graph-match`, qui exportent les fonctions
de calcul. Le **format** (écriture, ouverture, contrôle d'identité) est dans `ord-graph` (`ord_graph::service_index`),
comme `.orda` : il ne dépend d'aucun crate du moteur.

### 5.3 Lecture par le service

- Configuration : `ORD_GENERATOR__INDEX` ; par défaut `graph` avec l'extension `.ordi` (`GeneratorConfig::index_path`,
  sur le modèle d'`attractor_paths`).
- **Fichier absent** : `info` « no service index, computing it », démarrage d'aujourd'hui (avec les corrections des §3
  et §4).
- **Fichier présent mais pas de ce graphe** (identité, `CAP` trop petit, partie illisible) : générateur désactivé
  (`DisabledReason::IndexMismatch`, 503 `data_unavailable` comme `ViewMismatch`) et `warn` qui dit quoi. Recalculer en
  silence tuerait le conteneur sur la planète.
- `Engine::with_index(base, road, gravel, params, index)` : `ViewIndex` lit `foreign` et `short_comp` dans le fichier
  (tranches projetées, recherche par dichotomie, mêmes signatures `foreign_of`, `comp_len_dm`). `Engine::new` garde son
  calcul (tests, banc, graphes sans fichier).
- `node_extent` lit la boîte et l'échantillon du fichier ; `GraphIndex` et la liste des arêtes larges viennent du
  fichier (les points des arêtes larges sont relus dans la base : 16 448 arêtes, une fraction de seconde
  **[estimé]**).
- Journal `generator ready` : `index` = `file` ou `computed`, `index_mb` compte la mémoire anonyme seulement.

**Attendu sur la planète** : démarrage de l'ordre de la minute, pages froides **[estimé]** (contrôles d'identité, lecture
des en-têtes, points des arêtes larges) ; mémoire anonyme au démarrage de l'ordre de 0,1 Go **[estimé]**.

## 6. Ce que ça change pour le déploiement

Cinq fichiers au lieu de quatre (`.ordg`, deux `.ordv`, `.poly`, `.ordi`), plus les `.orda` quand ils existent. La
chaîne de construction (`build`, `views`, puis `attractors`) ajoute la commande `index` après `views`. Pour la France, le fichier est
facultatif (le démarrage reste de quelques secondes sans lui).

## 7. Tests

- **Fenêtre** (`ord-loops`) : sur les graphes de test et le graphe de région, pour un échantillon de requêtes,
  `N_nouveau(req) ⊆ N_ancien(req)` et mêmes parcours que le moteur à plat (les tests d'équivalence fenêtré / à plat du
  lot 6 tournent sur la nouvelle zone sans changer leurs références). Un graphe de test avec une chaîne de 500 km (un
  bac) : la fenêtre d'une requête loin du bac n'en contient pas les tuiles lointaines ; une requête à l'embarcadère
  contient la tuile de l'autre rive, et rien entre les deux.
- **Arêtes larges** (`ord-graph-match`, `ord-loops-check`) : seuil par cellule = même liste que le seuil global, sur des
  graphes de test à haute latitude (une arête courte près du pôle, une longue à l'équateur).
- **Fichier** (`ord-graph`) : écriture puis ouverture rendent les mêmes tables ; refus d'un fichier d'une autre base,
  d'une autre vue, d'un `CAP` trop petit, d'une partie tronquée, d'une table non triée.
- **Équivalence** (`ord-loops`) : `Engine::with_index` et `Engine::new` ont les mêmes `foreign`, `comp_len_dm` et
  `Lmax` (le `selfcheck` existant), et rendent les mêmes parcours sur la série du banc région.
- **Service** : démarrage avec un `.ordi` juste (journal `index=file`), avec un `.ordi` d'un autre graphe (désactivé,
  503), sans `.ordi` (calcul, comme avant) ; un dépôt n'appelle plus le balayage des arêtes larges.
- **Bout en bout** (`ord-graph-build`) : `build`, `views`, `index --check` sur le mini-PBF.

## 8. Protocole de mesure

1. `ord-graph-build index` sur la France puis la planète : mur, pic anonyme, tailles.
2. Le service sur la planète, sous `systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0`, environnement de
   l'image, cache de pages froid : temps jusqu'à `generator ready`, pic anonyme, puis la série de 56 générations du
   §10.4 et la série des dix villes du lot 6 (temps, codes), `anon` du groupe échantillonné.
3. La France avec et sans `.ordi` : mêmes parcours sur la série du lot 6, temps de démarrage.

## 9. Points de tension

- **Le pic de la commande `index`** (≈ 3,5 Go **[calculé]**) dépasse le budget de 1 Gio des constructions hors mémoire.
  Il tient sur le poste (15 Gio) ; un Tarjan plus économe (un tableau de 4 o par nœud, algorithme de Pearce) le
  diviserait par trois, au prix d'un second code de composantes à garder identique au premier. Proposé : le même code,
  mesuré, et l'économie seulement si le pic gêne.
- **Un fichier de plus à garder cohérent** : l'identité est contrôlée ; un `.ordi` oublié lors d'un changement de
  graphe désactive le générateur au lieu de le faire servir faux.
- **Le budget de 800 Mo** ne bouge pas : les fenêtres de la planète ont la densité locale de celles de la France.
  À confirmer par la série des villes (§8).

## 10. Étapes de code

1. `fix(loops)` : zone d'accroche par les bouts des chaînes (§3), repli par `Lmax(B)` ; tests de fenêtre.
2. `fix(graph-match, loops-check)` : seuil des arêtes larges par cellule (§4) ; tests à haute latitude.
3. `fix(server)` : liste des arêtes larges lue une fois et partagée par les dépôts, plafond à 100 000.
4. `feat(graph)` : format `.ordi` (`ord_graph::service_index`) : écriture, ouverture, identité, `verify` ; tests.
5. `feat(graph-build)` : commande `index` ; bout en bout sur le mini-PBF.
6. `feat(loops, server)` : `Engine::with_index`, lecture du fichier par le service, configuration, désactivation sur
   fichier étranger ; tests.
7. Mesures (§8), puis relevé dans cette note.

## 11. Réalisation (20 novembre 2026)

### 11.1 Écarts à la conception

- **Pas de nouvelle raison de désactivation** : un `.ordi` illisible ou d'un autre graphe éteint le générateur avec
  `graph_unreadable` (et un `warn` qui dit lequel), au lieu d'un `IndexMismatch` du contrat : le contrat HTTP ne change
  pas. Le moteur, lui, a bien `EngineError::IndexMismatch`.
- **Validation complète à l'ouverture** : `IndexFile::open` relit l'empreinte de chaque section et contrôle le tri et
  les bornes de toutes les tables (comme `.orda`), au lieu de le réserver à `--check` : 0,58 Go relus, compris dans
  les 7,6 s du démarrage.
- **Un débordement trouvé en chemin** (`fix(loops)` séparé) : le centre de tuile de la classe de fenêtre (§5.5 du
  lot 6) se calculait en `i32`. Au-delà de 107,4° de longitude, la somme de deux longitudes E7 débordait : classe de
  millions de tuiles, 503 de Tokyo à Anchorage (et un `panic` en debug). Calcul en `i64`, même résultat ailleurs ; test
  aux longitudes du Pacifique.
- **France sans arête large** : 0 [mesuré, examen exhaustif de toutes les arêtes] ; le chemin de 83,6 km du lot 6
  n'existe plus dans l'extrait du 17 novembre (plus longue arête de base : 15,7 km).

### 11.2 Vérifications

`cargo fmt`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace` : 989 tests, 0 échec. Tests
ajoutés : zone d'accroche d'un bac de 300 km (la tuile de l'autre rive, pas celle du milieu, même parcours que le
moteur à plat) ; arêtes larges au seuil par latitude = examen de toutes les arêtes, à 84° N et à l'équateur (mutation
du facteur ½ détectée) ; aller-retour, refus et corruption du `.ordi` ; `Engine::with_index` = `Engine::new` (tables,
longueurs de composante pour chaque chaîne, parcours de 12 requêtes, au seuil des réglages et au triple) ; refus d'un
fichier d'un autre graphe ou d'un seuil trop bas ; service : mêmes parcours avec et sans fichier, fichier d'un autre
graphe ou tronqué → éteint ; `ord-graph-build index --check` de bout en bout sur le mini-PBF.

### 11.3 Relevés [mesuré]

**`ord-graph-build index`** (binaire de `b391f15`, sans cgroup) :

| | France | Planète |
|---|---|---|
| Mur | 6,5 s | 20 min 03 s |
| Pic anonyme | 0,33 Gio | **4,76 Gio** (estimé 3,5) |
| Fichier | 15,1 Mo | 582 Mo |
| Composantes `road` / `gravel` | 1,5 / 2,4 s | 63 / 780 s |
| Chaînes étrangères `road` / `gravel` | 121 466 / 223 406 | 3 399 631 / 6 195 931 |
| Composantes courtes `road` / `gravel` | 150 561 / 224 629 | 21 890 652 / 14 273 526 |
| Arêtes larges (segment / bout) | 0 / 0 | 16 448 / 16 356, en 85 s (14,4 + 8,2 min avant) |

Les composantes de `gravel` attendent le disque (780 s pour 272 M nœuds, contre 63 s pour les 195 M de `road`) : la
vue (39 Go) et la base ne tiennent pas ensemble dans le cache de pages.

**Le service sur la planète**, avec le `.ordi`, sous `MemoryMax=3G`, `MemorySwapMax=0`, environnement de l'image,
cache de pages des cinq fichiers évincé au départ, série du §10.4 de la note des vues (14 lieux, route 60 km, gravel
40 km, route 100 km, deux passes, 84 générations en série) :

| | Avant (12 Go, sans fichier) | Après (3 Go, avec `.ordi`) |
|---|---|---|
| Prêt (`load_ms`) | 2 203 521 ms | **7 606 ms** |
| `index_mb` | 646 | 0 |
| Pic anonyme | 5,0 Gio (démarrage) | **920 Mo** (pendant la série) |
| `oom` / `oom_kill` | — (tué à 3 Go) | 0 / 0 |
| 200 | 6 sur 56 | **55 sur 84** ; médiane 0,87 s, p90 3,40 s, max 7,85 s |
| 503 `overloaded` | 39 | **0** |
| 504 | 1 | 1 (Tokyo, route 100 km, pages froides, 8,0 s ; 7,46 s au second passage) |
| 422 | 10 | 28 : 18 `no_candidate`, 10 `start_too_far` |

Les 422 sont les échecs ordinaires des tirages : plein Atlantique et Taveuni (`start_too_far`), Ushuaia, Reykjavik en
route 60 km, Anchorage en route 100 km, et Suva (`no_candidate`). Suva, sondé au moteur : des candidats, tous à L/D de
0,1 à 0,6 (aucune boucle de 15 à 60 km sans repasser par la même route) ; Nadi et le Kamtchatka de même, alors que
Tonga (−175°), Samoa, Auckland, Wellington, Dunedin et Honolulu rendent leurs boucles : la longitude n'y est pour
rien. Fenêtres en mémoire à la fin : 654 Mo (budget 800 Mo).

### 11.4 Ce qui reste

- **Tokyo à pages froides** : 7,8 s pour une boucle de 60 km, 504 à 100 km. La densité (1,4 M chaînes pour 22 tuiles)
  et le disque, pas la conception ; à suivre sur le serveur.
- **Pic de la commande `index`** : 4,76 Gio. Tient sur le poste ; le Tarjan économe du §9 reste possible.
- **Non mesuré ici** : la série des dix villes et la concurrence du lot 6 sur la planète, les dépôts sur la planète.
