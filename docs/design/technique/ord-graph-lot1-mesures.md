# Graphe vélo OSM — lot 1 : relevé de la surcouche de fréquentation par équipe

> Relevé du **15 novembre 2026**, de 17 h 11 à 17 h 30. Conception : [ord-graph-lot1.md](ord-graph-lot1.md) (§2 à §9,
> « Écarts ») ; socle : [ord-graph-lot0.md](ord-graph-lot0.md) et [ord-graph-lot0-mesures.md](ord-graph-lot0-mesures.md) ;
> exigences : plan Pédalons `docs/plans/2026-11-15-generateur-parcours-osm.md` (§11, §12 lot 1, §12 bis).
> **[mesuré]** : sortie de l'outil `ord-graph-overlay` (rapport JSON cité en §2). **[sonde]** : mesuré par une sonde
> jetable hors dépôt, qui réutilise les modules de l'outil (même appariement) et ajoute ses propres tests (§9) ; ses
> chiffres de contrôle retombent sur ceux du rapport (193 069,0 km dans la région, 692,5 km de `direction_closed`).
> **[calculé]** : dérivé de chiffres mesurés par une règle que le texte donne.
> Donnée **privée** : ce relevé ne contient que des agrégats et des ids d'équipe, ni ligne, ni id de trace ou d'arête,
> ni coordonnée.

## 1. Verdicts

| Attendu (note de conception, plan) | Mesure | Verdict |
|---|---|---|
| Tout l'export en moins de **2 min** de mur, **1,5 Go** de pic (§11) ; échec au-delà de 10 min | **8,84 à 8,94 s** de mur à 16 fils (3 passes), **39,0 s** à 1 fil ; pic **720 248 à 722 692 kio (≈ 705 Mio)** (`time -v`) | **Tenu**, avec une marge de × 13 sur le mur et × 2 sur la mémoire |
| Prototype Python : **99,2 %** des km appariés (§2) | **99,22 %** des 193 069 km-passages de la région ; 99,10 à 99,40 % pour les cinq équipes de plus de 1 000 km | **Confirmé**, et sur tout l'export plutôt que sur un tirage |
| Hausdorff p99 du prototype : **0,62 m** ; alerte du contrôle 2 si p99 > 1 m (§9) | Contrôle 2 (haversine, méthode distincte) : p99 **0,64 m**, max 2,95 ; appariement : p99 0,87 | **Confirmé**, pas d'alerte |
| Surcouche ≈ 0,5 à 1 Mo pour A, 4 à 6 o par arête (§6) | **662 610 o** pour 140 766 arêtes de base (4,71 o par arête) | **Confirmé** |
| `direction_closed` hors de la surcouche (§3) | **Infirmé avant correction** : 9 entrées de la surcouche portaient un sens fermé au vélo ; **0 après** (§10) | **Corrigé** pendant ce relevé |
| Contrôle 2 capable de dire si la chaîne est la bonne de deux voies parallèles (§9) | Non : il ne voit que les arêtes acceptées à 3 m, il borne la dérive. Test des parallèles ajouté par la sonde : **312 points sur 12,66 M** ont une autre voie plus proche de 0,3 m (§9.3) | **Comblé par la sonde**, pas par l'outil |

**En une phrase** : la surcouche retrouve 99,05 % des km-passages de la région dans le graphe OSM, n'y met presque
jamais une voie à tort (0,75 km-passages), et ce qu'elle rate tient d'abord, selon toute vraisemblance (§9.2), à
l'appariement amont sur les tuiles (chaussée d'en face d'une voie à chaussées séparées) plus qu'à la différence de
données tuiles / PBF.

## 2. Machine, code et données

