# Générateur de parcours — lot 6 : la France entière dans 2 Go, moteur fenêtré, conception

> Statut : **codé** (branche `feat/ord-loops-window`) ; les relevés sont dans [`ord-loops-lot6-mesures.md`](ord-loops-lot6-mesures.md).
> Les écarts entre cette note et le code sont listés au §19 ; les passages concernés sont corrigés ci-dessous. Socle : [lot 2](ord-loops-lot2.md) (moteur), [lot 3a](ord-loops-lot3a.md)
> (service) et leurs relevés ; graphe et vues France construits le 16 novembre 2026 (extrait Geofabrik du 15/11).
> **[mesuré]** : commande lancée pour cette note (binaires `release` de `develop`, ou sonde jetable hors dépôt du §18) ;
> **[calculé]** : compte des tableaux du code à partir des comptes mesurés du graphe ; **[estimé]** : à relever au codage.
> Mo et Go sont décimaux. Équipes lettrées A à I comme aux relevés précédents ; ni coordonnée de départ, ni identifiant
> d'équipe, ni secret ici. Les seuls lieux nommés (Paris, Lyon, Nantes, Clermont-Ferrand) sont des centres de villes,
> choisis pour mesurer la densité du réseau, sans lien avec les départs du banc.

## 0. Ce que le lot livre

Un moteur qui sert le graphe France dans le conteneur de 2 Go, **sans changer** le contrat HTTP (api 1.9), les formats
`.ordg`, `.ordv` et `.ordo`, ni **un seul** parcours rendu. Le moteur à plat recopie aujourd'hui tout le graphe en
mémoire ; le moteur fenêtré ne recopie que les cellules z11 autour de la requête, renumérotées localement, et garde à
part, à l'échelle du graphe, les seules choses qui ne se calculent pas dans une fenêtre (§3).

La piste du responsable est **retenue**. Trois amendements, qui ne la contredisent pas :

1. **Le graphe de base n'est plus recopié du tout**, même par fenêtre : la recherche ne lit que les tableaux de la vue ;
   les arêtes de base (attributs, longueur, extrémités, géométrie) sont lues dans le fichier projeté, à la demande (§4.4).
2. **Ce qui est global reste global, sous forme creuse** : composantes fortement connexes des vues et attributs d'équipe
   (arêtes d'équipe, comblements, composantes strictes) sont calculés une fois à l'échelle du graphe, sur ce qui compte
   seulement (§6, §7). Les calculer dans la fenêtre donnerait d'autres résultats au bord.
3. **Une fenêtre est un ensemble de tuiles, pas une requête** : toute fenêtre qui contient les tuiles dont une requête a
   besoin rend exactement le même résultat (§5.4). Le cache peut donc servir une requête avec une fenêtre plus grande
   bâtie pour une autre, et son état ne change jamais un parcours.

Deux chantiers hors du moteur sont **dans** le lot, parce que sans eux le service ne tient pas sur la France (§1.3,
§8) : le contrôleur indépendant reconstruit des tables sur tout le graphe **à chaque requête**, et l'index spatial des
dépôts retombe sur un index entier parce que la France a une arête plus large qu'une cellule.

## 1. Constat chiffré

### 1.1 Le graphe France [mesuré]

| | base `.ordg` | vue `road` | vue `gravel` |
|---|---|---|---|
| nœuds | 18 351 265 | 6 423 851 | 10 203 966 |
| arêtes (base) / chaînes (vue) | 23 477 176 | 8 042 109 | 13 228 298 |
| arêtes de base couvertes | — | 12 540 197 | 17 137 875 |
| fichier | 1,85 Go | 866 Mo | 1,43 Go |
| cellules z11 | — | 3 294 | 3 326 |
| plus longue chaîne | — | 26,5 km | 15,3 km |

Construction : base 80 s (pic 4,4 Go), vues 47 s (pic 6,2 Go), surcouches des 9 équipes 20 s (pic 8,9 Go). Banc
`ord-loops-bench` avec une équipe et une requête : **8,0 Go** de RSS maximal, 16,9 s de chargement du moteur, ~2 s de
préparation par équipe ; neuf équipes : 9,3 Go. Les générations marchent (même résultat au banc court, 59 ms à chaud).

### 1.2 Où part la mémoire du moteur à plat [calculé]

Tailles des tableaux de `engine.rs`, `team.rs` et `search.rs`, aux comptes du §1.1 (`n` nœuds de vue, `c` chaînes, `N` et
`E` nœuds et arêtes de base ; arêtes orientées ≈ 1,82 `c` en `road`, 1,89 `c` en `gravel`) :

| Bloc | Contenu | France |
|---|---|---|
| `BaseData` | `ends`, `attrs`, `way` (8 o × `E`), `len_dm` (4 o × `E`), `pos` (8 o × `N`) | **804 Mo** |
| `ViewData` `road` | positions, extrémités, CSR sortant et entrant, chaînes, caps, poids, composantes, **`binc` indexé par nœud de base (73 + 100 Mo)**, **`edge_chain` indexé par arête de base (94 Mo)** | **823 Mo** |
| `ViewData` `gravel` | idem, deux types servis | **1 252 Mo** |
| Index d'accroche (`warm`) | une entrée de 8 o par case de 0,01° traversée, et le tri | ~0,2 à 0,4 Go |
| `Workspace`, une vue | trois arbres : `Slot` de 16 o par arête orientée, deux `u32` par nœud ; `ban` | `road` 958 Mo, `gravel` **1 568 Mo** |
| `TeamData`, une équipe | `ChainTeam` 12 o par chaîne et par type, composantes strictes 12 o par nœud de vue et par type | **736 Mo** (+ 159 Mo transitoires) |
| Pages des fichiers | tout est lu au chargement | 4,15 Go (récupérables) |

Moteur 2,88 Go + espace de travail `road` 0,96 Go + pages lues 4,15 Go = **7,99 Go** : c'est le 8,0 Go mesuré. Le tas
seul du moteur à plat (2,9 Go sans équipe ni espace de travail) dépasse déjà le conteneur ; aucun réglage ne le fait
entrer. Sur la région, le même calcul donne ~150 Mo de moteur et ~55 Mo par équipe, cohérent avec les 238 Mo et
621 Mo du service relevés au lot 3a.

### 1.3 Ce qui compte aussi, hors du moteur

- **Le contrôleur indépendant** (`ord-loops-check`) est neuf à chaque requête (`generate.rs` du service :
  `Checker::new` par génération) et construit paresseusement, **sur tout le graphe de base**, le degré admis de chaque
  nœud (18 Mo), une incidence CSR (73 Mo de décalages, un tri de 2 × `E` admises paires de 8 o, ~0,3 Go transitoires),
  le degré d'équipe (18 Mo), un booléen par arête (23 Mo), puis deux balayages de tous les nœuds [calculé]. Sur la
  région, c'est déjà 120 ms par génération stricte (relevé du lot 3a, §4.3) ; sur la France, environ 0,3 à 0,45 Go et
  une à deux secondes **par requête** [estimé].
- **L'index spatial des dépôts** : `CellIndex::fits` est **faux** sur la France [mesuré] : une seule arête de base est
  plus large qu'une cellule (83,6 km, `footway`, dans aucune vue). Le service retombe alors sur l'index entier de tout le
  graphe (107 287 900 segments [mesuré]), hors de question dans 2 Go. Le contrôle lui-même balaie toute la géométrie :
  16,0 s au démarrage [mesuré].
- **Le démarrage** : 16,9 s de chargement du moteur, plus 16 s de `fits`, plus le contrôle du polygone.

### 1.4 Ce que mesure la sonde [mesuré]

Sonde du §18, un fil, sur les vues France projetées en mémoire :

| | `road` | `gravel` |
|---|---|---|
| Index des chaînes étrangères à une tuile (§4.1) : balayage | 1,70 s | 2,74 s |
| chaînes dont les deux bouts sont dans deux cellules | 114 014 | 209 751 |
| chaînes à décoder (longueur ≥ distance du bout `a` au bord) | 374 624 | 700 998 |
| entrées « tuile traversée sans y avoir de bout » | 119 415 | 219 209 |
| Composantes fortement connexes (Tarjan itératif sur le CSR des cellules) | 1,13 s | 1,85 s |
| composantes | 89 504 | 120 691 |
| composantes de 180 km ou plus | 3 | 4 |
| chaînes dont la composante fait moins de 180 km (ou dont les bouts sont dans deux composantes) | 144 154 | 213 181 |

Taille d'une fenêtre : chaînes des tuiles dont le centre est à moins de `r` + 12 km d'un centre-ville (approximation de
« tuiles qui touchent le disque de rayon `r` ») :

| Centre | `r` | tuiles | chaînes `road` | chaînes `gravel` | liens `gravel` |
|---|---|---|---|---|---|
| Paris | 50 km | 69 | 551 467 | 741 075 | 1 057 507 |
| Paris | 100 km | 240 | 870 010 | 1 285 746 | 1 742 002 |
| Paris | 150 km | 498 | 1 298 674 | 1 999 538 | 2 638 982 |
| Lyon | 100 km | 201 | 715 632 | 1 211 740 | 1 608 347 |
| Lyon | 150 km | 434 | 1 166 526 | **2 088 421** | 2 754 831 |
| Nantes | 100 km | 192 | 679 026 | 964 756 | 1 266 640 |
| Clermont-Ferrand | 100 km | 201 | 395 807 | 686 929 | 864 028 |
| Clermont-Ferrand | 150 km | 437 | 959 508 | 1 657 826 | 2 129 471 |

Une boucle de 200 km a un disque de 100 km de rayon : **au plus 1,3 M de chaînes `gravel`**, un dixième de la France.
L'aller simple de 300 km de la cible du plan tient, au pire (départ et arrivée proches), dans un disque de 150 km : au
plus 2,1 M de chaînes.

## 2. Principe

Le moteur se coupe en trois étages, de durées de vie différentes :

| Étage | Quoi | Vie | Mémoire France |
|---|---|---|---|
| `Graph` | fichiers projetés, contrôles d'identité, et par vue : index des chaînes étrangères à chaque tuile, longueurs de composantes **creuses**, plus longue chaîne | le processus | ~8 Mo [calculé] |
| `TeamData` | par équipe et par type : arêtes d'équipe et comblées (ids **globaux**), longueurs des composantes strictes (creuses), statistiques | l'entrée du cache d'équipes | quelques Mo par équipe [estimé] |
| `Window` | par vue : les chaînes des tuiles d'une zone, à plat, renumérotées ; et, paresseusement, les agrégats d'une équipe (`WindowTeam`) et la grille d'accroche par tuile | cache LRU sous budget | 66 à 69 o par chaîne (§4.2) |

La génération (`generate.rs`, `search.rs`, `reverse.rs`, `verify.rs`) garde son code : elle lit un `ViewData` qui est
celui d'une fenêtre au lieu de celui de toute la vue, et un accès au graphe de base qui lit le fichier au lieu d'un
tableau. Le moteur à plat devient le cas particulier « fenêtre de toutes les tuiles ».

## 3. Ce que la génération lit vraiment

L'équivalence ne se démontre pas en général : elle se démontre donnée par donnée. Inventaire de tout ce que lisent la
génération, le contrôle interne et la construction des attributs d'équipe, et de la zone dont chaque lecture dépend.
`R` est l'ensemble des **nœuds atteints** : `note_node` ne voit que des nœuds à l'intérieur du `Disc` (chaque relâchement
teste `disc.inside` sur le nœud d'arrivée de l'arbre aller, de départ de l'arbre retour, et sur les nœuds non terminaux
des deux A*), **plus** les deux bouts de la chaîne de départ et de la chaîne d'arrivée (les états initiaux ne sont pas
testés).

