# Générateur de parcours — lot 4 : relevé de la sonde du goût

> Relevé du 17 novembre 2026, binaire `release` du commit `bb8986f` de la branche `feat/ord-loops-lot4-sonde` (banc
> `ord-loops-bench likeness --variants`, crochet de goût du §12.1 de la note, relu ; service, contrat et contrôleur
> **inchangés**), Rust 1.100.0, poste de développement (AMD Ryzen 7 4800H, 16 fils, 30 Go). Graphe et vues France
> `france-20261115-z11`, export privé et surcouches du 4a. Conception : [`ord-loops-lot4.md`](ord-loops-lot4.md), §12 ;
> relevé précédent : [`ord-loops-lot4-mesures.md`](ord-loops-lot4-mesures.md). Sorties complètes, **privées et hors
> dépôt**, sous `~/.cache/osm-route-details/graph/lot4/` : `sonde-*.{json,md}` et `sonde2-*.{json,md}`. Équipes
> désignées par les lettres A à I ; ni départ, ni coordonnée, ni identifiant d'équipe, de trace ou d'arête ici.
>
> Marques : **[mesuré]** vient des rapports du banc (`<label>-compare.md`, résumés par variante) ou d'un agrégat calculé
> sur leur JSON (scripts jetables hors dépôt, mêmes boucles, mêmes jeux, même normalisation : ils redonnent les `S` du
> banc au millième) ; **[calculé]** d'un calcul fait ici sur ces mesures. Sans marque dans un tableau : mesuré.

## 1. Question et protocole

**Question** (décision 7 du §9.1, §12 de la note) : un multiplicateur par classe de voie (72 classes du §3.2), appliqué à
**toutes** les arêtes de la vue, arêtes d'équipe comprises, aux deux curseurs, divise-t-il par deux au moins l'écart des
parts de classes entre parcours générés et réels ? La familiarité au-dessus de 0,5, qui ne demande aucun code moteur, en
fait-elle autant ?

- **Réel, requêtes, graine, retenue** : ceux du 4a, à l'identique (2 855 boucles retenues, 2 071 ROAD, 780 GRAVEL ;
  curseurs 0,25 et 1 ; 286 boucles mises à l'écart). Premier parcours gardé, jeux `OK`.
- **Variantes** (§12.3 de la note) :

