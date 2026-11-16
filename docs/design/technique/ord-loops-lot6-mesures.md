# Générateur de parcours — lot 6 : relevés (France entière dans 2 Go)

> Relevés du 16 novembre 2026, binaires `release` de la branche `feat/ord-loops-window` (commits du lot 6), poste de
> développement (AMD Ryzen 7 4800H, 16 fils, 30 Go). Graphe et vues France construits le 16 novembre (extrait Geofabrik du
> 15/11) ; région : Pays de la Loire. Conception : [`ord-loops-lot6.md`](ord-loops-lot6.md). Mo et Go sont décimaux.
> Les neuf équipes sont lettrées A à I ; aucune coordonnée de départ, aucun identifiant d'équipe ni secret ici. Les seuls
> lieux nommés (Paris, Lyon, Nantes, Clermont-Ferrand) sont quatre centres-villes publics, sans lien avec les départs.
> Les relevés ont tourné en même temps que d'autres tâches de la machine (charge 3 à 4) : les temps sont des **ordres de
> grandeur**, la mémoire et l'équivalence sont exactes. Chaque ligne dit comment la relever.

## 1. Équivalence : zéro écart

| Quoi | Comment | Résultat |
|---|---|---|
| Sorties figées du moteur à plat d'avant le lot | `cargo test -p ord-loops` : 140 requêtes (boucles et allers simples, trois modes, trois types, avec et sans cap) sur un graphe synthétique de 48 × 48 carrefours, à z11 et à z16 ; la fenêtre des tuiles nécessaires, la classe de fenêtre, la fenêtre de toutes les tuiles et des sur-ensembles tirés rendent chacune l'empreinte écrite **avant** le moteur fenêtré | identiques |
| Fenêtres partielles sur grille dense z16 | 60 requêtes × 3 fenêtres (nécessaires, classe, sur-ensemble tiré) contre la fenêtre de toutes les tuiles ; un tiers au moins des fenêtres sont de moins de la moitié des tuiles | identiques, 0 échappement |
| Garde-fou | fenêtre réduite de moitié : lève `WindowEscape` ou rend la même réponse, jamais une réponse différente | levé, jamais de réponse fausse |
| Index globaux contre la force brute | `--self-check` du banc : chaînes étrangères (boîtes de tous les segments), longueurs de composantes (Tarjan à plat depuis les sens ouverts), degré admis de chaque nœud de base, aucune arête de base bouclée | **région** 9 051 + 13 293 étrangères, 9 518 + 15 782 chaînes sous le plafond, 820 044 + 977 849 nœuds, 1,5 s : identiques ; **France** 119 803 + 219 922, 146 004 + 215 777, 10 921 939 + 14 113 543 nœuds, 23 477 176 arêtes, 24,3 s : identiques |
| Fenêtré contre à plat, même processus | `--compare` : chaque requête jouée sur le moteur fenêtré (cache de fenêtres) **et** sur la fenêtre de toutes les tuiles, comparaison de `Debug` du résultat et des diagnostics | **région** 1 292 requêtes (323 × 4 crans du curseur) : 0 différence ; **France** 688 requêtes (30 premières de chaque équipe × 4 crans) : 0 différence |
| Binaire d'avant le lot contre ce binaire | `--dump` : une empreinte FNV par requête de la sortie complète, binaire du tag `v1.9.0` (plus l'option `--dump`, 8 lignes, hors dépôt) contre le binaire du lot, région, 1 292 requêtes | **1 292 empreintes identiques** (`diff` vide) |
| Contrôleur indépendant, tables bornées | tests : tables de la zone = tables du graphe entier (5 surcouches, trois types, trous comblés), degrés des nœuds avec une arête plus large qu'une cellule | identiques ; au banc, 0 parcours refusé par le contrôleur alors que le contrôle interne l'avait accepté, 0 écart de mesure sur 217 + 989 + 1 051 + 1 262 parcours (France) |
| Index des dépôts | `CellIndex` avec liste d'exceptions contre `SegIndex`, arêtes de 20 km sur une grille à cellules de 13 km | mêmes `Hit` |

## 2. Mémoire

### 2.1 Ce que tient le moteur

