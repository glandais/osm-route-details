# Générateur de boucles sur graphe OSM — lot 2 : moteur « raccords » (`ord-loops`), conception

> Statut : **moteur codé** (`ord-graph` : vue `road` corrigée, section `VIEW_HEADING` ; `crates/ord-loops`) ; contrôleur indépendant (`crates/ord-loops-check`) et banc (`tools/ord-loops-bench`) écrits, §13 ; campagne de parité faite,
> relevé dans [ord-loops-lot2-mesures.md](ord-loops-lot2-mesures.md) (§15) ; parité non atteinte (D et E) ; les écarts à cette conception sont en fin de note (« Écarts »). Exigences : plan Pédalons `docs/plans/2026-11-15-generateur-parcours-osm.md`
> (§5, §8, §11 décisions 3 et 10, §12, §12 bis, §12 ter) ; garanties Java (plan d'équipe `2026-11-13-generateur-boucles-equipe.md`
> §2 et §5.5, `RouteGuarantees.java`, conception lot 2 §6 et §13). Socle : [lot 0 §11](ord-graph-lot0.md),
> [relevé 0 bis](ord-graph-lot0bis-mesures.md), [lot 1](ord-graph-lot1.md), [relevé lot 1](ord-graph-lot1-mesures.md).
> **[mesuré]** : sonde jetable hors dépôt du 15 novembre 2026 (§11) sur `pays-de-la-loire-20261114-z11.ordg`, ses vues et les
> surcouches du lot 1 ; **[estimé]** : à confirmer au codage. Équipes lettrées A à I par km dans la région, comme au relevé du
> lot 1. Ni coordonnée, ni identifiant d'équipe, de trace ou d'arête ici ; dérivés privés sous `~/.cache/osm-route-details/graph/lot2/`.

## 0. Ce que le lot livre

- **`crates/ord-loops`** : requête (départ, arrivée, cap, D, type, mode, surcouche) → au plus trois parcours, suites d'arêtes **de
  base** orientées ; modes **strict** (arêtes roulées, trous courts comblés, §2.3), **équipe d'abord** (défaut, plan §11.3),
  **découverte**. **`crates/ord-loops-check`** : contrôleur **indépendant**, écrit par une autre personne depuis le §6 seul, sur les
  seuls lecteurs d'`ord-graph`. **`tools/ord-loops-bench`** : banc Java rejoué (§8).
- Formes d'aujourd'hui (point de demi-tour, polygone) ; pas de squelette organique (lot 4), que rien ne gêne : les deux arbres
  du §5.1 sont ceux dont il aura besoin.

## 1. Le graphe de recherche

### 1.1 Une vue par type ; `road` corrigée

Route : `road` **corrigée** ; gravel : `gravel` ; VTT : `gravel` à poids VTT calculés au chargement (`all` coûte 3,4 fois `road`
par arbre, relevé 0 bis §6.1) ; aucune élaguée. **Nouvelle définition de `road`** (`ViewKind::keep`, vue reconstruite) : classes
routières actuelles sans surface connue non revêtue, **plus** `path`, `footway`, `pedestrian`, `bridleway` avec
`bicycle=designated` sans surface connue non revêtue (voies vertes), **plus** `track` à `tracktype=grade1` ou surface connue
revêtue (règle Java §5.5) ; exclusions inchangées (accès `No`/`Private`, allées `service=*`). **[mesuré]**, arêtes de base vivantes :

| | `road` (lot 0 bis) | + voies vertes | + `track` revêtus (**retenue**) |
|---|---|---|---|
| km ; arêtes de base | 95 747 ; 914 349 | 96 987 (+1 240 : `path` 1 044, `footway` 191, `pedestrian` 4) | **98 797** (+3 050) ; 941 489 |
| nœuds / arêtes de vue ; fichier | 477 074 / 589 715 ; 60,2 Mo | 488 249 / 607 571 ; 61,7 Mo | **494 980 / 615 595 (+4,4 %)** ; 62,5 Mo |
| km sans `surface` (présumés) ; nationaux gardés / 1 738 km | 67 426 ; 1 073 (62 %) | 68 059 ; 1 219 (70 %) | 69 319 (70,2 %) ; 1 254 (72 %) |

Des 519 km nationaux encore dehors avec les voies vertes, **441 ont une surface connue non revêtue** (`path` 257, `cycleway` 83,
`track` 73…) : « jamais de non-revêtu en route » est gardé (§10). Plus grande composante : 99,19 % des km (99,26 % avant). Vue
construite en 0,65 s, contrôle exhaustif passé ; coût d'un arbre du même ordre que l'ancienne vue (§4).

### 1.2 Pas d'élagage ; accroche

Les départs sont dans les impasses : 3 des 16 départs du banc dans la région ont leur arête la plus proche dans un arbre élagué par
`road-prune` **[mesuré]**, 31 des 50 départs du relevé 0 bis ne sont pas des nœuds de `road-prune`. Le temps tient sans élagage
(deux arbres complets par génération, §5.5) ; les vues élaguées restent pour le lot 4. **Accroche** (leçon Java §13.9, l'arête
admise la plus proche peut être un îlot) : projection sur les arêtes de base du graphe du mode (vivantes dans la vue ; en strict,
d'équipe ou comblées) à 2 km au plus (`START_TOO_FAR` / `END_TOO_FAR`), parmi celles dont la composante (sens ignorés, précalculée
par vue, et en strict au chargement de la surcouche) fait ≥ 0,9 × D ; la plus proche gagne, égalité par id. Position
`(arête de base, décalage)` : le parcours part et finit au milieu d'une arête. **[mesuré]** pour les 16 départs : arête `road` à
0–71 m, toujours dans la grande composante ; arête d'équipe (sans comblement) à 0–185 m.

## 2. Attributs d'équipe

### 2.1 Arête d'équipe, fréquence, chaîne