| | |
|---|---|
| Machine | celle du lot 0 : AMD Ryzen 7 4800H, 8 cœurs, 16 fils, 30,9 Gio de RAM, SSD NVMe ; Linux 7.0.0-43-generic |
| Charge | charge moyenne 1,35 avant les passes (bureau, GitKraken) ; pas de compilation pendant les mesures |
| Toolchain | `rustc 1.97.0 (2d8144b78 2026-07-07)`, profil `release` |
| Code | worktree `feat/ord-graph-overlay`, HEAD `c2f2335` (lot 0) **plus les modifications non commitées du lot 1**, dont la correction du §10 ; binaire `target/release/ord-graph-overlay` construit à 17 h 11 min 54 s, après la dernière modification des sources |
| Graphe | `pays-de-la-loire-20261114-z11.ordg` (lot 0) : 1 290 856 nœuds, 1 653 822 arêtes, 3 145 589 orientées ; `ID_EDGES` xxh3 `5787d0098d407780`, `ID_NODES` `a2505c268d6fbf4a` ; PBF Geofabrik, réplication 2026-11-14T21:20:41Z |
| Région | Geofabrik `pays-de-la-loire.poly` |
| Export | `prod-network.csv.gz` du banc (privé) : 3 377 lignes, aucune ignorée ni illisible, aucun parcours exclu ; 2 366 982 arêtes d'équipe, 354 888 géométries distinctes ; xxh3 `ca53159cc90498d6` |
| Paramètres | empreinte `3b776d5feaa3fcf1` : couloir 5 m, nœud 1,5 m, arête 8 m, Hausdorff 3 m, longueur ± max(5 m, 5 %), ambiguïté 0,5 m, morceau 1 m ; graine 20261115, 2 000 re-vérifications par équipe |
| Rapport | `~/.cache/osm-route-details/graph/overlays/report-20261114-z11.json` ; copies des quatre passes dans `~/.cache/osm-route-details/graph/lot1-runs/` |

## 3. Commandes

```sh
cargo build --release -p ord-graph-overlay
G=~/.cache/osm-route-details/graph
/usr/bin/time -v target/release/ord-graph-overlay build \
  --graph $G/pays-de-la-loire-20261114-z11.ordg \
  --export ~/code/perso/tribly-generator-bench/data/prod-network.csv.gz \
  --poly $G/pays-de-la-loire.poly \
  --out-dir $G/overlays --report $G/overlays/report-20261114-z11.json   # trois fois ; puis une fois --threads 1
for f in $G/overlays/*.ordo; do
  target/release/ord-graph-overlay inspect --overlay $f --graph $G/pays-de-la-loire-20261114-z11.ordg
done
cargo test -p ord-graph -p ord-graph-overlay        # vert
cargo clippy -p ord-graph -p ord-graph-overlay --all-targets -- -D warnings   # propre
```

## 4. Durée et mémoire [mesuré]

| Passe | Mur | CPU (user + sys) | Pic RSS (`time -v`) | `VmHWM` interne |
|---|---|---|---|---|
| 16 fils, 1 | 8,85 s | 62,9 s | 720 248 kio | 721 160 kio |
| 16 fils, 2 | 8,94 s | 62,3 s | 722 692 kio | 723 976 kio |
| 16 fils, 3 (rapport retenu) | 8,84 s | 63,5 s | 721 836 kio | 721 836 kio |
| **1 fil** | **39,01 s** | 38,4 s (user) | 714 308 kio | — |

Les quatre rapports sont **identiques hors du bloc `run`** (durées et mémoire), 1 fil compris : la sortie ne dépend
pas du nombre de fils.

**Phases** (s, passe 3 à 16 fils / passe à 1 fil) : graphe ouvert et `verify()` 0,04 / 0,05 ; index spatial 0,40 / 3,07
(6 115 067 segments) ; lecture de l'export 4,07 / 6,31 ; appariement des 354 888 géométries 1,87 / 17,41 ; agrégation
0,85 / 0,88 ; écriture des neuf surcouches et re-vérification 1,31 / 11,00 ; rapport 0,06 / 0,06. **La lecture de l'export
(gunzip et JSON, 981 Mo décompressés) fait près de la moitié du mur** à 16 fils ; l'appariement coûte **≈ 49 µs par
géométrie distincte à 1 fil** **[calculé : 17,41 s / 354 888]**, dans la fourchette native estimée (30 à 100 µs, §11). La part de l'index dans le pic n'est pas isolée.

## 5. Ce que couvre la région, par équipe [mesuré]

Une trace est « dedans » si tous les sommets de toutes ses arêtes sont dans le `.poly`, « dehors » si aucun ne l'est.
Les km sont des **km-passages** (somme des `trv` : une rue roulée par 10 parcours compte 10 fois).

