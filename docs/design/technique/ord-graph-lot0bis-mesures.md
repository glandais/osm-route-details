# Graphe vélo OSM en cellules — lot 0 bis : rapport de mesures (vues, état local)

> Relevé du **15 novembre 2026**, de 17 h 31 à 18 h 45. Conception : [ord-graph-lot0.md](ord-graph-lot0.md) §11
> (vues, état local, contrôle) et §5 (protocole). Rapport du lot 0, dont ce relevé reprend la méthode :
> [ord-graph-lot0-mesures.md](ord-graph-lot0-mesures.md). Cible comparée : plan Pédalons
> `docs/plans/2026-11-15-generateur-parcours-osm.md`, §4.3 et §12 bis.
> **[mesuré]** : relevé sur la machine ci-dessous. **[calculé]** : dérivé d'un chiffre mesuré par une règle que le
> texte donne. **[non mesuré]** : chiffre absent à dessein.
> Ce rapport remplace les temps du « premier relevé de bout en bout » de la conception (§11.7, machine chargée,
> 20 départs, un tour chaud), qui n'étaient pas exploitables. **Aucun code n'a été modifié pour ce relevé** : aucun
> défaut qui fausse une mesure n'a été trouvé (une étiquette fausse du JSON est signalée en §2).

## 1. Verdicts

| Question | Mesure | Verdict |
|---|---|---|
| Une vue ramène-t-elle le disque de 50 km au repère du plan (180 000 arêtes) ? | `road` : **128 413** arêtes (médiane, 50 départs), **189 334** pour les 8 disques intérieurs ; base 377 866 / 539 515 | **Oui** : ÷ 2,9 sur la base, au repère pour les disques intérieurs. `gravel` 190 191 / 292 541 : au repère en médiane, × 1,6 à l'intérieur |
| La vue `road` jette-t-elle de vraies routes ? | Classes routières (`primary` … `unclassified`, `residential`, `living_street`, bretelles) : **783 km retirés sur 84 593 (0,93 %)** ; mais 1 235 km de `path`/`footway` désignés vélo et 255 km de `cycleway` non revêtues, donc 38 % des km d'itinéraires nationaux | **Non pour les routes ; oui pour les voies vertes** (§5) |
| Un arbre complet à 50 km en « quelques dizaines de ms » ? | `road`, état local : **25,7 ms** médiane, 39,3 ms p90 (intérieurs 38,5 ms) ; base 116,7 ms | **Oui sur `road`** (÷ 4,5) ; 37,9 / 56,2 ms si l'on ramène le coût par état à celui de la base (§7) |
| L'état local au disque tient-il la mémoire sans coûter de temps ? | 50 km : `road` 4,5 Mo (dense 14,2), base 13,0 Mo (dense 39,7) ; temps **+1,6 %** sur `road`, +7,7 % sur la base ; la table de hachage coûte +46 % (`road`) à +73 % (base) | **Oui.** France : 6 à 7 Mo par recherche à 50 km au lieu de 0,20 Go (`road`) ou 0,57 Go (base) en dense **[calculé]** |
| **Une génération (≈ 10 arbres) sous 500 ms ?** | 10 × un arbre de 50 km, état local : `road` **257 ms** médiane / **393 ms** p90 ; `road-prune` 140 / 218 ms ; `gravel` 418 / 677 ms ; base 1 167 / 1 738 ms | **`road` + état local, jusqu'à 50 km de rayon (boucles de 100 km)** : tenu, marge × 1,9 en médiane, × 1,3 au p90 ; **pas à 75 km** (550 / 691 ms). `road-prune` tient 75 km en médiane. `gravel` ne tient qu'en médiane. Détail et réserves en §11 |