| nom | ce qui change | source de la table |
|---|---|---|
| `base` | rien : le moteur 1.9.2 | — |
| `fam075`, `fam100` | familiarité 0,75 ou 1 (au curseur 1 : équipe d'abord à λ = 1) | — |
| `ge05`, `ge10` | goût **d'équipe**, γ = 0,5 ou 1 | parts roulées de l'équipe contre parts disponibles à ~1 km de son réseau (§12.2) |
| `gg05`, `gg10` | goût **général**, γ = 0,5 ou 1 | les mêmes sommes, toutes équipes réunies, par type |
| `rc05`, `rc10`, `rc15` | goût **recalé**, γ = 0,5, 1 ou 1,5 | longueurs réelles par classe contre longueurs du premier parcours de `base`, mêmes boucles (hors boucles mises à l'écart) : le réglage de `K` par type qui vise l'écart mesuré |
| `fam100-rc05`, `fam100-ge05` | combinaisons | — |

- **Trois lectures** (§12.6) : « toutes » (surcouche complète, toutes les boucles) ; **« écart »**, la lecture honnête
  (boucles mises à l'écart, surcouche « écart », tables apprises sans elles) ; « écart, dans l'échantillon » (mêmes
  boucles, surcouche complète).
- **Critère** : `S` = moyenne des W1 / IQR des parts de classes (ROAD : `way_major`, `way_tertiary`, `way_minor`,
  `way_street`, `way_cycle`, `surf_paved`, `surf_presumed`, `marked` ; GRAVEL : les mêmes plus `way_path` et
  `surf_unpaved`), IQR du réel de tout le type, la même pour toutes les lectures et équipes ; **critère du 4b :
  `S / S_base ≤ 0,5`**.
- **Commandes** (depuis le dépôt, `B=./target/release/ord-loops-bench`) :
  1. essai court et déterminisme (`--teams C --limit 5`, `--teams B --threads 1` / `--threads 16`) : faits à la relecture
     sur le même commit (§12.11, point 12 de la note), non refaits ;
  2. campagne : `$B likeness --label sonde --variants base,fam:0.75,fam:1,ge:0.5,ge:1,gg:0.5,gg:1,rc:1` ;
  3. seconde campagne, **justifiée par la lecture de la première** (§12.3 : `rc10` dépasse la cible en ROAD, `fam100` est
     la meilleure variante en ROAD) : `$B likeness --label sonde2 --variants base,rc:0.5,rc:1.5,fam:1+rc:0.5,fam:1+ge:0.5`.
- **Durée** : 32 min 29 s (8 variantes) puis 20 min 36 s (5 variantes), l'une après l'autre ; 204 à 296 s par variante.
  **Mémoire** : pic anonyme 15,3 Go (`base`) à 16,7 Go (`rc10`), RSS maximal 19,6 Go avec les pages du graphe, sous les
  30 Go ; une seule campagne à la fois.

## 2. Neutralité et empreintes

`base` redonne **`55c3b065dc085f42`, l'empreinte du 4a**, dans les deux campagnes (contrôle « `base` = 4a » : oui) : le
crochet est neutre à l'échelle de la France, jeux « écart » compris [mesuré]. Les `S` de `base` sont ceux du 4a (0,614 ·
0,721 · 0,735 · 0,680, §12.6 de la note) au millième.

| variante | empreinte de résultat | `taste_xxh3` | durée (s) |
|---|---|---|---:|
| base | `55c3b065dc085f42` | `2d06800538d394c2` | 209 · 204 |
| fam075 | `0fbd0017274beb8e` | `2d06800538d394c2` | 208 |
| fam100 | `bef985fca452cc97` | `2d06800538d394c2` | 220 |
| ge05 | `a874ee9198a070fe` | `b4cbcddc950c1aa6` | 227 |
| ge10 | `96b9b989e2d2d32c` | `16ea9692a80cb3d9` | 296 |
| gg05 | `d1d5434d56bdc341` | `235e2878f4aefdd1` | 227 |
| gg10 | `f82d6fdb78041cd4` | `c124c974263f7901` | 295 |
| rc05 | `ebb20ed148f75185` | `4002e232ea272c83` | 213 |
| rc10 | `116d56dd0773b0c7` | `7266019be3601ae5` | 241 |
| rc15 | `edb191b5fde8f36b` | `1bc6b9e29ae2c5c1` | 280 |
| fam100-rc05 | `84d652d4684278d0` | `4002e232ea272c83` | 242 |
| fam100-ge05 | `6b7006750908e0b6` | `b4cbcddc950c1aa6` | 256 |

## 3. Les tables apprises

g par grande classe (moyenne pondérée par la part disponible ou générée, surcouches complètes ; g > 1 : la classe est
prise plus que sa part, elle coûte moins cher, multiplicateur g^(−γ)) [mesuré]. « Général » = roulé contre disponible,
toutes équipes (A y pèse 79 % des km-passages ROAD et 73 % GRAVEL [calculé]) ; « recalé » = réel contre généré par
`base`.

| classe | ROAD général | ROAD recalé | GRAVEL général | GRAVEL recalé |
|---|---:|---:|---:|---:|
| grande route | 1,77 | 1,90 | 0,80 | 1,24 |
| tertiaire | 2,16 | 1,20 | 1,14 | **0,63** |
| petite route | 0,68 | 0,73 | 0,77 | 0,74 |
| rue | 0,47 | 0,76 | 0,71 | 0,95 |
| voie cyclable ou verte | **1,20** | **0,52** | **1,93** | **0,84** |
| chemin | 0,47 | 2,99 | **1,07** | **1,82** |
| non revêtu connu | — | — | 1,96 | 1,84 |
| balisé | **1,77** | **0,76** | **2,10** | **1,02** |

Les deux estimateurs s'accordent là où `K` n'a pas d'avis tranché (grandes routes en ROAD, petites routes, non revêtu
en GRAVEL) et **s'opposent exactement sur les classes où le relevé du 4a a trouvé l'écart** (en gras) : voies cyclables
et balisage en ROAD, chemins, `tertiary`, voies cyclables et balisage en GRAVEL. L'équipe A roule ses voies cyclables
ROAD à peu près dans la proportion où elles sont disponibles (g 1,15) et ses chemins GRAVEL un peu plus (g 1,17) : « roulé
contre disponible » n'y voit **pas de goût à corriger**, alors que le moteur, par `K`, en prend 5 fois trop (cyclables
ROAD) ou 2,7 fois trop peu (chemins GRAVEL) [mesuré, §4.1 du 4a]. Les tables d'équipe (`ge`) de A, B, C, D, E ont la même
forme que la générale en ROAD (grande route et `tertiary` à g 1,5 à 2,7, rue à 0,4 à 0,65) ; B s'en distingue en GRAVEL
(chemin 0,83, cyclable 2,22) et en ROAD par ses rues (0,64) et ses cyclables (1,84) [mesuré].

