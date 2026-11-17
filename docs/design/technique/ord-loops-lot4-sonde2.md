# Générateur de parcours — lot 4 : relevé de la seconde sonde du goût (recalé itéré et par équipe)

> Relevé du 17 novembre 2026, binaire `release` du commit `4472078` de la branche `feat/ord-loops-lot4-sonde2` (banc
> `ord-loops-bench likeness --variants`, chaînes du §13 de la note, relues ; moteur, service, contrat et contrôleur
> **inchangés** ; le rapport porte « arbre modifié » : seule la note était en cours d'édition, le binaire est celui du
> commit), Rust 1.100.0, poste de développement (16 fils, 30 Go). Graphe et vues France `france-20261115-z11`, export
> privé et surcouches du 4a. Conception : [`ord-loops-lot4.md`](ord-loops-lot4.md), §13, décisions du §9.3 ; relevé
> précédent : [`ord-loops-lot4-sonde.md`](ord-loops-lot4-sonde.md). Sorties complètes, **privées et hors dépôt**, sous
> `~/.cache/osm-route-details/graph/lot4/` : `recal-essai-*`, `recal-general-*`, `recal-equipe-*`, `recal-plafond500-*`,
> `recal-plafond-aucun-*`. Équipes désignées par les lettres A à I ; ni départ, ni coordonnée, ni identifiant d'équipe,
> de trace ou d'arête ici.
>
> Marques : **[mesuré]** vient des rapports du banc (`<label>-compare.md`, résumés par variante) ou de leur JSON relu
> sans recalcul (valeurs non arrondies) ; **[calculé]** d'un calcul fait ici sur ces mesures. Sans marque dans un
> tableau : mesuré.

## 1. Questions et protocole

**Questions** (§13 de la note, décision 2 du §9.2) : un recalé **itéré** (3 pas, chacun appris contre les parcours du
précédent) fait-il passer le GRAVEL sous `S / S_base ≤ 0,5`, et à quel pas ? Un recalé **par équipe** (`rt`, longueurs de
l'équipe seule, repli vers la table commune à `n0` = 25 boucles) garde-t-il B juste en ROAD sans coûter à A ? Que
coûterait un profil par équipe appris au dépôt ? Le service tient-il ?

- **Réel, requêtes, graine, retenue, critère `S`** : ceux du 4a et de la première sonde, à l'identique (2 855 boucles
  retenues, 2 071 ROAD, 780 GRAVEL ; curseurs 0,25 et 1 ; premier parcours, jeux `OK`).
- **Le pas** (§13.2 de la note) : `g` = rapport lissé réel / généré par classe (72 cases par type, ou 11 effets
  principaux), multiplicateur cumulé `mult ← mult · g^(−γ)`, γ = 0,5, γ d'un paramètre divisé par deux quand sa correction
  change de signe, borne de pas `[1/3, 3]`, borne cumulée `[1/4, 4]` ; pas de normalisation (décision 4 du §9.3).
- **Variantes** :

| chaîne | ce qui change | appris sur |
|---|---|---|
| `base` | rien : le moteur 1.9.2 | — |
| `rc05-p1..p3` | table **commune** par type, 72 cases ; `rc05-p1` est le `rc05` de la première sonde | `base`, puis le pas précédent |
| `rc05-me-p1..p3` | la même en **effets principaux** (voie, revêtement, balisé) | idem |
| `rt05-p1..p3` | une table **par équipe**, 72 cases, mélangée à la table commune du même pas avec le poids `G_T / (G_T + n0 · G_P / N_P)` par classe, `n0` = 25 | idem, chaque équipe sur ses jeux |
| `famroad100-…` | les mêmes chaînes avec la familiarité 1 en ROAD, équipe d'abord (curseur 0,25) seulement | idem |

- **Lectures** : « toutes » (surcouche complète, toutes les boucles) ; « écart » (boucles mises à l'écart, surcouche
  « écart », tables apprises sans elles) ; « écart, dans l'échantillon » ; « apprentissage » (boucles non mises à l'écart,
  surcouche complète ; §13.13 point 13 de la note). Le critère GRAVEL se lit sur « toutes » ; « écart » sert de contrôle de
  surapprentissage, toutes équipes (décision 6 du §9.3).
