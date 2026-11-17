# Générateur de parcours — lot 4 (4a) : relevé du moteur 1.9.2 au banc de vraisemblance

> Relevé du 17 novembre 2026, binaire `release` du commit `45e6110` de la branche `feat/ord-loops-lot4` (banc
> `ord-loops-bench likeness`, relu ; moteur, contrôleur et service **inchangés** depuis 1.9.2), Rust 1.100.0, poste de
> développement (AMD Ryzen 7 4800H, 16 fils, 30 Go). Graphe et vues France `france-20261115-z11` ; surcouches
> reconstruites par le code du dépôt, identiques aux fichiers du lot 6. Conception : [`ord-loops-lot4.md`](ord-loops-lot4.md),
> §2 et §11. Sorties complètes, **privées et hors dépôt** : `~/.cache/osm-route-details/graph/lot4/campagne-report.json`
> et `campagne-summary.md`. Équipes désignées par les lettres A à I (lettrage par km du lot 2) ; ni départ, ni
> coordonnée, ni identifiant d'équipe, de trace ou d'arête ici.
>
> Marques : **[mesuré]** vient du rapport du banc ou d'un agrégat calculé sur son JSON (script jetable hors dépôt, mêmes
> boucles, mêmes jeux) ; **[calculé]** d'un calcul fait ici sur ces mesures ou sur un modèle géométrique. Sans marque
> dans un tableau : mesuré.

## 1. Protocole

- **Réel** : chaque trace de l'export privé (3 377 lignes) est appariée au graphe par `match_job` (le code du dépôt) ; sa
  **suite appariée ordonnée** devient une marche d'arêtes de base (§11.4 de la note). Retenue : non exclue, lisible,
  type `ROAD`, `GRAVEL` ou `MTB`, part appariée ≥ 0,95, plus long trou ≤ 1 000 m, D ≥ 5 km, arrivée à moins de 5 % de D
  du départ. Les mesures portent sur la polyligne de la marche (même graphe, mêmes longueurs que le généré).
