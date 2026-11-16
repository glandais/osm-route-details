# Générateur de parcours — lot 6 : relevés (France entière dans 2 Go)

> Relevés de la **seconde relecture**, 16 novembre 2026, binaires `release` du commit `8a6f8d7` de la branche
> `feat/ord-loops-window` (corrections de la première relecture comprises) et du tag `v1.9.0` (moteur à plat, code
> identique à `develop`), Rust 1.100.0, poste de développement (AMD Ryzen 7 4800H, 16 fils, 30 Go, NVMe). Graphe et
> vues France : extrait Geofabrik du 15/11 ; région : Pays de la Loire, extrait du 14/11. Conception :
> [`ord-loops-lot6.md`](ord-loops-lot6.md). Mo et Go décimaux. Équipes désignées par les lettres A à I ; aucune
> coordonnée de départ, aucun identifiant d'équipe ni secret. Les seuls lieux nommés sont des centres-villes publics,
> sans lien avec les départs des équipes.
>
> Tout ce qui suit a été **remesuré sur `8a6f8d7`**, sauf la §6 (relevés antérieurs, non refaits). Les deux binaires
> du banc ont reçu, hors dépôt, la même instrumentation d'une vingtaine de lignes : par requête, l'empreinte FNV-1a du
> `Debug` complet du résultat et des diagnostics, **et la liste des arêtes de base orientées** de chaque parcours.

## 1. Équivalence

