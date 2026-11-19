# Générateur de parcours — lot 4 (4d) : relevé du codage, fichiers de la France et essai de fumée

> Relevé du 19 novembre 2026 sur la branche `feat/ord-loops-lot4d` (depuis `develop` à `b288206`, 1.12.0), commits 1 à 6 du §17.14
> de [`ord-loops-lot4.md`](ord-loops-lot4.md) (`e83ac21` à `a0ab6a4`, paquet 1.13.0, API 1.13), Rust 1.100.0, poste de
> développement (16 fils, 30 Go). Graphe et vues France `france-20261115-z11`, extrait `france-261115.osm.pbf`. Décisions : §9.10
> de la note. Écarts du codage : §17.20.
> **Ce relevé n'est pas celui des campagnes** : seuls les fichiers de la France et un essai de fumée (équipes B et C, 20 boucles
> chacune) ont été joués ; les campagnes 1 à 5 du §17.15, la bascule (commit 7) et la séance en conteneur restent à faire.
> Sorties complètes **privées et hors dépôt** sous `~/.cache/osm-route-details/graph/lot4/` (`4d/orda/`, `smoke-4d/`) ; les
> fichiers `.orda` sous `lot6/`, à côté des vues. Ici : agrégats par catégorie OSM, une lettre d'équipe, aucune coordonnée, aucun
> identifiant, aucun nom de lieu.
>
> Marques : **[mesuré]** vient des sorties ; **[calculé]** d'un calcul fait ici sur ces sorties. Sans marque : mesuré.

## 0. En bref

- **Les deux fichiers de la France se construisent en 45 s** (16 fils, pic 1,18 Go ; 2 min 47 à un fil, 0,84 Go) et pèsent
  **9,8 Mo (route) et 11,9 Mo (gravel)** ; le contenu (ancrages, zone) est le même à 1 et 16 fils.
- **Invariants tenus** : I1 (`Classic` au bit près, fichier chargé ou non : les références figées du moteur et, à l'essai, les
  empreintes de `base` et `svc`), I3 (contrôleur indépendant, 0 refus), I4 (déterminisme, fenêtre), **I5** (l'organique sans
  attracteurs actifs rend 1.12 : `50adca52ef713d18` sans fichier, avec fichier et poids nuls, avec fichier et `off`).
- **L'effet est net, et il dépasse** : sur l'essai, en ROAD, la part des premiers parcours dont le point lointain est à moins de
  300 m d'un commerce passe de 9,5 · 23,8 % (1.12) à **33,3 · 42,9 %** (réel : 22,7 %) ; E3 (≤ réel + 0,03) ne tient pas.
  Leviers prévus (§17.17) : `ltol` puis les poids, campagne 3.
- **Temps** : P50 × 1,047 · × 1,040 de `svc-org` sur l'essai (seuil × 1,05), P90 × 1,037 · × 0,833 : à la limite, bruit d'un
  essai de 78 jeux compris. À rejouer sur la campagne complète.
- **Pas de bascule** à ce stade : `attractors` reste `off`.

## 1. Les fichiers de la France

Commande (campagne 0 du §17.15, sorties sous `lot6/` par consigne, §17.20 point 2) :

```text
ord-graph-build attractors --pbf $G/lot6/france-261115.osm.pbf --graph $G/lot6/france-20261115-z11.ordg --check \
    --report $G/lot4/4d/orda/report.json
```

L'extrait est celui du graphe (taille 5 116 354 877 octets, xxh3 `9fbe483bc4f4845c`, égaux à l'en-tête). 134 979 objets
(4 931 ways étiquetées, 58 832 nœuds demandés, tous retrouvés).

| | route | gravel |
|---|---:|---:|
| fichier | `france-20261115-z11.road.orda` | `france-20261115-z11.gravel.orda` |
| taille (octets) | 9 815 808 | 11 852 456 |
| xxh3 | `b99d4e0667eab9c1` | `18e55d5c72ebd938` |
| sha256 | `b8fbb4a7c949eae1dffa34a9da822655b5f77d19115aa909205355aa206bc6cc` | `479e5774c302a5985e722eeb360ad33422e1ac7471d425611a3ce702f4ccab3a` |
| ancrages | 97 751 | 120 163 |
| nœuds de zone | 1 859 928 | 2 244 792 |
| accroche, zone et écriture, avec `--check` | 2,4 s | 3,3 s |