| Lecture | Où | Dépend de | Traitement |
|---|---|---|---|
| `out(x)`, `inn(x)`, `deg[x]` | arbres, A*, `turn_cost`, `retro_ok` | toutes les chaînes incidentes à `x`, `x` ∈ `R` | fenêtre : `x` dans une tuile de la fenêtre ⇒ complet (§5.2) |
| `pos`, `ends`, `len_dm`, `open`, `head`, `links` d'une chaîne | partout | la chaîne seule | fenêtre, recopié tel quel |
| `w_osm[c]` | `Weighter::chain` | les arêtes de base de la chaîne | fenêtre, calculé des attributs de base lus par `mmap` |
| `ChainTeam` (`a`, `b`, `z`, `strict`) | `Weighter::chain` | drapeaux d'équipe des arêtes de la chaîne | `WindowTeam`, depuis les drapeaux **globaux** de `TeamData` (§7) |
| drapeaux d'une arête de base (`TEAM`, `BRIDGED`, `z`) | `Weighter::edge` (accroche), `check_route` | la règle des trous : tout ce qui est à moins de 200 m de réseau | `TeamData` global, construit sur la zone de l'équipe (§7) |
| `comp_len_of_chain` | `snap` (composante ≥ 0,9 D) | **composante fortement connexe de toute la vue** | global creux, au démarrage (§6) |
| `strict_comp_len_of_chain` | `snap` en strict | composante stricte de **tout** le réseau de l'équipe | `TeamData` global creux (§7) |
| grille d'accroche | `snap` | toutes les chaînes dont un point échantillonné tombe dans les cases de 0,01° de la boîte d'accroche | grille paresseuse par tuile dans la fenêtre (§4.3) |
| `base_inc(x).len()` | `admitted_degree` de `check_route` | arêtes vivantes autour d'un nœud de base de l'itinéraire | `deg` du nœud de vue, ou 2 pour un nœud intérieur à une chaîne (§4.5) |
| attributs, extrémités, way, position, géométrie de base | accroche, morceaux partiels, contrôle, cap moyen, recouvrement | l'arête seule | lecture `mmap` (§4.4) |
| `n_chains`, `n_nodes` | tailles des arbres et de `ban` | — | tailles de la fenêtre |
| ordre des états (tas `(coût, état)`), ordre des nœuds (`pick`, `reached`), plus petit id de chaîne (`snap`) | départage | les ids | renumérotation **croissante** (§4.2) |

Rien d'autre ne lit un tableau à l'échelle du graphe. Les trois lignes en gras sont les seules dépendances non locales :
elles restent globales.

## 4. Structures

### 4.1 `Graph` : le global, au démarrage

`Engine::new(base, road, gravel, params)` (le nom est resté `Engine`, pas `Graph`) fait les contrôles d'aujourd'hui
(vues non élaguées, du bon genre, de ce graphe, du zoom de la base) puis, **par vue**, sans recopier la vue :

- **Index des chaînes étrangères** : pour chaque tuile z11, les chaînes qui la **touchent** sans lui appartenir. Une
  chaîne appartient à la cellule de son bout `a` (le plus petit id de nœud de vue : `write_inner` range les arêtes par
  `a`). Elle touche toutes les tuiles des boîtes de ses segments (`tile_of_e7` des deux coins, comme `segment_tiles` de
  `ord-graph-match`), bout `b` compris. On ne décode la géométrie que si la longueur de la chaîne dépasse la distance de
  `a` au bord de sa tuile : 4,7 % des chaînes `road`, 5,3 % des `gravel` [mesuré]. Stockage : `Vec<(clé de tuile, id
  global de chaîne)>` trié, la clé de tuile étant `y << 16 | x` (x, y < 2¹¹ à z11). France : 233 429 entrées `road`,
  428 960 `gravel`, **au plus 5,3 Mo** les deux [calculé ; la sonde compte les tuiles des points, le code comptera celles
  des boîtes de segments, un peu plus].
- **Longueurs de composantes fortement connexes**, creuses (§6) : 2,9 Mo les deux [calculé].
- **Plus longue chaîne** `Lmax` (26,5 km `road`, 15,3 km `gravel`) : elle borne l'accroche (§5.1).

