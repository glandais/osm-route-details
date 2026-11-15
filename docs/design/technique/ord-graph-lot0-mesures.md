# Graphe vélo OSM en cellules — lot 0 : rapport de mesures

> Relevé du **15 novembre 2026**, de 15 h 24 à 15 h 45. Conception et protocole :
> [ord-graph-lot0.md](ord-graph-lot0.md) (§5). Estimations comparées : plan Pédalons
> `docs/plans/2026-11-15-generateur-parcours-osm.md`, §1, §3 et §4.3.
> **[mesuré]** : relevé sur la machine ci-dessous. **[calculé]** : dérivé d'un chiffre mesuré par une règle
> que le texte donne. **[non mesuré]** : chiffre absent à dessein, ou laissé à un lot suivant.
> Ce rapport remplace, pour ses chiffres, ceux de §9 d'ord-graph-lot0.md : il a été refait avec le binaire
> actuel, chaque recherche trois fois, et chaque rapport JSON qu'il cite contient les champs qu'il utilise.

## 1. Verdicts

| Estimation du plan | Mesure | Verdict |
|---|---|---|
| **216 m par arête** (§3) | **99,9 m** sur tout le graphe (165 290 km / 1 653 822 arêtes) ; **90,9 m** en médiane dans un disque de 50 km | **Infirmé, ÷ 2,2 à 2,4.** Les deux chiffres ne comptent pas la même chose (§6) : le plan mesurait le réseau roulé d'une équipe, ici c'est toute voie ouverte au vélo |
| **≈ 180 000 arêtes dans un disque de 50 km**, à 5 km de voie par km² (§1, §4.3) | **377 866** arêtes en médiane (tous départs), **539 515** pour les 8 disques intérieurs ; 4,60 km/km² (tous), 5,85 (intérieurs) ; 35 990 km de voies contre 39 000 au plan | **Infirmé, × 2,1 à 3,0** sur les arêtes. La **longueur** de voie est, elle, au repère (−8 %) : le surplus vient de la finesse du graphe, pas de la surface |
| **Un Dijkstra en « quelques dizaines de ms »** sur tableaux plats (§1, §4.3) | Arbre complet à 50 km, chaud : **96 ms à plat**, **111 ms** avec le lecteur par cellules (médianes, tous départs) ; p90 143 et 168 ms ; intérieurs 138 et 161 ms ; froid 147 ms | **Infirmé, × 2 à 5** (sur 30 à 50 ms). Le coût par état (0,13 µs à plat) tient le plan : ramené à 180 000 arêtes, la même recherche ferait ≈ 46 ms **[calculé]**. L'écart vient du nombre d'arêtes |
| **12 à 14 millions d'arêtes pour la France** (§3) | **≈ 24 M** **[calculé]**, fourchette 22,6 à 28 M selon la règle d'extrapolation (§5) | **Infirmé, × 1,7 à 2** |
| **≈ 1 Go pour la France** : 0,5 Go de topologie et d'attributs, autant de géométrie (§3) | Topologie et attributs **≈ 0,97 Go**, géométrie **≈ 0,29 Go**, identifiants OSM ≈ 0,53 Go : fichier **≈ 1,8 Go** **[calculé]** | **Infirmé en répartition** : topologie × 1,9, géométrie ÷ 1,7 ; topologie + géométrie ≈ 1,26 Go (+ 26 %) ; avec les identifiants, × 1,8 |
| 2,5 à 3 millions de km de voies cyclables en France (§3, hypothèse sous-jacente) | **≈ 2,4 à 2,5 M km** **[calculé]** (2,8 M par la surface) | **Confirmé**, au bas de la fourchette |

**En une phrase** : la longueur de voie et le coût d'un état de Dijkstra sont ceux du plan ; le graphe a deux fois
plus d'arêtes que prévu, et tout ce qui en dépend (arêtes par disque, temps d'un arbre, taille France) double avec lui.

## 2. Machine et données