Les tables ne sont pas normalisées (§12.11, point 12) : au multiplicateur moyen pondéré par la part disponible, `ge10`
renchérit le réseau de A de ×1,5 environ en ROAD [calculé], ce qui rend les virages (coût absolu) relativement moins
chers. Effet du second ordre, non isolé par la sonde.

## 4. Le critère

`S / S_base` par groupe, toutes équipes ; en gras ≤ 0,5 [mesuré]. `S_base` : ROAD 0,25 · ROAD 1 · GRAVEL 0,25 ·
GRAVEL 1.

### 4.1 Lecture « toutes » (`S_base` 0,614 · 0,721 · 0,735 · 0,680)

| variante | ROAD 0,25 | ROAD 1 | GRAVEL 0,25 | GRAVEL 1 |
|---|---:|---:|---:|---:|
| fam075 | 0,78 | 0,75 | 0,95 | 0,93 |
| fam100 | 0,57 | 0,52 | 0,94 | 0,99 |
| ge05 | 0,84 | 0,74 | 1,26 | 1,30 |
| ge10 | 1,03 | 0,90 | 1,33 | 1,43 |
| gg05 | 0,82 | 0,74 | 1,24 | 1,33 |
| gg10 | 1,01 | 0,90 | 1,37 | 1,48 |
| rc05 | 0,503 | **0,49** | 0,54 | 0,57 |
| rc10 | 1,00 | 1,03 | 0,54 | 0,51 |
| rc15 | 1,32 | 1,27 | 0,76 | 0,71 |
| **fam100-rc05** | **0,42** | **0,34** | 0,64 | 0,61 |
| fam100-ge05 | 0,78 | 0,70 | 1,19 | 1,26 |

### 4.2 Lecture « écart », la lecture honnête (204 boucles ROAD, 81 GRAVEL ; `S_base` 0,628 · 0,728 · 0,717 · 0,684)

| variante | ROAD 0,25 | ROAD 1 | GRAVEL 0,25 | GRAVEL 1 |
|---|---:|---:|---:|---:|
| fam075 | 0,76 | 0,73 | 1,01 | 0,97 |
| fam100 | 0,58 | 0,54 | 0,96 | 1,03 |
| ge05 | 0,82 | 0,74 | 1,20 | 1,29 |
| ge10 | 1,04 | 0,92 | 1,32 | 1,42 |
| gg05 | 0,86 | 0,74 | 1,28 | 1,37 |
| gg10 | 0,98 | 0,92 | 1,34 | 1,39 |
| rc05 | 0,54 | **0,49** | 0,53 | 0,56 |
| rc10 | 1,00 | 1,03 | 0,58 | 0,52 |
| rc15 | 1,31 | 1,23 | 0,80 | 0,72 |
| **fam100-rc05** | **0,45** | **0,36** | 0,65 | 0,65 |
| fam100-ge05 | 0,78 | 0,70 | 1,15 | 1,15 |

La lecture « écart, dans l'échantillon » (mêmes boucles, surcouche complète) donne les rapports de la lecture « écart »
à 0,05 près, une fois 0,09 (`gg05`, GRAVEL 0,25), sans sens constant (`fam100-rc05` 0,41 · 0,35 · 0,65 · 0,60 ; `rc05`
0,50 · 0,49 · 0,54 · 0,56 ; `ge05` 0,79 · 0,77 · 1,23 · 1,29) : **la fuite de l'apprentissage est faible**, et les
lectures « toutes » et « écart » disent la même chose.

### 4.3 Où l'écart reste (curseur 0,25, lecture « toutes ») [mesuré]

P50 du premier parcours, puis W1 / IQR entre parenthèses ; le curseur 1 dit la même chose.

