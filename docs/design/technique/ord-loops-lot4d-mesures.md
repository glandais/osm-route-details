# Générateur de parcours — lot 4 (4d) : attracteurs OSM, fichiers de la France, campagnes et lecture des critères

> Relevé du 19 novembre 2026 sur la branche `feat/ord-loops-lot4d` (depuis `develop` à `b288206`, 1.12.0), commits 1 à 6 du §17.14
> de [`ord-loops-lot4.md`](ord-loops-lot4.md) (`e83ac21` à `a0ab6a4`, paquet 1.13.0, API 1.13) et corrections de la relecture
> (`19f9e07` à `63b160a`). Campagnes jouées avec le binaire `release` de `63b160a`, Rust 1.100.0, poste de développement (16 fils,
> 30 Go), une à la fois. Graphe et vues France `france-20261115-z11`, extrait `france-261115.osm.pbf`. Décisions du propriétaire :
> §9.10 de la note ; spécification : §17 ; écarts du codage et des campagnes : §17.20. Sonde :
> [`ord-loops-lot4d-sonde.md`](ord-loops-lot4d-sonde.md).
> Sorties complètes **privées et hors dépôt** sous `~/.cache/osm-route-details/graph/lot4/4d/` (campagnes) et
> `~/.cache/osm-route-details/graph/lot4/4d/orda/` (rapport de construction) ; les fichiers `.orda` sous `lot6/`, à côté des vues.
> Ici : agrégats par catégorie OSM, des lettres d'équipes, aucune coordonnée, aucun identifiant, aucun nom de lieu. Rien n'a été
> poussé ni déployé.
>
> Marques : **[mesuré]** vient des rapports du banc ; **[calculé]** d'un calcul fait ici sur ces rapports. Sans marque dans un
> tableau : mesuré. Les temps du banc sont pris à 16 fils simultanés : comparables entre variantes d'un même tirage seulement.

## 0. En bref

- **Bascule : non.** `generator.attractors` reste `off` ; le commit 7 du §17.14 n'est pas fait et la séance en conteneur (C3) n'est
  pas jouée. Aucun des sept réglages essayés ne tient tous les critères du §17.12 (§9) : E4 (commerce GRAVEL au curseur 0,25) et
  A5 (garde des formes contre 1.12) manquent pour **tous**, B1 (goût) et le P90 de C1 au curseur 1 pour tous sauf l'apprentissage
  avec attracteurs, E3 (dépassement) pour tous ceux dont la lentille joue.
- **Invariants tenus** : I1 (`Classic` au bit près, fichier chargé ou non), I5 (l'organique sans attracteurs actifs rend 1.12 :
  `6324bdfdabf62f38` sur toutes les équipes, sans fichier et avec fichier à poids nuls), D (mêmes empreintes à 1 et 16 fils), I3
  (0 refus du contrôleur indépendant sur 19 944 à 20 356 parcours rendus par variante), et la métrique contre la sonde (§2).
- **L'effet existe et dépasse** : en ROAD, la part des premiers parcours dont le point lointain est à 300 m d'un commerce passe de
  0,147 · 0,125 (1.12, curseurs 0,25 · 1) à **0,272 · 0,290** aux réglages par défaut, pour un réel de 0,200 ; le point de vue de
  0,033 · 0,017 à 0,068 · 0,072 (réel 0,046). **La lentille fait l'essentiel** : sans elle (`ltol:0`), 0,164 · 0,167 en commerce ;
  à 1 % de D (`ltol:10`), 0,230 · 0,234, au plafond d'E3.
- **Ce qui coûte** : le goût (B1 : `S / S_base` ROAD au curseur 1 0,193 → 0,216 à 0,250, par les grandes routes des bourgs) et le
  P90 au curseur 1 (× 1,13 à × 1,18 de 1.12, deux tirages). Le P50 tient (× 1,02 à × 1,05).
- **Le plus proche** : `ltol:0` appris avec attracteurs (`-la`) tient E1 à E3, B1, C1 et C2, mais manque E4 (+0,017 de commerce
  GRAVEL), A2 (ROAD 0,25 : 0,591 de premiers à deux sommets pour une borne de 0,58) et A5 ; il invaliderait tous les profils.