| | |
|---|---|
| CPU | AMD Ryzen 7 4800H with Radeon Graphics, 8 cœurs, 16 fils, 1,4 à 2,9 GHz (base) |
| RAM | 31 683 016 kio (30,9 Gio), swap 31 Gio inutilisé |
| Disque | Samsung SSD 980 1 To (NVMe), ext4, 92 % plein |
| Système | Linux 7.0.0-43-generic |
| Toolchain | `rustc 1.97.0 (2d8144b78 2026-07-07)`, profil `release` |
| Code | worktree `feat/ord-graph`, HEAD `03a4b5ce8`, **modifications non commitées** (le crate `ord-graph` et l'outil ne sont pas encore commités : le JSON porte `commit` vide) ; binaire `target/release/ord-graph-build` construit à 15 h 16, après la dernière modification des sources (15 h 13) |
| Extrait | Geofabrik `pays-de-la-loire-latest.osm.pbf`, 374 234 171 octets, md5 `ffe01f1d9e3a38a40f7b2a8bb31fa8f8` (revérifié), réplication 2026-11-14T21:20:41Z |
| `.poly` | Geofabrik `pays-de-la-loire.poly` (24 Ko) |

**Charge pendant la mesure.** Un banc Java d'une autre session (`tribly-generator-bench`, ≈ 1,6 à 1,8 cœur sur 16)
a tourné jusque vers 15 h 40 : pendant les cinq constructions, les trois passes `--flat` et les passes `map-1` et
`map-2`. `map-3` et `dense-noflat` ont tourné sans lui (charge 1,9). L'effet se lit dans la dispersion : sans le
banc, la table de hachage est 13 à 19 % plus rapide à 50 km (178 contre 205 et 220 ms), le lecteur dense 0 à 8 %
plus rapide (106 contre 107 à 115 ms). Les recherches sont mono-fil, et 14 fils restaient libres ; le cache L3 et la
fréquence étaient partagés. Pas de compilation cargo pendant les mesures (vérifié par `pgrep`).

## 3. Construction

```sh
# PBF chassé du cache de pages avant la première (posix_fadvise DONTNEED ; fincore : 0 page résidente)
/usr/bin/time -v target/release/ord-graph-build build \
  --pbf ~/.cache/osm-route-details/graph/pays-de-la-loire-latest.osm.pbf \
  --out-dir ~/.cache/osm-route-details/graph/lot0-runs/<run> --report <run>/build.json [--threads 1]
```

| Construction | Mur | CPU (user + sys) | Pic RSS (`time -v`) | `VmHWM` interne | Lecture disque |
|---|---|---|---|---|---|
| 16 fils, **PBF froid** | 5,16 s | 22,0 s | 430 940 kio | 420 Mio | 730 936 blocs de 512 o (374 Mo : tout le PBF) |
| 16 fils, chaud, 1 | 4,91 s | 22,4 s | 428 984 kio | 418 Mio | 0 |
| 16 fils, chaud, 2 | 5,00 s | 21,5 s | 418 328 kio | 408 Mio | 0 |
| 16 fils, chaud, 3 | 4,86 s | 22,6 s | 434 436 kio | 424 Mio | 0 |
| **16 fils, chaud : médiane [min–max]** | **4,91 s [4,86–5,00]** | 22,4 s | **428 984 kio (439 Mo) [418 328–434 436]** | 418 Mio | |
| **1 fil** | **14,47 s** | 14,3 s | **327 720 kio (336 Mo)** | 320 Mio | 0 |

Le PBF froid coûte ≈ 0,25 s de plus (5,16 contre 4,91 s) : sur NVMe, la lecture de 374 Mo n'est pas le goulot.

**Phases** (`build.json`, mur en s ; `VmHWM` en fin de phase, Mio) :

| Phase | 16 fils, chaud 1 | 1 fil | `VmHWM` 16 fils | `VmHWM` 1 fil |
|---|---|---|---|---|
| ouverture du PBF (en-tête, xxh3) | 0,04 | 0,05 | 4 | 4 |
| passe 1 : ways et relations | 2,78 | 7,43 | 318 | 109 |
| nœuds utilisés (tri, jonctions) | 0,18 | 0,89 | 318 | 191 |
| passe 2 : nœuds | 0,60 | 4,71 | 318 | 191 |
| découpe en arêtes | 0,41 | 0,43 | 318 | 203 |
| sommets, cellules, tri des arêtes | 0,28 | 0,40 | 388 | 285 |
| écriture, relecture et `verify()` | 0,57 | 0,55 | 418 | **320** |

- **Le pic à 1 fil est dans la dernière phase** (320 Mio), pas dans « sommets, cellules, tri des arêtes » (285 Mio) :
  `write_graph` relit le fichier entier dans un `Vec` (124,5 Mo) pendant que les tableaux de construction vivent
  encore. Ce surcoût **grandit avec le graphe** ; une vérification par `open_mmap` le retirerait (non fait).
- **Le surcoût dépendant des fils** (≈ 210 Mio dès la passe 1 : 318 contre 109) vient des lots de blobs décodés en
  parallèle et des arènes de l'allocateur ; on admet qu'il **ne grandit pas** avec le graphe (hypothèse de la
  conception, non vérifiée sur un extrait plus grand).
- **Déterminisme** **[mesuré]** : les cinq fichiers produits sont identiques octet pour octet au fichier de référence
  `pays-de-la-loire-20261114-z11.ordg` sur 124 541 750 octets (tout après l'en-tête de 4 096 o, hors `STATS` ;
  `cmp -i 4096`), 1 fil et 16 fils compris.

**France** **[calculé, non mesuré]** : base = pic à **1 fil** 336 Mo × 14,4 (règle par classes, §5) ≈ **4,8 Go**,
plus ≈ 0,1 à 0,2 Go de surcoût de fils. À 16 fils, si tout grandissait (borne haute) : 439 Mo × 14,4 ≈ 6,3 Go.
L'objectif de 2 Go de la conception (§2) n'est **pas tenu** sans les leviers qu'elle nomme (libérer les tableaux
avant les sommets, tri radix en place, vérification par `mmap`, pré-filtre `osmium tags-filter`).