Le tout est **parallélisé** : une vue par fil (`std::thread::scope`, aucune dépendance nouvelle), le balayage de
l'index pouvant aussi se couper par paquets de cellules (`Params::index_threads`, 2 par défaut : un fil par vue).
**Écart à la conception** : c'est `Engine::new` qui lance les fils, et non le service ; `ord-loops` n'est donc plus « sans
fil » à l'ouverture (la génération, elle, n'en lance aucun). Temps relevés : voir les mesures. Le Tarjan travaille sur
un **tampon unique** de successeurs (pas de `Vec` par nœud) : 13 o par nœud, une pile d'appels de 16 o par niveau.

### 4.2 `Window` : une vue à plat, renumérotée

Une fenêtre est construite pour une vue et un ensemble trié de tuiles `T` :

1. **Chaînes** : pour chaque tuile de `T` qui a une cellule dans la vue, ses chaînes possédées (ids globaux contigus
   `first_edge..first_edge + n_edges`), plus les chaînes étrangères de l'index (§4.1). Union triée, sans doublon : c'est
   `gid`, la table **locale → globale**. L'id local d'une chaîne est son rang dans `gid`.
2. **Nœuds** : les bouts de ces chaînes (ids globaux de vue), triés, sans doublon : `ngid`. Un nœud est **complet** si
   sa cellule (celle du fichier, `node_cell`) est une tuile de `T` ; un bit par nœud.
3. Pour chaque chaîne : extrémités locales (dichotomie dans `ngid`), longueur, sens ouverts, liens (recopiés de
   `VIEW_CHAIN` : ids **globaux** d'arêtes de base), caps (lus chaîne par chaîne dans `VIEW_HEADING`, ou recalculés comme
   aujourd'hui pour une vue minor 1), poids sans équipe par type servi (somme de `cost::edge_weight` sur les arêtes de
   base lues par `mmap`).
4. CSR sortant et entrant, `deg`, exactement comme `load_view` (même boucle, chaînes dans l'ordre croissant).

La renumérotation est **croissante** dans les deux tables : l'ordre des ids locaux est celui des ids globaux. Tous les
départages du moteur (tas `(coût, état)`, `pick` sur l'id de nœud, `snap` sur le plus petit id de chaîne, ordre des
listes `out` et `inn`, ordre de `reached`) en sont inchangés. Les étapes d'un parcours restent des arêtes de base
**globales** : elles viennent des liens recopiés. `Diagnostics::sharp_nodes` aussi (nœuds de base).

Le `ViewData` de la fenêtre perd `binc_off`, `binc`, `edge_chain` (ils ne servaient qu'à l'équipe et au contrôle,
§4.5 et §7), `comp` et `comp_len_dm` (§6), et la grille (§4.3) ; il gagne `gid`, `ngid` et le bit de complétude.
Mémoire par chaîne [calculé, rapports `n/c` et liens/`c` mesurés à Paris 100 km] :

| | `road` | `gravel` |
|---|---|---|
| positions (8 o × `n`) | 6,1 | 5,9 |
| extrémités, longueur, sens | 13,0 | 13,0 |
| CSR sortant et entrant | 20,7 | 21,0 |
| décalages et liens de chaîne | 10,3 | 9,4 |
| caps, `deg` | 4,8 | 4,7 |
| poids sans équipe | 4,0 (un type) | 8,0 (gravel, VTT) |
| `gid`, `ngid` | 7,1 | 7,0 |
| **total** | **≈ 66 o** | **≈ 69 o** |

Boucle de 200 km à Paris : 57 Mo en `road`, 89 Mo en `gravel`. Aller simple de 300 km au pire (Lyon, 150 km) : 144 Mo
en `gravel`.

### 4.3 Grille d'accroche paresseuse, par tuile

Aujourd'hui, `warm` construit la grille sur toute la vue (décode toute la géométrie). La fenêtre la construit **par tuile,
à la demande** : pour une tuile `t`, les paires `(case de 0,01°, chaîne locale)` des points échantillonnés (même code :
pas de 400 m par segment, même interpolation entière) **qui tombent dans `t`**, triées. Une accroche réunit les paires
des tuiles qui recouvrent les cases de sa boîte, garde les cases de la boîte, trie et dédoublonne les chaînes : c'est le
même ensemble de candidats que la grille globale (un point échantillonné dans une case de la boîte est dans une des
tuiles qui la recouvrent), parcouru dans le même ordre croissant. Une à quatre tuiles par accroche, ~5 000 à 10 000
chaînes chacune, quelques millisecondes la première fois [estimé] ; gardée dans la fenêtre (`OnceLock` par tuile).

### 4.4 Le graphe de base lu dans le fichier

`BaseData` (804 Mo) disparaît. Ses lecteurs passent par le `GraphFile` projeté : `edge_ends` et la way dans `ID_EDGES`
(accès direct), attributs et longueur par `edge(e)` (dichotomie sur 3 300 cellules, puis l'enregistrement), position par
`node_pos`, géométrie par `geometry`. Ce sont les mêmes octets que ceux que `BaseEdges::load` recopiait : mêmes valeurs.
Aucune boucle chaude ne les lit : les arbres et les A* ne lisent que la vue. Coût [estimé] : quelques milliers de
lectures par candidat contrôlé (≈ 0,1 µs chacune), et une par lien à la construction d'une fenêtre (1,7 M de liens à
Paris 100 km : ~0,2 s).

### 4.5 Le degré admis du contrôle interne sans incidence de base

`check_route` lit `base_inc(x).len()` aux nœuds de l'itinéraire : les arêtes de base vivantes dans la vue autour de `x`.
Toute arête vivante est le lien d'une chaîne (vue non élaguée : `base_edges_covered` = `filtered_edges`, §1.1) ; la
contraction ne fond qu'un nœud qui a **exactement** deux arêtes retenues. D'où la règle, sans aucun tableau :
`admitted_degree(x) = deg[x]` si `x` est un nœud de vue (dichotomie dans `VIEW_BASE_NODE`, puis dans `ngid`), sinon 2.
`deg` sature à 255 et le contrôle ne compare qu'à 2 et 3 : sans effet. Les nœuds de l'itinéraire sont dans `R`, donc
complets. Hypothèse à verrouiller par un test (§14, point 3) : aucune arête de base n'a ses deux bouts au même nœud
(elle compterait deux fois dans `binc`).

## 5. La fenêtre d'une requête

### 5.1 Tuiles nécessaires

Toutes les distances du moteur sont prises dans le repère `Local` du départ (`Disc::new`), affine en (lat, lon). Les
tuiles nécessaires `N(req)` sont celles dont le rectangle, projeté dans ce repère (un rectangle aussi), coupe l'une de
ces zones :

- **le disque de recherche** : boucle, disque de rayon D/2 autour du départ ; aller simple, disque de rayon D/2 autour du
  milieu départ-arrivée, qui contient l'ellipse (`|p−s| + |p−e| ≤ D` ⇒ `|p−m| ≤ D/2`) ; plus une marge `window_margin_m`
  (1 km) contre les arrondis ;
- **les deux zones d'accroche** : la boîte exacte des cases de 0,01° que `snap` balaie (mêmes formules), **élargie de
  `Lmax` de la vue** : une chaîne candidate dont un point est dans la boîte a ses deux bouts à moins de `Lmax` de ce
  point.

Pour une boucle, la deuxième zone est dans la première dès que D/2 ≥ ~30 km ; pour une petite boucle (D = 5 km, le
minimum du contrat), elle domine : ~25 tuiles, 100 000 à 200 000 chaînes [estimé].

### 5.2 Pourquoi la recherche ne sort pas de la fenêtre

Soit `W` ⊇ `N(req)` l'ensemble de tuiles de la fenêtre qui sert la requête.

1. Un nœud de vue `x` d'une tuile de `W` est **complet** : toute chaîne incidente à `x` touche la tuile de `x` (son
   polyligne passe par `x`), donc elle est possédée par cette tuile ou listée comme étrangère (§4.1) ; elle est chargée.
   Ses `out`, `inn` et `deg` sont ceux de la vue entière.