| | Avant (à plat, mesuré au lot 6) | Maintenant |
|---|---|---|
| Moteur ouvert, France | 2,9 Go de tas (8,0 Go de RSS maximal au banc) | **8,5 Mo** (index des chaînes étrangères et composantes creuses) ; `RssAnon` du service au démarrage : **28 Mo** |
| Région | ~150 Mo | 0,6 Mo |
| Une équipe en cache, France | 736 Mo | quelques Mo (arêtes d'équipe, composantes strictes) |

### 2.2 Construction des données d'une équipe (France, une à la fois, `RssAnon` échantillonné toutes les 25 ms)

| Équipe | A | B | C | D | E | F | G | H | I |
|---|---|---|---|---|---|---|---|---|---|
| `TeamData` : temps | 1,7 s | 0,2 s | 1,8 s | 3,1 s | 0,4 s | 0,1 s | 0,1 s | 0,1 s | 0,1 s |
| pic anonyme (Mo, dont 32 de banc) | 347 | 55 | 434 | **704** | 106 | 26 | 32 | 32 | 32 |
| estimation réservée au budget (Mo) à 150 o par chaîne, avant réglage | 229 | 31 | 334 | 535 | 70 | 15 | 16 | 13 | 13 |
| tables du contrôleur (`TeamCheck`) : temps / pic | 1,0 s / 232 | 0,2 s / 55 | 1,3 s / 283 | 1,9 s / 418 | 0,2 s / 108 | 0,1 s / 32 | 0,1 s / 32 | 0,1 s / 32 | 0,1 s / 32 |

La première estimation (150 o par chaîne de la zone) sous-estimait de 20 à 30 % ; elle est maintenant de **210 o** : à ce
tarif les estimations couvrent les pics mesurés (A 321, C 468, D 749 Mo, contre 315, 402, 672 mesurés à 32 Mo de banc près).
Le pic de la plus grande équipe (D) est de 704 Mo pendant 3 s : **c'est la plus grosse part du budget de 900 Mo**.

### 2.3 Banc France sous 2 Go (`systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0`)

`ord-loops-bench --cursors 0,0.01,0.25,1 --rounds 1`, moteur fenêtré, budget 900 Mo, neuf équipes, **2 192 requêtes**
(toutes celles de chaque équipe dans le périmètre France, quatre crans) : termine, code 0, sans OOM.

- 55 fenêtres construites, 2 137 servies du cache, 8 évincées, **0 échappement** ; fenêtres de 244 464 chaînes en médiane
  et 590 262 au plus ; mémoire comptée au plus **900 Mo** (le budget est atteint) ;
- construction à froid d'une fenêtre : **77 / 165 / 198 ms** (médiane / p90 / maximum) ; agrégats d'équipe 13 / 29 / 37 ms ;
- `RssAnon` à la fin : **1 128 Mo** (budget 900 Mo, plus les données d'équipe, le rapport du banc et les restes du
  tas) ; `VmHWM` 3,4 Go dont 2,1 Go de pages de fichier récupérables : sous 2 Go de cgroup, le noyau les reprend au fil de
  l'eau.

### 2.4 Le service sur la France, dans 2 Go (`ord-server`, neuf dépôts puis la génération par HTTP)

Service lancé sous `systemd-run --user --scope --unit=… -p MemoryMax=2G -p MemorySwapMax=0` avec `MALLOC_ARENA_MAX=2`,
graphe, vues, neuf dépôts réels (850 Mo de NDJSON, dépôts par `PUT`), puis 60 requêtes en série, 108 à 4 clients, 108
à 12 clients (mélange de distances de 20 à 180 km, trois types, curseurs 0, 0,25 et 1, départs pris sur les réseaux
déposés ; les 422 sont des échecs de génération de la catégorie « départ trop loin », « réseau trop petit », « aucun
candidat », propres à ces tirages). Le polygone est un rectangle autour des nœuds (voir §4).

| | sans `MALLOC_MMAP_THRESHOLD_` | avec `MALLOC_MMAP_THRESHOLD_=1048576` |
|---|---|---|
| démarrage (`load_ms`) | 4 818 ms (index 3 612 + 1 072) | 4 604 ms (index 3 411 + 1 077) |
| `anon` après les neuf dépôts | **513 Mo** | **216 Mo** |
| `anon` au plus, série (échantillonné) | 1 355 Mo | 1 022 Mo |
| `anon` au plus, 4 clients | 1 538 Mo | 1 026 Mo |
| `anon` au plus, 12 clients | 1 634 Mo | 1 020 Mo |
| `memory.peak` du cgroup (au plus) | **1 741 Mo** | **1 076 Mo** |
| réponses 5xx autres que 503 | 0 | 0 |
| 503 `overloaded` à 12 clients | 18 (file pleine : 2 calculs, 8 en attente, comme au lot 3a) | 18 |
| latence médiane / p90 / max (série) | 0,34 / 1,60 / 5,12 s | 0,34 / 1,70 / 5,18 s |
| latence médiane / p90 / max (4 clients) | 0,66 / 2,40 / 4,46 s | 0,67 / 2,74 / 4,09 s |

Sans le réglage, la fragmentation de `malloc` laisse **400 à 600 Mo** de plus d'anonyme au service que ce que compte son
budget : le tas ne rend pas au système les grands tableaux libérés (le seuil de `mmap` de glibc monte tout seul jusqu'à
32 Mo après chaque libération). `MALLOC_MMAP_THRESHOLD_=1048576` les fait toujours passer par `mmap`, donc revenir au
système à la libération : c'est la décision 3 de la note, **chiffrée**. Métriques relevées à la fin (avec le réglage) :
`ord_generator_memory_bytes` fenêtres 226 Mo, agrégats d'équipe 178 Mo, espaces de travail en réserve 285 Mo ;
fenêtres 30 + 44 construites, 175 servies du cache, 1 attente d'une construction en cours ; 67 fenêtres et 127 agrégats
évincés ; `ord_generator_window_escapes_total` à 0.

Le conteneur de production partage ces 2 Go avec l'altitude (deux caches de 256 Mo) : 1,08 Go de pic de cgroup pour le
générateur plus 0,5 Go d'altitude laisse de la marge **avec** le réglage ; **sans**, 1,74 + 0,5 dépasse 2 Go. Je n'ai pas
mesuré les deux ensemble (l'altitude lit l'IGN et Mapterhorn par le réseau).

### 2.5 Fenêtres autour de quatre centres-villes (`cargo run --release -p ord-loops --example window_probe`)

Classe de fenêtre (pas de rayon de 10 km), pages chaudes ; construction, un `generate_in` en découverte, octets tenus
(les espaces de travail sont ceux du premier usage, sans réemploi).

| Centre | Type | D | tuiles | chaînes | construction | fenêtre | espace de travail |
|---|---|---|---|---|---|---|---|
| Paris | route | 100 km | 89 | 584 865 | 156 ms | 39 Mo | 82 Mo |
| Paris | gravel | 100 km | 89 | 804 251 | 229 ms | 56 Mo | 108 Mo |
| Paris | route | 200 km | 268 | 929 417 | 253 ms | 62 Mo | 136 Mo |
| Paris | gravel | 200 km | 268 | 1 382 579 | 389 ms | **96 Mo** | 188 Mo |
| Paris | gravel | 300 km, aller simple | 534 | 2 084 489 | 619 ms | 145 Mo | 297 Mo |
| Lyon | gravel | 300 km, aller simple | 481 | **2 234 108** | 677 ms | **158 Mo** | **316 Mo** |
| Nantes | gravel | 200 km | 247 | 1 050 270 | 303 ms | 75 Mo | 150 Mo |
| Clermont-Ferrand | gravel | 200 km | 241 | 817 960 | 221 ms | 58 Mo | 110 Mo |

- Une boucle de 200 km tient dans **96 Mo** de fenêtre et 188 Mo d'espace de travail (la conception disait 89 et 152) ; le
  pire aller simple de 300 km (Lyon) dans **158 Mo** et **316 Mo** (conception : 144 et 246). L'espace de travail est de
  **142 à 146 o par chaîne**, pas 118 : les listes d'états fixés grossissent. La réservation du service est passée à 150 o.
- Fenêtre à froid : **156 à 253 ms à 100 et 200 km** en route, **229 à 412 ms** en gravel ; **617 ms** pour le pire aller simple
  de 300 km. Pages du fichier évincées (`posix_fadvise(DONTNEED)` des trois fichiers) : 207 à 487 ms en route, 249 à
  617 ms en gravel — le disque est un NVMe, la lecture ne coûte presque rien. Critère « ≤ 2 s pour 100 km » : **tenu avec
  un facteur 8**. Sur un disque réseau, le §16 de la note reste à relever.
- Génération en découverte : 0,2 à 2,1 s jusqu'à 200 km, **1,4 à 3,9 s** pour l'aller simple de 300 km (budget du service :
  8 s).

## 3. Temps

| | Avant | Maintenant |
|---|---|---|
| Ouverture du moteur, France | 16,9 s (chargement à plat) + 16 s (contrôle `fits`) | **4,5 à 5,9 s** (index 2,9 à 3,9 s + 0,7 à 1,1 s en route, 4,7 à 5,7 s + 1,1 à 1,2 s en gravel, une vue par fil) ; service : `load_ms` **4,6 à 4,8 s**, écoute au bout de ~5,4 s |
| Ouverture, région | 0,4 s | 0,35 à 0,4 s (index 0,2 + 0,05 s) |
| Contrôle de l'index des dépôts | 16,0 s (`CellIndex::fits`) | ~0,3 s (`exceptions` ; 1 arête sur la France) |
| Préparation d'une équipe, France | ~2 s par équipe | A 1,7 s, C 1,8 s, **D 3,1 s**, les autres ≤ 0,4 s ; tables du contrôleur 0,1 à 1,9 s |
| Génération à chaud, région (même banc, deux binaires) | voir §3.1 | voir §3.1 |
| Dépôts sur la France | impossible (index entier de 107 M de segments) | 703 Mo en 71 s, 67 Mo en 5,4 s, 38 Mo en 88 s, 30 Mo en 58 s, les petits en 1 à 8 s |

Les dépôts de 30 à 40 Mo qui durent une minute viennent de la construction, cellule par cellule, de l'index spatial des
dépôts (`CellIndex` neuf à chaque dépôt, 3 × 3 cellules relues à chaque cellule touchée). Ce n'est pas le moteur ; la durée n'a
pas été comparée à celle d'un dépôt sur la région dans ces relevés.

### 3.1 Génération à chaud, région : le moteur fenêtré contre le moteur à plat

`ord-loops-bench --cursors 0,0.25,1 --rounds 3 --limit 25` (129 requêtes de la région, 387 requêtes × crans), un fil, le binaire
du tag `v1.9.0` (moteur à plat) puis le binaire du lot (fenêtres), l'un après l'autre sur la machine au repos
(charge 1,6 à 2,6) ; médiane des trois tours chauds, **médiane / p90 / maximum en ms** :

| Cran du curseur | moteur à plat (`v1.9.0`) | moteur fenêtré | médiane |
|---|---|---|---|
| 0 (strict) | 2 / 49 / 265 | 2 / 50 / 256 | égale |
| 0,25 | 245 / 768 / 1 369 | 257 / 791 / 1 313 | **+ 4,9 %** |
| 1 (découverte) | 175 / 581 / 1 019 | 182 / 572 / 1 033 | **+ 4,0 %** |
| 0,25, D ≤ 100 km | 150 / 456 / 996 | 157 / 458 / 1 046 | + 4,7 % |
| 1, D ≤ 100 km | 114 / 275 / 460 | 119 / 275 / 467 | + 4,4 % |

Critère « du même ordre, ≤ + 10 % » : **tenu** (+ 4 à + 5 %, le surcoût est la lecture du graphe de base dans le fichier,
`make_anchor` et le contrôle interne). Moteur fenêtré sur la région : 14 fenêtres construites pour 387 requêtes, 373 servies
du cache, construction à froid **65 / 124 / 124 ms**, 287 Mo comptés au plus, RSS maximal 641 Mo (725 Mo à plat),
0 échappement. Sur la région, une fenêtre de classe couvre déjà une bonne part du graphe : le gain de mémoire se lit sur la
France (§2).

## 4. Ce que les relevés ont trouvé en dehors du moteur

- **Le polygone `france.poly` de Geofabrik ne passe pas `polygon_fits`** : sa boîte est `[-6,94 ; 41,24 ; 9,9 ; 51,43]`,
  celle des nœuds du graphe `[-5,14 ; 41,33 ; 9,84 ; 51,09]` ; il déborde de 1,8° à l'ouest (plus de 15 km). Le
  générateur s'éteint avec `reason = polygon_mismatch`. Les relevés du §2.4 ont utilisé un rectangle autour des nœuds.
  Pour déployer la France, il faut un polygone de couverture à sa mesure (ou une décision sur `POLY_OVERHANG_M`).
- **Dépôts** : voir §3 ; ils marchent sur la France depuis ce lot (l'index par cellule gère l'unique arête plus large
  qu'une cellule).

## 5. Reproduction (hors dépôt)

```sh
ord-loops-bench --self-check --compare --cursors 0,0.01,0.25,1 --rounds 1 --dump <fichier> …          # région
ord-loops-bench --graph <france.ordg> --road … --gravel … --overlays <overlays France> --poly <france.poly> \
    --cursors 0,0.01,0.25,1 --rounds 1                                                               # France, sous systemd-run
cargo run --release -p ord-loops --example window_probe -- <france.ordg> <road.ordv> <gravel.ordv>
```

Le service : variables `ORD_GENERATOR__GRAPH`, `…__VIEW_ROAD`, `…__VIEW_GRAVEL`, `…__COVERAGE_POLY`, `…__OVERLAY_DIR`,
`MALLOC_ARENA_MAX=2`, `MALLOC_MMAP_THRESHOLD_=1048576`, sous `systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0` ;
dépôts par `PUT /v1/generator/overlay` (en-têtes `ord-namespace-key`, `ord-team`), génération par
`POST /v1/generator/routes`, `memory.stat` et `memory.peak` du cgroup échantillonnés par un script local.