- **Généré** : pour chaque boucle retenue, une requête au moteur en processus : même départ, D arrondie au km, même type,
  cap du départ vers le barycentre de la trace arrondi à 45°, graine propre à la trace. Deux curseurs par la correspondance
  du service (`cursor::apply`) : **0,25** (équipe d'abord, λ = 2,5, familiarité 0,5) et **1** (découverte). Parcours gardés
  = ceux que le contrôleur indépendant accepte, dans l'ordre du moteur. **Premier** = le premier gardé ; **`best_shape`** =
  le gardé le plus proche du réel sur r / D, `far_pos` et compacité ; **`best_recovered`** = le gardé qui retrouve le plus
  de mètres de la trace.
- **Distributions** : quantiles P10 à P90 (type 7), W1 exacte normalisée par l'écart interquartile du réel, Δ médian
  apparié (généré − réel de la même boucle, jeux `OK`).
- **Reconstruction** : par équipe d'au moins 10 boucles retenues, 10 % mises à l'écart par l'empreinte de l'identifiant
  de trace ; surcouche refaite sans elles ; chaque boucle mise à l'écart est rejouée sur cette surcouche.
- **Commandes** (depuis le dépôt, `B=./target/release/ord-loops-bench`) :
  1. `$B likeness --calibrate-only --label calib`, puis `diff` avec `python3 -I harness/shapes.py data/prod-network.csv.gz`
     tronqué au même nombre de lignes : **identique** ;
  2. essai court `--teams C --limit 5` et déterminisme `--teams B --threads 1` / `--threads 16` : faits à la relecture
     sur le même commit (empreintes égales, `5f04c040f3d10bf4`), non refaits ;
  3. campagne : `$B likeness --label campagne` (toutes les équipes, curseurs 0,25 et 1, reconstruction).
- **Durée** : 231 s de banc (3 min 54 s au total), dont 210 s de jeux sur 16 fils. **Mémoire** : pic anonyme 14,2 Go
  (phase des jeux ; 7,1 Go à l'appariement), RSS maximal 17,5 Go avec les pages du graphe. Charge sur 1 minute de 1,0 au
  début à 11,9 à la fin (le banc occupe la machine).

## 2. Périmètre

### 2.1 Retenue par équipe

| équipe | lignes | part appariée < 0,95 | trou > 1 km | < 5 km | pas une boucle | **retenues** | ROAD | GRAVEL | MTB | km retenus | mises à l'écart |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A | 2 726 | 23 | 103 | 3 | 191 | **2 406** | 1 860 | 545 | 1 | 149 493 | 241 |
| B | 446 | 2 | 1 | 1 | 45 | **397** | 167 | 228 | 2 | 18 261 | 40 |
| C | 108 | 27 | 6 | 1 | 47 | **27** | 21 | 5 | 1 | 3 273 | 3 |
| D | 71 | 7 | 8 | 1 | 44 | **11** | 11 | 0 | 0 | 1 522 | 2 |
| E | 10 | 0 | 0 | 0 | 1 | **9** | 9 | 0 | 0 | 1 847 | 0 |
| F | 1 | 0 | 0 | 0 | 0 | **1** | 1 | 0 | 0 | 132 | 0 |
| G | 1 | 0 | 0 | 0 | 1 | **0** | 0 | 0 | 0 | 0 | 0 |
| H | 7 | 0 | 4 | 0 | 1 | **2** | 1 | 1 | 0 | 103 | 0 |
| I | 7 | 0 | 4 | 0 | 1 | **2** | 1 | 1 | 0 | 103 | 0 |
| **total** | **3 377** | | | | | **2 855** | **2 071** | **780** | **4** | **174 734** | **286** |

Aucune trace exclue, illisible ou de type refusé. **A fait 85,6 % des kilomètres retenus et 84 % des boucles, B 10,5 %**
[calculé] : les distributions « toutes équipes » sont celles de A, et seules A et B dépassent 30 boucles d'un type. H et
I ont les **mêmes** traces (mêmes boucles, même surcouche de 2 264 arêtes) : un doublon dans les données, sans effet sur
les conclusions. **3 boucles seulement sur 2 855 ont une marche continue par nœuds** (C, H, I) : la mesure `crossings` du
réel et la section du contrôleur sur le réel sont vides (§6).

### 2.2 Surcouches

Les neuf surcouches refaites par `Deposit::push_matched` sont **identiques**, arête par arête et compte par compte, aux
fichiers du lot 6. Les surcouches « écart » de A, B, C, D perdent 3 220, 773, 435 et 296 arêtes.

## 3. Sommets, étalonnage, et l'incohérence du §1

### 3.1 Étalonnage

`shapes.py` porté en Rust sur l'export brut (`raw_legacy`) rend **la même sortie au caractère près** que le script. Sur
toute l'export : 2 967 boucles ; r / D P10 à P90 0,249 · 0,285 · **0,322** · 0,356 · 0,380 ; compacité 0,155 · 0,219 ·
**0,300** · 0,385 · 0,481 ; sommets (Douglas-Peucker 5 % de D, règle du script) : 1 → 1,5 %, **2 → 27,7 %, 3 → 40,0 %,
4 → 24,7 %**, 5 → 5,5 %, 6 → 0,5 %. Ce sont les chiffres du plan (§2) : le banc les retrouve.

### 3.2 Les quatre comptes sur les boucles retenues

Parts des boucles, plafonnées à 4 pour la lecture [mesuré] :

| type | compte | 1 | 2 | 3 | ≥ 4 |
|---|---|---:|---:|---:|---:|
| ROAD (2 071) | `raw_legacy` (brut, règle du script) | 1,4 % | 25,1 % | 40,4 % | 33,0 % |
| ROAD | `vertices_legacy` (apparié, règle du script) | 1,6 % | 26,3 % | 40,2 % | 31,9 % |
| ROAD | `vertices` (apparié, définition figée, 5 %) | 1,7 % | 26,3 % | 45,6 % | 26,4 % |
| ROAD | **`vertices_10`** (définition figée, 10 %) | **40,5 %** | **53,0 %** | **6,5 %** | 0,0 % |
| GRAVEL (780) | `raw_legacy` | 1,7 % | 33,6 % | 39,4 % | 25,3 % |
| GRAVEL | `vertices_legacy` | 1,7 % | 36,7 % | 40,5 % | 20,8 % |
| GRAVEL | `vertices` | 2,7 % | 39,9 % | 39,6 % | 17,8 % |
| GRAVEL | **`vertices_10`** | **55,4 %** | **42,9 %** | **1,7 %** | 0,0 % |

(La règle du script compte aussi des boucles GRAVEL à 0 sommet, 1 en brut et 3 en apparié : sa corde est alors la droite
du trajet.)

- **Effet des données** (brut contre apparié, même règle) : faible, 1 à 4 points par classe.
- **Effet de la définition** (droite contre départ, à 5 %) : faible, quelques points.
- **Effet du seuil** (5 % contre 10 %) : il **décide tout**. À 5 %, une boucle réelle a 2 ou 3 sommets dans 65 à 80 %
  des cas ; à 10 %, 1 ou 2 dans 93 à 98 %.

Sur le généré (premier parcours, curseur 0,25 ; le curseur 1 et l'ensemble des gardés donnent les mêmes parts à 2 points
près) : `vertices` 1 → 10 %, **2 → 64 %**, 3 → 24 %, ≥ 4 → 2 % en ROAD, 15 / 64 / 18 / 2 % en GRAVEL ; **`vertices_10` 1 →
91 %**, 2 → 9 %, 3 → 0 % dans les deux types [mesuré]. Le moteur ne fait que des lentilles (famille `Turnaround`, 7 647
parcours sur 7 647 gardés au curseur 0,25) : **à 5 % une lentille compte 2 sommets, à 10 % elle en compte 1**.

### 3.3 Réponse à l'incohérence du §1

Le plan comptait 2 = lentille (28 %), 3 = triangle (40 %), 4 = quadrilatère (25 %) avec la règle du script à 5 % de D.
Le relevé montre que **ce compte à 5 % ne sépare pas les figures** : une lentille du moteur y vaut 2 dans 64 % des cas et
3 dans 24 % (un sommet par côté dont la flèche dépasse 5 % de D, §11.0 point 3 de la note). Le décalage d'un sommet entre
le §2 et le §5.2 du plan n'est donc pas un point gardé en trop : c'est la flèche des côtés de la lentille.

**Définition proposée pour le tirage du §4.1 : `vertices_10`** = nombre de points lointains (Douglas-Peucker sur la
polyligne ouverte au départ, premier point = le plus éloigné du départ, seuil 10 % de D). Elle classe 91 % des parcours du
moteur en lentille (ce qu'ils sont) et rend, sur le réel :

| type | k = 1 (lentille) | k = 2 (triangle) | k = 3 (quadrilatère) |
|---|---:|---:|---:|
| ROAD, toutes équipes | 40,5 % | 53,0 % | 6,5 % |
| ROAD, A (1 860) | 39 % | 55 % | 7 % |
| ROAD, B (167) | 53 % | 44 % | 4 % |
| GRAVEL, toutes équipes | 55,4 % | 42,9 % | 1,7 % |
| GRAVEL, A (545) | 54 % | 44 % | 1 % |
| GRAVEL, B (228) | 56 % | 42 % | 2 % |

au lieu des 28 / 40 / 25 % du plan [mesuré]. Le quadrilatère n'est presque pas une figure réelle (≤ 7 %).

## 4. Distributions, réel contre généré

Toutes équipes (donc surtout A), surcouche complète. « Premier » : P10 · P25 · P50 · P75 · P90 du premier parcours
gardé. W1 / IQR : 0 = mêmes distributions, 1 = décalage moyen d'un écart interquartile du réel. Le VTT (4 boucles) est
dans le résumé privé, indicatif, et ne change rien ci-dessous.

### 4.1 Forme et classes de voie

#### ROAD, forme

n réel = 2071 ; `OK` : 2038 au curseur 0,25 (98,4 %), 2042 au curseur 1 (98,6 %).

| mesure | réel P10 · P25 · P50 · P75 · P90 | premier, curseur 0,25 | W1 / IQR | Δ médian | premier, curseur 1 | W1 / IQR | Δ médian | `best_shape` W1 / IQR (0,25 · 1) |
|---|---|---|---:|---:|---|---:|---:|---:|
| r_over_d | 0,257 · 0,295 · 0,329 · 0,359 · 0,382 | 0,369 · 0,378 · 0,390 · 0,402 · 0,410 | 1,032 | 0,059 | 0,370 · 0,381 · 0,393 · 0,402 · 0,410 | 1,047 | 0,059 | 0,955 · 0,986 |
| far_pos | 0,421 · 0,463 · 0,502 · 0,543 · 0,589 | 0,468 · 0,479 · 0,490 · 0,504 · 0,515 | 0,491 | -0,013 | 0,469 · 0,480 · 0,491 · 0,504 · 0,517 | 0,492 | -0,011 | 0,480 · 0,482 |
| compactness | 0,177 · 0,236 · 0,320 · 0,404 · 0,501 | 0,117 · 0,161 · 0,216 · 0,297 · 0,358 | 0,579 | -0,097 | 0,115 · 0,159 · 0,212 · 0,287 · 0,349 | 0,609 | -0,101 | 0,441 · 0,453 |
| vertices | 2,000 · 2,000 · 3,000 · 4,000 · 4,000 | 1,000 · 2,000 · 2,000 · 3,000 · 3,000 | 0,418 | -1,000 | 1,000 · 2,000 · 2,000 · 2,000 · 3,000 | 0,438 | -1,000 | 0,359 · 0,378 |
| vertices_10 | 1,000 · 1,000 · 2,000 · 2,000 · 2,000 | 1,000 · 1,000 · 1,000 · 1,000 · 1,000 | 0,569 | -1,000 | 1,000 · 1,000 · 1,000 · 1,000 · 1,000 | 0,567 | -1,000 | 0,547 · 0,542 |
| turns_per_10km | 7,870 · 9,304 · 11,504 · 14,757 · 19,139 | 9,885 · 11,457 · 13,565 · 15,915 · 17,999 | 0,394 | 1,833 | 10,378 · 11,990 · 14,113 · 16,352 · 18,261 | 0,459 | 2,440 | 0,415 · 0,492 |
| shared_share | 0,000 · 0,000 · 0,002 · 0,005 · 0,012 | 0,001 · 0,001 · 0,003 · 0,008 · 0,014 | 0,762 | 0,001 | 0,001 · 0,001 · 0,003 · 0,008 · 0,012 | 0,804 | 0,001 | 0,770 · 0,853 |

#### ROAD, classes

n réel = 2071 ; `OK` : 2038 au curseur 0,25 (98,4 %), 2042 au curseur 1 (98,6 %).

| mesure | réel P10 · P25 · P50 · P75 · P90 | premier, curseur 0,25 | W1 / IQR | Δ médian | premier, curseur 1 | W1 / IQR | Δ médian | `best_shape` W1 / IQR (0,25 · 1) |
|---|---|---|---:|---:|---|---:|---:|---:|
| way_major | 0,093 · 0,161 · 0,244 · 0,339 · 0,430 | 0,046 · 0,073 · 0,104 · 0,150 · 0,220 | 0,772 | -0,125 | 0,035 · 0,059 · 0,094 · 0,140 · 0,199 | 0,845 | -0,143 | 0,789 · 0,832 |
| way_tertiary | 0,207 · 0,307 · 0,418 · 0,532 · 0,614 | 0,166 · 0,243 · 0,314 · 0,424 · 0,510 | 0,378 | -0,090 | 0,143 · 0,214 · 0,282 · 0,380 · 0,477 | 0,510 | -0,121 | 0,405 · 0,562 |
| way_minor | 0,026 · 0,073 · 0,140 · 0,229 · 0,312 | 0,109 · 0,169 · 0,265 · 0,343 · 0,423 | 0,659 | 0,098 | 0,118 · 0,194 · 0,278 · 0,361 · 0,424 | 0,757 | 0,115 | 0,773 · 0,864 |
| way_street | 0,018 · 0,036 · 0,067 · 0,115 · 0,180 | 0,059 · 0,077 · 0,108 · 0,148 · 0,204 | 0,456 | 0,037 | 0,067 · 0,088 · 0,124 · 0,160 · 0,210 | 0,592 | 0,048 | 0,466 · 0,581 |
| way_cycle | 0,003 · 0,010 · 0,033 · 0,079 · 0,151 | 0,062 · 0,105 · 0,157 · 0,218 · 0,274 | 1,559 | 0,106 | 0,067 · 0,114 · 0,171 · 0,242 · 0,301 | 1,816 | 0,120 | 1,448 · 1,735 |
| way_path | 0,000 · 0,001 · 0,002 · 0,009 · 0,047 | 0,000 · 0,000 · 0,000 · 0,000 · 0,003 | 3,145 | -0,002 | 0,000 · 0,000 · 0,000 · 0,000 · 0,004 | 3,143 | -0,002 | 3,123 · 3,104 |
| surf_paved | 0,537 · 0,643 · 0,738 · 0,818 · 0,884 | 0,510 · 0,604 · 0,681 · 0,788 · 0,862 | 0,213 | -0,040 | 0,494 · 0,593 · 0,677 · 0,777 · 0,840 | 0,256 | -0,052 | 0,253 · 0,292 |
| surf_presumed | 0,108 · 0,172 · 0,252 · 0,345 · 0,430 | 0,138 · 0,212 · 0,319 · 0,396 · 0,490 | 0,296 | 0,050 | 0,160 · 0,223 · 0,323 · 0,407 · 0,506 | 0,355 | 0,059 | 0,337 · 0,389 |
| surf_unpaved | 0,000 · 0,000 · 0,000 · 0,002 · 0,028 | 0,000 · 0,000 · 0,000 · 0,000 · 0,000 | 10,324 | 0,000 | 0,000 · 0,000 · 0,000 · 0,000 · 0,000 | 10,324 | 0,000 | 10,324 · 10,324 |
| marked | 0,068 · 0,103 · 0,163 · 0,242 · 0,334 | 0,147 · 0,188 · 0,249 · 0,331 · 0,403 | 0,577 | 0,081 | 0,159 · 0,200 · 0,259 · 0,333 · 0,422 | 0,642 | 0,092 | 0,492 · 0,624 |

#### GRAVEL, forme

n réel = 780 ; `OK` : 758 au curseur 0,25 (97,2 %), 758 au curseur 1 (97,2 %).

| mesure | réel P10 · P25 · P50 · P75 · P90 | premier, curseur 0,25 | W1 / IQR | Δ médian | premier, curseur 1 | W1 / IQR | Δ médian | `best_shape` W1 / IQR (0,25 · 1) |
|---|---|---|---:|---:|---|---:|---:|---:|
| r_over_d | 0,220 · 0,253 · 0,285 · 0,312 · 0,338 | 0,358 · 0,373 · 0,387 · 0,402 · 0,410 | 1,760 | 0,101 | 0,359 · 0,377 · 0,391 · 0,402 · 0,411 | 1,811 | 0,105 | 1,564 · 1,633 |
| far_pos | 0,412 · 0,458 · 0,504 · 0,546 · 0,602 | 0,469 · 0,483 · 0,494 · 0,506 · 0,517 | 0,483 | -0,010 | 0,468 · 0,479 · 0,489 · 0,501 · 0,512 | 0,509 | -0,012 | 0,477 · 0,493 |
| compactness | 0,113 · 0,159 · 0,226 · 0,288 · 0,345 | 0,102 · 0,139 · 0,186 · 0,231 · 0,272 | 0,315 | -0,032 | 0,128 · 0,159 · 0,196 · 0,246 · 0,295 | 0,212 | -0,023 | 0,198 · 0,191 |
| vertices | 2,000 · 2,000 · 3,000 · 3,000 · 4,000 | 1,000 · 2,000 · 2,000 · 2,000 · 3,000 | 0,683 | -1,000 | 1,000 · 2,000 · 2,000 · 2,000 · 3,000 | 0,689 | -1,000 | 0,522 · 0,576 |
| vertices_10 | 1,000 · 1,000 · 1,000 · 2,000 · 2,000 | 1,000 · 1,000 · 1,000 · 1,000 · 1,000 | 0,368 | 0,000 | 1,000 · 1,000 · 1,000 · 1,000 · 1,000 | 0,384 | 0,000 | 0,277 · 0,301 |
| turns_per_10km | 18,219 · 23,765 · 27,635 · 31,184 · 34,263 | 12,647 · 13,977 · 15,712 · 18,059 · 20,755 | 1,479 | -11,364 | 13,177 · 14,561 · 17,343 · 19,910 · 21,797 | 1,318 | -10,030 | 1,447 · 1,288 |
| shared_share | 0,000 · 0,002 · 0,007 · 0,017 · 0,030 | 0,001 · 0,002 · 0,005 · 0,011 · 0,020 | 0,582 | -0,001 | 0,001 · 0,001 · 0,004 · 0,012 · 0,017 | 0,614 | -0,001 | 0,570 · 0,579 |

#### GRAVEL, classes

n réel = 780 ; `OK` : 758 au curseur 0,25 (97,2 %), 758 au curseur 1 (97,2 %).

| mesure | réel P10 · P25 · P50 · P75 · P90 | premier, curseur 0,25 | W1 / IQR | Δ médian | premier, curseur 1 | W1 / IQR | Δ médian | `best_shape` W1 / IQR (0,25 · 1) |
|---|---|---|---:|---:|---|---:|---:|---:|
| way_major | 0,015 · 0,026 · 0,043 · 0,066 · 0,092 | 0,007 · 0,013 · 0,030 · 0,055 · 0,095 | 0,348 | -0,011 | 0,009 · 0,013 · 0,025 · 0,045 · 0,071 | 0,500 | -0,016 | 0,509 · 0,579 |
| way_tertiary | 0,038 · 0,059 · 0,085 · 0,122 · 0,177 | 0,127 · 0,164 · 0,225 · 0,304 · 0,352 | 2,147 | 0,131 | 0,063 · 0,121 · 0,181 · 0,252 · 0,321 | 1,444 | 0,082 | 1,959 · 1,419 |
| way_minor | 0,034 · 0,059 · 0,097 · 0,142 · 0,203 | 0,073 · 0,120 · 0,188 · 0,252 · 0,313 | 0,958 | 0,079 | 0,066 · 0,107 · 0,176 · 0,259 · 0,318 | 0,916 | 0,068 | 1,120 · 0,904 |
| way_street | 0,076 · 0,103 · 0,145 · 0,201 · 0,289 | 0,097 · 0,130 · 0,169 · 0,212 · 0,264 | 0,207 | 0,014 | 0,096 · 0,135 · 0,172 · 0,216 · 0,282 | 0,222 | 0,018 | 0,189 · 0,219 |
| way_cycle | 0,065 · 0,095 · 0,134 · 0,190 · 0,260 | 0,041 · 0,082 · 0,136 · 0,289 · 0,370 | 0,463 | 0,008 | 0,045 · 0,101 · 0,177 · 0,292 · 0,399 | 0,641 | 0,037 | 0,525 · 0,838 |
| way_path | 0,214 · 0,312 · 0,447 · 0,536 · 0,592 | 0,054 · 0,098 · 0,166 · 0,233 · 0,330 | 1,085 | -0,254 | 0,086 · 0,138 · 0,205 · 0,267 · 0,323 | 0,982 | -0,226 | 0,990 · 0,976 |
| surf_paved | 0,281 · 0,326 · 0,400 · 0,493 · 0,568 | 0,400 · 0,487 · 0,570 · 0,659 · 0,742 | 0,948 | 0,160 | 0,401 · 0,457 · 0,528 · 0,613 · 0,689 | 0,751 | 0,125 | 0,752 · 0,740 |
| surf_presumed | 0,182 · 0,243 · 0,326 · 0,411 · 0,493 | 0,121 · 0,242 · 0,329 · 0,400 · 0,466 | 0,103 | -0,005 | 0,183 · 0,279 · 0,361 · 0,437 · 0,501 | 0,159 | 0,023 | 0,134 · 0,108 |
| surf_unpaved | 0,119 · 0,181 · 0,253 · 0,325 · 0,376 | 0,033 · 0,052 · 0,099 · 0,143 · 0,215 | 0,992 | -0,145 | 0,032 · 0,062 · 0,104 · 0,136 · 0,179 | 1,004 | -0,145 | 0,908 · 0,932 |
| marked | 0,197 · 0,246 · 0,311 · 0,392 · 0,478 | 0,166 · 0,231 · 0,298 · 0,398 · 0,486 | 0,099 | -0,005 | 0,159 · 0,224 · 0,289 · 0,375 · 0,512 | 0,184 | -0,021 | 0,055 · 0,085 |

`surf_unpaved` en ROAD et `way_path` en ROAD ont un écart interquartile réel de 0,002 et 0,008 : leurs W1 / IQR élevées
ne disent qu'une absence (le moteur ne prend aucun chemin en route), sans poids réel.

### 4.2 Lecture

- **r / D est l'écart principal de forme**, aux deux curseurs et dans les deux types : P50 généré 0,387 à 0,393 contre
  0,329 (ROAD) et 0,285 (GRAVEL) ; W1 / IQR 1,03 à 1,05 en ROAD, 1,76 à 1,81 en GRAVEL ; 96 % (ROAD) et 99,6 à 99,9 %
  (GRAVEL) des premiers parcours sont **au-dessus du P75 réel**. Parmi les trois, `best_shape` ne fait guère mieux (0,96
  et 1,56 à 1,63) : **le moteur ne sait pas proposer un r / D réel**. Le point lointain est à mi-parcours chez les deux
  (`far_pos` P50 0,49 contre 0,50), mais le réel est bien plus dispersé (IQR 0,08 contre 0,025).
- **Compacité** : ROAD généré 0,212 à 0,216 contre 0,320 réel, **sous le P25 réel (0,236)**, Δ apparié −0,10 ; GRAVEL
  0,186 à 0,196 contre 0,226, **dans** l'intervalle P25–P75, Δ −0,02 à −0,03. Le 0,30 du plan est une valeur ROAD : le
  réel GRAVEL est à 0,226.
- **Boîte r / D × compacité** : 26,5 % (ROAD) et 23,7 % (GRAVEL) des boucles réelles sont dans [P25, P75] des deux
  mesures à la fois ; 0,6 % et 0,3 % des premiers parcours au curseur 0,25, 2,1 % et 0,4 % pour « l'un des trois »
  [mesuré].
- **Virages** : en ROAD le moteur tourne **plus** que le réel (13,6 à 14,1 contre 11,5 aux 10 km), en GRAVEL **bien
  moins** (15,7 à 17,3 contre 27,6 ; W1 / IQR 1,3 à 1,5).
- **Classes, ROAD** : le moteur évite les grandes routes (P50 0,10 contre 0,24, W1 / IQR 0,77 à 0,85) et la
  `tertiary` (0,28 à 0,31 contre 0,42), et prend à leur place petites routes (0,27 contre 0,14), voies cyclables ou vertes
  (0,16 à 0,17 contre 0,03, W1 / IQR 1,6 à 1,8) et itinéraires balisés (0,25 contre 0,16).
- **Classes, GRAVEL** : le moteur prend **deux à trois fois moins de chemins** (0,17 à 0,21 contre 0,45, Δ −0,23 à
  −0,25) et de non-revêtu connu (0,10 contre 0,25), et plus du double de `tertiary` (0,18 à 0,23 contre 0,085, W1 / IQR
  1,4 à 2,1). Le balisage est juste (W1 / IQR 0,10 à 0,18).
- **Recouvrement** : faible des deux côtés (P50 0,002 à 0,007), écart sans portée.
- **Curseur** : 0,25 et 1 donnent presque les mêmes formes (r / D à 0,004 près, compacité à 0,01 près) ; la découverte
  déplace un peu les classes (moins de grandes routes et de `tertiary` en ROAD). Raison au §7.

### 4.3 r / D à figure égale [mesuré, puis calculé]

Médianes par nombre de points lointains (`vertices_10`), premier parcours au curseur 0,25 :

| type | k | réel : n, r / D, compacité | généré : n, r / D, compacité |
|---|---:|---|---|
| ROAD | 1 | 838 · 0,353 · 0,243 | 1 851 · 0,392 · 0,211 |
| ROAD | 2 | 1 098 · 0,318 · 0,356 | 186 · 0,372 · 0,294 |
| ROAD | 3 | 135 · 0,285 · 0,470 | 1 · 0,317 · 0,560 |
| GRAVEL | 1 | 432 · 0,300 · 0,189 | 687 · 0,388 · 0,181 |
| GRAVEL | 2 | 335 · 0,267 · 0,275 | 70 · 0,361 · 0,237 |

**À figure égale, le r / D généré reste trop haut** : +0,04 sur les lentilles ROAD, +0,09 sur les lentilles GRAVEL, à
compacité voisine. Si le moteur avait le r / D réel de chaque figure **avec sa propre répartition** (91 % de lentilles),
son P50 serait d'environ 0,350 en ROAD et 0,297 en GRAVEL [calculé] : la répartition des figures explique **0,02 des 0,06**
d'écart en ROAD et **0,01 des 0,10** en GRAVEL ; le reste vient du chemin lui-même.

Le **détour jusqu'au point lointain** (abscisse du point lointain / distance à vol d'oiseau, `far_pos / (r / D)`) le dit
directement : médiane **1,25 à 1,28** pour le généré dans les deux types et aux deux curseurs, contre **1,41** (lentilles
ROAD), 1,49 (ROAD), **1,66** (lentilles GRAVEL) et 1,72 (GRAVEL) pour le réel [calculé sur le JSON]. L'aller du moteur
est le chemin le moins coûteux vers T ; les équipes, elles, n'y vont pas tout droit, surtout en gravel (chemins, virages,
§4.2).

Modèle grossier [calculé] : une lentille de deux arcs de cercle sur une corde c, de flèche s, a r / D = 0,497 et une
compacité de 0,21 pour s / c = 0,05, r / D = 0,472 et 0,57 pour s / c = 0,15. **Élargir la lentille (le couloir) monte
la compacité mais ne baisse presque pas r / D** (−5 % pour une compacité multipliée par 2,7). Un triangle partant d'un
sommet, à 30 à 45° au départ, a r / D = 0,36 à 0,40 et une compacité de 0,50 à 0,58 : sur le papier, la figure baisse r / D de 20
à 27 % ; sur le réel, triangles et lentilles ne diffèrent que de 10 % (0,318 contre 0,353 en ROAD), et la répartition
des figures ne porte qu'un tiers de l'écart ROAD (ci-dessus).

## 5. Par équipe

Réel P50 / premier P50 / W1 / IQR, équipes d'au moins 30 boucles du type ; « autres » = C à I réunies.

| type | curseur | équipe | n | r / D | compacité | `vertices_10` | virages / 10 km |
|---|---:|---|---:|---|---|---|---|
| ROAD | 0,25 | A | 1 860 | 0,331 / 0,390 / 1,05 | 0,327 / 0,210 / 0,65 | 2 / 1 / 0,60 | 11,3 / 13,8 / 0,46 |
| ROAD | 0,25 | B | 167 | 0,311 / 0,394 / 1,14 | 0,256 / 0,246 / 0,13 | 1 / 1 / 0,35 | 16,0 / 12,6 / 0,57 |
| ROAD | 0,25 | autres | 44 | 0,347 / 0,373 / 0,43 | 0,227 / 0,246 / 0,17 | 1 / 1 / 0,38 | 8,1 / 8,9 / 0,31 |
| ROAD | 1 | A | 1 860 | 0,331 / 0,393 / 1,06 | 0,327 / 0,208 / 0,67 | 2 / 1 / 0,59 | 11,3 / 14,5 / 0,55 |
| ROAD | 1 | B | 167 | 0,311 / 0,388 / 1,11 | 0,256 / 0,230 / 0,24 | 1 / 1 / 0,39 | 16,0 / 12,2 / 0,60 |
| ROAD | 1 | autres | 44 | 0,347 / 0,392 / 0,66 | 0,227 / 0,200 / 0,21 | 1 / 1 / 1,08 | 8,1 / 9,5 / 0,39 |
| GRAVEL | 0,25 | A | 545 | 0,286 / 0,390 / 1,95 | 0,243 / 0,185 / 0,43 | 1 / 1 / 0,37 | 28,4 / 16,6 / 1,98 |
| GRAVEL | 0,25 | B | 228 | 0,280 / 0,375 / 1,33 | 0,200 / 0,193 / 0,19 | 1 / 1 / 0,39 | 23,7 / 14,1 / 0,83 |
| GRAVEL | 1 | A | 545 | 0,286 / 0,396 / 2,03 | 0,243 / 0,195 / 0,28 | 1 / 1 / 0,38 | 28,4 / 18,3 / 1,71 |
| GRAVEL | 1 | B | 228 | 0,280 / 0,375 / 1,33 | 0,200 / 0,199 / 0,16 | 1 / 1 / 0,41 | 23,7 / 14,4 / 0,81 |

(GRAVEL « autres » : 7 boucles, omises.) **Le défaut de compacité est celui de A** : en ROAD, B est juste (0,246 contre
0,256, W1 / IQR 0,13) et A ne l'est pas (0,210 contre 0,327, 0,65). **Le défaut de r / D est commun** à A et B, dans les
deux types (Δ apparié de r / D au curseur 0,25 : A +0,070, B +0,087 ; C à I de +0,004 à +0,114 sur quelques boucles).

## 6. Le contrôleur sur les boucles réelles

3 boucles continues sur 2 855 (0,1 %) : toutes enfreignent `DEAD_END` et `NOT_ADMITTED`, deux `REUSE_OUTSIDE_TAIL` et
`SHARP_TURN` [mesuré]. Trois boucles ne disent rien des règles. La continuité manque parce que les géométries non
appariées laissent des trous (relecture : ~10 trous par boucle en C) et que deux nœuds distincts à moins de 5 m ne sont
jamais reliés par une arête commune ; combler les trous par un plus court chemin pour le seul contrôleur est une décision
laissée ouverte (§10).

## 7. Reconstruction par équipe

Part agrégée (Σ mètres retrouvés / Σ mètres réels) / médiane par trace ; « écart » = surcouche sans la boucle, « dans »
= surcouche complète, mêmes boucles. C et D : 2 jeux `OK` chacun, indicatifs ; E à I : moins de 10 boucles, pas de
reconstruction.

| équipe | à l'écart (`OK`) | curseur | premier, total | meilleur, total | meilleur, arêtes d'équipe | meilleur, arêtes qu'aucune autre trace ne prend | meilleur, total dans l'échantillon | jumelles ≥ 0,9 | meilleur, total sans les jumelles |
|---|---|---:|---|---|---|---|---|---:|---|
| A | 241 (238) | 0,25 | 0,107 / 0,089 | 0,153 / 0,134 | 0,157 / 0,140 | 0,123 / 0,000 | 0,150 / 0,134 | 59 | 0,149 / 0,131 |
| A | | 1 | 0,093 / 0,078 | 0,132 / 0,125 | 0,137 / 0,132 | 0,024 / 0,000 | 0,132 / 0,125 | 59 | 0,133 / 0,125 |
| B | 40 (39) | 0,25 | 0,112 / 0,074 | 0,164 / 0,135 | 0,181 / 0,156 | 0,072 / 0,000 | 0,162 / 0,132 | 5 | 0,155 / 0,131 |
| B | | 1 | 0,125 / 0,077 | 0,164 / 0,118 | 0,182 / 0,139 | 0,059 / 0,000 | 0,164 / 0,118 | 5 | 0,159 / 0,116 |
| C | 3 (2) | 0,25 | 0,352 / 0,436 | 0,352 / 0,436 | 0,440 / 0,491 | 0,000 / 0,000 | 0,622 / 0,649 | 1 | 0,197 / 0,197 |
| C | | 1 | 0,091 / 0,094 | 0,118 / 0,114 | 0,139 / 0,133 | 0,029 / 0,017 | 0,118 / 0,114 | 1 | 0,127 / 0,127 |
| D | 2 (2) | 0,25 | 0,587 / 0,534 | 0,587 / 0,534 | 0,694 / 0,628 | 0,018 / 0,028 | 0,659 / 0,598 | 0 | 0,587 / 0,534 |
| D | | 1 | 0,098 / 0,115 | 0,099 / 0,116 | 0,117 / 0,135 | 0,000 / 0,000 | 0,099 / 0,116 | 0 | 0,099 / 0,116 |

Ce que le tableau établit [mesuré] :

- **La lecture « arêtes qu'aucune autre trace ne prend » est vide pour A et B.** Ces arêtes font **2,1 %** des mètres des
  boucles mises à l'écart de A et **5,5 %** de B (médiane par trace : 0,000 et 0,008) ; 13 % et 15 % pour C et D, sur deux
  boucles. Les 0,123 et 0,072 de la colonne portent sur quelques centaines de mètres par boucle.
- **La surcouche ne fait presque pas retrouver une trace donnée.** A et B retrouvent 13 à 16 % des mètres (meilleur des
  trois), et **autant avec la trace dans la surcouche que sans elle** (A 0,150 contre 0,153, B 0,162 contre 0,164) ;
  l'équipe d'abord ne fait guère mieux que la découverte (A 0,153 contre 0,132, B 0,164 et 0,164).
- La raison est au §7.1 : **le réseau de A et B couvre presque toutes les routes autour de leurs départs.** Même en
  découverte, où le moteur ignore l'équipe, le premier parcours est à **93,8 %** (P50 ; P10 0,829) sur des arêtes
  d'équipe ; au curseur 0,25, à 99,3 %. Par équipe et par type, en découverte : A ROAD 0,954, A GRAVEL 0,903, B ROAD
  0,928, B GRAVEL 0,948.
- **Jumelles** : 65 des 286 boucles mises à l'écart (A 59, B 5, C 1) ont dans leur équipe une autre boucle retenue qui
  prend au moins 90 % de leurs mètres (sortie de groupe enregistrée plusieurs fois). Les retirer change peu pour A et
  B (0,149 contre 0,153 et 0,155 contre 0,164, meilleur des trois au curseur 0,25) ; pour C, la seule jumelle fait toute la différence (0,197 contre 0,352).

### 7.1 Pourquoi les curseurs se ressemblent

Le service passe la familiarité 0,5 aux deux curseurs ; à 0,5 la table des multiplicateurs `M[z]` vaut 256 partout
(`cost::multiplier_table`) : **une arête d'équipe coûte `w = len × K(classe)`, quel que soit le nombre de passages**.
Au curseur 0,25 seules les arêtes hors équipe paient λ = 2,5 ; en découverte, rien ne distingue l'équipe. Comme 90 à 95 %
des routes candidates sont déjà des arêtes d'équipe, le moteur choisit dans les deux cas selon le seul `K` par classe,
et **les comptes de la surcouche ne pèsent sur aucun choix**. C'est aussi pourquoi les parts de classes du §4 (moins de
grandes routes en ROAD, moins de chemins en GRAVEL) sont celles de `K`, pas celles de l'équipe [lu dans le code, mesuré
par la part d'équipe].

