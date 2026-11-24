# Calcul d'itinéraire de A à B — lot 1 bis : le banc sur tronçons de traces réelles

> Relevé du 24 novembre 2026 sur `develop` à `c3fc4ed` (1.17.0) plus le banc de cette note (non commité), binaire `release`,
> Rust 1.100.0, poste de développement (16 fils sur 8 cœurs physiques, 30 Go). Graphe et vues France `france-20261115-z11`.
> Moteur : [`ord-route-lot1.md`](ord-route-lot1.md) (§8 : préférence de voie, `way_factors`, `avoid_main_roads`). Plan appelant :
> Pédalons `docs/plans/2026-11-23-itineraire-velo-osm.md` (ledger `API-217`).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/route-bench/` (préfixe `full-`, journal
> `full.log`). Ici, des comptes et des agrégats seulement : ni coordonnée, ni identifiant de trace, d'équipe ou d'arête. **Aucun poids
> du moteur n'a changé** ; rien dans `ord-loops` ni `ord-server`.
>
> Marques : **[mesuré]** vient des sorties du banc (le `.md` et le `.json` d'agrégats) ; **[calculé]** d'un calcul fait ici sur les jsonl
> privés ou sur les agrégats ; **[hypothèse]** n'est pas vérifié. Sans marque dans un tableau : mesuré. Jaccard en **points** (× 100).
> Intervalles : 95 %, bootstrap **par trace** (1 000 tirages), pas par tronçon. Temps pris à 16 fils simultanés : pessimistes pour le service.

## 0. En bref

- **Le modèle neutre reproduit bien les petits tronçons et mal les grands.** Route, Jaccard moyen 38,4 [37,3 ; 39,7] sur A et
  39,1 [37,9 ; 40,4] sur B ; de 60,7 à 63,1 entre 2 et 5 km, 32,7 à 32,8 entre 5 et 15 km, 16,7 à 17,1 entre 15 et 40 km. Gravel
  nettement plus bas : 21,9 [20,6 ; 23,1] sur A, 20,5 [19,2 ; 21,8] sur B. L'itinéraire est **plus court** que le réel (détour
  `L / L_ref` P50 0,95 en route, 0,89 en gravel) : au-delà de quelques kilomètres, la personne ne prenait pas le plus court.
- **Répartition par case, route : `secondary` est juste** (réel 23,6 %, neutre 23,0 % sur A ; 22,7 et 23,8 % sur B). L'écart est
  ailleurs : **trop de `tertiary`** (48,0 % contre 42,7 %), **pas assez de petites routes** (13,6 contre 17,3 %) ni de chemins
  (1,0 contre 2,1 %), et presque **aucun non revêtu** (0,1 contre 2,4 %). Gravel : **trop de voies cyclables ou vertes** (19,2 contre
  12,7 %), pas assez de chemins (38,9 contre 44,2 %).
- **Le neutre est un optimum local étroit.** Le balayage case par case ne gagne au mieux que 0,5 point ; `best2` (choisi sur A) gagne
  **+0,8 [+0,2 ; +1,3] point sur B en route**, **+0,0 [−0,4 ; +0,6] en gravel**. Le gain route tient sur B mais vaut 2 % du Jaccard.
- **`secondary` est la case la plus raide** : × 1,41 fait tomber sa part de 23,0 à 8,4 %, × 0,71 la monte à 46,4 %. C'est ce que
  `avoid_main_roads` exploite : **× 2 sur `secondary` la ramène à 4,8 %** (−79 % [calculé]) pour un détour P90 de 1,05 à 1,15 ;
  × 1,41 en ôtait déjà 63 % [calculé] avec un P90 de 1,09.
- **Les crans du curseur vont dans le bon sens et de beaucoup** : en route au neutre 0,25, de `use_roads` 0 à 1, `secondary` passe de
  12,8 à 49,5 % et les voies cyclables de 13,3 à 0,4 % ; en gravel, les chemins de 41,7 à 6,4 %.

## 1. Protocole

### 1.1 Population

Toutes les traces de l'export privé (3 377 lignes, xxh3 `ca53159cc90498d6`), posées sur le graphe de la France par `retouche::posed::pose`
(appariement, marche, coutures : les mêmes que le relevé de la retouche, [`ord-retouche-lot0-mesures.md`](ord-retouche-lot0-mesures.md)).
Les traces avec **au moins une anomalie d'ordre** (§2.4 de ce relevé) sont écartées à l'entrée, comme il le recommandait ; le filtre
n'existait pas, il est ajouté au banc (champ `Posed::order_anomalies`, la valeur que calculait déjà la projection).

| | traces |
|---|---|
| lignes lues | 3 377 |
| posées (ROAD, GRAVEL, MTB lisibles et non vides) | 3 355 |
| écartées pour anomalie d'ordre | 169 (5,0 % [calculé]) |
| gardées | 3 186 |
| avec au moins un tronçon | 3 175 |

### 1.2 Tronçons

Par trace, au plus un tronçon par bande de vol d'oiseau : **2–5 km, 5–15 km, 15–40 km** (bornes comprises). Un tronçon va d'une
jonction `Node` de la marche à une autre, plus loin dans le sens de la marche, **les deux jambes voisines de chaque bout étant dans la
vue du type** (le départ et l'arrivée tombent sur des chaînes que le moteur connaît). Départ et arrivée de la requête : les positions
de ces deux nœuds. La **référence** est la sous-marche entre eux (jambes et fractions d'arête de l'appariement), sa longueur `L_ref`
celle de ses jambes.

Tirage reproductible (graine `20261124`, hachage de la trace) : jusqu'à 64 départs tirés parmi les nœuds de la marche, et pour un
départ, une arrivée tirée uniformément parmi les nœuds plus loin qui tiennent les trois règles ; le premier départ qui en a gagne.
Règles : vol d'oiseau dans la bande ; `L_ref ≤ 3 ×` vol d'oiseau (aller-retour, boucle) ; **somme** des trous non appariés de la
sous-marche au plus 5 % de `L_ref`.

| type | tronçons | 2–5 km | 5–15 km | 15–40 km | traces A / B | vol d'oiseau P50 [calculé] | `L_ref` / vol d'oiseau P50 [calculé] |
|---|---|---|---|---|---|---|---|
| route | 6 328 | 2 278 | 2 266 | 1 784 | 1 180 / 1 098 | 7,6 km | 1,33 |
| gravel | 1 977 | 885 | 880 | 212 | 443 / 442 | 5,3 km | 1,58 |
| VTT | 29 | 12 | 12 | 5 | 6 / 6 | 5,4 km | 1,78 |
| **total** | **8 334** | 3 175 | 3 158 | 2 001 | | | |

La bande longue est clairsemée en gravel (212) et presque vide en VTT (5) : les traces y sont plus courtes ou trop sinueuses pour la
règle du détour. **Le VTT (29 tronçons, 12 traces) n'est pas interprétable** ; ses chiffres sont dans les sorties, pas dans les
conclusions. Trous de la référence : 0,58 % de `L_ref` en route, 1,09 % en gravel [calculé]. Référence hors de la vue du type
(trottoir en route, sentier interdit…) : 2,7 % des kilomètres [mesuré], que le moteur ne peut pas reproduire.

Moitiés : **A** si `xxh3(hachage de la trace)` est pair, **B** sinon ; tous les tronçons d'une trace sont dans la même moitié.

### 1.3 Requêtes

`RouteRequest` : type de la trace (`Posed::ride`), `Mode::Discovery`, **sans équipe** (goût commun), `RouteTune::default()`, moteur aux
`Params::default()` (goût commun, roulabilité du lot 5 : ceux du service). Par tronçon et par phase, la fenêtre de la zone 0 est bâtie
une fois ; une recherche neutre de chauffe (non chronométrée) fixe la zone : sans chemin, zone 1 (**38 tronçons**), puis échec commun
(**3 tronçons `no_route`**). L'accroche et l'existence d'un chemin ne dépendent pas de `way_q8` (aucun multiplicateur ne rend une chaîne
interdite) : un échec commun vaut pour toutes les variantes sans les rejouer. Échappement de fenêtre : rejoué une fois dans une fenêtre
élargie, comme `ord_loops::route` (**0 sur 515 012 recherches**). Virage serré en seconde passe : 59 itinéraires neutres sur 8 334 [calculé].

### 1.4 Variantes de `way_q8`

Cases : 0 grande route (`primary`, `trunk`), 1 `tertiary`, 2 petite route, 3 rue, 4 voie cyclable ou verte, 5 chemin, 6 `secondary`.

1. **Phase 1** : `neutral` ; `avoid` = `avoid_main_roads(WAY_NEUTRAL)` ; le **balayage** une case à la fois à × 0,5, 0,71, 1,41, 2, 3
   (q8 128, 182, 361, 512, 768), les autres à 256 (35 variantes) ; les **crans du curseur** `way_factors(ride, u, 0,25)` pour
   u ∈ {0 ; 0,25 ; 0,5 ; 0,75 ; 1}, et `way_factors(road, u, 1)` en route.
2. **Phase 2** : `best1`, case par case le multiplicateur du balayage (256 compris) qui maximise le Jaccard moyen **sur A**, par type ;
   et ses **voisins** d'un cran vers le haut et vers le bas dans {128, 182, 256, 361, 512, 768}, une case à la fois.
3. **Phase 3** : `best2`, pour chaque case le voisin qui bat `best1` sur A (sinon la valeur de `best1`), rejoué.

La descente est **une passe parallèle** (Jacobi) : les 14 voisins sont tous jugés autour de `best1`, puis combinés. Une passe séquentielle
(Gauss-Seidel) demanderait sept tours de la population ; non jouée.

### 1.5 Mesures

Par (tronçon, variante) : trouvé ou échec ; `L` (`route.length_m`) ; détour `L / L_ref` ; **recouvrement en longueur arête de base par
arête de base**, chaque côté réuni par arête (une arête prise deux fois compte une fois) puis intersecté avec l'autre, les arêtes
partielles des bouts à leurs fractions : précision = commun / itinéraire, rappel = commun / référence, **Jaccard =
commun / (itinéraire + référence − commun)** (critère principal, **un échec compte 0**) ; kilomètres par case et non revêtus (`offroad`)
des deux côtés ; temps de recherche à chaud. Précision, rappel, détour et parts par case : sur les tronçons trouvés.

### 1.6 Commandes, durée, mémoire, empreintes

| Campagne | Commande (`./target/release/ord-loops-bench`) | Durée | Empreintes (lignes sans temps / tronçons) |
|---|---|---|---|
| essai, 16 fils | `route-play --teams B --limit 20 --label trial16 --threads 16` | 21 s | `c3a15c2d7f45a8c7` / `d1711bb053d529f8` |
| essai, 2 fils | `route-play --teams B --limit 20 --label trial2 --threads 2` | — | `c3a15c2d7f45a8c7` / `d1711bb053d529f8` |
| **complète** | `route-play --label full --threads 16` | **16 min 28 s** | **`93a077c77074a1ec`** / **`b33848c8e8d14eca`** |

**Déterminisme** : deux fils contre seize sur l'essai (47 tronçons, 2 793 lignes), mêmes empreintes et même Markdown aux temps près.
L'empreinte des lignes porte sur tout sauf le temps (empreinte de chaque itinéraire comprise).
Mémoire : 19 Go disponibles avant ; pic résident 12,6 Go, anonyme 9,1 Go (dont 7,5 Go pour l'export et l'appariement) ; 16 fils gardés,
trois tronçons par trace (rien à réduire, la campagne tient en 17 min pour une cible d'une heure). Phases : pose 17 s, phase 1 635 s,
phase 2 247 s, phase 3 79 s. 515 012 recherches.

## 2. Le modèle neutre

### 2.1 Recouvrement [mesuré]

| type | bande | moitié | tronçons (traces) | échecs | J moyen [IC] | J médian [IC] | précision | rappel | détour P50 / P90 | ms P50 / P90 |
|---|---|---|---|---|---|---|---|---|---|---|
| route | toutes | A | 3 263 (1 180) | 0,1 % | **38,4 [37,3 ; 39,7]** | 23,8 [22,0 ; 25,8] | 47,0 % | 45,2 % | 0,95 / 1,05 | 13 / 28 |
| route | toutes | B | 3 065 (1 098) | 0,0 % | **39,1 [37,9 ; 40,4]** | 24,6 [22,5 ; 26,4] | 47,9 % | 46,0 % | 0,95 / 1,05 | 13 / 28 |
| route | 2–5 km | A | 1 180 | 0,1 % | 60,7 [58,6 ; 62,8] | 67,4 [63,1 ; 73,5] | 68,3 % | 68,0 % | 1,00 / 1,09 | 7 / 19 |
| route | 2–5 km | B | 1 098 | 0,0 % | 63,1 [60,9 ; 65,3] | 72,6 [68,7 ; 78,9] | 70,1 % | 70,1 % | 1,00 / 1,10 | 7 / 20 |
| route | 5–15 km | A | 1 171 | 0,2 % | 32,8 [31,0 ; 34,8] | 18,2 [15,8 ; 20,5] | 42,3 % | 39,8 % | 0,92 / 1,04 | 12 / 23 |
| route | 5–15 km | B | 1 095 | 0,0 % | 32,7 [30,7 ; 34,6] | 19,8 [17,1 ; 21,7] | 42,7 % | 39,9 % | 0,92 / 1,05 | 12 / 23 |
| route | 15–40 km | A | 912 | 0,0 % | 16,7 [15,3 ; 18,2] | 7,9 [6,2 ; 8,9] | 25,4 % | 22,7 % | 0,82 / 1,01 | 22 / 35 |
| route | 15–40 km | B | 872 | 0,0 % | 17,1 [15,7 ; 18,7] | 9,2 [7,9 ; 10,5] | 26,5 % | 23,3 % | 0,81 / 1,00 | 21 / 35 |
| gravel | toutes | A | 998 (443) | 0,0 % | **21,9 [20,6 ; 23,1]** | 14,1 [13,0 ; 15,4] | 32,6 % | 30,0 % | 0,89 / 1,11 | 15 / 32 |
| gravel | toutes | B | 979 (442) | 0,0 % | **20,5 [19,2 ; 21,8]** | 14,0 [12,3 ; 15,8] | 31,3 % | 29,0 % | 0,89 / 1,14 | 15 / 32 |
| gravel | 2–5 km | A | 443 | 0,0 % | 32,7 [30,2 ; 35,1] | 24,8 [22,3 ; 28,2] | 44,9 % | 43,2 % | 0,97 / 1,16 | 11 / 25 |
| gravel | 2–5 km | B | 442 | 0,0 % | 28,2 [25,9 ; 30,6] | 22,1 [20,1 ; 24,8] | 40,2 % | 38,9 % | 0,98 / 1,20 | 12 / 25 |
| gravel | 5–15 km | A | 442 | 0,0 % | 13,8 [12,4 ; 15,1] | 8,9 [7,7 ; 11,1] | 23,6 % | 20,2 % | 0,81 / 1,06 | 17 / 31 |
| gravel | 5–15 km | B | 438 | 0,0 % | 14,6 [13,1 ; 15,9] | 10,1 [8,3 ; 11,3] | 25,0 % | 21,6 % | 0,80 / 1,07 | 16 / 30 |
| gravel | 15–40 km | A | 113 | 0,0 % | 11,2 [8,8 ; 13,9] | 5,9 [4,7 ; 8,3] | 19,1 % | 17,2 % | 0,86 / 1,02 | 33 / 49 |
| gravel | 15–40 km | B | 99 | 0,0 % | 12,0 [9,2 ; 14,7] | 6,6 [4,6 ; 10,9] | 20,0 % | 18,3 % | 0,88 / 1,02 | 32 / 45 |
| VTT | toutes | A / B | 15 / 14 (6 / 6) | 0,0 % | 43,1 [25,3 ; 62,8] / 36,9 [25,0 ; 47,6] | — | — | — | — | — |

Distribution du Jaccard neutre (A et B réunies) [calculé] :

| type, bande | J ≥ 80 | J ≥ 50 | J < 10 |
|---|---|---|---|
| route 2–5 km | 45,1 % | 60,9 % | 13,4 % |
| route 5–15 km | 16,2 % | 27,4 % | 36,9 % |
| route 15–40 km | 3,4 % | 8,9 % | 54,2 % |
| gravel 2–5 km | 6,9 % | 20,1 % | 23,3 % |
| gravel 5–15 km | 0,5 % | 3,5 % | 51,1 % |
| gravel 15–40 km | 0,0 % | 2,8 % | 61,8 % |

La distribution est **bimodale** (la médiane du court route, 67 à 73, est au-dessus de la moyenne, celle du long, 8 à 9, très en dessous) :
un tronçon est soit retrouvé presque en entier, soit par un autre chemin.

### 2.2 Réel contre itinéraire, par case (toutes bandes) [mesuré]

Parts des kilomètres, sur les tronçons trouvés par le neutre. Route : 48 828 km réels sur A, 46 604 sur B.

| case | route réel A / B | neutre A / B | `avoid` A / B | `best2` A / B |
|---|---|---|---|---|
| 0 grande route | 2,9 % / 2,9 % | 1,5 % / 1,4 % | 0,8 % / 0,9 % | 2,4 % / 2,1 % |
| 1 `tertiary` | 42,7 % / 43,3 % | **48,0 % / 48,8 %** | 55,1 % / 55,7 % | 52,3 % / 51,9 % |
| 2 petite route | 17,3 % / 17,8 % | **13,6 % / 13,1 %** | 20,5 % / 20,7 % | 13,9 % / 13,4 % |
| 3 rue | 6,7 % / 6,6 % | 8,8 % / 8,3 % | 12,0 % / 11,6 % | 4,2 % / 4,1 % |
| 4 cyclable ou verte | 4,6 % / 4,4 % | 4,0 % / 3,7 % | 6,0 % / 5,8 % | 1,3 % / 1,2 % |
| 5 chemin | 2,1 % / 2,3 % | 1,0 % / 1,0 % | 0,7 % / 0,7 % | 0,2 % / 0,2 % |
| 6 `secondary` | **23,6 % / 22,7 %** | **23,0 % / 23,8 %** | 4,8 % / 4,7 % | 25,8 % / 27,1 % |
| non revêtu | 2,4 % / 2,5 % | 0,1 % / 0,0 % | 0,1 % / 0,1 % | 0,0 % / 0,0 % |

Gravel : 12 378 km réels sur A, 11 594 sur B.

| case | gravel réel A / B | neutre A / B | `avoid` A / B | `best2` A / B |
|---|---|---|---|---|
| 0 grande route | 1,0 % / 0,7 % | 0,8 % / 0,6 % | 0,7 % / 0,5 % | 0,7 % / 0,5 % |
| 1 `tertiary` | 9,9 % / 9,0 % | 9,4 % / 9,4 % | 10,3 % / 9,6 % | 11,9 % / 11,7 % |
| 2 petite route | 14,0 % / 13,4 % | **10,1 % / 9,5 %** | 10,6 % / 10,1 % | 11,1 % / 10,2 % |
| 3 rue | 14,4 % / 15,2 % | 17,5 % / 18,4 % | 19,7 % / 20,6 % | 10,9 % / 11,6 % |
| 4 cyclable ou verte | 12,7 % / 13,0 % | **19,2 % / 18,7 %** | 18,6 % / 18,8 % | 21,1 % / 20,8 % |
| 5 chemin | 44,2 % / 44,7 % | **38,9 % / 39,5 %** | 38,9 % / 39,2 % | 39,1 % / 39,9 % |
| 6 `secondary` | 3,8 % / 4,0 % | 4,1 % / 3,9 % | 1,2 % / 1,2 % | 5,3 % / 5,2 % |
| non revêtu | 47,4 % / 47,6 % | 42,4 % / 42,6 % | 41,7 % / 41,7 % | 43,3 % / 43,4 % |

Par bande (sorties, section « Réel contre neutre, par bande ») : en route, l'excès de `tertiary` et le manque de petites routes
grandissent avec la distance (15–40 km, A : `tertiary` 42,1 contre 48,1 %, petites routes 19,0 contre 14,8 %) ; le manque de chemins
est surtout court (2–5 km : 2,8 contre 0,6 %). En gravel, l'excès de voies cyclables est le même dans toutes les bandes (+4 à +8 points).

### 2.3 Balayage une case à la fois : ΔJ / neutre en points [mesuré]

Différence appariée (mêmes tronçons), toutes bandes. Route :

| case | A : × 0,5 | × 0,71 | × 1,41 | × 2 | × 3 | B : × 0,5 | × 0,71 | × 1,41 | × 2 | × 3 |
|---|---|---|---|---|---|---|---|---|---|---|
| 0 grande route | −1,3 | +0,2 | −0,5 | −0,7 | −0,9 | −1,2 | **+0,3 [+0,0 ; +0,7]** | −0,8 | −1,2 | −1,5 |
| 1 `tertiary` | −3,4 | −1,5 | −4,7 | −12,5 | −16,1 | −3,5 | −1,5 | −4,7 | −12,9 | −16,9 |
| 2 petite route | −6,0 | −2,8 | −0,2 | −1,0 | −2,2 | −5,8 | −2,6 | −0,2 | −1,4 | −2,3 |
| 3 rue | −8,5 | −3,7 | **+0,5 [+0,1 ; +0,9]** | +0,2 | −0,6 | −9,3 | −3,9 | +0,2 [−0,2 ; +0,7] | −0,3 | −0,9 |
| 4 cyclable ou verte | −3,8 | −1,7 | +0,1 | +0,1 | +0,1 | −4,6 | −2,4 | +0,0 | +0,1 | −0,0 |
| 5 chemin | −0,6 | −0,2 | +0,3 | **+0,3 [+0,1 ; +0,5]** | +0,3 | −0,5 | −0,2 | +0,2 | **+0,3 [+0,2 ; +0,5]** | +0,4 |
| 6 `secondary` | −3,3 | −0,8 | −4,1 | −7,0 | −8,5 | −4,3 | −1,3 | −4,1 | −7,2 | −8,6 |

Gravel : aucune valeur ne gagne plus de 0,5 point sur A (rue × 1,41 : +0,5 [−0,1 ; +1,1]) ; sur B, aucune ne gagne : au mieux
+0,2 [−0,3 ; +0,7] (sorties). Les pertes les plus fortes : chemin × 3 (−5,3 sur A, −4,8 sur B), `tertiary` × 0,5 (−3,2 / −3,7), rue × 0,5 et × 3,
`secondary` × 0,5 (−3,3 / −3,6).

Lecture : **les poids neutres sont au sommet ou à un cran du sommet sur chaque case**, des deux moitiés. Les deux seules cases
« dures » en route sont `tertiary` et `secondary` (toute modification coûte, dans les deux sens), les cases « molles » rue, cyclable
et chemin (monter leur coût ne coûte rien ou gagne un peu).

### 2.4 `best1`, `best2` et leur tenue sur B [mesuré]

| type | `best1` (× par case 0…6) | `best2` (× par case 0…6) | ΔJ `best1` A / B | **ΔJ `best2` A / B** |
|---|---|---|---|---|
| route | 0,71 · 1 · 1 · 1,41 · 1,41 · 2 · 1 | 1 · 1 · 1 · **1,41** · **1,41** · **2** · 1 | +0,8 [+0,3 ; +1,5] / +0,3 [−0,3 ; +0,9] | +1,3 [+0,8 ; +1,8] / **+0,8 [+0,2 ; +1,3]** |
| gravel | 1 · 1 · 1 · 1,41 · 1 · 0,71 · 1 | **1,41** · 1 · 1 · **1,41** · 1 · 1 · 1 | +0,4 [−0,3 ; +1,1] / −0,2 [−0,8 ; +0,4] | +0,4 [−0,1 ; +1,0] / **+0,0 [−0,4 ; +0,6]** |
| VTT (12 traces) | 1 · 1 · 1 · 1 · 1 · 1 · 0,71 | identique | +0,7 / +1,7 | +0,7 [−0,2 ; +2,5] / +1,7 [+0,0 ; +5,7] |

En route, `best1` combine des optimums pris un à un et **perd sur B** ce qu'il gagnait sur A : la grande route à × 0,71, bonne seule
(+0,2 / +0,3), monte à 7,5 % des kilomètres une fois les rues et les chemins renchéris (1,5 % au neutre) ; la descente la ramène à 1. Par
bande, `best2` route sur B : +1,1 [+0,0 ; +2,1] entre 2 et 5 km, +0,4 [−0,4 ; +1,3] entre 5 et 15 km, +0,8 [+0,2 ; +1,4] entre 15 et 40 km.

**Le gain du Jaccard éloigne les parts par case du réel** : `best2` route ramène les voies cyclables de 3,7 à 1,2 % (réel 4,4 %), les
chemins de 1,0 à 0,2 % (réel 2,3 %), les rues de 8,3 à 4,1 % (réel 6,6 %), sur B. Le Jaccard récompense la suppression de raccourcis par
des rues et des chemins **que la personne n'a pas pris** ; il ne récompense pas d'en prendre d'autres. Les deux critères ne se
recouvrent pas.

## 3. `avoid_main_roads` et les crans du curseur

### 3.1 `avoid_main_roads` [mesuré, sauf mention]

| type, moitié | J neutre → `avoid` | ΔJ [IC] | `secondary` | grande route | détour P50 / P90 | ms P50 |
|---|---|---|---|---|---|---|
| route A | 38,4 → 31,0 | −7,4 [−8,3 ; −6,5] | 23,0 → 4,8 % | 1,5 → 0,8 % | 0,95 / 1,05 → 0,99 / 1,15 | 13 → 23 |
| route B | 39,1 → 31,3 | −7,8 [−8,9 ; −6,9] | 23,8 → 4,7 % | 1,4 → 0,9 % | 0,95 / 1,05 → 0,99 / 1,16 | 13 → 23 |
| gravel A | 21,9 → 20,8 | −1,1 [−1,8 ; −0,4] | 4,1 → 1,2 % | 0,8 → 0,7 % | 0,89 / 1,11 → 0,90 / 1,13 | 15 → 27 |
| gravel B | 20,5 → 19,8 | −0,6 [−1,2 ; −0,1] | 3,9 → 1,2 % | 0,6 → 0,5 % | 0,89 / 1,14 → 0,90 / 1,16 | 15 → 27 |

Les kilomètres vont **dans le sens attendu** : en route, `secondary` perd 79 à 80 % de sa part, la grande route 36 à 47 % [calculé] ;
ils passent surtout sur `tertiary` (+7 points) et les petites routes (+7). La baisse du Jaccard est attendue : les personnes du banc
roulaient 23 % sur `secondary` et n'avaient pas demandé à l'éviter. Elle est plus forte en court (2–5 km, B : −13,2 [−14,9 ; −11,4]).

Le balayage de la case 6 seule (route, A ; B à ±0,4 point près) donne la courbe de la pénalité :

| `secondary` × | 0,71 | 1 | 1,41 | 2 | 3 |
|---|---|---|---|---|---|
| part de `secondary` | 46,4 % | 23,0 % | 8,4 % | 4,7 % | 3,4 % |
| détour P90 | 1,05 | 1,05 | 1,09 | 1,14 | 1,18 |
| ΔJ | −0,8 | — | −4,1 | −7,0 | −8,5 |

La case est **raide** : le goût commun met `secondary` au prix d'une rue (§8 de la note du moteur), si bien que × 1,41 ôte déjà 63 % de
ses kilomètres [calculé]. × 2 en ôte 80 %, × 3 en ôterait 85 % [calculé]. La grande route (case 0) pèse peu en route (1,5 % au
neutre) : son × 1,5 ne déplace presque rien.

### 3.2 Crans du curseur (route, toutes bandes) [mesuré]

| neutre du profil, `use_roads` | J A / B | ΔJ B [IC] | `secondary` | `tertiary` | petite route + rue | cyclable ou verte | chemin |
|---|---|---|---|---|---|---|---|
| réel (B) | — | — | 22,7 % | 43,3 % | 24,5 % | 4,4 % | 2,3 % |
| 0,25, u = 0 | 34,2 / 34,4 | −4,7 [−5,4 ; −4,1] | 13,2 % | 43,9 % | 28,0 % | 12,3 % | 1,6 % |
| 0,25, u = 0,25 (neutre) | 38,4 / 39,1 | — | 23,8 % | 48,8 % | 21,4 % | 3,7 % | 1,0 % |
| 0,25, u = 0,5 | 39,0 / 39,8 | +0,7 [+0,1 ; +1,3] | 36,9 % | 44,7 % | 13,9 % | 1,0 % | 0,5 % |
| 0,25, u = 0,75 | 38,0 / 38,2 | −0,9 [−1,7 ; −0,1] | 43,7 % | 39,6 % | 10,9 % | 0,7 % | 0,3 % |
| 0,25, u = 1 | 36,4 / 36,5 | −2,6 [−3,6 ; −1,7] | 48,9 % | 33,8 % | 8,4 % | 0,4 % | 0,2 % |
| 1, u = 0 | 21,7 / 21,8 | −17,3 [−18,4 ; −16,2] | 4,0 % | 24,4 % | 43,5 % | 25,6 % | 2,0 % |
| 1, u = 0,5 | 28,7 / 28,4 | −10,7 [−11,6 ; −9,8] | 7,5 % | 33,5 % | 36,2 % | 19,9 % | 2,2 % |
| 1, u = 1 (neutre) | 38,4 / 39,1 | — | 23,8 % | 48,8 % | 21,4 % | 3,7 % | 1,0 % |

Gravel, neutre 0,25 : u = 0 fait monter les chemins de 39,5 à 42,1 % et le non revêtu de 42,6 à 45,4 % (B) ; u = 1 les fait tomber à
6,9 et 7,1 %, `secondary` à 39,9 % ; J de 20,5 à 10,4.

Les crans **déplacent les kilomètres dans le sens attendu, de façon monotone** sur toutes les cases lues. L'amplitude est
**asymétrique** au neutre 0,25 : un cran vers les routes (0,25 → 0,5) double `secondary` (23,8 → 36,9 %), un cran vers les pistes
(0,25 → 0) la divise par deux et triple les voies cyclables (3,7 → 12,3 %) ; au neutre 1, tout le curseur va d'un côté, jusqu'à 25,6 %
de voies cyclables.

## 4. Ce que les chiffres disent

**(a) Le modèle neutre reproduit-il les tronçons réels ?** Sur les courts, oui pour la route : entre 2 et 5 km, Jaccard moyen 60,7 et
63,1 (A et B), médiane près de 70, 45 % des tronçons à 80 ou plus [calculé]. Il les reproduit de moins en moins avec la distance : 33
entre 5 et 15 km, 17 entre 15 et 40 km, et un tronçon long sur deux partage moins de 10 % avec la trace [calculé]. Le gravel est deux fois
plus bas à toute distance (33 et 28 en court, 11 à 15 au-delà). Les deux moitiés donnent les mêmes chiffres à leurs intervalles près.
L'itinéraire est presque toujours **plus court** que le réel (détour P50 0,95 en route, 0,81 à 0,82 entre 15 et 40 km ; P90 ≈ 1,0) :
ce qui sépare l'itinéraire du réel au-delà de 5 km tient surtout à ce que le réel n'est pas un plus court chemin (§4 e), pas au réglage
d'une case.

**(b) Où l'itinéraire s'écarte-t-il du réel ?** En route, **pas sur `secondary`** : réel 23,6 / 22,7 %, neutre 23,0 / 23,8 % (A / B). Il
prend **trop de `tertiary`** (+5,3 et +5,5 points) et **pas assez de petites routes** (−3,7 et −4,7), trop de rues (+2,1 et +1,7), la
moitié des chemins (1,0 contre 2,1 à 2,3 %) et presque **aucun non revêtu** (0,0–0,1 contre 2,4–2,5 %) ; un peu moins de grandes routes
(1,5 contre 2,9 %). En gravel, **trop de voies cyclables ou vertes** (+6,5 et +5,7 points), trop de rues (+3,1 et +3,2), **pas assez de
chemins** (−5,3 et −5,2) et de petites routes (−3,9) ; `secondary` juste (3,8–4,0 contre 3,9–4,1 %).

**(c) Quels multiplicateurs donnent le meilleur recouvrement, et le gain tient-il sur B ?** Le neutre est déjà à un cran de l'optimum de
chaque case, prise seule. La table `best2` route (rue × 1,41, voie cyclable × 1,41, chemin × 2) gagne **+0,8 [+0,2 ; +1,3] point sur
B** (+1,3 sur A) : le gain tient, il est faible (2 % du Jaccard [calculé]), et il éloigne les parts de rues, de voies cyclables et de
chemins du réel (§2.4). En gravel, `best2` (grande route × 1,41, rue × 1,41) **ne gagne rien sur B** (+0,0 [−0,4 ; +0,6]). `best1`,
combinaison naïve, ne tient pas sur B en route (+0,3 [−0,3 ; +0,9]) : les cases interagissent. Aucune table ne corrige l'écart
`tertiary` / petite route en route : baisser `tertiary` ou la petite route coûte (−1,5 à −6 points).

**(d) `avoid_main_roads` et les crans du curseur déplacent-ils les kilomètres dans le sens attendu ?** Oui, les deux. `avoid` ôte 80 %
des kilomètres de `secondary` en route (23 → 4,8 %), 70 % en gravel (4,0 → 1,2 % [calculé]) et 36 à 47 % de ceux de la grande route,
pour un détour P90 de 1,05 à 1,15. Le curseur est monotone sur chaque case, avec un cran vers les routes qui double `secondary` et un
cran vers les pistes qui triple les voies cyclables (§3.2). Le temps à chaud d'une variante non neutre est de 22 à 23 ms en P50 contre
13 ms au neutre (P90 34 à 39 contre 28) [mesuré, 16 fils] ; la cause (le multiplicateur par chaîne calculé à la première lecture, ou la
chauffe neutre qui sert la variante neutre) n'est pas isolée [hypothèse].

**(e) Limites.**

- **Le recouvrement suppose qu'on va de A à B.** Une personne qui roule de A à B dans une sortie ne relie pas A à B : elle passe par
  un lieu, suit un parcours connu, cherche une côte ou un paysage. La règle `L_ref ≤ 3 ×` vol d'oiseau écarte les allers-retours, pas les
  détours voulus ; la médiane de `L_ref` / vol d'oiseau est de 1,33 en route et 1,58 en gravel [calculé], contre un détour de
  l'itinéraire de 1,0 à 1,05. Un Jaccard bas sur un long tronçon ne veut pas dire que l'itinéraire est mauvais.
- **Le Jaccard et les parts par case ne disent pas la même chose** (§2.4). Une table calée sur l'un éloigne l'autre ; aucune ne mesure ce
  qu'une personne trouverait « bon ».
- **Biais de la population** : deux équipes font la quasi-totalité des traces (3 172 sur 3 355 au relevé de la retouche), de surcroît
  dans les mêmes régions ; les tronçons sont donc géographiquement groupés. Le bootstrap par trace corrige la dépendance entre tronçons
  d'une trace, pas celle entre traces d'une même équipe ou d'un même secteur. Le VTT (12 traces) n'est pas mesuré en pratique ; la
  bande longue du gravel est petite (212 tronçons).
- **La référence n'est pas le terrain** : c'est la marche appariée (trous de 0,6 à 1,1 % des kilomètres, 2,7 % hors de la vue du type),
  et ses cases viennent des attributs OSM de France du 15 novembre 2026.
- **Non mesuré** : l'altitude (le moteur ne l'a pas) ; le goût d'une équipe (requêtes sans équipe) ; une descente séquentielle et
  plusieurs passes ; des multiplicateurs hors de [× 0,5 ; × 3] ; des pénalités `avoid_main_roads` autres que celles du moteur, sauf par
  le balayage de la case 6 seule ; l'aménagement cyclable (`infra`), hors de la table ; la comparaison avec Stadia (§6 de la note du
  moteur).

## 5. Recommandation (non appliquée)

- **Garder `WAY_NEUTRAL` comme neutre en gravel et en VTT** : aucune table ne gagne sur B (gravel +0,0 [−0,4 ; +0,6]) ; le VTT n'est
  pas mesurable.
- **Route : `best2` est une candidate, pas une évidence.** Si un neutre de route devait changer, ce serait

  | case | 0 grande route | 1 `tertiary` | 2 petite route | 3 rue | 4 cyclable ou verte | 5 chemin | 6 `secondary` |
  |---|---|---|---|---|---|---|---|
  | `way_q8` | 256 | 256 | 256 | **361** (× 1,41) | **361** (× 1,41) | **512** (× 2) | 256 |

  pour +0,8 [+0,2 ; +1,3] point de Jaccard sur B (+1,3 sur A), au prix de parts de rues, de voies cyclables et de chemins plus éloignées
  du réel. Le gain est net au sens statistique et petit en valeur ; il ne justifie pas à lui seul de changer ce que voit une personne
  qui demande un itinéraire, d'autant qu'il retire des voies cyclables. **Je ne la recommande pas sans un critère de plus** (par
  exemple une appréciation de quelques itinéraires par le propriétaire). La variante la plus sûre serait chemin × 2 seul (+0,3
  [+0,1 ; +0,5] sur A, +0,3 [+0,2 ; +0,5] sur B), qui ne touche pas aux voies cyclables.
- **`avoid_main_roads`** : le × 2 de `secondary` est **assez fort, pas trop petit** : il ôte 80 % des kilomètres de `secondary` ; × 1,41
  en ôterait 63 % pour un détour P90 de 1,09 au lieu de 1,14. Si l'option paraît trop brutale en usage, × 1,41 garderait l'essentiel de
  l'effet ; rien dans le banc ne demande de l'augmenter. Le × 1,5 de la grande route ne change presque rien (les itinéraires neutres y
  roulent 1,5 %) : sans enjeu, à garder.
- **Ce que la table ne corrigera pas** : l'excès de `tertiary` sur les petites routes en route, et l'excès de voies cyclables sur les
  chemins en gravel. Tous deux viennent du goût commun (qui met déjà ces voies à des prix voisins) et du fait que le réel n'est pas un
  plus court chemin. Un gain au-delà de quelques points demanderait autre chose qu'un multiplicateur par case [hypothèse].

## 6. Le banc

Sous-commande `route-play` d'`ord-loops-bench` (`tools/ord-loops-bench/src/route_bench/`) : `draw.rs` (tirage des tronçons, moitiés),
`overlap.rs` (morceaux d'arête, réunions, recouvrement, kilomètres par case, fractions des pas d'un itinéraire), `agg.rs` (agrégats,
bootstrap par grappe), `md.rs` (le Markdown), `mod.rs` (les phases). Tests unitaires des fonctions pures (tirage, recouvrement,
agrégats, choix de `best1` et `best2`). Options : `--threads`, `--teams`, `--limit`, `--seed`, `--label`, `--max-minutes`,
`--per-track` (tronçons par trace, 3), `--no-tune` (phase 1 seule). Sorties : `<label>-lines.jsonl` (une ligne par tronçon et
variante), `<label>-segments.jsonl` (les références), `<label>-route.json` et `<label>-route.md`. La case d'une arête reprend
`way_index` du moteur (privé), recopié dans le banc.