**En une phrase** : filtrer puis fondre les chaînes est le levier qui manquait au lot 0 (le graphe `road` a 2,8 fois
moins d'arêtes, 162 m par arête, et un arbre de 50 km y coûte 26 ms) ; l'état local rend la mémoire indépendante de la
taille de la France pour 2 % de temps ; la cible de 500 ms par génération est atteinte pour le type route jusqu'à 50 km
de rayon, mesurée avec un poids synthétique et sans coût de virage.

## 2. Machine, données, code, charge

| | |
|---|---|
| CPU, RAM, disque | AMD Ryzen 7 4800H (8 cœurs, 16 fils), 30,9 Gio, Samsung SSD 980 1 To NVMe (ext4, 93 % plein) — la machine du lot 0 |
| Toolchain | `rustc 1.97.0 (2d8144b78 2026-07-07)`, profil `release` |
| Code | worktree `feat/ord-graph`, HEAD `c2f2335` (lot 0) **plus les modifications non commitées du lot 0 bis** ; binaire `target/release/ord-graph-build` du 15 novembre 16 h 24, aucun source plus récent (`find -newer` vide), `cargo build --release` sans recompilation à 17 h 30 |
| Extrait | `pays-de-la-loire-latest.osm.pbf`, md5 `ffe01f1d9e3a38a40f7b2a8bb31fa8f8` (revérifié), graphe de base `pays-de-la-loire-20261114-z11.ordg` du lot 0 (format 1.0, non reconstruit) |
| Vues | reconstruites pour ce relevé (§4) dans `~/.cache/osm-route-details/graph/lot0bis-mesures/` |

**Étiquette fausse du JSON** : le champ `environment.commit` des rapports vaut `ba86fd64a`, le HEAD du dépôt Pédalons
d'où le processus a été lancé (`git rev-parse` dans le répertoire courant), pas celui d'osm-route-details. Sans effet
sur les chiffres ; non corrigé (ce n'est pas une mesure).

**Charge** **[mesuré]** : `/proc/loadavg` relevé toutes les 15 s pendant les trois passes (272 relevés, le processus
de mesure compte pour 1) : médiane **1,89**, p90 2,33, max 3,85 (de 18 h 10 à 18 h 13, un onglet Chrome, pendant la
passe 2). Aucun `cargo`, `rustc` ni banc Java parmi les quatre processus les plus gourmands à aucun relevé ; les
conteneurs du poste (un backend Quarkus au repos) tournaient. Recherches mono-fil, 15 fils libres. L'effet de la charge
se lit dans l'écart entre passes (§6) : ≤ 8 % pour `dense` et `local`, ≤ 15 % pour `map`.

## 3. Contrôle de correction (`measure --check`)

```sh
G=~/.cache/osm-route-details/graph; D=$G/lot0bis-mesures
target/release/ord-graph-build measure --graph $G/pays-de-la-loire-20261114-z11.ordg \
  --views road,gravel,all,road-prune,gravel-prune,all-prune --view-dir $D --poly $G/pays-de-la-loire.poly \
  --check --check-starts 10 --skip-timing --out $D/check.json
```

Code de sortie **0**, 34,6 s, pic 880 Mo **[mesuré]**. Pour chaque vue : structure exhaustive (chaque chaîne contiguë,
longueurs, poids et sens exacts, chaque arête de base vivante dans exactement une chaîne, empreinte de la base), puis
les distances de **tous** les nœuds de la vue depuis 10 départs, égales à un Dijkstra par nœuds sur le graphe de base
filtré (sans disque ni interdiction de demi-tour), puis les trois états d'accord (mêmes distances, mêmes états fixés et
poussés, 25 km, demi-tour interdit) sur la base et sur chaque vue.

| Vue | Arêtes de vue | Arêtes de base couvertes | Distances comparées | Atteintes | Écarts | Chaîne la plus longue | Paires parallèles | Durée |
|---|---|---|---|---|---|---|---|---|
| `road` | 589 715 | 914 349 | 4 770 740 | 4 670 800 | **0** | 38 | 12 371 | 3,1 s |
| `gravel` | 893 341 | 1 168 842 | 7 023 480 | 6 837 110 | **0** | 36 | 14 812 | 4,9 s |
| `all` | 1 583 012 | 1 653 822 | 12 200 460 | 12 021 810 | **0** | 10 | 22 391 | 9,9 s |
| `road-prune` | 337 934 | 697 594 | 2 225 840 | 2 209 180 | **0** | 42 | 18 509 | 1,7 s |
| `gravel-prune` | 560 876 | 923 182 | 3 646 070 | 3 613 870 | **0** | 42 | 22 673 | 2,9 s |
| `all-prune` | 1 110 520 | 1 341 085 | 7 461 740 | 7 377 890 | **0** | 35 | 29 445 | 6,0 s |

Les trois passes chronométrées (§6) refont, sans `--check`, les mêmes recherches sur les trois états : pour chaque
graphe, chaque rayon et **chaque départ**, le nombre d'états fixés est le même dans les trois passes, les trois états,
à froid et à chaud (vérifié depuis les JSON, 0 écart).

## 4. Construction des vues

```sh
/usr/bin/time -v target/release/ord-graph-build views --graph $G/pays-de-la-loire-20261114-z11.ordg \
  --views road,gravel,all,road-prune,gravel-prune,all-prune --out-dir $D --report $D/views-build.json
```

Six vues en **5,9 s** de mur (0,4 à 1,3 s chacune, chacune relue et contrôlée contre la base), pic **618 Mo**
(`time -v`), un fil **[mesuré]**. Les six fichiers sont identiques octet pour octet, hors en-tête de 4 Kio, à ceux
construits une heure plus tôt pour la relecture (`cmp -i 4096`) : la construction est déterministe.

## 5. Base contre vues : tailles, et ce que chaque vue retire

### 5.1 Tailles (**[mesuré]**, identiques sur les trois passes)

| Graphe | Nœuds | Arêtes | Orientées | km | m / arête | Fichier | o / arête | Topologie o / arête | Arêtes à 50 km (méd. tous / intérieurs) | Départs rattachés (méd. / max) |
|---|---|---|---|---|---|---|---|---|---|---|
| `base` | 1 290 856 | 1 653 822 | 3 145 589 | 165 290 | 99,9 | 124,5 Mo | 75,3 | 40,7 | 377 866 / 539 515 | 0 |
| `road` | 477 074 | 589 715 | 1 078 870 | 95 747 | 162,4 | 60,2 Mo | 102,1 | 40,6 | 128 413 / 189 334 | 19 (48 m / 570 m) |
| `gravel` | 702 348 | 893 341 | 1 680 643 | 143 511 | 160,6 | 88,6 Mo | 99,1 | 40,7 | 190 191 / 292 541 | 16 (48 m / 252 m) |
| `all` | 1 220 046 | 1 583 012 | 3 011 712 | 165 290 | 104,4 | 144,2 Mo | 91,1 | 40,6 | 366 161 / 516 352 | 1 (11 m / 11 m) |
| `road-prune` | 222 584 | 337 934 | 581 900 | 74 438 | 220,3 | 36,6 Mo | 108,4 | 38,0 | 72 210 / 111 436 | 31 (110 m / 931 m) |
| `gravel-prune` | 364 607 | 560 876 | 1 021 636 | 114 904 | 204,9 | 57,8 Mo | 103,1 | 38,7 | 116 046 / 193 381 | 25 (69 m / 547 m) |
| `all-prune` | 746 174 | 1 110 520 | 2 072 650 | 134 379 | 121,0 | 101,9 Mo | 91,7 | 39,2 | 253 164 / 381 216 | 12 (69 m / 339 m) |

- La **topologie** coûte le même nombre d'octets par arête dans une vue que dans la base (38 à 40,7 o) : le surpoids
  d'une vue par arête (91 à 108 o contre 75) est fait de géométrie complète des chaînes, d'identités, des chaînes et
  des poids. `road` pèse la moitié de la base, `all` 16 % de plus.
- **Fusion seule** (`all`, aucun filtre) : −4,3 % d'arêtes. **Filtre puis fusion** (`road`) : −64 %. C'est le filtre
  qui crée les nœuds de degré 2 : 328 967 nœuds fondus dans `road` contre 77 461 dans `all`.
- Médianes d'arêtes de disque : médiane statistique des 50 départs (moyenne des deux valeurs centrales), comme au
  lot 0 ; l'outil imprime l'élément `n/2` (base : 388 852 au lieu de 377 866, les deux départs centraux étant éloignés).

### 5.2 Ce que retirent `road` et `gravel`, par classe (**[mesuré]**, exact)

Comptes exacts sur les arêtes de base vivantes de chaque vue (`alive_edges`, l'ensemble que le contrôle de §3 a
vérifié couvert une fois par les chaînes), par une sonde hors dépôt
(`lot0bis-mesures/scripts/lot0bis-probe`, `km-par-classe.json`). Les `graph_stats` d'une vue dans le JSON de `measure`
**ne conviennent pas** pour cela : ils comptent chaque chaîne avec les attributs approchés de sa plus longue arête et
l'OU des itinéraires (`road` : itinéraires nationaux 1 150 km dans `graph_stats` contre **1 073** km exacts, VTT 1 119
contre **692**).