| Quoi | Comment | Résultat |
|---|---|---|
| Moteur fenêtré contre moteur à plat d'avant le lot, région | `ord-loops-bench --modes strict,team,discovery --rounds 0` : binaire `v1.9.0` puis binaire du lot (moteur fenêtré, cache sous 900 Mo), **toutes** les requêtes du banc, 323 × 3 modes = **969** ; fichiers d'empreintes triés et comparés octet à octet | **969 / 969 identiques**, empreinte du `Debug` complet **et** liste des arêtes de base ; 696 requêtes rendent des parcours (strict 91, équipe d'abord 300, découverte 305), 273 échouent de la même façon des deux côtés |
| Moteur à plat du même binaire (`--engine flat`), région | mêmes 969 requêtes | identique octet à octet aux deux précédents |
| Fenêtré contre fenêtre de toutes les tuiles, même processus, région | `--compare` sur les mêmes 969 requêtes | 0 différence |
| Index globaux contre la force brute, région | `--self-check` | chaînes étrangères 9 051 + 13 293, chaînes sous le plafond 9 518 + 15 782, degré admis de 820 044 + 977 849 nœuds : identiques (1,5 s) |
| Référence figée, graphe synthétique | `cargo test --workspace --locked` ; la fixture `window_reference.txt` (140 requêtes, z11 et z16) n'a pas changé depuis le commit de l'étape 0, écrit avant le moteur fenêtré | 739 tests, 0 échec |
| **Service, France, par HTTP** : binaire du lot sous 2 Go contre binaire `v1.9.0` sans limite | mêmes neuf dépôts réels, puis 69 générations (une première génération par équipe, puis 60 requêtes tirées sur les réseaux déposés : 30 à 200 km, route, gravel et VTT, curseurs 0, 0,25 et 1) ; empreinte du tableau `routes` de chaque réponse JSON | **69 / 69 identiques** (codes, nombre de parcours, contenu) ; le parcours que le contrôleur indépendant refuse en `SHARP_TURN` dans cette série l'est **aussi** par `v1.9.0` : c'est un écart moteur / contrôleur antérieur au lot |
| France contre région, mêmes requêtes | banc fenêtré France (surcouches France) contre le banc fenêtré région, 969 requêtes communes | requêtes **intérieures** à la région : **591 / 591 même issue** ; sur les 435 qui rendent des parcours des deux côtés, 386 ont des géométries identiques, 49 des parcours de mêmes longueurs mais d'autres tracés. Requêtes **de bord** : 370 / 378 même issue, 8 échecs « aucun candidat » de la région réussissent sur la France |

Les écarts de géométrie France / région ne mettent pas le moteur en cause : les deux graphes sont des extraits de dates
différentes et **numérotent autrement** nœuds et arêtes ; or le moteur départage les coûts égaux par le plus petit
identifiant. L'équivalence exacte se prouve sur un même graphe : lignes 1 à 3 (région) et ligne 6 (France, par HTTP).

Le moteur à plat de `v1.9.0` sur la France, pour la ligne 6, a tenu **15,0 Go** de groupe de contrôle au plus
(12,5 Go d'anonyme après le premier dépôt, 14,2 Go en série) ; le moteur du lot, 2 Go par construction (§2.2).

## 2. Mémoire

### 2.1 Banc France sous 2 Go, cache de pages froid

`posix_fadvise(DONTNEED)` des trois fichiers avant le lancement (sinon les pages déjà en cache ne sont pas comptées au
groupe de contrôle), puis `systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 ord-loops-bench …
--modes strict,team,discovery --rounds 1 --budget-mb 900`, neuf équipes, surcouches France, **1 644 requêtes** :

- code 0, sans mise à mort ; `VmHWM` 2 032 Mo (le groupe se remplit de pages de fichier et les rend sous pression) ;
- `anon` du groupe : **1 115 Mo au plus**, 808 Mo à la fin ; le pic tombe pendant la construction des données de
  l'équipe D (1 101 Mo échantillonnés par le banc). Au premier passage, sans réservation de cette construction dans le
  budget, le pic était de 1 459 Mo : la correction tient ;
- 57 fenêtres construites, 1 587 servies du cache, 26 évincées, **0 échappement** ; mémoire comptée au plus 888 Mo
  (constructions d'équipe comprises) ; fenêtres de 244 648 chaînes en médiane, 590 262 au plus.

Banc région, pour mémoire : `VmHWM` 1 006 Mo fenêtré, 597 Mo à plat (`--engine flat`, une fenêtre de toutes les tuiles
de la région).

### 2.2 Le service sur la France (`osm-route-details`, configuration par `ORD_GENERATOR__*`)

Lancé sur `127.0.0.1`, sans altitude (`ORD_CACHE__OFFLINE=true`, IGN désactivé), sous
`systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0`, avec l'environnement de l'image
(`MALLOC_ARENA_MAX=2 MALLOC_MMAP_THRESHOLD_=1048576`), budget de 900 Mo, cache de pages des trois fichiers évincé au
départ, **polygone `france.poly` de Geofabrik** (accepté par la nouvelle règle d'étendue, aucun avertissement au
démarrage). Clé d'espace de noms tirée au hasard par le script, jamais écrite. Déroulé : **neuf dépôts réels**
(703, 67, 38, 30, 6, 1, 1, 0,4 et 0,4 Mo de NDJSON), attente des constructions de fond, une première génération par
équipe (curseur 0,25), puis 70 générations en série dans dix villes (Paris, Lyon, Marseille, Toulouse, Bordeaux,
Lille, Strasbourg, Brest, Grenoble, Nantes : boucles de 50, 100 et 200 km en route et en gravel, aller simple de
200 km, en découverte), deux fois ; 60 générations en série depuis les réseaux déposés ; 214 générations (réseaux et
villes) à 4 clients puis à 12 clients. Puis **redémarrage** sur les mêmes surcouches, pages froides : une première
génération par équipe et la même série de 60. `anon` du groupe échantillonné toutes les 0,25 à 0,5 s.

| | Mesure |
|---|---|
| écoute / `load_ms` (pages froides) | 8,5 s / 8 172 ms (8 102 ms au redémarrage) |
| `anon` après les neuf dépôts | **407 Mo** (210 Mo au premier passage, cinq dépôts sans construction de fond) |
| `anon` au plus pendant les dépôts (constructions de fond comprises) | 1 061 Mo |
| `anon` au plus, sur la séance | **1 182 Mo** |
| pages de fichier au plus bas | 973 Mo |
| `oom` / `oom_kill` | 0 / 0 |
| constructions de fond | 9 (une par dépôt), finies 7 s après le dernier dépôt ; les neuf premières générations sont servies du cache (0,26 à 0,87 s) |
| villes, 2 × 70 en série (méd. / p90 / max) | 140 × 200 ; 0,99 / 2,55 / 5,23 s puis 1,00 / 2,48 / 5,34 s |
| réseaux, 60 en série | 41 × 200, 19 × 422, **0 × 504** (max 3,60 s) |
| 214 à 4 clients | 169 × 200, 45 × 422 ; 1,35 / 3,30 / 6,96 s |
| 214 à 12 clients | 138 × 200, 37 × 422, 35 × 503, **4 × 504** ; 3,56 / 5,96 / 8,07 s |
| après redémarrage, première génération par équipe | 0,45 à **5,40 s**, toutes 200 (une 422 hors couverture) |
| après redémarrage, 60 en série | mêmes codes qu'avant le redémarrage, 0,40 / 1,96 / 3,70 s, `anon` au plus 927 Mo |

Les 422 sont les échecs ordinaires de ces tirages (départ hors couverture, trop loin du réseau strict, réseau trop
petit, aucun candidat). Les 503 `overloaded` à 12 clients sont la file pleine (2 calculs, 8 en attente). Les quatre
504 à 12 clients sont des générations restées trop longtemps en file : à ce débit, le budget de 8 s couvre l'attente.

Métriques à la fin : `ord_generator_memory_bytes` fenêtres 341 Mo, agrégats d'équipe 93 Mo, espaces de travail en
réserve 94 Mo ; 371 fenêtres construites (172 route, 199 gravel) pour 223 servies du cache, 363 évincées ; 0
échappement ; 5 parcours refusés `SHARP_TURN` par le contrôleur indépendant, en découverte à 100 km et plus (voir
§1 : le même refus se reproduit avec `v1.9.0`).

**Avec l'altitude** (deux caches de 256 Mo dans le même conteneur, non mesurée ici) : 1 182 + 512 = 1,69 Go d'anonyme
au pire relevé, 0,3 Go pour les pages de fichier. Ça tient, avec peu de marge (voir §4).

## 3. Temps

### 3.1 Génération à chaud, région : moteur fenêtré contre moteur à plat

`ord-loops-bench --modes strict,team,discovery --rounds 3 --limit 25` (129 requêtes × 3 modes), un fil, l'un après
l'autre sur la machine peu chargée : binaire du lot, binaire `v1.9.0`, binaire du lot une seconde fois. Médiane des
trois tours chauds par requête, puis **médiane / p90 / maximum en ms** :

| Mode | à plat (`v1.9.0`) | fenêtré, 1er passage | fenêtré, 2e passage | écart de médiane |
|---|---|---|---|---|
| strict | 2 / 71 / 199 | 2 / 72 / 209 | 2 / 72 / 209 | égale |
| équipe d'abord | 250 / 775 / 1 889 | 258 / 797 / 1 941 | 256 / 791 / 1 963 | **+ 2 à + 3 %** |
| découverte | 175 / 569 / 1 010 | 179 / 564 / 1 015 | 178 / 562 / 1 008 | **+ 2 %** |
| tout | 111 / 624 / 1 889 | 116 / 631 / 1 941 | 115 / 631 / 1 963 | + 4 % |

RSS maximal : 640 à 650 Mo fenêtré, 724 Mo à plat. Durée du banc : 5 min 57 s à 5 min 59 s fenêtré, 5 min 41 s à plat.

### 3.2 France

| | Mesure |
|---|---|
| Ouverture du moteur, banc, pages froides | 5,8 s (index : route 2,9 + 0,65 s, gravel 4,7 + 1,1 s, une vue par fil) ; 8,5 Mo tenus |
| Démarrage du service, pages froides | écoute à 8,5 s, `load_ms` 8,1 à 8,2 s |
| Génération, banc France (1 tour chaud, 969 requêtes communes avec la région) | strict 3 / 94 / 277 ms, équipe d'abord 261 / 914 / 2 075 ms, découverte 172 / 496 / 1 204 ms (médiane / p90 / max) ; tour froid région au même banc : 4 / 101 / 268, 254 / 819 / 1 934, 160 / 491 / 1 138 |
| Fenêtre à froid, banc France | 95 / 180 / 213 ms (médiane / p90 / max, 57 constructions) |
| Fenêtre à froid, service France (2 Go) | gravel 0,32 / 0,54 / 0,75 s, route 0,14 / 0,32 / 0,42 s (médiane / p90 / max, 199 et 172 constructions ; fenêtres jusqu'à 929 417 chaînes en route, 1 394 066 en gravel) |
| Construction des données d'équipe, banc France (en série, pages froides) | `TeamData` puis `TeamCheck` : A 2,4 + 1,0 s, C 3,3 + 1,5 s, D 5,6 + 2,6 s, les autres sous 0,5 s |
| Dépôts, service France | 703 Mo en 73 s, 67 Mo en 6 s, 38 Mo en 89 s, 30 Mo en 60 s, 6 Mo en 9 s, les quatre petits en 1 à 2 s |

Critère « fenêtre à froid ≤ 2 s pour 100 km » : tenu (0,75 s au plus, toutes distances jusqu'à 200 km confondues).

### 3.3 La première génération d'une équipe

- **Après un dépôt** : les données sont construites en tâche de fond ; les neuf premières générations relevées les
  trouvent au cache (0,26 à 0,87 s). Plus de 504 en série (un par séance au premier passage).
- **Après un redémarrage**, pages froides : la première demande construit encore (`TeamData` et `TeamCheck` en
  parallèle) ; relevé 0,45 à 5,40 s, sous le budget de 8 s. Une équipe plus grosse que D, ou un disque plus lent,
  peut encore le dépasser.

## 4. Constats de la seconde relecture

Aucun ne remet en cause l'équivalence ni la tenue dans 2 Go ; ils sont à traiter ou à trancher avant d'ouvrir plus
d'équipes sur la France.

- **Données d'équipe hors budget, désormais toujours résidentes** (`crates/ord-server/src/generator/store.rs`,
  `ready_build` ; `deposit.rs`, `warm_team`) : l'entrée d'une équipe (surcouche, `TeamData`, `TeamCheck`) n'est pas
  comptée dans `memory_budget_mb`. Avant, seules les équipes qui généraient la tenaient ; avec la construction de fond,
  **chaque dépôt** la monte, jusqu'à `team_cache_entries` (16). Relevé : `anon` après dépôts de 210 Mo (cinq équipes,
  sans construction) à 407 Mo (neuf équipes, construites), pic de séance 1 182 Mo contre 1 101 Mo. Avec 16 équipes de
  la taille de D, la marge du §2.2 disparaît. Correction attendue : compter les entrées dans le budget (ou les borner
  en octets) et le relever avec 16 équipes.
- **`TeamCheck` construit en parallèle sans réservation** (`store.rs`, `ready_build`) : seule l'estimation de
  `TeamData` (210 o par chaîne) est réservée ; la mémoire transitoire de `TeamCheck`, autrefois construite après la
  libération de celle de `TeamData`, s'y ajoute désormais. Relevé : 1 061 Mo d'anonyme au plus pendant les dépôts et
  leurs constructions de fond, sous le pic de séance. À ajouter à l'estimation réservée.
- **Le banc construit encore en série** (`tools/ord-loops-bench/src/main.rs`) alors que le service construit
  `TeamData` et `TeamCheck` en parallèle : le pic du banc pendant la construction de D (1 101 Mo) ne représente pas
  celui du service.
- **Découverte sans équipe sous pression mémoire** (`generate.rs`, `compute`) : quand la réservation de la
  construction d'équipe échoue, le mode découverte génère sans l'équipe ; la réponse dit alors `overlay: null` et ne
  compte plus la part d'équipe des parcours, alors qu'une surcouche existe : pour une même requête, la réponse dépend
  de la charge. Les parcours eux-mêmes ne changent pas (la découverte n'utilise pas l'équipe dans la recherche ni
  dans le score). À documenter dans le contrat ou à remplacer par un 503.
- **Arrêt** : la construction de fond attend jusqu'à 120 s une place dans le budget sur un fil bloquant ; un arrêt du
  service pendant cette attente l'attend aussi.

## 5. Ce qui n'a pas été mesuré

- Le service **avec l'altitude** réelle dans le même conteneur ; **Docker** (relevés sous `systemd-run` seulement,
  image non construite : les variables `MALLOC_*` du `Dockerfile` ne sont éprouvées que posées à la main) ; le service
  **sans** `MALLOC_MMAP_THRESHOLD_` n'a pas été rejoué (premier passage : 1 743 Mo d'anonyme).
- Un **disque lent** (réseau) : fichiers sur NVMe ; une fenêtre froide et la première génération après redémarrage
  coûtent surtout de la lecture de pages.
- **16 équipes** en cache, ou une équipe plus grosse que D.
- Le chemin « échappement, puis fenêtre élargie » du service : aucun échappement aux relevés (0 sur 969 + 1 644
  requêtes au banc et 1 200 générations au service).
- Le moteur à plat sur la France au banc (`--compare`) : remplacé par la comparaison HTTP au binaire `v1.9.0`
  (§1, 69 requêtes).
- Les distances au-delà de 200 km par HTTP (le contrat s'arrête à 200 km).

## 6. Relevés antérieurs, non refaits

Repris des passages précédents, sans les avoir rejoués :

- sonde `window_probe` (fenêtres de classe autour de Paris, Lyon, Nantes et Clermont-Ferrand) : boucle de 200 km en
  gravel à Paris, 1 382 579 chaînes, 96 Mo de fenêtre et 188 Mo d'espace de travail ; pire aller simple de 300 km
  (Lyon, gravel) 2 234 108 chaînes, 158 Mo et 316 Mo, construit en 0,68 s ; espace de travail mesuré à 142 à 146 o par
  chaîne ;
- pic de mémoire transitoire de la construction des données d'équipe, une à une : D 704 Mo, C 434 Mo, A 347 Mo
  (estimation réservée à 210 o par chaîne : 749, 468, 321 Mo) ;
- service sans `MALLOC_MMAP_THRESHOLD_` : 1 743 Mo d'anonyme au plus (contre 1 101 Mo avec), et sous 1,5 Go avec le
  réglage : 1 106 Mo, sans mise à mort.

## 7. Écarts à la note et aux comptes rendus

- La note (§10.1) annonce un budget unique qui borne ce que le générateur alloue ; mesuré, il ne le borne qu'avec
  `MALLOC_MMAP_THRESHOLD_` (désormais posé par l'image), et il laisse dehors les entrées d'équipe et la mémoire
  transitoire de `TeamCheck` (§4).
- Le compte rendu des corrections annonçait la première génération après redémarrage « encore dans son budget de
  8 s » sans mesure : relevé 5,40 s au plus sur ces neuf équipes, pages froides.
- Le polygone `france.poly` de Geofabrik, qui éteignait le générateur au premier passage, est accepté par la nouvelle
  règle (débord toléré jusqu'à 20 % de la boîte des nœuds) : vérifié au démarrage du service. La règle reste à
  confirmer par le responsable.
