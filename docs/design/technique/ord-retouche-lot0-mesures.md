# Retouche d'un parcours — lot 0 : relevé des campagnes complètes

> Relevé du 22 novembre 2026 sur `develop` à `7245aaf` (les livraisons A et B du lot 0 et leur relecture), binaire `release`,
> Rust 1.100.0, poste de développement (16 fils sur 8 cœurs physiques, 30 Go). Graphe et vues France `france-20261115-z11`,
> surcouches `lot6/overlays`. Conception : [`ord-retouche-lot0.md`](ord-retouche-lot0.md) (critères au §9, écarts aux §12 et §13) ;
> plan appelant : `docs/plans/2026-11-22-retouche-parcours.md` de Pédalons (ledger `API-212`).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/retouche/` (préfixes `full…`, journaux
> `full-*.log`). Lettres d'équipes et agrégats seulement ici : ni départ, ni coordonnée, ni identifiant d'équipe, de trace ou
> d'arête. Rien n'a été poussé ni déployé. Aucune ligne du moteur ni de ses tests n'a changé pour ce relevé.
>
> Validé le même jour : K1, l'ordre, K2 et ses classes de refus, K3 brut, pondéré et à ±1, 2, 3 %, et K4 recalculés depuis
> les jsonl privés ; un dénominateur de K4 corrigé (§4.3), une formulation du §2.3 précisée, le dénominateur de K2 explicité (§3.1).
>
> Marques : **[mesuré]** vient des sorties du banc ; **[calculé]** d'un calcul fait ici sur ces sorties (relecture des jsonl privés
> ou arithmétique sur les agrégats) ; **[supposé]** n'est pas vérifié. Sans marque dans un tableau : mesuré. Les temps sont pris à
> 16 fils simultanés, sauf mention : pessimistes pour le service.

## 0. En bref

- **Aucun des quatre critères n'est tenu.** K1 (couture à 1 km) **non tenu** : 54,2 % [52,8 ; 55,6]. K2 (`move_end` sans refus)
  **non tenu** : 79,5 % [79,2 ; 79,7]. K3 (±3 % pour |δ| ≤ 25 %) **à la limite** au total pondéré (88,9 % [87,4 ; 90,3] pour 90 %),
  **non tenu** au total brut (80,1 % [79,1 ; 81,0]). K4 : `move_end` **tenu** (P90 127 ms), `resize` **non tenu** (P90 4 060 ms
  pour 2 000). Au sens du §9 de la note (K1 et K2 ouvrent le lot 1, K3 et K4 le lot 2), aucun des deux lots ne s'ouvre ;
  la conclusion (§9) dit ce que les chiffres autorisent malgré tout.
- **K1 ne se corrige ni par la marge, ni par `Touch`, ni par les trous.** La marge de 200 m ramenée à 50 m et `Touch` tenu pour
  continu font passer K1 de 54,2 à 70,8 %, pas plus. La distance maximale à la couture a un P50 de 830 m **même à 50 m de marge avec `Touch` continu**
  (aucune variante sans marge n'a été jouée) ; son P95 est à 2,6 km à 200 m [calculé]. Les trous de l'appariement sont minuscules (corde de P50 32 m). Ce qui reste, ce sont de longs espaces entre
  deux coutures possibles, que le banc n'a pas décomposés par règle (§2.3).
- **K2 : 89 % des refus ne tiennent qu'à un `SHARP_TURN` du contrôleur indépendant sur le recollé (C2)** [calculé] : 14 435
  des 16 209 refus d'essais joués. Sans eux, K2 serait de **96,3 %** [calculé, contrefactuel, non mesuré]. Le moteur ne connaît pas
  le cap d'arrivée de la jambe gardée (prévu au §11 de la note) : ce n'est pas un virage de plus de 150° à la couture, car 93 %
  des refusés ont un angle de couture sous 120° [calculé].
- **K3 : le raccourcissement est le point faible, l'allongement est bon.** +10, +25, +35 % : 97,4 · 97,5 · 96,5 %. −10, −25, −35 % :
  66,9 · 58,4 · 52,8 %. Les aller simples tiennent mal (70,0 % sur |δ| ≤ 25 %) contre 89,5 % pour les boucles. Il n'y a **pas de falaise
  à ±30 %** : le succès décroît régulièrement en raccourcissant.
- **K4 : le temps vient de l'allongement, pas du raccourcissement.** P90 par δ : −35 % 371 ms, −10 % 561 ms, +10 % 2,3 s, +25 % 10,7 s.
  Les traces de plus de 200 km (P90 18 s) dominent la queue.
- **Ce qu'apporte le masque des arêtes gardées** (`--no-keep-out`, `resize`) : K3 pondéré 83,1 % sans lui contre 88,9 % avec, et surtout
  le rendement des contrôles : 19,5 % des demandes atteignent le plafond de contrôles sans masque, 1,2 % avec.

## 1. Les campagnes

Chacune a été lancée en arrière-plan, journal sous `…/retouche/full-<nom>.log`, suivie à son compte rendu de progression. Toutes ont
terminé sans coupure (`--max-minutes` 60 jamais atteint). Les durées de `retouche-play` sont **plus longues que l'estimation du §7.4
de la note** (15 à 20 min) : 29 min au lieu de 15 à 20, parce que l'échantillon de `resize` compte **587 traces et 7 044 demandes**
(estimé : environ 300 traces, 3 600 demandes) et `move_end` **87 792 essais** (estimé : 80 000). Pic anonyme observé : 18,3 Go
(estimé 14 Go) pour la campagne de base, 20,5 Go sans masque ; la marge de 30 Go a tenu. La portée n'a pas été réduite.

| Campagne | Commande (binaire `./target/release/ord-loops-bench`) | Durée | Empreintes |
|---|---|---|---|
| continuité (marge 200 m) | `retouche-continuity --label fullc` | 17 s | jsonl `2120e0c1463a86a7` |
| continuité, `Touch` continue | `retouche-continuity --label fullct --touch-continues` | 17 s | jsonl `4fcfbb78c4ed6c60` |
| continuité, marge 100 m | `retouche-continuity --label fullc100 --seam-margin-m 100` | 17 s | jsonl `4cf8199be7b3e318` |
| continuité, 100 m et `Touch` | `retouche-continuity --label fullctc100 --touch-continues --seam-margin-m 100` | 17 s | jsonl `9fcaba976274838a` |
| continuité, marge 50 m | `retouche-continuity --label fullc50 --seam-margin-m 50` | 17 s | jsonl `5e324ba5346eb637` |
| continuité, 50 m et `Touch` | `retouche-continuity --label fullctc50 --touch-continues --seam-margin-m 50` | 17 s | jsonl `5be2df796a8d8e92` |
| jeu complet (`move_end` et `resize`) | `retouche-play --label fullp` | 1 741 s | `move_end` `77c33e00d9b1f779`, `resize` `3abd855c083b1afb` |
| sans masque (`resize` seul) | `retouche-play --label fullnoko --no-keep-out --op resize` | 1 422 s | `resize` `f1afdc90c75886c2` |
| marge 100 m (jeu complet) | `retouche-play --label fullm100 --seam-margin-m 100` | 1 725 s | `move_end` `afc9750420ab2b25`, `resize` `4151e2f74b43aafc` |
| recalage à 1 fil | `retouche-play --label fullt1 --threads 1 --op resize --teams B --limit 8` | 62 s | `resize` `18c0cfb5cb2d029f` |

Tous les jeux lisent le même export, d'empreinte xxh3 **`ca53159cc90498d6`** (3 377 lignes). Les variantes demandées par la note sont `fullct`, `fullc100`, `fullnoko`,
`fullm100` et `fullt1` ; `fullctc100`, `fullc50` et `fullctc50` sont ajoutées ici, à 17 s pièce, pour séparer la marge de ce qui reste. Le recalage à 1 fil ne porte que sur 8 traces de l'équipe B (96 demandes) : ce n'est **pas** un P90 de la population (§6.3).
Pas de campagne de déterminisme refaite : celle de la livraison B (`--threads 1` contre `--threads 16`, mêmes empreintes) tient ;
ici, les 48 demandes du recalage retrouvées dans la campagne de base ont **toutes** la même empreinte de recollé [calculé].

## 2. Livraison A : continuité et coutures

### 2.1 Population [mesuré]

| groupe | lignes | illisibles | vides | km des vides | mesurées | dont exclues |
|---|---|---|---|---|---|---|
| équipe A | 2 726 | 0 | 0 | 0 | 2 726 | 0 |
| équipe B | 446 | 0 | 0 | 0 | 446 | 0 |
| autres équipes | 205 | 0 | 22 | 3 324 | 183 | 0 |
| ROAD | 2 451 | 0 | 21 | 2 032 | 2 430 | 0 |
| GRAVEL | 913 | 0 | 0 | 0 | 913 | 0 |
| MTB | 13 | 0 | 1 | 1 292 | 12 | 0 |
| **total** | **3 377** | **0** | **22** | **3 324** | **3 355** | **0** |

22 traces vides, annoncées pour 3 324 km (1,5 % des kilomètres annoncés [calculé : 3 324 / (216 044 + 3 324)]) : la retouche ne peut rien pour
elles, et K1 les écarte de son dénominateur (D1 de la note). Aucune ligne illisible, aucune exclue.

### 2.2 K1 : une couture à 1 km ou moins de tout point, pour 95 % des parcours

Dénominateur : les 3 355 traces ROAD, GRAVEL et MTB mesurées, exclues comprises.

| groupe | traces | ≤ 500 m | ≤ 1 km | ≤ 2 km | ≤ 1 km sur [5 %, 95 %] | dernière couture ≤ 1 km de l'arrivée |
|---|---|---|---|---|---|---|
| **toutes** | 3 355 | 1,4 % | **54,2 % [52,8 ; 55,6]** | 90,8 % | 62,1 % | 98,7 % |
| ROAD | 2 430 | 1,4 % | 54,4 % [52,7 ; 56,0] | 88,8 % | 60,6 % | 98,6 % |
| GRAVEL | 913 | 1,4 % | 53,7 % [50,9 ; 56,4] | 96,2 % | 65,9 % | 99,0 % |
| MTB (12) | 12 | 0,0 % | 66,7 % [43,1 ; 84,1] | 91,7 % | 91,7 % | 91,7 % |
| boucles retenues par le banc | 2 855 | 1,3 % | 58,6 % [57,1 ; 60,1] | 94,2 % | 67,1 % | 99,2 % |
| ROAD boucles | 2 151 | 1,3 % | 58,0 % [56,2 ; 59,7] | 92,4 % | 64,9 % | 99,1 % |
| ROAD aller simple | 279 | 2,2 % | **26,5 % [22,4 ; 31,1]** | 60,9 % | 27,6 % | 95,3 % |
| GRAVEL boucles | 823 | 1,0 % | 51,8 % [48,9 ; 54,6] | 96,0 % | 64,4 % | 99,1 % |
| GRAVEL aller simple | 90 | 5,6 % | 71,1 % [62,7 ; 78,3] | 97,8 % | 80,0 % | 97,8 % |
| équipe A | 2 726 | 1,3 % | 57,1 % [55,6 ; 58,7] | 93,3 % | 66,1 % | 98,9 % |
| équipe B | 446 | 1,8 % | 53,1 % [49,2 ; 57,0] | 93,0 % | 57,0 % | 99,6 % |
| autres équipes | 183 | 2,2 % | 13,7 % [10,0 ; 18,4] | 48,6 % | 16,4 % | 94,0 % |

**Verdict K1 : non tenu** (54,2 %, borne haute 55,6 %). Il l'est dans chaque groupe du tableau, même MTB (12 traces, borne haute 84,1 %) ;
il l'est surtout sur les aller simples ROAD (26,5 %) et sur les équipes autres que A et B (13,7 %).

Quantiles de la distance maximale à la couture, en m, sur les 3 353 traces non vides [calculé] :

| marge | `Touch` continue | P50 | P80 | P90 | P95 | P99 |
|---|---|---|---|---|---|---|
| 200 m | non | 960 | 1 490 | 1 879 | **2 594** | 4 863 |
| 200 m | oui | 908 | 1 420 | 1 811 | 2 515 | 4 629 |
| 100 m | non | 885 | 1 303 | 1 728 | 2 458 | 4 709 |
| 100 m | oui | 841 | 1 295 | 1 728 | 2 420 | 4 597 |
| 50 m | non | 833 | 1 272 | 1 702 | 2 344 | 4 665 |
| 50 m | oui | 830 | 1 261 | 1 695 | 2 342 | 4 597 |

Part des traces à 1 km ou moins, selon la variante (K1, toutes traces) :

| marge | `Touch` coupe (défaut) | `Touch` continue |
|---|---|---|
| 200 m | 54,2 % [52,8 ; 55,6] | 60,4 % [59,1 ; 61,8] |
| 100 m | 66,3 % [65,0 ; 67,6] | 68,3 % [67,0 ; 69,7] |
| 50 m | 70,3 % [68,9 ; 71,5] | 70,8 % [69,5 ; 72,1] |

Aucune variante n'est tenue. Le seuil de distance qui donnerait 95 % est de **2,6 km** à la marge de 200 m [calculé] : à 2,5 km,
94,5 % ; à 3 km, 96,4 % ; à 5 km, 99,0 % [calculé].

### 2.3 Quelle cause domine ? [mesuré et calculé]

La note demandait de dire si K1 manque à cause des trous ou de la marge de 200 m. Ni l'un ni l'autre :

- **Les trous sont petits.** Corde des trous par trace : P50 32 m, P90 223 m ; 10,8 trous aux 100 km en médiane. Par groupe de densité de
  trous, K1 va de 59,3 % (moins de 5 trous aux 100 km) à 39,4 % (plus de 20) [calculé] : la densité pèse, mais les 12 traces sans trou ni `Touch`
  ne sont à 1 km ou moins que 5 fois sur 12.
- **La marge a un effet réel mais borné** : passer de 200 à 100 m ajoute 12 points, de 100 à 50 m ajoute 4. Avec `Touch` continu et 50 m,
  70,8 % : il manque 24 points.
- **Les plages sont courtes** (P50 par trace 1 046 m, P10 22 m, P90 10,6 km) parce que `Touch` coupe : 15 plages et 6 trous en
  médiane. `Touch` continu gagne 6 points à 200 m, 2 à 100 m.
- **Ce qui reste** (distance maximale d'un P50 de 830 m même à 50 m de marge avec `Touch` continu, 396 coutures valides en médiane par trace, soit environ une tous
  les 130 m sur la longueur médiane de 52 km [calculé]) est de **longs espaces rares** : une trace a des coutures serrées presque partout
  et pourtant un espace de 1 à 2 km quelque part. Les règles 1 et 2 de la couture (le nœud est un nœud de la vue du type, et les arêtes des deux
  jambes sont dans cette vue) sont les suspectes **[supposé]** : une chaîne de vue sans carrefour, ou un passage hors de la vue du
  type, ne porte aucune couture. Le banc ne compte pas les coutures refusées par règle ; **le relevé ne tranche pas entre ces règles**.
  À instrumenter au lot 1 (un compteur par règle, trois lignes).

La conclusion de la note (§9) : « si K1 n'est pas tenu à cause des trous (et non de la marge), l'entrée doit être complétée par un
recalage géométrique ». Les chiffres disent que **ce n'est pas la marge** (K1 reste à 70,8 % à 50 m) **ni les trous** (cordes de
quelques dizaines de mètres) ; la cause est sur la vue ou la densité de nœuds, pas sur l'appariement.

### 2.4 Ordre des jambes : critère « moins de 1 % de traces avec anomalie d'ordre » **non tenu** [mesuré]

| groupe | traces avec anomalie d'ordre | dérive > 2 % |
|---|---|---|
| toutes | **5,0 % [4,5 ; 5,7]** | 0,3 % [0,2 ; 0,5] |
| ROAD boucles | 6,2 % [5,4 ; 7,1] | 0,0 % |
| ROAD aller simple | 2,9 % [1,6 ; 5,0] | 3,9 % |
| GRAVEL boucles | 3,0 % [2,2 ; 4,2] | 0,0 % |
| équipe A | 5,8 % | 0,0 % |
| équipe B | 1,6 % | 0,0 % |
| retenues par le banc | 5,2 % | 0,0 % |

L'essai à 37 traces donnait 0 % ; la campagne complète est cinq fois au-dessus du critère. Les 169 traces concernées ont **deux fois plus
de trous aux 100 km** que la médiane (21,0 contre 10,8 [calculé]) et une anomalie en médiane (maximum 346). La distance de projection
reste bonne (maximum du P50 par trace 0 m, maximum des maximums 73 m). K1 ne change pas selon l'anomalie (54,3 % sans, 53,8 % avec
[calculé]). **Conséquence pour le lot 1** : une trace avec anomalie d'ordre ne doit pas servir de parcours posé ; un rejet à
l'entrée en écarte 5 % de la population.

### 2.5 Ce que les filtres actuels du banc écartent [mesuré]

| cause | traces | km |
|---|---|---|
| `NotLoop` | 331 | 27 725 |
| `Hole` (> 1 km) | 126 | 11 203 |
| `Matched` (< 0,95) | 37 | 2 949 |
| `Short` | 6 | 13 |
| **écartées** | **500** | **41 890** [calculé] |

K1 à 1 km par cause : `NotLoop` 41,1 %, `Hole` 1,6 %, `Matched` 8,1 %, `Short` 5 sur 6, contre 58,6 % pour les retenues. Les 331 `NotLoop` (aller
simples et semi-boucles) restent à 41 % de K1 : le filtre du banc de ressemblance les écartait, la retouche d'un aller simple les demande.
27 % des `Matched` ont une dérive de projection de plus de 2 % : de grandes parts hors région (`outside_m`).

## 3. Livraison B : `move_end` (K2)

87 792 essais sur 3 354 traces éligibles, deux curseurs (0,25 et 1), 13 lieux d'arrivée par curseur (trois rayons de 2, 5 et 15 km
à quatre caps, un lieu d'équipe, une fermeture de boucle). Issues : `ok` 83 135, `truncated` 1 556, `end_too_far` 1 564,
`no_seam_reached` 1 357, `end_on_masked_chain` 166, `end_on_route_no_seam` 14.

### 3.1 K2 : arrivée déplacée sans refus du contrôleur, 95 % des essais

**67 922 sur 85 482 = 79,5 % [79,2 ; 79,7]. Non tenu.** Le critère (§9 de la note) compte comme échec un `EndOnRoute` sans couture.
Dénominateur [calculé à la validation] : les 87 792 essais, moins les 750 fermetures de boucle (que le banc ne compte pas : 0/0)
et les 1 560 `end_too_far` restants (arrivée hors de portée, refusée avant toute recherche) ; les 1 351 `no_seam_reached` y sont des
échecs. Remettre les fermetures de boucle (601 acceptées sur 746 jouables) donnerait 79,5 % aussi : le choix ne pèse pas.
Refus classés [calculé sur le jsonl : 84 131 essais joués et jugés, dont 16 209 refusés ; le dénominateur du banc, 85 482, compte aussi les essais sans couture atteinte] :

| cause du refus | essais | part des refus |
|---|---|---|
| **`SHARP_TURN` en C2 seul** (ni C1, ni autre code) | 14 435 | 89,1 % |
| C1 et C2 tous deux refusent | 1 125 | 6,9 % |
| C1 seul (surtout `SHARP_TURN`, `REUSE_OUTSIDE_TAIL`) | 181 | 1,1 % |
| `EndOnRoute` sans couture (chaîne masquée, ou sans couture après `E′`) | 104 | 0,6 % |
| C2 seul, autres codes (grappes, croisements, réutilisation) | 364 | 2,2 % |
| **refus** | 16 209 | 100 % |

C3 (réutilisation de chaîne hors zone) : **0 sur 83 135**. Le masque tient sur `move_end`. Reprises après un virage serré : 20,7 % des essais
(1 à 4 passes) ; rayon élargi : 0,04 % ; rejeu du préfixe : 1,6 %.

Contrefactuel [calculé, **non mesuré**] : si les 14 435 refus à `SHARP_TURN` seul étaient évités, K2 serait de (67 922 + 14 435) / 85 482 =
**96,3 %**. Cela suppose que la cause se corrige sans en créer d'autres.

L'angle de couture n'explique pas ces refus : 93 % des refusés ont un angle sous 120° (la couture valide impose 150° au plus), et la
distribution est la même entre acceptés et refusés (P50 83° contre 88°) [calculé]. Le `SHARP_TURN` de C2 est donc un virage
**près de la couture** (la règle de grappe de nœuds à moins de 100 m de `ord-loops-check` en est le suspect) [supposé]. Le moteur ne connaît pas le cap d'arrivée de la jambe gardée : c'est ce que le §11 de la note prévoyait
pour le lot 1.

K2 par groupe (K2 du banc) :

| groupe | essais | K2 |
|---|---|---|
| ROAD boucles | 55 244 | 79,3 % [79,0 ; 79,5] |
| ROAD aller simple | 6 766 | 81,2 % [80,4 ; 81,9] |
| GRAVEL boucles | 20 932 | 78,7 % [78,2 ; 79,1] |
| GRAVEL aller simple | 2 230 | 85,1 % [83,8 ; 86,3] |
| MTB boucles | 154 | 82,5 % [76,9 ; 86,9] |
| MTB aller simple | 156 | 100,0 % [98,3 ; 100,0] |
| rayon 2 km | 26 798 | 79,0 % [78,6 ; 79,4] |
| rayon 5 km | 26 406 | 80,2 % [79,8 ; 80,6] |
| rayon 15 km | 25 738 | 80,0 % [79,6 ; 80,4] |
| lieu d'équipe | 6 540 | 76,0 % [75,1 ; 76,9] |
| curseur 0,25 | 42 741 | 79,3 % [79,0 ; 79,6] |
| curseur 1 | 42 741 | 79,6 % [79,3 ; 79,9] |
| équipe A | 70 518 | 79,1 % [78,8 ; 79,3] |
| équipe B | 10 620 | 83,2 % [82,5 ; 83,7] |
| équipe C | 2 114 | 78,9 % [77,4 ; 80,3] |
| équipe D | 1 582 | 77,2 % [75,4 ; 78,9] |
| équipe E | 258 | 56,6 % [51,5 ; 61,6] |

Aucun groupe n'approche 95 %, sauf MTB aller simple (156 essais). Le rayon, le curseur et le type font peu : **la cause est
uniforme**, ce qui va dans le sens d'un défaut de construction (le cap) plutôt que d'un défaut de données. Parmi les grands groupes, les
lieux d'équipe sont le moins bon cas (76,0 %).

### 3.2 Qualité de la proposition [mesuré]

| lieu | essais | raccord, m (P50 / P90) | raccord / vol d'oiseau (P50 / P90) | L′ / L (P50 / P90) | mètres abandonnés (P50 / P90) | part d'équipe du raccord (P50) |
|---|---|---|---|---|---|---|
| rayon 2 km | 26 313 | 2 603 / 3 607 | 1,33 / 1,81 | 1,042 / 1,074 | 244 / 1 586 | 0,96 |
| rayon 5 km | 25 970 | 6 294 / 7 940 | 1,29 / 1,60 | 1,109 / 1,180 | 265 / 2 635 | 0,96 |
| rayon 15 km | 25 546 | 18 975 / 22 278 | 1,28 / 1,50 | 1,340 / 1,530 | 314 / 4 717 | 0,98 |
| lieu d'équipe | 6 198 | 1 489 / 3 976 | 1,33 / 1,84 | 1,022 / 1,070 | 228 / 1 240 | 1,00 |

(Calcul sur la meilleure variante, essais ayant une proposition ; la fermeture de boucle donne un raccord nul par construction :
`E′` est le départ.) On quitte le parcours **tôt** : 244 à 314 m abandonnés en médiane, 1,6 à 4,7 km au P90. Le raccord fait 1,3 fois le vol d'oiseau.

Temps (§5.2) : `move_end_in` plus contrôles, **P50 72 ms, P90 127 ms, P99 227 ms** (maximum 8,9 s) ; la génération seule 28 ms en
médiane. **K4 `move_end` tenu.**

## 4. Livraison B : `resize` (K3, K4)

7 044 demandes (587 traces tirées au rang de `xxh3(id ^ graine)` ; poids de strate = population / échantillon), 6 valeurs de δ,
2 curseurs. 676 demandes (9,6 %) n'ont **aucune fenêtre** (`no_window`) : toutes sur des aller simples, 650 d'entre elles en raccourcissement, pour une longueur
médiane de 98 km. Issues : `ok` 5 513 (78,3 %), `engine_fail` 603, `no_window` 676, `all_refused` 237, `off_target` 15.

### 4.1 K3 : distance à ±3 % pour |δ| ≤ 25 %

| mesure | résultat | verdict |
|---|---|---|
| **pondérée** par strate (n effectif de Kish 1 281), seuil du banc | 1 139/1 281 = **88,9 % [87,4 ; 90,3]** | **à la limite** |
| brute | 3 760/4 696 = 80,1 % [79,1 ; 81,0] | non tenue |
| brute, avant les contrôles (un candidat à 3 % existe) | 3 899/4 696 = 83,0 % [82,1 ; 83,9] | |
| toutes valeurs de δ | 5 513/7 044 = 78,3 % [77,4 ; 79,1] | |

Le verdict dépend du choix de la pondération : le critère du §9 la prescrit, mais elle donne leur poids de population aux boucles de
l'équipe A (les plus nombreuses, et les mieux traitées). **Il faut lire les deux** : en population réelle, le critère est à la limite ;
sur ce qu'on teste, il ne l'est pas, et près d'un tiers des demandes d'aller simple échouent.

Succès par δ (non pondéré / pondéré [calculé]) :

| δ | demandes | succès | pondéré |
|---|---|---|---|
| +10 % | 1 174 | 97,4 % [96,6 ; 98,1] | 99,4 % |
| +25 % | 1 174 | 97,5 % [96,7 ; 98,2] | 99,6 % |
| +35 % | 1 174 | 96,5 % [95,5 ; 97,3] | 99,4 % |
| −10 % | 1 174 | 66,9 % [64,6 ; 69,1] | 83,9 % |
| −25 % | 1 174 | 58,4 % [56,0 ; 60,8] | 72,7 % |
| −35 % | 1 174 | 52,8 % [50,4 ; 55,2] | 66,0 % |

Par forme, |δ| ≤ 25 % (brut / pondéré [calculé]) :

| groupe | demandes | succès brut | pondéré |
|---|---|---|---|
| boucles | 2 424 | 89,5 % [88,4 ; 90,5] | 91,1 % |
| aller simple | 2 272 | **70,0 % [68,4 ; 71,6]** | 71,1 % |
| ROAD boucles | 1 360 | 87,9 % [86,3 ; 89,2] | 90,8 % |
| ROAD aller simple | 1 544 | 61,7 % [59,6 ; 63,7] | 65,2 % |
| GRAVEL boucles | 1 016 | 91,4 % [89,9 ; 92,8] | 91,9 % |
| GRAVEL aller simple | 680 | 89,0 % [86,8 ; 90,8] | 88,9 % |
| MTB boucles | 48 | 93,8 % [85,4 ; 97,5] | 93,8 % |
| MTB aller simple | 48 | 70,8 % [59,2 ; 80,3] | 70,8 % |
| curseur 0,25 | 2 348 | 78,9 % [77,5 ; 80,2] | 88,5 % |
| curseur 1 | 2 348 | 81,3 % [79,9 ; 82,5] | 89,3 % |

Par δ et forme (brut, [calculé]) : boucles +10/+25/+35 % : 99,0 · 99,8 · 99,3 % ; boucles −10/−25/−35 % : 85,3 · 73,8 · 72,9 % ; aller
simple +10/+25/+35 % : 95,8 · 95,1 · 93,5 % ; **aller simple −10/−25/−35 % : 47,2 · 42,1 · 31,3 %**.

Par longueur de l'original, |δ| ≤ 25 % (brut / pondéré [calculé]) : moins de 30 km 76,5 % / 77,7 % ; 30 à 60 km 86,8 % / 89,6 % ;
60 à 100 km 74,9 % / 90,1 % ; 100 à 200 km 67,8 % / 87,1 % ; 200 km et plus 66,8 % / 65,7 %.

### 4.2 Causes des échecs de `resize`, classées [mesuré, sauf mention]

**Issue de la demande** (7 044 ; 1 531 échecs) :

| cause | demandes | part des échecs [calculé] | où |
|---|---|---|---|
| `no_window` : aucune fenêtre (i, j) montée | 676 | 44,2 % | aller simples en raccourcissement : −35 % 272, −25 % 226, −10 % 152 (et 26 en allongement) |
| `engine_fail` : le moteur ne rend aucune route | 603 | 39,4 % | ROAD 461, GRAVEL 124, MTB 18 ; échec dominant `DISTANCE_TOO_SHORT` pour 437 |
| `all_refused` : des routes, toutes refusées par le contrôleur | 237 | 15,5 % | |
| `off_target` : une proposition, mais à plus de 3 % | 15 | 1,0 % | |

**Échec dominant du moteur** (603 `engine_fail`) : `DISTANCE_TOO_SHORT` 437, `NO_ROUTE:END_UNREACHABLE` 122, `TOO_LONG` 24, `NO_ROUTE:NO_CANDIDATE` 15,
`NETWORK_TOO_SMALL` 5. Sur les raccourcissements (3 522 demandes) : `DISTANCE_TOO_SHORT` comme échec dominant 437/3 522 = **12,4 % [11,5 ; 13,4]** ;
sur au moins une fenêtre 1 636/3 522 = **46,5 % [45,1 ; 47,8]** ; sur les fenêtres de raccourcissement 4 350/14 276 = 30,5 %.
Le seuil de 20 % de la note pour introduire `--shorten-tree` est franchi ou non selon la mesure choisie ; il l'est à 30,5 % par fenêtre
et à 46,5 % par demande, pas à 12,4 %.

**Refus du contrôleur** (codes sur tous les candidats) : `C2:U_TURN` 9 184, `C2:REUSE_OUTSIDE_TAIL` 5 275, `C2:SHARP_TURN` 2 386, `C3:REUSE` 2 292,
`C2:TOO_MANY_CROSSINGS` 779, `C2:EDGE_USED_MORE_THAN_TWICE` 167, `C1:SHARP_TURN` 60, divers 17. C'est la même prédiction que pour `move_end`
(cap d'arrivée inconnu à la couture) : le demi-tour à la couture et la réutilisation de l'arrivée sont les deux premiers refus.
Le plafond de contrôles est atteint pour 1,2 % des demandes contrôlées (71/5 765) : **ce n'est pas le biais principal**.

**Rendement des fenêtres** : 39 845 fenêtres sur 48 270 rendent une route, soit 82,5 %. La meilleure proposition vient de la famille de fenêtres 0 dans 4 207 cas sur 5 513 (76,3 %), de la famille 1 dans 1 306 (23,7 %) [calculé]
(en allongement, la famille 0 est `ℓ = 1,5 |Δ|` et la 1 `3 |Δ|` ; en raccourcissement, le rang du `j` : §5.3 de la note).

**Ce que le masque apporte** (`--no-keep-out`, mêmes 7 044 demandes) :

| | avec masque | sans masque |
|---|---|---|
| K3 pondéré | 88,9 % [87,4 ; 90,3] | 83,1 % [81,3 ; 84,8] |
| K3 brut | 80,1 % | 73,9 % [72,8 ; 74,9] |
| succès toutes δ | 78,3 % | 71,5 % [70,6 ; 72,4] |
| `all_refused` | 237 | 955 |
| plafond de contrôles atteint | 1,2 % | 19,5 % |
| `C3:REUSE` (candidats refusés) | 2 292 | 31 984 |
| fenêtres qui rendent une route | 82,5 % | 89,8 % |
| temps P50 / P90, ms | 524 / 4 060 | 580 / 4 090 |

Sur les raccourcissements (|δ| ≤ 25 %), le masque fait passer le succès de 53,8 % à 62,6 % [calculé : 1 263 → 1 471 sur 2 348]. Sur les
allongements : 94,0 % → 97,5 % (2 207 → 2 289). Le masque ne coûte pas de temps (P90 de 4 060 contre 4 090 ms). **Il est utile et reste
acquis** (§5.5 de la note).

### 4.3 K4 : P90 du temps sous 2 s

| mesure | P50 | P90 | verdict |
|---|---|---|---|
| `resize`, somme des générations, 16 fils | 524 ms | **4 060 ms** | **non tenu** |
| `resize`, avec les contrôles [calculé] | 789 ms | **4 470 ms** | **non tenu** |
| `resize`, génération seule, pondéré par strate [calculé] | 566 ms | 3 882 ms | |
| `resize`, avec les contrôles, pondéré par strate [calculé] | 910 ms | 4 242 ms | |
| `move_end`, génération et contrôles, 16 fils | 72 ms | **127 ms** | **tenu** |
| première proposition à 3 % (`resize`) | 93 ms | 442 ms | |

Toutes les lignes `resize` portent sur les **6 368 demandes qui ont au moins une fenêtre** : c'est le dénominateur du P90 du banc
(4 060 ms), les 676 `no_window` ne lançant aucune recherche (temps nul). *Correction de la validation* : la première version de ce
relevé calculait les lignes « avec les contrôles » et « pondéré » sur les 7 044 demandes, `no_window` à zéro compris (655 / 4 139 ms et
544 / 3 778 ms), ce qui les rendait plus flatteuses que la ligne du banc et non comparables à elle. Les contrôles font partie du
chemin d'une retouche (contrôle du recollé, lot 1) : **le P90 de service à retenir est 4,5 s** (4,2 s pondéré) ; les contrôles seuls :
P50 181 ms, P90 441 ms. Part des demandes sous 2 s, sur les 6 368 : 76,4 % en génération seule, 73,4 % avec les contrôles ; pondérée
74,6 % et 71,5 % [calculé] (78,6 % et 75,1 % dans la première version, `no_window` compris).

Où le temps passe, génération seule, 16 fils [calculé] :

| groupe | P50 | P90 | P99 |
|---|---|---|---|
| −35 % | 100 ms | 371 ms | 1,4 s |
| −25 % | 120 ms | 443 ms | 1,0 s |
| −10 % | 173 ms | 561 ms | 1,8 s |
| +10 % | 647 ms | 2,3 s | 8,5 s |
| +25 % | 3 113 ms | 10,7 s | 30,0 s |
| +35 % | 1 728 ms | 5,4 s | 18,7 s |
| original < 30 km | 167 ms | 1,0 s | 2,1 s |
| 30 à 60 km | 378 ms | 2,6 s | 5,4 s |
| 60 à 100 km | 443 ms | 3,5 s | 7,5 s |
| 100 à 200 km | 799 ms | 7,0 s | 18,5 s |
| 200 km et plus | 1 537 ms | **18,0 s** | 44,0 s |
| curseur 0,25 · 1 | 428 · 404 ms | 4,0 · 3,5 s | |

**La queue est faite des longs parcours et des allongements** ; le raccourcissement est rapide mais il échoue (§4.2). De 30 à 100 km, le P90
est de 2,6 à 3,5 s : K4 ne tiendrait pas même sans les très longues traces. Le P90 du 1,05 s mesuré sur les 8 traces du recalage
(§6.3) vient d'un échantillon de boucles courtes de B ; il ne dit rien de la population.

### 4.4 Qualité des propositions rendues (sans seuil) [mesuré, calculé pour les ventilations]

| δ | rendues | part gardée P10 / P50 / P90 | `changed_m` / \|Δ\| (P50) | part d'équipe du tronçon P50 | angle d'entrée P50 / P90 | angle de sortie P50 / P90 |
|---|---|---|---|---|---|---|
| −35 % | 620 | 0,49 / 0,62 / 0,72 | 0,6 | 0,96 | 16° / 92° | 16° / 98° |
| −25 % | 686 | 0,51 / 0,65 / 0,80 | 0,9 | 0,96 | 10° / 91° | 19° / 98° |
| −10 % | 785 | 0,56 / 0,73 / 0,88 | 1,9 | 0,96 | 17° / 92° | 21° / 98° |
| +10 % | 1 144 | 0,77 / 0,77 / 0,78 | 2,1 | 0,90 | 8° / 89° | 9° / 95° |
| +25 % | 1 145 | 0,20 / 0,50 / 0,51 | 2,3 | 0,91 | 11° / 94° | 11° / 98° |
| +35 % | 1 133 | 0,35 / 0,35 / 0,36 | 2,3 | 0,89 | 10° / 93° | 8° / 92° |
| toutes | 5 513 | 0,35 / 0,57 / 0,78 | 2,0 | 0,93 | 11° / 92° | 12° / 96° |

Écart à la cible : P50 0,15 %, P90 0,61 %, maximum 2,99 % [mesuré]. Part d'équipe du tronçon par curseur : **0,25 → P50 0,97
(P10 0,46) ; 1 → P50 0,88 (P10 0,11)** [calculé] : le curseur règle bien le goût de l'équipe contre l'ouverture.

La part gardée est presque constante à +10 % et à +35 % (P10 à P90 de 0,77 à 0,78 et de 0,35 à 0,36) : les fenêtres de l'allongement sont
dictées par la cible, et la proposition ne dépend guère du curseur à cet égard. La médiane des parts gardées, toutes δ, est de 0,57.

## 5. Variante : marge de 100 m (`fullm100`) [mesuré]

Même échantillon, même graine, mêmes demandes ; seule la marge de couture change.

| | marge 200 m | marge 100 m |
|---|---|---|
| K1 (continuité) | 54,2 % [52,8 ; 55,6] | 66,3 % [65,0 ; 67,6] |
| K2 | 79,5 % [79,2 ; 79,7] | 80,3 % [80,0 ; 80,5] |
| K3 pondéré | 88,9 % [87,4 ; 90,3] | 90,4 % [88,9 ; 91,6] |
| K3 brut | 80,1 % [79,1 ; 81,0] | 80,4 % [79,4 ; 81,3] |
| K4 `resize` P90 | 4 060 ms | 4 070 ms |
| K4 `move_end` P90 | 127 ms | 126 ms |
| `no_seam_reached` | 1 357 | 1 337 |
| −35 % · −25 % · −10 % | 52,8 · 58,4 · 66,9 % | 55,5 · 58,9 · 67,5 % |

Aucun verdict ne change : K1, K2 non tenus, K4 `resize` non tenu, K3 pondéré encore « à la limite » (borne basse 88,9 pour 90). Les 12 points de K1
gagnés ne font gagner que 0,8 point à K2 et 1,5 à K3 : **la marge a peu d'effet sur le service**.

## 6. Limites de la mesure

1. **L'échantillon de `resize`** pèse 587 traces et 7 044 demandes, dont toutes les petites équipes. La pondération donne leur poids de
   population aux strates de l'équipe A, plus faciles ; les totaux bruts et pondérés divergent de 9 points sur K3. Aucun des deux n'est
   « le » chiffre.
2. **C2 contrôle par différence** avec l'original : un défaut déjà présent dans la trace réelle n'est jamais compté. `unchecked_m` vaut 48,7 km
   au P50 de `move_end` (P90 91 km) à la fenêtre entière. Selon `control.rs`, la mesure est « atteint moins vérifié » : à la fenêtre entière, la portée est
   toute la marche et le contrôleur ne lit que la plage continue de la couture, donc le chiffre compte **tout le reste de la marche**, pas ce que les
   trous laissent sans contrôle près des coutures. **Il n'est pas interprétable comme tel** ; à ±5 km il valait environ 2 km à l'essai de la relecture
   [mesuré à l'essai, non rejoué ici].
3. **Le recalage à 1 fil** (`fullt1`) ne porte que sur 8 traces de B (4 ROAD boucles, 4 GRAVEL boucles), 96 demandes, 62 s : P50 152 ms, P90
   1 052 ms. Sur les 48 demandes que la campagne de base contient aussi (même clé), le temps est de P50 151 ms et P90 1 008 ms à 1 fil contre
   278 ms et 1 603 ms à 16 fils : un rapport de **0,54 au P50 et 0,63 au P90** [calculé]. Si ce rapport s'appliquait à la population, le P90
   de `resize` à 1 fil serait d'environ **2,6 s** [calculé, extrapolation d'un échantillon de boucles courtes, à ne pas retenir sans
   un recalage sur un tirage de toutes les strates]. Il resterait au-dessus de 2 s.
4. **`no_window` n'a pas de cause décomposée** : 676 demandes sans fenêtre, toutes sur des aller simples, 650 en raccourcissement. Le banc n'écrit pas
   la règle qui a refusé chaque paire (i, j). Hypothèse [supposé] : la règle du §5.3 de la note exige `(ℓ + Δ) / c(i, j) ≥ 1,3` ; sur un aller simple à peu près droit,
   le vol d'oiseau est proche de `ℓ` et aucun raccourcissement de 10 à 35 % n'y satisfait.
5. **Les temps** sont à 16 fils sur 8 cœurs : pessimistes pour le service, et le recalage à 1 fil est partiel (point 3).
6. **K2 ne compte que la variante `best`.** Les deux autres variantes de `move_end` sont mesurées, pas contrôlées.

## 7. Ce que les chiffres disent des seuils « proposés » du plan

| seuil du plan | ce que le relevé mesure | lecture |
|---|---|---|
| **plage d'au moins 200 m de part et d'autre** (§3, marge de couture) | 200 → 100 → 50 m : K1 54,2 → 66,3 → 70,3 % ; K2 79,5 → 80,3 % ; K3 pondéré 88,9 → 90,4 % ; temps inchangés | la marge de 200 m coûte 12 points de K1 pour presque rien en sécurité mesurée ; **100 m est préférable** (le contrôleur ne dégrade rien, K2 monte). À 50 m, K1 gagne 4 points de plus [calculé] mais **non mesuré en jeu** : à jouer avant de descendre. |
| **couture à 1 km de tout point, 95 % des parcours** (§10, critère) | 54,2 % à 1 km ; 94,5 % à 2,5 km ; 95 % à 2,6 km ; aller simples ROAD 26,5 % à 1 km | le seuil de 1 km **n'est pas atteignable** par la construction actuelle, ni par aucune variante (70,8 % au mieux). Il **ne prédit pas** le succès : K3 est à 88,9 % avec un K1 de 54 %, parce que le moteur choisit les fenêtres lui-même. Le critère utile est « il existe une paire de coutures valide à l'échelle de δ », pas « toute position du parcours est proche d'une couture ». |
| **±30 % de distance, refus au-delà** (D3) | succès à −10, −25, −35 % : 66,9 · 58,4 · 52,8 % ; à +10, +25, +35 % : 97,4 · 97,5 · 96,5 % | **il n'y a pas de falaise à 30 %** : en allongement, +35 % tient à 96,5 % (la borne pourrait être relevée) ; en raccourcissement, **−10 % déjà échoue une fois sur trois** (47 % de succès pour les aller simples). Le seuil de ±30 % est un choix de produit, pas un seuil que la mesure impose. Un refus à −25 % qui renverrait au générateur est cohérent avec ce qui est mesuré. |
| **±3 % de tolérance** (D1) | écart P50 0,15 %, P90 0,61 % ; 96,3 % des propositions à 1 % ou moins, 99,2 % à 2 % ; K3 à ±1 % : 86,7 % pondéré (77,9 % brut), à ±2 % : 88,6 % (79,5 %) | la tolérance **n'est pas ce qui limite** : K3 ne gagne que 2,2 points entre ±1 et ±3 % [calculé], et 15 demandes seulement sur 7 044 sont `off_target`. Resserrer à ±1 % coûterait 102 demandes sur 4 696 et ramènerait K3 à 86,7 % pondéré (non tenu). Les échecs sont des absences de proposition, pas des propositions imprécises ; **±3 % est à garder**, ±2 % (88,6 %) serait possible. |

## 8. Ce que le relevé laisse ouvert

- Quelle **règle de couture** porte les espaces de plus de 1 km (§2.3) : un compteur par règle, au lot 1.
- Le **`SHARP_TURN` près de la couture** (§3.1) : à localiser (grappe de nœuds, ou virage au premier nœud après la couture) avant d'ajouter le
  cap d'arrivée au moteur. La prévision de K2 à 96,3 % est un contrefactuel, pas une garantie.
- La **cause de `no_window`** (§4.2, §6) et la raison pour laquelle l'allongement à +25 % coûte plus cher que celui à +35 % (P90 10,7 s contre 5,4 s).
- Le **recalage à 1 fil** sur un tirage de toutes les strates.
- L'**anomalie d'ordre** de 5 % (§2.4) : rejet à l'entrée ou correction de la projection.

## 9. Conclusion

**Lecture des critères du §9 de la note, avec le chiffre de chacun.**

| # | critère | résultat [mesuré] | verdict |
|---|---|---|---|
| K1 | couture à ≤ 1 km pour 95 % des parcours | 1 819/3 355 = **54,2 % [52,8 ; 55,6]** (à 2,6 km : 95 %) ; meilleure variante 70,8 % | **non tenu** |
| K2 | `move_end` sans refus du contrôleur, 95 % | 67 922/85 482 = **79,5 % [79,2 ; 79,7]** ; 89 % des refus sont un `SHARP_TURN` de C2 seul (contrefactuel 96,3 % [calculé]) | **non tenu** |
| K3 | ±3 % pour \|δ\| ≤ 25 %, 90 % | pondéré 1 139/1 281 = **88,9 % [87,4 ; 90,3]** ; brut 3 760/4 696 = 80,1 % [79,1 ; 81,0] | **à la limite** (pondéré) ; **non tenu** (brut) |
| K4 | P90 du temps sous 2 s | `resize` **4 060 ms** à 16 fils en génération seule, **4 470 ms** avec les contrôles [calculé] (environ 2,6 s à 1 fil, génération seule [calculé, incertain]) ; `move_end` **127 ms** | `resize` **non tenu** ; `move_end` **tenu** |
| ordre | moins de 1 % de traces avec anomalie d'ordre | **5,0 % [4,5 ; 5,7]** | **non tenu** |

**Décision du §9 de la note.** K1 et K2 n'étant pas tenus, le **lot 1 (arrivée) ne s'ouvre pas tel que le critère le demande**. K3 n'étant
qu'à la limite et K4 `resize` n'étant pas tenu, le **lot 2 (distance) non plus**. Mais les causes sont plus étroites que les verdicts :

- **`move_end` est le plus près.** Il est rapide (P90 127 ms) ; son seul vrai défaut (89 % des refus) est un virage que le moteur ne peut pas
  voir (le cap d'arrivée de la jambe gardée). Le lot 1 se justifie **à condition que son premier travail soit ce cap** et un compteur pour localiser
  le `SHARP_TURN` ; la cible est un K2 de 96 % [calculé, non garanti]. K1 à 1 km n'est pas la bonne condition d'entrée (voir ci-dessous).
- **Le critère K1 est mal posé**, pas seulement manqué : aucune variante (marge de 50 m, `Touch` continu) ne le tient, et il ne prédit pas le
  succès de `resize` (K3 à 88,9 % avec K1 à 54 %). Il faut le remplacer par une condition sur les paires de coutures offertes au moteur. Le
  **recalage géométrique du §11 du plan n'est pas justifié** par ces chiffres : ni les trous (cordes de quelques dizaines de mètres), ni
  la marge n'en sont la cause.
- **Le raccourcissement est le vrai problème de `resize`** : −10 % réussit 66,9 % du temps (aller simples : 47,2 %), −35 % 52,8 %. 44 % des
  échecs sont des demandes **sans fenêtre** et 39 % des échecs du moteur (`DISTANCE_TOO_SHORT` en tête). L'option `--shorten-tree` est
  justifiée si le seuil de 20 % est lu sur les fenêtres (30,5 %) ou sur les demandes qui l'ont rencontrée (46,5 %), moins s'il est lu sur
  l'échec dominant (12,4 %).
- **L'allongement est bon mais lent** : 97 % de succès pour +10 à +35 %, P90 de 2,3 à 10,7 s. Les 200 km et plus (P90 18 s) et les
  allongements à +25 % font la queue.
- **Le masque des arêtes gardées doit rester** : sans lui K3 perd 5,8 points et un cinquième des demandes atteignent le plafond de contrôles.
- **Seuils du plan.** Marge de couture : **100 m** plutôt que 200 (K1 +12 points, K2 +0,8, K3 +1,5, temps inchangés), 50 m à jouer. Seuil de
  K1 : 1 km est inatteignable, 2,6 km donnerait 95 % [calculé]. **±30 %** : choix de produit, sans falaise dans la mesure ; l'allongement
  tient à +35 %, le raccourcissement échoue déjà à −10 %. **±3 %** : non limitant, mais à garder : ±2 % donne 88,6 % et ±1 % 86,7 % de K3 pondéré, au lieu de 88,9 %.
- **À corriger avant tout lot** : 5 % de traces avec anomalie d'ordre (critère de moins de 1 % non tenu) ; elles ont deux fois plus de
  trous aux 100 km, et doivent être rejetées à l'entrée ou corrigées.

**Recommandation** : ne pas ouvrir le lot 2 ; ouvrir le lot 1 sous la forme étroite « cap d'arrivée au moteur, un compteur par règle de
couture, rejet des traces à anomalie d'ordre, marge de 100 m », avec un critère K1 remplacé. Le choix reste au propriétaire.
