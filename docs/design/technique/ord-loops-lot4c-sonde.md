# Générateur de parcours — lot 4 (4c) : sonde du coût d'une recherche point à point

> Relevé du 18 novembre 2026 sur la branche `feat/ord-loops-lot4c` (depuis `develop` à `6889245`, le 3b fusionné), binaire
> `release` des commits `a980eb7` et `99ab8d9`, Rust 1.100.0, poste de développement (16 fils, 30 Go). Graphe et vues
> France `france-20261115-z11`. Conception : [`ord-loops-lot4.md`](ord-loops-lot4.md), §4.2, §4.3 et §4.5 ; c'est
> l'étape 2 du §10. Relevés précédents : [`ord-loops-lot4-mesures.md`](ord-loops-lot4-mesures.md),
> [`ord-loops-lot4-3b-mesures.md`](ord-loops-lot4-3b-mesures.md).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/lot4/4c-sonde/` (`campagne2.jsonl`,
> une ligne d'agrégats par jeu, et `campagne2-agg.txt`). Ni départ, ni coordonnée, ni identifiant d'équipe, de trace ou
> d'arête ici ; rien n'a été poussé ni déployé.
>
> Marques : **[mesuré]** vient des lignes de la sonde (agrégats faits par un script jetable hors dépôt) ; **[calculé]**
> d'un calcul fait ici sur ces mesures. Sans marque dans un tableau : mesuré. Temps pris **16 fils simultanés sur 8 cœurs
> physiques** : indicatifs, comparables entre eux, pas à un relevé à un fil ni au service.

## 0. En bref

- **Une recherche P1 → P2 coûte peu** : 5,4 ms au P50 et 21 ms au P90, **0,08 × T1** états au P50 et **0,28 × T1** au
  P90, soit 1,4 % et 4,6 % du budget d'aujourd'hui (6 × T1). Avec la jambe d'arrivée, un triangle complet coûte 5,7 ms et
  0,086 × T1 au P50. L'ordre de grandeur attendu au §4.5 (« quelques millisecondes ») tient.
- **Les bornes triangulaires T1 / T2 fixent 30 % d'états de moins** (rapport P50 0,70) et gagnent un quart du temps.
  L'heuristique reste faible : au départ, elle vaut 0,29 du coût trouvé. Le facteur minimal du vol d'oiseau ne vaut que
  0,11 à 0,43 du facteur médian.
- **Ce n'est pas le temps qui borne le triangle, c'est la longueur.** Le chemin T2 depuis P2 reprend les jambes 1 ou 2
  dans **70 %** des triangles. L'A* d'arrivée qui le remplace est très bon marché (0,2 ms) mais échoue dans 18 % des cas.
  Sur les triangles complets, **46 %** tombent à ±5 % de D et **71 %** à ±10 % ; le réel dépasse l'estimation de 4,2 %
  de D au médian. Avec quatre paires par requête, **63 %** des requêtes ont un triangle à ±5 %, **78 %** à ±10 %.
- **Surcoût calculé** pour quatre triangles par requête : +8 % sur le P50 d'une requête 1.10 (290 ms), et environ 0,34 × T1
  d'états, à côté des 0,16 × T1 que consomment les retours d'aujourd'hui. **Le budget d'états n'est pas le frein.**

## 1. Protocole

**Jeu.** Le banc de vraisemblance rejoue le chemin du service 1.10, la variante `svc` : profil de goût du 3b appris par
équipe, table commune, 25 % de boucles à l'écart comme à la campagne 4. Chaque boucle réelle retenue donne une requête par
curseur (0,25 : équipe d'abord ; 1 : découverte), avec le même départ, le même cap et D. La sonde se branche **après** la
génération, sur la même fenêtre et les mêmes agrégats d'équipe : jeux de la surcouche complète, **D de 30 à 150 km**,
types ROAD et GRAVEL. Les 8 jeux MTB sont exclus de la lecture. Cela fait **5 502 requêtes** (ROAD 4 004, GRAVEL 1 498),
couvrant 8 équipes. L'équipe A pèse 86 % des jeux.

**Ce que fait la sonde** (`ord_loops::probe`, fonctionnalité `probe`, jamais appelée par la génération) :

1. Mêmes accroche, poids (profil compris), disque D/2 et arbres T1 / T2 que la génération (premier passage, sans
   interdits). Les arbres sont refaits et chronométrés à part.
2. **Points lointains (§4.2).** On ne garde que les secteurs de 30° du cap (même règle que le moteur : centre à 50° au plus
   du cap). On ne garde aussi que les nœuds atteints par T1 et T2 dont les mètres T1 tombent dans la **bande [0,27 ; 0,39]
   × D**. On prend le plus agréable, `coût T1 / mètres T1` minimal, **par secteur et par tiers de bande** (3 sous-bandes, §3).
   La bande est **[calculé]** : pour deux sommets aux mêmes mètres m et un écart θ de 40° à 90°, l'estimation vaut à peu
   près 2m (1 + sin θ/2), soit D pour m entre 0,29 et 0,37 D ; on ajoute ±5 % de marge.
3. **Paires** : deux candidats écartés de 40° à 90° vus du départ, P2 dans le sens horaire après P1. L'estimation est
   `mètres T1(P1) + φ × vol d'oiseau(P1, P2) + mètres T2(P2)`, où φ est la moyenne des rapports mètres T1 / vol d'oiseau
   de P1 et P2. On garde les paires à ±5 % de D, celles du §4.2 « à la lettre » d'abord, puis par agrément moyen croissant.
   On en cherche **au plus 4 par requête**.