## 8. Temps, issues, fenêtres

Temps d'un appel, 16 fils simultanés sur 8 cœurs physiques, sans tour chaud : **indicatifs**, non comparables au relevé
à un fil du lot 2. Curseur 0,25 / 1, surcouche complète, médiane / P90 en ms [mesuré] :

| D | jeux | curseur 0,25 | curseur 1 |
|---|---:|---|---|
| 5 à 40 km | 448 | 162 / 335 | 151 / 315 |
| 40 à 70 km | 1 805 | 245 / 514 | 216 / 470 |
| 70 à 100 km | 435 | 544 / 1 022 | 518 / 921 |
| 100 à 150 km | 104 | 781 / 1 632 | 730 / 1 770 |
| ≥ 150 km | 63 | 2 603 / 6 601 | 1 893 / 5 813 |
| toutes | 2 855 | 263 / 696 (max 12 784) | 235 / 655 (max 11 480) |

Issues [mesuré] : au curseur 0,25, **2 800 `OK` sur 2 855** (98,1 %), `NO_CANDIDATE` 49, `TOO_LONG` 6 ; au curseur 1,
2 804 `OK`, `NO_CANDIDATE` 44, `TAIL_TOO_LONG` 1, `TOO_LONG` 6 ; surcouches « écart » : 281 `OK` sur 286 aux deux
curseurs. Aucune panique, aucun échappement de fenêtre (54 fenêtres construites, 6 228 servies du cache, rien au-delà du
budget). Le contrôleur indépendant a refusé 1 parcours (curseur 0,25) et 3 (curseur 1) en `SHARP_TURN` : l'écart moteur
/ contrôleur déjà connu au lot 6. Trois parcours gardés dans 76 % des jeux `OK` ; longueur / D P10 à P90 0,951 à
1,051.