Une arête de base est **d'équipe pour le type t** si la surcouche lui donne un parcours, tous types et sens confondus (le réseau Java
était fait de tous les parcours), et qu'elle est vivante dans la vue de t. Exceptions Java (§5.5) : en route, voie verte ou `track`
**sans** `surface` seulement si roulé par `ROAD` ou `MIXED` ; en gravel, `path`, `footway` ou non-revêtu rugueux seulement par
`GRAVEL` ou `MIXED`. Sens permis : ceux d'OSM, contresens cyclables compris (le contresens partiel du §13.5 Java disparaît).
`n_t(b)` = max des deux sens (`ROAD`+`MIXED`, `GRAVEL`+`MIXED`, `MTB` puis `GRAVEL`), `z = ln(1+n_t) / ln(1+max n_t)`, repli tous
types sous 0,5 × D d'arêtes du type dans le disque (Java). Une **chaîne** est admise en strict si **chacune** de ses arêtes de base
est d'équipe ou comblée ; ses poids sont la **somme** de ceux de ses arêtes (§3). **[mesuré]** (`road` + voies vertes) : A 70 352
chaînes entièrement roulées (10 924 km), 1 620 en partie (244 km roulés, 315 non) ; B 1 863 / 82 km, C 1 498 / 83, D 1 988 / 11,
E 1 411 / 8. Une partielle est un trou (543 des 1 620 de A n'ont que des trous ≤ 50 m, comblés au §2.3) ou une sortie par une voie
hors vue : l'équipe n'a jamais roulé la chaîne entière, le strict la refuse à raison.

### 2.3 Trous de la surcouche : règle « cul-de-sac fictif, 100 m »

**Cul-de-sac fictif** : nœud de degré d'équipe 1 et de degré ≥ 2 dans la vue. **D(L)** : depuis chacun, le plus court chemin
d'arêtes vivantes non roulées, ≤ L, vers un autre nœud d'équipe, dont les arêtes deviennent **comblées** ; **J(L)** : tout chemin ≤ L
entre deux nœuds d'équipe. **[mesuré]**, sens ignorés ; cycl. = km hors ponts ; 2-CC = plus grande composante 2-arête-connexe :

| Équipe : réseau ; cycl. ; 2-CC ; culs-de-sac fictifs | D(50 m) : ajouté → cycl., 2-CC | **D(100 m)** | D(200 m) | J(100 m) |
|---|---|---|---|---|
| A : 11 169 km ; 8 023 ; 7 847 ; 2 414 | +7,4 km → 8 477, 8 295 | **+24,5 → 8 706, 8 553** | +61,4 → 9 108, 8 966 | +301 → 9 068, 8 788 |
| B : 1 945 ; 1 396 ; 1 233 ; 666 | +2,0 → 1 446, 1 247 | **+6,0 → 1 461, 1 258** | +17,1 → 1 487, 1 278 | +67 → 1 550, 1 312 |
| C : 1 582 ; 308 ; 133 ; 719 | +2,0 → 512, 330 | **+5,1 → 529, 331** | +11,3 → 574, 331 | +90 → 650, 343 |
| D : 1 999 ; 362 ; 288 ; 162 | +1,0 → 704, 597 | **+2,0 → 707, 597** | +4,5 → 721, 609 | +104 → 853, 637 |
| E : 1 419 ; 185 ; 43 ; 174 | +1,2 → 395, 221 | **+1,9 → 415, 310** | +4,9 → 561, 522 | +76 → 515, 339 |
| F, G (une trace) : 132 et 127 km, aucun cycle | aucun cycle | aucun cycle | aucun cycle | 5 et 9 km, 2-CC ≤ 1 km |

**Décision : D(100 m)** : 0,1 à 0,3 % de route non roulée admise (A : 24,5 km sur 11 169), +8,5 % de km cycliques pour A, × 2 pour D
et E ; J admet 12 fois plus de non-roulé (des raccourcis) pour moins de gain. 100 m couvre le p75 des arêtes fermées du lot 1 (p50
44 m, p90 187 m). 2,75 s pour 7 équipes × 8 règles. Comblées comptées à part (`bridged_m`), bornées au contrôle (§6, `OFF_TEAM`).

**Jonction courte, ajoutée après la revue (§14)** : D(100 m) ne comble que depuis un cul-de-sac fictif ; la seconde chaussée d'une 2 × 2, roulée dans un seul sens, n'en a pas, et coupait le retour de E (réseau atteint 272 km, 22 km menant au départ). Règle ajoutée : J(30 m), tout chemin non roulé de **30 m au plus** entre deux nœuds d'équipe est comblé (`Params::connector_m`). Comblé, **[mesuré]** route : A 24,9 → 61,2 km, E 1,9 → 11,2 km. Le moteur et le contrôleur calculent chacun l'ensemble des arêtes comblées (D(100 m) ∪ J(30 m)) depuis la surcouche ; le contrôle interne du moteur et le contrôleur n'acceptent hors équipe que ces arêtes.

### 2.4 Arêtes roulées hors de la vue du type

**[mesuré]** km roulés en `ROAD` hors de `road` + voies vertes : A 930 sur 11 301 (8,2 %), B 302 / 1 810, C 137 / 1 486, D 19 / 2 018,
E 5 / 1 424 ; surtout `track` sans surface (A 225), `path` non revêtu (220), `track` non revêtu (122), `path` sans `bicycle` (100) ;
les `track` revêtus en reprennent 39 km chez A. `GRAVEL` hors `gravel` : A 202 / 4 317, B 88 / 1 890, C 20 / 687, surtout des
`footway` non désignés. **Décision** : admises dans aucun mode (règle Java en route ; en gravel Java admettait le `footway` roulé
en gravel : écart assumé, lu au banc) ; le diagnostic donne les km d'équipe hors vue du disque.

## 3. Modèle de coût

En entiers (décimètres pondérés), sur chaque arête de base b d'une chaîne :
`w(b) = len(b) × K_t(classe, surface, accès) × E_m(b)`, `w(chaîne) = Σ w(b)`, plus le coût de virage à la transition (§4).

- **K_t**, sans équipe : en route, la préférence de classe d'`edge_cost` ; × 1,3 pour une surface **présumée** hors classe
  routière (voie verte ou `track` sans `surface`, le `roadUncertainFactor` Java) ; rien pour une classe routière sans surface
  (70 % des km : la pénaliser pénaliserait tout) ; `Destination` × 1,25. Gravel : non-revêtu roulant × 0,75, grande route × 1,25 ;
  VTT : hors route × 0,6, grande route × 1,4 (facteurs Java de départ, réglés au banc).
- **E_m** (familiarité f ∈ [0, 1] comme Java, β = 1) : arête d'équipe → `exp(β(1 − 2f) z)` ; comblée → 1 ; autre → **∞ en
  strict**, **λ = 3 en équipe d'abord** (la route d'équipe gagne tant que le raccord OSM n'est pas trois fois plus court),
  **1 en découverte**, où β = 0. λ ∈ {2, 3, 5} est comparé au banc (§8.5).
- **Stockage** : `Σ len × K_t` par chaîne remplace le poids synthétique de `VIEW_WEIGHT` dans la vue du type (VTT : calculé au
  chargement, 3,6 Mo). Par équipe et par vue, au chargement : une liste **creuse** triée des chaînes touchées, `(arête de vue u32,
  A = Σ w_osm roulés f32, z̄ u8, admise-strict)` ≈ 10 o × 72 000 chaînes = 0,7 Mo pour A **[calculé]**, construite en 0,02 à
  0,05 s par équipe et par vue **[mesuré]**. **Par requête**, le poids des chaînes des cellules touchées est écrit dans des pages
  comme celles de l'état local : `λ·(w_osm − A) + A·M[z̄]`, `M` = 256 multiplicateurs entiers calculés **sans `exp` flottant**
  (même poids au bit près en natif et en WASM) ; 0,7 ms par requête (`road`), 1,1 ms (`gravel`) **[mesuré]**. La recherche lit un
  `u32` (+5 à 11 % par arbre, §4) : la crainte du relevé 0 bis (§7, « au coût par état de la base ») ne vaut pas.

## 4. Règle de virage sur des chaînes

- **Caps** : par arête de vue, cap de départ (fenêtre 30–50 m depuis `a`) et d'arrivée (30–50 m avant `b`), sur la géométrie
  complète de la chaîne comme `RouteGuarantees.pathHeading` ; le sens inverse prend le cap opposé de l'autre bout. Nouvelle
  section `VIEW_HEADING` (2 × `u16` par arête, 2,5 Mo pour `road`, `format_minor` 2) écrite par `ord-graph-build views` : au
  chargement ils coûteraient 0,35 s (`road`) et 0,52 s (`gravel`) **[mesuré]**, trop pour WASM.
- **Seuil Java** : virage ≥ **150°** refusé, demi-tour sur la même arête refusé, aux nœuds de vue de degré ≥ 3 (un nœud de degré 2
  a été fondu, sauf changement de sens). Seuil et fenêtres repris tels quels : validés au banc Java (§13.4), et les changer
  romprait la comparaison.
- **Chaînes courtes** : 235 385 des 607 571 arêtes de vue (39 %) font moins de 50 m **[mesuré]** ; leur cap ne couvre pas la
  fenêtre, la recherche le **prolonge par le prédécesseur** de l'étiquette (comme `SearchTurns.walkedEnd`), pour elles seules.
  La règle des grappes de 100 m n'est vérifiée qu'à l'assemblage et au contrôle, qui ont le dernier mot.
- **Coût de virage** : 0 sous 45°, linéaire jusqu'à 200 m équivalents à 150°, réglé au banc ; aucun état de plus. **Épingle** :
  exemptée chez Java par un passage de trace, que la surcouche n'a pas ; ici par « même way OSM et géométrie » (§6, point 3).

**Coût mesuré** (sonde : tableaux à plat, état dense, 50 départs, 50 km, chaud, médiane de 3 tours ; ms médiane / p90) :

| Vue | poids stocké | + poids d'équipe par requête | + règle de virage | + coût de virage | poussées / fixées |
|---|---|---|---|---|---|
| `road` (lot 0 bis) | 14,4 / 26,1 | 15,1 / 29,5 | 16,7 / 32,3 | 18,2 / 35,4 (× 1,26) | 1,013 (max 1,016) |
| `road` retenue (§1.1) | 13,5 / 23,1 | 15,0 / 25,5 | 17,2 / 29,2 | **18,7 / 31,8 (× 1,39)** | — |
| `gravel` | 26,8 / 45,6 | 29,7 / 54,4 | 33,0 / 60,0 | 37,2 / 67,1 (× 1,39) | 1,016 (max 1,020) |
| `road-prune` + voies vertes | 8,8 / 17,1 | 10,1 / 19,5 | 11,1 / 21,6 | 12,3 / 23,8 (× 1,40) | 1,016 |

La règle seule retire 1,0 à 1,2 % des états fixés (poussées / fixées 1,000) ; le coût de virage porte le rapport à 1,013–1,023.
Les départs changent d'une vue à l'autre : seuls les rapports d'une ligne se comparent. Transposé au lecteur par cellules du
relevé 0 bis (`road`, 25,7 / 39,3 ms) : **36 / 55 ms par arbre** **[calculé]**.

## 5. L'algorithme de génération

### 5.1 Les recherches, exactement

1. **T1, arbre aller** depuis S : arêtes orientées, virage, poids du mode, disque de rayon D/2 (Java), état local ; coût et
   **mètres** du meilleur chemin par état. **T2, arbre retour** vers E (= S pour une boucle), sur les entrantes.
2. **Recherches ciblées** : A* d'heuristique T2 (coût de T2 sans interdits : admissible, interdits et pénalités ne font
   qu'augmenter les coûts), arrêtées dès que la clé dépasse 2 × le coût T2 de leur point de départ, dans un budget global
   **d'états fixés ≤ 6 × ceux de T1** (déterministe ; l'horloge ne sert qu'au `stop` de l'appelant). Soit **2 arbres complets + au
   plus l'équivalent de 4** en recherches ciblées, au lieu des « 10 arbres » du modèle 0 bis.