2. Tout nœud de `R` est dans le disque de recherche (tests `disc.inside`) ou bout d'une chaîne d'accroche, donc dans une
   tuile de `N(req)` (§5.1) : il est complet.
3. Toute chaîne lue par la recherche part d'un nœud de `R` ou y arrive : elle est chargée, avec ses données propres.
4. Toute chaîne candidate à l'accroche a un point échantillonné dans la boîte d'accroche ; ce point est dans une tuile de
   la boîte, la chaîne la touche, elle est chargée ; la grille paresseuse la trouve (§4.3).

La recherche **ne peut donc pas toucher le bord** : un nœud incomplet est hors du disque et n'est jamais fixé ; s'il est
la cible d'une chaîne, le test `disc.inside` l'écarte avec la position recopiée, comme dans le moteur à plat.

**Garde-fou** (une preuve se trompe aussi) : `note_node` et les deux `expand` des A* testent le bit de complétude du nœud
qu'ils fixent ou dont ils partent (un bit par nœud, sans coût mesurable). Un nœud incomplet lève `ws.escaped`. Le service
jette alors le résultat, compte `ord_generator_window_escapes_total`, et refait la génération une fois sur une fenêtre de
`N(req)` élargie de `Lmax` + 10 km ; un second échappement rend 500 `internal` avec un `error` sans coordonnée. Les tests
et le banc exigent zéro échappement.

### 5.3 Marge

La marge de 1 km ne porte que des arrondis (le repère `Local`, les rectangles de tuile en E7) : la recherche n'a besoin
d'aucun « tampon de réseau » autour du disque, puisque ses tests de bord sont géométriques et faits sur des positions
exactes. Les dépendances à longue portée (composantes, règle des trous) ne passent pas par la fenêtre (§6, §7).

### 5.4 Une fenêtre plus grande rend le même parcours

Si `W` ⊇ `N(req)`, les points 1 à 4 du §5.2 tiennent, et les chaînes et nœuds en plus n'apparaissent dans aucune lecture
du §3 (ils sont hors de `R`, et l'accroche ne les voit pas : hors de la boîte). La renumérotation reste croissante.
**Toute** fenêtre qui contient `N(req)` rend le même résultat, y compris la fenêtre de toutes les tuiles, qui est le
moteur à plat. C'est ce qui permet le cache (§10) et l'essentiel du plan de tests (§14).

### 5.5 Clé de cache et réemploi

Pour qu'une équipe qui part presque toujours du même lieu retombe sur la même fenêtre, la fenêtre bâtie pour une requête
n'est pas `N(req)` mais une **classe** qui le contient : centre = la tuile du départ (boucle) ou du milieu (aller
simple) ; rayon = le rayon utile de la requête vu du centre de cette tuile, arrondi au pas `window_radius_step_m`
(10 km) au-dessus ; tuiles = celles qui coupent ce disque, **réunies à `N(req)`** (l'arrondi ne peut donc jamais perdre
une tuile). La recherche d'une fenêtre en cache est un test d'inclusion `N(req)` ⊆ `W` (fusion de deux listes triées) sur
les fenêtres de la même vue ; la plus petite qui convient sert. Le centre et le rayon ne sont qu'une manière de choisir
`W` ; seule l'inclusion compte pour le résultat.

Aller simple : le disque de rayon D/2 autour du milieu couvre l'ellipse avec du gaspillage quand le départ et l'arrivée
sont loin l'un de l'autre (à 220 km l'un de l'autre pour 300 km, l'ellipse est 32 % plus petite). On garde le disque :
le pire cas (départ et arrivée proches) est un disque de toute façon, et le budget est dimensionné sur lui.

## 6. Composantes connexes et accroche

`snap` refuse une chaîne dont la composante fortement connexe (sens respectés, virages ignorés) fait moins de 0,9 D ;
la composante est celle de **toute** la vue. Une fenêtre peut couper une composante (une boucle de sens uniques qui ne se
referme que dehors) : sa longueur locale est un minorant, et le seuil changerait d'avis. Elle reste donc globale.

Au démarrage, par vue, Tarjan itératif **sur le CSR des cellules du fichier** (les arêtes orientées de la vue y sont déjà,
cibles en ids globaux de nœud) : pas de copie de la vue. Puis, pour chaque chaîne, `L(c)` = longueur de la composante si
ses deux bouts y sont, sinon 0 (exactement `chain_strong_len`). On ne garde que les chaînes où `L(c) < CAP`, en
`Vec<(id global de chaîne, L(c))>` trié ; une chaîne absente vaut « au moins `CAP` ». La comparaison `L(c) ≥ min_dm` est
exacte tant que `min_dm ≤ CAP`, avec `CAP = ⌈min_component_share × max_distance_m × 10⌉` dm. France, à `CAP` = 180 km :
144 154 + 213 181 chaînes, **2,9 Mo** [mesuré, calculé]. Le nouveau réglage `Params::max_distance_m` vaut 400 km par
défaut ; le service refuse de démarrer si `distance_m_max` le dépasse (une demande au-delà ne serait plus exacte). Les
composantes de plus de 360 km sont aussi rares que celles de plus de 180 km : le tableau ne grossit pas [estimé, à
relever].

En strict, la même règle porte sur la composante du réseau strict de l'équipe : globale aussi, dans `TeamData` (§7).

## 7. Données d'équipe : globales creuses, puis par fenêtre

### 7.1 `TeamData`, une fois par surcouche

`build_type` d'aujourd'hui fait trois choses de portées différentes :

1. **les drapeaux par arête de base** (`TEAM`, `BRIDGED`, `z`) : la règle des trous lance, depuis chaque cul-de-sac
   fictif, un Dijkstra de 100 m au plus sur les arêtes vivantes, et les jonctions de 30 m ; le drapeau d'une arête ne
   dépend que du réseau à moins de 200 m d'elle ; la normalisation de `z` dépend du maximum sur **toute** la surcouche ;
2. **les agrégats par chaîne** (`ChainTeam`) : fonction des seuls drapeaux de ses liens ;
3. **les composantes strictes** : fonction de **toutes** les chaînes strictes de l'équipe.

(1) et (3) se calculent sur la **zone de l'équipe**, puis se gardent en ids globaux, creux :

- **Zone** : les tuiles qui contiennent un point (bout ou point intérieur) d'une arête de la surcouche, et celles que
  touche la boîte de ± 1 km autour de chacun de ces points. Une fenêtre de la vue (§4.2) est construite sur ces tuiles,
  avec en plus, pour ce seul usage, l'incidence des arêtes de base vivantes (`binc`, indexée par le rang du nœud de base
  dans la liste triée de ses nœuds) et la table arête de base → chaîne locale (liste triée des liens).
- **Exactitude** : un Dijkstra des trous part d'un nœud d'équipe et ne dépasse pas 100 m, donc reste dans la boîte de
  ± 1 km de ce nœud, donc dans des tuiles de la zone, dont les nœuds sont complets (§5.2, point 1) ; un nœud intérieur
  atteint l'est par une chaîne chargée. Toutes les arêtes de la surcouche y sont (la normalisation de `z` et `outside_m`
  sont exactes). Une chaîne stricte a tous ses liens d'équipe ou comblés, donc dans la zone : elles y sont **toutes**, et
  le Tarjan restreint aux chaînes strictes de la zone donne les composantes globales.
- **Ordre** : les Dijkstra gardent les ids **globaux** de nœud de base dans le tas et dans `dist` (départage inchangé),
  et `binc` de la zone liste les arêtes d'un nœud dans l'ordre croissant des arêtes, comme aujourd'hui.