| mesure | réel P50 | base | fam100 | ge05 | rc05 | rc10 | fam100-rc05 |
|---|---:|---|---|---|---|---|---|
| **ROAD** way_major | 0,244 | 0,104 (0,77) | 0,187 (0,31) | 0,228 (0,15) | 0,411 (0,83) | 0,585 (1,77) | 0,351 (0,57) |
| way_tertiary | 0,418 | 0,314 (0,38) | 0,369 (0,20) | 0,485 (0,32) | 0,334 (0,33) | 0,297 (0,50) | 0,388 (0,11) |
| way_minor | 0,140 | 0,265 (0,66) | 0,203 (0,31) | 0,086 (0,39) | 0,118 (0,22) | 0,055 (0,60) | 0,110 (0,23) |
| way_street | 0,067 | 0,108 (0,46) | 0,070 (0,25) | 0,027 (0,61) | 0,051 (0,35) | 0,027 (0,63) | 0,047 (0,40) |
| way_cycle | 0,033 | 0,157 (1,56) | 0,123 (1,13) | 0,130 (1,26) | 0,057 (0,30) | 0,014 (0,52) | 0,054 (0,30) |
| surf_paved | 0,738 | 0,681 (0,21) | 0,724 (0,09) | 0,807 (0,37) | 0,761 (0,16) | 0,794 (0,30) | 0,764 (0,16) |
| surf_presumed | 0,252 | 0,319 (0,30) | 0,276 (0,16) | 0,193 (0,28) | 0,239 (0,08) | 0,206 (0,21) | 0,236 (0,08) |
| marked | 0,163 | 0,249 (0,58) | 0,215 (0,37) | 0,288 (0,76) | 0,174 (0,20) | 0,123 (0,39) | 0,153 (0,20) |
| **GRAVEL** way_major | 0,043 | 0,030 (0,35) | 0,042 (0,22) | 0,031 (0,27) | 0,046 (0,35) | 0,100 (1,20) | 0,062 (0,50) |
| way_tertiary | 0,085 | 0,225 (2,15) | 0,226 (2,07) | 0,193 (1,74) | 0,103 (0,24) | 0,059 (0,45) | 0,132 (0,67) |
| way_minor | 0,097 | 0,188 (0,96) | 0,174 (0,80) | 0,146 (0,51) | 0,134 (0,41) | 0,108 (0,15) | 0,142 (0,53) |
| way_cycle | 0,134 | 0,136 (0,46) | 0,175 (0,51) | 0,291 (1,29) | 0,159 (0,52) | 0,151 (0,22) | 0,166 (0,54) |
| way_path | 0,447 | 0,166 (1,09) | 0,157 (1,11) | 0,147 (1,18) | 0,276 (0,60) | 0,355 (0,35) | 0,260 (0,75) |
| surf_unpaved | 0,253 | 0,099 (0,99) | 0,107 (0,87) | 0,120 (0,85) | 0,166 (0,50) | 0,175 (0,41) | 0,157 (0,55) |
| surf_paved | 0,400 | 0,570 (0,95) | 0,578 (0,91) | 0,632 (1,23) | 0,502 (0,51) | 0,474 (0,39) | 0,499 (0,52) |
| marked | 0,311 | 0,298 (0,10) | 0,343 (0,16) | 0,555 (1,50) | 0,396 (0,40) | 0,375 (0,34) | 0,385 (0,33) |

- **Le goût d'équipe corrige les grandes routes ROAD** (0,104 → 0,228, W1 / IQR 0,77 → 0,15) **mais ni les voies
  cyclables, ni le balisage** (0,13 contre 0,033 ; 0,29 contre 0,16, pire que `base`), et il pousse trop loin rues et
  petites routes. En GRAVEL, il double les voies cyclables et le balisage (0,29 et 0,56 contre 0,13 et 0,31) **sans
  toucher aux chemins** (0,147 contre 0,447) : c'est le désaccord du §3.
- **Le recalé corrige les classes que `K` biaise** (cyclables ROAD 0,157 → 0,057 ; `tertiary` GRAVEL 0,225 → 0,103 ;
  chemins GRAVEL 0,166 → 0,276 à 0,355) ; à γ = 1 il dépasse la cible en ROAD (grandes routes 0,585 contre 0,244) : la
  réponse du moteur à une baisse de poids des grandes routes est très élastique (×0,53 sur le poids, ×5,6 sur la part).
- **La familiarité à 1** rapproche le ROAD sur toutes les classes à la fois (les arêtes d'équipe les plus passées sont
  grandes routes et `tertiary`) ; en GRAVEL elle ne change presque rien aux chemins (0,157), et augmente un peu la
  `tertiary` au curseur 1 (0,210 contre 0,181) : les arêtes les plus passées d'un réseau gravel sont ses routes d'accès.
- En GRAVEL, **aucune variante ne ramène les chemins** : 0,26 à 0,36 au mieux contre 0,447 (`rc15` 0,371 à 0,436, mais il
  triple les grandes routes, W1 / IQR 2,7). Les 72 classes sont couplées : un pas multiplicatif unique ne suffit pas.

## 5. Par équipe

`S / S_base`, lectures « toutes » puis « écart » ; curseurs 0,25 · 1 [mesuré]. B « écart » : 14 boucles ROAD et 26
GRAVEL, indicatif.