## 4. Taille et contenu du graphe

Comptes **identiques** sur les sept passes `measure` (déterministes). Rapport : `measure-dense-flat-1.json`.

| | Valeur |
|---|---|
| Fichier | **124 549 853 o** = `CELL_BLOCKS` 67 874 816 + `GEOMETRY` 19 865 600 + `ID_EDGES` 26 461 152 + `ID_NODES` 10 326 848 + `CELL_INDEX` 15 680 + `STATS` 1 629 |
| Nœuds / arêtes / orientées / cellules z11 | 1 290 856 / **1 653 822** / 3 145 589 / 245 |
| Octets par arête | **75,3** au total ; **40,7** de topologie (blocs : nœuds, CSR, orientées, attributs, bord) ; 12,0 de géométrie ; 22,2 d'identifiants OSM |
| Longueur | **165 290 km** ; accès `Yes` 157 380, `Private` 5 809, `No` 1 618, `Destination` 484 |
| Degrés | 1 : 260 532 ; 2 : 177 834 (dont 77 492 entre attributs identiques) ; ≥ 3 : 852 490 |
| Longueur d'arête | p50 43,6 m, p90 263 m, p99 774 m, max 5,59 km |
| Arêtes par cellule z11 | p50 4 450, p90 13 245, p99 57 216, max 102 741 |
| Divers | 162 055 sens uniques, 8 974 contresens, 14 888 arêtes de bord, 8 228 `attrs` distincts, plus grande composante 98,6 % des nœuds |
| `open` / `verify()` / comptes | 0,04 à 0,11 ms / **47 à 61 ms** / 927 à 949 ms (7 passes, cache de pages chaud) |