- **Résultat, par type** : `edges: Vec<(arête, drapeaux, z)>` trié (comme aujourd'hui), `strict_len: Vec<(id global de
  chaîne stricte, longueur de sa composante stricte)>` trié, et les statistiques. Quelques dizaines de milliers d'entrées :
  ~1 à 3 Mo par équipe [estimé], au lieu de 736 Mo.
- **Coût** : la zone d'une équipe de 132 km (F) tient en une poignée de tuiles ; une grande équipe peut toucher 300 tuiles
  (un huitième de la France), ~2 à 3 M de chaînes `gravel`, ~50 o par chaîne transitoires (~150 Mo), compté dans le
  budget le temps de la construction (§10). Moins de 2 s pour la plus grande des neuf [estimé, à relever].

### 7.2 `WindowTeam`, par fenêtre et par équipe

Pour une fenêtre et une équipe : `ChainTeam` pour chaque chaîne locale et chaque type servi par la vue, par le même calcul
qu'aujourd'hui, les drapeaux de chaque lien étant lus par dichotomie dans `TeamData::edges` (~50 ms pour 1,7 M de liens
[estimé]). 12 o par chaîne et par type : 31 Mo pour une boucle de 200 km en `gravel` à Paris. Construit à la première
génération de l'équipe dans la fenêtre, gardé dans la fenêtre sous le budget. `strict_comp_len_of_chain(c)` lit
`TeamData::strict_len` à l'id global `gid[c]`. `Weighter::edge` et `check_route` lisent `TeamData::flags`, global, comme
aujourd'hui.

## 8. Contrôleur indépendant et index de dépôt sur la France

### 8.1 Le contrôleur, borné à l'équipe et à l'itinéraire

Le contrôleur garde sa logique, écrite depuis le §6 du lot 2 seulement ; seule change l'étendue de ses tables :

- **par équipe** (`TeamCheck`, construit avec `TeamData` et gardé dans l'entrée du cache d'équipes) : les arêtes comblées
  par type, recalculées par sa propre règle d'admission, sur la zone de l'équipe (même définition qu'au §7.1, sur le
  graphe de base) ; l'incidence de chaque nœud de la zone vient des arêtes possédées par les tuiles de la zone **et leurs
  voisines**, plus les arêtes larges (§8.2) : une arête possédée par la cellule de `a` a son bout `b` dans cette cellule ou
  une voisine tant qu'elle n'est pas plus large qu'une cellule ;
- **par itinéraire** : le degré admis des seuls nœuds de l'itinéraire, par la même incidence sur les tuiles de
  l'itinéraire et leurs voisines (un itinéraire de 300 km touche ~30 tuiles, ~250 avec les voisines : quelques millions
  d'arêtes balayées, ~20 à 40 ms [estimé]) ; le degré d'équipe depuis la surcouche.

`Checker::new` prend en plus l'éventuel `TeamCheck`. Le banc compare l'ancien et le nouveau contrôleur rapport pour
rapport (§14, point 8).

### 8.2 Arêtes plus larges qu'une cellule

`CellIndex::fits` devient une **liste d'exceptions** : les arêtes dont un segment déborde des voisines de leur cellule.
Une arête plus courte que la largeur d'une tuile ne peut pas déborder ; à z11 une tuile fait au moins 12,3 km en France
(51,1° N), donc on ne décode que les arêtes de base de plus de 10 km (marge prise) : un balayage des enregistrements, ~0,3 s au lieu de
16 s [estimé]. Sur la France, la liste a **une** arête [mesuré]. `CellIndex` l'ajoute à chaque cellule que touchent ses
segments ; le contrôleur l'ajoute à l'incidence de ses deux bouts. Le repli sur l'index entier ne sert plus que si la
liste dépasse un plafond (1 000 arêtes), avec le `warn` d'aujourd'hui. On ne coupe pas l'arête dans le graphe : ce serait
changer les ids, donc l'identité du graphe et toutes les surcouches.

## 9. Espace de travail

Le `Workspace` garde un jeu de trois arbres et un `ban` par vue, dimensionnés sur la fenêtre : 118 o par chaîne en `road`
comme en `gravel` [calculé]. **Relevé : 142 à 146 o** (les listes d'états fixés grossissent) : 188 Mo pour une boucle de
200 km en `gravel` à Paris, 316 Mo pour l'aller simple de 300 km au pire (Lyon) [mesuré]. C'est **plus que la fenêtre
elle-même**. Règles :

- `Tree::begin` et `ban` ne réallouent plus quand la taille change d'un iota : ils gardent leurs tableaux si
  `n ≤ capacité ≤ 2 n`, et n'en lisent que le préfixe. L'estampille croît strictement, une case d'un usage précédent
  n'est jamais lue comme fixée : même résultat ;
- au retour dans la réserve, un espace de travail de plus de `workspace_keep_mb` (160 Mo) est libéré ;
- ses octets (capacités réelles) sont comptés dans le budget, en usage comme en réserve.

## 10. Cache, budget mémoire, concurrence

### 10.1 Budget

Un seul compte, `memory_budget_mb` (1 000 Mo par défaut depuis la finition, §18 ; 900 avant), pour tout ce que le
générateur alloue au-delà du fixe : fenêtres, `WindowTeam`, espaces de travail, constructions transitoires de `TeamData`
et `TeamCheck`, et **équipes résidentes** (§18). Chaque objet déclare ses octets
**après** construction (somme des capacités de ses `Vec`, exacte) ; avant de construire, on **réserve** une estimation
(chaînes des tuiles, lues dans l'index des cellules et l'index des étrangères, fois le coût par chaîne du §4.2), corrigée
à la fin.

Réserver : si le budget libre ne suffit pas, on évince les objets inutilisés (personne d'autre ne tient leur `Arc`) du
moins récemment servi au plus récent ; si cela ne suffit toujours pas, on attend qu'une génération en cours rende sa part,
dans le budget de temps de la requête ; à l'échéance, 503 `overloaded` avec `Retry-After` (code déjà au contrat). Une
fenêtre qui dépasserait à elle seule le budget n'arrive pas avec les bornes du contrat (le pire, 440 Mo avec son
espace de travail et son équipe, §4.2, §7.2 et §9).

Ordre de grandeur [calculé] : trois équipes en cache, chacune avec sa fenêtre `gravel` de boucle de 200 km et son
`WindowTeam` (120 Mo), plus deux espaces de travail : ~660 Mo. Le conteneur : altitude et rues (deux caches de 256 Mo),
index des dépôts (128 Mo), service (~0,1 Go), générateur 0,9 Go : ~1,75 Go d'anonyme au pire, le reste en pages de
fichier, récupérables.

### 10.2 Concurrence

- `Graph`, les fenêtres et `TeamData` sont immuables après construction et partagés par `Arc` ; la génération reste du
  CPU pur sous le sémaphore `max_concurrent` existant.
- Une fenêtre n'est construite qu'**une** fois par clé : une seconde demande attend la première puis relit le cache
  (même motif que les constructions de `TeamData` dans `store.rs`). La grille par tuile (`OnceLock`) et `WindowTeam`
  (table protégée, une construction par équipe) aussi.
- La construction d'une fenêtre se fait dans la tâche bloquante de la génération, dans son budget de 8 s ; elle lit le
  `stop` entre deux tuiles et s'abandonne sans rien laisser en cache.
- Le cache ne change jamais un résultat (§5.4) : deux requêtes identiques servies par deux fenêtres différentes rendent
  le même parcours.

## 11. Configuration (`ORD_GENERATOR__*`)

| Clé | Défaut | Rôle |
|---|---|---|
| `MEMORY_BUDGET_MB` | 900 → **1 000** | budget du §10.1 ; la plus grosse équipe de la France en réserve 924 pour se construire (§18) |
| `WINDOW_MARGIN_M` | 1 000 | marge d'arrondi autour des zones (§5.1) |
| `WINDOW_RADIUS_STEP_M` | 10 000 | pas d'arrondi du rayon de classe (§5.5) |
| `WORKSPACE_KEEP_MB` | 160 | au-delà, un espace de travail rendu est libéré (§9) |
| `INDEX_THREADS` | 2 | fils du calcul des index au démarrage (§4.1) |
| `TEAM_CACHE_ENTRIES` | 3 → **16** | plafond **en nombre** ; les octets des équipes (de 0,2 à 16 Mo chacune sur la France) sont sous le budget, qui en évince la moins récente quand la place manque (§18) |

`distance_m_max` doit rester sous `max_distance_m` du moteur (400 km) : sinon le service démarre, générateur éteint,
`reason = not_configured` et un `warn`. Rien de nouveau dans `/v1/capabilities` (contrat inchangé).

## 12. Métriques et journaux

Compteurs et histogrammes, sans étiquette de lieu :

- `ord_generator_window_requests_total{view, outcome = hit|built|wait}` ;
- `ord_generator_window_build_seconds{view}`, `ord_generator_window_chains{view}` (taille des fenêtres construites) ;
- `ord_generator_window_evictions_total{kind = window|window_team|workspace}` ;
- `ord_generator_memory_bytes{kind = window|window_team|workspace|team_build|team_cache}` et `ord_generator_memory_budget_bytes` ;
- `ord_generator_team_evictions_total{reason = budget|count}` (équipes évincées : par le budget, par le plafond en nombre) ;
- `ord_generator_budget_wait_seconds`, et l'issue 503 déjà comptée par `ord_generator_requests_total` ;
- `ord_generator_window_escapes_total` (doit rester à 0) ;
- au démarrage, `generator ready` porte en plus `index_ms` et `scc_ms`.

**Aucune tuile, aucun centre, aucun rayon de classe n'est journalisé ni étiqueté** : une tuile z11 dit à 14 km près
d'où part une équipe. Les tailles (chaînes, octets) et la vue suffisent à l'exploitation.

## 13. Ce qui garantit l'équivalence

Le moteur fenêtré rend la même liste d'étapes (et les mêmes issues, positions de départ et d'arrivée, mesures et
diagnostics) que le moteur à plat parce que :

1. **le code de génération est le même** ; il ne change que la provenance de ses tableaux ;
2. **chaque lecture du §3 rend la même valeur** : données propres d'une chaîne recopiées ; adjacence et degré complets en
   tout nœud de `R` (§5.2) ; drapeaux, `z` et composantes strictes globaux (§7.1) ; composantes de la vue globales et
   exactes sous `CAP` (§6) ; candidats d'accroche identiques (§4.3) ; données de base lues aux mêmes octets (§4.4) ;
   degré admis par une identité vérifiée (§4.5) ;
3. **tous les départages sont conservés** par la renumérotation croissante (§4.2) et par les ids globaux gardés là où le
   moteur à plat en avait (étapes, nœuds de base, tas des trous) ;
4. **les arrondis sont les mêmes** : mêmes formules, mêmes types, même ordre des sommes (le poids d'une chaîne se somme
   dans l'ordre de ses liens, `ChainTeam` aussi) ;
5. **une fenêtre plus grande ne change rien** (§5.4), donc ni la classe, ni l'état du cache ;
6. **le garde-fou** de complétude rend visible, au lieu de silencieuse, toute faille de la preuve.

## 14. Plan de tests

Synthétiques (`cargo test --workspace`), sur les graphes de `tests.rs` et sur un graphe plus grand fabriqué pour le lot :
une grille de 60 × 60 nœuds à sens uniques, attributs et trous tirés d'une graine fixe, construite à **z16** (tuiles de
~400 m) pour qu'une requête de quelques kilomètres ait une vraie fenêtre partielle ; paramètres de fenêtre réduits
(marge 20 m, pas 500 m).

1. **Équivalence** : sortie figée du moteur à plat d'avant le lot (fixture commitée ; pas de module `legacy`, §19),
   fenêtre de toutes les tuiles, fenêtre `N(req)`, fenêtre de classe : mêmes `Result<Generation, Failure>`
   (étapes, positions, mesures, `Diagnostics` champ par champ), sur 500 requêtes tirées (boucles et allers simples, trois
   modes, trois types, avec et sans cap, D de 1 à 15 km) et sur les tests existants.
2. **Surensemble** : la même requête sur dix fenêtres tirées entre `N(req)` et tout : mêmes résultats ; zéro échappement.
3. **Identités des index**, comparées à la force brute : chaînes chargées pour une tuile = chaînes dont un segment touche
   la tuile ; `L(c)` creux = `chain_strong_len` du Tarjan à plat pour chaque chaîne ; `admitted_degree` (§4.5) =
   `base_inc(x).len()` pour chaque nœud de base vivant ; aucune arête de base `a == b`.
4. **Équipe** : `TeamData` sur la zone = `TeamData` à plat (drapeaux, `z`, statistiques, longueur stricte de chaque chaîne
   stricte) ; `WindowTeam` = `ChainTeam` à plat chaîne par chaîne ; une surcouche dont un trou chevauche une frontière de
   tuile, une équipe sur deux zones disjointes, une composante stricte qui sort de la fenêtre d'une requête (la longueur
   reste la globale), une composante de vue qui ne se referme que hors de la fenêtre.
5. **Bord** : une chaîne plus longue que la marge qui sort du disque ; un départ à moins de 2 km d'une chaîne de `Lmax` ;
   un garde-fou provoqué (fenêtre volontairement trop petite) qui doit lever `escaped`.
6. **Espace de travail** : la même requête après une fenêtre plus grande puis plus petite (préfixe) : même résultat.
7. **Cache et budget** (`ord-server`, synthétique) : éviction du moins récent inutilisé seulement ; attente puis 503 quand
   tout est tenu ; une construction par clé sous concurrence ; deux requêtes servies par deux fenêtres différentes rendent
   le même corps ; comptes d'octets égaux aux capacités.
8. **Contrôleur** : `Checker` borné = `Checker` global (rapports égaux) sur les graphes synthétiques et une arête plus
   large qu'une cellule (§8.2) ; `CellIndex` avec exception = index entier, sur les requêtes de proximité du dépôt.

Banc (`ord-loops-bench`, hors CI, données privées locales) :

9. **Région** : les 323 requêtes × 4 crans du curseur, moteur à plat et moteur fenêtré dans le même processus, comparaison
   **exacte** des étapes et des issues ; plus `--self-check` (points 3 et 4 sur le vrai graphe et les neuf surcouches).
10. **France** : les mêmes requêtes sur le graphe France, à plat (poste de 30 Go) et fenêtré ; mêmes étapes. Les ids de
    la France diffèrent de ceux de la région : on compare France à plat contre France fenêtrée, et la géométrie
    France contre la géométrie région à titre indicatif.
11. **Mémoire** : banc fenêtré France neuf équipes, et service France sous `systemd-run --user --scope -p MemoryMax=2G
    -p MemorySwapMax=0`, neuf dépôts puis la campagne par HTTP et la charge du lot 3a (4 puis 12 clients) ; relevés : RSS
    max (`/usr/bin/time -v`), `RssAnon` échantillonné, `memory.peak` et `anon` / `file` de `memory.stat`, budget compté.
12. **Temps** : médiane et p90 à chaud au banc région (référence 255 ms) ; fenêtre à froid pour D = 100 et 200 km, en
    `road` et `gravel`, pages chaudes et pages évincées ; démarrage du service ; préparation d'équipe.

Critères : 0 écart aux points 1, 2, 9, 10 ; 0 échappement ; service France ≤ 2 Go sans OOM ni 5xx autre que 503 de
charge, générateur ≤ 1,2 Go de tas compté avec plusieurs équipes en cache ; génération à chaud du même ordre (≤ +10 %) ;
fenêtre à froid ≤ 2 s pour un rayon de 100 km ; démarrage ≤ 6 s hors lecture disque froide.

## 15. Étapes de code, dans l'ordre

Chaque étape finit par `cargo fmt`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test
--workspace --locked` verts (Rust stable à jour), et un commit. Aucune dépendance nouvelle n'est prévue (`std::thread`,
`rustc-hash` et `memmap2` sont déjà là) ; si une arrive, `cargo deny check`.

