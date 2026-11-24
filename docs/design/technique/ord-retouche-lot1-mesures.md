# Retouche d'un parcours — lot 7a (« lot 1 » de la note) : relevé des campagnes du moteur

> Relevé du 24 novembre 2026 sur `develop` (arbre de travail **non commité** : le lot 7a, moteur `ord-loops` et banc), binaire `release`,
> poste de développement (16 fils sur 8 cœurs physiques, 30 Go). Graphe et vues France `france-20261115-z11`, surcouches `lot6/overlays`,
> même export que le lot 0 (empreinte xxh3 `ca53159cc90498d6`, 3 377 lignes). Conception : [`ord-retouche-lot1.md`](ord-retouche-lot1.md)
> (écarts au §14) ; relevé de référence : [`ord-retouche-lot0-mesures.md`](ord-retouche-lot0-mesures.md) ; plan appelant :
> `docs/plans/2026-11-22-retouche-parcours.md` de Pédalons (ledger `API-212`).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/retouche-bench/` (préfixes `engine16`, `lot0m100`,
> `engine1t`, `freesection`, `engine16long` pour le premier relevé ; `rbase`, `rall`, `final`, `mefar`, `final1t` pour le second). Lettres
> d'équipes et agrégats seulement ici : ni départ, ni coordonnée, ni identifiant d'équipe, de trace ou d'arête. Rien n'a été commité, poussé ni déployé.
>
> **Deux relevés dans ce fichier.** Le premier (`engine16` et suivants) a mesuré le moteur tel que livré au premier jet. La relecture a
> montré que l'API perdait les diagnostics d'un refus, ce qui a faussé plusieurs lectures du premier relevé (§9). Le moteur a été corrigé
> (diagnostics rendus sur un refus, contrôle du recollé jugé par position, refus de la règle des grappes nommé) et **une** modification de
> réglage tentée pour K3 (§5) ; la campagne complète a été rejouée (`final`). **Les chiffres du résumé, des §2 à §4 et de la conclusion sont ceux
> du second relevé** ; ceux du premier restent donnés, marqués « premier relevé », là où la comparaison sert.
>
> Marques : **[mesuré]** vient des sorties du banc ; **[calculé]** d'un calcul fait ici sur les jsonl privés ; **[lu]** vient du code ; **[supposé]**
> n'est pas vérifié. Sans marque dans un tableau : mesuré. Les temps sont pris à 16 fils simultanés, sauf mention : pessimistes pour le service.

## 0. En bref

- **K2 (`move_end`) est tenu : 98,4 % [98,3 ; 98,5]** (inchangé par le second relevé) contre 79,5 % au lot 0 (80,3 % à marge de 100 m, rejoué ici à
  tirage égal : 80,3 %). Le `SHARP_TURN` de C2, 89 % des refus du lot 0, tombe à **6 refus sur 1 248** [calculé]. Réserve : les essais `end_too_far`
  (arrivée à plus de 500 m d'une voie, hors dénominateur comme au lot 0) passent de 1 564 à 5 174 ; si on les compte comme des échecs, K2 vaut 92,2 % [calculé]. **Cause lue,
  non plus supposée** : le filtre `dist_m ≤ end_snap_m` (500 m) du moteur, absent du prototype du lot 0 **[lu]** ; avec `--end-snap-m 1000000`, `end_too_far` retombe à
  1 396 (lot 0 : 1 564) et K2 vaut **98,2 %** à dénominateur égal à celui du lot 0 [mesuré, `mefar`]. Le seuil de 500 m est donc une décision de produit, pas un défaut de recherche.
- **K3 (`resize`) : 89,0 % pondéré [87,5 ; 90,3]**, brut 76,7 %, **à la limite du critère de 90 %** (point sous 90, intervalle qui le contient),
  contre 88,9 % pondéré au lot 0 (90,4 % à 100 m) ; **64,0 % au premier relevé**. Le gain vient de **trois réglages** essayés d'un bloc (une seule itération, §5) :
  la partie remplacée libre pour l'allongement (allonger 75,2 → 96,1 %), libre aussi dans l'arbre de raccourcissement, et la lecture des `j` jusqu'à 3 % au lieu de 1 %
  (raccourcir 41,8 → 57,2 %). **Raccourcir reste le point faible** : 57,2 % contre 63,2 % pour la lentille du lot 0 à 100 m, 38,9 % sur aller simple.
- **K4 est tenu : P90 de `resize` 1 140 ms** (contre 4 060 ms au lot 0 ; 1 130 ms à 1 fil sur 10 traces par strate), `move_end` P90 139 ms. Le chiffre est
  **borné par les échéances du moteur** (souple 1 s, budget 1,8 s ; maximum mesuré 2,14 s) et ne prouve pas un moteur plus rapide à tâche égale (§4).
- **L'ordre des jambes** : 167 traces sur 3 355 (5,0 %) refusées à l'entrée pour `OrderAnomaly`, 16 (0,5 %) pour `Drift` ; 3 172 posées.

## 1. Les campagnes [mesuré]

Lancées en arrière-plan l'une après l'autre (jamais deux à la fois : la mémoire et les fils sont partagés), `./target/release/ord-loops-bench retouche-play …`.

**Premier relevé** (moteur du premier jet, partie remplacée masquée par défaut) :

| Campagne | Options | Durée | Empreintes (`move_end` · `resize`) |
|---|---|---|---|
| `engine16` | `--impl engine --max-minutes 120` (marge 100 m, `--max-change 0,40`, échéances 1 s / 1,8 s) | 873 s | `42ebd884c6eda5cd` · `cf7f34a4185176c3` |
| `lot0m100` | `--impl lot0 --seam-margin-m 100 --max-minutes 120` | 1 661 s | `afc9750420ab2b25` · `4151e2f74b43aafc` |
| `engine1t` | `--impl engine --threads 1 --limit 10` | 349 s | `fdf4636d13ffe533` · `5ea842c34a047570` |
| `freesection` | `--impl engine --free-section --op resize` (l'option est devenue `--masked-section`, inverse) | 310 s | · `11efcbd59144f0a6` |
| `engine16long` (diagnostic) | `--impl engine --op resize --soft-ms 30000 --budget-ms 60000` | 473 s | · `0f492acc6b91ba6d` |

**Second relevé** (moteur corrigé ; défauts : `free_section`, `free_shorten`, `shorten_read_tol` 3 %) :

| Campagne | Options | Durée | Empreintes (`move_end` · `resize`) |
|---|---|---|---|
| `rbase` | `--impl engine --op resize`, moteur aux diagnostics corrigés et aux défauts du premier relevé (partie remplacée masquée, lecture à 1 %) : **le comportement du premier relevé**, avec les compteurs rendus sur un refus | 340 s | · non relevée |
| `rall` | `--impl engine --op resize --free-section --free-shorten --read-tol 0.03` (les trois réglages, premier tri des lectures ; options d'alors, devenues les défauts) | 264 s | · non relevée |
| `final` | `--impl engine --max-minutes 120` (défauts du moteur), campagne complète | 937 s | `e36195cda6091608` · `66449432a5f4ca40` |
| `mefar` | `--impl engine --op move-end --end-snap-m 1000000` (K2 à dénominateur égal au lot 0) | 660 s | `fde38a8c5d2023fd` · |
| `final1t` | `--impl engine --threads 1 --limit 10` | 448 s | `fdf4636d13ffe533` · `fc5b7056cba6a1af` |

Les empreintes de `move_end` de `final` et d'`engine16` diffèrent alors que les 83 000 issues sont identiques à l'unité près : l'empreinte d'un refus est celle du nom de l'issue, et
`junction_cut` (42 essais) n'existait pas au premier jet (il comptait dans `no_seam_reached`). `rbase` et `rall` ont tourné sur des états intermédiaires du code (le tri des lectures et les défauts ont
changé entre `rall` et `final`) : ils servent à attribuer le gain, pas de relevé. Ce relevé-ci ne rejoue pas `lot0m100`, déjà
**identique au relevé du lot 0** (K2 80,3 %, K3 pondéré 90,4 %, mêmes empreintes `afc9750420ab2b25` et `4151e2f74b43aafc` que `fullm100`). Pic anonyme 14,9 Go (`engine16long`, premier relevé). Le banc n'a pas planté.

**Population.** 3 355 traces mesurées ; le moteur en pose 3 172 (refus d'entrée : `OrderAnomaly` 167, `Drift` 16). `move_end` : 3 171 traces
éligibles, 83 000 essais (lot 0 : 3 354 et 87 792). `resize` : 576 traces, 6 912 demandes (lot 0 : 587 et 7 044). **Les deux jeux diffèrent donc
de 183 traces** (les refusées) : les chiffres du lot 0 et du lot 1 ne sont pas au même dénominateur (K2 et K3 ne comptent pas les traces
à anomalie d'ordre, plus difficiles : 5,0 % de la population en moins).

## 2. `move_end` (K2)

### 2.1 K2 [mesuré]

**75 866 sur 77 114 = 98,4 % [98,3 ; 98,5]. Tenu** (critère 95 %), identique aux deux relevés. Issues des 83 000 essais (`final`) : `ok` 75 757, `truncated` 1 436,
`end_too_far` 5 174, `no_seam_reached` 543, `junction_cut` 42, `all_refused` 48. Dénominateur de K2 (comme au lot 0) : tous les essais moins `end_too_far`, moins les fermetures de boucle ;
`no_seam_reached` et `junction_cut` y sont des échecs.

| | lot 0 (marge 200 m) | lot 0 à 100 m, rejoué | lot 1 (moteur, 100 m) | lot 1, `--end-snap-m 1000000` |
|---|---|---|---|---|
| K2 | 79,5 % [79,2 ; 79,7] | 80,3 % [80,0 ; 80,5] | **98,4 % [98,3 ; 98,5]** | **98,2 %** (79 470 / 80 892) |
| refus | 16 209 sur 84 131 jugés | 16 878 | 1 248 | 1 422 |
| `end_too_far` | 1 564 | 1 564 | **5 174** (6,2 % des essais) | 1 396 |
| essais refaits après un virage serré | 20,7 % | 20,4 % | 2,7 % | |
| `SHARP_TURN` (C1 ou C2) parmi les refus | ≈ 90 % | 16 263 codes | **6** | |
| temps de l'appel P50 · P90 | 72 · 127 ms | 70 · 120 ms | 61 · 139 ms | 62 · 144 ms |

**Réserve sur `end_too_far`, désormais lue dans le code.** Le moteur refuse avant toute recherche une arrivée à plus de `end_snap_m` = 500 m d'une voie du type
(`snapped.filter(|s| s.dist_m <= tune.end_snap_m)`, `graft_move_end.rs`) **[lu]** ; le prototype du lot 0 n'avait pas ce filtre. Le nombre passe donc de 1 564 à 5 174 (surtout
`ring_15` : 2 570, `ring_2` 1 304, `ring_5` 1 300 ; équipe B 3 180 de ces essais [calculé]). **Mesuré** : avec `--end-snap-m 1000000`, 1 396 essais restent `end_too_far` (aucune voie du type dans la fenêtre,
comme au lot 0) et K2 est **98,2 % à dénominateur égal à celui du lot 0** (`mefar`) : le moteur ne perd rien par rapport au prototype, et c'est le seuil de 500 m qui décide. Compter les 5 174 essais comme des échecs donne
75 866 / 82 288 = **92,2 %** [calculé], sous le critère : la question est de savoir si un lieu d'arrivée à plus de 500 m d'une voie doit être un refus du moteur ou une erreur d'entrée, ou si le seuil doit monter.

### 2.2 Refus restants [calculé sur le jsonl `final` : 1 248 refus parmi 77 114 essais]

| cause | essais | part des refus |
|---|---|---|
| `no_seam_reached` (aucune sortie de couture dans l'arbre, rayon maximal compris) | 535 | 42,9 % |
| `REUSE_OUTSIDE_TAIL` (C1 et/ou C2) | 513 | 41,1 % |
| `CORRIDOR_DOUBLED` (C2) | 70 | 5,6 % |
| `all_refused` (le moteur n'a rien proposé de recevable) | 48 | 3,8 % |
| `TOO_MANY_CROSSINGS` (C2) | 41 | 3,3 % |
| `junction_cut` (des sorties atteintes, toutes coupées par la règle des grappes) | 40 | 3,2 % |
| `SHARP_TURN` | 6 | 0,5 % |

**Correction du premier relevé.** Il lisait `no_seam_reached` (46,1 % des refus) comme « aucune couture atteinte par la recherche ». Le moteur rendait sous ce nom aussi le cas où toutes les sorties
atteintes étaient coupées par la règle des grappes. Distingué à présent (`RetouchFailure::JunctionCut`) : **ce second cas ne pèse que 42 essais sur les 585 (7 %)**, pas la moitié ; `no_seam_reached` au sens strict reste 543.
Le contrefactuel du lot 0 (« sans les `SHARP_TURN`, 96,3 % ») est confirmé et dépassé : le cap d'arrivée de la jambe gardée donné à la recherche supprime la cause.

`REUSE_OUTSIDE_TAIL` (41 %) : le raccord revient sur le parcours gardé hors de sa fin. **Hypothèse proposée par la relecture** [supposé] : le contrôleur déplace la queue avec l'arrivée, et une réutilisation que la trace
d'origine faisait déjà dans sa queue est reprochée au raccord. **Non vérifiée jusqu'au bout** : le C2 du banc retire déjà les codes que l'original a autour des mêmes coutures (`control::c2`, `now.difference(&before)`) **[lu]**,
ce qui rend l'hypothèse moins probable pour C2 (443 codes) ; pour C1 (395 codes), le contrôleur indépendant juge le raccord seul, sans référence à l'original, et l'hypothèse reste ouverte. Comparer les arêtes réutilisées à celles
de la trace d'origine demande un instrument que ce lot n'a pas écrit.
Concentration : 504 traces sur 3 171 ont au moins un refus (au plus 16 essais pour la pire) ; 2 667 traces (84,1 %) n'ont aucun refus [calculé, premier relevé].

### 2.3 K2 par groupe [calculé, premier relevé : K2 total identique au second]

| groupe | essais | K2 |
|---|---|---|
| ROAD boucles | 49 970 | 98,5 % |
| ROAD aller simple | 5 898 | **95,4 %** |
| GRAVEL boucles | 18 890 | 98,9 % |
| GRAVEL aller simple | 2 054 | 98,4 % |
| MTB (boucles · aller simple) | 150 · 152 | 98,7 · 100,0 % |
| rayon 2 · 5 · 15 km | 24 064 · 24 068 · 22 798 | 98,3 · 98,0 · 99,0 % |
| lieu d'équipe | 6 184 | 98,3 % |
| curseur 0,25 · 1 | 38 557 · 38 557 | 98,4 · 98,4 % |
| équipe A · B | 65 322 · 8 130 | 98,7 · 98,4 % |
| équipes C · D · E | 1 744 · 1 334 · 250 | 92,3 · 94,9 · 98,4 % |
| équipes H · I (petites) | 154 · 138 | 77,9 · 83,3 % |

Tous les grands groupes dépassent 95 %, sauf les aller simples ROAD (95,4 %, à la limite) et les petites équipes C, D, H et I (moins de
2 000 essais chacune). Le lieu d'équipe, le moins bon du lot 0 (76,0 %), est à 98,3 %.

### 2.4 Temps [mesuré]

`move_end_in` plus contrôles, 16 fils : **P50 61 ms, P90 139 ms** (maximum 759 ms). À 1 fil (10 traces par strate) : P50 27 ms, P90 72 ms. **Tenu.**
Le contrôle indépendant seul (`indep_ms`) est compté à part : l'appel du moteur est le temps de service.

## 3. `resize` (K3)

### 3.1 K3 [mesuré]

| mesure | lot 0 (marge 200 m) | lot 0, 100 m, rejoué | lot 1, premier relevé | **lot 1, `final`** |
|---|---|---|---|---|
| **K3 pondéré** (n effectif de Kish) | 88,9 % [87,4 ; 90,3] | 90,4 % [88,9 ; 91,6] | 64,0 % [61,8 ; 66,2] | **89,0 % [87,5 ; 90,3]** |
| K3 brut | 80,1 % | 80,4 % [79,4 ; 81,3] | 58,4 % [57,2 ; 59,6] | **76,7 % [75,7 ; 77,7]** |
| raccourcir seul (\|δ\| ≤ 25 %), brut | | 63,2 % [calculé] | 41,8 % [40,2 ; 43,5] | **57,2 % [55,5 ; 58,9]** |
| allonger seul (\|δ\| ≤ 25 %), brut | | 97,6 % [calculé] | 75,0 % [73,5 ; 76,5] | **96,1 % [95,4 ; 96,7]** |
| succès toutes δ | 78,3 % | 79,0 % | 54,3 % | **73,9 % [73,0 ; 74,7]** (5 107 / 6 912) |

**Verdict K3 : à la limite, non tenu à la lettre.** 89,0 % est sous 90 %, mais l'intervalle [87,5 ; 90,3] contient le critère ; le lot 0 (88,9 %) l'avait manqué de la même manière et lui-même ne passait qu'à 100 m.
Le gain sur le premier relevé est de 25 points pondérés. Il reste 1 074 échecs sur 4 608 demandes (§3.3).

Succès par δ (tous types ; lot 0 à 100 m · premier relevé · `final`) :

| δ | lot 0 à 100 m | lot 1, premier relevé | lot 1, `final` |
|---|---|---|---|
| +10 % | 97,5 % | 72,1 % | **97,3 % [96,4 ; 98,0]** |
| +25 % | 97,6 % | 77,9 % | **95,0 % [93,8 ; 95,9]** |
| +35 % | 96,7 % | 57,6 % | **89,1 % [87,5 ; 90,6]** |
| −10 % | 67,5 % | 43,7 % | **61,2 % [58,8 ; 63,5]** |
| −25 % | 58,9 % | 40,0 % | **53,3 % [50,9 ; 55,7]** |
| −35 % | 55,5 % | 34,7 % | **47,4 % [45,0 ; 49,8]** |

Par forme, \|δ\| ≤ 25 %, brut [calculé] : boucles raccourcir **73,8 %** (lot 0 à 100 m : 80,0 % ; premier relevé : 55,0 %), boucles allonger **98,0 %** (99,5 % ; 79,9 %), aller simple raccourcir **38,9 %** (45,3 % ; 27,3 %),
aller simple allonger **94,0 %** (95,5 % ; 69,6 %). Raccourcir reste en dessous du lot 0 dans les deux formes ; allonger est à 1,5 point du lot 0 sur les deux.

**Tolérance** : à la lecture de 3 % (`shorten_read_tol`), des routes lues entre 1 et 3 % d'écart ont été gagnées, mais **98 demandes de raccourcissement ont été refusées par le seul contrôleur indépendant** (le moteur avait rendu des routes) ; 82 portent le code
`C1:DISTANCE_OUT_OF_TOLERANCE` et 74 n'ont que celui-là [mesuré, `final`] : le contrôleur juge la distance du tronçon neuf à ±3 % **de la section visée** (`sect = (to_s − from_s) + Δ`), pas de la distance totale, et une section plus courte
que le parcours rend les 3 % de lecture trop larges. Piste (non tentée, §5) : lire les `j` à 3 % **de la section**.

### 3.2 Distance tenue à ±3 % selon la taille de Δ [calculé]

La taille compte moins que le signe. Raccourcir tient moins quand Δ grandit (61,2 → 53,3 → 47,4 %), allonger moins nettement (97,3 → 95,0 → 89,1 %). Le banc joue `--max-change 0,40` (le service garde 0,30) : les ±35 % sont hors du périmètre du produit.

### 3.3 Causes des échecs [calculé, \|δ\| ≤ 25 %, 4 608 demandes dont **1 074 échecs**, `final`]

| cause | demandes | part des échecs | où |
|---|---|---|---|
| `no_economy` : aucune paire (i, j) lue dans l'arbre à 3 % de la cible | 568 | **52,9 %** | raccourcir seulement ; aller simples 516, boucles 52 (24,6 % des raccourcissements, tous formes : 568 / 2 304) |
| `all_refused` : des routes, toutes refusées | 404 | **37,6 %** | raccourcir 388 (`REUSE_OUTSIDE_TAIL` 272 : le chemin lu reprend une jambe gardée d'après `j` ; 98 refusées par le contrôleur indépendant, 74 par `C1:DISTANCE_OUT_OF_TOLERANCE` seul ; `SHARP_TURN` 7, `DEAD_END` 6, `NOT_ADMITTED` 5), allonger 16 |
| `timeout` | 32 | 3,0 % | allonger seulement |
| `change_too_small`, `too_long`, `off_target`, `no_seam` | 35 | 3,3 % | |
| `junction_cut` (les chemins lus sont tous coupés par la règle des grappes) | 19 | 1,8 % | raccourcir seulement (2,3 coupes par demande) |
| `engine_fail` (la lentille ne rend rien) | 16 | 1,5 % | allonger : `NO_ROUTE:END_UNREACHABLE` 8, `NO_ROUTE:NO_CANDIDATE` 8 |

Premier relevé (partie remplacée masquée) pour comparaison : `no_economy` 1 153 (60,2 %), `engine_fail` 446 (23,3 % : allonger 332 `END_UNREACHABLE`, raccourcir 114), `all_refused` 222, `timeout` 54. **Les 114 `engine_fail` de raccourcissement du premier
relevé, « sans cause rendue », sont des `junction_cut`** [mesuré : sous `rbase`, ils sortent tous en `junction_cut`, 114 sur 114 ; `no_economy` reste à 1 153]. Les 332 `END_UNREACHABLE` d'allongement tombent à 8 avec la partie remplacée libre.

**Corrections du premier relevé, mesurées sur `rbase` (même moteur, diagnostics rendus sur un refus)** :

- **Les 1 782 `no_economy` (toutes δ < 0) ont joué leurs arbres** : `trees > 0` pour les 1 782, temps P50 69 ms, P90 189 ms sous `rbase` (66 ms et 192 ms au premier relevé : le temps était déjà celui d'arbres joués). Les `trees: 0` et `windows_tried: 0` du premier relevé étaient un artefact du banc, qui ne lisait les compteurs du
  moteur que sur un succès, parce que l'API ne les rendait pas sur un refus. Même artefact pour « `END_UNREACHABLE` après zéro arbre » : le moteur n'a pas « donné la main sans chercher » ; ses arbres avaient tourné.
- **`spliced_refused` ne comptait que les demandes réussies** (le banc ne lisait les diagnostics que sur un succès) : « toutes demandes » du premier relevé sous-estimait les refus de jonction. Le banc les lit à présent sur tout résultat.

### 3.4 Pourquoi le raccourcissement échoue encore [mesuré sur `final`, 568 `no_economy`]

Les arbres ont tourné ; **aucune lecture n'a passé le seuil de 3 %** (les compteurs sont désormais rendus sur un refus) :

- 160 des 568 n'ont atteint **aucun `j`** (l'arbre, de rayon borné à 20 km, n'arrive à aucune couture admissible) ;
- les 408 autres ont atteint 76 495 lectures en tout ; le meilleur écart `err₀` par demande est au P10 5,2 %, **P50 11,1 %**, P90 33,3 % ; **aucun ≤ 3 %**, 37 entre 3 et 5 %, 97 entre 5 et 10 % ;
- 911 couples (arbre, `j`) ont été refusés au virage d'entrée dans la jambe gardée (`turn_cost`, `retro_ok`), contre 76 495 lus : le virage n'est pas ce qui limite.

Sur un aller simple (516 des 568), l'itinéraire de `i` à `j` aux poids du générateur est à peu près celui que le parcours suit déjà : l'économie `ℓ − m` est faible, et celle qu'il faudrait (`|Δ|`, 10 à 25 % de L) n'existe pas. C'est la limite
assumée de la note (§6.5), pas un défaut de lecture : **raccourcir un aller simple droit exige de déplacer une extrémité**.

## 4. Temps (K4) [mesuré, [calculé] pour les ventilations]

| mesure | P50 | P90 | P99 | verdict |
|---|---|---|---|---|
| `resize`, 16 fils, 6 892 demandes avec fenêtre | 323 ms | **1 140 ms** | 1 802 ms | tenu |
| `resize`, 16 fils, lot 0 à 100 m | 512 ms | 3 884 ms | | non tenu |
| `resize` à 1 fil (744 demandes, 10 traces par strate) | 169 ms | 1 130 ms | | tenu |
| `move_end` | 61 ms | 139 ms | | tenu |

Maximum mesuré de `resize` : 2 136 ms (échéance de 1,8 s plus la fin du pas en cours) ; un maximum au-dessus de 2 s ne touche pas le critère, qui porte sur le P90. Premier relevé : P50 170 ms, P90 1 112 ms.

**Ce que ce chiffre vaut.** Le P90 sous 2 s est **tenu**, mais il est contraint par les échéances de l'appel (1 s souple, 1,8 s budget) : il ne dit pas que la même tâche prend moins de temps. **Correction du premier relevé** :
« `no_economy` répond en un arbre ou deux » n'est pas porté par les chiffres ; les `no_economy` jouent six arbres (P50 69 ms sous `rbase` ; 232 ms et P90 508 ms au second relevé, où l'arbre libre lit bien davantage de `j`). Libérer la partie remplacée a allongé le P50 de `resize` (170 → 323 ms) sans toucher le P90 ; le
raccourcissement libre coûte un masque par arbre (`KeepOut::build` à chaque arbre). Avec les échéances levées (`engine16long`, premier relevé), le P90 restait à 1 273 ms (maximum 25,6 s) et K3 ne bougeait pas (64,4 %) : ce n'est pas l'échéance qui limite.

## 5. Les trois réglages tentés pour K3 (une itération) [mesuré]

Les pistes de la relecture, jouées **d'un bloc**, en une seule itération (le gain par réglage n'est donc pas isolé, sauf pour l'allongement, que `freesection` du premier relevé avait déjà isolé) :

| réglage | rôle |
|---|---|
| 1. `free_section = true` (défaut) | allonger : la partie remplacée n'est pas masquée |
| 2. `free_shorten = true` (défaut) | raccourcir : par arbre, ne sont masquées que les jambes d'avant `i` et d'après `j_max` ; un chemin lu qui reprend une jambe d'après `j` est refusé (C3 local) |
| 3. `shorten_read_tol = 0,03` (défaut ; était `pick_tol` = 0,01) | raccourcir : les `j` sont lus jusqu'à 3 %, triés par écart arrondi à 0,5 % puis part gardée |

| campagne | K3 pondéré | K3 brut | allonger | raccourcir | P90 |
|---|---|---|---|---|---|
| `engine16` (premier relevé) | 64,0 % | 58,4 % | 75,0 % | 41,8 % | 1 112 ms |
| `rbase` (moteur corrigé, comportement du premier relevé) | 64,1 % | 58,5 % | 75,2 % | 41,8 % | 1 071 ms |
| `rall` (réglages 1 à 3, tri des lectures sans arrondi) | 88,4 % | 76,9 % | 96,4 % | 57,4 % | 1 129 ms |
| **`final`** (réglages 1 à 3, tri arrondi à 0,5 %) | **89,0 % [87,5 ; 90,3]** | 76,7 % | 96,1 % | 57,2 % | 1 140 ms |

**Décision : gardée**, la campagne remesurée est meilleure (+25 points pondérés, sans dégrader K4 ni K2). Le tri des lectures par écart arrondi (de `rall` à `final`) corrige un défaut que le test `a_loop_with_a_lobe_is_shortened_by_exactly_the_lobe` a montré :
avec une lecture à 3 %, trier par `err₀` exact faisait préférer, à l'écart de 10⁻⁶ près, une paire qui garde moins du parcours que le lobe exact. La différence `rall` → `final` (88,4 → 89,0 %) est de l'ordre du bruit de la campagne (`rbase` contre `engine16` : 64,1 contre 64,0 %).

**Non tenté** (limite d'une itération) : 12 ou 18 arbres au lieu de 6 (`shorten_trees`, le raccourcissement ne coûte que P90 ≈ 500 ms), lecture des `j` à 3 % de la section et non de la distance totale (§3.1), recours à la lentille quand l'arbre rend `NoEconomy`
(en dernier recours : elle réussit 63,2 % au lot 0 à 100 m, mais ne descend pas sous 1,05 × le plus court chemin). Le calcul de la relecture (« avec l'allongement à ~96 %, K3 ne passe 90 % que si raccourcir dépasse ~84 % ») vaut pour le brut (76,7 % ici, moyenne de 57,2 et 96,1) ; **K3 pondéré atteint 89,0 % avec raccourcir à 57,2 %** [mesuré],
l'écart venant de la pondération par strate **[supposé : les strates où raccourcir réussit le mieux, boucles des grosses équipes, pèsent davantage ; non décomposé]**.

## 6. Ce qui échoue encore et pourquoi (par ordre de poids)

**`resize`, échecs de \|δ\| ≤ 25 % (1 074), moteur par défaut :**
1. **`no_economy`, 52,9 %** (568) : raccourcir un aller simple (516) ; les arbres ont tourné, aucune lecture à 3 % (§3.4).
2. **`all_refused`, 37,6 %** (404) : surtout des raccourcissements (388) dont le chemin lu reprend une jambe gardée d'après `j` (272, refus propre à la partie remplacée libre) ou que le contrôleur refuse à la distance de la
   section (74 sur 98). Les deux se travaillent : mieux choisir `j` selon les jambes d'après (masque par `j`, coûteux), lire à 3 % de la section.
3. **`timeout`, 3,0 %** : allonger, parcours de 200 km et plus.

**`move_end`, refus restants (1 248 sur 77 114) :** `no_seam_reached` 42,9 %, `REUSE_OUTSIDE_TAIL` 41,1 %, `CORRIDOR_DOUBLED` 5,6 %, `junction_cut` 3,2 %.

## 7. Limites de la mesure

1. **Dénominateurs différents du lot 0** : 183 traces refusées à l'entrée en moins ; `end_too_far` ×3,3 par le seuil de 500 m, qui ne se compare au lot 0 qu'avec `--end-snap-m` grand (§2.1).
2. **Pondération** : comme au lot 0, brut et pondéré diffèrent (76,7 contre 89,0 % ici) ; les deux sont donnés.
3. **Temps** : 16 fils sur 8 cœurs, pessimistes ; le 1 fil porte sur 10 traces par strate, pas sur la population. K2 à 1 fil (93,5 %) n'est pas comparable : échantillon de 10 traces par strate, non pondéré.
4. **K4 borné par les échéances** (§4) ; K4 du banc pour `--impl engine` est le temps de l'appel du moteur seul, `indep_ms` à part (note de conception §14).
5. **Continuité** : le banc ne rejoue pas l'invariant de continuité du lot 0 ni le suivi I5 sur la pose du moteur (§14 de la note). K1 n'est pas remesuré ici. Le déterminisme 1 fil contre 16 fils (même empreinte de recollés) n'a pas été rejoué dans le second relevé.
6. **`rbase` et `rall` sont des états intermédiaires** (§1) : ils attribuent le gain, ils ne sont pas rejouables à l'identique avec le binaire final (les réglages se retrouvent par `--masked-section --masked-shorten --read-tol 0,01`, au tri des lectures près).
7. **`--max-change 0,40`** au banc (le service garde 0,30) : les chiffres au-delà de 30 % sont hors du périmètre du produit.
8. **Contrôle du recollé** : jugé par position (`UTurn`) et par différence de comptes (croisements, couloir) depuis la relecture ; K2 est resté identique à l'unité (75 866 / 77 114), donc cette correction n'a pas changé les issues de `move_end`.
   Sur `resize`, elle n'est pas isolée des trois réglages.

## 8. Conclusion

| critère | lot 0 | lot 1, premier relevé | lot 1, second relevé (`final`) | verdict |
|---|---|---|---|---|
| arrivée déplacée sans refus du contrôleur, 95 % (K2) | 79,5 % (80,3 % à 100 m) | 98,4 % [98,3 ; 98,5] | **98,4 % [98,3 ; 98,5]** (98,2 % à dénominateur égal au lot 0 ; 92,2 % en comptant `end_too_far` à 500 m) | **tenu** (à dénominateur du banc) |
| distance à ±3 % pour \|δ\| ≤ 25 %, 90 % (K3) | 88,9 % pondéré (90,4 % à 100 m) | 64,0 % [61,8 ; 66,2] | **89,0 % [87,5 ; 90,3]** pondéré, 76,7 % brut | **à la limite** (sous 90 à la lettre, intervalle qui le contient) |
| P90 du temps sous 2 s (K4) | 4 060 ms (4,5 s avec contrôles) | 1 112 ms | **1 140 ms** (1 130 ms à 1 fil) | **tenu** (borné par les échéances) |

**`move_end` est prêt** : le cap d'arrivée a supprimé la cause du lot 0, et le seuil de 500 m (`end_too_far`) est une décision de produit à prendre. **`resize` allonge bien** (96,1 %, au niveau de la lentille du lot 0) **et raccourcit
moins bien que la lentille** (57,2 contre 63,2 %), surtout sur aller simple (38,9 %), où l'économie n'existe pas (§3.4). K3 est à la limite grâce à l'allongement ; il passerait avec un raccourcissement mieux lu (section, plus d'arbres) ou en renvoyant
le raccourcissement d'un aller simple à une autre opération (déplacer l'arrivée). Aucun commit, aucun contrat, aucun service.

## 9. Corrections apportées au moteur après la relecture (état du code, pas de mesure)

- **Diagnostics sur un refus** : `resize_in` et `move_end_in` rendent `Err(Box<Refused { failure, diagnostics }>)` ; `resize` et `move_end` (outils, tests) gardent `Err(RetouchFailure)`. Le banc lit les compteurs sur tout résultat.
  Diagnostics ajoutés au raccourcissement : `shorten_reached`, `shorten_turn_refused`, `shorten_head_mismatch`, `shorten_reuse_refused`, `tree_best_err0` (le meilleur écart `err₀` lu, par arbre).
- **Contrôle du recollé** (`check_spliced_in`) : `UTurn` jugé par indices comme `SharpTurn` (nouveau champ `Check::uturn_at`), `TooManyCrossings` par différence de comptes, `CorridorDoubled` par comparaison de la part doublée (`Check::corridor`) ; `seam_u_turn` vaut
  « un demi-tour nouveau touche une couture », plus « un `UTurn` quelque part ». Test `the_spliced_check_blames_the_new_section_only_for_what_it_brings`, cas `dirty_back` : un contexte qui a déjà un demi-tour plus un tronçon neuf qui fait demi-tour à la couture rendent `UTurn` et `seam_u_turn`.
- **Règle des grappes nommée** : `RetouchFailure::JunctionCut` (des sorties ou des chemins atteints, tous coupés) distinct de `NoSeamReached` (aucune sortie dans l'arbre) ; le banc écrit l'issue `junction_cut`. Test : le cas `cluster_graph` rend `JunctionCut`.
  Limite : les compteurs `junction_cut` restent à part de `spliced_refused` (pas de code `Violation` pour la règle des grappes).
- **Test** `a_refusal_keeps_what_the_search_saw` : un `NoEconomy` rend `trees > 0`, `settled > 0`, `shorten_reached > 0`.