- **Commandes** (depuis le dépôt, `B=./target/release/ord-loops-bench`), l'une après l'autre, jamais deux processus :
  1. essai court : `$B likeness --teams B,C --limit 5 --variants base,rc:0.5+n:2,rt:0.5+n:2,famroad:1+rt:0.5+dim:me+n:2 --label recal-essai` ;
  2. pas de passe de déterminisme complète (décision 1 du §9.3 ; faite à petite échelle à la relecture, §13.13 point 15) ;
  3. campagne 1 (réglages du 4a, 10 % à l'écart) : `$B likeness --label recal-general --variants base,rc:0.5+n:3,rc:0.5+dim:me+n:3` ;
  4. campagne 2 (25 % à l'écart, forme `72` par la règle du §13.8 point 4, §3.3 ci-dessous) :
     `$B likeness --label recal-equipe --holdout-share 0.25 --variants base,rc:0.5+n:3,rt:0.5+n:3,famroad:1+rc:0.5+n:3,famroad:1+rt:0.5+n:3` ;
  5. plafond de 500 boucles (décision 8 du §9.3, sans code nouveau, §8.2) :
     `$B likeness --teams A --limit 500 --holdout-share 0.25 --variants base,rc:0.5+n:3 --label recal-plafond500`, puis la
     même sans `--limit` (`recal-plafond-aucun`).
- **Durée et mémoire** [mesuré] : essai 57 s (RSS 12,5 Gio) ; campagne 1, 27 min 38 s, 216 à 306 s par variante, pic
  anonyme 15,3 à 16,6 Go, RSS maximal 18,9 Gio ; campagne 2, 58 min 05 s, 255 à 298 s par variante, pic anonyme 9,1 à
  14,4 Go, RSS maximal 16,8 Gio. Sous les 21 Go attendus.

## 2. Contrôles et empreintes

- **Essai court** : sorties et comparaison lisibles, aucune panique ; `rt` diffère de `rc` dès le pas 1 (deux équipes).
  À 5 boucles par équipe, aucune n'est mise à l'écart (seuil de 10) : les lectures « écart » y sont vides, attendu.
- **Campagne 1** : `base` = **`55c3b065dc085f42`**, l'empreinte du 4a (contrôle : oui) ; `rc05-p1` = **`ebb20ed148f75185`**,
  `taste_xxh3` **`4002e232ea272c83`**, le `rc05` de la première sonde au bit près (contrôle « pas 1 = `rc05` » : oui). Aucun
  arrêt.
- **Campagne 2** : contrôle « `S_base` toutes = 0,614 · 0,721 · 0,735 · 0,680 » : **oui** (la mise à l'écart à 25 % ne
  change pas la lecture « toutes ») ; les deux autres contrôles sont sans objet hors réglages du 4a.
- **Familiarité ROAD seulement** : les chaînes `famroad100-…` redonnent en GRAVEL les `S` des chaînes sans familiarité à
  la troisième décimale, et leur pas 1 la même `taste_xxh3` (`40c8278314911d91` pour `rc`, `a8c20f73e3e00422` pour `rt`) :
  `famroad` ne touche que le ROAD en équipe d'abord, comme voulu [mesuré].

| variante | empreinte (campagne 1) | `taste_xxh3` | durée (s) | | variante | empreinte (campagne 2) | `taste_xxh3` | durée (s) |
|---|---|---|---:|---|---|---|---|---:|
| base | `55c3b065dc085f42` | `2d06800538d394c2` | 216 | | base | `4686c904975302e7` | `2d06800538d394c2` | 255 |
| rc05-p1 | `ebb20ed148f75185` | `4002e232ea272c83` | 218 | | rc05-p1 | `a123cb82e6b36fc1` | `40c8278314911d91` | 271 |
| rc05-p2 | `9254759cd668dda5` | `93b93e06596b7596` | 221 | | rc05-p2 | `ad57882c3ed75c10` | `2f57dc10b00d143d` | 262 |
| rc05-p3 | `8d10e088e41385ee` | `2672c1afd730652b` | 226 | | rc05-p3 | `2c563b8bbe8059ae` | `66ee08d3eb9c910c` | 264 |
| rc05-me-p1 | `0614d4b7d7b5d11a` | `a7885b595633eef7` | 221 | | rt05-p1 | `b6a5be1b4cb30f61` | `a8c20f73e3e00422` | 298 |
| rc05-me-p2 | `3752697eb1e94700` | `525eb5b08e2a8cf9` | 224 | | rt05-p2 | `f2d29d64d429cbbc` | `b022f20476770f25` | 278 |
| rc05-me-p3 | `615236381a4bb375` | `0f87dc465b4dac1e` | 306 | | rt05-p3 | `451651c82f8a4b92` | `3fc8e87f9b9fbfcf` | 262 |
| | | | | | famroad100-rc05-p1..p3 | `daebf457…` · `2b8e0f60…` · `f7c39d1e…` | `40c82783…` · `dedf52d2…` · `093343ba…` | 262 · 258 · 261 |
| | | | | | famroad100-rt05-p1..p3 | `f9f77af0…` · `e571cbcb…` · `0a20ba21…` | `a8c20f73…` · `a204fcf4…` · `81c5f7dc…` | 261 · 265 · 263 |

L'empreinte de `base` en campagne 2 diffère de celle du 4a parce qu'elle couvre aussi les jeux « écart », qui changent
avec la part mise à l'écart ; ses `S` « toutes » sont ceux du 4a.

## 3. Campagne 1 : la table commune, pas à pas (10 % à l'écart)

`S / S_base`, toutes équipes ; groupes ROAD 0,25 · ROAD 1 · GRAVEL 0,25 · GRAVEL 1 ; en gras ≤ 0,5 [mesuré]. `S_base`
« toutes » 0,614 · 0,721 · 0,735 · 0,680 ; « écart » (204 boucles ROAD, 81 GRAVEL) 0,628 · 0,728 · 0,717 · 0,684.

### 3.1 72 cases (`rc05`)

| pas | toutes | écart | écart − toutes [calculé] |
|---|---|---|---|
| p1 (= `rc05`) | 0,503 · **0,485** · 0,543 · 0,572 | 0,536 · **0,490** · 0,528 · 0,562 | +0,03 · +0,01 · −0,02 · −0,01 |
| p2 | **0,277 · 0,242 · 0,420 · 0,328** | **0,263 · 0,242 · 0,477 · 0,311** | −0,01 · 0,00 · +0,06 · −0,02 |
| p3 | **0,247 · 0,203 · 0,352 · 0,314** | **0,240 · 0,200 · 0,369 · 0,318** | −0,01 · 0,00 · +0,02 · 0,00 |

### 3.2 Effets principaux (`rc05-me`)

| pas | toutes | écart | écart − toutes [calculé] |
|---|---|---|---|
| p1 | 0,536 · **0,477 · 0,487 · 0,459** | 0,588 · 0,523 · **0,465** · 0,508 | +0,05 · +0,05 · −0,02 · +0,05 |
| p2 | **0,265 · 0,229 · 0,442 · 0,380** | **0,270 · 0,209 · 0,420 · 0,423** | +0,01 · −0,02 · −0,02 · +0,04 |
| p3 | **0,253 · 0,202 · 0,346 · 0,343** | **0,258 · 0,214 · 0,371 · 0,359** | +0,01 · +0,01 · +0,03 · +0,02 |

- **L'itération tient sa promesse** : le pas 2 fait passer les quatre groupes sous 0,5 dans les deux lectures, le pas 3
  gagne encore 0,01 à 0,07. Le GRAVEL passe de 0,54 à 0,57 (un pas, la première sonde) à **0,31 à 0,35** (« toutes »).
- **Diagnostics** (§3 ter du banc) : l'écart L1 des parts de classes que laissent les parcours passe, en ROAD, de 0,547
  (`base`) à 0,375, 0,127 puis 0,093 ; en GRAVEL de 0,555 à 0,270, 0,155 puis 0,123. Il décroît encore au pas 3, de moins
  en moins (−27 % ROAD, −21 % GRAVEL) : un quatrième pas gagnerait peu. L'amortissement a joué : 16 paramètres ROAD et 8
  GRAVEL ont vu leur γ divisé au pas 2, 10 et 8 au pas 3 (γ minimal 0,125) ; 1 à 4 classes à la borne cumulée au pas 3.
- **Multiplicateurs cumulés** au pas 3 (pondérés par la part générée du pas 1) : ROAD grandes routes ×0,79, cyclables
  ×1,59, balisé ×1,29, petites routes ×1,07 ; GRAVEL chemins ×0,64, non revêtu ×0,66, `tertiary` ×1,35, petites routes ×1,34,
  cyclables ×1,31, revêtu ×1,25. Au pas 1 les grandes routes ROAD étaient à ×0,73 : le pas 2 a corrigé le dépassement de
  `rc05` (0,411 de part contre 0,244 réel ; 0,274 au pas 3).

### 3.3 La forme retenue pour la campagne 2 : 72 cases

Règle du §13.8 point 4 (décision 3 du §9.3) : effets principaux s'ils font au plus 0,03 de plus que 72 cases au pas 3 en
lecture « écart », dans chacun des 4 groupes. Écarts `me − 72` [mesuré, non arrondis] : ROAD 0,25 **+0,018**, ROAD 1
**+0,014**, GRAVEL 0,25 **+0,002**, GRAVEL 1 **+0,042**. Le groupe GRAVEL 1 dépasse 0,03 : **X = 72**. En « toutes », les
effets principaux font +0,006 · −0,002 · −0,006 · +0,030 : ils ne gagnent nulle part de quoi changer la règle, et coûtent
plus de temps au pas 3 (P50 ×1,41 · ×1,32 contre ×1,16 · ×1,08 pour 72 cases, §7).

L'effet de jauge du mélange par équipe en effets principaux (option (a) de la décision 5 du §9.3, §13.13 point 16) est
donc **sans objet** : `rt` n'a été joué qu'en 72 cases.

## 4. Campagne 2 : commune ou par équipe, toutes équipes (25 % à l'écart)

`S / S_base`, toutes équipes [mesuré]. `S_base` « toutes » 0,614 · 0,721 · 0,735 · 0,680 ; « écart » (509 boucles ROAD,
202 GRAVEL) 0,628 · 0,755 · 0,752 · 0,685.

| chaîne | pas | toutes | écart | écart − toutes [calculé] |
|---|---|---|---|---|
| `rc05` | p1 | **0,498 · 0,467** · 0,546 · 0,578 | **0,474 · 0,451** · 0,519 · 0,545 | −0,03 · −0,02 · −0,03 · −0,03 |
| | p2 | **0,266 · 0,220 · 0,411 · 0,323** | **0,228 · 0,191 · 0,439 · 0,308** | −0,04 · −0,03 · +0,03 · −0,02 |
| | p3 | **0,255 · 0,194 · 0,377 · 0,303** | **0,228 · 0,176 · 0,387 · 0,303** | −0,03 · −0,02 · +0,01 · 0,00 |
| `rt05` | p1 | **0,494 · 0,483** · 0,566 · 0,531 | **0,476 · 0,460** · 0,508 · **0,493** | −0,02 · −0,02 · −0,06 · −0,04 |
| | p2 | **0,239 · 0,219 · 0,433 · 0,314** | **0,217 · 0,190 · 0,472 · 0,324** | −0,02 · −0,03 · +0,04 · +0,01 |
| | p3 | **0,244 · 0,195 · 0,371 · 0,259** | **0,203 · 0,183 · 0,379 · 0,286** | −0,04 · −0,01 · +0,01 · +0,03 |
| `famroad100-rc05` | p1 | **0,399 · 0,467** · 0,546 · 0,578 | **0,369 · 0,451** · 0,519 · 0,545 | |
| | p3 | **0,263 · 0,197 · 0,377 · 0,303** | **0,229 · 0,184 · 0,387 · 0,303** | −0,03 · −0,01 · +0,01 · 0,00 |
| `famroad100-rt05` | p1 | **0,380 · 0,483** · 0,566 · 0,531 | **0,330 · 0,460** · 0,508 · **0,493** | |
| | p3 | **0,253 · 0,172 · 0,371 · 0,259** | **0,221 · 0,165 · 0,379 · 0,286** | −0,03 · −0,01 · +0,01 · +0,03 |

- **Toutes équipes, `rt` et `rc` font la même chose en ROAD** (A y pèse 90 % des boucles) ; en GRAVEL, `rt` gagne au pas 3
  (0,26 contre 0,30 au curseur 1) : les deux grandes équipes GRAVEL (A et B) n'ont pas le même écart.
- **Surapprentissage** : `écart − toutes` reste dans ±0,06 à tous les pas, ±0,04 au pas 3 (les deux campagnes) ; la
  mesure du banc à surcouche égale (« écart, dans l'échantillon » − « apprentissage », §13.13 point 13) ne dépasse pas
  +0,04 en campagne 2 et +0,05 en campagne 1 (`rc05-me`), toutes équipes ; la fuite de la surcouche (« écart, dans
  l'échantillon » − « écart ») reste dans ±0,06. Rien n'approche le seuil de 0,10.
- **La familiarité ROAD n'apporte plus rien au pas 3** : elle n'aide qu'au pas 1 (0,38 à 0,40 contre 0,49 à 0,50 au
  curseur 0,25, la combinaison de la première sonde) ; au pas 3, 0,25 à 0,26 contre 0,24 à 0,26. L'itération fait ce que
  faisait la familiarité.

## 5. Par équipe (campagne 2)

`S / S_base` par équipe, curseurs 0,25 · 1 ; `rt − rc` au même pas [mesuré, non arrondis pour les différences]. A :
1 860 boucles ROAD, 545 GRAVEL (« écart » 459 et 142) ; B : 167 ROAD, 228 GRAVEL (« écart » 42 et 58, **indicatif**).
`S_base` « toutes » A 0,694 · 0,796 · 0,839 · 0,778, B 0,349 · 0,324 · 0,601 · 0,575.

### 5.1 ROAD

| équipe, lecture | pas | `rc05` | `rt05` | `rt − rc` | `famroad100-rc05` | `famroad100-rt05` |
|---|---|---|---|---|---|---|
| A, toutes | p1 | **0,45 · 0,42** | **0,47 · 0,47** | +0,022 · +0,042 | **0,33 · 0,42** | **0,34 · 0,47** |
| | p2 | **0,20 · 0,16** | **0,22 · 0,20** | +0,017 · +0,040 | **0,21 · 0,20** | **0,23 · 0,21** |
| | p3 | **0,20 · 0,15** | **0,22 · 0,17** | **+0,021 · +0,027** | **0,20 · 0,17** | **0,21 · 0,15** |
| A, écart | p3 | **0,19 · 0,14** | **0,18 · 0,17** | −0,008 · +0,023 | **0,17 · 0,17** | **0,18 · 0,15** |
| B, toutes | p1 | 2,11 · 2,21 | 0,77 · 0,95 | −1,33 · −1,26 | 1,92 · 2,21 | 1,28 · 0,95 |
| | p2 | 1,97 · 2,23 | 0,77 · 0,95 | −1,20 · −1,28 | 1,67 · 2,19 | 1,17 · 1,06 |
| | p3 | **1,78 · 2,14** | **0,80 · 0,93** | −0,98 · −1,21 | 1,66 · 2,04 | **1,05** · 0,98 |
| B, écart (indicatif) | p3 | 1,54 · 1,82 | 0,85 · 0,90 | −0,69 · −0,92 | 1,37 · 1,75 | 0,88 · 0,89 |

### 5.2 GRAVEL

| équipe, lecture | pas | `rc05` | `rt05` | `rt − rc` |
|---|---|---|---|---|
| A, toutes | p1 | 0,66 · 0,61 | 0,64 · 0,54 | −0,02 · −0,07 |
| | p3 | 0,53 · **0,39** | **0,41 · 0,24** | −0,12 · −0,14 |
| A, écart | p3 | 0,53 · **0,39** | **0,43 · 0,28** | −0,10 · −0,12 |
| B, toutes | p1 | 0,55 · 0,69 | 0,56 · 0,73 | +0,02 · +0,04 |
| | p3 | 0,67 · 0,82 | **0,43 · 0,42** | −0,24 · −0,40 |
| B, écart (indicatif) | p3 | 0,63 · 0,81 | **0,37 · 0,41** | −0,26 · −0,40 |

(`famroad` ne change rien au GRAVEL : mêmes valeurs.)

- **B garde son ROAD avec `rt`, et pas avec `rc`** : `rt` le tient sous 1,0 à tous les pas et aux deux curseurs (0,77 à
  0,95), alors que la table commune le double (1,78 à 2,23 ; elle s'améliore un peu en itérant, jamais sous 1,7). La
  lecture « écart » de B (42 boucles) dit la même chose : 0,72 à 1,04 contre 1,54 à 1,97.
- **B ne gagne pas en ROAD** : `rt` ne descend pas sous 0,77. B part d'un écart deux fois plus petit que A (`S_base` 0,35
  contre 0,69) ; son poids propre est de 0,83 (121 boucles ROAD d'apprentissage) et l'écart L1 de ses parcours ne bouge
  presque plus après le pas 1 (0,412 → 0,341 → 0,296 → 0,289) : sur 121 boucles, le levier « multiplicateur par classe »
  est à son plancher [mesuré, §3 ter du banc].
- **A perd un peu en ROAD** (+0,02 à +0,04 selon le pas, **+0,021 · +0,027 au pas 3** en « toutes ») : sa table propre est
  plus bruitée que la commune, dont il fait 90 %. En GRAVEL, **A gagne nettement** (−0,10 à −0,14 au pas 3) : la table
  commune y était tirée par B (29 % des boucles GRAVEL d'apprentissage), dont l'écart va ailleurs.
- **B gagne en GRAVEL avec `rt`** à partir du pas 2 (0,42 à 0,52 contre 0,67 à 0,82) ; la table commune itérée **l'éloigne**
  (0,55 → 0,67 au curseur 0,25, 0,69 → 0,82 au curseur 1) en suivant A.
- **La familiarité ROAD à 1 gâte B** : `famroad100-rt05` remonte B ROAD au curseur 0,25 à 1,05 à 1,28 (contre 0,77 à 0,80
  sans elle). Les arêtes d'équipe les plus passées de B sont ses grandes routes et `tertiary` ; elle n'apporte à A que
  −0,01 au pas 3.

## 6. Où l'écart reste, et forme

P50 du premier parcours, W1 / IQR entre parenthèses ; lecture « toutes », campagne 2 [mesuré].

| type | curseur | mesure | réel | base | `rc05-p3` | `rt05-p3` |
|---|---|---|---:|---|---|---|
| ROAD | 0,25 | grandes routes | 0,244 | 0,104 (0,77) | 0,272 (0,16) | 0,260 (0,10) |
| ROAD | 0,25 | voies cyclables | 0,033 | 0,157 (1,56) | 0,041 (0,32) | 0,041 (0,30) |
| GRAVEL | 0,25 | chemins | 0,447 | 0,166 (1,08) | 0,355 (0,35) | 0,369 (0,34) |
| GRAVEL | 0,25 | non revêtu connu | 0,253 | 0,099 (0,99) | 0,171 (0,41) | 0,169 (0,40) |
| GRAVEL | 0,25 | `tertiary` | 0,085 | 0,225 (2,15) | 0,115 (0,43) | 0,107 (0,32) |
| GRAVEL | 1 | chemins | 0,447 | 0,205 (0,98) | 0,397 (0,23) | 0,411 (0,17) |
| GRAVEL | 1 | non revêtu connu | 0,253 | 0,104 (1,00) | 0,194 (0,34) | 0,199 (0,33) |
| ROAD | 0,25 | r / D | 0,329 | 0,390 (1,03) | 0,402 (1,23) | 0,401 (1,22) |
| ROAD | 0,25 | détour jusqu'au point lointain | 1,492 | 1,253 (0,88) | 1,200 (1,02) | 1,202 (1,00) |
| ROAD | 0,25 | compacité | 0,320 | 0,216 (0,58) | 0,211 (0,63) | 0,214 (0,60) |
| ROAD | 0,25 | virages / 10 km | 11,5 | 13,6 (0,39) | 10,6 (0,37) | 10,5 (0,38) |
| GRAVEL | 0,25 | r / D | 0,285 | 0,387 (1,76) | 0,364 (1,41) | 0,363 (1,38) |
| GRAVEL | 0,25 | détour jusqu'au point lointain | 1,722 | 1,276 (1,15) | 1,345 (0,99) | 1,347 (1,00) |
| GRAVEL | 0,25 | compacité | 0,226 | 0,186 (0,32) | 0,195 (0,15) | 0,189 (0,25) |
| GRAVEL | 0,25 | virages / 10 km | 27,6 | 15,7 (1,48) | 19,6 (1,01) | 20,5 (0,93) |

- **Ce qui reste en classes** est en GRAVEL : chemins 0,36 à 0,41 contre 0,447, non revêtu 0,17 à 0,20 contre 0,253 ; les
  grandes routes et les cyclables ROAD sont réglées (W1 / IQR 0,10 à 0,32).
- **La forme ne bouge pas plus qu'avec un pas** : r / D ROAD 0,39 → 0,40 (le réglage des classes l'éloigne, comme à la
  première sonde), détour ROAD 1,25 → 1,20 ; GRAVEL r / D 0,387 → 0,363, détour 1,28 → 1,35 contre 1,72. Le curseur 1 dit la
  même chose à 0,01 près pour r / D. **Tout l'écart de forme reste au 4c** [mesuré]. Seule `famroad100-rt05-p3` rapproche un
  peu la compacité ROAD au curseur 0,25 (0,245, W1 / IQR 0,44).

## 7. Service

Surcouche complète, toutes les boucles ; P50 sur 16 fils, machine pleine, **indicatifs** ; « ×base » contre `base` de la
même campagne, curseurs 0,25 · 1 [mesuré].

| variante | `OK` (0,25 · 1) | ms P50 au curseur 0,25 · 1 | ×base | issues nouvelles |
|---|---|---|---|---|
| base (c. 1 · c. 2) | 98,1 % · 98,2 % | 276 · 246 / 290 · 264 | 1 · 1 | — |
| rc05-p1..p3 (c. 1) | 98,0 à 98,1 % · 98,0 à 98,1 % | 282 à 321 · 266 à 276 | 1,02 à 1,16 · 1,08 à 1,12 | `BUDGET` 2 à 5 |
| rc05-me-p1..p2 (c. 1) | 98,2 à 98,4 % · 98,0 % | 297 à 333 · 274 à 290 | 1,08 à 1,21 · 1,11 à 1,18 | `BUDGET` 1 à 3 |
| **rc05-me-p3** (c. 1) | 98,2 % · 98,0 % | 389 · 324 | **1,41 · 1,32** | `BUDGET` 2 |
| rc05-p1..p3 (c. 2) | 98,1 à 98,2 % · 98,0 à 98,1 % | 311 à 354 · 275 à 296 | 1,07 à 1,22 · 1,04 à 1,12 | `BUDGET` 1 à 5 |
| **rt05-p1** (c. 2) | 98,0 % · 98,0 % | 363 · 337 | **1,25 · 1,28** | `BUDGET` 3 à 4 |
| rt05-p2..p3 (c. 2) | 98,0 à 98,1 % · 97,9 % | 338 à 344 · 275 à 290 | 1,17 à 1,19 · 1,04 à 1,10 | `BUDGET` 3 à 4 |
| famroad100-rc05-p1..p3 | 97,8 à 98,2 % · 97,9 à 98,1 % | 324 à 338 · 269 à 286 | 1,12 à 1,17 · 1,02 à 1,09 | `BUDGET` 2 à 4 |
| famroad100-rt05-p1..p3 | 97,9 à 98,2 % · 98,0 % | 318 à 360 · 275 à 289 | 1,10 à 1,24 · 1,04 à 1,10 | `BUDGET` 3 à 4 |

- **Aucune `PANIC`, aucun `WINDOW_ESCAPE`** ; part `OK` en baisse de 0,3 point au plus (`famroad100-rc05-p2`, 97,8 %
  contre 98,1 % ; plusieurs variantes à 97,9 % contre 98,2 % au curseur 1) ; `NO_ROUTE:BUDGET` 1 à 5 jeux sur 2 855 par curseur, comme à la première sonde.
- **Signal « P50 > 1,25 × base »** (indicatif) : `rc05-me-p3` (1,41 · 1,32) et `rt05-p1` (1,25 · 1,28, juste au seuil ;
  le même pas sous `famroad` fait 1,15 · 1,10, la machine pleine compte). **La variante retenue, `rt05-p3`, fait 1,17 ·
  1,04** ; `rc05-p3` 1,11 · 1,04 (campagne 2) et 1,16 · 1,08 (campagne 1).
- **Multiplicateur moyen** (`Σ p_d · mult`, décision 9 du §9.3) : table commune ROAD 0,907 à 0,914 aux pas 2 et 3, dans
  1 ± 0,10 mais au bord ; GRAVEL 0,997 à 1,013. **Hors de 1 ± 0,10 : la table propre de A en ROAD, 0,891 aux pas 2 et 3**
  (`famroad100-rt05` : 0,897 au pas 2) ; celle de B en ROAD monte à 1,100 au pas 3 (au seuil) ; les petites équipes
  (2 boucles) 1,11 à 1,13 au pas 1 (H ROAD 1,133, une équipe GRAVEL, B MTB). La dérive d'échelle reste à traiter au 3b
  (normalisation contre le coût des virages).
- **`WindowTeam` avec goût** : construction ×2,0 à ×2,3 (14,8 s pour 219 constructions → 29 à 35 s), la plus grosse 33,4 →
  44,5 Mo, comme à la première sonde [mesuré].

## 8. Coût d'un profil par équipe appris au dépôt

### 8.1 Temps

Trois pas demandent trois générations de toutes les boucles de l'équipe (`base`, p1, p2 ; le pas 3 n'est pas joué).
Temps cumulés des jeux sur la surcouche complète (`Σ PlayRec::ms`), mesurés **à 16 fils sur 8 cœurs, machine pleine**
(un cœur dédié est plus rapide, non mesuré) ; temps sur `k` fils = total / `k` [mesuré pour les totaux, calculé pour
les divisions].

| équipe (boucles) | génération | jeux | cpu (s) | mur à 16 fils (s) |
|---|---|---:|---:|---:|
| A (2 406) | base · p1 · p2 | 4 812 chacune | 2 069 · 2 634 · 2 437 | 199 · 244 · 227 |
| B (397) | base · p1 · p2 | 794 chacune | 156 · 177 · 147 | 15 · 17 · 14 |
| autres, somme (C à I, 52 boucles) | base · p1 · p2 | 104 chacune | 192 · 171 · 161 | 41 · 36 · 36 |

Soit **0,49 s-fil par appel pour A** (7 139 s pour 14 436 appels), 0,20 pour B [calculé]. Budget (décision 8 du §9.3) :
au plus **30 min sur 4 fils** pour la plus grosse équipe.

| A, `rt05`, 3 pas [calculé] | total (min-fil) | sur 4 fils | sur 12 fils |
|---|---:|---:|---:|
| deux curseurs, toutes les boucles | 119,0 | **29,7 min** | 9,9 min |
| un seul curseur (0,25) | 63,4 | 15,9 min | 5,3 min |
| plafond de 500 boucles, deux curseurs (× 500 / 2 406) | 24,7 | **6,2 min** | 2,1 min |
| plafond de 500 boucles, un seul curseur | 13,2 | 3,3 min | 1,1 min |
| deux pas seulement (`base`, p1), deux curseurs | 78,4 | 19,6 min | 6,5 min |
| **toutes les équipes**, plafond de 500, deux curseurs (A 24,7 + B 8,0 + autres 8,7) | 41,4 | 10,4 min | 3,5 min |

- **Sans allègement, A tient tout juste dans le budget** (29,7 min sur 4 fils), à condition de compter à la machine du
  banc : sur le serveur de production (12 cœurs partagés avec tous les services), 4 fils ne sont pas 4 cœurs libres. **Avec
  le plafond de 500 boucles, A passe à ~6 min sur 4 fils** et toutes les équipes d'une nuit à ~10 min [calculé] ; un
  recalcul hebdomadaire divise encore la charge par sept. Le temps proportionnel au nombre de boucles est vérifié au §8.2
  (×0,21 mesuré pour ×0,21 de boucles).
- La construction des fenêtres d'équipe est négligeable (3 s par génération pour A) ; la partie « autres » est dominée par
  les fenêtres des petites équipes (mur 36 à 41 s pour 104 jeux).
- **Mémoire** : **non mesurée** pour un conteneur de 3 Gio. Le banc tient le graphe de la France, toutes les fenêtres et un
  cache partagé de 6 Go (pic anonyme 9 à 16 Go) ; une fenêtre d'équipe avec goût pèse 44,5 Mo au plus. Ce que coûte la
  régénération dans le processus du service, à côté de ses fenêtres, reste à mesurer au 3b.

### 8.2 Effet d'un plafond de 500 boucles sur la qualité

Mesuré sans code nouveau, sur A seule (`--teams A`, 25 % à l'écart, chaîne `rc05` de 3 pas) : avec une seule équipe,
`rt` et `rc` donnent la même table (§13.13 point 6 de la note), c'est donc la table propre de A. Deux passes l'une après
l'autre : `--limit 500` (les 500 premières boucles de A dans l'ordre de l'export, dont 125 mises à l'écart : la table
apprend sur 375) et sans limite (2 406 boucles, ~1 800 d'apprentissage). `S_base` est recalculé dans chaque passe (IQR
du réel de A seule) : seuls les rapports se comparent [mesuré].

| passe | lecture (boucles ROAD · GRAVEL) | p1 | p2 | p3 |
|---|---|---|---|---|
| sans limite | toutes (1 860 · 545) | 0,459 · 0,451 · 0,613 · 0,535 | 0,222 · 0,201 · 0,431 · 0,278 | **0,218 · 0,183 · 0,386 · 0,239** |
| | écart (459 · 142) | 0,439 · 0,432 · 0,587 · 0,545 | 0,197 · 0,182 · 0,493 · 0,301 | **0,180 · 0,171 · 0,425 · 0,248** |
| plafond 500 | toutes (384 · 116) | 0,476 · 0,461 · 0,573 · 0,475 | 0,201 · 0,179 · 0,449 · 0,316 | **0,206 · 0,196 · 0,364 · 0,267** |
| | écart (85 · 40, indicatif en GRAVEL) | 0,513 · 0,418 · 0,581 · 0,507 | 0,235 · 0,198 · 0,446 · 0,367 | **0,219 · 0,233 · 0,364 · 0,350** |

- **En lecture honnête (« écart »), le plafond coûte +0,04 · +0,06 en ROAD au pas 3** (0,22 · 0,23 contre 0,18 · 0,17) ;
  en GRAVEL, −0,06 · +0,10 sur 40 boucles seulement, sans signe lisible. Tout reste **sous 0,5 avec de la marge** ; le
  plafond ne remet en cause ni le critère GRAVEL ni la décision par équipe pour A [calculé sur les mesures].
- **Réserves** : les deux lectures « écart » ne portent pas sur les mêmes boucles (125 contre 602 mises à l'écart) ; les
  500 premières boucles de l'export ne sont pas un tirage (leur `S_base` GRAVEL vaut 1,13 contre 1,11 pour toute l'équipe,
  ROAD 0,73 contre 0,76) ; au dépôt, la table apprendrait sur 500 boucles, pas 375. Un plafond par **tirage** (les 500 plus
  récentes, ou au hasard) n'est pas jouable sans code : non mesuré.
- **Temps** [mesuré] : `base`, p1 et p2 prennent 127 s de mur avec le plafond contre 596 s sans (×0,21, pour un rapport de
  boucles de ×0,21) : le temps est bien proportionnel au nombre de boucles, comme le suppose le §8.1. Durées des deux passes :
  3 min 11 s et 13 min 44 s ; RSS maximal 14,1 et 15,7 Gio.

## 9. Réserves

- **A fait la statistique** (84 % des boucles) ; B est la seule autre équipe lisible, et en lecture « écart » sur 42
  boucles ROAD et 58 GRAVEL seulement (indicatif) : la décision par équipe repose sur la lecture « toutes » de B, comme le
  prévoit la décision 7 du §9.3.
- **La table commune du banc est contemporaine** : `rt` se replie, à chaque pas, vers la moyenne des **mêmes** jeux de
  toutes les équipes. Au dépôt, une équipe est recalculée seule : sa cible de repli devra être une table commune figée
  (la table générale `rc05-p3`, par exemple), ce qui n'a pas été joué tel quel.
- **Les chaînes apprennent contre leur propre générateur** (surcouche complète) ; la fuite de la surcouche et le
  surapprentissage restent sous 0,06 en valeur absolue (§4).
- **Temps** : machine pleine (16 fils sur 8 cœurs) ; les rapports entre variantes sont lisibles, pas les valeurs absolues.
  Le coût au dépôt est un calcul sur ces temps, pas une mesure sur le serveur.
- **Tables non normalisées** : la dérive d'échelle de A (0,89) n'est pas isolée de l'effet des classes.

## 10. Ce que la seconde sonde décide

Lecture des décisions 6 à 9 du §9.3.

1. **Critère GRAVEL atteint** (décision 6, lecture « toutes ») [mesuré] : **0,41 · 0,33 au pas 2 et 0,35 · 0,31 au pas 3**
   pour la table commune (campagne 1, 10 % à l'écart) ; 0,41 · 0,32 puis 0,38 · 0,30 en campagne 2 ; **0,37 · 0,26** pour
   `rt05-p3`. Bien sous 0,5, loin de la zone 0,50 à 0,55. **Contrôle de surapprentissage** : « écart » − « toutes » au pas 3,
   toutes équipes, ±0,04 au plus pour toutes les chaînes (seuil 0,10) : pas de surapprentissage, le pas 3 se garde. Le ROAD
   reste sous 0,5 à tous les pas ≥ 2 (0,17 à 0,29).
2. **Profil par équipe** (décision 7) : les trois conditions sont remplies au pas 3, sans familiarité [mesuré] :
   - B ROAD avec `rt05-p3`, lecture « toutes » : **0,80 · 0,93**, sous 1,0 aux deux curseurs (« écart », indicative :
     0,85 · 0,90) ;
   - `rc05-p3` y reste au-dessus de 1,2 : **1,78 · 2,14** ;
   - A ne perd pas plus de 0,03 : `rt − rc` **+0,021 · +0,027** en ROAD (« écart » −0,008 · +0,023), et gagne 0,12 à 0,14
     en GRAVEL.

   **Le profil par équipe est retenu** : `rt`, 72 cases, γ = 0,5, 3 pas, `n0` = 25. Au pas 2, la condition sur A échouerait
   (+0,040 au curseur 1) : le troisième pas est nécessaire, pas seulement utile. Avec `famroad`, B ROAD au curseur 0,25 est à
   1,05 au pas 3 : la condition échoue.
3. **Pas et γ** : **3 pas, γ = 0,5, amortissement au changement de signe, borne de pas `[1/3, 3]`, borne cumulée
   `[1/4, 4]`, 72 cases** (les effets principaux font +0,042 en GRAVEL 1 « écart », et P50 ×1,41 au lieu de ×1,16 au pas 3). L'écart L1
   décroît encore au pas 3, de 20 à 27 % : un quatrième pas n'est pas justifié par le critère.
4. **Familiarité** : **rester à 0,5 partout**. Avec trois pas, `famroad` n'apporte plus rien toutes équipes (0,25 à 0,26
   contre 0,24 à 0,26 en ROAD 0,25 au pas 3) et remonte B ROAD au-dessus de 1,0. La décision 3 du §9.2 (familiarité 1 en
   ROAD au curseur 0,25) est à lever : elle valait pour le recalé à un pas.
5. **Coût** (décision 8) : A, la plus grosse équipe, tient dans 30 min sur 4 fils **sans allègement** au temps du banc (29,7
   min), et en ~6 min avec le plafond de 500 boucles [calculé]. Le plafond coûte à A +0,04 à +0,06 en ROAD (lecture
   « écart », pas 3), tout restant sous 0,25 ; GRAVEL non lisible sur 40 boucles (§8.2) : **plafond de 500 boucles
   recommandé** pour la marge (12 cœurs partagés) ; les deux curseurs sont gardés, l'allègement « un seul curseur »
   n'est pas nécessaire. La mémoire dans un conteneur de 3 Gio n'est pas
   mesurée.
6. **Service** (décision 9) : `rt05-p3` fait P50 ×1,17 · ×1,04, part `OK` −0,1 et −0,3 point, ni `PANIC` ni
   `WINDOW_ESCAPE` : critères tenus. Signalé : multiplicateur moyen de la table de A en ROAD à **0,891**, hors de 1 ± 0,10
   (et B 1,100 au bord) ; P50 de `rt05-p1` ×1,25 · ×1,28 et de `rc05-me-p3` ×1,41 · ×1,32 (variantes non retenues).
7. **Ce qui reste pour le 4c** : toute la forme. r / D ROAD 0,40 (réel 0,33, plus loin que `base` 0,39), détour ROAD 1,20
   (réel 1,49) ; GRAVEL r / D 0,36 (réel 0,285), détour 1,35 (réel 1,72), virages 20,5 aux 10 km (réel 27,6). En classes, il
   reste les chemins et le non revêtu GRAVEL (0,37 à 0,41 et 0,17 à 0,20 contre 0,447 et 0,253).

### 10.1 Ce qui change dans la note (proposé, non appliqué)

- **§3.2** : le goût est un **profil par équipe appris au dépôt par regénération** : une table de 72 classes × 3 types,
  trois pas de recalé (réel contre généré, γ = 0,5, amortissement au changement de signe, bornes `[1/3, 3]` par pas et
  `[1/4, 4]` cumulée), mélangée classe par classe à une table commune avec le poids `G_T / (G_T + n0 · G_P / N_P)`,
  `n0` = 25. La **table commune** devient une constante du moteur (la table `rc05-p3` du banc, apprise sur toutes les
  équipes) : elle sert de repli au dépôt et de goût aux requêtes sans équipe ou d'une équipe sans profil. Elle s'applique à
  toutes les arêtes de la vue, aux deux curseurs.
- **§3.2, familiarité** : reste 0,5 par type ; le paramètre « par type » proposé après la première sonde devient inutile.
- **§3.5** : le fichier de profil `.ordp.enc` porte la table de 72 × 3 `u16` de l'équipe (plus formes et destinations au
  4c et au 4d), calculée en fin de dépôt ; au plus 500 boucles par équipe (§8.2) ; recalcul de nuit ou
  hebdomadaire.
- **§10** :
  - **3a** (seconde sonde) : fait, ce relevé ;
  - **3b** : profil par regénération au dépôt et table commune dans le moteur ; à y trancher : la normalisation des tables
    (A à 0,89), la cible de repli figée au lieu de contemporaine (§9), la mémoire dans le conteneur de 3 Gio, le plafond de
    boucles ; références figées et tests dorés re-bénis, version du moteur montée ;
  - **3c** (familiarité par type) : **supprimée** ;
  - le 4c (triangle et couloir) suit 3b, ses distributions rejouées avec le profil.