**Arêtes et longueur par classe** (champs `edges_by_class`, `km_by_class`, présents dans les nouveaux rapports) :

| Classe | Arêtes | km | m par arête |
|---|---|---|---|
| `service` | 356 128 | 20 541 | 58 |
| `residential` | 283 425 | 16 758 | 59 |
| `footway` | 263 161 | 8 773 | 33 |
| `unclassified` | 205 663 | 40 445 | 197 |
| `track` | 163 643 | 38 155 | 233 |
| `tertiary` | 132 567 | 15 830 | 119 |
| `path` | 90 917 | 11 129 | 122 |
| `secondary` | 80 438 | 7 401 | 92 |
| `primary` | 37 104 | 3 647 | 98 |
| `cycleway` | 17 350 | 1 452 | 84 |
| autres (`living_street`, `steps`, `pedestrian`, `bridleway`, `_link`…) | 23 426 | 1 160 | 50 |

`service` par sous-type (`edges_by_service`) : `driveway` 89 677, `parking_aisle` 82 987,
`drive-through`/`emergency_access` 1 032, autres 182 432.

## 5. Extrapolation à la France

Quatre règles **[calculé]**, toutes sur la région seule (aucun extrait France téléchargé) :

| Règle | Facteur | Arêtes | km | Fichier | Topologie | Géométrie | État dense par recherche |
|---|---|---|---|---|---|---|---|
| poids du PBF : `france-261114.osm.pbf` 5 115 762 725 o (en-tête HTTP) / 374 234 171 o | × 13,67 | 22,6 M | 2,26 M | 1,70 Go | 0,92 Go | 0,27 Go | 0,54 Go |
| nombre de ways `highway=*` des classes gardées (taginfo Geofabrik, données du 2026-11-14T21:20:30Z) : 12 442 241 / 853 984 | × 14,57 | 24,1 M | 2,41 M | 1,81 Go | 0,98 Go | 0,29 Go | 0,58 Go |
| **même source, pondérée par classe** (arêtes de chaque classe × rapport de ses ways) | × 14,37 | **23,8 M** | **2,47 M** | **1,79 Go** | **0,97 Go** | **0,29 Go** | **0,57 Go** |
| surface : 543 940 / 32 082 km² | × 16,95 | 28,0 M | 2,80 M | 2,11 Go | 1,14 Go | 0,34 Go | 0,67 Go |

**Retenu : ≈ 24 M d'arêtes, ≈ 1,8 Go de fichier**, la règle pondérée par classe. Le `.poly` de Geofabrik pour
`france` ne couvre que la métropole et la Corse (lon −6,9 à 9,9, lat 41,2 à 51,4) : pas d'outre-mer dans le PBF.

**Faiblesses de la méthode** :
- **Le poids du PBF** suit les nœuds, donc surtout le bâti (39,2 M nœuds dans l'extrait, dont 5,74 M utilisés par le
  graphe) : il mesure la densité d'import du cadastre plus que celle des routes.
- **Le nombre de ways** suppose le même nombre d'arêtes par way partout. C'est faux là où les ways sont découpées
  autrement (Paris, montagne) et là où les croisements sont plus denses : le nombre d'arêtes dépend des jonctions,
  pas des ways. Les rapports par classe vont de × 5 (`bridleway`) à × 23 (`pedestrian`) ; `track` × 17,9 et
  `path` × 18,9 pèsent au-dessus de la moyenne, `footway` × 12,2 en dessous.
- **La surface** suppose la densité de la région (5,15 km/km²) partout : les massifs en ont moins, l'Île-de-France
  plus ; c'est la borne haute.
- Aucune règle ne voit la longueur d'arête propre à une autre région. La seule mesure qui tranche est une
  construction sur un extrait plus grand (Bretagne + Centre-Val de Loire) ou sur la France (lot 6).

## 6. Ce que change « toute voie ouverte au vélo » au lieu du « réseau roulé d'une équipe »

