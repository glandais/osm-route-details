# Graphe vélo OSM — vues hors mémoire : conception

> Statut : conception technique, **implémentée** le 19 novembre 2026 (§10 : écarts et vérifications). Suite de
> [`ord-graph-build-hors-memoire.md`](ord-graph-build-hors-memoire.md) (graphe de base hors mémoire, réalisé et mesuré
> jusqu'à la planète : [relevés](ord-graph-build-hors-memoire-mesures.md)) et du lot 0 bis
> ([`ord-graph-lot0.md`](ord-graph-lot0.md) §11 : vues par type). Motif : servir le monde, il faut les vues `road` et
> `gravel` de la planète, et `ord-graph-build views` les construit en mémoire.
> **[mesuré]** : relevé de ce document ou déjà publié ; **[calculé]** : dérivé de mesures ; **[estimé]** : à confirmer.

## 0. Périmètre

- **Dans la note** : `ord-graph-build views --external`, graphe de base `.ordg` → vues `.ordv` `road`, `gravel` et
  `all`, **sans élagage**. Mêmes octets que la construction en mémoire, à `build_epoch`, somme d'en-tête et `STATS`
  près (même règle que pour la base).
- **Hors de la note** : les vues `-prune` (élagage itératif des impasses), que le service ne lit pas. En
  `--external`, elles sont refusées. Le polygone du monde, déjà fait (`scripts/planet.poly`, écrit à côté du planet par
  `scripts/osm-planet-update.sh`, test `the_planet_polygon_fits_a_planet_graph_and_stops_at_the_antimeridian`).
- **Inchangé** : le format `.ordv`, `build_view` (il reste la référence), le service.

## 1. Constat

`views` sur la France (`france-20261117-z11.ordg`, 24,0 M arêtes, 18,8 M nœuds), `road,gravel`, 16 fils :
**40,6 s, pic 6,0 Gio** **[mesuré]** (6,2 Go publiés dans `OPERATIONS.md` de Pédalons).

| Vue | Arêtes filtrées | Nœuds de vue | Arêtes de vue | Nœuds fondus | Boucles coupées | Anneaux | Fichier |
|---|---|---|---|---|---|---|---|
| `road` | 12 760 366 | 6 519 402 | 8 162 395 | 4 648 315 | 49 875 | 469 | 0,88 Go |
| `gravel` | 17 460 590 | 10 377 093 | 13 451 562 | 4 063 949 | 54 212 | 709 | 1,46 Go |

`build_view` tient tout à la fois : les arêtes de base à plat (20 o par arête), l'incidence complète (CSR), quatre
tableaux par nœud ou par arête, chaque chaîne dans son propre `Vec`, puis la vue entière (nœuds, arêtes, **toute la
géométrie** en `refs` + `coords`) pour `write_graph`. Le contrôle `check_view_structure` recharge les arêtes de base et
une table de hachage de toutes les paires `(a, b)`.

Planète (553,2 M arêtes, 430,0 M nœuds, ≈ 23 × la France) : **≈ 140 Go** **[calculé : 6,0 Gio × 23]** ; les fichiers
de vue feraient ≈ 20 Go (`road`) et ≈ 34 Go (`gravel`) **[calculé]**.

## 2. Principe

Comme pour la base : des flux triés par le tri externe (`extsort`, mêmes enregistrements à taille fixe, même budget),
l'écriture par `StreamWriter`. Trois différences :

1. **Le graphe de base est déjà un fichier projeté** : attributs, longueurs, extrémités et géométrie d'une arête se
   relisent par `mmap` (pages de fichier, que le noyau reprend), sans rien recopier. Les accès suivent les chaînes,
   donc l'espace : les ids de nœuds et d'arêtes sont dans l'ordre des cellules (Morton), les pages lues sont voisines.
2. **L'état par élément tient en bits** : arête retenue, arête visitée, nœud fondu, nœud ancre. Pour la planète,
   4 × ≤ 69 Mo **[calculé]**.
3. **La marche des chaînes reste séquentielle** et suit exactement l'ordre de `contract` (§4) : c'est ce qui donne les
   mêmes chaînes, et en particulier le même point de coupure des boucles.

## 3. Ce qui ne se déduit pas d'un flux

`contract` lit l'**incidence non orientée** de chaque nœud : les arêtes retenues qui le touchent, dans l'ordre des ids
d'arête. Le fichier de base ne la porte pas. Ses listes `out` ne contiennent que les sens ouverts : une arête à sens
unique n'y apparaît qu'à sa source. Il faut donc la reconstruire (V1–V2).

Pour un nœud fondu (exactement deux arêtes retenues, mêmes sens de part et d'autre), la marche a besoin de ses deux
arêtes et de leurs bouts opposés. C'est la seule table qui croît avec le graphe : 16 o par nœud fondu, ≈ 1,7 Go pour
`road` sur la planète **[calculé : 4,65 M × 23 × 16 o]**. Elle est écrite sur un fichier temporaire et **lue par
`mmap`** (indexée par le rang du nœud parmi les nœuds fondus) : des pages de fichier, pas de mémoire anonyme.

## 4. Étapes

### V1 — Filtre et incidence (flux)

Parcours des cellules du graphe de base dans l'ordre (les arêtes y sont dans l'ordre des ids).
- Bit `alive[e]` = `ViewKind::keep(attrs)`.
- Pour chaque arête retenue, deux enregistrements `Inc { node, edge, other, attrs, side }` (24 o) vers un tri par
  `(node, edge)`.

### V2 — Nœuds fondus et ancres (fusion du tri)

Les enregistrements d'un même nœud arrivent ensemble, dans l'ordre des arêtes : c'est `inc.of(v)`.
- **Nœud fondu** : exactement deux arêtes distinctes et `mergeable_at` vrai. Calcul fait sur les attributs portés par
  `Inc` (la fonction est réécrite sur ces champs, sans `BaseEdges`). Bit `merged[v]`, et la paire
  `(e1, far1, e2, far2)` dans la table des nœuds fondus (§3), au rang de `v`.
- **Ancre** : degré non nul et pas fondu. Bit `anchor[v]`.
- Le flux trié est rejoué en V3 (seconde fusion, ou série écrite à part).

### V3 — Chaînes (marche séquentielle)

Même procédure que `contract` :
- pour chaque ancre `v` par id croissant, pour chaque arête incidente `e` (flux de V2) non visitée, marcher jusqu'à
  une ancre ou au retour en `v` ;
- la marche lit `merged` (bit) et la table des nœuds fondus. Les bouts sont connus par `Inc` et par la table : aucune
  relecture de la base.

Puis les anneaux :
- arêtes `alive` et non visitées, par id croissant ;
- départ en `ends(e).0` (relu dans la base) ;
- rotation sur le plus petit nœud, coupure au milieu, comme aujourd'hui.

Les boucles sont coupées au milieu, et le nœud du milieu devient une ancre (bit `anchor`).

Sortie :
- les maillons de chaque chaîne dans un fichier temporaire (`u32`, `e | DIR_BIT` si parcouru à rebours) ;
- un enregistrement `Chain { from, to, off, n }` (base) vers V4.

Mémoire : les bits, la table (fichier), un tampon.

### V4 — Arêtes de vue (tri)

Ids de vue : `view_id(b) = rang(anchor, b)`, par un répertoire de rangs sur le bitset (≈ 3 Mo pour la planète).

Pour chaque chaîne :
- orientation `a < b`, maillons relus et retournés si besoin ;
- longueur, sens, pire accès, routes, attributs du plus long maillon, poids ;
- les attributs et les longueurs de base sont relus par `mmap` de la base.

Enregistrement `VEdge { a, b, first_link, off, n, rev, len_dm, weight, attrs }` (≈ 41 o), trié par
`(a, b, first_link)`, la clé de `vedges.sort_by_key`. Les doublons sont impossibles : une arête de base n'est que dans
une chaîne.

### V5 — Écriture

Les arêtes triées sortent dans l'ordre de `a`, donc des cellules : les arêtes possédées par une cellule sont
contiguës.
- **Nœuds de vue** : les ancres dans l'ordre des ids de base. Table des cellules : `node_cell` de la base.
  Positions et ids OSM relus dans la base.
- **Arêtes orientées** : `(a, b, e)` et `(b, a, e | DIR_BIT)` selon les sens, vers un tri `(src, dst, edge)`. C'est
  l'ordre de `write_graph`, comme `dir` dans la base hors mémoire. La fusion est lue cellule par cellule en même temps
  que les arêtes.
- **Géométrie** : pour chaque arête, dans l'ordre :
  - la polyligne est recomposée depuis la base (géométrie de chaque maillon, retournée si besoin, plus la position du
    nœud d'arrivée de chaque maillon) ;
  - elle est encodée par `encode_geometry`, comme `write_graph` le fait de `interior` ;
  - les caps sont pris par `polyline_heading_pair` sur la même polyligne.

  Les arêtes étant déjà dans l'ordre final, la géométrie sort dans l'ordre du fichier. **Pas de redistribution par
  paquets** (à la différence d'E7 dans la base).
- **Sections de vue** : `VIEW_WEIGHT`, `VIEW_CHAIN_OFF`, `VIEW_CHAIN`, `VIEW_BASE_NODE`, `VIEW_HEADING` écrites en
  flux dans des fichiers temporaires, puis recopiées dans l'ordre de `extras`, après `ID_EDGES`. `VIEW_META` est un
  petit JSON.
- **Ajout à `StreamWriter`** : `put_extra(kind, reader, len)`, l'actuel `put_small`.

### V6 — Contrôle

`check_view_structure` réécrit en flux :
- bits `alive` et `seen` ;
- attributs et longueurs relus par `mmap` ;
- paires parallèles comptées sur les arêtes **triées** : une suite d'arêtes de même `(a, b)` compte une fois. Plus de
  table de hachage, même nombre.

Le contrôle exhaustif reste systématique, comme aujourd'hui.

## 5. Ressources attendues (planète, `gravel`, budget 1 Gio)

| | Valeur | |
|---|---|---|
| Mémoire anonyme | budget + ≈ 0,3 Go (bits ≈ 0,25 Go, tampons, table des cellules) | **[estimé]** |
| Pages de fichier | table des nœuds fondus ≈ 1,5 Go, base relue le long des chaînes | **[calculé]** |
| Disque temporaire | incidence ≈ 19 Go (×2 pendant la fusion), arêtes de vue ≈ 13 Go, orientées ≈ 7 Go, géométrie ≈ 30 Go : pic ≈ 50 à 70 Go | **[calculé]** |
| Mur | une passe sur la base, deux tris de l'ordre de 10⁹ enregistrements, l'écriture d'environ 34 Go : de l'ordre de 30 à 60 min par vue | **[estimé]** |

## 6. Interface

`ord-graph-build views --graph <base>.ordg --views road,gravel --external [--mem-budget-mb 1024] [--tmp-dir D]`.

Mêmes options que `build --external`, même dossier temporaire verrouillé (`TmpDir`). Le rapport ajoute
`tmp_peak_bytes`, `runs` et `peak_rss_before_verify_kb` par vue. `-prune` avec `--external` : erreur claire.

## 7. Tests

- **Mêmes octets** : `--external` contre `build_view` + `write_graph`.
  - Sur les graphes aléatoires et denses de `tests/external.rs`, à budget minuscule : plusieurs séries par tri, et des
    fusions à plusieurs niveaux.
  - Sur des cas construits : boucle de longueur paire et impaire, anneau sans ancre, double sens suivi d'un sens
    unique, arêtes parallèles, nœud isolé.
- **Bout en bout** : Andorre et les Pays de la Loire. Vues `--external` identiques aux vues en mémoire, et
  `check_view_structure` réussi.
- **Échecs** : `-prune` refusé ; dossier temporaire nettoyé après une erreur.

## 8. Protocole de mesure

1. **Région et France**, `road,gravel`, en mémoire puis `--external` à 16 Mo et 1 Gio : mêmes octets, mur et pic.
2. **Europe** (`europe-20261116-z11.ordg`, déjà construit), `--external` à 1 Gio.
3. **Planète** : à la suite de la mise à jour du planet (`osm-planet-update.sh`), base puis vues, `--external` à
   1 Gio. Vérifier d'abord que les quatre fichiers passent le chargement du service (`polygon_fits` réel, temps de
   démarrage), sur le poste, avant de copier ≈ 100 Go sur le serveur.

## 9. Points de tension

- **La table des nœuds fondus** croît avec le graphe. Sur `mmap`, elle sort de la mémoire anonyme, mais ses pages
  restent comptées dans le RSS du processus. Si elle devient gênante, une marche entièrement externe (classement de
  listes par tris répétés) la supprime. Elle coûterait plusieurs tris de plus.
- **Les relectures de la base par `mmap`** supposent la localité des chaînes. Une chaîne très longue, ou un anneau
  (rares : 469 sur la France), font des accès épars. À mesurer en V3–V5 sur l'Europe.
- **Ordre de la marche** : l'identité des octets repose sur le même ordre de découverte que `contract`. Les tests à
  boucles impaires le vérifient : leur point de coupure dépend du sens de la marche.

## 10. Réalisation (19 novembre 2026)

Module `crates/ord-graph/src/view_external.rs` (`build_view_external`), bitset `bits.rs`, option `--external` de
`ord-graph-build views` (avec `--mem-budget-mb` et `--tmp-dir`), `StreamWriter::put_extra`.

### 10.1 Écarts à la conception

- **Règles partagées** : `mergeable_at`, `chain_summary` (longueur, sens, accès, routes, attributs du plus long
  maillon, poids), `link_from` et `link_to` sont écrits sur le trait `EdgeSrc`. Ce trait est implémenté par
  `BaseEdges` (en mémoire) et par `GraphFile` (le fichier de base). `build_view` les appelle, au lieu de son code en
  ligne : les deux constructions ne peuvent plus diverger sur ces règles.
- **`Inc` sans le bout opposé ni les attributs** (9 o au lieu de 24) : le bout et les attributs se relisent dans la
  base (`edge_ends` est un accès direct à `ID_EDGES`).
- **La table des nœuds fondus ne garde que les deux arêtes** (8 o par nœud fondu au lieu de 16), pour la même raison.
  Elle est lue par un petit cache de blocs (16 Mio, lecture positionnée), pas par `mmap` : aucun `unsafe` de plus.
  Les maillons, relus au hasard en V5, passent par le même cache.
- **Ids de base dans `VEdge`** : seul `base_a` est gardé (le point de départ de la polyligne). Le reste se déduit des
  maillons.
- **`check_view_structure` réécrit pour toutes les vues**, pas seulement en `--external`. La base est lue dans son
  fichier, avec deux bits par arête de base. Les paires parallèles sont comptées sur les arêtes triées, et une vue
  dont les arêtes ne sont pas triées par `(a, b)` est maintenant une erreur. Seules les vues `-prune` rechargent
  encore `BaseEdges`.
- **Rapport** : `peak_rss_anon_kb` (pic de `RssAnon` relevé toutes les 2^20 itérations et à chaque étape) et les
  durées des étapes. `VmHWM` compte les pages du graphe de base projeté (1,9 Go pour la France), lu en entier par
  `verify()` : il ne dit pas la mémoire de la construction.

### 10.2 Vérifications

- **Tests `tests/views_external.rs`** :
  - **40 réseaux riches en chaînes**, trois vues, budgets de 1 o, 4 Kio et 64 Mio : octets identiques, contrôle de
    structure réussi. Ces réseaux contiennent :
    - une grille d'ancres et des ways à nœuds intermédiaires (parfois coupées en deux, parfois doublées) ;
    - des raquettes de longueur paire et impaire, des anneaux isolés ;
    - des sens uniques dans les deux sens, des accès privés, des surfaces non revêtues.
  - **Garde-fou** : le test vérifie qu'il y a bien eu des boucles coupées, des anneaux et des nœuds fondus.
  - **Fusion à plusieurs niveaux** : une grille de 30 × 30, plus de 64 séries.
  - **Graphe vide** ; vue `-prune` refusée, dossier propre.
  - **Mutation** : couper les boucles un maillon plus loin (`div_ceil`) est détecté.
- **Bout en bout** (`e2e_tests.rs`) : sur le mini-PBF, `road`, `gravel` et `all` en `--external` à 1 Mo ont les
  octets des vues en mémoire, en-tête mis à part ; `-prune` refusé.
- **France** (`france-20261117-z11.ordg`), `road,gravel` : octets identiques à la construction en mémoire, en-tête
  mis à part **[mesuré]**.

| | En mémoire | `--external` 16 Mo | `--external` 1 Gio (cgroup 1,5 Gio) |
|---|---|---|---|
| Mur (deux vues) | 40,6 s | 56 à 139 s | 138 s |
| Pic | 6,0 Gio (`VmHWM`) | **0,06 Gio** anonyme | **0,98 Gio** anonyme |
| Disque temporaire (`gravel`) | — | 2,18 Gio | 1,23 Gio |

Le mur varie d'un lancement à l'autre (56 s puis 139 s à 16 Mo, sans autre charge visible) ; l'étape V5 (géométrie
recomposée depuis la base) en prend la plus grande part. Sous cgroup de 1,5 Gio, le cache de pages est compté et la
base (1,9 Go) n'y tient pas : elle est relue du disque.

**Europe** (`europe-20261116-z11.ordg`, 191,2 M arêtes, 15,2 Go), `road,gravel`, `--external` 1 Gio, sans cgroup
**[mesuré]** :

| Vue | Arêtes de vue | Fichier | Mur | Pic anonyme | Disque temporaire | Séries au plus |
|---|---|---|---|---|---|---|
| `road` | 62 175 542 | 6,26 Gio | 814 s | 1,34 Gio | 9,64 Gio | 4 |
| `gravel` | 106 641 655 | 10,88 Gio | 619 s | 1,47 Gio | 17,01 Gio | 6 |

Étapes (s), `road` / `gravel` :

| V1 | V2 | V3 | V4 | V5 arêtes, géométrie | V5 écriture | relecture |
|---|---|---|---|---|---|---|
| 27 / 28 | 119 / 23 | 16 / 10 | 53 / 57 | **527 / 370** | 60 / 109 | 11 / 23 |

La commande entière dure 33,7 min. Elle compte en plus `verify()` de la base et le contrôle exhaustif de chaque vue.

**Lecture** :
- **Le pic anonyme dépasse le budget de 0,34 à 0,47 Gio.** Hors tampons de tri, il comprend :
  - les quatre bitsets (≈ 0,1 Gio) ;
  - les deux caches de blocs (32 Mio) ;
  - en V5, le tri des arêtes orientées (moitié du budget), en même temps que les tampons d'écriture des sept flux.

  Le critère « budget + 0,3 Go » de la base est donc légèrement dépassé ; à resserrer si besoin (part du tri
  `dir`).
- **V5 domine** (65 % et 60 % du mur). Pour chaque maillon, il relit la géométrie dans la base (recherche de cellule,
  décodage). Pour chaque point d'arrivée, il refait une recherche de cellule. Les arêtes sortent dans l'ordre des
  cellules, mais leurs maillons visitent la base au fil des chaînes. À 8,5 µs par arête de vue, c'est le premier
  levier si la planète est trop lente.
- **Le disque temporaire** est de l'ordre de 1,5 × le fichier de vue. Le pic `VmHWM` (12,0 Gio) compte les pages
  de la base et des vues relues.

Planète **[calculé, × 2,9 sur l'Europe]** : ≈ 40 min par vue, ≈ 30 Go (`road`) et ≈ 50 Go (`gravel`) de disque
temporaire, pic anonyme ≈ 1,5 Gio.

### 10.3 Planète (20 novembre 2026)

`planet-261116.osm.pbf` mis à jour par `scripts/osm-planet-update.sh` jusqu'à la séquence quotidienne 5181
(données du 19 novembre 2026, 00:00 UTC), puis `hm-runs/world-build.sh` : `build --external --mem-budget-mb 1024`,
puis `views --views road,gravel --external --mem-budget-mb 1024`, binaire de `6e26240`, sans cgroup **[mesuré]**.

**Base** `planet-261116-20261119-z11.ordg` : 431 524 740 sommets, 555 203 623 arêtes (263,8 M ways retenues sur
1 234 M), 45,9 Go ; mur 1:42:36, pic anonyme avant `verify()` 1,27 Gio, disque temporaire 150,7 Go au plus (comme
le relevé du §5.1 de la note de construction : 1:42:10, 1,49 Gio).

| Vue | Nœuds | Arêtes de vue | Fichier | Mur | Pic anonyme | Disque temporaire | Séries au plus |
|---|---|---|---|---|---|---|---|
| `road` | 195 144 379 | 247 912 662 | 26,7 Go | 2 497 s (41,6 min) | 1,73 Gio | 41,5 Go | 13 |
| `gravel` | 272 415 226 | 353 279 888 | 39,3 Go | 4 779 s (79,6 min) | 1,76 Gio | 60,9 Go | 18 |

Contrôle exhaustif des deux vues réussi : toutes les arêtes filtrées couvertes (328 586 418 et 429 328 530),
0 arête inutilisable ; 1 038 112 et 1 243 633 boucles coupées, 14 453 et 18 392 anneaux.

Étapes, mur / processeur (s), `road` puis `gravel` :

| V1 | V2 | V3 | V4 | V5 arêtes, géométrie | V5 écriture | relecture |
|---|---|---|---|---|---|---|
| 134 / 86 | 161 / 71 | 53 / 26 | 277 / 202 | **1 577 / 354** | 237 / 60 | 58 / 19 |
| 678 / 135 | 652 / 108 | 351 / 46 | 771 / 309 | **1 853 / 478** | 373 / 85 | 101 / 26 |

**Lecture** :
- **Le disque limite, pas le calcul** : 4 à 5 fois plus de mur que de processeur. La base (46 Go) ne tient pas dans
  le cache de pages (15 Gio de mémoire sur le poste) ; les relectures au fil des chaînes ratent le cache. Les
  étapes V1 à V4 de `gravel` sont 3 à 7 fois plus longues que celles de `road` pour 1,3 fois plus d'arêtes
  (hypothèse non vérifiée : le cache de pages est occupé par la vue `road` qui vient d'être écrite).
- **Le pic anonyme dépasse le budget de 0,73 à 0,76 Gio** (0,34 à 0,47 en Europe). Les quatre bitsets en prennent
  ≈ 0,25 Go **[calculé : 2 × 555 M + 2 × 432 M bits]** ; le reste n'est pas ventilé. Sous 2 Gio : acceptable pour
  une construction sur le poste.
- **Estimation du §10.2** (≈ 40 min par vue, 30 et 50 Go de temporaire) : juste pour `road`, deux fois trop
  optimiste pour le mur de `gravel` ; le temporaire est 1,6 fois le fichier de vue.
- **Chaîne complète** (mise à jour du planet exclue) : 4 h 19 min, 112 Go de fichiers.

### 10.4 Chargement de la planète par le service (poste, 20 novembre 2026)

Binaire `osm-route-details` de `6e26240`, environnement de l'image (`MALLOC_ARENA_MAX=2 MALLOC_MMAP_THRESHOLD_=1048576`),
sans altitude (`ORD_CACHE__OFFLINE=true`, IGN désactivé), réglages par défaut (budget de 800 Mo), les quatre fichiers
de la planète et `planet-261116.poly`, cache de pages des fichiers évincé au départ, sous
`systemd-run --user --scope -p MemoryMax=… -p MemorySwapMax=0` **[mesuré]**.

- **À 3 Go (la limite du conteneur de production) : tué par le noyau** (`oom-kill`) au bout de 2 min 53 s, avant
  d'être prêt. En cause, les composantes fortement connexes de `ViewIndex::build` : quatre tableaux par nœud de vue
  (≈ 13 o par nœud, 195 M et 272 M nœuds), les deux vues en parallèle.
- **À 12 Go : prêt après 36,7 min** (`load_ms` 2 203 521 ; 8,5 s pour la France), pic anonyme 5,0 Gio, ≈ 0,75 Gio
  au repos ensuite ; 0 `oom`. Polygone accepté (`nodes_inside` 1,0), identités des vues vérifiées.

| Étape du chargement | Durée |
|---|---|
| Index des vues (`foreign_ms` 159 071, `scc_ms` 559 603 au plus des deux vues) | 14,1 min |
| Arêtes plus larges qu'une cellule (`CellIndex::exceptions`) : **16 448**, plafond 1 000 dépassé, repli sur l'index entier pour les dépôts | 14,4 min |
| `GraphIndex::build` du contrôleur | 8,2 min |

Le processus attend le disque presque tout ce temps (7 min de processeur à 31 min) : ces balayages sont en un fil,
par fautes de pages sur 112 Go qui ne tiennent pas dans le cache.

**Générations** (découverte, curseur 1, boucles de 60 km en route et 40 km en gravel, 14 lieux sur tous les
continents dont Suva et Taveuni de part et d'autre de l'antiméridien, deux passes en série, 56 requêtes) :
6 × 200 (route : Denver, Le Cap, Nairobi), 10 × 422 attendues (Reykjavik `no_candidate`, Ushuaia, plein Atlantique
`start_too_far`), **39 × 503 `overloaded`** et 1 × 504, toutes au bout du budget de 8 s.

Cause des 503 : la zone 2 d'une fenêtre (`Engine::plan_with`) élargit les cases d'accroche de la **plus longue
chaîne de la vue** (`lmax_dm`). Sur la planète, elle fait **566 km** (`road`) et **1 406 km** (`gravel`) ; 1 et 5
chaînes de plus de 500 km, 303 et 420 de plus de 100 km. Chaque fenêtre couvre alors des centaines de kilomètres
autour du départ, son estimation dépasse le budget et la réservation attend jusqu'à l'échéance.

**La planète n'est pas servable en l'état**, ni dans 3 Go ni en temps de démarrage. Les fichiers ne sont pas en
cause, ce sont trois hypothèses du service calibrées sur la France :
1. l'élargissement des fenêtres par la plus longue chaîne globale ;
2. les composantes fortement connexes calculées en mémoire, sur toute la vue, au démarrage ;
3. les balayages complets de la base au démarrage (index des vues, arêtes larges, index du contrôleur), et le
   plafond de 1 000 arêtes larges.