| Classe | km base | `road` garde | `road` retire | `gravel` garde | `gravel` retire |
|---|---|---|---|---|---|
| `primary` (+ `_link`) | 3 741 | 3 698 | 43 (1,1 %) | 3 698 | 43 |
| `secondary` (+ `_link`) | 7 419 | 7 389 | 31 (0,4 %) | 7 389 | 31 |
| `tertiary` (+ `_link`) | 15 843 | 15 838 | 6 (0,04 %) | 15 839 | 5 |
| `unclassified` | 40 445 | 39 957 | **488 (1,2 %)** | 40 256 | 189 |
| `residential` | 16 758 | 16 581 | 177 (1,1 %) | 16 671 | 86 |
| `living_street` | 341 | 331 | 10 | 333 | 8 |
| `trunk`, `motorway_link`, `road`, `busway` (+ `_link`) | 46 | 17 | 28 | 18 | 28 |
| **Sous-total routes** | **84 593** | **83 810** | **783 (0,93 %)** | **84 203** | **390 (0,46 %)** |
| `service` | 20 541 | 10 744 | 9 797 (48 %) | 11 023 | 9 518 |
| `cycleway` | 1 452 | 1 193 | **259 (18 %)** | 1 448 | 4 |
| `track` | 38 155 | 0 | 38 155 | 35 862 | 2 293 |
| `path` | 11 129 | 0 | 11 129 | 10 718 | 412 |
| `footway` | 8 773 | 0 | 8 773 | 243 | 8 530 |
| `bridleway`, `pedestrian`, `steps` | 647 | 0 | 647 | 15 | 632 |
| **Total** | **165 290** | **95 747** | **69 543** | **143 511** | **21 779** |

Motifs du filtre `road` (dans cet ordre de priorité ; toutes classes) : accès `No`/`Private` 7 427 km ;
`service=driveway|parking_aisle|drive-through|emergency_access` 5 994 km ; surface connue non revêtue (classe routière
ou `cycleway`) 8 533 km ; classe hors filtre 47 589 km.