Le plan a mesuré 216 m par arête sur le réseau de la plus grande équipe (14 979 km, 69 458 arêtes) : un graphe fait
**des seules routes roulées**, où une jonction n'existe que là où deux routes roulées se rencontrent. Ici, toute voie
ouverte au vélo est dans le graphe, et **chaque allée, trottoir, piste ou chemin qui touche une route la coupe** :

1. **Les classes rurales sont au repère, les classes fines non.** `unclassified` (197 m) et `track` (233 m) sont
   proches de 216 m, alors même qu'ici elles sont coupées par toutes les voies latérales. `service` (58 m),
   `residential` (59 m) et `footway` (33 m) font 54 % des arêtes et 28 % des km. Les seules routes et pistes
   cyclables (`primary` … `unclassified`, `_link`, `cycleway`) donnent 145 m par arête sur 474 904 arêtes ;
   avec `residential` et `living_street`, 112 m.
2. **Les deux grandeurs ne s'additionnent pas de la même façon.** Les km d'un réseau d'équipe sont un
   sous-ensemble des km OSM ; ses arêtes ne sont pas un sous-ensemble des arêtes OSM : une route roulée de 2 km
   croisée par 20 allées est une arête dans le graphe d'équipe, 21 ici. Le repère de 216 m ne se transpose donc
   pas, même en filtrant les classes.
3. **Le plan sur-estimait la densité, sous-estimait la finesse.** Dans le disque médian de 50 km : 35 990 km
   (−8 % sur 39 000), 4,60 km/km² (au lieu de 5), mais 90,9 m par arête au lieu de 216.
4. **Leviers, comptés en arêtes** (sur 1 653 822) :

   | Levier | Arêtes | Part |
   |---|---|---|
   | coutures de degré 2 entre attributs identiques, à fondre | **71 942 à 77 492** | 4,4 à 4,7 % |
   | `service=driveway`, `parking_aisle`, `drive-through`/`emergency_access`, à écarter ou replier | 173 696 | 10,5 % |
   | les deux, **union** | **173 696 à 245 638** | 10,5 à 14,9 % |
   | `footway` (33 m par arête), à replier | 263 161 | 15,9 % |

   La borne basse des coutures retire les 5 550 milieux de boucles coupées (`loops_cut`) : la construction les crée
   et les fondre ramènerait une boucle `a = b`, que le format interdit. Le compteur actuel
   (`nodes_degree_2_same_attrs`) ne les distingue pas. L'union est encadrée parce qu'une couture entre deux allées
   identiques est dans les deux comptes : le recouvrement n'est pas mesuré. Même au mieux, ces leviers ne ramènent
   pas le disque à 180 000 arêtes (il faudrait en retirer plus de la moitié).

## 7. Recherche bornée

```sh
G=~/.cache/osm-route-details/graph
target/release/ord-graph-build measure --graph $G/pays-de-la-loire-20261114-z11.ordg \
  --starts 50 --seed 20261115 --radii 15,25,50,75 --poly $G/pays-de-la-loire.poly \
  --state dense --flat --out $G/lot0-runs/measure-dense-flat-<i>.json   # i = 1, 2, 3
#   … --state map --out $G/lot0-runs/measure-map-<i>.json               # i = 1, 2, 3
#   … --state dense --out $G/lot0-runs/measure-dense-noflat.json        # RSS du processus sans la référence à plat
```

Protocole (conception §5) : 50 départs tirés une fois (SplitMix64, graine 20261115) parmi 391 839 candidats, les
mêmes pour tous les rayons et toutes les passes ; arbre complet du disque, arêtes orientées, demi-tour interdit hors
cul-de-sac. **Froid** : vue fermée, `posix_fadvise(DONTNEED)`, vue neuve, une exécution ; **chaque** requête froide
de chaque passe a eu des défauts majeurs (`cold_not_guaranteed` = 0 partout). **Chaud** : un tour jeté, trois tours,
médiane par départ. Chaque cellule ci-dessous : **médiane des trois passes [min–max des trois]** de la médiane (ou du
p90) sur les départs, en ms. États fixés, cellules et disques : identiques d'une passe à l'autre.