0. **Référence figée** : sortie du moteur d'aujourd'hui sur les graphes synthétiques (dont la grille z16) en fixture
   commitée (synthétique, rien de privé) ; banc région complet en local comme référence. *Vérif. : la fixture se relit.*
1. **`ord-graph`** : lecteurs sans allocation (`view_heading(c)`, recherche dans `VIEW_BASE_NODE`), aides de tuiles
   (rectangle E7, clé `y << 16 | x`, cellule d'une tuile). *Vérif. : tests unitaires.*
2. **`Graph` et index** (`ord-loops/src/graph.rs`) : contrôles d'ouverture, index des étrangères, Tarjan sur le CSR des
   cellules, `L(c)` creux, `Lmax`. *Vérif. : point 3 du §14 ; banc `--self-check` région ; temps et mémoire France.*
3. **Base lue par `mmap`** : `BaseData` remplacé par un accès au `GraphFile` dans tout le moteur (le moteur à plat aussi).
   *Vérif. : fixture de l'étape 0 et banc région identiques.*
4. **`Window`** : construction, grille par tuile, bit de complétude et garde-fou, `N(req)`, point d'entrée
   `generate(&Graph, &Window, …)` ; le moteur à plat d'aujourd'hui passe en `legacy` (tests et fonction du banc).
   *Vérif. : points 1, 2, 5, 6 du §14.* (Faite avec les étapes 3 et 5, §19.)
5. **`TeamData` sur la zone, `WindowTeam`**. *Vérif. : points 1, 2 et 4 du §14 dans les trois modes.*
6. **Espace de travail** : préfixe, octets comptés. *Vérif. : point 6.*
7. **Banc** : `--engine flat|window`, `--compare`, cache LRU sous budget, section « Fenêtres » du rapport (tailles,
   construction à froid, échappements), `RssAnon`. *Vérif. : point 9 du §14 sur la région, 0 écart.*
8. **Contrôleur borné** et **exceptions de `CellIndex`**. *Vérif. : point 8, puis banc région, rapports du contrôleur
   identiques.*
9. **Service** : `Graph` au démarrage (index en parallèle), cache de fenêtres et budget, `TeamData` et `TeamCheck` dans
   l'entrée d'équipe, configuration, métriques. *Vérif. : point 7 ; tests d'API existants inchangés ; contrat OpenAPI
   identique octet pour octet.*
10. **Relevés France** : points 10 à 12 du §14, consignés dans `ord-loops-lot6-mesures.md`.
11. **Ménage** : pas de `legacy` à retirer (§19) ; note du lot 3a (configuration) et `README` mis à jour.

## 16. Risques

- **Une dépendance globale oubliée** : l'inventaire du §3 a été fait sur le code du 16 novembre ; une règle ajoutée plus
  tard qui lirait un voisinage large casserait l'équivalence en silence. Parades : garde-fou, test de surensemble en CI,
  et toute nouvelle lecture ajoutée au tableau du §3.
- **Pages de fichier au démarrage** : l'index et le Tarjan lisent les deux vues entières (2,3 Go) dans un cgroup de 2 Go ;
  les pages sont propres et récupérées au fil de l'eau, mais un disque lent allonge le démarrage. Repli si les relevés le
  demandent : persister index et longueurs dans un fichier dérivé, à côté des surcouches, vérifié par l'empreinte des
  vues (pas un changement de format des fichiers existants).
- **Fragmentation de `malloc`** (relevé : `ord-loops-lot6-mesures.md` §2.4, **confirmé et chiffré** : 400 à 600 Mo d'anonyme de
  plus sans `MALLOC_MMAP_THRESHOLD_=1048576`, `memory.peak` 1 741 Mo contre 1 076 Mo) : glibc ne rend au système que les blocs alloués par `mmap` (au-delà d'un seuil dynamique
  plafonné à 32 Mo) ; des fenêtres évincées peuvent laisser un RSS au-dessus du budget compté. Parade :
  `MALLOC_ARENA_MAX=2` et `MALLOC_MMAP_THRESHOLD_=1048576` dans l'environnement du conteneur, et un test d'éviction en
  boucle qui compare `RssAnon` au budget.
- **Pire cas sous charge** : deux allers simples de 300 km en `gravel` dans deux régions denses tiennent tout juste
  (2 × 440 Mo) ; avec trois générations ou un budget plus petit, la troisième attend ou reçoit un 503.
- **Fenêtre à froid sur disque froid** : une fenêtre de 100 km lit ~10 % des fichiers (~0,3 Go) ; sur un disque réseau,
  la première génération d'une équipe après un redémarrage peut approcher le budget de 8 s.
- **Zone d'une très grande équipe** : une équipe qui roule dans toute la France a une zone de toute la France ; sa
  `TeamData` se construirait alors sur ~20 M de chaînes (~1 Go transitoire). Parade : construire la zone **par
  composantes de tuiles** (paquets de tuiles voisines, la règle des trous ne traverse pas un écart de 1 km), une à la fois,
  et réunir les résultats ; à faire à l'étape 5 si l'une des neuf équipes dépasse 300 tuiles.
- **`CAP`** : relever `distance_m_max` au-delà de 400 km impose de relever `max_distance_m` (tableau creux un peu plus
  gros, refusé au démarrage sinon).

## 17. Décisions qui reviennent au propriétaire

1. **Le budget du générateur** (`memory_budget_mb`, 900 Mo proposés, 1 000 depuis la finition) dans le conteneur de 2 Go partagé avec l'altitude :
   c'est un partage entre deux usages du même conteneur.
2. **La borne de distance** : le contrat dit 200 km ; la cible du plan parle d'un aller simple de 300 km. Le moteur est
   conçu jusqu'à 400 km (`max_distance_m`) ; relever `distance_m_max` du service est une décision de produit, hors de ce
   lot.
3. **L'environnement du conteneur** (`MALLOC_ARENA_MAX`, `MALLOC_MMAP_THRESHOLD_`) est posé par le `Dockerfile` ; le service journalise un avertissement s'il manque . Un déploiement qui
   remplace l'environnement de l'image (Pédalons) doit les reprendre.
4. **Le 503 sous charge extrême** (§16) plutôt qu'une limite de concurrence par taille de requête : acceptable ou non.

## 18. Sonde et reproduction (hors dépôt)

La sonde de cette note est un petit binaire jetable (dépendance `ord-graph` par chemin, `features = ["mmap"]`) dans le
répertoire de travail de la session, hors dépôt : balayage de l'index des étrangères, Tarjan itératif sur le CSR des
cellules et comptage des fenêtres autour de quatre centres-villes, puis un balayage façon `CellIndex::fits` du graphe de
base. Commandes : `probe <vue>.ordv` et `fits <base>.ordg`, sous `/usr/bin/time`, sur les fichiers de
`~/.cache/osm-route-details/graph/lot6/`. Le RSS de la sonde (0,78 et 1,30 Go) est surtout fait des pages des vues
qu'elle lit ; seuls les temps et les comptes sont repris ici.

## 19. Écarts entre cette note et le code (corrigés dans le texte quand le texte était faux)

Le code a suivi la note ; ce qui a changé, et pourquoi :

1. **Pas de module `legacy`** (§14 point 1, §15 étapes 4 et 11). Garder l'ancien moteur à plat à côté du nouveau aurait
   dupliqué `search.rs`, `generate.rs`, `verify.rs` et `team.rs` (3 500 lignes) pour un seul usage, la comparaison. La
   référence est autre : (a) une **fixture commitée**, écrite avec le moteur à plat **avant** d'y toucher
   (`crates/ord-loops/tests/fixtures/window_reference.txt`, 140 requêtes sur un graphe synthétique de 48 × 48 carrefours
   à z11 et z16, empreinte de la sortie complète : étapes, positions, mesures, diagnostics) ; (b) la fenêtre de toutes les
   tuiles (`Engine::full_window`), qui **est** le moteur à plat (§5.4), comparée à la fenêtre fenêtrée dans le même
   processus (`--compare` du banc) ; (c) le binaire d'avant le lot (tag `v1.9.0`, plus une option `--dump` de huit
   lignes appliquée hors dépôt) dont les empreintes par requête sont comparées à celles de ce binaire (`--dump`).