3. **Faisabilité** sur T1 et T2, avant tout candidat (corrections Java §13.10–13.11) : A→B, `NO_ROUTE`/`END_UNREACHABLE` si E
   n'est pas atteint, `DISTANCE_TOO_SHORT` si D < 1,05 × le plus court chemin ; `NETWORK_TOO_SMALL` si la route atteinte à moins de
   D/2 fait moins de 0,9 × D ; boucle, `NO_ROUTE`/`NO_WAY_BACK` si les arêtes atteintes **et** menant à S font moins de 0,5 × D.

### 5.2 Familles

- **Point de demi-tour** (gardée : la famille des petits réseaux, Java §13.4). Par secteur de 30° (12, ou ceux à ±35° du cap), le
  nœud T atteint par T1 et T2 dont les mètres aller sont les plus proches de D / (1 + ρ), ρ = 1,09 au départ (rapport retour /
  aller médian **[mesuré]**). **Aller = chemin de T1** (aucune recherche) ; **retour = A* T → E** sur le graphe privé de l'aller
  (§5.3). Hors ±10 %, le ρ du candidat corrige la cible et un second T est essayé dans le secteur. Si l'A* échoue : ordre inverse,
  retour = chemin de T2, aller = A* S → T d'heuristique vol d'oiseau × plus petit facteur de coût.
- **Polygone à liste noire** (gardé, petit budget : 4 réussites Java sur 53 lui étaient propres) : 2 ou 3 sommets sur la figure
  Java dans le cap, choisis par leurs mètres T1 ; première jambe = chemin T1, dernière = A* d'heuristique T2, intermédiaires =
  A* géométrique ; jambes précédentes interdites ; échelle corrigée par la sécante sur le rapport route / figure.
- **Abandonné, le flot de Suurballe** (`EdgeDisjointPaths`) : aveugle aux sens, puis deux passes orientées par point ; chez A,
  15 s des 26 s de la famille dans des flots refusés au contrôle (§13.13) ; ni virage ni couloir. L'optimalité de la paire cède au
  **nombre d'essais** (un retour : 0,05 ms). Abandonnés aussi : réseau d'équipe en mémoire, composantes hors du graphe parcouru (§13.10).

### 5.3 Garder le retour hors de l'aller

- **Interdiction** des arêtes de vue de l'aller (donc de leurs arêtes de base) hors des queues.
- **Queues** : les états de l'aller à ≤ 0,15 × D de S (et vers E) restent permis au retour, au coût × 2 ; un départ en impasse en
  sort sans queue forcée ni ponts. Le contrôleur la borne (Java : 80 `TAIL_TOO_LONG` chez C) ; la sonde, sans queue, laisse 8 départs sur 50 sans boucle.
- **Couloir** : une arête de vue aux deux bouts à ≤ 25 m de l'aller (échantillons de 10 m en grille de 25 m, hors 30 m aux bouts et
  queues) coûte × 3 au retour (plan §5.3 ; plafond au contrôle : 20 % du cœur) : seconde chaussée d'une 2 × 2, piste le long de la route.

### 5.4 La distance à ±10 %

Java échouait d'abord en `DISTANCE_OUT_OF_TOLERANCE` : 16 points et 32 polygones chers, longueur connue après le flot. Ici les mètres
aller de **tous** les T sont dans T1, un retour coûte 0,05 ms, 24 à 48 candidats sont mesurés et la sécante corrige ρ par secteur.
**[mesuré]** sans queue, couloir ni contrôle (borne basse), 50 départs, `road` + voies vertes, équipe d'abord, surcouche de A :

| D | T1 + T2, méd. / p90 (ms) | A* méd. / p90 (ms) ; états méd. | 1er tour : retours, à ±10 % | 2e tour : retours, à ±10 % | départs avec une boucle à ±10 % | par départ méd. / p90 / max (ms) |
|---|---|---|---|---|---|---|
| 50 km | 5,5 + 6,1 / 15 + 16 | 0,03 / 0,8 ; 137 | 237, 130 (55 %) | 65, 28 | 42 / 50 | 27 / 73 / 135 |
| 100 km | 20,3 + 21,7 / 46 + 48 | 0,05 / 3,4 ; 330 | 241, 151 (63 %) | 53, 22 | 42 / 50 | 107 / 185 / 320 |
| 150 km | 49,5 + 51,3 / 95 + 96 | 0,03 / 10,1 ; 151 | 238, 124 (52 %) | 74, 32 | 42 / 50 | 261 / 364 / 708 |
| gravel 100 km | 45,7 + 47,5 / 86 + 90 | 0,26 / 9,4 ; 1 315 | 244, 166 (68 %) | 47, 22 | 40 / 50 | 231 / 344 / 738 |