## 9. Cohérence et déterminisme

- `|l_poly − d_graph| / d_graph` : médiane 0,000 (réel) et 0,001 (généré), P90 0,001 : D des formes et D de la requête
  coïncident.
- Recouvrement mesuré par le banc contre `Route::shared_m` : médiane 57 m, P90 115 m d'écart (le banc compte une arête
  prise deux fois en entier, le moteur la longueur réellement doublée) ; sans effet sur la lecture, le recouvrement étant
  négligeable des deux côtés.
- Empreinte de résultat de la campagne : `55c3b065dc085f42`. Déterminisme vérifié à la relecture sur le même commit (B
  entière, 1 fil contre 16, même empreinte).

## 10. Réserves

- **Une équipe fait la statistique** : A pèse 84 % des boucles ; les lignes « toutes équipes » sont les siennes. B
  diffère en ROAD (lentilles plus nombreuses, compacité juste). C à I : 44 boucles ROAD et 7 GRAVEL en tout, aucun
  chiffre par équipe ne tient.
- **Le réel est mesuré sur la suite appariée**, avec des trous comblés par des segments droits (`gap_share` faible), des
  jonctions hors nœud qui perdent ~5 % des virages réels (sous-estimation du réel) et des éperons de 2 à 50 m qui peuvent
  en ajouter (relecture) : les écarts de virages du §4 sont de 18 à 23 % (ROAD) et de 37 à 43 % (GRAVEL), au-dessus de ce bruit.