| type | équipe | `S_base` (toutes) | fam100 | ge05 | gg05 | rc05 | rc10 | fam100-rc05 | fam100-ge05 |
|---|---|---|---|---|---|---|---|---|---|
| ROAD | A, toutes | 0,694 · 0,796 | 0,57 · 0,54 | 0,77 · 0,69 | 0,76 · 0,69 | **0,45 · 0,44** | 0,85 · 0,89 | **0,35 · 0,29** | 0,71 · 0,65 |
| ROAD | A, écart | 0,690 · 0,770 | 0,58 · 0,56 | 0,74 · 0,70 | 0,81 · 0,71 | **0,50 · 0,46** | 0,89 · 0,94 | **0,39 · 0,32** | 0,70 · 0,66 |
| ROAD | B, toutes | 0,349 · 0,324 | 1,24 · 1,40 | 1,49 · 1,30 | 1,79 · 2,21 | 2,11 · 2,28 | 3,03 · 3,62 | 1,97 · 2,07 | 1,43 · 1,66 |
| ROAD | B, écart | 0,454 · 0,457 | 0,96 · 0,96 | 1,44 · 1,21 | 1,20 · 1,65 | 1,42 · 1,46 | 2,28 · 2,48 | 1,49 · 1,30 | 1,43 · 1,40 |
| GRAVEL | A, toutes | 0,839 · 0,778 | 0,98 · 1,02 | 1,23 · 1,28 | 1,26 · 1,33 | 0,67 · 0,61 | 0,58 · **0,47** | 0,75 · 0,74 | 1,24 · 1,29 |
| GRAVEL | A, écart | 0,812 · 0,742 | 1,06 · 1,19 | 1,16 · 1,37 | 1,37 · 1,45 | 0,74 · 0,66 | 0,64 · 0,53 | 0,81 · 0,89 | 1,28 · 1,28 |
| GRAVEL | B, toutes | 0,601 · 0,575 | 0,91 · 0,96 | 1,34 · 1,32 | 1,14 · 1,17 | 0,54 · 0,70 | 0,76 · 0,86 | **0,46** · 0,55 | 1,13 · 1,21 |
| GRAVEL | B, écart | 0,654 · 0,705 | 0,81 · 0,80 | 1,23 · 1,01 | 0,99 · 0,99 | 0,64 · 0,61 | 0,68 · 0,67 | **0,47** · 0,55 | 0,98 · 0,95 |

- **L'écart ROAD du 4a est celui de A.** B part d'un `S` deux fois plus petit (0,35 contre 0,69 à 0,80), du niveau que A
  n'atteint qu'après correction. Tout réglage tiré des sommes communes (A y pèse ~85 %) l'éloigne : `rc05` double son `S`
  ROAD (2,1 à 2,3), parce qu'il pousse vers les grandes routes (B : 0,30 généré contre 0,125 réel) et retire les voies
  cyclables que B roule (0,038 contre 0,126) [mesuré].
- **Le goût d'équipe de B ne le sauve pas** : `ge05` aggrave aussi son ROAD (1,2 à 1,5) et son GRAVEL (1,0 à 1,3). Moins que le
  réglage commun en ROAD, mais pire que ne rien faire.
- En GRAVEL, **le recalé commun aide les deux équipes** (`rc05` et `rc10` : A 0,47 à 0,74, B 0,54 à 0,86) : leurs écarts GRAVEL vont dans le
  même sens (chemins, `tertiary`).

## 6. Forme

P50 du premier parcours, W1 / IQR entre parenthèses ; lecture « toutes », curseur 0,25 ; `ge10` d'après le tableau 6 du
banc [mesuré].

| type | mesure | réel | base | fam100 | ge05 | ge10 | rc05 | rc10 | fam100-rc05 |
|---|---|---:|---|---|---|---|---|---|---|
| ROAD | r / D | 0,329 | 0,390 (1,03) | 0,396 (1,10) | 0,388 (0,99) | 0,374 (0,77) | 0,408 (1,30) | 0,401 (1,19) | 0,400 (1,16) |
| ROAD | détour jusqu'au point lointain | 1,492 | 1,253 (0,88) | 1,236 (0,93) | 1,257 (0,86) | 1,308 (0,73) | 1,185 (1,06) | 1,201 (1,00) | 1,215 (0,98) |
| ROAD | compacité | 0,320 | 0,216 (0,58) | 0,218 (0,55) | 0,271 (0,37) | 0,263 (0,36) | 0,216 (0,59) | 0,244 (0,46) | 0,249 (0,40) |
| ROAD | virages / 10 km | 11,5 | 13,6 (0,39) | 11,0 (0,32) | 10,7 (0,34) | 10,4 (0,38) | 9,6 (0,53) | 9,0 (0,65) | 9,5 (0,57) |
| GRAVEL | r / D | 0,285 | 0,387 (1,76) | 0,382 (1,68) | 0,373 (1,52) | 0,353 (1,19) | 0,375 (1,57) | 0,364 (1,40) | 0,370 (1,50) |
| GRAVEL | détour jusqu'au point lointain | 1,722 | 1,276 (1,15) | 1,296 (1,11) | 1,329 (1,04) | 1,398 (0,87) | 1,311 (1,07) | 1,348 (1,01) | 1,335 (1,03) |
| GRAVEL | compacité | 0,226 | 0,186 (0,32) | 0,172 (0,33) | 0,208 (0,18) | 0,223 (0,11) | 0,173 (0,28) | 0,185 (0,27) | 0,186 (0,24) |
| GRAVEL | virages / 10 km | 27,6 | 15,7 (1,48) | 15,7 (1,48) | 17,5 (1,32) | 18,4 (1,19) | 19,3 (1,04) | 21,8 (0,79) | 17,0 (1,31) |