### 7.1 Tous les départs (50)

| Rayon | À plat, chaud méd. | p90 | Lecteur par cellules, chaud méd. | p90 | Lecteur / plat | Table de hachage, chaud méd. | p90 | Froid dense méd. | p90 | Froid table méd. |
|---|---|---|---|---|---|---|---|---|---|---|
| 15 km | 9,4 [9,2–9,5] | 50,6 [49,9–51,7] | 11,3 [10,9–11,6] | 59,2 [57,3–63,9] | 1,20 | 13,9 [13,4–15,0] | 88,9 [80,4–89,8] | 16,6 [16,1–17,8] | 71,0 [70,9–72,1] | 19,9 [19,2–22,3] |
| 25 km | 22,2 [21,0–22,5] | 66,8 [66,7–71,3] | 25,4 [25,1–25,5] | 83,6 [80,0–84,0] | 1,19 | 36,2 [33,3–36,6] | 131 [116–144] | 36,7 [36,2–38,5] | 103 [98–105] | 45,9 [45,1–47,6] |
| **50 km** | **95,9 [91,3–100,8]** | **143 [141–146]** | **111,3 [106,8–115,2]** | **168 [167–169]** | **1,18** | **205 [179–220]** | 341 [276–354] | **147 [139–150]** | 220 [212–221] | 235 [227–237] |
| 75 km | 199,5 [193,3–202,7] | 258 [254–260] | 236,6 [218,6–237,6] | 310 [303–319] | 1,19 | 426 [414–455] | 667 [569–673] | 292 [265–305] | 373 [369–494] | 517 [502–555] |

La passe `dense-noflat` (sans le banc Java, sans la référence à plat entrelacée) donne pour le lecteur
10,9 / 25,0 / **106,4** / 218,9 ms en chaud et 17,0 / 35,8 / 149,6 / 278,7 ms en froid : dans la fourchette.

### 7.2 Départs dont le disque est intérieur à la région

| Rayon | Départs | À plat, chaud méd. | Lecteur, chaud méd. | p90 | Table de hachage, chaud méd. | Froid dense méd. | p90 |
|---|---|---|---|---|---|---|---|
| 15 km | 36 | 15,2 [15,2–17,9] | 18,3 [18,1–18,6] | 59,3 [58,9–64,5] | 24,2 [22,7–25,7] | 23,9 [23,6–24,6] | 74,8 [73,0–77,8] |
| 25 km | 28 | 51,9 [51,1–52,2] | 61,5 [60,4–63,3] | 84,8 [83,4–85,1] | 104 [87–118] | 83,4 [76,2–86,6] | 106 [99–111] |
| 50 km | **8 (peu fiable)** | 137,9 [136,7–138,4] | 161,3 [159,8–163,3] | 170 [168–171] | 296 [273–307] | 202,5 [199,0–211,7] | 212 [209–217] |
| 75 km | **0** | — | — | — | — | — | — |

### 7.3 Contenu des disques, états, cellules

| Rayon | Arêtes du disque (méd.) | Orientées (méd.) | km (méd.) | km/km² | m par arête (méd.) | États fixés (méd.) | Cellules (méd. / p90) | Défauts majeurs froid (méd.) |
|---|---|---|---|---|---|---|---|---|
| 15 km | 45 548 | 86 915 | 3 810 | 5,44 | 82,0 | 85 523 | 9 / 10 | 18 |
| 25 km | 99 483 | 188 074 | 10 110 | 5,26 | 97,9 | 184 768 | 19 / 21 | 34 |
| **50 km** | **377 866** | 736 425 | **35 990** | **4,60** | **90,9** | 727 989 | 58 / 61 | 94 |
| 75 km | 731 401 | 1 425 315 | 69 205 | 3,94 | 90,1 | 1 398 534 | 101 / 113 | 161 |
| 50 km, intérieurs (8) | 539 515 | 1 021 622 | 45 948 | 5,85 | 86,3 | 1 008 819 | 60 / 62 | 129 |