**Lecture.**
- **`road` ne jette pas de vraies routes** : 0,93 % des km des classes routières, par une surface connue non revêtue
  ou un accès `Private`/`No` (des chemins d'exploitation tagués `unclassified`, des voies privées). La moitié de
  `service` part, dont 5 994 km d'allées, de parkings et de voies d'accès ; l'autre moitié (`service` sans sous-type)
  reste.
- **`road` jette les voies vertes** : 1 235 km de `path`/`footway` avec `bicycle=designated` et sans surface non
  revêtue (18 778 arêtes de base), dont 603 km de surface connue revêtue, plus 255 km de `cycleway` à surface connue non
  revêtue (`compacted`, `fine_gravel`… ; 1 372 arêtes) **[mesuré**, sonde de relecture `kmprobe`, relancée pour ce
  relevé]. Sur les itinéraires cyclables : `road` garde **62 %** des km nationaux (1 073 sur 1 738), 61 % des régionaux
  (1 601 sur 2 607), 74 % des locaux (2 145 sur 2 895), 38 % des VTT (692 sur 1 832) ; `gravel` en garde 98 à 99 %.
  Les ajouter rendrait à `road` ≈ 2 % d'arêtes de base (≈ 20 000 sur 914 349) **[calculé]** : sans effet sur les
  verdicts de taille et de temps, mais la **connexité** des corridors (Loire à Vélo) dans `road` n'est pas mesurée.
- **Revêtement présumé** : 67 426 des 95 747 km de `road` (**70,4 %**) n'ont aucun `surface` ; la décision « surface
  absente gardée » (conception §11.2) porte donc sur l'essentiel de la vue. `gravel` : 74,2 %.
- **`gravel`** retire surtout des trottoirs (8 530 km de `footway` non désignés) et les `service` à sous-type ;
  0,46 % des routes et 6 % des `track` (accès `Private`/`No`).

## 6. Recherche bornée : base et vues, trois états

```sh
target/release/ord-graph-build measure --graph $G/pays-de-la-loire-20261114-z11.ordg \
  --views road,gravel,all,road-prune,gravel-prune,all-prune --view-dir $D \
  --state dense,local,map --starts 50 --seed 20261115 --radii 15,25,50,75 --warm-runs 3 \
  --poly $G/pays-de-la-loire.poly --out $D/measure-<i>.json        # i = 1, 2, 3 (run-passes.sh)
```

Trois passes de 22 min 18 s, 22 min 44 s et 23 min 01 s, code de sortie 0, pic RSS du processus 779 à 785 Mo (sept
graphes projetés, ce n'est pas la mémoire d'une requête) **[mesuré]**. Protocole du lot 0 : 50 départs tirés une fois
dans la base (graine 20261115) ; pour une vue, un départ fondu ou élagué est rattaché au nœud de vue le plus proche
(tableau 5.1) et le disque est centré sur lui ; arbre complet du disque, arêtes orientées, demi-tour interdit hors
cul-de-sac ; **froid** = vue neuve après `posix_fadvise(DONTNEED)`, une exécution, état déjà alloué (dense) ou
dimensionné (local) ; **chaud** = un tour jeté, trois tours, médiane par départ. Chaque requête froide de chaque passe a
eu des défauts majeurs (`cold_not_guaranteed` = 0 partout). « Intérieurs » : disque entièrement dans le `.poly`, même
classement pour la base et les vues (position du départ tiré) : 36 / 28 / 8 / 0 départs à 15 / 25 / 50 / 75 km.

Chaque cellule : **médiane des trois passes** du chaud médiane / p90 · froid médiane / p90 (ms) ; puis la mémoire de
l'état, médiane des départs (max), en Mo. Dernière colonne : écart maximal entre les trois passes sur la médiane chaude.

### 6.1 État `local` (retenu, §8)

Tous les départs (50) :

| Graphe | 15 km | 25 km | 50 km | 75 km | Écart entre passes |
|---|---|---|---|---|---|
| `base` | 11,7 / 61,0 · 17,6 / 78,2 ; 2,2 (6,8) | 27,2 / 82,4 · 39,5 / 105,9 ; 5,1 (9,3) | 116,7 / 173,8 · 154,6 / 222,9 ; 13,0 (20,5) | 231,9 / 313,7 · 302,9 / 391,7 ; 24,1 (29,9) | 6 % |
| `road` | 2,9 / 9,3 · 5,9 / 14,2 ; 0,9 (2,1) | 7,1 / 15,1 · 11,8 / 22,3 ; 2,0 (3,2) | **25,7 / 39,3** · 39,1 / 56,4 ; **4,5 (7,2)** | 55,0 / 69,1 · 74,8 / 97,0 ; 8,1 (10,5) | 4 % |
| `gravel` | 4,5 / 15,9 · 8,4 / 22,2 ; 1,3 (3,3) | 11,1 / 26,4 · 18,0 / 37,5 ; 2,8 (4,9) | 41,8 / 67,7 · 61,1 / 93,0 ; 7,0 (10,7) | 92,3 / 120,4 · 122,4 / 161,5 ; 12,2 (15,7) | 8 % |
| `all` | 8,9 / 42,5 · 14,2 / 56,1 ; 2,2 (6,5) | 20,3 / 58,3 · 30,6 / 78,1 ; 5,0 (8,8) | 88,1 / 129,8 · 123,8 / 172,8 ; 12,5 (19,4) | 176,2 / 236,9 · 235,3 / 304,0 ; 22,9 (28,5) | 5 % |
| `road-prune` | 1,8 / 5,2 · 3,9 / 8,8 ; 0,5 (1,3) | 3,7 / 8,3 · 7,4 / 13,5 ; 1,1 (1,9) | 14,0 / 21,8 · 21,9 / 32,6 ; 2,7 (4,2) | 29,7 / 38,8 · 42,6 / 55,5 ; 4,7 (6,0) | 3 % |
| `gravel-prune` | 2,8 / 10,5 · 5,8 / 15,6 ; 0,9 (2,3) | 6,8 / 17,8 · 11,7 / 25,3 ; 1,7 (3,3) | 24,8 / 43,2 · 37,8 / 61,2 ; 4,6 (7,0) | 55,8 / 74,2 · 73,9 / 100,8 ; 7,7 (10,2) | 6 % |
| `all-prune` | 6,2 / 33,3 · 10,0 / 45,3 ; 1,6 (5,1) | 13,8 / 45,3 · 21,2 / 60,9 ; 3,5 (6,8) | 59,3 / 90,4 · 82,8 / 122,3 ; 9,2 (14,3) | 124,8 / 161,8 · 163,0 / 208,1 ; 16,1 (20,6) | 4 % |

Départs intérieurs (36 / 28 / 8 ; **8 à 50 km : peu fiable** ; aucun à 75 km) :

| Graphe | 15 km | 25 km | 50 km | Écart entre passes |
|---|---|---|---|---|
| `base` | 19,2 / 62,6 · 26,6 / 79,6 ; 3,3 (6,8) | 63,3 / 84,7 · 83,6 / 109,5 ; 7,5 (9,3) | 167,7 / 172,4 · 217,7 / 220,5 ; 17,5 (19,1) | 2 % |
| `road` | 3,4 / 9,4 · 7,0 / 14,5 ; 1,1 (2,1) | 8,9 / 15,7 · 16,5 / 23,3 ; 2,6 (3,2) | **38,5 / 38,9** · 55,4 / 56,4 ; **6,4 (7,0)** | 5 % |
| `gravel` | 5,5 / 16,2 · 10,6 / 23,1 ; 1,6 (3,3) | 15,5 / 27,0 · 26,5 / 39,1 ; 4,0 (4,9) | 66,3 / 67,7 · 92,1 / 93,0 ; 9,8 (10,6) | 14 % |
| `all` | 13,8 / 43,8 · 20,5 / 58,3 ; 3,2 (6,5) | 44,7 / 59,6 · 60,7 / 83,4 ; 7,0 (8,8) | 125,4 / 126,5 · 166,7 / 170,1 ; 16,9 (18,4) | 4 % |
| `road-prune` | 2,2 / 5,4 · 4,5 / 8,9 ; 0,7 (1,3) | 4,8 / 8,5 · 9,5 / 13,8 ; 1,5 (1,9) | 21,1 / 21,7 · 31,8 / 32,6 ; 3,9 (4,2) | 2 % |
| `gravel-prune` | 3,8 / 11,3 · 6,7 / 16,0 ; 1,1 (2,3) | 9,5 / 18,2 · 18,0 / 26,7 ; 2,6 (3,3) | 42,5 / 43,2 · 59,5 / 61,2 ; 6,4 (6,9) | 7 % |
| `all-prune` | 9,5 / 34,1 · 15,0 / 46,3 ; 2,3 (5,1) | 33,2 / 45,9 · 43,7 / 62,3 ; 5,1 (6,8) | 88,0 / 88,8 · 121,0 / 122,3 ; 12,4 (13,3) | 6 % |

### 6.2 État `dense` (tableau sur tout le graphe, référence du lot 0)

Tous les départs (50) ; l'état fait 24 o × arêtes du graphe, quel que soit le rayon :

| Graphe | 15 km | 25 km | 50 km | 75 km | État | Écart entre passes |
|---|---|---|---|---|---|---|
| `base` | 10,9 / 57,6 · 16,7 / 75,3 | 25,2 / 78,3 · 36,4 / 104,2 | 108,4 / 164,4 · 152,7 / 218,4 | 219,0 / 297,9 · 292,6 / 374,0 | 39,7 Mo | 6 % |
| `road` | 2,8 / 8,9 · 5,9 / 13,9 | 6,6 / 14,5 · 11,4 / 21,5 | 25,3 / 37,6 · 37,7 / 56,8 | 53,4 / 68,3 · 75,4 / 96,6 | 14,2 Mo | 4 % |
| `gravel` | 4,2 / 14,6 · 8,4 / 21,5 | 10,5 / 25,6 · 18,0 / 36,7 | 41,1 / 66,2 · 59,3 / 90,9 | 88,6 / 115,8 · 118,3 / 154,5 | 21,4 Mo | 5 % |
| `all` | 8,4 / 40,6 · 13,2 / 54,8 | 19,8 / 56,9 · 30,2 / 75,5 | 85,5 / 125,6 · 118,4 / 164,9 | 175,5 / 228,2 · 221,7 / 295,7 | 38,0 Mo | 7 % |
| `road-prune` | 1,6 / 5,1 · 3,8 / 8,5 | 3,6 / 8,1 · 7,5 / 13,5 | 13,1 / 20,7 · 21,0 / 32,3 | 28,6 / 37,7 · 41,2 / 52,7 | 8,1 Mo | 4 % |
| `gravel-prune` | 2,7 / 9,9 · 5,7 / 14,9 | 6,5 / 17,1 · 11,1 / 24,3 | 24,2 / 41,5 · 36,9 / 58,1 | 54,4 / 72,1 · 74,0 / 95,6 | 13,5 Mo | 7 % |
| `all-prune` | 6,0 / 32,3 · 9,9 / 43,3 | 13,4 / 43,6 · 21,1 / 59,5 | 58,1 / 87,8 · 79,5 / 118,0 | 119,4 / 154,9 · 155,7 / 204,1 | 26,7 Mo | 4 % |

Départs intérieurs, 50 km (8) : base 159,6 / 162,4 · 208,9 / 218,3 ; `road` 37,0 / 37,3 · 56,3 / 56,8 ; `gravel`
64,6 / 66,1 · 87,4 / 90,7 ; `all` 122,2 / 122,5 ; `road-prune` 20,2 / 20,4 ; `gravel-prune` 40,8 / 41,5 ; `all-prune`
86,7 / 87,8 (15 et 25 km dans `tables.md`).

La base en dense reproduit le lot 0 : 108,4 ms à 50 km contre 111,3 [106,8–115,2] au lot 0 (la passe sans banc Java y
donnait 106,4), mêmes états fixés (727 989, élément `n/2`).

### 6.3 État `map` (table de hachage)

Tous les départs (50) ; la mémoire est la capacité de la table (puissance de deux), médiane (max) :

| Graphe | 15 km | 25 km | 50 km | 75 km | Écart entre passes |
|---|---|---|---|---|---|
| `base` | 13,6 / 80,0 · 20,0 / 100,5 ; 1,5 (6,0) | 34,1 / 120,7 · 46,6 / 151,6 ; 3,0 (11,9) | 187,6 / 300,0 · 237,5 / 366,3 ; 11,9 (23,9) | 395,6 / 568,9 · 508,3 / 666,7 ; 23,9 (47,7) | 10 % |
| `road` | 3,6 / 11,8 · 6,7 / 16,7 ; 0,7 (1,5) | 9,1 / 20,5 · 14,9 / 28,9 ; 1,5 (3,0) | 36,9 / 61,8 · 52,2 / 84,4 ; 6,0 (6,0) | 90,1 / 125,4 · 117,6 / 156,7 ; 11,9 (11,9) | 13 % |
| `gravel` | 5,3 / 20,6 · 9,5 / 29,3 ; 0,7 (3,0) | 14,0 / 40,4 · 23,4 / 52,8 ; 1,5 (6,0) | 67,1 / 118,0 · 91,2 / 148,8 ; 6,0 (11,9) | 162,4 / 223,5 · 207,5 / 287,6 ; 11,9 (23,9) | 10 % |
| `all` | 11,0 / 63,4 · 17,0 / 84,7 ; 1,5 (6,0) | 27,1 / 94,4 · 40,4 / 127,2 ; 3,0 (11,9) | 155,8 / 248,4 · 197,9 / 305,4 ; 11,9 (23,9) | 381,8 / 531,6 · 431,8 / 581,8 ; 23,9 (23,9) | 15 % |
| `road-prune` | 2,0 / 6,3 · 4,5 / 9,8 ; 0,4 (0,7) | 4,6 / 10,4 · 8,2 / 16,6 ; 0,7 (1,5) | 18,4 / 30,4 · 28,8 / 45,5 ; 3,0 (3,0) | 42,2 / 59,3 · 59,9 / 81,0 ; 6,0 (6,0) | 5 % |
| `gravel-prune` | 3,3 / 13,0 · 6,4 / 18,8 ; 0,4 (1,5) | 8,5 / 25,0 · 14,3 / 35,0 ; 1,5 (3,0) | 34,7 / 68,9 · 51,1 / 94,6 ; 3,0 (6,0) | 92,1 / 133,7 · 122,5 / 163,4 ; 6,0 (11,9) | 5 % |
| `all-prune` | 8,0 / 50,9 · 11,9 / 62,6 ; 1,5 (6,0) | 18,8 / 72,8 · 26,9 / 91,2 ; 3,0 (6,0) | 103,9 / 172,3 · 130,5 / 210,0 ; 11,9 (11,9) | 241,4 / 335,2 · 287,1 / 399,9 ; 23,9 (23,9) | 12 % |

Intérieurs, 50 km : base 296,6 / 300,0 ; `road` 57,0 / 58,7 ; `gravel` 112,9 / 116,5 ; `road-prune` 28,6 / 29,5.

### 6.4 Contenu des disques (médiane statistique des 50 départs ; intérieurs entre parenthèses : arêtes ; km)

| Graphe | 15 km : arêtes ; km ; m / arête ; états fixés | 25 km | 50 km | 75 km |
|---|---|---|---|---|
| `base` | 45 548 ; 3 810 ; 83,6 ; 83 846 (66 330 ; 4 904) | 99 483 ; 10 110 ; 101,6 ; 183 888 (209 300 ; 12 859) | 377 866 ; 35 990 ; 95,2 ; 709 321 (539 515 ; 45 948) | 731 401 ; 69 205 ; 94,6 ; 1 367 620 |
| `road` | 17 069 ; 2 280 ; 133,6 ; 30 488 (20 876 ; 2 612) | 38 696 ; 5 639 ; 145,7 ; 69 414 (49 105 ; 6 557) | **128 413 ; 20 671 ; 161,0 ; 232 916 (189 334 ; 25 799)** | 251 962 ; 39 985 ; 158,7 ; 452 927 |
| `gravel` | 23 874 ; 3 282 ; 137,5 ; 43 768 (30 628 ; 3 917) | 54 328 ; 8 643 ; 159,1 ; 99 156 (71 632 ; 10 074) | 190 191 ; 31 009 ; 163,0 ; 349 620 (292 541 ; 39 813) | 381 090 ; 59 432 ; 156,0 ; 703 042 |
| `all` | 43 724 ; 3 809 ; 87,1 ; 80 512 (62 962 ; 4 903) | 95 984 ; 10 108 ; 105,3 ; 177 776 (190 582 ; 12 856) | 366 161 ; 35 986 ; 98,3 ; 687 806 (516 352 ; 45 945) | 698 854 ; 69 202 ; 99,0 ; 1 309 573 |
| `road-prune` | 9 782 ; 1 761 ; 180,0 ; 16 728 (13 383 ; 2 072) | 22 738 ; 4 501 ; 198,0 ; 38 241 (28 752 ; 5 238) | 72 210 ; 16 192 ; 224,2 ; 123 638 (111 436 ; 20 726) | 143 883 ; 30 606 ; 212,7 ; 243 690 |
| `gravel-prune` | 15 198 ; 2 727 ; 179,4 ; 27 544 (21 082 ; 3 191) | 34 402 ; 7 031 ; 204,4 ; 60 624 (46 681 ; 8 262) | 116 046 ; 24 888 ; 214,5 ; 208 038 (193 381 ; 32 618) | 238 610 ; 47 476 ; 199,0 ; 429 288 |
| `all-prune` | 32 054 ; 3 138 ; 97,9 ; 57 908 (47 316 ; 4 018) | 68 752 ; 8 389 ; 122,0 ; 123 478 (143 726 ; 10 834) | 253 164 ; 29 666 ; 117,2 ; 468 770 (381 216 ; 38 547) | 493 720 ; 56 378 ; 114,2 ; 907 298 |

Pages du fichier lues par une requête froide (`RssFile` max, 50 km) : base 30,8 Mo, `road` 14,4, `gravel` 18,0,
`road-prune` 10,6 Mo ; 75 km : 47,6, 19,9, 26,4, 13,9 Mo **[mesuré]**.

## 7. Coût par état : une vue est moins chère par état, et ce n'est pas tout le mérite de la vue

Coût par état fixé (µs, médiane par départ des temps chauds / états fixés, médiane des trois passes) **[mesuré]** :

| État, rayon | base | `all` | `road` | `gravel` | `road-prune` | `gravel-prune` |
|---|---|---|---|---|---|---|
| dense, 50 km | 0,154 | 0,123 | 0,107 | 0,116 | 0,105 | 0,115 |
| local, 50 km | 0,163 | 0,126 | 0,110 | 0,119 | 0,110 | 0,119 |
| map, 50 km | 0,260 | 0,222 | 0,156 | 0,191 | 0,145 | 0,168 |
| local, 15 km | 0,141 | 0,109 | 0,095 | 0,101 | 0,096 | 0,100 |

`all` est presque la base (−4,3 % d'arêtes, mêmes cellules, mêmes attributs) et coûte **20 à 23 % de moins par état**
(× 1,25 à 1,30) : c'est l'écart de mesure annoncé en conception §11.5, désormais quantifié. Une vue **lit** son poids
dans `VIEW_WEIGHT` (4 o par arête), la base le **décode** des attributs à chaque relâchement. `road` gagne encore
× 1,15 sur `all` par état, qu'on attribue au plus petit ensemble de travail (cache), sans l'avoir isolé.

Le vrai moteur de coût (plan §6.5, surcouche d'équipe) ne lira pas un poids précalculé unique : il décodera des
attributs et lira une surcouche, donc paiera **au moins** le coût par état de la base. Temps « à coût par état de la
base » = états fixés de la vue × coût par état de la base au même rayon et au même état **[calculé]** :

| 50 km, médiane / p90 (ms) | `road` | `gravel` | `road-prune` | `gravel-prune` | base (mesuré) |
|---|---|---|---|---|---|
| dense, mesuré | 25,3 / 37,6 | 41,1 / 66,2 | 13,1 / 20,7 | 24,2 / 41,5 | 108,4 / 164,4 |
| dense, à coût de la base | 35,8 / 53,2 | 53,8 / 84,3 | 19,0 / 29,5 | 32,0 / 54,1 | — |
| local, mesuré | 25,7 / 39,3 | 41,8 / 67,7 | 14,0 / 21,8 | 24,8 / 43,2 | 116,7 / 173,8 |
| local, à coût de la base | 37,9 / 56,2 | 56,9 / 89,1 | 20,1 / 31,2 | 33,9 / 57,2 | — |
| local, intérieurs, à coût de la base | 56,4 / 57,2 | 89,8 / 91,3 | 31,3 / 31,8 | 58,0 / 58,6 | 167,7 / 172,4 |

Le gain d'une vue **par le seul nombre d'états** est donc ÷ 3,0 pour `road` (709 321 → 232 916 états fixés à 50 km)
et ÷ 5,7 pour `road-prune` ; le gain mesuré en temps (÷ 4,3 et ÷ 8,3 en dense) en ajoute un facteur ≈ 1,4 venu du
poids lu et du cache. La colonne « à coût de la base » est la borne à retenir pour la suite.

## 8. État local, dense, table de hachage

| 50 km, tous | Temps local / dense | Temps map / dense | Mémoire dense | Mémoire local méd. (max) | Mémoire map méd. (max) |
|---|---|---|---|---|---|
| base | **+7,7 %** (116,7 / 108,4) | +73 % | 39,7 Mo | 13,0 (20,5) Mo | 11,9 (23,9) Mo |
| `road` | **+1,6 %** (25,7 / 25,3) | +46 % | 14,2 Mo | 4,5 (7,2) Mo | 6,0 (6,0) Mo |
| `gravel` | +1,7 % | +63 % | 21,4 Mo | 7,0 (10,7) Mo | 6,0 (11,9) Mo |
| `road-prune` | +6,9 % | +40 % | 8,1 Mo | 2,7 (4,2) Mo | 3,0 (3,0) Mo |

- **L'état local coûte 1,6 à 8 % de temps** sur le dense selon le graphe et le rayon (`road` : +3,6 / +7,6 / +1,6 /
  +3,0 % à 15 / 25 / 50 / 75 km), **la table de hachage 40 à 73 %** à 50 km.
  Le lot 0 avait laissé ouvert le choix « table de hachage bornée, +84 % » contre « tableau dense local, non essayé » :
  le second est mesuré, il gagne.
- **Mémoire** : l'état local suit les cellules que le disque touche, pas le graphe. Sur la région, un disque de 50 km
  touche 58 des 245 cellules, d'où un gain de « seulement » × 3 sur le dense. Pour la France, le dense grandit avec le
  graphe (§10), le local non.
- L'état local et la table de hachage sont du même ordre (`road` 75 km : 8,1 contre 11,9 Mo ; base : 24,1 contre
  23,9 Mo) : le premier réserve des pages pour toutes les arêtes des cellules touchées, y compris hors du disque ; la
  seconde arrondit sa capacité à une puissance de deux.

## 9. Le levier « élagage des impasses », à part

| Passage | Arêtes | Arêtes à 50 km (méd.) | Arbre 50 km, local, chaud méd. / p90 | km gardés | km de routes élagués | Départs rattachés (méd. / max) |
|---|---|---|---|---|---|---|
| `road` → `road-prune` | 589 715 → 337 934 (**−43 %**) | 128 413 → 72 210 (−44 %) | 25,7 / 39,3 → **14,0 / 21,8** ms (−46 %) | 95 747 → 74 438 (**−22 %**) | **14 272 km (16,9 % des classes routières)** : `unclassified` 9 443, `residential` 4 229, `tertiary` 302, `secondary` 121, `primary` 85 ; plus `service` 6 819 et `cycleway` 219 | 31 sur 50 (110 m / 931 m) |
| `gravel` → `gravel-prune` | 893 341 → 560 876 (−37 %) | 190 191 → 116 046 (−39 %) | 41,8 / 67,7 → 24,8 / 43,2 ms (−41 %) | 143 511 → 114 904 (−20 %) | 8 883 km (10,5 %) ; plus `track` 12 356 | 25 sur 50 (69 m / 547 m) |
| `all` → `all-prune` | 1 583 012 → 1 110 520 (−30 %) | 366 161 → 253 164 (−31 %) | 88,1 / 129,8 → 59,3 / 90,4 ms (−33 %) | 165 290 → 134 379 (−19 %) | 7 499 km (8,9 %) | 12 sur 50 (69 m / 339 m) |

**[mesuré]**, km élagués par la sonde de §5.2. L'élagage presque **double la vitesse** de `road` mais retire **un
cinquième de ses kilomètres**, dont 14 272 km de vraies routes : des impasses réelles, et surtout des routes que le
filtre a rendues impasses (une `unclassified` qui continue en `track` devient un cul-de-sac dans `road`, puis disparaît).
Une boucle n'emprunte pas d'impasse, mais son départ, son arrivée et ses destinations candidates peuvent y être : 31
des 50 départs tirés (62 %) ne sont pas des nœuds de `road-prune` (fondus ou élagués) et sont déplacés, jusqu'à 931 m,
contre 19 et 570 m au plus dans `road`. L'élagage n'est donc utilisable que
pour la partie centrale de la génération (squelette, recherches entre destinations), avec un raccord au départ et à
l'arrivée sur la vue non élaguée **[non mesuré]**.

## 10. Extrapolation à la France (méthode du lot 0)

Règle retenue au lot 0 : nombre de ways `highway=*` de chaque classe en France / dans la région (taginfo Geofabrik,
2026-11-14), appliqué aux arêtes de chaque classe (× 14,37 sur la base, 23,8 M d'arêtes). Pour une vue, le facteur est
pondéré par les arêtes de base **qu'elle garde** (`road` × 13,98, `gravel` × 14,85, `road-prune` × 14,07,
`gravel-prune` × 14,77, `all` × 14,37, `all-prune` × 14,32 — `road` garde moins de `track` et `path`, qui pèsent plus
en France) et appliqué aux arêtes de la vue, en supposant le même taux de fusion qu'ici. Fourchette : règles « poids du
PBF » (× 13,67) et « surface » (× 16,95). Tout est **[calculé]**.

| Graphe | Arêtes France (fourchette) | Fichier | Topologie | État dense par recherche | État local à 50 km (région, intérieurs : méd. / max) |
|---|---|---|---|---|---|
| `base` | 23,8 M (22,6–28,0) | 1,79 Go | 0,97 Go | **0,57 Go** | 17,5 / 19,1 Mo |
| `road` | **8,2 M** (8,1–10,0) | **0,84 Go** | 0,33 Go | 0,20 Go | **6,4 / 7,0 Mo** |
| `gravel` | 13,3 M (12,2–15,1) | 1,32 Go | 0,54 Go | 0,32 Go | 9,8 / 10,6 Mo |
| `all` | 22,8 M (21,6–26,8) | 2,07 Go | 0,92 Go | 0,55 Go | 16,9 / 18,4 Mo |
| `road-prune` | 4,8 M (4,6–5,7) | 0,52 Go | 0,18 Go | 0,11 Go | 3,9 / 4,2 Mo |
| `gravel-prune` | 8,3 M (7,7–9,5) | 0,85 Go | 0,32 Go | 0,20 Go | 6,4 / 6,9 Mo |

- **Topologie** : `road` France ≈ 0,33 Go, sous le budget de 0,5 Go du plan ; `road` + `gravel` ≈ 0,87 Go de topologie
  et 2,2 Go de fichiers, en plus de la base si elle reste servie (géométrie et surcouche par arête de base).
- **État local** : il ne dépend que des cellules touchées et de leur densité ; on transpose les disques intérieurs de
  la région, ≈ 6 à 7 Mo par recherche à 50 km sur `road`, plus la table des pages (4 o par 256 arêtes, 0,13 Mo pour
  `road` France). Contre 0,20 Go en dense : **× 30**.
- **Temps** : une recherche bornée ne dépend que de la densité locale (méthode du lot 0) ; on transpose les départs
  intérieurs : `road`, état local, 50 km, **38,5 / 38,9 ms** mesurés, 56,4 / 57,2 ms à coût de la base, sur 8 disques
  seulement, sans montagne ni densité parisienne.

## 11. Verdict sur la cible du plan : une génération sous 500 ms

Le plan (§4.3) compte « une dizaine » de recherches par génération : un arbre depuis le départ, un vers l'arrivée, une
par destination candidate. Modèle **pessimiste** retenu ici : **10 arbres complets du même rayon depuis le même départ**,
à chaud (le temps d'une génération pour un départ est 10 × son arbre ; médiane et p90 des départs) **[calculé à partir
du mesuré]**. Les recherches par destination candidate seront en réalité bornées, donc plus courtes **[non mesuré]**.
Rayon = moitié de la distance de la boucle : 50 km pour une boucle de 100 km.

| 10 arbres, ms (méd. / p90) | 15 km | 25 km | 50 km | 75 km | 50 km, intérieurs | 50 km, à coût de la base (tous / intérieurs méd.) |
|---|---|---|---|---|---|---|
| `base` / dense | 109 / 576 | 252 / 783 | 1 084 / 1 644 | 2 190 / 2 979 | 1 596 / 1 624 | — |
| `base` / local | 117 / 610 | 272 / 824 | 1 167 / 1 738 | 2 319 / 3 137 | 1 677 / 1 724 | — |
| **`road` / local** | 29 / 93 | 71 / 151 | **257 / 393** | 550 / 691 | **385 / 389** | 379 / 562 ; 564 |
| `road` / dense | 28 / 89 | 66 / 145 | 253 / 376 | 534 / 683 | 370 / 373 | 358 / 532 |
| `road` / map | 36 / 118 | 91 / 205 | 369 / 618 | 901 / 1 254 | 570 / 587 | — |
| `gravel` / local | 45 / 159 | 111 / 264 | 418 / 677 | 923 / 1 204 | 663 / 677 | 569 / 891 ; 898 |
| **`road-prune` / local** | 18 / 52 | 37 / 83 | **140 / 218** | **297 / 388** | 211 / 217 | 201 / 312 ; 313 |
| `gravel-prune` / local | 28 / 105 | 68 / 178 | 248 / 432 | 558 / 742 | 425 / 432 | 339 / 572 ; 580 |
| `all` / local | 89 / 425 | 203 / 583 | 881 / 1 298 | 1 762 / 2 369 | 1 254 / 1 265 | — |

**Ce qui passe, de combien, à quel rayon.**
1. **`road` + état local : oui jusqu'à 50 km de rayon.** 257 ms en médiane (marge × 1,9), 393 ms au p90 (× 1,27),
   385 ms sur les disques intérieurs (× 1,3). Le premier arbre froid ajoute ≈ 13 ms (39,1 contre 25,7 ms). **Au coût par
   état de la base** (§7), la médiane passe encore (379 ms) mais le p90 et les intérieurs dépassent (562 et 564 ms) :
   la cible est tenue **à la marge**, et le coût par état du vrai poids décidera. **À 75 km : non** (550 / 691 ms).
   À 25 km (boucles de 50 km) : 71 / 151 ms, large marge.
2. **`road-prune` + état local : oui jusqu'à 75 km en médiane** (297 ms), 388 ms au p90 ; à 50 km, 140 / 218 ms, et
   201 / 312 ms au coût de la base : la seule configuration qui tient 50 km **avec** une hypothèse pessimiste sur le
   coût par état. Contrepartie : 22 % des km retirés et des départs déplacés (§9).
3. **`gravel` : non au p90 à 50 km** (418 / 677 ms, intérieurs 663 ms) ; **`gravel-prune`** passe en médiane et au p90
   mesuré (248 / 432 ms) mais pas au coût de la base (572 ms). Le type gravel demande l'élagage ou des recherches
   bornées par destination.
4. **La base ne passe à aucun rayon utile** (1,1 à 1,2 s à 50 km) : le plan doit chercher sur une vue, pas sur le graphe
   complet.
5. **État** : `local` (+2 % sur `road`) ; `dense` est à peine plus rapide mais coûte 0,20 Go par recherche vivante pour
   `road` France ; `map` ajoute 40 à 70 % et fait sortir `road` de la cible au p90 (618 ms).

**Ce qui reste à prouver** :
- **Poids synthétique** : longueur × facteur fixe, lu dans `VIEW_WEIGHT` ; le coût par état d'une vue est 1,25 à
  1,5 fois plus faible que celui de la base (§7), et le vrai poids (préférences, surcouche d'équipe) coûtera au moins
  celui de la base. L'étendue explorée changera aussi avec le vrai poids.
- **Pas de coût de virage ni de restriction de tourner** : poussées / fixées = 1,00 partout (structurel, conception
  §9.3) ; un coût de virage ajoutera des relaxations périmées et des états.
- **Le modèle de 10 arbres** : la génération réelle (lots 2 et 4) n'existe pas ; un arbre « vers l'arrivée » suppose un
  graphe inversé de même coût ; les recherches par destination candidate ne sont pas mesurées ; aucun parallélisme
  entre arbres n'a été supposé (un seul fil).
- **Petite région** : 8 disques intérieurs à 50 km, aucun à 75 km ; pas de montagne ni de densité parisienne ; les p90
  « tous » incluent des disques tronqués par la mer et sont optimistes.
- **Demi-tour dans une vue** : l'équivalence avec la base filtrée est prouvée sans interdiction de demi-tour (§3) ; avec
  elle, une impasse à sens unique fondue dans une chaîne peut différer (conception §11.5) ; non mesuré.
- **`road` sans voies vertes** (§5.2) : la connexité des itinéraires cyclables y est réduite (62 % des km nationaux) ;
  si on les rend à `road`, ≈ +2 % d'arêtes, sans effet attendu sur ces verdicts **[calculé]**.
- **France extrapolée** (§10) ; une requête à la fois, cache de pages non partagé, NVMe local.

## 12. Fichiers

Dans `~/.cache/osm-route-details/graph/lot0bis-mesures/` (hors dépôt, régénérables) :

| Fichier | Contenu |
|---|---|
| `pays-de-la-loire-20261114-z11.{road,gravel,all,road-prune,gravel-prune,all-prune}.ordv`, `views-build.{json,out,err}` | les six vues et leur construction (`time -v` dans `.err`) |
| `check.{json,out,err,load}` | le contrôle de §3 |
| `run-passes.sh`, `passes.log`, `load.log`, `measure-{1,2,3}.{json,out,err,time}` | les trois passes, la charge relevée toutes les 15 s |
| `aggregate.txt`, `aggregate-us.txt`, `tables.md` | agrégats des trois passes (médiane [min–max]), coût par état, tableaux de ce rapport |
| `km-par-classe.{json,txt}` | arêtes et km de base gardés, filtrés et élagués par classe, motif, itinéraire et accès, pour chaque vue |
| `scripts/` | `agg0bis.py`, `agg0bis_us.py`, `md0bis.py` (agrégation, aucun chiffre recopié à la main) ; `lot0bis-probe/` (sonde Rust des km par classe, dépend du crate `ord-graph` du worktree) |