| Équipe | Lignes | Traces dedans / en partie / dehors | km parcourus | km dans la région | km appariés | part appariée | km comptés |
|---|---|---|---|---|---|---|---|
| A | 2 726 | 2 705 / 20 / 1 | 166 939 | 165 016 | 163 684 | 99,19 % | 163 435 |
| B | 446 | 440 / 6 / 0 | 20 174 | 19 893 | 19 771 | 99,39 % | 19 718 |
| D | 71 | 11 / 16 / 44 | 16 626 | 2 916 | 2 894 | 99,23 % | 2 882 |
| C | 108 | 21 / 3 / 84 | 12 710 | 3 236 | 3 216 | 99,40 % | 3 214 |
| E | 10 | 5 / 5 / 0 | 1 945 | 1 748 | 1 732 | 99,10 % | 1 732 |
| H | 7 | 0 / 0 / 7 | 357 | 0 | 0 | — | 0 |
| I | 7 | 0 / 0 / 7 | 357 | 0 | 0 | — | 0 |
| F | 1 | 1 / 0 / 0 | 132 | 132 | 132 | 99,97 % | 132 |
| G | 1 | 1 / 0 / 0 | 127 | 127 | 127 | 99,89 % | 127 |
| **total** | 3 377 | 3 184 / 50 / 143 | 219 367 | **193 069** | **191 557** | **99,22 %** | **191 238** |