- **Croisements et règles du contrôleur sur le réel : non mesurés** (3 marches continues).
- **Le généré ne prend que les jeux `OK`** (97 à 99 %), le réel toutes les boucles ; le Δ apparié est la lecture homogène,
  et il dit la même chose que les quantiles.
- **Requête figée** : D arrondie au km, cap à 45° vers le barycentre. Une personne qui choisit son cap autrement aurait
  d'autres parcours ; la forme du moteur (point de demi-tour à mi-parcours) n'en dépend pas.
- **Reconstruction** : 10 % par équipe, sans retirer les jumelles ; elle ne peut juger le goût appris pour A et B (§7).
- **Temps** : machine pleine (16 fils, charge 11,9), non comparables au lot 2.
- **Mémoire** : pic anonyme 14,2 Go, au-dessus des ~12 Go estimés par la note (§11.9) ; durée 231 s, sous les ~10 min
  estimées.

## 11. Écarts du codage à la note (§11)

Relevés à la relecture du banc, portés en note dans le §11 de `ord-loops-lot4.md` :

1. **§11.4, règle 3 bis — raccord au nœud commun** : deux appariements voisins arrêtés à 25 m ou moins, le long de leur
   arête, d'un nœud qu'elles partagent sont prolongés jusqu'à lui (`Walk::bridge_at_nodes`) ; sans cela la jonction
   tombait à quelques mètres du carrefour (21 jonctions de plus sur 495 en C).