Construction : extraction 35,6 s (deux passes sur le PBF), total **44,6 s** de temps réel, 205,8 s de CPU, pic de mémoire
résidente **1 175 940 Ko**. À un fil (`RAYON_NUM_THREADS=1`) : extraction 157,5 s, total 2 min 46,7, pic 844 084 Ko ; les octets
`[128, fin − 8[` des deux fichiers sont ceux de la construction à 16 fils (seuls l'en-tête, qui porte la date d'écriture, et
l'empreinte de fin diffèrent).

Par catégorie (objets · accrochés · ancrages · nœuds de zone) :

| catégorie | objets | route : accrochés · ancrages · zone | gravel : accrochés · ancrages · zone |
|---|---:|---|---|
| café | 17 469 | 17 391 · 15 368 · 442 536 | 17 466 · 15 495 · 491 355 |
| boulangerie | 29 566 | 29 549 · 28 721 · 1 070 983 | 29 562 · 28 748 · 1 169 706 |
| point de vue | 18 952 | 10 854 · 9 731 · 153 160 | 18 160 · 17 267 · 258 365 |
| bourg (`town`) | 1 118 | 1 118 · 1 118 · 80 150 | 1 118 · 1 118 · 87 240 |
| village | 37 608 | 37 496 · 37 452 · 878 231 | 37 565 · 37 525 · 1 015 342 |
| col | 4 166 | 1 247 · 1 228 · 5 623 | 3 703 · 3 670 · 25 314 |
| sommet | 26 100 | 4 180 · 4 133 · 28 411 | 16 747 · 16 340 · 97 910 |

## 2. Invariants

- **I1** : les références figées `window_reference.txt`, `window_reference_192.txt` et `team_reference.txt` se rejouent **moteur
  avec fichier** (tests de `tests_attract.rs`) ; à l'essai (B et C, 20 boucles), `base` `93149ec262198e9e` et `svc`
  `87e04a7ce10a564f` sont les mêmes **avec** les deux fichiers chargés (première passe) et **sans** (`--no-attractors`, seconde
  passe). Les empreintes de la campagne complète (`4686c904975302e7`, `07b62287e64fa258`, `384f003c0bd19b7f`) sont contrôlées par
  le banc (arrêt si l'une manque) et restent à lire à la campagne 1.
- **I5** : `svc+org` rend `50adca52ef713d18` sans fichier et avec fichier, `svc+org+attr:0` (fichier, poids nuls) aussi, l'empreinte
  attendue pour `--teams B,C --limit 20` ; la référence `organic_reference.txt` se rejoue sans fichier, avec fichier et poids nuls,
  avec fichier et poids MTB (requêtes MTB, aller simple et strict identiques à 1.12, les autres changent).
- **I3** : 0 refus du contrôleur indépendant sur 234 parcours rendus à l'essai ; test `engine_vs_checker` avec fichier.
- **I4** : déterminisme et fenêtre plus grande, attracteurs actifs (test) ; le déterminisme à 1 et 16 fils au banc est la
  campagne 2.
- **Contrôleur, `graph_id`, `params_hash`, profils** : inchangés (`LearnParams::attractors = false` hors de l'empreinte).

## 3. Essai de fumée

```text
$B likeness --teams B,C --limit 20 --holdout-share 0.25 --variants base,svc,svc+org,svc+org+attr:0,svc+org+attr \
    --label 4d-essai --out $G/lot4/smoke-4d                                (5 min 42, pic 15,7 Go)
$B likeness --teams B,C --limit 20 --holdout-share 0.25 --no-attractors --variants base,svc,svc+org \
    --label 4d-essai-sans --out $G/lot4/smoke-4d                           (2 min 53)
```

Équipes B et C seulement (aucune du A, 84 % des boucles) : 22 boucles ROAD et 17 GRAVEL retenues, 21 · 15 premiers parcours par
curseur pour `svc-org` : **des ordres de grandeur, pas des mesures** (un point de pourcentage vaut 5 parcours). Part des premiers
parcours gardés dont le point lointain est à 300 m d'un attracteur, « réel · `svc-org` · `svc-org-attr` » (curseurs 0,25 · 1) :

| type | groupe | réel | `svc-org` | `svc-org-attr` | Δ à `svc-org` (IC 90 % apparié) |
|---|---|---:|---:|---:|---|
| ROAD | commerce | 0,227 | 0,095 · 0,238 | **0,333 · 0,429** | +0,238 [+0,10 ; +0,38] · +0,190 [0,00 ; +0,38] |
| ROAD | point de vue | 0,045 | 0,000 · 0,000 | 0,000 · 0,048 | 0,000 · +0,048 |
| ROAD | bourg | 0,136 | 0,095 · 0,286 | 0,238 · 0,286 | +0,143 · 0,000 |
| ROAD | col · sommet | 0 · 0 | 0 · 0 | 0 · 0 | — |
| GRAVEL | point de vue | 0,118 | 0,133 · 0,000 | 0,067 · 0,200 | −0,067 · +0,200 |
| GRAVEL | commerce | 0,176 | 0,133 · 0,000 | 0,333 · 0,000 | +0,200 · 0,000 |
| GRAVEL | col · sommet | 0 · 0 | 0 · 0 | 0 · 0,067 (aux deux curseurs) | 0 · +0,067 |

À 1 km (`retenus`, ROAD) : réel 0,409, `svc-org` 0,238 · 0,333, `svc-org-attr` 0,476 · 0,714. **Cols et sommets** (rapportés, sans
critère) : à 300 m, aucun premier parcours ROAD près d'un col ou d'un sommet, et en GRAVEL un sommet pour 15 parcours (6,7 %,
`svc-org` : 0) ; à 1 km, ROAD col 0 · 4,8 %, sommet 0 · 9,5 % (`svc-org` : 0 · 0), GRAVEL sommet 13,3 · 13,3 % (`svc-org` 6,7 · 6,7).
Sommets à 10 % de D, ROAD, commerce : 0,152 (réel, 33 sommets), 0,088 · 0,176 (`svc-org`), 0,333 · 0,353 (`svc-org-attr`).

**Compteurs par requête** (`svc-org-attr`, jeux sur la surcouche complète) : ROAD 8,3 sommets candidats ajoutés, 2,8 paires de poids
non nul cherchées, 8,7 demi-tours remplacés ; GRAVEL 4,4 · 1,9 · 4,9 ; 95 % (ROAD) et 100 % (GRAVEL) des requêtes touchées.

**Service** (P50 · P90 en ms, contre `svc-org` du même tirage) :

| variante | curseur | P50 | P90 | P50 × org | P90 × org | `OK` |
|---|---:|---:|---:|---:|---:|---:|
| `svc-org` | 0,25 · 1 | 259 · 299 | 2 919 · 3 519 | 1 · 1 | 1 · 1 | 92,5 % |
| `svc-org-attr0` | 0,25 · 1 | 249 · 282 | 2 700 · 3 018 | 0,963 · 0,945 | 0,925 · 0,858 | 92,5 % |
| `svc-org-attr` | 0,25 · 1 | 271 · 311 | 3 028 · 2 932 | **1,047 · 1,040** | 1,037 · 0,833 | 92,5 % |

(contre `svc` : P50 × 1,345 · × 1,468, P90 × 0,726 · × 1,107). Ni `PANIC`, ni `WINDOW_ESCAPE`, ni `VIOLATION` ; 0 refus du
contrôleur indépendant.

**Critères du §17.12 sur l'essai** (une lecture, pas un verdict : 21 parcours) : E1 tient (0,333 ≥ 0,161 ; 0,429 ≥ 0,233) ; E2 tient
de justesse (0,024 ≥ 0,023) ; **E3 ne tient pas** (commerce 0,333 · 0,429 pour un plafond de 0,257) ; E4 ne tient pas (commerce
GRAVEL 0,333 au curseur 0,25 pour un plafond de 0,138) ; A1 tient (r / D × 0,375 · × 0,375 en ROAD, × 0,75 · × 0,64 en GRAVEL) ;
A2, A3, A4, A5 et B1 ne tiennent pas ou ne se lisent pas sur 21 parcours ; B2 sans `common-org-attr` ; C1 tient (× 1,047 · × 1,040) ;
C2 tient. La part de l'écart fermée sur E1 et E2 vaut en moyenne 0,76 (plafonnée à 1).

## 4. Commandes de campagne (à jouer une à une, §17.15)

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`, binaire reconstruit au dernier commit du banc (les fichiers
sont trouvés sous `lot6/`) :

```text
# 1  I1 et I5, fichier chargé puis non : essai puis campagne complète à 25 %
$B likeness --teams B,C --limit 20 --holdout-share 0.25 --variants base,svc,svc+org,svc+org+attr:0,svc+org+attr --label 4d-essai --out $G/lot4/4d
$B likeness --teams B,C --limit 20 --holdout-share 0.25 --no-attractors --variants base,svc+org --label 4d-essai-sans --out $G/lot4/4d
# 2  déterminisme (D)
$B likeness --teams B,C --holdout-share 0.25 --variants base,svc+org+attr --threads 1  --label 4d-det1  --out $G/lot4/4d
$B likeness --teams B,C --holdout-share 0.25 --variants base,svc+org+attr --threads 16 --label 4d-det16 --out $G/lot4/4d
# 3  contrôles, métrique contre la sonde, premier jugement
$B likeness --holdout-share 0.25 --variants base,common,svc,svc+org,svc+org+attr:0,svc+org+attr,svc+org+attr+ltol:20,svc+org+attr+ltol:0,svc+org+attr+rcom:2+gvue:2,svc+org+attr+rvue:2 --label lot4-4d --out $G/lot4/4d
# 4  règle L′, B2 (common-org-attr), second tirage des temps
$B likeness --holdout-share 0.25 --variants base,common,svc,common+org,svc+org,common+org+attr<r*>,svc+org+attr<r*>,svc+org+attr<r*>+la --label lot4-4d-final --out $G/lot4/4d
$B likeness --holdout-share 0.25 --variants base,svc,svc+org,svc+org+attr<r*> --label lot4-4d-t2 --out $G/lot4/4d
```

`<r*>` : le réglage de la section 11 du rapport de la campagne 3 (champ `chosen` : parmi les réglages qui tiennent A, B1, C1, C2
et E3, ceux à 0,05 au plus de la plus grande part fermée, le plus doux, §17.20 point 17). Sans variante `common` en campagne 3, B2 ne se
lit pas : l'ajouter, ou lire la campagne 4. Campagne 5 (conteneur, après le commit de bascule) : images `b288206` puis 1.13.0, les deux
`.orda` posés à côté des vues du volume, méthode du §16.3 de [`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md).
