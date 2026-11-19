# Générateur de parcours — lot 4 (4d) : sonde des attracteurs OSM

> Relevé du 19 novembre 2026 sur la branche `feat/ord-loops-lot4d` (depuis `develop` à `b288206`, 1.12.0, figures organiques
> par défaut), binaire `release` de `b288206` augmenté d'un vidage jetable (§8), Rust 1.100.0, poste de développement
> (16 fils, 30 Go). Graphe et vues France `france-20261115-z11`, extrait `france-261115.osm.pbf`. Conception :
> [`ord-loops-lot4.md`](ord-loops-lot4.md), §5 (attracteurs), §4.2 (points lointains), §3.4 ; décisions du §9.1 (point 4 :
> **le 4d s'en tient aux destinations OSM**), du §9.7 (point 6) et du §9.9. Spécification du 4c : §15 et §16 de la note ;
> relevés : [`ord-loops-lot4-mesures.md`](ord-loops-lot4-mesures.md),
> [`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/lot4/4d/` (positions vidées,
> ancrages, tableaux d'agrégats, code de la sonde dans `sonde-code/`). Ici : agrégats seulement, par catégorie OSM et par
> type ; ni départ, ni coordonnée, ni identifiant, ni nom de lieu ; une lettre d'équipe. Rien n'a été poussé ni déployé.
>
> Marques : **[mesuré]** vient des sorties de la sonde ; **[calculé]** d'un calcul fait ici sur ces sorties ; **[estimé]**
> d'un ordre de grandeur. Sans marque dans un tableau : mesuré.

## 0. En bref

- **Les objets coûtent peu.** 172 600 objets en France, en dix catégories (dont les lacs, sous-ensemble des plans d'eau) ; accrochés à un nœud de vue à moins de 300 m, ils
  font 139 000 enregistrements en route et 173 000 en gravel. Un `.orda` par vue pèse **1,1 Mo (route) et 1,4 Mo
  (gravel)** à 8 octets l'enregistrement, et la lecture des attracteurs d'une fenêtre de 100 km coûte **14 µs au P50**
  [mesuré], à côté des 95 ms d'une fenêtre construite à froid (lot 6). Le temps n'est pas une question.
- **ROAD : le réel va vers les bourgs et leurs commerces, et 1.12 n'en fait qu'une partie.** Le point lointain d'une
  boucle réelle est à moins de 300 m d'un attracteur dans **26,3 %** des cas, contre **11,1 %** au hasard dans la même
  bande et le même cap (× 2,4), et **18,9 % · 17,1 %** pour le premier parcours de 1.12 aux curseurs 0,25 · 1. L'excès du
  réel sur 1.12 vaut **+7,4 et +9,2 points** [calculé, IC 90 % +5,2 à +11,3].
- **Le bourg est déjà rendu par 1.12, pas le commerce ni le point de vue.** Logit conditionnel (le point lointain choisi
  contre 20 points tirés dans sa bande) : bourg β = +0,98 réel contre +0,90 · +0,94 pour 1.12 ; commerce (café ou
  boulangerie) **+0,60 contre +0,37 · +0,27** ; point de vue **+0,74 contre +0,47 · +0,06**. Ce sont les deux catégories
  qui valent la peine en ROAD.
- **GRAVEL : aucun attracteur ne se dégage, sauf le point de vue.** Réel 13,3 % contre 11,9 % au hasard (n.s.) ; point de
  vue × 1,9 à 300 m, et **+3,8 points sur 1.12 à 1 km** (IC +1,3 à +6,7). À l'inverse, **1.12 est trop « bourg »** en
  gravel : commerce à 300 m 9,5 % · 11,7 % contre 6,9 % réel ; à 1 km, le réel évite les boulangeries (× 0,75 du hasard).
- **Cols et sommets : le banc ne peut pas les juger.** Aucun point lointain réel ROAD à moins de 1 km d'un col, 0,3 % en
  GRAVEL, et le hasard n'en trouve pas plus (0,0 et 0,1 %) : les équipes du banc roulent en plaine. Plans d'eau et
  points d'eau potable : pas d'effet propre une fois les bourgs comptés.
- **Une équipe pèse 84 % des boucles.** Sans elle (211 ROAD, 235 GRAVEL), l'excès à 300 m tient pour bourgs, commerces,
  points de vue, et apparaît pour les plans d'eau ; il retombe à 1 km. Les poids seront généraux, réglés au banc.

## 1. Protocole

### 1.1 Objets OSM et accroche (étape 1)

**Extrait.** Le PBF du graphe : `france-261115.osm.pbf`, 5 116 354 877 octets, xxh3 `9fbe483bc4f4845c`, **égaux à
l'empreinte de l'en-tête** de `france-20261115-z11.ordg` (`pbf_size`, `pbf_xxh3`) [mesuré, `PbfSource::open`, 7,3 s].

**Catégories** (filtre `osmium tags-filter`, puis export en géométries) :

| Catégorie | Étiquettes | Objets gardés |
|---|---|---|
| `col` | `mountain_pass=yes` | nœuds, ways |
| `sommet` | `natural=peak` | nœuds ; « desservi » = accroché |
| `vue` | `tourism=viewpoint` | nœuds, ways |
| `eau` | `natural=water` ou `landuse=reservoir`, hors `water=river|canal|stream|ditch|…` ; **aire ≥ 5 ha** | surfaces (ways fermées, multipolygones) |
| `lac` | `eau` d'aire ≥ 50 ha (sous-ensemble) | idem |
| `bourg_town`, `bourg_village` | `place=town`, `place=village` | nœuds, surfaces |
| `cafe`, `boulangerie` | `amenity=cafe`, `shop=bakery` | nœuds, surfaces (point représentatif) |
| `eau_potable` | `amenity=drinking_water|water_point` | nœuds, ways |

Gardés dans `france.poly` (le polygone de l'extrait, 790 996 km² **mer et marge frontalière comprises** [mesuré]).
Groupes de lecture : `bourg` = town ou village, `commerce` = café ou boulangerie, `nature` = col, sommet, vue ou eau,
`tous` = toutes les catégories.

**Accroche.** Au **nœud de vue** le plus proche à moins de 300 m (distance sur la sphère), vue route pour ROAD, vue gravel
pour GRAVEL ; les nœuds de vue sont les jonctions des chaînes (6,4 M en route, 10,2 M en gravel). Un objet ponctuel donne
au plus un ancrage. Un plan d'eau est échantillonné sur son bord tous les 150 m et donne **un ancrage par case de 1 km**
le long du bord (un grand lac en a plusieurs). Un ancrage = un couple (nœud, catégorie), dédoublonné.

### 1.2 Points mesurés : réel, généré, hasard (étape 2)

**Réel.** Toutes les boucles retenues du banc de vraisemblance, toutes équipes : **2 071 ROAD, 780 GRAVEL** (8 équipes ;
les 4 MTB sont ignorées) [mesuré]. Sur la polyligne de la marche appariée : le **point lointain** (le sommet le plus loin du
départ, celui de `r / D`) et les **sommets à 10 % de D** (`vertex_indices(…, 0,10)`, départ et arrivée exclus, le point
lointain compris : 3 440 ROAD, 1 141 GRAVEL).

**Généré.** Les mêmes requêtes rejouées par le banc (`--holdout-share 0.25`) : variante `svc+org` (le service 1.12 : profil
organique appris par équipe, figures organiques) et `base` (le moteur 1.9.2), surcouche complète, **premier parcours
gardé**, curseurs 0,25 et 1 : 2 060 ROAD et 772 · 769 GRAVEL pour `svc-org` [mesuré]. **Le vidage est neutre** :
empreintes `base` `4686c904975302e7` et `svc-org` `6324bdfdabf62f38`, celles du §16.1 du relevé du 4c.

**Hasard.** Pour chaque point réel à vol d'oiseau `r` du départ : **20 nœuds de vue** (5 pour un sommet) tirés dans la
couronne `[0,95 r ; 1,05 r]`, à **±60° du cap** du point, **pondérés par la demi-longueur des chaînes incidentes** (un
point du réseau tiré à la longueur, ramené à sa jonction). Même tirage autour des points lointains de 1.12 : il donne le
même niveau (ROAD `tous` 10,8 % contre 11,1 %) [mesuré], la bande de 1.12 ne change donc pas le hasard. Graine fixe.

Les points réels et générés sont ramenés au nœud de vue le plus proche avant la mesure (le hasard en est déjà) : le point
lointain réel est presque toujours un nœud (P50 0,03 m), celui de 1.12 à 1 m (ROAD) et 31 m (GRAVEL, au milieu d'une
chaîne) au P50 [mesuré]. Sans ce recalage, les parts changent de 0,006 au plus (GRAVEL, curseur 1), de 0,001 au plus en ROAD.

### 1.3 Lecture

- **Part** des points à moins de 300 m et de 1 km d'un ancrage de la catégorie, par type. L'**excès** réel − hasard et
  réel − 1.12, avec un **IC à 90 %** par rééchantillonnage des boucles (300 tirages).
- **Logit conditionnel** (étape 3) : chaque boucle « choisit » son point lointain parmi lui et ses 20 tirages ; variables
  = indicatrices « ancrage de la catégorie à moins de 300 m ». β est le logarithme du rapport de chances, à catégories
  égales par ailleurs ; exp β = combien de fois un nœud près de l'objet est plus choisi qu'un autre de la bande. Le même
  modèle sur les points lointains de 1.12 (contre leurs propres tirages) donne ce que le moteur rend déjà ; l'**écart
  Δβ = β réel − β 1.12** est ce qui manque. Erreurs types par la Hessienne. Les catégories sans aucun point réel à 300 m
  (`col`, `sommet`, `lac`) sont hors du modèle.

### 1.4 Commandes, temps, mémoire

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`, `L=$G/lot6`, `O=$G/lot4/4d` ; le code jetable
est dans `$O/sonde-code/` (§8).

| Étape | Commande | Durée · pic |
|---|---|---|
| Filtre | `osmium tags-filter $L/france-261115.osm.pbf nw/mountain_pass=yes n/natural=peak nw/tourism=viewpoint wr/natural=water wr/landuse=reservoir nwr/place=town,village nwr/amenity=cafe nwr/shop=bakery nw/amenity=drinking_water,water_point -o $O/attr.osm.pbf` | 43 s · 2,4 Go (124 Mo écrits) |
| Géométries | `osmium export $O/attr.osm.pbf -f geojsonseq -a type,id -o $O/attr.geojsonseq` | 22 s · 0,5 Go |
| Objets | `python3 -I parse_attr.py $O/attr.geojsonseq $L/france.poly $O` | 204 s · 0,2 Go |
| Nœuds de vue | `sonde4d nodes $L/france-20261115-z11.{road,gravel}.ordv $O/nodes_{road,gravel}.bin` | 1,5 s · 0,7 Go |
| Accroche, `.orda` | `python3 -I snap.py $O` | 7,7 s · 1,5 Go |
| Banc et vidage | `ORD_4D_DUMP=$O/points.jsonl $B likeness --holdout-share 0.25 --variants base,svc+org --label lot4d-sonde --out $O` | 12 min 39 · 19,1 Go |
| Lecture | `python3 -I analyse.py $O` | 2 min 03 · 1,7 Go |
| Fenêtres | `sonde4d windows $O/{road,gravel}.orda $L/…{road,gravel}.ordv 2000 {50,100,150}` | < 1 s |

## 2. Les objets : comptes, densités, accroche, taille (étape 1)

Densité rapportée aux 551 695 km² de la France métropolitaine [calculé] ; les objets de la marge frontalière du polygone y
sont comptés, la densité est donc un peu haute.

| Catégorie | Objets | / 1 000 km² | Accrochés route | nœuds route | / 1 000 km² | accroche P50 · P90 (m) | Accrochés gravel | nœuds gravel |
|---|---:|---:|---:|---:|---:|---|---:|---:|
| `col` | 4 124 | 7,5 | 30 % | 1 225 | 2,2 | 9 · 177 | 89 % | 3 645 |
| `sommet` | 26 029 | 47,2 | **16 %** | 4 133 | 7,5 | 195 · 282 | 64 % | 16 322 |
| `vue` | 18 943 | 34,3 | 57 % | 9 729 | 17,6 | 89 · 244 | 96 % | 17 263 |
| `eau` (≥ 5 ha) | 9 000 | 16,3 | 69 % des plans | 12 496 | 22,7 | 2 ancrages par plan (P50) | 94 % | 20 962 |
| `lac` (≥ 50 ha) | 603 | 1,1 | 80 % | 3 065 | 5,6 | 5 par lac | 98 % | 4 940 |
| `bourg_town` | 1 121 | 2,0 | 100 % | 1 121 | 2,0 | 29 · 59 | 100 % | 1 121 |
| `bourg_village` | 37 628 | 68,2 | 99,7 % | 37 480 | 67,9 | 32 · 82 | 99,9 % | 37 556 |
| `cafe` | 17 474 | 31,7 | 99,6 % | 15 377 | 27,9 | 24 · 68 | 100 % | 15 502 |
| `boulangerie` | 29 572 | 53,6 | 99,9 % | 28 723 | 52,1 | 23 · 54 | 100 % | 28 752 |
| `eau_potable` | 28 700 | 52,0 | 95 % | 25 377 | 46,0 | 29 · 100 | 99 % | 26 990 |
| **Total** (sans `lac`) | 172 591 [calculé] | | | **138 726** enregistrements, 128 274 nœuds | | | | **173 053**, 160 522 nœuds |

- **`.orda` prototype** (répertoire de tuiles z11 trié, puis enregistrements de 8 octets : nœud de vue `u32`, catégorie
  `u8`, classe de poids `u8`, 2 octets libres) : **1 135 888 octets** en route (3 257 tuiles), **1 410 744** en gravel
  (3 287 tuiles) [mesuré]. Avec les seules catégories retenues au §9 (commerce, vue, bourg) : 0,74 Mo et 0,80 Mo [calculé].
  Pour mémoire, la vue route pèse 866 Mo : le fichier d'attracteurs en vaut 0,13 %.
- Les cols et sommets sont souvent hors de la vue route (sentiers, pistes) : 30 % et 16 % y trouvent une jonction à
  300 m, 89 % et 64 % dans la vue gravel. Les objets de bourg (village, café, boulangerie) s'accrochent presque tous, à
  moins de 100 m.

## 3. Points lointains contre attracteurs (étape 2)

### 3.1 ROAD (2 071 boucles réelles ; 1.12 : 2 060 premiers parcours par curseur)

Part des points lointains à moins de **300 m** d'un ancrage [mesuré] ; excès [calculé], IC 90 % [mesuré par
rééchantillonnage] :

| Catégorie | réel | hasard | 1.12 c 0,25 | 1.12 c 1 | 1.9.2 c 0,25 | réel / hasard | réel − hasard | réel − 1.12 c 0,25 | réel − 1.12 c 1 |
|---|---:|---:|---:|---:|---:|---:|---|---|---|
| `vue` | 0,046 | 0,014 | 0,033 | 0,018 | 0,021 | × 3,2 | +0,031 [+0,024 ; +0,041] | +0,013 [+0,004 ; +0,022] | +0,028 [+0,019 ; +0,037] |
| `eau` | 0,008 | 0,007 | 0,004 | 0,004 | 0,004 | × 1,2 | +0,002 [−0,002 ; +0,005] | +0,004 [+0,001 ; +0,008] | +0,004 [+0,000 ; +0,008] |
| `bourg_town` | 0,023 | 0,005 | 0,014 | 0,011 | 0,014 | × 4,2 | +0,017 [+0,012 ; +0,022] | +0,009 [+0,002 ; +0,016] | +0,012 [+0,006 ; +0,019] |
| `bourg_village` | 0,156 | 0,048 | 0,130 | 0,117 | 0,100 | × 3,2 | +0,108 [+0,094 ; +0,119] | +0,026 [+0,009 ; +0,046] | +0,039 [+0,024 ; +0,056] |
| `cafe` | 0,074 | 0,023 | 0,042 | 0,041 | 0,049 | × 3,2 | +0,051 [+0,042 ; +0,061] | +0,032 [+0,021 ; +0,042] | +0,033 [+0,020 ; +0,045] |
| `boulangerie` | 0,187 | 0,068 | 0,140 | 0,121 | 0,123 | × 2,8 | +0,119 [+0,106 ; +0,133] | +0,047 [+0,029 ; +0,065] | +0,066 [+0,048 ; +0,084] |
| `eau_potable` | 0,042 | 0,022 | 0,022 | 0,014 | 0,010 | × 1,9 | +0,020 [+0,013 ; +0,027] | +0,020 [+0,012 ; +0,028] | +0,028 [+0,019 ; +0,035] |
| **`bourg`** | 0,179 | 0,054 | 0,144 | 0,128 | 0,114 | × 3,3 | +0,125 [+0,112 ; +0,139] | +0,035 [+0,017 ; +0,053] | +0,051 [+0,033 ; +0,068] |
| **`commerce`** | 0,199 | 0,075 | 0,146 | 0,128 | 0,132 | × 2,7 | +0,124 [+0,111 ; +0,139] | **+0,053** [+0,036 ; +0,073] | **+0,072** [+0,055 ; +0,089] |
| `nature` | 0,054 | 0,021 | 0,036 | 0,021 | 0,025 | × 2,6 | +0,033 [+0,024 ; +0,040] | +0,017 [+0,006 ; +0,026] | +0,032 [+0,023 ; +0,042] |
| **`tous`** | **0,263** | **0,111** | **0,189** | **0,171** | 0,174 | × 2,4 | +0,152 [+0,137 ; +0,167] | **+0,074** [+0,052 ; +0,093] | **+0,092** [+0,073 ; +0,113] |

`col`, `sommet`, `lac` : 0,000 partout à 300 m (le hasard, 0,000 · 0,000 · 0,002).

À **1 km** les excès fondent : `tous` 0,468 réel, 0,404 hasard (× 1,16), 0,445 · 0,421 pour 1.12 ; réel − 1.12 = +0,023
[−0,003 ; +0,048] · +0,047 [+0,023 ; +0,070]. Restent nets : `bourg_village` (+0,093 sur le hasard) et `eau_potable` (+0,039 ·
+0,041 sur 1.12) [mesuré]. **L'attraction est locale** : le point lointain est *dans* le bourg, pas à côté. 300 m est la
bonne distance d'accroche.

**1.12 a déjà fait une partie du chemin** depuis 1.9.2 : `tous` 0,174 → 0,189 au curseur 0,25, sans aucun attracteur (degré
≥ 3 des candidats, goût et surcouche de l'équipe, qui passent par les bourgs).

### 3.2 GRAVEL (780 boucles réelles ; 1.12 : 772 · 769)

| Catégorie | réel | hasard | 1.12 c 0,25 | 1.12 c 1 | 1.9.2 c 0,25 | réel − hasard | réel − 1.12 c 0,25 | réel − 1.12 c 1 |
|---|---:|---:|---:|---:|---:|---|---|---|
| `vue` (300 m) | 0,041 | 0,022 | 0,027 | 0,033 | 0,013 | +0,019 [+0,008 ; +0,032] | +0,014 [−0,002 ; +0,029] | +0,009 [−0,008 ; +0,024] |
| `vue` (1 km) | 0,164 | 0,127 | 0,126 | 0,130 | 0,055 | +0,037 [+0,016 ; +0,058] | **+0,038** [+0,013 ; +0,067] | **+0,034** [+0,005 ; +0,058] |
| `eau` (300 m) | 0,019 | 0,012 | 0,021 | 0,012 | 0,017 | +0,007 [+0,000 ; +0,015] | −0,001 | +0,008 |
| `bourg` (300 m) | 0,047 | 0,034 | 0,065 | 0,074 | 0,067 | +0,013 [+0,001 ; +0,024] | −0,017 [−0,036 ; +0,001] | **−0,027** [−0,047 ; −0,008] |
| `commerce` (300 m) | 0,069 | 0,071 | 0,095 | 0,117 | 0,109 | −0,002 [−0,019 ; +0,014] | **−0,025** [−0,048 ; −0,003] | **−0,048** [−0,072 ; −0,023] |
| `commerce` (1 km) | 0,253 | 0,345 | 0,306 | 0,346 | 0,301 | **−0,092** [−0,119 ; −0,066] | −0,053 [−0,091 ; −0,016] | −0,093 [−0,133 ; −0,055] |
| `nature` (300 m) | 0,055 | 0,033 | 0,045 | 0,044 | 0,030 | +0,022 [+0,009 ; +0,036] | +0,010 [−0,007 ; +0,028] | +0,011 [−0,007 ; +0,029] |
| `tous` (300 m) | 0,133 | 0,119 | 0,154 | 0,173 | 0,140 | +0,014 [−0,008 ; +0,031] | −0,021 [−0,050 ; +0,006] | −0,040 [−0,071 ; −0,009] |

Le gravel réel **ne va pas aux bourgs** (× 1,4 du hasard à 300 m, × 0,97 à 1 km) et **s'écarte des commerces** à 1 km
(× 0,73) ; 1.12 y va plus que le réel. Seul le point de vue attire, et 1.12 en rend une partie (0,027 · 0,033 contre 0,013 en
1.9.2). Cols : 0,003 réel, 0,000 pour 1.12 et le hasard, soit 2 boucles [calculé] : rien à lire.

### 3.3 Sommets à 10 % de D

Même lecture sur tous les sommets (le point lointain compris), à 300 m [mesuré] :

| | réel | hasard | 1.12 c 0,25 | 1.12 c 1 | 1.9.2 c 0,25 |
|---|---:|---:|---:|---:|---:|
| ROAD `tous` (3 440 sommets réels) | 0,284 | 0,126 | 0,222 | 0,193 | 0,173 |
| ROAD `commerce` | 0,210 | 0,084 | 0,175 | 0,138 | 0,132 |
| ROAD `bourg` | 0,189 | 0,052 | 0,152 | 0,128 | 0,112 |
| ROAD `vue` | 0,045 | 0,019 | 0,040 | 0,033 | 0,024 |
| GRAVEL `tous` (1 141) | 0,149 | 0,125 | 0,156 | 0,170 | 0,152 |
| GRAVEL `vue` | 0,056 | 0,022 | 0,028 | 0,041 | 0,016 |
| GRAVEL `commerce` | 0,070 | 0,078 | 0,083 | 0,104 | 0,110 |

Le dessin est celui du point lointain : excès ROAD de +0,06 à +0,09 sur 1.12 (`tous`) ; en gravel, le point de vue seul.
Les sommets intermédiaires (P1, P2 d'un triangle) sont donc des candidats aussi légitimes que le point lointain.

### 3.4 Sans l'équipe A

L'équipe A porte 84 % des boucles [mesuré]. Sur les autres (211 ROAD, 235 GRAVEL), à 300 m, réel contre hasard [mesuré] :

| | `bourg` | `commerce` | `vue` | `eau` | `eau_potable` | `tous` |
|---|---|---|---|---|---|---|
| ROAD | 0,128 · 0,054 | 0,175 · 0,100 | 0,038 · 0,012 | **0,043 · 0,013** | 0,085 · 0,026 | 0,261 · 0,158 |
| GRAVEL | 0,060 · 0,036 | 0,098 · 0,096 | 0,043 · 0,018 | 0,026 · 0,018 | 0,047 · 0,028 | 0,200 · 0,159 |

Bourgs, commerces et points de vue tiennent hors de A ; **les plans d'eau apparaissent** (× 3,3 en ROAD), ce que A
masque. À 1 km, les excès retombent sous +0,03, sauf en ROAD `nature` (0,185 · 0,131) et `eau` (0,090 · 0,058), et en GRAVEL
`bourg_town` (0,111 · 0,072). La boulangerie,
seule, n'attire pas hors de A (β ≈ 0, §4) : c'est le commerce en général, et le café d'abord.

## 4. Poids (étape 3)

Logit conditionnel, points lointains, 300 m ; β ± erreur type [mesuré] ; Δβ [calculé] :

| ROAD | réel | 1.12 c 0,25 | 1.12 c 1 | Δβ c 0,25 · c 1 | réel sans A |
|---|---|---|---|---|---|
| `bourg_village` | +0,95 ± 0,09 | +0,87 ± 0,09 | +0,92 ± 0,09 | +0,08 · +0,03 | +0,60 ± 0,28 |
| `bourg_town` | +0,90 ± 0,20 | +0,85 ± 0,23 | +0,80 ± 0,26 | +0,05 · +0,10 | +0,94 ± 0,51 |
| `cafe` | +0,36 ± 0,11 | −0,14 ± 0,14 | +0,09 ± 0,14 | **+0,50 · +0,27** | +0,55 ± 0,33 |
| `boulangerie` | +0,56 ± 0,09 | +0,48 ± 0,09 | +0,31 ± 0,10 | +0,08 · +0,25 | −0,04 ± 0,27 |
| `vue` | +0,73 ± 0,13 | +0,51 ± 0,14 | +0,08 ± 0,18 | **+0,22 · +0,65** | +1,10 ± 0,39 |
| `eau` | +0,19 ± 0,26 | −0,75 ± 0,36 | −0,54 ± 0,34 | +0,94 · +0,73 | +1,60 ± 0,37 |
| `eau_potable` | −0,42 ± 0,13 | −0,70 ± 0,17 | −1,16 ± 0,20 | +0,28 · +0,74 | +0,77 ± 0,33 |
| groupes : `bourg` · `commerce` · `vue` | +0,98 · **+0,60** · +0,74 | +0,90 · +0,37 · +0,47 | +0,94 · +0,27 · +0,06 | +0,08 · +0,23 · +0,27 ; +0,04 · +0,33 · +0,68 | |
| un seul indicateur `tous` | **+1,10 ± 0,05** (× 3,0) | +0,68 ± 0,06 (× 2,0) | +0,55 ± 0,06 (× 1,7) | **+0,42 · +0,55** | |

| GRAVEL | réel | 1.12 c 0,25 | 1.12 c 1 | Δβ c 0,25 · c 1 |
|---|---|---|---|---|
| `vue` | +0,66 ± 0,20 | +0,38 ± 0,24 | +0,70 ± 0,22 | +0,28 · −0,04 |
| `bourg` | +0,43 ± 0,21 | +0,43 ± 0,18 | +0,40 ± 0,18 | 0 · +0,03 |
| `commerce` | −0,22 ± 0,17 | +0,21 ± 0,15 | +0,37 ± 0,14 | **−0,43 · −0,59** |
| `eau` | +0,42 ± 0,28 | +0,55 ± 0,27 | −0,11 ± 0,35 | −0,13 · +0,53 |
| `tous` | +0,13 ± 0,11 | +0,33 ± 0,11 | +0,44 ± 0,10 | −0,20 · −0,31 |

**Ce qui se dégage** [calculé sur ces tableaux] :

1. **ROAD, commerce** : Δβ ≈ +0,3 (le café surtout : +0,5 · +0,3). Le seul effet à la fois fort, stable hors de A et
   absent de 1.12.
2. **ROAD, point de vue** : Δβ +0,2 · +0,7, plus grand en découverte ; stable hors de A (+1,1).
3. **ROAD, bourg** : Δβ ≈ 0. 1.12 rend déjà l'attrait du bourg ; un poids de plus n'est pas justifié par le banc.
4. **Plans d'eau, eau potable** : les Δβ positifs viennent de ce que 1.12 *évite* ces objets (β négatifs, centres de villes
   et rives sans jonction), pas d'une attraction mesurée dans le réel (β réel +0,19 ± 0,26 et −0,42). Seule l'équipe
   hors A montre un vrai attrait des plans d'eau (+1,6 ± 0,4, sur 211 boucles). À ne pas retenir au 4d.
5. **GRAVEL** : aucun attracteur ; le commerce est **répulsif** (Δβ −0,4 · −0,6). Le point de vue n'a pas de Δβ stable au
   modèle à 300 m, mais son excès à 1 km sur 1.12 est net (§3.2).

**Ordre de grandeur de l'effet visé** [calculé] : en ROAD, rapprocher le premier parcours du réel sur `tous` à 300 m, c'est
passer de 0,189 · 0,171 à 0,263, soit +7 à +9 points de points lointains sur un attracteur ; sur les seuls commerce et vue,
de 0,146 · 0,128 à 0,199 et de 0,033 · 0,018 à 0,046.

**Traduire β en poids moteur.** Le moteur ne tire pas ses points lointains par un logit : il les **classe** (§15.4 :
`(⌊|r_pair − ρ*| / 0,02⌋, agrément moyen, ids)` ; lentille : écart à D puis secteur). β ne se convertit donc pas
directement ; il fixe la **cible** (la part réelle) et l'**ordre** des catégories (commerce, vue). Le poids par catégorie
est réglé au banc sur la cible (§6, point 5).

## 5. Coût de lecture d'une fenêtre

`.orda` prototype lu par `mmap`, 2 000 fenêtres tirées sur des nœuds de vue, disque D/2 + 2 km à z11 : tuiles du
répertoire trouvées par dichotomie, enregistrements lus un à un. À chaud (pages en cache) [mesuré] :

| D | tuiles P50 | attracteurs lus P50 · P90 · max | temps P50 · P90 · max (µs) route | gravel |
|---|---:|---|---|---|
| 50 km | 22 | 923 · 2 115 · 10 962 | 6,3 · 13,3 · 60 | 6,6 · 9,5 · 79 |
| 100 km | 64 | 2 450 · 5 213 · 13 034 | **13,7** · 35,8 · 77 | 14,2 · 38,3 · 92 |
| 150 km | 128 | 4 442 · 10 073 · 15 714 | 27,7 · 52,8 · 147 | 26,0 · 61,0 · 130 |

Ouverture du fichier : 20 à 40 µs. À côté : 95 ms pour une fenêtre construite à froid au banc France (lot 6), 0,30 s pour
une génération 1.12 au P50 en conteneur. Le passage nœud de vue → nœud de la fenêtre, à faire pour chaque attracteur, n'est
pas mesuré : 2 450 recherches dans l'index de la fenêtre, **moins de 0,1 ms** [estimé]. À froid, un fichier de 1,4 Mo est
lu en entier une fois : rien de comparable aux 866 Mo de la vue. **Aucun cache n'est nécessaire** ; le budget de mémoire du
service (3 Gio) ne bouge pas (2,5 Mo de pages partagées) [calculé].

Dans la couronne des points lointains (`[0,27 ; 0,39] × D`, 4 secteurs de 30° sur 12), une requête de 100 km voit environ
**190 ancrages** [estimé : 0,23 ancrage par km² en route × 2 490 km² × 1/3] : de quoi offrir un attracteur à presque chaque
secteur et sous-bande.

## 6. Points techniques tranchés

1. **Ancrage = nœud de vue, un fichier par vue** : `<graphe>.road.orda` et `<graphe>.gravel.orda`, à côté des `.ordv`. Les
   deux vues n'ont pas les mêmes nœuds ni le même taux d'accroche (cols : 30 % contre 89 %) ; un fichier par vue se lit
   comme sa vue, sans table de correspondance.
2. **Format** : en-tête (magie, version, identité du graphe de base : nœuds, arêtes, `pbf_size`, `pbf_xxh3`, époque de
   construction, comme le `VIEW_META` des vues ; plus nœuds et arêtes de la vue), table des catégories, répertoire de
   tuiles z11 trié `(clé, premier)` avec sentinelle, puis enregistrements de 8 octets `(nœud de vue u32, catégorie u8,
   classe u8, 2 octets libres)`, triés par tuile puis nœud. Un couple (nœud, catégorie) par enregistrement. Le moteur
   refuse un fichier dont l'identité ne correspond pas (comme une vue) et sert sans attracteurs.
3. **Sous-commande** `ord-graph-build attractors` : relit le même PBF (empreinte comparée à l'en-tête, refus sinon), passe 1
   les ways (cafés et boulangeries en bâtiment, cols et points de vue en way), passe 2 les nœuds ; point représentatif d'une
   way fermée = barycentre de ses nœuds. **Pas de multipolygone** : les plans d'eau, qui seuls en demandent, sont hors du
   4d (§4, point 4). Accroche à 300 m par une grille des nœuds de vue. Coût : une lecture de plus du PBF, de l'ordre de la
   minute [estimé : 65 s pour `osmium` filtre et export].
4. **Lecture** : au plan de la fenêtre (mêmes tuiles), une fois par fenêtre, rangée avec elle ; mêmes tuiles ⇒ même
   ensemble d'attracteurs, donc **déterministe** et indépendant de la taille de la fenêtre au-delà du disque (invariant du
   lot 6). Fichier absent ou refusé : `generator.attractors = false` dans `/v1/capabilities`, et **1.12 au bit près**
   (l'empreinte `svc-org` `6324bdfdabf62f38` doit rester celle du banc sans fichier).
5. **Usage moteur, organique seulement** (`Classic` intact) : (a) **candidats** : par secteur et par sous-bande, en plus du
   plus agréable, le meilleur nœud accroché à une catégorie retenue (poids, puis agrément, puis id) ; (b) **rang des paires** :
   la clé du §15.4 devient `(⌊|r_pair − ρ*| / 0,02⌋, −poids(P1) − poids(P2), agrément moyen, ids)` ; (c) **lentille** : parmi
   les demi-tours à moins de 5 % de D de la cible, le meilleur attracteur passe devant (§4.2). `max_pairs` (4) et `tri_keep`
   (2) ne changent pas : **le temps ne bouge que de la lecture** (§5). Poids entiers par (type, catégorie), constants dans le
   code (comme la table commune), réglés au banc par une grille courte (0, 1, 2 par catégorie retenue) sur la cible du §9,
   question 4.
6. **Banc** : la part « point lointain à moins de 300 m d'un attracteur retenu » par catégorie entre dans `compare_org`, à
   côté des critères A à D ; le hasard n'y est pas rejoué (il est dans ce relevé).

## 7. Ce que la sonde ne dit pas

- **Une association, pas un effet** : rien n'a été joué avec des attracteurs. La sonde dit où sont les points lointains, pas
  ce que deviendraient r / D, la compacité ou le temps une fois les candidats changés. C'est le banc du 4d.
- **Les montagnes manquent** : aucune équipe du banc ne roule près des cols ; cols et sommets ne sont ni confirmés ni
  écartés.
- **Une équipe pèse 84 %** : les poids généraux seront d'abord les siens ; le tableau sans A (§3.4) montre que le sens
  tient pour commerce, vue et bourg, pas pour la boulangerie seule ni pour l'eau.
- **OSM d'aujourd'hui, traces d'hier** : un café fermé ou ouvert depuis la trace brouille un peu l'excès, dans les deux sens.
- **Le hasard est un choix** : un point du réseau tiré à la longueur, même couronne, ±60° du cap. Un tirage par nœud (non
  pondéré) donnerait un hasard plus urbain, donc des excès plus faibles.

## 8. Ce qui est commité

Ce relevé seul. Le code de la sonde est **jetable et hors dépôt** (`~/.cache/osm-route-details/graph/lot4/4d/sonde-code/`) :
un vidage des positions dans le banc (`dump4d.rs` et `bench-dump4d.patch`, actif si `ORD_4D_DUMP` nomme un fichier), un
exemple Rust (`sonde4d.rs` : empreinte du PBF, nœuds de vue, lecture d'un `.orda` par fenêtre) et trois scripts Python
(`parse_attr.py`, `snap.py`, `analyse.py`). Il n'est pas commité parce qu'il **écrit des positions privées** (même règle que
l'instrumentation du §16.12 de la note) et qu'il ne resservira pas tel quel : la sous-commande `attractors` (§6, point 3)
lira le PBF en Rust, et le banc mesurera la part du §6, point 6, sans positions.

## 9. Questions au propriétaire

1. **Quelles catégories en ROAD ?** Commerce : réel 0,199, hasard 0,075, 1.12 0,146 · 0,128 ; Δβ +0,23 · +0,33, stable hors de
   A. Point de vue : 0,046, 0,014, 0,033 · 0,018 ; Δβ +0,27 · +0,68. Bourg : réel 0,179, 1.12 0,144 · 0,128, mais Δβ +0,08 ·
   +0,04 : 1.12 le rend déjà, l'écart de part vient du commerce qu'il porte.
   **Recommandé** : **commerce (café et boulangerie ensemble) et point de vue** avec un poids ; bourgs écrits dans le fichier
   avec un poids nul en ROAD ; plans d'eau et eau potable hors du 4d (pas d'effet propre, §4, point 4).
2. **Et en GRAVEL ?** Aucune attraction nette à 300 m ; point de vue +0,038 sur 1.12 à 1 km (IC +0,013 à +0,067) ; commerce
   **répulsif** (réel 0,069 contre 0,095 · 0,117 pour 1.12 à 300 m ; Δβ −0,43 · −0,59).
   **Recommandé** : **point de vue seul** en GRAVEL, poids faible (classe 1). Le commerce répulsif (un poids négatif) n'entre
   pas au 4d : c'est un goût de lieu, pas une destination, et il demande sa propre mesure (le goût par classe de voie du 3b
   en explique peut-être une partie, les bourgs se traversant par de grandes routes).
3. **Cols et sommets, non mesurables ici ?** 0 point lointain réel à moins de 1 km d'un col en ROAD, 2 boucles en GRAVEL, et
   le hasard pas plus : le banc est en plaine. Les écrire coûte 43 Ko (route) [calculé].
   **Recommandé** : **écrits dans le fichier, poids nul** jusqu'à une mesure sur une équipe de montagne ; ne pas leur donner
   à la main le poids du point de vue (§5 de la note : « réglés sur la reconstruction, pas à la main »). Option : poids du
   point de vue a priori, si le propriétaire tient à l'effet « col » dès le 4d.
4. **Le critère du 4d au banc ?** **Recommandé** : sur les premiers parcours ROAD, la part des points lointains à moins de
   300 m d'un attracteur retenu ferme **au moins la moitié de l'écart au réel** aux deux curseurs (commerce : de 0,146 · 0,128
   à ≥ 0,173 · 0,164 ; point de vue : de 0,033 · 0,018 à ≥ 0,040 · 0,032) [calculé], **sans manquer** A1 à A4, B1, B2, C1
   (≤ × 1,40 de `svc`), C2 ni D aux seuils du §9.9 ; GRAVEL : point de vue en hausse, `commerce` pas plus haut qu'en 1.12. Pas
   de critère de reconstruction (le §9.1, point 3, juge le goût sur les classes, pas sur la reconstruction).
5. **Poids généraux ou par équipe ?** L'équipe A fait 84 % des boucles ; hors de A, la boulangerie n'attire plus (β −0,04
   ± 0,27) et l'eau oui (+1,6 ± 0,4), sur 211 boucles.
   **Recommandé** : **poids généraux** au 4d, constants dans le moteur ; un poids appris par équipe (comme le goût du 3b)
   viendra avec les destinations et les lieux d'équipe, après le lot 4 (§9.1, point 4).