2. **§11.8 — E(R) au sens du dépôt** : une arête de la trace n'est comptée que si le sens est ouvert au vélo et l'union
   couverte atteint `coverage_threshold` (`agg::union_coverage`) ; les arêtes seulement effleurées (bouts recalés sur une
   transversale) tombaient sinon toutes dans « qu'aucune autre trace ne prend ».
3. **§11.8 — mise à l'écart** : `ceil(0,10 × n)` calculé sans l'erreur d'arrondi flottante, qui donnait 4 au lieu de 3
   pour n = 30 et 8 au lieu de 7 pour n = 70.
4. **§11.7 — croisements** : le généré d'une mesure ne prend que les boucles dont le réel est mesuré.
5. **§11.10 — jumelles** : `twin_share` (part des mètres d'une boucle mise à l'écart prise par une autre boucle retenue
   de l'équipe, minorant) dans `LoopRec`, et deux colonnes en section 9 du résumé.
6. **§11.13 — commande d'étalonnage** : comparer à `head -n "$(wc -l < calib-calibration.txt)"` de la sortie du script
   (le banc n'écrit pas la ligne « unpaved share »), pas à `head -9`.

## 12. Ce que le relevé décide

Relecture du §2.4 de la note, règle par règle.

1. **« r / D et compacité déjà dans P25–P75 → le couloir descend »** : **non**. r / D est hors de l'intervalle pour tous
   les types et aux deux curseurs (P50 généré 0,387 à 0,393 contre un P75 réel de 0,359 en ROAD et 0,312 en GRAVEL ;
   96 à 99,9 % des parcours au-dessus) [mesuré]. La compacité y est en GRAVEL, pas en ROAD.
2. **« Compacité très en dessous de 0,30 → le couloir d'abord »** : **en partie, et pas pour la raison prévue**. ROAD :
   0,212 à 0,216 contre 0,320 réel, sous le P25 (0,236), Δ −0,10, et c'est le fait de A seule (B est juste). GRAVEL :
   0,186 à 0,196 contre 0,226, dans l'intervalle. Rien n'est « très en dessous » (pas de retour collé à l'aller :
   recouvrement P50 0,003). Surtout, **le couloir ne corrige pas l'écart principal** : élargir une lentille ne baisse
   presque pas r / D [calculé, §4.3].
3. **« Reconstruction hors routes d'équipe déjà haute en découverte → le goût appris apporte peu »** : **illisible**. Les
   arêtes qu'aucune autre trace ne prend font 2,1 % (A) et 5,5 % (B) des mètres mis à l'écart, et la reconstruction
   totale est la même avec ou sans la trace dans la surcouche [mesuré, §7]. Ce protocole ne peut ni confirmer ni
   écarter le goût appris.

Ce que les chiffres disent en plus, et qui commande l'ordre :

- **L'écart de forme est d'abord un écart de chemin, ensuite de figure.** À figure égale, l'aller du moteur fait 1,25 fois
  le vol d'oiseau, celui des équipes 1,41 (lentilles ROAD) à 1,66 (lentilles GRAVEL) ; la répartition des figures
  n'explique que 0,02 des 0,06 d'écart de r / D en ROAD et 0,01 des 0,10 en GRAVEL [calculé, §4.3].
- **L'écart de classes est grand et vient de `K`** : ROAD grandes routes 0,10 contre 0,24, voies cyclables 0,16 contre
  0,03 ; GRAVEL chemins 0,17 contre 0,45, non-revêtu 0,10 contre 0,25, `tertiary` 0,18 à 0,23 contre 0,085 [mesuré]. Moins de
  chemins en gravel, c'est aussi moins de virages (16 contre 28 aux 10 km) et un aller plus direct : **en GRAVEL, le goût
  est le levier de r / D**.
- **Le goût tel que le §3.2 l'écrit ne toucherait presque rien** : il ne s'applique qu'aux arêtes non roulées, qui font
  0,7 % (curseur 0,25) et 6,2 % (curseur 1) de la longueur des parcours générés (P50 de la part d'équipe, toutes
  équipes, 0,993 et 0,938 ; en découverte, A et B de 0,903 à 0,954 selon le type) [mesuré]. Les arêtes d'équipe coûtent aujourd'hui `len × K`, sans regard aux passages (§7.1).
- **Les figures** : triangle 53 % (ROAD) et 43 % (GRAVEL) des boucles réelles, quadrilatère 6,5 % et 1,7 % [mesuré,
  `vertices_10`]. Le moteur fait 9 % de « triangles » accidentels.

**Ordre recommandé : 4b (révisé) et 4c (réduit) gardent leur indépendance, mais le 4b passe devant le couloir, et le
quadrilatère sort du lot.**

1. **Sonde du goût, avant le stockage (nouvelle étape 2 bis)** : dans le banc seulement, un multiplicateur par classe
   tiré des parts roulées de l'équipe, appliqué **à toutes les arêtes de la vue, arêtes d'équipe comprises**, aux deux
   curseurs ; critère : W1 / IQR des classes du §4 et détour jusqu'au point lointain (GRAVEL 1,25 → vers 1,7), r / D
   GRAVEL. Une variante sans code nouveau dans le moteur est à essayer au même banc : la familiarité au-dessus de 0,5,
   qui fait déjà pondérer les arêtes d'équipe par leurs passages (`M[z]`) et que le service ne passe jamais.
2. **4c réduit : triangle d'abord, avec le couloir** dans la même étape, puisque les jambes du triangle en ont besoin et
   que la largeur de la lentille ROAD de A (compacité 0,21 contre 0,33) se règle avec lui. Critère : `vertices_10` à 2
   dans 40 à 55 % des parcours, r / D P50 ROAD sous 0,36.
3. **4b complet** (profil au dépôt, stockage) seulement si la sonde du point 1 réduit les écarts de classes de moitié au
   moins ; sinon le goût par classe reste une constante de `K` par type, réglée au banc, sans profil d'équipe.
4. **4d en dernier**, et pas jugé par la reconstruction (§7) : par la part des points lointains générés posés sur une
   destination de l'équipe, et par la distribution de `far_pos` (réel IQR 0,08, généré 0,025).

Ce qui change dans la note :

- **§3.2** : le goût s'applique aux arêtes **d'équipe aussi** (ou la familiarité remplace le goût sur elles), sinon il agit
  sur 1 à 6 % de la longueur ; la mesure de réussite est l'écart des parts de classes, pas la reconstruction « hors
  équipe ». La question 1 du §9 (goût en découverte) devient centrale : la découverte de A et B roule à 94 % sur leur
  réseau.
- **§3.3 et §4.1** : la figure se compte par **`vertices_10`** ; proportions générales ROAD 40 / 53 / 7 %, GRAVEL
  55 / 43 / 2 % (au lieu de 28 / 40 / 25 %) ; seules A et B passent le seuil de 50 boucles, et leurs ROAD diffèrent (A
  39 / 55 / 7, B 53 / 44 / 4) : la distribution par équipe sert.
- **§4.2 et §4.3** : **figures à 1 et 2 points lointains** ; le quadrilatère (k = 3) sort du lot (≤ 7 % du réel) avec sa
  seconde jambe intermédiaire, ce qui allège le budget du §4.5 (une recherche point à point par parcours au plus). Le
  couloir reste, comme séparation des jambes et réglage de la compacité ROAD, pas comme levier de r / D.
- **§4.2, repli** : le point lointain « le plus agréable » (`coût / mètres` minimal) choisit encore sur `K` ; il ne vaut
  qu'avec le goût du point 1.
- **§2.3 et question 3 du §9** : la reconstruction par arêtes ne départage rien pour les équipes au réseau dense ; la
  garder comme garde-fou, retirer les jumelles avec la boucle mise à l'écart, et ajouter au banc une mesure de sinuosité
  (détour jusqu'au point lointain, D sur le périmètre du polygone des sommets) avant la sonde.