4. **Jambe 2** : A* de P1 à P2. Il part de l'état d'arrivée de la jambe 1, avec ses virages. Il tourne sur le graphe privé
   de la jambe 1, avec les mêmes interdits que les retours d'aujourd'hui : `BAN` hors queue, `TAIL` au coût × 2 dans
   0,15 D. Il reste borné au disque. Son budget est 6 × T1, propre à chaque recherche. L'heuristique est celle du §4.3 :
   `max(vol d'oiseau × plus petit facteur coût / dm de la fenêtre, T1(P2) − T1(x), T2(x) − T2(P2))`, où les valeurs
   d'arbre sont celles du meilleur état du nœud. La **même recherche est rejouée sans les deux bornes triangulaires**.
5. **Jambe 3** : le chemin T2 depuis P2. S'il reprend la jambe 1 hors queue ou la jambe 2, on joue l'A* d'heuristique T2
   d'aujourd'hui (`search::astar`, son plafond `2 h0 + 10 000` compris) sur le graphe privé des deux jambes (§4.3).
6. **Longueur réelle** = mètres de la jambe 1 + jambe 2 + jambe 3. Elle est lue **avant tout contrôle** : ni
   `check_route_in`, ni le contrôleur indépendant, ni le couloir, ni la réparation des épis.

**Temps 1.10** : le `ms` du jeu `svc` lui-même, sur le même jeu de requêtes. Il compte la génération avec sa reprise
éventuelle après un échappement, sans la sonde, qui est chronométrée à part.

**Neutralité** : les empreintes de résultat sont identiques avec et sans la sonde. Sur la campagne, `base` vaut
`4686c904975302e7`, celle de la campagne 2 de la seconde sonde, et `svc` vaut `384f003c0bd19b7f` sur les deux tirages.
Sur un essai à une équipe et 20 boucles, `svc` vaut `c584b04e30bf54e9` avec la sonde et sans elle. Les deux campagnes
(avec et sans la jambe 3) fixent exactement les mêmes nombres d'états, et leurs temps P50 diffèrent de 1 ms au plus.