- « km comptés » : km des morceaux entrés dans la surcouche (seuil de couverture de §3 atteint) ; 193,5 km restent sous
  le seuil (bouts d'arête effleurés aux carrefours) et 0,113 km de morceaux de moins de 1 m dans un sens fermé sont
  tenus dehors depuis la correction (§10). Comptés sur la région : **99,05 %** **[calculé]**.
- **Portée.** La région porte 193 069 des 219 367 km-passages de l'export (88 %), pour **7 équipes sur 9**. Elle est
  dominée par une équipe : A fait **85,5 %** des km de la région, avec B **95,8 %** ;
  les cinq autres équipes en font 8 159 km à elles cinq, dont deux équipes d'une seule trace. Les deux grandes équipes
  itinérantes (D, C) n'ont que 18 % et 25 % de leurs km dans la région.
  H et I sont **entièrement dehors** (leurs chiffres sont identiques : mêmes sept
  parcours) ; leur surcouche est un fichier vide de 264 o. **Les conclusions de ce relevé valent pour ces 193 069 km et
  pour ces sept équipes, dont une seule pèse vraiment** ; rien n'est établi pour une autre région.

## 6. Ce qui n'est pas apparié [mesuré]

**Par cause et par équipe** (km-passages ; ordre du rapport : les causes sont jugées dans l'ordre de §4 de la note) :

| Équipe | `direction_closed` | `no_osm_edge` | `geometry` | `ambiguous_parallel` | `chain_not_connected` | `border` | `way_kind` | total (part de la région) |
|---|---|---|---|---|---|---|---|---|
| A | 628,4 | 250,2 | 168,4 | 155,7 | 109,1 | 17,9 | 1,9 | 1 331,7 (0,81 %) |
| B | 53,1 | 28,0 | 9,4 | 9,3 | 3,2 | 4,0 | 15,1 | 122,2 (0,61 %) |
| D | 1,3 | 0,6 | 1,5 | 7,1 | 1,0 | 10,9 | 0,0 | 22,5 (0,77 %) |
| C | 5,5 | 6,4 | 3,1 | 2,4 | 0,7 | 1,3 | 0,0 | 19,4 (0,60 %) |
| E | 4,0 | 0,9 | 1,1 | 2,2 | 0,0 | 7,6 | 0,0 | 15,8 (0,90 %) |
| F | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 (0,03 %) |
| G | 0,1 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,0 | 0,1 (0,11 %) |
| **total** | **692,5** | **286,1** | **183,5** | **176,8** | **114,1** | **41,9** | **17,0** | **1 511,9 (0,78 %)** |

`invalid` : 0. En arêtes d'équipe : `direction_closed` 24 443, `no_osm_edge` 6 065, `geometry` 3 794,
`ambiguous_parallel` 1 287, `chain_not_connected` 940, `way_kind` 243, `border` 133 ; en géométries distinctes :
7 116, 434, 444, 186, 147, 20, 104.

**Par type de voie de l'équipe** (`wk` ; dénominateur : km **dans la région** de ce `wk`, mesuré par la sonde, dont la
somme retombe sur 193 069,0 km) :

| `wk` | km dans la région | non appariés | part | dont |
|---|---|---|---|---|
| `major_road` | 107 104 | 780,8 | 0,73 % | `direction_closed` 452,7 ; `no_osm_edge` 186,6 ; `ambiguous_parallel` 48,1 ; `geometry` 33,9 ; `border` 28,1 ; `way_kind` 16,8 ; `chain_not_connected` 14,7 |
| `minor_road` | 48 109 | 317,9 | 0,66 % | `direction_closed` 171,8 ; `geometry` 68,0 ; `ambiguous_parallel` 52,8 |
| `path` | 12 996 | 156,3 | 1,20 % | `chain_not_connected` 87,0 ; `no_osm_edge` 36,1 ; `geometry` 21,7 |
| `track` | 8 898 | 52,1 | 0,59 % | `no_osm_edge` 23,0 ; `geometry` 22,5 |
| `cycleway` | 8 426 | 147,8 | **1,75 %** | `ambiguous_parallel` 64,8 ; `direction_closed` 53,4 ; `geometry` 15,6 ; `no_osm_edge` 13,0 |
| `footway` | 2 268 | 31,2 | 1,37 % | `geometry` 13,7 ; `direction_closed` 8,0 |
| absent | 5 265 | 25,6 | 0,49 % | |
| `steps` | 4,4 | 0,2 | 3,9 % | |

`direction_closed` selon le sens de tuile `ow` : `against` 570,1 km, `both` 105,8, `forward` 16,6.

## 7. La surcouche [mesuré]

| Équipe | arêtes de base | km de réseau | octets | o / arête | parcours par arête p50 / p90 / p99 / max | 1 | 2 | 3–4 | 5–9 | 10–19 | 20–49 | 50–99 | 100+ |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 140 766 | 13 346,0 | 662 610 | 4,71 | 3 / 47 / 203 / 892 | 43 156 | 17 542 | 17 479 | 18 653 | 15 304 | 15 341 | 7 877 | 5 414 |
| B | 27 053 | 2 594,7 | 135 843 | 5,02 | 3 / 23 / 74 / 246 | 7 845 | 4 130 | 4 203 | 4 476 | 3 192 | 2 540 | 605 | 62 |
| D | 20 316 | 2 017,8 | 84 363 | 4,15 | 1 / 2 / 8 / 11 | 15 470 | 3 148 | 1 273 | 282 | 143 | 0 | 0 | 0 |
| C | 19 196 | 1 930,5 | 81 684 | 4,26 | 1 / 4 / 6 / 11 | 11 866 | 3 756 | 2 825 | 747 | 2 | 0 | 0 | 0 |
| E | 15 601 | 1 423,9 | 64 964 | 4,16 | 1 / 2 / 4 / 6 | 13 235 | 1 628 | 710 | 28 | 0 | 0 | 0 | 0 |
| H | 0 | 0 | 264 | — | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| I | 0 | 0 | 264 | — | — | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| F | 1 216 | 132,2 | 5 211 | 4,29 | 1 / 1 / 1 / 1 | 1 216 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| G | 1 588 | 126,8 | 6 722 | 4,23 | 1 / 1 / 1 / 1 | 1 588 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| **union des équipes** | **174 910** | **16 976,4** | **1 041 925** (9 fichiers) | 4,62 par entrée **[calculé : 225 736 entrées]** | 3 / 41 / 189 / 905 | 50 588 | 24 499 | 22 863 | 24 993 | 19 453 | 18 260 | 8 571 | 5 683 |

- La ligne du bas est l'**union** des arêtes des équipes (une arête roulée par deux équipes compte une fois, ses
  parcours s'additionnent) ; la somme des entrées des neuf fichiers est 225 736. `inspect` relit chaque fichier contre le
  graphe (identité vérifiée) et retrouve les mêmes comptes d'entrées.
- **Union, par sens** : a→b 139 409 arêtes (12 573 km), b→a 105 973 (11 624 km). **Par type de parcours** : ROAD 151 068
  arêtes (14 519 km), GRAVEL 72 128 (6 181 km), MTB 2 838 (212 km). **Par classe OSM** (km) : tertiary 4 502,
  unclassified 4 430, secondary 2 364, residential 1 645, track 1 474, path 1 071, primary 514, cycleway 398, footway
  302, service 205, living_street 45, pedestrian 22, steps 3, busway 1, trunk 0,1. **Par accès** : `yes` 174 160 arêtes
  (16 891 km), `no` 439 (50,0 km), `private` 152 (22,6 km), `destination` 159 (12,5 km).
- **29 % des arêtes de l'union ne portent qu'un parcours** (50 588) ; pour A, la moitié des arêtes
  portent 3 parcours ou moins et 5 414 en portent plus de 100.

## 8. Les deux contrôles de l'outil [mesuré]

| Équipe | paires contiguës appariées | adjacentes | écarts (n ; p50 / p90 / max, m) | laissées : hors région / une non appariée | échantillon (demandé) | Hausdorff p50 / p90 / p99 / max (m) | > 1 m | Fréchet p99 | longueur chaîne / `g` p1 / p50 / p99 | Hausdorff de l'appariement p99 / max (géométries) |
|---|---|---|---|---|---|---|---|---|---|---|
| A | 1 925 950 | 99,98 % | 289 ; 9,6 / 102,7 / 740,8 | 13 880 / 15 043 | 2 000 (2 000) | 0,25 / 0,50 / 0,62 / 1,74 | 0,5 % | 0,62 | 0,96 / 1,00 / 1,05 | 0,95 / 2,96 (179 982) |
| B | 183 724 | 99,96 % | 75 ; 0,8 / 104,9 / 300,2 | 2 203 / 727 | 2 000 (2 000) | 0,23 / 0,50 / 0,62 / 1,41 | 0,3 % | 0,62 | 0,96 / 1,00 / 1,05 | 0,79 / 2,98 (33 950) |
| D | 24 292 | 99,99 % | 2 ; 1,7 / 1,8 / 1,8 | 78 910 / 70 | 2 000 (2 000) | 0,24 / 0,50 / 0,63 / 2,11 | 0,2 % | 0,63 | 0,96 / 1,00 / 1,04 | 0,63 / 2,89 (18 089) |
| C | 26 829 | 99,97 % | 8 ; 1,8 / 2,2 / 2,2 | 54 944 / 231 | 2 000 (2 000) | 0,24 / 0,50 / 0,63 / 2,95 | 0,5 % | 0,63 | 0,96 / 1,00 / 1,04 | 0,61 / 2,96 (17 053) |
| E | 15 135 | 99,97 % | 4 ; 1,8 / 44,3 / 44,3 | 1 466 / 56 | 2 000 (2 000) | 0,24 / 0,50 / 0,67 / 1,55 | 0,4 % | 0,67 | 0,96 / 1,00 / 1,05 | 0,64 / 2,99 (13 713) |
| F | 1 059 | 100 % | 0 | 0 / 0 | 1 061 (2 000) | 0,24 / 0,50 / 0,77 / 2,41 | 0,7 % | 0,77 | 0,96 / 1,00 / 1,04 | 0,74 / 2,41 (1 061) |
| G | 1 255 | 100 % | 0 | 0 / 2 | 1 255 (2 000) | 0,25 / 0,50 / 0,62 / 1,36 | 0,4 % | 0,62 | 0,95 / 1,00 / 1,05 | 0,59 / 1,36 (1 255) |
| **total** | **2 178 244** | **99,98 %** (2 177 866) | 378 ; 2,8 / 102,7 / 740,8 | 156 843 / 16 129 | **12 316** (18 000) | **0,24 / 0,50 / 0,64 / 2,95** | **0,4 %** | 0,64 | 0,96 / 1,00 / 1,04 | 0,87 / 2,99 (**265 103 géométries appariées, comptées par équipe** ; 224 130 distinctes sur tout l'export) |

- **Contrôle 1, continuité.** Même arête des deux côtés : 0,6 % des paires. **Raccords internes non adjacents : 0** sur
  231 266 chaînes. Lecture : ce contrôle est **presque acquis par construction** — deux chaînes contiguës partent du
  même nœud de tuile, recalé sur le même nœud OSM à 1,5 m — ; il attrape un bogue de découpe, pas un mauvais choix de
  voie. Les 378 écarts (0,02 %, dont des écarts de 100 à 741 m) n'ont **pas été examinés un par un**.
- **Contrôle 2, re-vérification.** Tirage par équipe (graine 20261115 xor id d'équipe) parmi les arêtes d'équipe entrées
  dans la surcouche, Hausdorff et Fréchet discrets en haversine sur la géométrie relue par `GraphFile::geometry`.
  Pas de 1 m **jusqu'à 200 m de ligne, 200 points au-delà** (plus grossier sur les longues arêtes). Au-delà de 3 m : 0.
  Alerte (p99 > 1 m) : **non**, pour chaque équipe et au total. Lecture : il ne porte que sur des arêtes déjà acceptées
  à Hausdorff ≤ 3 m ; il borne la dérive géométrique, **il ne peut pas dire si la chaîne est la bonne de deux voies
  parallèles à moins de 3 m**. D'où le test du §9.3.
- **Champs partiels** (`fo`, `to` contre `trv`) : 0 écart sur 2 171 054 arêtes ; départ attendu à un nœud mais premier
  morceau ailleurs 4 458 sur 2 154 812 (0,2 %), arrivée 4 318 sur 2 152 491.

## 9. Ce que la sonde a examiné [sonde]

La sonde (Rust, jetable, hors dépôt) inclut les modules `geo`, `export`, `index`, `matcher` et `pipeline` de l'outil tels
quels, rejoue l'appariement (mêmes résultats) et ajoute les tests ci-dessous ; elle n'écrit que des agrégats et des
descripteurs sans coordonnée. Pour le PBF, les bouts de voies concernés ont été extraits avec `osmium extract` dans le
dossier jetable de la session, puis supprimés.

### 9.1 Exemples tirés, cause par cause

Douze géométries en échec tirées par graine (20261115) pour chaque cause, décrites par : `wk`, `ow`, longueur,
passages, profil de distance au graphe tous les 2 m, classes OSM les plus proches, morceaux de la chaîne (classe,
sens ouvert, sens voiture, `bicycle`) et résultat de la variante « nœud **et** positions » (`EDGE_STATES_WITH_NODES`).

- **`direction_closed`** : 12 sur 12 sont des chaînes **entièrement** fermées dans le sens roulé (la part fermée de la
  chaîne vaut 1 du p10 au p90, pondérée par les km), le plus souvent une arête de 10 à 17 m de `tertiary`, `secondary`
  ou `primary` en sens unique voiture, `ow = against` ; une `cycleway` de 261 m `bicycle=designated` en sens unique,
  un `path` de 63 m `oneway:bicycle`. Longueur de l'arête fermée, pondérée : p10 11 m, p50 44 m, p75 96 m, p90 187 m.
- **`no_osm_edge`** : des `path` de 450 à 480 m entièrement à moins de 5 m du graphe, donc perdus à un bout (filtre de
  cap, §9.2), des
  `minor_road` avec 18 à 20 m de suite à plus de 5 m de toute arête, des `service` entièrement couverts ; la variante ne
  rattrape **aucune** d'entre elles (0 km sur 286).
- **`chain_not_connected`** : `tertiary` de 462 m, `residential` de 109 m, `track` de 109 m rattrapés par la variante ;
  des `footway` de 6 à 30 m qui ne le sont pas (la variante les rend `geometry`).
- **`geometry`** : toutes couvertes à 5 m ; des `path` de 92 à 136 m, une `tertiary` de 109 m roulée 17 fois ; la variante
  ne change rien (3,6 km rattrapés sur 184).
- **`ambiguous_parallel`** : une `cycleway` de 273 m roulée 72 fois (19,7 km-passages à elle seule), des `residential` de
  159 m, une `cycleway` de 17 m le long d'une `primary` roulée 31 fois.
- **`way_kind`** : 10 sur 12 sont des `major_road` d'équipe dont la chaîne est **une seule `footway` de 450 m** ou une
  `cycleway` de 8 à 110 m, Hausdorff 0,2 à 2,9 m : OSM a là une voie piétonne ou cyclable dessinée sur le tracé de la
  route de la tuile ; les deux autres : un `track` d'équipe sur un `service`, une `minor_road` sur un `path` de 6 m. Par
  km : `major_road` → `cycleway` 11,4, → `footway` 5,4, `track` → `service` 0,1, `minor_road` → `path` 0,1.

### 9.2 Ventilation des 1 512 km non appariés

| Origine | km | part | Comment elle est établie |
|---|---|---|---|
| **Appariement amont sur la mauvaise chaussée** : `direction_closed` alors qu'une autre voie, parallèle (±30°), à 25 m ou moins et **ouverte dans le sens roulé**, longe au moins 80 % de `g` | **539,6** | **35,7 %** | dont 443,5 km hors arête courte et courbe (chaussées séparées, pistes unidirectionnelles de part et d'autre ; 409,5 km en `ow = against`) et 96,1 km sur une arête courte et courbe (< 80 m, ≥ 20° de virage : bretelles et anneaux de giratoire) |
| **Contresens sans voie ouverte à côté** : `direction_closed`, aucune parallèle ouverte | 152,8 | 10,1 % | dont 113,0 km `against` hors arête courte et courbe : une rue à sens unique remontée, double-sens cyclable non tagué dans OSM ou pratique de l'équipe |
| **Différence tuiles / PBF** : `no_osm_edge` avec un trou au milieu de `g` (plus de 20 m de chaque bout) | 164,2 | 10,9 % | 75 géométries ont 20 m ou plus de suite sans arête à 5 m (160,2 km) ; au PBF du 14 novembre : **aucune voie à 8 m** 72,2 km (44 géométries), **`highway=construction`** 36,1 km (4), une voie OSM à 8 m ou moins mais aucune arête du graphe à 5 m (voie déplacée, ou écartée par le filtre vélo ; `secondary`, `path`, `cycleway`, `footway`…) 51,1 km, `highway=private` (hors graphe) 0,7 km |
| **Méthode, rattrapable** | 130,6 | 8,6 % | variante « nœud et positions » : `chain_not_connected` 92,4 km, `ambiguous_parallel` 4,3, `geometry` 3,6 ; `no_osm_edge` couvert à 5 m mais un bout rejeté par le filtre de cap à ±45° : 30,4 km |
| **Méthode, à dessein non tranché** : `ambiguous_parallel` que la variante laisse ambigu | 172,5 | 11,4 % | deux voies à moins de 0,5 m de Hausdorff l'une de l'autre ; `cycleway` 64,8 km |
| **Indéterminé** | 310,2 | 20,5 % | `geometry` couvert à 5 m, non rattrapé (179,9 km : écart de 3 à 5 m ou de longueur, simplification des tuiles ou retouche OSM) ; `no_osm_edge` avec un trou à moins de 20 m d'un bout (90,6 : carrefour redessiné, ou tolérance) ; `chain_not_connected` non rattrapé (21,7 : topologie) ; `way_kind` (17,0) ; reste 1,0 |
| Limite de la région (`border`) | 41,9 | 2,8 % | arêtes à cheval sur le `.poly` |

**Lecture.** La différence de **données** tuiles / PBF n'explique sûrement que **11 %** des km perdus (164 km), et au plus
**31 %** si tout l'indéterminé lui revient. La **méthode** de ce lot en explique **20 %** (303 km : 131 rattrapables, 173
ambigus par prudence). La plus grosse part (**36 %**) vient d'**en amont** : ord-core a recalé le parcours sur des tuiles
qui ne disent pas le sens vélo, et a pu poser la trace sur la chaussée d'en face d'une voie à chaussées séparées ;
l'appariement de ce lot suit `g` fidèlement et trouve donc une voie fermée. Ce dernier point est une **inférence** :
le test établit qu'une voie ouverte parallèle existe, pas que l'équipe l'a roulée.

### 9.3 Test des parallèles sur ce qui entre dans la surcouche

Pour les 224 130 géométries distinctes entrées dans la surcouche (191 557 km-passages) : un point tous les 2 m le long
de `g` (2 m de garde aux bouts), distance à la chaîne retenue contre distance à toute autre arête à 8 m ou moins dont le
cap est à ±30° ; signalé si l'autre est plus proche de plus de 0,3 m.

| Mesure | Résultat |
|---|---|
| Points examinés | 12 660 990 |
| Signalés (autre voie plus proche de 0,3 m) | **312 (0,0025 %)** ; plus proche de 1 m : 245 |
| Géométries dont la majorité des points sont signalés | **47**, toutes de `wk = footway`, **0,75 km-passages** en tout |
| Classe de l'autre voie, sur les points signalés | `footway` 254, `cycleway` 23, `service` 19, `secondary` 6, `path` 4, autres 6 |
| Par `wk` (part des points signalés) | `footway` 0,095 %, `cycleway` 0,008 %, `minor_road` 0,0006 %, `major_road` 0,0002 %, `path`, `track`, `steps` 0 |

Les 47 géométries tirées (12 examinées) : un trottoir `footway` de 11 à 14 m de l'équipe apparié à une `cycleway`
parallèle à 2,4 à 2,8 m de Hausdorff, alors que la `footway` est plus proche. C'est la seule erreur de choix de voie que
le test trouve ; elle ne pèse rien et, pour un générateur vélo, préfère la piste.

## 10. Correction apportée pendant ce relevé

**Défaut** (signalé par la revue, confirmé ici) : `closed_m` ne comptait que les morceaux d'au moins 1 m
(`MIN_PIECE_M`), mais le comptage retenait une arête dès min(½ longueur, 50 m) de couverture. Un morceau de moins de
1 m dans un sens fermé, sur une arête de moins de 2 m, entrait donc dans la surcouche. **Mesuré avant correction** :
`inspect` relisant les surcouches du relevé précédent contre les attributs du graphe trouve **9 entrées** à compte non
nul dans un sens fermé, toutes dans celle de A (140 772 entrées) ; les huit autres : 0.

**Correction** (`tools/ord-graph-overlay/src/aggregate.rs`) : un morceau dans un sens fermé n'entre plus dans la
couverture ; ses mètres sont rapportés (`km.closed_direction_pieces_skipped` : 0,113 km au total). `inspect` compte
désormais `closed_direction_entries` en relisant le fichier contre le graphe, et le test de bout en bout l'exige nul
pour chaque surcouche. **Après** : 0 pour les neuf surcouches ; celle de A passe à 140 766 entrées
(662 637 → 662 610 o), l'union à 174 910 arêtes (174 916 avant). Comparé champ à champ à l'ancien rapport, seuls les
comptes de la surcouche bougent, de quelques unités (arêtes par classe, par type, par sens, classes de parcours ;
km comptés −0,107 km) : causes, appariement et contrôles sont inchangés.

## 11. Jugement

**La surcouche est assez sûre pour orienter un générateur**, dans la région et pour ces équipes :

- **Elle ne ment presque pas.** Ce qu'elle crédite est bien la voie roulée : test des parallèles à 0,0025 % des points,
  47 géométries mal tranchées pour 0,75 km-passages ; re-vérification p99 0,64 m ; aucune entrée dans un sens fermé
  depuis la correction. Une préférence calculée sur elle ne poussera pas vers une voie que l'équipe ne roule pas.
- **Elle oublie peu, mais pas au hasard.** 99,05 % des km-passages sont comptés. Les trous sont **courts et localisés** :
  arêtes fermées de 11 à 187 m (p10 à p90) aux carrefours, bretelles et chaussées séparées ; pistes cyclables doublant
  une route (`cycleway` 1,75 % non appariés, le double de la moyenne) ; quelques voies neuves ou en travaux. Un
  générateur qui applique la surcouche comme un **bonus** sur le coût (et non comme un filtre) traversera ces trous sans
  dommage ; un filtre « voies de l'équipe seulement » casserait aux carrefours.
- **Elle ne dit pas tout ce que l'équipe accepte** : 29 % des arêtes n'ont qu'un parcours, 439 arêtes d'accès `no`
  (50 km) y figurent. Le générateur devra pondérer par le compte, et appliquer ses propres règles d'accès.

**Où elle échoue, par ordre de poids** : la mauvaise chaussée héritée de l'amont (540 km), les deux voies trop proches
pour trancher (173 km), le contresens sans alternative (153 km), les voies absentes ou en travaux du PBF (164 km), les
écarts de 3 à 5 m (180 km). **Part de la différence tuiles / PBF : 11 % sûrs, 31 % au plus ; part de la méthode de ce
lot : 20 %, dont 9 % rattrapables** (§9.2).

## 12. Ce qui reste ouvert

- **Rattraper la mauvaise chaussée** (540 km, plus que toute autre cause) : pour un `direction_closed`, chercher la
  chaîne dans un couloir élargi (25 m) restreint aux sens ouverts. À mesurer au lot 2 avant de l'adopter : le test du
  §9.2 n'établit qu'une voie parallèle ouverte, pas que l'équipe l'a prise.
- **Bouts** : la variante « nœud et positions » rattrape 92 km de `chain_not_connected` mais fait monter
  `ambiguous_parallel` (mesure de la note : 177 → 221 km, appariement 3,5 fois plus long) ; ajouter les positions
  seulement quand le nœud retenu n'appartient à aucune arête du couloir reste à essayer. Le filtre de cap à ±45° perd
  30 km de `path` et `service` couverts.
- Les 378 écarts de continuité de plus de 2 m (jusqu'à 741 m) n'ont pas été examinés ; `--explain` n'existe pas encore.
- `Overlay::read` décode sans vérifier l'identité du graphe (seuls `decode_for` et `inspect` la vérifient) : un futur
  lecteur doit passer par `decode_for`.
- La surcouche est liée à l'identité du graphe du lot 0 : si le lot 0 bis change les ids d'arêtes, elle est à
  reconstruire (moins de 10 s).
- Tout ceci vaut pour une région et, à 85 %, pour une équipe ; un relevé dans une autre région (et pour les équipes
  itinérantes) reste à faire avant de généraliser les tolérances.