Au curseur 1, mêmes valeurs à 0,01 près pour r / D, 0,03 pour le détour, 0,04 pour la compacité ; `rc10` GRAVEL y fait 0,361 de r / D,
1,354 de détour et 24,0 virages aux 10 km. Lecture « écart » : mêmes sens, mêmes ordres de grandeur. Par équipe, A et B
suivent les mêmes sens ; en GRAVEL, B a le meilleur r / D avec `ge05` (0,354 contre 0,375) et A avec `rc10` (0,362 contre
0,390) [mesuré].

- **GRAVEL** : le goût le plus fort sur les chemins (`rc10`, `rc15`) baisse r / D de 0,387 à 0,355 à 0,364 et monte le
  détour de 1,28 à 1,35 à 1,37 ; il reste **0,07 à 0,08 de r / D et 0,35 de détour** jusqu'au réel (0,285 ; 1,72). Le goût
  porte **20 à 30 % de l'écart de r / D** [calculé] ; les virages, eux, se rapprochent nettement (15,7 → 21,8 à 24,0 contre
  27,6). Les variantes `ge10` / `gg10` font aussi bien sur r / D (0,353) en empirant les classes : renchérir en bloc le réseau
  (§3) allonge l'aller sans le faire passer par les bonnes classes.
- **ROAD** : le réglage qui corrige les classes **éloigne** la forme : r / D 0,390 → 0,400 à 0,414, détour 1,25 → 1,17 à
  1,22 (les grandes routes sont droites), virages sous le réel (8,8 à 9,7 contre 11,5). La compacité, elle, se rapproche
  (0,216 → 0,244 à 0,249).

## 7. Service

Surcouche complète, toutes les boucles ; ms P50 / P90 sur 16 fils, machine pleine, **indicatifs** (même réserve qu'au
4a) ; « ×base » = P50 contre `base` de la même campagne, curseurs 0,25 · 1 [mesuré].

| variante | `OK` (0,25 · 1) | ms P50 / P90 au curseur 0,25 | ×base | issues nouvelles |
|---|---|---|---|---|
| base | 98,1 % · 98,2 % | 265 / 695 | 1 · 1 | — |
| fam075 | 98,1 % · 98,2 % | 280 / 701 | 1,06 · 1,06 | — |
| fam100 | 98,2 % · 98,3 % | 305 / 771 | 1,15 · 1,12 | `BUDGET` 2 |
| ge05 · gg05 | 98,1 à 98,2 % | 307 à 319 / 743 à 769 | 1,16 à 1,20 · 1,21 | `BUDGET` 0 à 1 |
| ge10 · gg10 | 97,9 à 98,1 % | 492 à 516 / 1 119 à 1 161 | 1,86 à 1,95 · 1,67 à 1,76 | `BUDGET` 0 à 1 |
| rc05 | 98,1 % · 98,1 % | 274 / 717 | 1,04 · 1,17 | `BUDGET` 5 |
| rc10 | 98,1 % · 98,0 % | 362 / 844 | 1,37 · 1,32 | `BUDGET` 5, `VIOLATION` 1 (`SHARP_TURN`) |
| rc15 | 98,1 % · 97,9 % | 466 / 951 | 1,78 · 1,72 | `BUDGET` 5 |
| fam100-rc05 | 98,4 % · 98,4 % | 339 / 866 | 1,29 · 1,39 | `BUDGET` 8 |
| fam100-ge05 | 98,2 % · 98,1 % | 371 / 935 | 1,42 · 1,44 | `BUDGET` 7 |

- **Aucune `PANIC`, aucun `WINDOW_ESCAPE`, aucune part `OK` en baisse de plus d'un point** ; `NO_ROUTE:BUDGET` apparaît
  (1 à 4 jeux sur 2 855 par curseur) et le contrôleur refuse un parcours de `rc10` (`SHARP_TURN`, écart moteur /
  contrôleur connu au lot 6).