2. **`Graph` s'appelle `Engine`** (§2, §4.1) : le type public existait, ses appelants (service, banc, contrôleur, tests)
   l'importent ; il ne porte plus que les index globaux. Les fils des index sont lancés par `Engine::new`.
3. **Étapes 3, 4 et 5 en un seul commit** (§15) : retirer `BaseData` (étape 3) sans fenêtre ne laissait pas de moteur qui
   tourne, et `TeamData` (étape 5) lisait `edge_chain` et `binc` des vues à plat ; les trois ne se séparent pas en
   commits qui compilent.
4. **Garde-fou** (§5.2) : il se lève en renvoyant `Failure::WindowEscape` ; le service refait la génération une fois
   dans une fenêtre élargie de la plus longue chaîne et de 10 km (`Engine::plan_with`).
5. **`Failure::TooLong`** (§6) : la distance demandée dépasse `Params::max_distance_m` (400 km) ; le service ne l'émet
   jamais (il refuse de démarrer si `distance_m_max` la dépasse).
6. **Le budget compte aussi la construction des données d'équipe** (§10.1, `team_build`) : `TeamData::plan_build` estime
   sa mémoire transitoire (210 o par chaîne de la zone, relevé sur les neuf équipes de la France) et le magasin la
   réserve avant de construire. La note l'annonçait ; le premier code l'avait oublié.