(La médiane de l'outil est l'élément `n/2` du tableau trié ; celle du script d'agrégation, la moyenne des deux
éléments centraux : écarts de quelques dixièmes de pour cent sur les colonnes « orientées », sans effet sur le texte.)

- **Coût par état** (50 km, médianes) : **0,132 µs à plat**, 0,153 µs avec le lecteur, 0,28 µs avec la table de hachage.
  Poussées / fixées = **1,000** à tous les rayons (structurel sans coût de virage : conception §9.3).
- **Au repère du plan** **[calculé]** : 180 000 arêtes, soit ≈ 351 000 orientées dans la proportion mesurée
  (736 425 / 377 866), donneraient ≈ **46 ms à plat, 54 ms avec le lecteur** : « quelques dizaines de ms ».
- **Une requête de génération** en demande « une dizaine » (plan §4.3) ; si c'étaient dix arbres complets de 50 km,
  ce serait ≈ 1,1 s à chaud avec le lecteur, au-delà de la cible de 500 ms **[calculé ; les recherches par
  destination candidate du plan sont bornées et plus courtes, non mesurées]**.
- **Froid / chaud** : × 1,2 à 1,5 selon le rayon (1,47 à 15 km, 1,23 à 75 km ; rapport des médianes), sur NVMe
  local, état déjà alloué.

## 8. Mémoire du processus de requête

`time -v` ne sépare pas la requête du reste de `measure` (`verify()` et les comptes parcourent tout le fichier par
`mmap`) : la mémoire de requête se lit dans `RssFile` et `RssAnon`, relevés par requête dans `/proc/self/status`.

| Mesure **[mesuré]** | 15 km | 25 km | 50 km | 75 km |
|---|---|---|---|---|
| `RssFile` froid max (pages du fichier lues par **une** requête) | 14,1 Mo | 17,8 Mo | **30,5 Mo** | **47,3 Mo** |
| `RssAnon` froid max, état **dense** (tout le processus) | 45,0 Mo | 45,0 Mo | 45,0 Mo | 45,0 Mo |
| octets de l'état dense (`state_mb`) | 39,7 Mo | 39,7 Mo | 39,7 Mo | 39,7 Mo |
| `RssAnon` froid max, **table de hachage** (tout le processus) | 16,7 Mo | 29,7 Mo | 55,7 Mo | 55,7 Mo |
| octets de la table de hachage (`state_mb`, méd. / max) | 1,5 / 6,0 Mo | 3,0 / 11,9 Mo | **11,9 / 23,9 Mo** | 23,9 / 47,7 Mo |
| Pic RSS de tout `measure` (`time -v`) | `dense-noflat` 158 008 kio ; `map` 205 456 à 206 184 kio ; `dense --flat` 239 324 à 239 748 kio (dont la copie à plat du graphe) | | | |

Les trois passes donnent les mêmes valeurs à 0,1 Mo près. `RssAnon` chaud n'est pas isolé par requête (conception §10.11).

**L'état dense coûte à la France ce que coûte toute la topologie du plan.** `DenseState::new` alloue
2 × (arêtes du graphe) emplacements de 12 o pour **tout** le graphe, pas pour le disque : 39,7 Mo ici
(1 653 822 × 24 o), **≈ 0,57 Go par recherche vivante pour la France** **[calculé]**, soit le budget de topologie
du plan (0,5 Go) pour un seul arbre, et une requête en tient au moins deux (départ et arrivée). Trois façons de le
borner, à trancher au lot 3 :
- **table de hachage** : bornée par le disque (≤ 24 Mo à 50 km, ≤ 48 Mo à 75 km, mesurés), mais **+84 %** de temps à
  50 km (205 contre 111 ms, médianes des passes ; 178 contre 106 ms entre les deux passes sans charge, +68 %) ;