- **Temps** : γ = 0,5 coûte 4 à 21 % au P50, γ ≥ 1 de 32 à 95 % ; la familiarité 1 de 12 à 15 %, la combinaison 29 à 39 %.
  Signal « P50 > 1,25 × base » du §12.6 : `ge10`, `gg10`, `rc10`, `rc15`, `fam100-rc05`, `fam100-ge05` (indicatif).
- **Part d'équipe au curseur 1** (P50) : 0,938 (`base`), 0,952 (`rc05`), 0,975 (`fam100`), 0,981 (`fam100-rc05`) : la
  familiarité au curseur 1 (équipe d'abord à λ = 1, §12.0 point 2) **réduit la découverte**.
- **`WindowTeam` avec goût** : construction 2,2 fois plus longue (12,4 s pour 214 constructions → 27 à 29 s pour 214 à
  222 ; ~58 → ~128 ms chacune [calculé]), la plus grosse de 33,4 → 44,5 Mo (+33 %).

## 8. Réserves

- **A fait la statistique** (84 % des boucles) ; le recalé et le général sont presque les tables de A. B est la seule
  autre équipe lisible, et seulement en lecture « toutes » (B « écart » : 14 et 26 boucles).
- **`rc` est un réglage appris sur des générations** : le dépôt ne sait pas le calculer (il ne génère pas). Sa table
  « écart » est apprise sans les boucles mises à l'écart, mais sur les jeux de `base` de la surcouche complète (fuite
  indirecte relevée à la relecture, négligeable : §4.2).
- **Un seul pas** : chaque table est un rapport réel / généré pris une fois, à la puissance γ. La réponse du moteur est
  non linéaire et les classes sont couplées ; un réglage itéré (recaler sur les sorties de `rc05`) n'a pas été essayé.
- **Tables non normalisées** : l'échelle des poids change contre le coût absolu des virages (§3) ; non isolé.
- **`fam` au curseur 1 change le mode** (découverte → équipe d'abord à λ = 1) : il se compare en distribution, et ce n'est
  plus la découverte du service.
- **Temps** : machine pleine (16 fils), non comparables au lot 2 ; les rapports entre variantes sont lisibles, pas les
  valeurs.

## 9. Ce que la sonde décide

1. **Le critère du 4b (écarts de classes divisés par deux) est atteint en ROAD, pas en GRAVEL, et jamais par un goût
   d'équipe.**
   - **ROAD** : `fam100-rc05` atteint 0,34 à 0,45 dans les deux lectures et aux deux curseurs (A 0,29 à 0,39) ; `rc05`
     seul est à la limite (0,49 à 0,54) [mesuré].
   - **GRAVEL** : le meilleur est le recalé, 0,51 à 0,58 (`rc10`) ou 0,53 à 0,57 (`rc05`). La familiarité n'y aide pas
     (0,94 à 1,03) et gâte le recalé (`fam100-rc05` 0,61 à 0,65) [mesuré].
   - **Goût d'équipe** (`ge`, l'estimateur du §3.2) : 0,74 à 0,84 en ROAD à γ = 0,5, **1,20 à 1,43 en GRAVEL**, pire que
     ne rien faire. Le général (`gg`) donne les mêmes chiffres à 0,08 près.
2. **Profil par équipe ou réglage de `K` par type : ni l'un ni l'autre tel quel.**
   - **L'estimateur « roulé contre disponible » est réfuté** (§3, §4.3). Il mesure un goût contre un routeur neutre, alors
     que le moteur a déjà le sien (`K`). Il ne voit donc rien là où `K` se trompe (cyclables ROAD, chemins GRAVEL), et
     double `K` là où l'équipe et `K` s'accordent (balisage, cyclables GRAVEL).
   - **Le profil d'équipe stocké au dépôt (4b complet) ne doit pas être construit sur cet estimateur.**
   - **Le levier qui marche est un réglage de `K` par classe et par type, appris contre le moteur** (`rc`). Mais tiré des
     sommes communes, c'est le goût de A : il double l'écart ROAD de B (§5).
   - **Une différence d'équipe existe** (B roule 3 à 4 fois plus de voies cyclables ROAD que A). Pour la servir, il faudrait
     un recalé **par équipe**, c'est-à-dire des générations par équipe : rejouer ses boucles, ce qu'un dépôt de nuit
     pourrait faire (B : 11 s sur 16 fils au banc). **Non mesuré.**