### 1.1 Commandes exactes

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`.

| Étape | Commande |
|---|---|
| Essais (une équipe, 20 boucles) | `ORD_P2P_PROBE=$G/lot4/4c-sonde/essai.jsonl $B likeness --teams C --limit 20 --holdout-share 0.25 --variants base,svc --label p2p-essai --out $G/lot4/4c-sonde` (puis `ORD_P2P_SUB_BANDS=3`, puis sans `ORD_P2P_PROBE` pour la neutralité) |
| Campagne 1 (sans jambe 3, `a980eb7`) | `ORD_P2P_SUB_BANDS=3 ORD_P2P_PROBE=$G/lot4/4c-sonde/campagne.jsonl $B likeness --holdout-share 0.25 --variants base,svc --label p2p --out $G/lot4/4c-sonde` : 12 min 01 s, pic RSS 19,1 Go |
| Campagne 2 (`99ab8d9`, ce relevé) | la même avec `campagne2.jsonl` et `--label p2p2` : 11 min 59 s, pic RSS 19,5 Go |
| Vérifications | `cargo fmt`, `cargo clippy -p ord-loops --features probe --all-targets -- -D warnings`, `cargo clippy -p ord-loops-bench --all-targets -- -D warnings`, `cargo test -p ord-loops-bench` (62 tests) |

## 2. Le temps d'une requête 1.10, pour situer

[mesuré] Jeux `svc`, D de 30 à 150 km, en ms. « A* / T1 » donne les états fixés par tous les A* de la génération (retours
et ordre inverse), rapportés aux états de T1 ; le budget est de 6.

| type | D (km) | jeux | P50 | P90 | arbres T1 + T2, P50 | T1, P50 (milliers d'états) | A* / T1, P50 · P90 |
|---|---|---:|---:|---:|---:|---:|---|
| ROAD | 30–60 | 2 264 | 170 | 376 | 82 | 129 | 0,10 · 0,49 |
| ROAD | 60–100 | 1 528 | 334 | 726 | 133 | 193 | 0,10 · 0,28 |
| ROAD | 100–150 | 212 | 695 | 1 318 | 231 | 305 | 0,13 · 0,48 |
| ROAD | tout | 4 004 | 240 | 647 | 95 | 145 | 0,10 · 0,41 |
| GRAVEL | 30–60 | 1 436 | 527 | 860 | 140 | 190 | 0,51 · 1,18 |
| GRAVEL | 60–100 | 40 | 966 | 1 352 | 229 | 296 | 0,51 · 1,25 |
| GRAVEL | 100–150 | 22 | 2 184 | 4 885 | 489 | 639 | 0,69 · 1,46 |
| GRAVEL | tout | 1 498 | 539 | 894 | 142 | 190 | 0,51 · 1,18 |
| **tous** | **tout** | **5 502** | **290** | **761** | 101 | 151 | 0,16 · 0,72 |

Un retour d'aujourd'hui (une recherche d'heuristique T2) fixe 0,005 × T1 au P50 en ROAD et 0,011 × T1 en GRAVEL
[mesuré, sur les diagnostics]. Les issues : 5 427 jeux rendent des parcours et 83 n'en rendent pas.

## 3. Les points lointains : rendement des paires

[mesuré] Part des requêtes qui ont au moins une paire dont l'estimation tombe à ±5 % de D :

| type | D (km) | requêtes | §4.2 à la lettre (1 nœud par secteur) | 3 sous-bandes | paires à ±5 %, P50 (3 sous-bandes) | estimation / D de toutes les paires, P10 · P50 · P90 |
|---|---|---:|---:|---:|---:|---|
| ROAD | tout | 4 004 | 62 % | 98 % | 7 | 0,87 · 0,99 · 1,14 |
| GRAVEL | 30–60 | 1 436 | 50 % | 98 % | 6 | 0,88 · 0,99 · 1,13 |
| GRAVEL | 60–150 | 62 | 29 % [calculé] | 80 % [calculé] | 3 à 6 | 0,84 à 0,87 · 0,97 · 1,14 |
| **tous** | **tout** | **5 502** | **58 %** | **97 %** | 7 | 0,88 · 0,99 · 1,14 |

Un seul nœud par secteur, le §4.2 à la lettre, ne donne une paire utilisable qu'à **58 %** des requêtes. Il y a quatre
secteurs dans le cap, donc au plus six paires, dont une partie hors de 40° à 90°. **Tranché pour le 4c : trois sous-bandes
par secteur**, à 97 %, pour un coût nul : les candidats se lisent dans T1 en une passe.

**Points lointains piégés.** Dans **9 %** des recherches (4 % au curseur 0,25, 14 % au curseur 1 ; 7 % en ROAD, 16 % en
GRAVEL), l'A* ne fixe aucun état. La jambe 1 arrive sur P1 par une impasse, et aucune sortie n'est permise : demi-tour
interdit, ou seule arête déjà prise. Le nœud « le plus agréable » est donc souvent un bout de voie verte ou de chemin.
**Tranché pour le 4c** : un candidat doit avoir une sortie autre que son arête d'arrivée, soit un degré ≥ 3 dans la vue.
Le filtre se lit à la passe des candidats ; c'est la forme la moins chère de la réparation des épis du §4.4.

## 4. La recherche P1 → P2

[mesuré] Toutes les paires cherchées (20 136 recherches), recherches piégées ôtées. « Avec » : heuristique du §4.3 ;
« sans » : vol d'oiseau × facteur minimal seul. Le temps est en ms, les états sont rapportés à T1 de la même requête.

| type | D (km) | recherches | aboutit | ms avec, P50 · P90 | ms sans, P50 · P90 | états / T1 avec, P50 · P90 | états / T1 sans, P50 · P90 | états avec / sans, P50 |
|---|---|---:|---:|---|---|---|---|---:|
| ROAD | 30–60 | 8 411 | 99 % | 3,4 · 13,7 | 4,8 · 18,0 | 0,072 · 0,251 | 0,112 · 0,370 | 0,72 |
| ROAD | 60–100 | 5 572 | 98 % | 6,1 · 25,1 | 7,7 · 36,5 | 0,074 · 0,274 | 0,104 · 0,450 | 0,75 |
| ROAD | 100–150 | 798 | 95 % | 8,9 · 45,3 | 10,7 · 65,7 | 0,070 · 0,299 | 0,096 · 0,472 | 0,76 |
| GRAVEL | 30–60 | 5 195 | 100 % | 8,1 · 33,1 | 12,7 · 38,5 | 0,112 · 0,319 | 0,180 · 0,450 | 0,61 |
| GRAVEL | 60–100 | 105 | 100 % | 11,6 · 67,9 | 25,3 · 78,3 | 0,096 · 0,431 | 0,213 · 0,519 | 0,51 |
| GRAVEL | 100–150 | 55 | 100 % | 39,5 · 123,0 | 64,5 · 181,1 | 0,161 · 0,312 | 0,377 · 0,512 | 0,53 |
| **tous** | **tout** | **20 136** | **99 %** | **5,4 · 21,4** | **7,2 · 28,7** | **0,082 · 0,278** | **0,126 · 0,433** | **0,70** |

- **Budget** : aucune recherche n'a épuisé ses 6 × T1. Au P90, une recherche prend 4,6 % du budget global d'aujourd'hui
  [calculé].
- **Le §4.2 à la lettre** (4 213 recherches dont les deux sommets sont les plus agréables de leur secteur) : 4,7 ms et
  0,067 × T1 au P50, 20,7 ms et 0,281 × T1 au P90. C'est la même loi.
- **Par équipe** (campagne 1) : l'équipe A, 4 710 jeux, cherche en 5,9 ms au P50 (0,080 × T1) ; les sept autres
  réunies, 792 jeux, en 2,5 ms (0,105 × T1). La grande équipe porte les temps, pas une loi différente.
- **Pourquoi l'heuristique est faible.** Le plus petit coût par décimètre de la fenêtre, P50, vaut 0,449 en ROAD et
  0,176 en GRAVEL. Le coût médian des états de T1 vaut 2,01 en ROAD au curseur 0,25, 1,06 en ROAD au curseur 1, 1,57 et
  1,35 en GRAVEL. Le terme du vol d'oiseau vaut donc **0,11 à 0,43** du coût réel [mesuré]. Au départ de P1, l'heuristique
  vaut 0,29 du coût trouvé avec les bornes et 0,28 sans : les bornes T1 / T2 ne servent pas à P1, où les deux sommets
  sont à la même distance du départ. Elles servent en chemin.
- **Admissibilité.** Dans 1 218 recherches sur 18 082 (6,7 %), le chemin trouvé avec les bornes coûte plus cher que sans
  elles : de 0,2 % au P50, 5,9 % au plus. Aucune recherche n'aboutit sans les bornes et échoue avec. La cause : les
  arbres sont lus au meilleur état du nœud, et le coût d'un virage (au plus `TURN_COST_MAX + 500` = 2 500) peut séparer
  deux états du même nœud. **Tranché pour le 4c** : on retranche 2 500 aux deux bornes, ce qui les rend admissibles. Le
  gain mesuré ici en diminuera un peu ; non mesuré.
- **Balayage du facteur minimal** : 1,0 à 2,8 ms au P50 par requête (P90 4,3 ms), autant qu'une recherche. **Tranché pour
  le 4c** : il est calculé une fois par fenêtre et par agrégat d'équipe, gardé avec eux, et non à chaque requête.

## 5. La jambe d'arrivée et la longueur du triangle

[mesuré] Triangles dont la jambe 2 a abouti (18 082) :

| type | D (km) | triangles | chemin T2 repris : A* d'arrivée | A* trouvé | A*, ms P50 · P90 | A*, états / T1 P50 · P90 |
|---|---|---:|---:|---:|---|---|
| ROAD | tout | 13 572 | 66 % | 82 % | 0,2 · 0,7 | 0,002 · 0,013 |
| GRAVEL | tout | 4 510 | 81 % | 83 % | 0,2 · 0,9 | 0,003 · 0,011 |
| **tous** | **tout** | **18 082** | **70 %** | **82 %** | **0,2 · 0,8** | **0,003 · 0,013** |

La reprise médiane du chemin T2, quand elle existe, vaut 0,037 × D. Elle dépasse 0,01 × D dans 58 % des triangles et
0,05 × D dans 26 % (campagne 1). **L'A* d'arrivée du §4.3 est donc le cas courant, pas l'exception.** Il coûte environ 25
fois moins que la jambe 2. Aucun n'épuise son budget : ses 2 263 échecs viennent tous de son plafond `2 h0 + 10 000`, le
retour passant à plus du double du plus court. **78,6 %** des paires cherchées donnent un triangle complet [mesuré].

Triangles complets (15 819) ; « réel » = jambes 1 + 2 + 3 :

| type | D (km) | triangles complets | réel à ±5 % de D | à ±10 % | \|réel − estimé\| / D, P50 · P90 | (réel − estimé) / D, P50 | réel / D, P10 · P50 · P90 |
|---|---|---:|---:|---:|---|---:|---|
| ROAD | 30–60 | 6 934 | 39 % | 65 % | 0,056 · 0,253 | +0,056 | 0,99 · 1,07 · 1,27 |
| ROAD | 60–100 | 4 352 | 52 % | 76 % | 0,037 · 0,311 | +0,033 | 0,97 · 1,04 · 1,31 |
| ROAD | 100–150 | 641 | 59 % | 76 % | 0,034 · 0,316 | +0,028 | 0,97 · 1,03 · 1,33 |
| GRAVEL | 30–60 | 3 782 | 52 % | 75 % | 0,036 · 0,261 | +0,029 | 0,96 · 1,03 · 1,28 |
| GRAVEL | 60–150 | 110 | 35 % [calculé] | 50 % [calculé] | 0,08 à 0,13 | +0,08 à +0,13 | 0,94 à 0,96 · 1,06 à 1,14 · 1,26 à 1,33 |
| **tous** | **tout** | **15 819** | **46 %** | **71 %** | **0,044 · 0,269** | **+0,042** | **0,97 · 1,05 · 1,28** |

Par requête, avec au plus 4 paires cherchées : **63 %** des requêtes ont au moins un triangle complet à ±5 % de D, et
**78 %** à ±10 % (ROAD 64 % · 79 %, GRAVEL 62 % · 75 %). Au curseur 0,25 : 68 % · 82 % ; au curseur 1 : 59 % · 75 %.
140 requêtes sur 5 502 n'ont aucune paire à chercher.

Lecture :

- **L'estimation sous-estime** le triangle de 4,2 % de D au médian, et jusqu'à 5,6 % en ROAD court. La jambe 2 réelle
  vaut 1,09 × φ × vol d'oiseau au P50 (0,94 au P10, 1,63 au P90 ; campagne 1) : φ, lu sur le chemin radial départ → P,
  est plus faible que le détour d'une jambe transverse. La reprise de la jambe 3 ajoute le reste.
- **La queue est longue** (P90 de l'erreur à 0,27 × D). Elle vient des jambes 2 qui contournent un obstacle (P90 à
  1,63) et des A* d'arrivée qui doivent s'écarter des deux jambes.
- **Tranché pour le 4c** : la cible de l'estimation est recentrée sur le biais mesuré. On garde les paires dont
  l'estimation tombe dans [0,91 ; 1,01] × D au lieu de ±5 %. Ensuite, la sécante du §4.4 recale φ au candidat suivant
  du même secteur. Le gain n'est pas mesuré ici : à mesurer au banc du 4c, sur les distributions.

## 6. Surcoût pour le 4c [calculé]

Sur ce jeu, une génération 1.10 coûte 290 ms au P50 et 761 ms au P90.

| Hypothèse | Temps ajouté | États ajoutés |
|---|---|---|
| 1 triangle (jambes 2 et 3) | 5,7 ms au P50 (+2 %), 22 ms au P90 | 0,086 × T1 au P50, 0,29 au P90 |
| 4 triangles par requête | **≈ 23 ms au P50 (+8 %)** ; la somme des 4 jambes 2 vaut 6,8 % de la génération au P50, 21,7 % au P90 (rapport par requête, mesuré) | **0,29 × T1 au P50, 0,80 au P90** (mesuré, 4 jambes 2) |
| Budget d'aujourd'hui | — | 6 × T1, dont 0,16 consommé au P50 et 0,72 au P90 |

Les triangles tiennent dans **le même budget d'états** (§4.5), avec une marge de plus de 4 × T1 au P90. **Baisser le nombre
de figures essayées n'est pas nécessaire pour le temps** : le levier de qualité est la longueur (§5), pas la recherche. La
sonde complète, arbres refaits et balayage compris, prend 173 ms au P50 ; ce n'est **pas** un coût du 4c, puisque les
arbres sont ceux de la génération.

## 7. Ce que la sonde ne dit pas

- **Rien sur le contrôle** : les triangles n'ont passé ni `check_route_in` ni le contrôleur indépendant (croisements ≤ 2,
  virage < 150°, couloir doublé ≤ 20 %). La part rendue sera plus basse que les 63 % et 78 % du §5.
- **Pas de couloir** (bit `COR`, κ = 2) : il ne change pas le coût d'une jambe 2, qui ne part que de la jambe 1, mais il
  allongera les jambes d'arrivée.
- **Pas de sécante ni de réparation des épis** (§4.4) : un seul essai par paire.
- **Pas de forme** : ni r / D, ni compacité. C'est au banc du 4c, sur les distributions rejouées sur le profil du 3b, et
  avec la réserve du fait (d) (§9.6 de la note).
- **Temps à 16 fils sur 8 cœurs** : ni un relevé à un fil, ni le service en conteneur.

## 8. Ce qui est commité

Le module `ord_loops::probe` (fonctionnalité `probe`, que seul le banc active) et le branchement du banc
(`likeness/p2p.rs`, actif seulement si `ORD_P2P_PROBE` nomme un fichier). Ils sont gardés parce qu'ils **resserviront au
4c**. `astar_p2p` est la première forme de la recherche point à point à écrire (§4.3). La sonde rejouera le même relevé
une fois les familles codées : sous-bandes, filtre des impasses, bornes admissibles, cible recentrée. Rien ne change pour
le service ni pour la génération.

## 9. Questions au propriétaire

1. **La cible « médiane sous 500 ms jusqu'à 100 km » (§4.5), lue où ?** Au banc à 16 fils, la génération 1.10 est **déjà
   au-dessus** en GRAVEL avant tout triangle : 527 ms au P50 de 30 à 60 km, 966 ms de 60 à 100 km [mesuré]. Le
   triangle ajoute environ 8 % [calculé] et ne décide donc pas de la cible.
   **Recommandé** : lire la cible sur la mesure du service en conteneur (étape 6 du §10), comme le critère du 3b (P50 par
   appel contre `base`). Au banc, juger le 4c par le **rapport** à `svc` : P50 ≤ +15 %, soit environ deux fois le surcoût
   calculé ici. On ne réduit pas le nombre de figures pour le temps.
2. **Combien de paires de triangle par requête, et que faire quand aucune n'aboutit ?** Avec 4 paires, 63 % des requêtes
   ont un triangle à ±5 % et 78 % à ±10 % [mesuré], pour 23 ms et 0,29 × T1 au P50 [calculé / mesuré].
   **Recommandé** : au plus **4 paires** par requête, les candidats lentille du moteur d'aujourd'hui restant en repli. Le
   tirage de la figure (§9.1, point 5) garde alors sa part de triangles aussi souvent que possible, sans rendre moins de
   parcours qu'en 1.10. Au-delà de 4 paires, le gain n'est pas mesuré ; le banc du 4c le dira, si le propriétaire le
   souhaite, en jouant 4 contre 8.
