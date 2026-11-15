# Générateur de boucles sur graphe OSM — lot 2 : campagne de parité, réglage, relevé

> Relevé du 15 novembre 2026, sur le moteur `crates/ord-loops`, le contrôleur indépendant `crates/ord-loops-check` et le banc
> `tools/ord-loops-bench` (conception : [ord-loops-lot2.md](ord-loops-lot2.md), §8 pour le protocole). Tous les chiffres
> viennent des rapports du banc sous `~/.cache/osm-route-details/graph/lot2/` (`camp1-…`, `camp2-…`, `camp3-…`) ou d'une sonde
> jetable hors dépôt ; ils sont **mesurés** sauf mention contraire. Équipes lettrées A à I par km dans la région, comme au
> relevé du lot 1 ; ni départ, ni identifiant d'équipe, de trace ou d'arête ici.

## 1. Protocole

- **Requêtes** : celles du banc Java `bench-1115-head` des équipes dont le départ est dans la région, rejouées telles quelles
  (départ, arrivée d'un aller simple, cap, D, type, familiarité ; graine = indice de la requête). 323 requêtes, 197
  **intérieures** (disque D/2 dans la région) et 126 **de bord** ; équipes A à G (H et I n'ont aucun départ dans la région).
  Chacune est jouée dans les **trois modes** : 969 requêtes × modes par tour de campagne.
- **Réussite** : au moins un parcours rendu **et** accepté par le contrôleur indépendant. Chaque parcours rendu passe le
  contrôleur (§6 de la conception), qui ne lit ni le code de recherche ni le contrôle interne du moteur.
- **Temps** : horloge murale autour d'un appel `generate_with_diagnostics`, un fil ; un tour « froid » (premier appel de la
  requête) puis **3 tours chauds**, la médiane des tours chauds est le temps rapporté.
  - **Dans le minuteur** : accroche du départ et de l'arrivée, arbres aller et retour, toutes les recherches ciblées, l'ordre
    inverse, le contrôle interne de chaque candidat, le choix des trois parcours, les passes de virages interdits.
  - **Hors du minuteur** : `Engine::new` (1,1 s par processus, fichiers en mmap), `TeamData::build` (0,09 à 0,17 s par équipe),
    le contrôleur indépendant, l'écriture des rapports et des GeoJSON. Le `Workspace` est réutilisé d'une requête à l'autre :
    le tour « froid » a déjà ses tableaux alloués et le cache de pages chaud ; ce n'est pas un premier appel de processus.
- **Machine** : AMD Ryzen 7 4800H, 16 fils, `rustc 1.97.0`, profil `release`. **Charge** : session de bureau ouverte (client git
  et shell graphique actifs, 94 % de processeur libre au relevé `top` avant le premier tour) ; charge moyenne sur 1 minute de
  1,2 à 1,9 pendant les trois tours (`camp1` 1,47 → 1,89 ; `camp2` 1,20 → 1,46 ; `camp3` 1,36 → 1,83). Le banc n'occupe qu'un fil
  sur 16 ; la machine n'était pas au repos complet, aucun travail lourd ne tournait.
- **Tours de campagne** (trois, le maximum fixé) :

| Tour | Code | Durée |
|---|---|---|
| `camp1` | moteur tel que reçu (état « fixfinal2 » de la note de conception §14) | 772 s |
| `camp2` | + accroche sur composante fortement connexe, + demi-tours écartés sur le cycle d'un candidat écarté, + 6 essais par secteur (§5) | 825 s |
| `camp3` | `camp2` + deux diagnostics sans effet sur la recherche ; c'est le **relevé final** | 825 s |

`camp2` et `camp3` donnent la **même issue, le même nombre de parcours et les mêmes longueurs** pour les 969 requêtes × modes : le
moteur est déterministe d'une campagne à l'autre.

## 2. Parité en strict, par équipe (relevé final `camp3`)

Réussites sur les requêtes du périmètre ; entre parenthèses, les intérieures.

| Équipe | requêtes | Java `head` | Rust strict | Rust équipe d'abord | Rust découverte |
|---|---|---|---|---|---|
| A | 120 (70) | 53 (37) | **62 (49)** | 114 (65) | 113 (65) |
| B | 60 (13) | 22 (7) | **27 (10)** | 47 (10) | 49 (10) |
| C | 31 (21) | 0 (0) | 0 (0) | 30 (20) | 31 (21) |
| D | 31 (27) | 4 (4) | **2 (2)** | 31 (27) | 31 (27) |
| E | 77 (62) | 6 (6) | **0 (0)** | 73 (58) | 77 (62) |
| F | 2 (2) | 0 (0) | 0 (0) | 2 (2) | 2 (2) |
| G | 2 (2) | 0 (0) | 0 (0) | 2 (2) | 2 (2) |
| **total** | **323 (197)** | **85 (54)** | **91 (61)** | **299 (184)** | **305 (189)** |

Avant réglage (`camp1`) : strict 78 (56), équipe d'abord 290 (180), découverte 302 (186).

Critères du §8.4 de la conception :

1. **Réussites Rust ≥ Java pour chaque équipe et au total, et sur les intérieures** : au total **tenu** (91 ≥ 85), sur les
   intérieures **tenu** (61 ≥ 54), pour A, B, C, F, G **tenu**, pour **D (2 < 4) et E (0 < 6) non tenu**. Contre `head-alone`,
   plus exigeant (257 requêtes du périmètre jouées) : Java 87, Rust 89. **Critère (1) : non tenu.**
2. **Zéro violation** : **0** violation du contrôleur indépendant sur **1 811** parcours rendus (218 strict, 742 équipe
   d'abord, 851 découverte), 0 parcours accepté par le contrôle interne et refusé par le contrôleur, et 0 écart moteur / contrôleur
   sur la longueur (0,5 % de D), les croisements, la longueur recouverte (5 m) et la part d'équipe (1 % de D). **Tenu.**
3. **Médiane < 500 ms pour D ≤ 100 km**, intérieures, sur les générations réussies : strict **67 ms** (p90 175, max 234, 59
   réussites) ; toutes les requêtes : 2 / 94 / 244 ms. **Tenu** (détail au §4).

Lecture plus fine, avec ce que le banc Java dit de ses propres parcours : chaque parcours Java porte les violations de son
« oracle » (contrôle indépendant du banc Java, plus strict que ses garanties). Une réussite Java dont **tous** les parcours sont
signalés par cet oracle n'est pas une réussite propre. Réussites Java avec au moins un parcours sans signalement : A 42, B 20, D 4,
E **0**, total **66**. Contre ce compte, Rust strict n'est en dessous que pour **D** (2 contre 4) ; les six réussites Java de E
font toutes, d'après l'oracle Java, un virage de 150° ou plus sur un carrefour.

## 3. Ce qui échoue encore

### 3.1 Strict : les 23 requêtes réussies par Java et pas par Rust

Contre 29 en `camp1` (15 intérieures, 14 de bord), rien de perdu par le réglage (aucune requête réussie en `camp1` n'est ratée en
`camp2`). Classées requête par requête avec les diagnostics du banc (réseau atteint, chemin de retour, L / D des candidats hors
tolérance, nœuds des virages refusés) et la sonde :

| Classe | Requêtes | Preuve |
|---|---|---|
| **règle de virage plus stricte que Java** | D 92, 112, 114, 119 (intérieures, 50 km) | 10 à 20 candidats dans la tolérance, tous refusés `SHARP_TURN` ; les refus tombent sur deux groupes de carrefours, à 0,4 km et 3,4 km du départ, que tout parcours sortant traverse ; les quatre parcours Java sont propres pour l'oracle Java. Java exempte un virage à un nœud de degré d'équipe 2 traversé par une même trace (`RouteGuarantees.turnApplies`, `riddenThrough`) ; la surcouche ne garde pas l'identité des traces, et une partie de ces nœuds a ici un degré d'équipe 3 parce qu'un morceau comblé (§2.3) s'y ajoute. |
| **parcours Java non propre** (virage ≥ 150° sur un carrefour, signalé par l'oracle Java) | E 2, 8, 9, 33, 42, 59 (intérieures, 110 km) ; A 23 (aller simple) | E : réseau atteint 192 km, 123 km ramènent au départ, candidats hors tolérance ; les six réussites Java sont toutes signalées (virage de 150° et plus ; pour E 2, deux signalements vers 25 km du départ). A 23 : `START_TOO_FAR` (aucune composante stricte fortement connexe de 0,9 × D à 2 km) ; son parcours Java est signalé. |
| **bord : la région coupe le réseau de l'équipe** | A 2, 49, 61, 87, 100, 109 (un même départ, 80 à 150 km) ; A 105 (aller simple) | réseau strict atteint 155 à 168 km, ou `START_TOO_FAR`, quand Java comptait 1 268 à 7 287 km de réseau autour du même départ (champ `networkNearKm` du banc Java) : ses traces sortent de la région. |
| **recherche, au bord** | A 66 (110 km), A 102, A 115 (150 km), B 43 (80 km) | réseau atteint du même ordre que celui de Java (6 257 à 8 995 km contre 6 655 à 9 823 ; B : 1 625 contre 1 790) ; A 66 : ses 2 candidats à 0,70 × D ; A 102 et A 115 : 48 retours cherchés chacune, aucun trouvé (demi-tours au sud, vers la limite de la région) ; B 43 : candidats de 0,25 à 0,90 × D. Probablement le bord (demi-tours sur la frange coupée), non prouvé. |
| **recherche, intérieure** | A 67 (30 km) | réseau strict de 404 km ; 8 candidats, tous à 0,69 ou 1,5 à 1,65 × D : le réseau strict n'offre au point de demi-tour que deux cycles, le parcours Java (29,5 km, propre) passe ailleurs. Seule perte intérieure qui relève de la recherche (famille polygone absente). |

Aucune n'est un « trou non comblé » ni une « arête roulée hors vue » : les km d'équipe hors de la vue du type (§3.4) sont de 13,8 km
pour D et 3,6 km pour E. Au sens du §8.4, une seule perte intérieure est un défaut de recherche (A 67), et quatre de bord le sont
peut-être ; les quatre de D tiennent à une **règle**, pas à la recherche.

### 3.2 Strict : toutes les causes d'échec

232 échecs sur 323 (Java réussissait 23 d'entre eux, §3.1) :

| Issue | intérieures | de bord | dont réussies par Java |
|---|---|---|---|
| `NO_ROUTE:NO_CANDIDATE` | 68 | 71 | 16 |
| `NETWORK_TOO_SMALL` | 21 | 10 | 0 |
| `NO_ROUTE:NO_WAY_BACK` | 30 | 0 | 0 |
| `START_TOO_FAR` | 14 | 14 | 6 |
| `NO_ROUTE:BUDGET` | 3 | 0 | 0 |
| `NO_ROUTE:END_UNREACHABLE` | 0 | 1 | 1 |

L'accroche sur composante fortement connexe (§5) a déplacé des échecs de `NO_WAY_BACK` (100 en `camp1`) et `NETWORK_TOO_SMALL`
(41) vers `START_TOO_FAR` (0 → 28) : un départ dont aucune composante stricte fortement connexe de 0,9 × D n'est à moins de 2 km
échoue d'emblée, ce qui est la même impossibilité dite plus tôt.

### 3.3 Équipe d'abord et découverte

Équipe d'abord : 24 échecs (23 `NO_CANDIDATE`, 1 `BUDGET`), dont 1 réussi par Java (B 77, de bord, que le **strict réussit** :
l'équipe d'abord ne retombe pas sur le parcours strict quand sa propre recherche échoue). Classés par le L / D des candidats :

| | intérieures | de bord |
|---|---|---|
| candidats dans la tolérance tous refusés par le contrôle interne (surtout `SHARP_TURN`, puis `REUSE_OUTSIDE_TAIL`) | 9 | 3 |
| tous les candidats trop courts (le réseau du disque est coupé par la région, ou un retour disjoint ne se trouve qu'en court) | 4 | 8 |

Découverte : 18 échecs, 4 intérieurs refusés au contrôle, 14 trop courts (dont 10 de bord).

### 3.4 Arêtes roulées hors de la vue du type

Km de base roulés par l'équipe hors de la vue du type demandé, par un parcours du type (et tous types), contre les km d'équipe
de la vue : route A 899 (2 125) sur 11 200 km, B 293 (634) sur 1 956, C 129 (327) sur 1 589, D 13,8 sur 2 004, E 3,6 sur 1 420,
F et G 0 ; gravel A 202 (275) sur 12 908, B 88 (98) sur 2 478, C 20 (23) sur 1 888, D, E, F, G 0 (au plus 2,3 tous types). La décision
de la conception (§2.4 : admises dans aucun mode) est gardée : elle ne coûte aucune des pertes du §3.1.

## 4. Temps

Médiane des 3 tours chauds, ms, `camp3` : `n réussies : médiane / p90 / max`, puis toutes les requêtes de la case.

| Mode | Case | réussies | toutes |
|---|---|---|---|
| strict | route 30 km | 26 : 38 / 106 / 175 | 67 : 2 / 69 / 175 |
| strict | route 50 km | 17 : 45 / 105 / 128 | 58 : 2 / 61 / 185 |
| strict | route 80 km | 21 : 74 / 208 / 234 | 56 : 11 / 108 / 244 |
| strict | route 110 km | 5 : 146 / 272 / 272 | 51 : 1 / 104 / 272 |
| strict | route 150 km | 4 : 172 / 234 / 234 | 51 : 1 / 49 / 234 |
| strict | gravel et VTT 50 km | 12 : 161 / 197 / 204 | 28 : 20 / 172 / 204 |
| strict | aller simple 53 à 82 km | 6 : 90 / 112 / 112 | 8 : 89 / 94 / 112 |
| équipe d'abord | route 30 km | 60 : 95 / 262 / 395 | 67 : 95 / 301 / 447 |
| équipe d'abord | route 50 km | 54 : 170 / 418 / 682 | 58 : 167 / 393 / 682 |
| équipe d'abord | route 80 km | 51 : 251 / 590 / 869 | 56 : 251 / 614 / 1 131 |
| équipe d'abord | route 110 km | 45 : 503 / 998 / 1 340 | 51 : 497 / 998 / 1 747 |
| équipe d'abord | route 150 km | 49 : 782 / 1 197 / 1 969 | 51 : 770 / 1 197 / 1 969 |
| équipe d'abord | gravel et VTT 50 km | 28 : 225 / 286 / 632 | 28 : 225 / 286 / 632 |
| équipe d'abord | aller simple | 8 : 201 / 250 / 350 | 8 : 201 / 250 / 350 |
| découverte | route 30 km | 63 : 66 / 132 / 236 | 67 : 65 / 132 / 236 |
| découverte | route 50 km | 54 : 110 / 175 / 191 | 58 : 106 / 174 / 191 |
| découverte | route 80 km | 51 : 225 / 322 / 465 | 56 : 179 / 329 / 465 |
| découverte | route 110 km | 48 : 331 / 483 / 737 | 51 : 318 / 491 / 737 |
| découverte | route 150 km | 49 : 551 / 987 / 1 136 | 51 : 536 / 987 / 1 136 |

(quatre requêtes gravel et VTT de 10 km, réussies en équipe d'abord et en découverte en 49 à 53 ms, sont omises.)

Contre la cible (médiane < 500 ms jusqu'à 100 km), D ≤ 100 km, générations réussies, médiane / p90 / max :

| Mode | toutes portées | intérieures |
|---|---|---|
| strict | 56 / 161 / 234 (82) | 67 / 175 / 234 (59) |
| équipe d'abord | 163 / 442 / 869 (205) | 183 / 470 / 1 131 sur toutes les requêtes intérieures (172) |
| découverte | 118 / 273 / 465 (208) | 128 / 284 / 465 sur toutes les requêtes intérieures (172) |

**La cible tient en médiane dans les trois modes.** Elle ne tient pas au p90 ni au maximum de l'équipe d'abord à 80 km (p90 590,
max 869 ms) et la médiane de l'équipe d'abord la dépasse à 110 km (503 ms) et 150 km (782 ms), hors cible. Le réglage a coûté du
temps : sur les requêtes réussies avant et après, rapport `camp2` / `camp1` médian 1,13 (strict) à 1,15 (équipe d'abord), p90
1,38 à 1,43 ; le p90 de l'équipe d'abord sur toutes les requêtes intérieures D ≤ 100 km passe de 428 à 470 ms.

## 5. Réglage fait (tous les changements de code)

Le contrôleur, les garanties, la définition de la réussite et la règle de comblement des trous n'ont pas été touchés.

1. **Accroche sur composante fortement connexe** (`engine.rs` : `strong_components`, Tarjan itératif ; `strong_lengths` ;
   `chain_strong_len` ; `ViewData::comp_len_of_chain` ; `team.rs` : composantes du réseau strict). La composante qui doit faire
   0,9 × D pour qu'une chaîne serve d'accroche est maintenant **fortement connexe** (sens respectés, virages ignorés) et une chaîne
   n'y compte que si ses deux bouts y sont ; avant, elle était calculée sens ignorés (conception §1.2, comme le `leadingOn` Java).
   Défaut révélé : un départ de A est au bout d'une rue à sens unique roulée une fois, qui sort du réseau de l'équipe ; sens
   ignorés, elle tenait au grand réseau, et tout départ de là s'y accrochait pour n'atteindre que 0,1 km (`NETWORK_TOO_SMALL`
   ou `END_UNREACHABLE`, Java réussissait A 13, 53, 93). L'accroche prend maintenant une arête roulée à 120 m. Tests :
   `strict_does_not_snap_onto_a_one_way_team_street_that_leads_off_the_team_network`,
   `a_start_in_a_one_way_pocket_snaps_onto_the_strongly_connected_network` (remplace `a_one_way_pocket_has_no_way_back`, dont
   l'attente était l'ancien comportement). Effet : +9 en strict (A 13, 42, 53, 55, 57, 73, 88, 93, 103, toutes à départ déplacé).
   Chargement du moteur : 1,0 → 1,1 s.
2. **Demi-tours écartés sur le cycle d'un candidat écarté** (`generate.rs`) : dans un secteur, après un candidat hors tolérance ou
   refusé par le contrôle, tous les nœuds de son parcours sortent des demi-tours du secteur. Défaut révélé : sur les réseaux
   stricts, un demi-tour pris sur le même cycle redonnait le même parcours, de même longueur, quel que soit ρ (sonde : 5 à 18
   candidats identiques, à 0,70 ou 0,90 × D, pour A 66, B 43, 77, 89). Test :
   `a_sector_does_not_retry_turnarounds_on_a_loop_it_has_just_rejected` (4 candidats, 8 sans la règle).
3. **Six essais par secteur au lieu de trois** (`Params::tries_per_sector`, nouveau paramètre ; même test). Avec le point 2 :
   +4 en strict (A 27, B 18, 77, 89), +10 et −1 en équipe d'abord, +3 en découverte, au coût de temps du §4. Les effets des
   points 2 et 3 ne sont pas séparés au banc complet (sonde sur 8 requêtes : la règle 2 avec 3 essais rattrape B 77 et 89 ; 6 essais sans la règle 2 rattrapent B 18 ; les deux ensemble, les trois).
4. **Diagnostics** (sans effet sur la recherche) : `Diagnostics::off_tolerance` (L / D des candidats hors tolérance),
   `Diagnostics::sharp_nodes` (nœuds de base des virages refusés), `Rec::off_tolerance` dans le rapport du banc.

Essayé et écarté avant cette campagne (note de conception §14.3) : interdire toute la fenêtre d'un groupe de virages refusés.

## 6. Ce que « équipe d'abord » apporte

Sur les **238 requêtes que Java a ratées** : strict 29, **équipe d'abord 215**, découverte 221. Par équipe (ratées par Java →
réussies en équipe d'abord) : A 67 → 61, B 38 → 26, C 31 → 30, D 27 → 27, E 71 → 67, F 2 → 2, G 2 → 2. **Départs muets** (Java ne
rendait rien, quelle que soit la requête) : 5 départs (C 2, D 1, F 1, G 1), 51 requêtes ; strict 0, **équipe d'abord 50**,
découverte 51.

Ce que coûtent les raccords : km **hors équipe** (longueur moins arêtes d'équipe et comblées, comptés par le contrôleur) du
**premier** parcours proposé, équipe d'abord, 299 requêtes réussies : **médiane 11,3 km, p90 42,1 km**, max 102,9 km ; part
d'équipe médiane 0,81 (p10 0,51). Sur les trois propositions : médiane 13,6 km, p90 52,9 km. Par équipe, premier parcours,
médiane / p90 km hors équipe et part d'équipe médiane : A 4,2 / 31,7 (0,95), B 7,9 / 80,9 (0,90), C 25,0 / 36,0 (0,76), D 16,6 /
30,2 (0,68), E 16,6 / 42,6 (0,72), F 8,2 (0,47), G 7,7 (0,31) ; par type : route 12,6 / 43,2, gravel 7,8 / 19,6, VTT 8,6 / 26,9.
Plafond (découverte, sans préférence) : part d'équipe médiane 0,30, 38,1 km hors équipe en médiane. Le strict reste à 0 km hors
équipe (part 0,999, le reste étant comblé).

## 7. Ce qu'une personne verrait

- **Équipe A ou B, mode strict** : une boucle de la route connue dans 62 et 27 cas sur 120 et 60, plus souvent que Java ; la
  boucle part parfois d'une rue voisine quand la rue du départ est un sens unique qui ne ramène pas (accroche des réussites
  strictes : médiane 39 m, p90 54 m, max 120 m ; 2 km permis).
- **Équipes C à G, ou un départ où Java ne rendait rien** : en strict, presque toujours rien (réseau roulé trop petit ou sans
  cycle) ; en **équipe d'abord, presque toujours trois propositions**, dont la première emprunte en médiane 11 km de route que
  l'équipe n'a jamais roulée (p90 42 km) pour relier les morceaux de son réseau : la boucle est faite surtout de routes connues
  (part médiane 0,81), pas entièrement.
- **Attente** : une boucle de 50 km en équipe d'abord vient en 0,17 s en médiane, une de 80 km en 0,25 s (une sur dix au-delà
  de 0,59 s) ; à 110 et 150 km, 0,5 et 0,8 s en médiane.
- Aucun parcours rendu ne viole une garantie (demi-tour, impasse, réemprunt hors queue, croisements, distance ±10 %, et en strict
  arêtes d'équipe seulement) : un refus du moteur se voit comme « pas de parcours », jamais comme un parcours faux.

## 8. Verdict

- **Parité en strict : non atteinte** au sens du §8.4. Tenue au total (91 contre 85), sur les intérieures (61 contre 54), contre
  `head-alone` (89 contre 87), pour A et B, avec zéro violation sur 1 811 parcours et une médiane de 67 ms ; **non tenue pour D
  (2 contre 4) et E (0 contre 6)**. Les six de E sont des parcours Java que l'oracle Java lui-même signale (virage de 150° et
  plus) ; les quatre de D sont propres côté Java et tombent sur la règle de virage, plus stricte ici que `RouteGuarantees.java`
  (exemption `riddenThrough`). **Décision au propriétaire** : reprendre l'exemption Java demanderait l'identité des traces dans la
  surcouche (ou une approximation par les sens roulés, plus faible) ; la garantie n'a pas été élargie ici. Contre les réussites
  Java propres pour l'oracle Java (66), seule D reste en dessous.
- **Équipe d'abord** : 299 réussites sur 323 (Java 85), 215 des 238 requêtes ratées par Java et 50 des 51 requêtes des départs
  muets, sans violation, pour un raccord médian de 11 km hors équipe par boucle (p90 42 km).
- **Ce qui reste non prouvé** :
  - **une seule région** (Pays de la Loire), et les requêtes de bord y perdent ce que Java gagnait hors région ;
  - **une équipe domine les données** : A fait 120 des 323 requêtes et 11 200 des 18 400 km d'équipe en route de la région
    (61 %) ; C à G ne sont que 143 requêtes, dont E seule 77 sur un seul départ ;
  - **aucun utilisateur réel** : « réussite » veut dire « un parcours qui passe le contrôleur », pas « un parcours qu'une équipe
    roulerait » ; la comparaison avec Java porte sur l'issue des requêtes, pas sur les parcours eux-mêmes (leur géométrie n'est
    pas dans les résultats Java, le contrôleur n'a pas pu les relire) ;
  - **formes d'aujourd'hui** (point de demi-tour, ordre inverse ; ni polygone ni squelette organique) : le lot 4 reste à faire ;
  - la marge de temps de l'équipe d'abord au p90 (442 à 590 ms entre 50 et 80 km) et au-delà de 100 km ; les effets séparés des
    réglages 2 et 3 ; la cause exacte des refus de virage de D sur le groupe de carrefours à 3,4 km.

## 9. GeoJSON pour le propriétaire

`~/.cache/osm-route-details/graph/lot2/geojson/` (**privé**, hors dépôt), 75 fichiers `<lettre>-<indice>-<mode>.geojson` : 25
requêtes × 3 modes, 131 candidats acceptés (strict 15, équipe d'abord 54, découverte 62), avec le départ, l'arrivée d'un aller
simple, chaque candidat (longueur, rapport à D, part d'équipe, violations) et les arêtes d'équipe du disque. Choisis : les
départs rattrapés par l'accroche (A 13, 42, 53, 93), les recherches rattrapées (B 18, 77), les pertes de D (92, 114), de E (2) et
A 67, et deux ou trois requêtes ratées par Java et réussies en équipe d'abord pour chaque équipe A à G. Les GeoJSON des tours
précédents sont dans `geojson-before-camp3/`. Rapports complets : `camp3-report.json`, `camp3-summary.md` (et `camp1-…`,
`camp2-…`) sous `~/.cache/osm-route-details/graph/lot2/`.