3. **Familiarité : utile en ROAD, pas en GRAVEL.**
   - **ROAD** : à 1 seule, 0,52 à 0,58 ; avec `rc05`, elle fait passer le ROAD sous 0,5. Elle ne demande que de passer
     `familiarity = 1` en ROAD au curseur 0,25, sans code moteur.
   - **Au curseur 1** : ne pas la passer (elle y supprime la découverte, part d'équipe 0,94 → 0,98). Au curseur 1, `rc05`
     seul fait déjà 0,49.
   - **GRAVEL** : laisser 0,5.
4. **γ retenu : 0,5.**
   - γ = 1 dépasse la cible en ROAD (grandes routes 0,585 contre 0,244, `S` inchangé) et coûte +32 à 37 % de temps.
   - γ = 1,5 dépasse dans les deux types.
   - En GRAVEL, γ = 0,5 et γ = 1 sont équivalents (0,53 à 0,57 contre 0,51 à 0,58).
   - Pour `ge` / `gg`, 0,5 est aussi moins mauvais que 1.
5. **Détour et r / D : le goût ne les règle pas, ils restent au 4c.**
   - **GRAVEL** : r / D 0,387 → 0,355 à 0,375 au mieux (réel 0,285) ; détour jusqu'au point lointain 1,28 → 1,31 à 1,37
     (réel 1,72). C'est 20 à 30 % de l'écart de r / D [calculé]. Le relevé du 4a (§12 : « en GRAVEL, le goût est le levier
     de r / D ») est **démenti** : c'est un levier secondaire.
   - **ROAD** : le réglage qui corrige les classes allonge un peu r / D (0,39 → 0,40 à 0,41) et raccourcit le détour (1,25
     → 1,19 à 1,22).
   - Le 4c (triangle et couloir) garde **tout** l'écart de forme. Son critère r / D ROAD < 0,36 part de 0,40 si le réglage
     de `K` passe avant lui.

**Décision recommandée** : ne pas lancer le 4b complet (profil de goût au dépôt sur « roulé contre disponible »). À la
place :

- **(a)** une **courte sonde de plus au banc**, avant tout changement moteur, avec un recalé itéré (2 à 3 pas à γ = 0,5)
  et un recalé par équipe. Elle doit dire si le GRAVEL passe sous 0,5 et si B garde son ROAD. Elle demande au banc une
  source de table « recalée sur une variante » et « par équipe » : quelques dizaines de lignes, pas de code moteur.
- **(b)** puis, selon elle :
  - soit une **table de goût générale par type**, constante du moteur, appliquée à toutes les arêtes, qui remplace la
    part « classe » de `K` ;
  - soit, si le recalé par équipe l'emporte nettement pour B, un profil d'équipe **appris par regénération au dépôt**.
- **(c)** **familiarité 1 en ROAD au curseur 0,25**, réglage du service.

### 9.1 Ce qui change dans la note

- **§3.2** : l'estimateur « roulé / disponible à 1 km » est abandonné (relevé de la sonde, §3 et §4). Le goût est une table
  de 72 classes × 3 types, **apprise contre le moteur** (parts réelles / parts générées, γ = 0,5, un ou plusieurs pas). Il
  s'applique à **toutes** les arêtes de la vue, aux deux curseurs, sans distinction d'arête d'équipe. Le crochet du §12.1
  (`Taste`, `taste_weight`, neutre par défaut) en est la forme moteur.
  - **Table générale** : elle n'a pas sa place dans `WindowTeam`, mais dans les poids OSM de la `Window` (`w_osm`, via
    `edge_weight`). Ni mémoire par équipe ni surcoût de construction par équipe : le banc a mesuré +33 % de mémoire et ×2,2
    de temps pour la variante par équipe.
  - **Table par équipe** : elle garde la forme du §12.1. Les 12 → 16 octets par chaîne et par type deviennent mesurés :
    33,4 → 44,5 Mo pour la plus grosse fenêtre d'équipe.
  - La familiarité, réglage du service, devient un paramètre **par type** : 1 en ROAD au curseur 0,25, 0,5 ailleurs.
- **§3.5** : le fichier de profil `.ordp.enc` **perd le goût** : plus de 72 × 3 compteurs ni de trame de 500 m. Il ne
  garde que formes (4c) et destinations (4d). Si le recalé par équipe est retenu, il porte une table de 72 × 3 `u16`
  calculée en fin de dépôt par regénération, dont le temps de nuit est à mesurer.
- **§10** : l'étape 3 (« 4b : profil au dépôt ») devient :
  - **3a** : sonde courte, recalé itéré et par équipe (banc seulement) ;
  - **3b** : table de goût générale par type dans le moteur, ou profil par regénération, selon 3a. C'est un changement
    de poids voulu : références figées et tests dorés re-bénis, version du moteur montée.
  - **3c** : familiarité par type au service.
  - Le 4c (triangle et couloir) suit, avec ses distributions rejouées **après** 3b, puisque le réglage de `K` déplace
    r / D ROAD.
  - Formes et destinations au dépôt restent rattachées au 4c et au 4d.