- **Cols et sommets** (actifs, décision 2) : aucun effet lisible sur le banc d'équipes de plaine, comme prévu (§6).
- **Prêt à fusionner : oui**, attracteurs en option (`off`) : sans `on`, 1.13 sert comme 1.12 au bit près, fichier posé ou non.

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
l'empreinte de fin diffèrent). Après la relecture (table `CATS` contrôlée à l'ouverture, §17.20 point 16), les deux fichiers
s'ouvrent toujours.

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

## 2. Campagnes et contrôles

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`, `--out $G/lot4/4d`, fichiers trouvés sous `lot6/`,
une campagne à la fois, contrôles d'abord. Les variantes `attr` jouent les poids `AttrWeights::general` du §17.20 point 1 (ROAD
café, boulangerie, point de vue, col, sommet à 1 ; GRAVEL point de vue, col, sommet à 1) et `ltol` 50 sauf mention.

| # | Commande (`$B likeness …`) | Durée | Pic RSS |
|---|---|---:|---:|
| 1 | `--teams B,C --limit 20 --holdout-share 0.25 --variants base,svc,svc+org,svc+org+attr:0,svc+org+attr` (label `4d-relecture`, même binaire, §17.20 point 20) | 5 min 43 | 15,9 Go (relecture) |
| 1 | la même, `--no-attractors --variants base,svc+org --label 4d-essai-sans` | 1 min 57 | 14,8 Gio |
| 2 | `--teams B,C --holdout-share 0.25 --variants base,svc+org+attr --threads 1 --label 4d-det1`, puis `--threads 16 --label 4d-det16` | 13 min 01 · 2 min 22 | 10,3 · 15,0 Gio |
| 3 | `--holdout-share 0.25 --variants base,common,svc,svc+org,svc+org+attr:0,svc+org+attr,svc+org+attr+ltol:20,svc+org+attr+ltol:0,svc+org+attr+rcom:2+gvue:2,svc+org+attr+rvue:2 --label lot4-4d` | 1 h 09 min 38 | 19,3 Gio |
| 4 | `--holdout-share 0.25 --variants base,common,svc,common+org,svc+org,common+org+attr+ltol:0,svc+org+attr+ltol:0,svc+org+attr+ltol:0+la --label lot4-4d-final` (`<r*>` = `ltol:0`, choisi à la main, §17.20 point 21) | 47 min 25 | 19,0 Gio |
| 4 | `--holdout-share 0.25 --variants base,svc,svc+org,svc+org+attr+ltol:0,svc+org+attr+ltol:10 --label lot4-4d-t2` (second tirage ; `ltol:10` ajouté, point 22) | 33 min 44 | 18,8 Gio |

| Contrôle | Attendu | Obtenu | |
|---|---|---|---|
| Essai, fichier chargé : `base` · `svc` · `svc-org` · `svc-org-attr0` | `93149ec262198e9e` · `87e04a7ce10a564f` · `50adca52ef713d18` · `50adca52ef713d18` | les mêmes | tient (I1, I5) |
| Essai, sans fichier : `base` · `svc-org` | `93149ec262198e9e` · `50adca52ef713d18` | les mêmes | tient (I1, I5) |
| D : B et C, `base` · `svc-org-attr` à 1 et 16 fils | mêmes empreintes aux deux | `052224c85ef4145d` · `e6374e0339dd2083` (goût `70f77e7abd93e449`) aux deux | tient |
| I1 complet (campagnes 3 et 4 ; `base` et `svc` aussi au second tirage) : `base`, `common`, `svc` | `4686c904975302e7`, `07b62287e64fa258`, `384f003c0bd19b7f` | les mêmes | tient |
| I5 complet : `svc-org` et `svc-org-attr0` | `6324bdfdabf62f38` | le même (`svc-org` aux campagnes 3, 4 et au second tirage, `svc-org-attr0` à la 3) | tient |
| `common-org` (campagne 4) | `1024983e6cf5c1d2` (§15.1 du relevé du 4c) | le même | tient |
| Métrique contre la sonde, ROAD : réel commerce · point de vue | 0,199 · 0,046 (±0,006) | 0,200 · 0,046 | tient |
| Idem, `svc-org` au curseur 0,25 | 0,146 · 0,033 (±0,006) | 0,147 · 0,033 | tient |
| `svc-org-attr-ltol0`, campagnes 3, 4 et second tirage | une seule empreinte | `44cb66c6290cc49b` aux trois | tient (reproductible) |

Le goût appris (`taste_xxh3`) de toutes les variantes `svc+org+attr…` est celui de `svc-org` (`2b6a67cc9ce68b6e`) : apprendre sans
attracteurs donne bien les profils de 1.12 (règle L′, aucun redépôt) ; `-la` apprend un autre profil (`670bd05b57bf849b`).

## 3. Effet par catégorie et par type

Premier parcours gardé, lecture « toutes », toutes équipes, point lointain à 300 m d'un ancrage ; campagne 3 (et 4 pour `-la`,
second tirage pour `ltol:10`). Cellule : curseur 0,25 · curseur 1. Réel : 2 071 boucles ROAD, 780 GRAVEL.

| type | groupe | réel | `svc-org` (1.12) | `attr` | `ltol:20` | `ltol:10` | `ltol:0` | `ltol:0-la` | `rcom:2+gvue:2` | `rvue:2` |
|---|---|---:|---|---|---|---|---|---|---|---|
| ROAD | commerce | 0,200 | 0,147 · 0,125 | 0,272 · 0,290 | 0,252 · 0,261 | 0,230 · 0,234 | 0,164 · 0,167 | 0,175 · 0,180 | 0,300 · 0,313 | 0,258 · 0,267 |
| ROAD | point de vue | 0,046 | 0,033 · 0,017 | 0,068 · 0,072 | 0,071 · 0,066 | 0,067 · 0,062 | 0,058 · 0,053 | 0,049 · 0,052 | 0,049 · 0,058 | 0,107 · 0,105 |
| ROAD | bourg (poids 0) | 0,176 | 0,140 · 0,126 | 0,250 · 0,254 | 0,236 · 0,233 | 0,217 · 0,215 | 0,175 · 0,169 | 0,194 · 0,166 | 0,253 · 0,266 | 0,235 · 0,243 |
| GRAVEL | point de vue | 0,040 | 0,027 · 0,033 | 0,073 · 0,089 | 0,064 · 0,068 | 0,061 · 0,060 | 0,053 · 0,039 | 0,049 · 0,037 | 0,082 · 0,087 | 0,073 · 0,089 |
| GRAVEL | commerce (poids 0) | 0,063 | 0,089 · 0,112 | 0,095 · 0,093 | 0,100 · 0,088 | 0,104 · 0,095 | 0,106 · 0,108 | 0,106 · 0,090 | 0,099 · 0,087 | 0,095 · 0,093 |

- **Intervalles** (IC à 90 % apparié de l'écart à `svc-org`, 300 tirages) : en ROAD commerce, `attr` +0,125 [+0,107 ; +0,142] ·
  +0,165 [+0,146 ; +0,183], `ltol:0` +0,017 [+0,004 ; +0,030] · +0,042 [+0,029 ; +0,054] ; en GRAVEL commerce au curseur 0,25,
  `attr` +0,005 [−0,012 ; +0,025], `ltol:0` **+0,017 [+0,005 ; +0,028]**.
- **La lentille fait l'effet** [calculé sur le tableau] : à poids égaux, passer `ltol` de 0 à 10, 20 puis 50 (‰ de D) porte le
  commerce ROAD de 0,164 à 0,230, 0,252 puis 0,272 au curseur 0,25. Sans lentille, les seuls candidats et le rang des paires
  ferment 0,32 · 0,56 de l'écart au réel en commerce ; à 1 % de D, déjà 1,57 · 1,45 (au-delà du réel).
- **Les bourgs suivent les commerces** sans poids : cafés et boulangeries sont dans les bourgs ; la part `bourg` dépasse le réel
  de 0,07 à 0,09 dès que la lentille joue (plafond d'E3 : réel + 0,03).
- **Hors de l'équipe A** (211 boucles ROAD), 1.12 est déjà au-dessus du réel en commerce (0,205 · 0,205 contre 0,171) ; `attr`
  y monte à 0,371 · 0,410, `ltol:0` à 0,220 · 0,254 [mesuré]. Le réglage général est fait par A (84 % des boucles).
- **À 1 km** (ROAD, toutes équipes) : commerce réel 0,366, `svc-org` 0,364 · 0,339, `attr` 0,567 · 0,587, `ltol:0` 0,426 · 0,427 ;
  point de vue réel 0,103, `svc-org` 0,095 · 0,077, `attr` 0,169 · 0,152.
- **Sommets à 10 % de D** (ROAD, commerce, part des sommets) : réel 0,211, `svc-org` 0,172 · 0,135, `attr` 0,315 · 0,317, `ltol:0`
  0,243 · 0,240.
- **Compteurs par requête** (`attr`) : ROAD 7,68 sommets candidats ajoutés, 2,79 paires pesantes cherchées, 7,19 demi-tours
  remplacés, 100 % des requêtes touchées ; GRAVEL 3,40 · 1,84 · 5,35, 98,9 %. `ltol:0` : mêmes candidats et paires, 0 demi-tour.

## 4. Formes

Contre `svc` aux seuils du §16.11 (A1 à A4), et contre `svc-org` (A5, §17.12 « de plus »). Le banc juge A3 GRAVEL (≤ `svc` +
0,05) alors que le §16.11 le rapporte seulement : A3 est lu ici sur ROAD (§17.20 point 23).

| critère | `attr` | `ltol:20` | `ltol:10` | `ltol:0` | `ltol:0-la` | `rcom:2+gvue:2` | `rvue:2` |
|---|---|---|---|---|---|---|---|
| A1 : r / D, × `svc` (ROAD · GRAVEL, deux curseurs) | 0,36 · 0,39 ; 0,58 · 0,63 | 0,36 · 0,40 ; 0,57 · 0,63 | 0,36 · 0,40 ; 0,57 · 0,62 | 0,36 · 0,40 ; 0,58 · 0,63 | 0,34 · 0,39 ; 0,56 · 0,61 | 0,35 · 0,39 ; 0,59 · 0,64 | 0,37 · 0,43 ; 0,58 · 0,63 |
| A2 : premiers à 2 sommets, ROAD 0,25 (borne 0,58) | **0,584** | 0,574 | 0,569 | 0,574 | **0,591** | **0,588** | 0,567 |
| A3 : compacité ROAD, W1 / IQR (≤ 0,80 × `svc`, 0,61 · 0,64) | 0,18 · 0,13 | 0,19 · 0,14 | 0,19 · 0,14 | 0,19 · 0,15 | 0,18 · 0,15 | 0,20 · 0,14 | 0,17 · 0,10 |
| A4 : quatre mesures et `shared_share` | tient | tient | tient | tient | tient | tient | tient |
| A5 : r / D et compacité ≤ `svc-org` + 0,03, part à 2 sommets à ±0,03 | ne tient pas | ne tient pas | ne tient pas | ne tient pas | ne tient pas | ne tient pas | ne tient pas |

`svc-org` (1.12, référence d'A5) : r / D 0,437 · 0,472 (ROAD), 0,749 · 0,776 (GRAVEL) ; compacité 0,190 · 0,166, 0,337 · 0,248 ; part à
deux sommets 0,546 · 0,545, 0,475 · 0,427. Ce qui fait manquer A5 [mesuré] : la compacité GRAVEL (`attr` 0,365 · 0,280,
`ltol:0` 0,401 · 0,257, `-la` 0,393 · 0,261), la part à deux sommets ROAD au curseur 0,25 (+0,021 à +0,045 ; `-la` +0,045) et la
r / D au curseur 1 (`rvue:2` 0,537 en ROAD, +0,065). Les attracteurs déplacent un peu les sommets et favorisent le triangle (deux
sommets) ; A1 reste très au-dessous de son seuil : la forme gagnée au 4c n'est pas perdue, elle bouge de quelques centièmes.

## 5. Goût

`S / S_base`, quatre groupes (ROAD 0,25 · ROAD 1 · GRAVEL 0,25 · GRAVEL 1), « toutes » puis « écart » ; B1 : ≤ `svc-org` + 0,02 et
≤ 0,5.

| variante | toutes | écart | B1 |
|---|---|---|---|
| `svc-org` (1.12) | 0,227 · 0,193 · 0,284 · 0,217 | 0,224 · 0,187 · 0,314 · 0,242 | — |
| `attr` | 0,245 · **0,228** · 0,296 · 0,226 | 0,237 · **0,214** · 0,332 · 0,250 | ne tient pas |
| `ltol:20` | 0,238 · **0,221** · 0,294 · 0,224 | 0,230 · **0,211** · **0,336** · 0,251 | ne tient pas |
| `ltol:10` (second tirage) | 0,238 · **0,218** · 0,292 · 0,225 | 0,232 · 0,207 · 0,328 · 0,253 | ne tient pas |
| `ltol:0` | 0,235 · **0,216** · **0,305** · 0,224 | 0,230 · **0,214** · **0,350** · 0,253 | ne tient pas |
| `ltol:0-la` | 0,236 · 0,202 · 0,280 · 0,199 | 0,235 · 0,196 · 0,325 · 0,221 | **tient** |
| `rcom:2+gvue:2` | 0,245 · **0,225** · 0,302 · 0,227 | **0,247** · **0,214** · **0,339** · 0,257 | ne tient pas |
| `rvue:2` | 0,243 · **0,250** · 0,296 · 0,226 | 0,224 · **0,250** · 0,332 · 0,250 | ne tient pas |

- Tout reste très au-dessous de 0,5 : le goût n'est pas perdu, il recule de 0,01 à 0,06 ; mais B1 est relatif à 1.12.
- **Le recul vient des grandes routes** [mesuré, écarts nommés] : en ROAD, W1 / IQR de `way_major` passe de 0,053 · 0,126 (1.12) à
  0,100 · 0,224 (`attr`) et 0,103 · 0,200 (`ltol:0`) : un attracteur de bourg tire le parcours sur la route qui traverse le bourg.
  Apprendre avec attracteurs (`-la`) recale le goût sur ces parcours et tient B1.
- **B2** (`ltol:0`, A ROAD « toutes », `svc-org-attr-ltol0 − common-org-attr-ltol0`) : −0,040 · +0,003, ≤ +0,054 : **tient**. B2 de
  `-la` ne se lit pas (aucun `common+org+attr+…+la` au §17.15, point 24).

## 6. Cols et sommets (décision 2, rapportés sans critère)

Actifs au poids du point de vue de chaque type. Part des premiers parcours dont le point lointain est près d'un col ou d'un sommet,
toutes équipes [mesuré] :

| | réel | `svc-org` | `attr` | `ltol:0` | `rvue:2` (poids 2) |
|---|---:|---|---|---|---|
| ROAD, col, 300 m | 0,000 | 0 · 0 | 0 · 0 | 0 · 0 | 0 · 0 |
| ROAD, sommet, 300 m | 0,000 | 0 · 0 | 0,000 · 0,001 | 0 · 0 | 0,000 · 0,004 |
| ROAD, sommet, 1 km | 0,004 | 0,003 · 0,001 | 0,002 · 0,007 | 0,002 · 0,001 | 0,003 · 0,022 |
| GRAVEL, col, 300 m et 1 km | 0,003 | 0 · 0 | 0 · 0 | 0 · 0 | 0 · 0 |
| GRAVEL, sommet, 300 m | 0,000 | 0 · 0 | 0,003 · 0,001 | 0 · 0 | 0,003 · 0,001 |
| GRAVEL, sommet, 1 km | 0,000 | 0,004 · 0,001 | 0,005 · 0,005 | 0,003 · 0,001 | 0,005 · 0,005 |

Aucun effet lisible, comme la décision 2 le prévoyait : les équipes du banc roulent en plaine (2 boucles GRAVEL réelles sur 780
près d'un col). Un parcours ROAD sur 1 000 au plus finit près d'un sommet. Ils restent bornés par E3 (ROAD, ≤ 0,03 : tenu partout)
et couverts par A, B, C1 et C2.

## 7. Temps et issues

P50 et P90 par appel (ms), jeux sur la surcouche complète ; × `svc-org` du même tirage. C1 : P50 ≤ × 1,05, P90 ≤ × 1,10 à chaque
curseur.

| tirage | variante | P50 0,25 · 1 | × `svc-org` | P90 0,25 · 1 | × `svc-org` | C1 |
|---|---|---|---|---|---|---|
| 3 | `svc-org` | 390 · 365 | 1 · 1 | 740 · 764 | 1 · 1 | — |
| 3 | `attr` | 400 · 375 | 1,026 · 1,028 | 748 · 884 | 1,010 · **1,156** | ne tient pas |
| 3 | `ltol:20` | 404 · 377 | 1,036 · 1,033 | 745 · 888 | 1,006 · **1,162** | ne tient pas |
| 3 | `ltol:0` | 410 · 382 | 1,050 · 1,047 | 777 · 885 | 1,049 · **1,158** | ne tient pas |
| 3 | `rcom:2+gvue:2` | 403 · 382 | 1,033 · 1,048 | 761 · 899 | 1,027 · **1,177** | ne tient pas |
| 3 | `rvue:2` | 402 · 375 | 1,029 · 1,029 | 742 · 873 | 1,002 · **1,142** | ne tient pas |
| 4 | `svc-org` | 390 · 365 | 1 · 1 | 737 · 762 | 1 · 1 | — |
| 4 | `ltol:0` | 398 · 371 | 1,020 · 1,017 | 745 · 861 | 1,010 · **1,130** | ne tient pas |
| 4 | `ltol:0-la` | 401 · 370 | 1,027 · 1,016 | 757 · 725 | 1,027 · 0,951 | tient |
| t2 | `svc-org` | 385 · 361 | 1 · 1 | 739 · 761 | 1 · 1 | — |
| t2 | `ltol:0` | 398 · 371 | 1,034 · 1,028 | 744 · 861 | 1,006 · **1,131** | ne tient pas |
| t2 | `ltol:10` | 399 · 370 | 1,036 · 1,026 | 745 · 874 | 1,008 · **1,148** | ne tient pas |

- **Le P90 au curseur 1** est reproductible : `ltol:0` vaut × 1,158, × 1,130 et × 1,131 sur trois tirages [mesuré], `svc-org`
  761 à 764 ms. Il ne vient pas de la lentille (`ltol:0` n'en remplace aucun) mais des candidats et des paires : les triangles
  essayés passent par des bourgs. `-la` fait exception (0,951) : son profil appris change les parcours essayés.
- Contre `svc` (rapporté, × 1,40 au §9.9) : P50 × 1,25 à × 1,28 · × 1,38 à × 1,40.
- **C2 tient partout** : `OK` 99,1 à 99,3 % (`svc-org` 99,3 · 99,2), ni `PANIC`, ni `WINDOW_ESCAPE`, ni `VIOLATION` ; refus du
  contrôleur indépendant : 0 sur 19 944 à 20 356 parcours rendus par variante.

## 8. Réglage (§17.15)

Le champ `chosen` du banc vaut « aucun » aux deux campagnes : aucun réglage ne tient ensemble A, B1, C1, C2 et E3. Lu à la main
(A3 GRAVEL écarté, point 23), la conclusion ne change pas : seul `ltol:0` tient E3, et il manque B1, C1 (P90) et A5. Part fermée
moyenne d'E1 et E2 (plafonnée à 1) : 1,000 pour `attr`, `ltol:20`, `ltol:10`, `rcom:2+gvue:2` et `rvue:2`, 0,623 pour `ltol:0`,
0,754 pour `ltol:0-la`. Les poids 2 (`rcom:2+gvue:2`, `rvue:2`) creusent le dépassement et le recul du goût ; aucun poids plus fort
n'est utile, aucun poids négatif n'est permis au 4d.

## 9. Critères du §17.12

Candidat lu : `ltol:0` (la plus douce, la seule qui tient E3) ; entre parenthèses les autres réglages quand ils diffèrent.

| critère | seuil | `ltol:0` | autres | |
|---|---|---|---|---|
| E1 ROAD commerce | ≥ 0,174 · ≥ 0,163 | 0,164 · 0,167 | `attr` 0,272 · 0,290 ; `-la` 0,175 · 0,180 | **manque** (tient pour tous les autres) |
| E2 ROAD point de vue (moyenne) | ≥ 0,035 | 0,055 | 0,050 à 0,106 | tient |
| E3 pas de dépassement, ROAD | commerce ≤ 0,230, point de vue ≤ 0,076, bourg ≤ 0,206 | 0,164 · 0,167 ; 0,058 · 0,053 ; 0,175 · 0,169 | lentille active : commerce 0,230 à 0,313, bourg 0,215 à 0,266 | tient (**manque** dès que la lentille joue) |
| E4 GRAVEL | point de vue ≥ 0,035 ; commerce ≤ 0,094 · ≤ 0,117 | 0,046 ; **0,106** · 0,108 | commerce 0,095 à 0,106 au curseur 0,25 | **manque** (tous) |
| A1, A3 ROAD, A4 | §16.11 | tiennent | tiennent | tient |
| A2 | ROAD 0,25 ≤ 0,58 | 0,574 | `attr` 0,584, `rcom:2` 0,588, `-la` 0,591 | tient (manque pour trois) |
| A5 | ±0,03 de `svc-org` | compacité GRAVEL 0,25 +0,064 | tous | **manque** (tous) |
| B1 | ≤ `svc-org` + 0,02 | ROAD 1 +0,023, GRAVEL 0,25 écart +0,036 | tous sauf `-la` | **manque** |
| B2 | ≤ +0,054 | −0,040 · +0,003 | — | tient |
| C1 | P50 ≤ × 1,05, P90 ≤ × 1,10 | P50 × 1,02 à × 1,05 ; **P90 × 1,13 à × 1,16** au curseur 1 | tous sauf `-la` | **manque** |
| C2 | `OK`, refus | tient | tient | tient |
| D | 1 et 16 fils | tient (`attr`) | — | tient |
| L′ | B1 et B2 sans, puis avec | sans : B1 manque | `-la` : B1 tient, B2 ne se lit pas, E4, A2, A5 manquent | pas de bascule |

## 10. Conteneur

**Non jouée** : C3 suit la bascule (§17.12), qui n'a pas lieu. Préparée : l'image 1.12 de la séance serait
`osm-route-details:lot4-4c-bascule` (construite sur `d5dac61`, le code de `b288206`) ; le protocole retenu (§17.20 point 25) dépose
18 équipes sous 1.13, puis joue sur le même volume 1.12, 1.13, 1.12, 1.13 (chauffe de 54 générations non comptée, 54 en série et 162
à 4 clients au millième), les deux `.orda` montés à côté des vues. Rien n'indique que la mémoire bouge : 1 octet par chaîne de
fenêtre en plus, et le pic du banc (`svc-org-attr` contre `svc-org`) est le même à 0,1 Go près.

## 11. Réserves

- **Une équipe fait le réglage** (A, 84 % des boucles ROAD) ; hors d'elle, 1.12 dépasse déjà le réel en commerce.
- **E4 est étroit** : +0,005 sur une part dont l'erreur type vaut ≈ 0,010 sur 780 boucles [calculé] ; seul `ltol:0` (+0,017) en sort
  au-delà de son intervalle. Les autres manquent d'un à six millièmes.
- **A5 mesure un déplacement, pas une perte** : r / D reste × 0,34 à × 0,43 de `svc`, la compacité ROAD × 0,16 à × 0,32.
- **Le temps du banc est à 16 fils simultanés** ; le P90 au curseur 1 est reproductible sur trois tirages, le P50 bouge de 1 à 2 %.
- **Cols et sommets** : aucune équipe de montagne au banc ; leur effet en service est inconnu.

## 12. Ce que le relevé décide

- **Bascule : non.** E4, A5, B1 et le P90 de C1 manquent pour le réglage par défaut et pour chacun des six autres ; `generator.attractors`
  reste `off`, `AttrWeights::general` et `OrganicTune::default()` sont inchangés, l'apprentissage reste sans attracteurs. Pas de
  commit 7, pas de séance C3.
- **Prêt à fusionner : oui**, attracteurs en option : I1, I5, I3, I4 et D tiennent ; poser les fichiers ne change aucun parcours
  tant que `attractors = off` ; `graph_id`, `params_hash` et les profils sont inchangés.
- **L'effet est trop fort, pas trop faible** : la lentille à 5 % de D dépasse le réel de 0,07 à 0,09 en commerce et en bourgs ; le
  levier qui le règle est `ltol` (entre 0 et 10 ‰), pas les poids.
- **Ce qui reste au propriétaire** (§17.20 point 27) : garder `off` ; ou rendre les seuils E4, A5, B1 et le P90 de C1 moins stricts,
  et basculer `ltol:0` appris avec attracteurs (seuls E4, A2 et A5 y manquent ; tous les profils `incompatible`, une nuit de
  redépôt) ; ou une étape de plus (lentille entre 0 et 10 ‰, poids des bourgs à 0 dans la zone, apprentissage avec attracteurs
  jugé avec son partenaire `common`).