Le temps par départ inclut le choix des T (12 relectures d'une table de tous les nœuds, un passage dans le moteur). Sans plafond
d'A* et T au vol d'oiseau (0,32 × D) : maximum 605 ms à 100 km, longueurs p10 / p90 0,84 / 1,26 × D (contre 0,82 / 1,16).

### 5.5 Temps et mémoire

Boucle de 100 km, `road` retenue : 2 arbres × 36 / 55 ms (§4) + au plus 48 A* plafonnés + choix, assemblage et contrôle ≈
**100 à 150 ms en médiane, 250 à 400 ms au p90** **[estimé à partir du mesuré]**, sous la cible de 500 ms. Gravel à 100 km : 2,2
fois plus cher (sonde 231 / 344 ms), la cible y tient en médiane, pas sûrement au p90. Mémoire : 3 états locaux (T1, T2, A*) et
les pages de poids, ≈ 4 × 4,5 Mo à 50 km sur `road` (relevé 0 bis §8) **[calculé]**.

### 5.6 Choix de trois parcours, déterminisme

Tout candidat passe le contrôle du moteur (spécification du §6) ; un refus le jette et compte sa cause. Note (plus bas = mieux) :
écart de distance, queue, croisements, écart de cap (barycentre), **part hors équipe**, |f − z̄|, virages > 45° par 10 km, couloir.
Choix glouton : le meilleur, puis ceux qui partagent moins de 50 % du plus court avec chaque retenu (`CandidateSelector`).
**Déterminisme** : poids entiers, égalités par id d'état, aucune table de hachage parcourue sans tri ; la graine
(`SplitMix64(seed ^ empreinte de la requête)`) ne tourne que l'ordre des secteurs, ρ (±3 %) et les angles ; graine + 1 = « autres propositions ».

## 6. Les garanties : spécification du contrôleur indépendant

**Entrées** : graphe de base (`GraphFile` : extrémités, attributs, longueurs, géométrie, way de `ID_EDGES`), surcouche, requête
(S, E, D, type, mode) et parcours : départ `(b₀, o_s)`, suite `(bᵢ, sensᵢ)` i = 0..n−1, arrivée `(b_{n−1}, o_e)`. Morceau i = partie
parcourue de bᵢ (de o_s au bout pour i = 0, du début à o_e pour i = n−1, entière sinon ; un seul morceau si n = 1) ; vᵢ (0 < i < n)
= fin du morceau i−1. Longueur : `len_dm / 10` pour un morceau entier, abscisse haversine le long de la géométrie sinon ; caps
dans un repère équirectangulaire local. **Constantes Java** : virage 150°, fenêtre 30–50 m, grappe 100 m, queue 0,15 × D,
2 croisements, ±0,10 × D, couloir 25 m (pas 10 m, écart 400 m, bouts 30 m, plafond 0,20) ; plus accroche 2 km et trou 100 m.
**Admis pour t** = le `keep` de la vue du type (§1.1), recalculé depuis les attributs ; **degré admis** = nombre d'arêtes admises.

1. **Structure** : `EMPTY` (n = 0) ; `NOT_ON_GRAPH` (id hors graphe) ; `DISCONTINUOUS` (fin d'un morceau ≠ début du suivant) ;
   `ONE_WAY` (sens fermé au vélo) ; `NOT_ADMITTED` (arête non admise pour t) ; `WRONG_START` / `WRONG_END` (point à plus de 2 km
   de S ou de E ; boucle dont l'arrivée n'est pas le départ : même arête, |o_e − o_s| ≤ 1 m).
2. **Équipe**, en strict : « d'équipe » selon le §2.1, recalculé depuis la surcouche ; toute suite maximale de morceaux
   consécutifs non d'équipe fait ≤ 100 m **et** touche, à l'un de ses bouts, un cul-de-sac fictif (degré d'équipe 1, degré admis
   ≥ 2), sinon `OFF_TEAM` ; les morceaux partiels du départ et de l'arrivée suivent la même règle. Hors strict : rien d'exigé.
3. **Demi-tour, virage** : `U_TURN` si deux morceaux consécutifs sont la même arête en sens opposés. `SHARP_TURN` à tout vᵢ de
   degré admis ≥ 3 si l'angle entre le cap des 30–50 m **parcourus** avant vᵢ et celui des 30–50 m après (fenêtres prises le long
   du parcours, sur autant de morceaux qu'il faut) est ≥ 150°. **Grappe** : i < j, vᵢ…vⱼ à moins de 100 m le long du parcours, vᵢ et
   vⱼ de degré admis ≥ 3, angle entre le cap avant vᵢ et celui après vⱼ ≥ 150° ⇒ `SHARP_TURN`, **sauf épingle** : les morceaux i à
   j+1 sont tous de la même way OSM, la corde vᵢ→vⱼ fait moins de 30 m en travers de chacun des deux caps, et aucune portion
   droite de 15 m ou plus (points à moins de 1 m fondus, inflexion ≤ 20°) n'est à 90 ± 25° de l'un d'eux (`hasConnectorRun`).
4. **Impasse** : `DEAD_END` si un vᵢ a un degré admis < 2 (redondant avec 3, explicite comme chez Java).
5. **Réemprunt** : deux morceaux d'une arête se recouvrent s'ils en partagent plus de 1 m. `EDGE_USED_MORE_THAN_TWICE` si trois
   se recouvrent deux à deux. **Marqués** : les morceaux qui en recouvrent un autre. **Cœur** : la plus longue suite (en m) de
   non marqués, la première en cas d'égalité ; zones de départ et d'arrivée : avant et après. `REUSE_OUTSIDE_TAIL` si une zone
   dépasse 0,15 × D ou s'il n'y a pas de cœur ; `TAIL_TOO_LONG` si la longueur recouverte, une fois par arête, dépasse 0,15 × D.
6. **Croisements** : l'algorithme Java (`RouteGuarantees`, point 5) sur les nœuds du cœur, positions partielles exclues ; plus
   de 2 ⇒ `TOO_MANY_CROSSINGS`.
7. **Couloir** : cœur échantillonné tous les 10 m sans ses 30 premiers et derniers mètres ; un échantillon est doublé si un autre
   du cœur est à ≤ 25 m et à ≥ 400 m d'abscisse ; 10 × doublés / (2 × longueur du cœur) > 0,20 ⇒ `CORRIDOR_DOUBLED`.
8. **Distance** : L = Σ longueurs des morceaux ; |L − D| > 0,10 × D ⇒ `DISTANCE_OUT_OF_TOLERANCE`.

Sortie : l'ensemble des violations, L, la longueur recouverte, les croisements, `team_m`, `bridged_m`. Le contrôleur fait
confiance au graphe (comme Java à son `QueryGraph`) et ne lit ni vues, ni poids, ni caps stockés.

## 7. API du crate `ord-loops`

`ord-graph` seul, sans `pbf` ni `mmap` (le serveur ouvre le fichier, WASM passe un `Vec<u8>`) ; ni I/O, ni fil, ni horloge, ni
`unsafe` ; `cargo check --target wasm32-unknown-unknown` en CI.

```rust
pub struct Engine<B> { /* base, vues road et gravel (GraphFile<B>), composantes, Params */ }
impl<B: AsRef<[u8]>> Engine<B> { pub fn new(base: GraphFile<B>, road: GraphFile<B>, gravel: GraphFile<B>, p: Params) -> Result<Self, EngineError>; }
pub struct TeamData { /* listes creuses par vue, réseau strict, trous comblés, composantes strictes */ }
impl TeamData { pub fn build<B: AsRef<[u8]>>(e: &Engine<B>, overlay: &Overlay) -> Result<Self, EngineError>; } // identité vérifiée
pub enum RideType { Road, Gravel, Mtb }       pub enum Mode { Strict, TeamFirst, Discovery }
pub struct Request { pub start: LatLon, pub end: Option<LatLon>, pub bearing: Option<Bearing8>, pub distance_m: f64,
                     pub ride_type: RideType, pub mode: Mode, pub familiarity: f32, pub seed: u64 }
pub fn generate<B: AsRef<[u8]>>(e: &Engine<B>, team: Option<&TeamData>, r: &Request, ws: &mut Workspace,
                                stop: &mut dyn FnMut() -> bool) -> Result<Generation, Failure>;
pub struct Generation { pub routes: Vec<Route>, pub network_m: f64, pub familiarity_fallback: bool, pub diagnostics: Diagnostics }
pub struct Route { pub start: Position, pub end: Position, pub steps: Vec<Step /* base_edge u32, reverse bool */>, pub length_m: f64,
                   pub team_m: f64, pub bridged_m: f64, pub shared_m: f64, pub crossings: u32, pub unpaved_m: f64,
                   pub presumed_surface_m: f64, pub bearing_deviation_deg: Option<f64>, pub familiarity: f32, pub family: Family }
pub fn route_geometry<B: AsRef<[u8]>>(base: &GraphFile<B>, r: &Route) -> Vec<LatLon>;
pub enum Failure { StartTooFar, EndTooFar, DistanceTooShort, NetworkTooSmall, NoRoute(NoRouteDetail), NoTeamData, Stopped }
pub enum NoRouteDetail { EndUnreachable, NoWayBack, TailTooLong, NoCandidate, Budget }
```

Codes de `GeneratorFailure` Java et ses détails, plus `NoTeamData` (strict ou équipe d'abord sans surcouche) et `Stopped` (jamais de
résultat partiel). `Engine`, `TeamData` immuables et partageables ; un `Workspace` (états, pages de poids) par fil. Le moteur ne rend
que des candidats passés à son propre contrôle ; l'arbitre indépendant est appelé par le banc (plus tard, par le backend).

## 8. Protocole du banc (`tools/ord-loops-bench`)

### 8.1 Rejouer les requêtes Java

`requests-<équipe>.jsonl` (`bench-1115-head`, et `-head-alone` pour A, B, E) : `i`, `kind` (`loop`, `loop-gravel`, `loop-mtb`,
`oneway`), `start` (un **indice**), `km`, `bearing`, `familiarityAsked`, `type`, `outcome` (`OK`, `GENERATOR_*[:détail]`,
`STOPPED`), `ms`, `routes[]` (`km`, `sharedM`, `crossings`…). L'outil **reconstruit les points depuis l'export** par la règle de
`RealDataGeneratorBench.starts` (premier point des traces non exclues, cases de 0,0045° × 0,0065°, les plus fréquentes d'abord, à
plus de 2 km entre eux, 8 au plus ; arrivée d'un `oneway` : premier autre départ à 20–60 km). **Auto-contrôle [mesuré]** : nombre
de départs égal au `starts` des 9 `team-<équipe>.json`, `km` = ⌈1,4 × vol d'oiseau⌉ pour **16 `oneway` sur 16**. Graine : `i`.

### 8.2 Périmètre [mesuré]

**Du périmètre** : départ dans le `.poly` ; **intérieure** : disque D/2 dedans (32 points, lot 0) ; **de bord** sinon (Java déborde de la région).

| Équipe | requêtes Java | du périmètre (intérieures / bord) | réussites Java `head` : intérieures / bord | `head-alone` |
|---|---|---|---|---|
| A | 120 | 120 (70 / 50) | 37 / 16 | 41 / 18 |
| B | 120 | 60 (13 / 47) | 7 / 15 | 7 / 15 |
| C | 120 | 31 (21 / 10) | 0 / 0 (ses 2 réussites sont hors région) | — |
| D | 120 | 31 (27 / 4) | 4 / 0 | — |
| E | 77 | 77 (62 / 15) | 6 / 0 | 6 / 0 |
| F, G ; H, I | 2 + 2 ; 9 + 9 | 4 (4 / 0) ; 0 (départs hors région) | 0 ; — | — |
| **Total** | 579 | **323 (197 / 126)** | **54 / 31 = 85** | — |

### 8.3 Comptes

Chaque requête est jouée dans les **trois modes**. **Réussite** : un parcours rendu **et** accepté par `ord-loops-check` ;
**violation** : un parcours rendu qu'il refuse, par code. **Temps** : mur, un fil, `Engine` chargé, un tour jeté puis médiane de
trois, plus un tour froid ; machine, charge, commit et empreintes au rapport. Par requête, Java et Rust côte à côte (issue, L / D,
queue, croisements, temps).

### 8.4 Parité (retrait de la recherche Java, plan §11.10)

En **strict**, sur les 323 requêtes : (1) réussites Rust ≥ réussites Java `head` pour **chaque** équipe et au total (≥ 85), et
≥ 54 sur les intérieures ; l'écart à `head-alone`, plus exigeant (A : 59 au lieu de 53), est rapporté ; (2) **zéro violation** ;
(3) génération **médiane < 500 ms** pour D ≤ 100 km (172 requêtes intérieures, 47 réussites Java), p90 et maximum rapportés. Toute
requête intérieure réussie par Java et pas par Rust est listée (indice, sans point) et classée : trou non comblé, arête roulée
hors vue (§2.4), bord, défaut du moteur ; une seule de la dernière classe refuse la parité.

### 8.5 Gain et export

Réussite en **équipe d'abord** par équipe, surtout là où Java ne rendait rien (C, D, E, F, G, départs muets de A) ; part
`team_m` / L et `bridged_m` (médiane, p10) ; découverte comme plafond ; λ ∈ {2, 3, 5} comparés sur ces mesures. Un GeoJSON par
requête et par mode (candidats et leurs violations, départ accroché, arêtes d'équipe du disque) dans
`~/.cache/osm-route-details/graph/lot2/geojson/`, **jamais ailleurs** ; rapport JSON et résumé Markdown dans `lot2/` ; la note de
relevé du dépôt n'a que des agrégats par lettre.

## 9. Tests synthétiques (`MemSource` faits à la main, surcouches fabriquées)

1. **Vue `road`** : voie verte désignée revêtue ou sans surface gardée, non revêtue ou `bicycle=yes` écartée ; `track` `grade1`
   ou `asphalt` gardé, sans surface écarté. **Équipe** : chaîne roulée en entier ou en partie (refusée en strict) ; voie verte sans
   surface roulée en gravel seulement ; `MIXED` ; repli de z ; trou de 40 m depuis un cul-de-sac fictif comblé, de 120 m non,
   raccourci de 30 m entre deux routes roulées non comblé, trou sur un sens unique.
2. **Coût et virage** : poids par cellules touchées = calcul complet ; strict infini hors équipe et trous ; monotone en λ ; table
   `M` golden en natif et WASM ; 149° permis, 150° refusé ; cap d'une chaîne de 20 m prolongé ; avec coût de virage, distances
   égales à un Dijkstra de référence sur le graphe des lignes.
3. **Génération** : grille ; départ en impasse (queue ≤ 15 %) ; poche à sens unique → `NO_WAY_BACK` en deux arbres ; îlot,
   A→B trop court, point à 3 km → `NETWORK_TOO_SMALL`, `DISTANCE_TOO_SHORT`, `START_TOO_FAR` ; strict avec trou de 40 m réussi, de
   150 m refusé, équipe d'abord raccordée ; 2 × 2 sans retour par l'autre chaussée ; cap sur un graphe en étoile ; trois parcours
   à moins de 50 % de recouvrement ; même graine même sortie ; `stop` → `Stopped`, budget → `Budget`.
4. **Contrôleur** (par l'autre personne) : un cas positif et un négatif par violation ; départ, arrivée et fermeture au milieu
   d'une arête ; épingle d'une même way permise, connecteur d'une autre way refusé ; trou de 100 m permis depuis un cul-de-sac
   fictif, refusé depuis un nœud ordinaire. **Propriétés** : grilles aléatoires à sens uniques, tout parcours rendu passe le contrôleur.

## 10. Risques ouverts

- **Pas de queue dans la sonde** (8 départs sur 50 sans boucle) : la réussite réelle ne se lit qu'au banc. **Gravel au p90** : 344 ms
  dans la sonde à 100 km, × 1,4 sur le lecteur **[calculé]**, plus de 500 ms ; leviers : `gravel-prune` au cœur, rayon 0,4 × D.
- **Retour séquentiel** au lieu d'une paire optimale (piège du seul passage), l'ordre inverse en rattrape une partie, non mesuré ;
  **épingle par la way** : un connecteur dessiné dans la même way passerait ; **écarts à Java** : `footway` roulés en gravel,
  véloroutes stabilisées (441 km nationaux) ; **trous de plus de 100 m** ouverts en strict (p90 187 m) ; **bord** : 126 requêtes.

## 11. Sonde et reproduction (hors dépôt)

`~/.cache/osm-route-details/graph/lot2/scripts/` : `starts.py`, `scope.py`, `lot2-probe/` (Rust, copie d'`ord-graph` à la vue
corrigée, diff dans `ordg-copy-patch/` ; `views`, `outside`, `holes`, `chains`, `timing`, `gen`, `gen2`, `snap`), sorties JSON à
côté. Machine du lot 0 (Ryzen 7 4800H, 16 fils), `rustc 1.97.0`, `release`, charge 1,1 à 2,0.

## 12. Écarts (moteur codé, 15 novembre 2026)

Ce qui a été codé, et où le code s'écarte de la conception ci-dessus. Les chiffres sont **mesurés** sur la région avec
`crates/ord-loops/examples/region.rs` ou avec une sonde de développement hors dépôt (départs tirés au hasard sur les
arêtes de la surcouche : **ce n'est pas le banc du §8**, il reste à écrire ; aucun chiffre de parité n'est donc donné ici).

### 12.1 Livré

- **`ord-graph`** : `ViewKind::Road` redéfinie (§1.1 : voies vertes désignées non revêtues exclues, `track` revêtus) ;
  section `VIEW_HEADING` (`format_minor` 2, deux `u16` par arête de vue, écrite par `build_view`, facultative en lecture :
  le moteur recalcule les caps d'une vue de minor 1) ; `polyline_heading` / `heading_to_u16` ; vues `road` et `gravel`
  reconstruites sous `~/.cache/osm-route-details/graph/lot2/views/` (2,6 s les deux, contrôle exhaustif passé).
  `road` : 494 455 nœuds, 614 921 arêtes de vue, 940 510 arêtes de base vivantes, 64,8 Mo ; `gravel` : 702 348 / 893 341,
  92,1 Mo. Les tests du lot 0 bis, du lot 1 et des outils passent sans changement.
- **`crates/ord-loops`** (aucun `unsafe`, aucune E/S, fil ni horloge) : `Engine`, `TeamData`, `generate`, `check_route`
  (contrôle interne du §6, sur lequel le moteur juge chaque candidat), `route_geometry`, `Workspace`. Tests synthétiques
  du §9 (points 1 à 3) : 17 tests, dont arbre = Dijkstra de référence avec coûts de virage, trou de 40 m comblé / 120 m
  refusé / raccourci de 30 m non comblé, 149° permis / 151° refusé, cap d'une chaîne de 20 m prolongé, impasse de départ,
  poche à sens unique (`NO_WAY_BACK`), codes `START_TOO_FAR`, `END_TOO_FAR`, `NETWORK_TOO_SMALL`, `DISTANCE_TOO_SHORT`,
  `NO_TEAM_DATA`, `STOPPED`, aller simple, cap, trois parcours à moins de 50 % de recouvrement, même graine même sortie.
  Le §9 point 4 (contrôleur indépendant) et les « propriétés » ne sont pas faits ici.
- Mesures de chargement (release, mmap, machine du lot 0) : `Engine::new` 0,31 s ; index d'accroche paresseux, 0,7 s pour
  les deux vues (`Engine::warm` le paie d'avance) ; `TeamData::build` 0,15 s pour l'équipe A (11 199,7 km d'équipe en
  route, 24,9 km comblés ; le §2.3 annonçait 11 169 et 24,5 km : le moteur compte aussi les arêtes roulées sans
  exception de surface).
- Mesures de génération, **sonde de développement** (départs sur des arêtes de la surcouche, 30 départs par équipe, D = 50
  km, route, chaud, un fil ; réussite = parcours rendu, zéro violation du contrôle interne) : équipe d'abord, 27 à 30 réussites
  sur 30 pour A à G (médiane 74 à 114 ms, p90 141 à 327 ms) ; strict : A 17 / 30, B 21 / 30, les équipes C, D, E, F, G 0 / 30 sur
  ce tirage (réseau strict en morceaux de moins de 0,9 × D : `NETWORK_TOO_SMALL` ou `START_TOO_FAR` dans presque tous les cas).
  À 100 km, A : strict 11 / 20 (médiane 124 ms, p90 377), équipe d'abord 18 / 20 (262 ms, p90 477), gravel équipe d'abord
  20 / 20 (378 ms, p90 758, maximum 1,8 s). Découverte, centre d'Angers : 50 km 97 à 130 ms, 100 km gravel 283 à 330 ms.

### 12.2 Écarts à la conception

1. **Recherche à plat** (§4, §7). Le moteur relit une fois les vues en tableaux (nœuds, arêtes, chaînes, caps, CSR
   sortantes et entrantes) et cherche dessus, sans lecteur par cellules : l'état « local au disque » n'est pas utilisé
   (états denses à estampille, 16 o par arête orientée et par arbre, trois arbres). Plus rapide que les 36 / 55 ms du
   lecteur par cellules ; mémoire de l'ordre de 150 Mo pour la région. Le moteur reste sans E/S et prend des `GraphFile<B>`.
2. **Poids de type calculés au chargement**, pas dans `VIEW_WEIGHT` (§3) : changer `VIEW_WEIGHT` aurait cassé les mesures
   et les tests des lots 0 et 1. `ord-loops::cost` calcule `K_t` par arête de base (entiers, soixante-quatrièmes) et somme
   par chaîne : route sur `road`, gravel et VTT sur `gravel` (deux tableaux). Le coût est dans les 0,31 s de `Engine::new`.
3. **Listes d'équipe denses** (§3) : 12 o par arête de vue et par type au lieu de listes creuses (7 à 11 Mo par type, 30 Mo
   par équipe pour la région), lues directement dans la boucle de recherche ; les poids ne sont pas écrits par page et par
   requête, ils sont calculés à la relaxation (`λ·(W − A − B) + A·M[z̄] + B`, entiers, table `M` sans `exp` flottant).
4. **`road` : un `track` `grade1` de surface connue non revêtue est écarté** (la sonde le gardait) : « jamais de non-revêtu
   connu en route ». D'où 614 921 arêtes de vue au lieu de 615 595 (§1.1).
5. **Repli de `z` par arête**, pas par disque (§2.1) : une arête d'équipe sans passage du type demandé prend sa fréquence
   tous types.
6. **Accroche en strict sur les arêtes de vue entièrement d'équipe ou comblées**, pas sur les arêtes de base d'équipe d'une
   chaîne partielle (§1.2) ; le décalage est ramené à au moins 1 m des bouts d'arête de base (pas de morceau de longueur
   nulle, qui donne des caps arbitraires au contrôle).
7. **Virage : trois ajouts absents de la conception**, nés de la mesure (sur le premier jet, 228 refus `SHARP_TURN` pour 23
   réussites en strict). (a) Les caps du morceau partiel de départ et d'arrivée sont ceux de sa géométrie réelle
   (`Anchor::out_head` / `in_head`). (b) Un contrôle **rétroactif** : le cap de départ d'une chaîne de moins de 50 m déborde
   sur la suivante, que `turn_cost` ne voit pas ; il est refait quand le successeur est connu (`retro_ok`). (c) Une boucle
   **contre-exemple** : les transitions que le contrôle interne refuse en `SHARP_TURN` sont interdites aux recherches de
   la passe suivante (jusqu'à six passes, arrêt dès trois parcours). Les grappes de 100 m, les fenêtres qui traversent
   plusieurs chaînes courtes et les épingles restent vues seulement par le contrôle : les refus ne sont pas nuls (de l'ordre
   de 5 à 10 pour un parcours rendu), le temps tient.
8. **Zone de recherche d'un aller simple** : ellipse de foyers départ et arrivée de somme D (le disque de rayon D/2 vaut pour
   la boucle) ; les secteurs d'un aller simple sont comptés depuis le milieu départ-arrivée.
9. **Choix du point de demi-tour** : par secteur de 30°, le nœud T qui minimise `|aller(T) + ρ·retour_T2(T) − D|` (et non
   `aller(T) ≈ D/(1 + ρ)`) : identique pour une boucle, et valable pour un aller simple ; ρ se corrige par secteur à chaque
   essai (au plus trois essais par secteur) avec le rapport mesuré.
10. **Budget d'A*** : `6 × états fixés de T1 + 20 000` (le plancher évite d'épuiser le budget sur les petits réseaux).

### 12.3 Non fait

- **Famille polygone**, **ordre inverse** (retour par T2, aller par A*), **pénalité de couloir × 3 au retour**, **queue du
  côté de l'arrivée** pour un aller simple : le contrôle interne refuse `CORRIDOR_DOUBLED` et les queues trop longues, mais le
  moteur ne les évite pas à la recherche. Sur la sonde, les échecs en `NO_CANDIDATE` viennent surtout d'A* qui ne trouve
  aucun retour disjoint de l'aller (peu de candidats, pas de refus) : un réseau en arbre, que ni l'ordre inverse ni un
  polygone ne répareraient ; la part récupérable par un polygone (4 réussites Java sur 53) reste à mesurer au banc.
- `Generation::familiarity_fallback` vaut toujours `false` (le repli est par arête, point 5). `NoRouteDetail::Budget` est
  atteignable mais n'a pas de test (le budget plancher le rend rare).
- **WASM** : `cargo check -p ord-loops --target wasm32-unknown-unknown` **passe** (lancé après la revue, §14.6) ; le crate n'a
  ni E/S, ni fil, ni horloge, mais utilise `sin` / `cos` flottants pour les caps, dont les résultats peuvent différer à
  l'ulp entre plateformes : seuls les poids (entiers) sont garantis identiques au bit près.
- Le contrôleur indépendant et le banc rejoué sont écrits (§13) ; la campagne de mesure et le réglage restent à faire.

## 13. Contrôleur indépendant et banc (écrits, épreuve de bout en bout du 15 novembre 2026)

### 13.1 `crates/ord-loops-check`

Écrit depuis le §6 et `RouteGuarantees.java`, sans lire ni appeler le code de recherche ou de contrôle du moteur : du moteur,
seuls les types publics `Request`, `Route`, `Position`, `Step`, `Mode`, `RideType`. Géométrie, caps, réemploi, croisements,
couloir, admission et arête d'équipe y sont réécrits (`geo.rs`, `rules.rs`, `check.rs`). Un test par garantie et des
parcours valides (33 tests, sur des graphes faits à la main), plus un test de propriété (`tests/engine_vs_checker.rs` : 10
grilles à sens uniques tirés au hasard, tout parcours rendu par le moteur passe le contrôleur).

Choix et écarts de lecture, à relire :

1. **Fenêtre des caps** : la note dit « 30–50 m » ; `RouteGuarantees.pathHeading` prend `near = min(30, far / 2)`, soit
   **25–50 m** dès que 50 m de parcours sont disponibles. Le contrôleur suit Java (référence formelle).
2. **Épingle d'un nœud seul** : le §6 n'exempte l'épingle (même way, corde < 30 m, pas de portion droite en travers) que pour
   une grappe ; Java exempte aussi un nœud que l'équipe traverse (`riddenThrough`). Le contrôleur étend l'exemption à
   l'apex d'un seul nœud (`Config::hairpin_single_node`, vrai par défaut, `--hairpin-single-node false` au banc).
   Sur A, équipe d'abord : aucune différence de résultat avec ou sans.
3. **Admission** : `keep` de `road` et `gravel` réécrit d'après le §1.1 et la définition des vues ; le VTT prend `gravel`.
   **Arête d'équipe** : §2.1 ; « non-revêtu rugueux » (gravel) = surface connue parmi `ground, dirt, earth, grass, sand,
   mud, rock, clay, woodchips, salt, ice` (Java : `dirt, sand, grass, rock`) ; en VTT, `MTB` s'ajoute à `GRAVEL` et `MIXED`.
4. **Positions** : offsets en abscisse haversine ; `len_dm / 10` pour un morceau entier. Une position hors de son arête, ou
   incohérente avec le sens de marche, est un code ajouté, `BAD_OFFSET`.
5. **`TAIL_TOO_LONG`** ne peut pas se produire seul : la longueur recouverte est au plus la moitié des morceaux marqués, donc
   au plus 0,15 × D quand les deux zones tiennent. Le test vérifie qu'il accompagne `REUSE_OUTSIDE_TAIL`.
6. Sans surcouche, le mode strict refuse tout (`OFF_TEAM`).

### 13.2 `tools/ord-loops-bench`

Rejoue les requêtes `requests-<équipe>.jsonl` des équipes de la région dans les trois modes, un fil, tour froid puis
médiane de `--rounds` tours chauds ; contrôle chaque parcours rendu avec le contrôleur (et compte les refus du contrôle
interne : il n'y en a pas, le moteur ne rend que des candidats passés) ; écrit `<label>-report.json` (un enregistrement par
requête et par mode, issue Java en regard), `<label>-summary.md` (périmètre, comptes par équipe et par mode, temps, violations,
concordance moteur / contrôleur, parité du §8.4 avec verdict calculé, requêtes perdues, gain) et, à la demande, des GeoJSON
(`--geojson <lettre>:<i>`, `--geojson-violations`, `--geojson-lost`), tout sous `~/.cache/osm-route-details/graph/lot2/`.

Écart : les **départs ne sont pas reconstruits en Rust depuis l'export** ; l'outil lit `lot2/bench-starts.json`, produit par
`scripts/starts.py` hors dépôt (§11), et rejoue l'autocontrôle du §8.1 (`km` d'un aller simple). Les lettres viennent d'un
fichier local `lot2/team-letters.json`. Le périmètre (323 requêtes : 197 intérieures, 126 de bord ; Java 85 réussites : 54 / 31)
retombe sur le §8.2. Les « classes » du §8.4 ne sont pas automatiques : seul `bord` l'est ; le reste se lit au GeoJSON.

### 13.3 Épreuve de bout en bout (pas la campagne)

Release, machine du lot 0 non au repos (charge 1,8 à 2,0), 1 tour froid + 1 chaud, 969 requêtes × modes en 397 s. Agrégats :

| | strict | équipe d'abord | découverte |
|---|---|---|---|
| réussites / 323 (Java : 85) | 71 | 290 | 300 |
| intérieures / 197 (Java : 54) | 49 | 180 | 186 |
| violations du contrôleur | **0** sur 174 parcours | **0** sur 704 | **0** sur 809 |
| médiane / p90 / max, D ≤ 100 km, intérieures, ms | 1 / 80 / 223 (réussies : 68 / 149 / 223) | 172 / 407 / 828 | 130 / 252 / 422 |

Par équipe en strict (Rust / Java) : A 47 / 53, B 24 / 22, C 0 / 0, D 0 / 4, E 0 / 6, F, G 0 / 0. **Parité non tenue** : au total
71 < 85, intérieures 49 < 54, et A, D, E sont en dessous. 15 requêtes intérieures réussies par Java sont perdues : 8 `NETWORK_TOO_SMALL` (6 de E à
110 km, 2 de A en gravel et VTT), 5 `NO_CANDIDATE` (A, D), 2 `END_UNREACHABLE` (A, aller simple). Violation : aucune. Médiane : tenue (mais la médiane sur toutes les
requêtes est tirée par les échecs rapides ; sur les réussites 68 ms). En équipe d'abord, Rust réussit 207 des 238 requêtes
que Java a ratées. Concordance moteur / contrôleur : longueurs, croisements (1 écart en strict) et longueurs recouvertes
concordent ; la part d'équipe diffère de plus de 1 % de D sur 4 parcours (équipe d'abord) et 6 (découverte).

## 14. Corrections après la revue indépendante (15 novembre 2026)

Neuf constats de la revue ; chacun a été vérifié avant d'être corrigé. Chiffres **mesurés** au banc complet (`--label fixfinal2`,
1 tour froid + 1 chaud, machine non au repos), mêmes 323 requêtes que le §13.3.

### 14.1 Parité en strict : toujours non tenue, mieux classée

Strict : **78** réussites sur 323 (71 avant), intérieures **56 / 197** contre Java 54 (49 avant : **tenu** sur les intérieures) ; par équipe A 52 / 53,
B 24 / 22, D 2 / 4, E 0 / 6. Total 78 < 85 : les 22 / 126 de bord (Java 31) sont celles dont le disque sort de la région, hors
périmètre du graphe (classe `bord`). **Le critère (1) reste non tenu** (D et E, 22 de bord) ; aucun chiffre ne dit le contraire.
Zéro violation du contrôleur sur 189 + 685 + 814 parcours, concordance moteur / contrôleur sans écart (longueur, croisements, recouvrement, part d'équipe).

Les 15 requêtes intérieures perdues (identiques avant et après : ce que la jonction courte a gagné vient d'autres requêtes) sont maintenant
**classées automatiquement** par le banc, qui garde les diagnostics des échecs (`generate_with_diagnostics`, `Rec::reached_m`, `way_back_m`,
`rejected`) :

| Classe | Requêtes | Preuve |
|---|---|---|
| poche orientée : le réseau atteint ne ramène pas au départ | E 2, 8, 9, 33, 42, 59 (110 km) | 266 km atteints, 22 km ramènent au départ ; 18 arêtes de vue entre la zone qui ramène et la poche (13 à sens unique de 4 à 205 m, 5 non roulées de 73 à 249 m) ; avec trou 250 m et jonction 250 m : toujours 0 / 6 (`--hole-m`, `--connector-m`) |
| virage de ≥ 150° que Java exempte (`riddenThrough`, nœud roulé par l'équipe) | D 92, 112, 114, 119 | tous les candidats dans la tolérance (18 à 30) refusés en `SHARP_TURN` ; sans ce refus (essai jetable) le moteur rend les 4 parcours, que le contrôleur refuse même avec l'exemption d'un nœud seul |
| poche : le départ ne mène qu'à 84 m de réseau d'équipe | A 13 (VTT), A 53 (gravel), A 93 (aller simple) | arêtes d'équipe de 52 et 32 m entre des rues non roulées |
| arrivée hors du réseau strict atteint (accroche à 211 m) | A 23 | |
| recherche : aucun retour disjoint dans la tolérance | A 67 (30 km) | 4 à 6 candidats, aucun à ±10 % |

Ce ne sont **pas** des défauts de recherche que l'ordre inverse ou le polygone répareraient, sauf peut-être A 67 : un seul cas sur 15. La
classe D est une **décision de règle** (Java exempte un virage à un nœud que l'équipe traverse, §6 non : la revue demande de ne pas l'élargir), à
trancher par le propriétaire ; E dépend de liens que l'overlay n'a pas (appariement à 99,2 %).

### 14.2 Règle gravel et VTT

`team.rs` exigeait un parcours gravel pour toute surface `is_unpaved()` (`compacted`, `fine_gravel`, `gravel`, `unpaved`, `pebblestone`…). La
règle Java (`EdgeWeights.roughUnpaved`) et le §2.1 : seulement `path`, `footway` ou **surface rugueuse**. Corrigé dans le moteur et le contrôleur :
rugueux = `ground, dirt, earth, grass, sand, mud, rock, wood, clay, woodchips, salt, ice` et `tracktype` grade4 ou grade5 (ajout de `wood` et des deux
grades, absents des deux). Le contrôleur garde sa liste élargie à `ground, earth, mud, clay, woodchips, salt, ice` (Java : `dirt, sand, grass, rock,
wood`), déjà notée au §13.1 point 3. Tests : `a_rough_surface_is_presumed_only_for_a_ride_of_the_right_type_not_a_compacted_one` (moteur),
`in_gravel_only_a_rough_surface_is_presumed_like_java` (contrôleur). **Mesuré** : plus aucun désaccord de part d'équipe moteur / contrôleur (6 en découverte avant).

### 14.3 Ordre inverse (polygone et couloir non faits)

Ajouté : **ordre inverse** (`reverse.rs`) quand l'A* de l'ordre de base ne trouve pas de retour disjoint : retour = chemin de l'arbre retour depuis le demi-tour,
aller cherché **à rebours** du demi-tour au départ (A* arrière d'heuristique T1) sur le graphe privé du retour, budget propre. Effet **mesuré** : +1
parcours en strict (77 → 78, mêmes réglages par ailleurs), non mesuré à part en équipe d'abord ; il ne répare aucune des 15 requêtes (§14.1). **Non faits, avec leur raison mesurée** : la famille
polygone (aucune des classes perdues n'en relève, sauf A 67 peut-être), la pénalité de couloir × 3 au retour (le contrôle interne refuse déjà
`CORRIDOR_DOUBLED` ; le banc n'en compte aucun refus parmi les échecs), la queue côté arrivée. Si une campagne montre des pertes
de recherche, c'est là qu'il faut revenir. Essai écarté : interdire toute la fenêtre d'un groupe de virages refusés (au lieu de sa seule entrée)
a fait **perdre** 21 requêtes en équipe d'abord ; revenu à la seule entrée.

### 14.4 Contrôleur

- **Trous** : le contrôleur recalcule lui-même l'ensemble des arêtes comblées, D(100 m) (plus court chemin depuis chaque cul-de-sac fictif vers
  le plus proche nœud d'équipe) ∪ J(30 m) (chemins ≤ 30 m entre nœuds d'équipe), d'après la surcouche et sa propre règle d'admission, et n'accepte hors équipe que
  ces arêtes (`bridged_edges`). Il ne se contente plus d'une suite ≤ 100 m qui touche un cul-de-sac fictif. Tests : `strict_bridges_a_connector_of_at_most_30_m_between_two_team_nodes`
  et les trois tests de trou existants.
- **Virages** : l'exemption de l'apex d'un nœud seul est **désactivée par défaut** (`Config::hairpin_single_node = false`, option `--hairpin-single-node true` au
  banc) ; le test `a_hairpin_of_one_way_is_allowed_and_a_connector_of_another_way_is_not` vérifie le refus par défaut et le refus
  d'un connecteur d'une autre way ou d'une portion droite même avec l'option.
- **Croisements** : le moteur suit Java (le départ et l'arrivée sont des nœuds du graphe de requête, un seul pour une boucle) ; le contrôleur les
  ignorait, d'où l'écart de A 54 (moteur 1, contrôleur 0). Aligné sur Java ; **mesuré** : 0 écart de croisements sur 1 688 parcours.
- **Propriété moteur / contrôleur** (`tests/engine_vs_checker.rs`) : surcouches aléatoires à 85 % d'arêtes roulées (donc des trous) et **les trois
  modes** ; chaque mode rend des parcours et tous passent.

### 14.5 Temps, équipe d'abord, banc

- **Temps** : jugé sur les générations **réussies** (le banc donne aussi toutes les requêtes). D ≤ 100 km, intérieures, médiane / p90 / max :
  strict 59 / 147 / 208 ms (54 réussies ; toutes : 2 / 98 / 243), équipe d'abord 182 / 429 / 878 (158), découverte 125 / 249 / 413 (163). Cible de 500 ms en médiane : tenue ;
  le p90 d'équipe d'abord est à 429 ms et son maximum à 878 ms, sans marge.
- **Équipe d'abord** : une proposition qui garde moins de la moitié de la meilleure part d'équipe n'est plus gardée si la familiarité demandée est ≥ 0,25
  (`MIN_SHARE_RATIO`, `MIN_FAMILIARITY_FOR_SHARE`). Pas de test dédié : le comportement est une règle de sélection à vérifier au banc de gain.
- **Réseau atteint** : les diagnostics (`Diagnostics`, `Rec`) portent `reached_m`, `way_back_m`, `t2_settled`, `rejected`, `reverse_found` aussi sur échec.
  Le banc affiche les km d'équipe **hors vue** par équipe et type (A route : 899 km du type, 2 125 km tous types ; E : 3,6 km), qui ne
  sont pas la cause des pertes en strict de D et E (14 km et 4 km).
- Autres réglages tenus : file de queue du retour à 0,15 × D (elle était à 0,10 × D, sous la borne du contrôle) ; `ORD_*` de débogage retirés.

### 14.6 WASM

`rustup target add wasm32-unknown-unknown` puis `cargo check -p ord-loops --target wasm32-unknown-unknown` : **passe**.

### 14.7 Ce qui reste

Strict : parité non tenue (D 2 / 4, E 0 / 6, bord 22 / 31) ; la règle de virage de D est une décision du propriétaire, E demande des liens
que l'overlay n'a pas (ou une jonction plus longue testée à chaque campagne). Équipe d'abord et découverte : 290 et 302 réussites sur 323,
sans violation. Pas de campagne à 3 tours chauds sur machine au repos : un seul tour chaud ici, machine non au repos.

## 15. Campagne de parité (15 novembre 2026)

Relevé complet, protocole, tables et verdict : [ord-loops-lot2-mesures.md](ord-loops-lot2-mesures.md). Trois tours du banc
(1 froid + 3 chauds), le dernier sur le code final. En strict : 91 réussites sur 323 (Java 85), 61 sur 197 intérieures (Java 54),
**D 2 / 4 et E 0 / 6** : critère (1) non tenu ; zéro violation sur 1 811 parcours ; médiane de 67 ms. Équipe d'abord : 299 / 323.
Les six pertes de E sont des parcours Java que l'oracle du banc Java signale lui-même (virage de 150° et plus) ; le §14.1 les
disait « poche orientée », ce que l'accroche fortement connexe ci-dessous a levé (réseau atteint 192 km, 123 km qui ramènent).

Écarts à cette conception introduits par la campagne :

- **§1.2, accroche** : la composante de 0,9 × D est **fortement connexe** (sens respectés, virages ignorés), dans la vue comme
  dans le réseau strict, et une chaîne n'y compte que si ses deux bouts y sont. Sens ignorés, un départ au bout d'une rue à sens
  unique roulée une fois, qui sort du réseau de l'équipe, s'accrochait là et n'atteignait que 0,1 km. Un départ sans composante
  fortement connexe assez grande à 2 km échoue en `START_TOO_FAR` au lieu de `NETWORK_TOO_SMALL` ou `NO_WAY_BACK`.
- **§5.2, point de demi-tour** : dans un secteur, les nœuds d'un candidat écarté (hors tolérance ou refusé) ne sont plus essayés
  comme demi-tour (ils redonnaient le même cycle) ; six essais par secteur au lieu de trois (`Params::tries_per_sector`).
- **Diagnostics** : `Diagnostics::off_tolerance` et `Diagnostics::sharp_nodes`, pour classer les échecs.

Reste ouvert, à trancher par le propriétaire : l'exemption Java des virages à un nœud « traversé par une trace »
(`riddenThrough`), que la surcouche ne permet pas de reprendre telle quelle et dont dépendent les quatre pertes de D.