- **tableau dense local au disque**, indexé par une renumérotation des orientées des cellules touchées : ≈ 8,8 Mo à
  50 km (736 425 × 12 o) **[calculé, non implémenté, temps non mesuré]** ;
- tableau dense global partagé entre requêtes successives d'un même fil (un par fil de travail) : la mémoire
  devient 0,57 Go × fils × arbres vivants par requête **[calculé]**.

C'est la mémoire de l'**état**, pas du graphe, qui décide de la limite du conteneur : la topologie lue par une
requête tient en 30 à 47 Mo de pages, partagées entre requêtes par le cache de pages.

## 9. Réserves

- **Région trop petite pour les grands rayons** : 8 disques intérieurs à 50 km, aucun à 75 km. Les chiffres « tous
  départs » incluent des disques tronqués par la mer ou la limite régionale : ils sont **optimistes** pour la France
  (à 50 km, intérieurs 161 ms contre 111 ms tous départs).
- **Charge concurrente** pendant cinq passes sur sept et toutes les constructions (§2) : la dispersion, pas la médiane,
  la reflète ; sans charge, les temps baissent de 0 à 8 % (dense) et de 13 à 19 % (table de hachage) à 50 km.
- **Poids synthétique** (longueur × facteur fixe) : les pénalités réelles changeront l'étendue explorée et ajouteront
  des relaxations périmées (aujourd'hui aucune). **Pas de coût de virage, pas de restrictions de tourner.**
- **États par arêtes orientées** : ≈ 2,4 fois plus d'états qu'un Dijkstra par nœuds **[calculé, non mesuré]**.
- **Froid sur NVMe local**, cache de pages non partagé avec d'autres processus, un seul fil, une requête à la fois.
- **France entièrement extrapolée** (§5) : arêtes, taille, mémoire de construction et d'état ; aucune construction
  France ni sur un extrait multi-régions.
- **Fichier non vérifié** : `validate_structure()` ne contrôle pas les extrémités `a`, `b` de `ID_EDGES` contre le
  nombre de nœuds ; `geometry()` et les comptes de `measure` peuvent paniquer sur un fichier forgé qui passe
  `verify()`. Sans effet sur ces mesures (fichiers produits par `build`, vérifiés ; la recherche ne lit pas
  `ID_EDGES`), mais §4 de la conception promet trop. Code non modifié par ce relevé.
- **Coutures** : le compteur ne sépare pas les milieux de boucles coupées (§6) ; la borne basse est calculée.
- **Code non commité** : le relevé n'est pas rattaché à un commit ; le binaire, oui (horodatage).

## 10. Fichiers

Dans `~/.cache/osm-route-details/graph/lot0-runs/` (hors dépôt, régénérables) :

| Fichier | Contenu |
|---|---|
| `builds.log`, `<run>/time.txt`, `<run>/build.json` | 5 constructions (`cold16`, `warm16-1..3`, `t1`) : `time -v`, phases, charge au départ |
| `<run>/pays-de-la-loire-20261114-z11.ordg` | les 5 fichiers construits, identiques au fichier de référence hors en-tête et `STATS` |
| `measures.log`, `measure-<passe>.{json,txt,time}` | 7 passes : `dense-flat-1..3`, `map-1..3`, `dense-noflat` ; chaque JSON porte `edges_by_class` et `edges_by_service` |

Sources de l'extrapolation : en-tête HTTP de `https://download.geofabrik.de/europe/france-latest.osm.pbf`
(redirigé vers `france-261114.osm.pbf`, `Content-Length` 5 115 762 725) ; `https://download.geofabrik.de/europe/france.poly` ;
`https://taginfo.geofabrik.de/europe:france/api/4/key/values?key=highway&filter=ways` et son équivalent
`europe:france:pays-de-la-loire`.