7. **Ce qui n'est pas compté ni borné** : les agrégats d'équipe d'une fenêtre sont construits **hors du verrou** du cache
   (deux générations simultanées de la même équipe dans la même fenêtre neuve peuvent les calculer deux fois, la
   seconde copie étant jetée) ; la mémoire d'une génération est réservée à 150 o par chaîne de fenêtre **après** son
   acquisition ; la gauge `ord_generator_memory_bytes{kind=workspace}` ne porte que la réserve (pas les espaces en
   cours d'usage).
8. **Zone d'une grande équipe construite d'un bloc** (§16, risque « zone d'une très grande équipe ») : la construction
   par paquets de tuiles n'est **pas** faite. La plus grande équipe de la France (D) demande 704 Mo transitoires et 3,1 s
   (mesuré) ; une équipe qui roulerait dans toute la France dépasserait le budget et recevrait 503.
9. **Contrôleur** (§8) : les types s'appellent `GraphIndex` (arêtes plus larges qu'une cellule, une fois par graphe) et
   `TeamCheck` (arêtes comblées, une fois par équipe, dans l'entrée du cache d'équipes) ; `Checker::with_tables` les
   reçoit, `Checker::new` les construit au premier besoin (tests, outils).
10. **`CellIndex`** (§8.2) : `CellIndex::exceptions` relit les arêtes de plus de la moitié de la plus petite tuile ;
    `CellIndex::new` les relit à chaque création (~0,3 s sur la France par dépôt), le service n'a plus que le plafond de
    1 000 arêtes comme repli.
11. **Banc** : `--engine window|flat`, `--compare`, `--self-check`, `--dump`, `--budget-mb`, `--radius-step-m` ; la
    section « Fenêtres » est faite de lignes d'en-tête du résumé (pas d'un tableau), avec `RssAnon` échantillonné par phase.
12. **Espace de travail réservé à 150 o par chaîne** (§9, relevé sur la France : 142 à 146 o). L'estimation de la mémoire de
    construction d'une équipe est de 210 o par chaîne de la zone (6).
13. **Le polygone de Geofabrik ne passe pas le contrôle d'étendue sur la France** : voir les relevés §4.

## 18. Finition (16 novembre 2026)

Constats non bloquants de la seconde relecture (relevés : [`ord-loops-lot6-mesures.md`](ord-loops-lot6-mesures.md) §4 et
§8), version du paquet 1.9.1 (api inchangée, 1.9).

1. **Les équipes résidentes entrent dans le budget unique.** Le magasin (`store.rs`) déclare à `WindowManager` ce qui
   entre (`TeamMemory::hold_team`) et ce qui sort (`release_team`) ; l'entrée d'une équipe vaut `size_of::<Entry>()` +
   `Overlay::bytes()` (capacités des tableaux de la surcouche **déchiffrée**, sans le texte clair) + `TeamData::bytes()`
   + `TeamCheck::bytes()` (alvéoles réelles des tables de hachage : 5 o par alvéole, les 8/7 de la capacité). Quand la
   place manque (réservation d'une fenêtre, d'un espace de travail, d'une construction d'équipe, ou entrée neuve qui
   dépasse), `WindowManager` évince **d'abord les fenêtres** que personne ne tient, **puis les équipes** que personne ne
   tient (`TeamEvictor::evict_idle`, la moins récemment utilisée d'abord, ses agrégats par fenêtre avec elle). Une
   équipe tenue par une génération n'est jamais évincée (sa mémoire ne reviendrait pas) ; une équipe évincée se
   reconstruit à la demande comme après un redémarrage. `team_cache_entries` reste un plafond en nombre (la plus ancienne
   part, tenue ou non : sa mémoire revient quand la génération qui la tient finit). Une entrée neuve est comptée **après**
   la libération de sa mémoire transitoire, sinon elle chasserait des équipes avec sa propre construction. Verrous :
   `Inner` du gestionnaire puis cache du magasin, jamais l'inverse (le magasin n'appelle le gestionnaire que cache
   relâché). Pourquoi fenêtres d'abord : une fenêtre se reconstruit en moins d'une seconde (0,75 s au plus), une équipe en
   0,1 à 5 s et à chaque fois sur le disque.
2. **`TeamCheck` réserve sa mémoire transitoire.** `TeamCheck::plan` (comme `TeamData::plan_build`) donne les cellules de
   la zone et l'estimation : 16 o par arête des cellules (liste d'incidence) + 24 o par arête de la surcouche ; le magasin
   réserve la **somme** des deux estimations, les deux constructions tournant en parallèle.
3. **Le banc construit comme le service** (en parallèle, avec la réservation de la somme) et rapporte les octets résidents
   par équipe.
4. **Découverte sans mémoire d'équipe : 503.** Quand la réservation de la construction d'équipe échoue à l'échéance,
   toute génération avec une clé d'équipe répond 503 `overloaded` (avec `Retry-After`), découverte comprise ; le délai de
   génération coupé reste 504. Avant, la découverte rendait `overlay: null` et `team_m: 0` alors qu'une surcouche existe
   et la réponse dépendait de la charge. Le contrat ne change pas (503 `overloaded` y figure pour la génération). Une
   surcouche absente ou périmée garde son comportement : découverte sans équipe, `overlay: null`.
5. **Arrêt.** `Generator::begin_shutdown` (appelé par `main` au signal d'arrêt) lève un drapeau que la construction de
   fond d'un dépôt (`warm_team`) lit toutes les 50 ms dans son attente de place, et avant de commencer : l'attente
   s'interrompt. Une construction déjà commencée finit (quelques secondes au plus, mesurées) ; les requêtes en vol ont
   leur échéance.

**Budget par défaut : 900 → 1 000 Mo.** La réservation d'une construction qui dépasse le budget est refusée tout de suite
(503). Avec la mémoire de `TeamCheck`, la plus grosse équipe de la France (D) réserve 924 Mo (749 + 175) : sous 900 elle
serait refusée à chaque fois, une régression de comportement ; 1 000 Mo lui laisse 76 Mo de fenêtres pendant ses 3 s de
construction (avant : 151 Mo sur 900 ; le gain réel de comptage est que la réservation couvre désormais tout). Les
équipes résidentes elles-mêmes sont petites (35 Mo pour les neuf de la France, 16 Mo pour la plus grosse, A) : elles ne
justifient pas de relever le budget. Pic de séance attendu : le relevé de la seconde relecture (1 115 Mo anonymes avec
900) monte au plus de la même marge, +100 Mo, soit environ 1,8 Go estimés avec l'altitude : **à remesurer** dans le
conteneur avec l'altitude réelle avant d'ouvrir plus d'équipes ; `ORD_GENERATOR__MEMORY_BUDGET_MB` permet de redescendre
(pas en dessous de la plus grosse équipe attendue + sa marge de fenêtres).
