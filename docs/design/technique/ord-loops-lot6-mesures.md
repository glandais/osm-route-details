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

## 8. Finition (16 novembre 2026)

Traitement des cinq constats du §4 ; conception : [`ord-loops-lot6.md`](ord-loops-lot6.md) §18. Banc `release`, mêmes
fichiers France et mêmes neuf surcouches que ci-dessus, `--modes discovery --rounds 0 --limit 1 --budget-mb 1000`, une
équipe par processus pour les pics (le pic anonyme échantillonné ne redescend pas dans un même processus : les équipes
suivantes y gardent la mémoire que l'allocateur n'a pas rendue).

| Équipe | construction (données et contrôleur en parallèle) | pic anonyme au-dessus de l'ouverture | estimation réservée (données + contrôleur) | tables du contrôleur seules, pic mesuré / estimé |
|---|---|---|---|---|
| A | 1,8 s | 359 Mo | 403 Mo (320 + 82) | 63 / 82 Mo |
| C | 1,9 s | 490 Mo | 587 Mo (467 + 120) | 99 / 120 Mo |
| D | 3,2 s | 748 Mo | 924 Mo (749 + 175) | 139 / 175 Mo |

- **L'estimation de `TeamCheck`** : un premier réglage à 32 o par arête des cellules et 48 o par arête de surcouche
  rendait 2,5 fois le pic mesuré (D : 1 099 Mo réservés, au-dessus d'un budget de 900) ; ramené à 16 et 24 o, il couvre
  les trois équipes mesurées avec 20 à 30 % de marge. Le pic de la construction **parallèle** (D : 748 Mo) est proche du
  pic de `TeamData` seul (704 Mo, §6) : les deux pics ne coïncident pas, la somme des estimations (924 Mo) reste
  prudente.
- **Octets résidents** (surcouche déchiffrée + `TeamData` + `TeamCheck`), en Mo : A 15,8 (3,1 + 12,6 + 0,1), B 2,7,
  C 5,4, D 8,2, E 1,9, F à I 0,1 à 0,2 chacune ; **35 Mo pour les neuf**. Les 200 Mo d'anonyme de plus relevés au §4 entre
  cinq équipes sans construction (210 Mo) et neuf équipes construites (407 Mo) ne sont donc **pas** les entrées d'équipe :
  c'est de la mémoire de construction que l'allocateur garde (tables de hachage et petits blocs sous le seuil de 1 Mio, que
  `MALLOC_MMAP_THRESHOLD_` laisse dans les arènes). Compter les équipes dans le budget et les évincer ne rend que 35 Mo ;
  la rétention de l'allocateur reste hors du budget (à surveiller par `RssAnon`, pas par le budget).
- **Pas de relevé** : le service par HTTP avec plus de neuf équipes (13 aujourd'hui avec staging et prod), l'éviction
  d'équipes sous charge réelle, l'altitude dans le même conteneur. L'éviction est couverte par des tests unitaires
  (`windows.rs`, `store.rs`), pas par un relevé.
- **Constats du §4**, tous traités : entrées d'équipe comptées et évincées (budget unique, LRU) ; mémoire transitoire de
  `TeamCheck` réservée ; banc en parallèle ; découverte sans mémoire d'équipe : 503 `overloaded` au lieu d'une réponse sans
  `overlay` ; attente de la construction de fond interrompue à l'arrêt.
- **Budget par défaut** : 900 → 1 000 Mo (voir la note §18 : la réservation de D, 924 Mo, ne tient pas sous 900).

## 9. Relecture de la finition : service en conteneur Docker (16 novembre 2026, soir)

Commit `e66503d` (paquet 1.9.1). Image construite depuis le `Dockerfile` du dépôt (`osm-route-details:lot6`, 40,5 Mo),
lancée par `docker run --memory=2g --memory-swap=2g`, ports liés à `127.0.0.1` seulement, les quatre fichiers France
(`.ordg`, deux `.ordv`, `france.poly`) montés en lecture seule, un volume nommé temporaire sur
`/var/cache/osm-route-details` (surcouches comprises), **sans altitude** (`ORD_CACHE__OFFLINE=true`, IGN désactivé :
aucune donnée d'altitude locale), configuration par défaut du générateur (budget 1 000 Mo, `team_cache_entries` 16).
Cache de pages des fichiers évincé avant chaque démarrage. Deux clés d'espace de noms tirées au hasard par le script
(jamais écrites) pour simuler staging et prod : les neuf dépôts réels sous chacune, **18 équipes**. Mémoire : `anon`,
`file`, `memory.current` et `memory.peak` du groupe de contrôle du conteneur toutes les 0,5 s, `docker stats` toutes
les 10 s. 2 Gio = 2 147 Mo.

### 9.1 Vérifications

| Quoi | Résultat |
|---|---|
| `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` | verts |
| `cargo test --workspace --locked` | 744 tests, 0 échec (fixture figée et test d'équivalence inchangés) |
| Banc région `--compare --self-check --budget-mb 1000`, `--rounds 0`, toutes les requêtes | **969 / 969, 0 différence** avec la fenêtre de toutes les tuiles ; auto-contrôle identique à la force brute ; `VmHWM` 1 010 Mo |
| Variables `MALLOC_*` dans le conteneur | lues dans `/proc/1/environ` du service (conteneur annexe partageant son espace de processus, même utilisateur) : `MALLOC_ARENA_MAX=2`, `MALLOC_MMAP_THRESHOLD_=1048576` |

### 9.2 Séance complète, 2 Gio

| | Mesure |
|---|---|
| écoute / générateur prêt / `load_ms` | 8,7 s / 8,8 s / 8 062 ms (redémarrage : 8,4 s) |
| 18 dépôts | tous 200 ; 703 Mo en 59 s, 38 Mo en 71 s, 30 Mo en 48 s ; `anon` après dépôts 475 Mo (451 après les neuf premiers) |
| équipes résidentes après dépôts | 51 Mo comptés (`team_cache`) ; 2 évictions au plafond en nombre (18 > 16) |
| premières générations (18) | 16 × 200, 2 × 422 hors couverture ; médiane 0,88 s, max 5,58 s ; **16 reconstructions** sur 18 (13 évictions au budget, 4 au nombre pendant ces 18 requêtes) |
| réseaux, 54 en série | 37 × 200, 17 × 422 ; 0,47 / 2,64 / 4,65 s (médiane / p90 / max) |
| villes en découverte (avec identifiants d'équipe), 60 en série | 58 × 200, **2 × 503 `overloaded` à 8,0 s avec un seul client** ; 1,69 / 4,23 / 8,00 s |
| réseaux + villes, 114 à 4 clients, deux passages | 83 et 84 × 200, 16 et 15 × 422, **10 et 10 × 503, 5 et 5 × 504** ; 3,27 / 8,00 / 10,06 s puis 3,68 / 8,00 / 9,71 s |
| reconstructions de données d'équipe sur la séance | 262 défauts de cache pour 111 succès ; **208 évictions au budget**, 25 au nombre ; pendant les deux passages à 4 clients : 89 reconstructions pour 228 requêtes |
| `anon` au plus | **1 309 Mo** (séance), 905 Mo (redémarrage) ; pages de fichier au plus bas 817 Mo pendant le pic |
| `memory.current` / `memory.peak` | au plafond (2 147 Mo) presque toute la séance : le reste est du cache de pages, rendu sous pression (`memory.events` `max` 140 158) |
| `docker stats` (sans le cache de pages inactif) | 987 Mio au plus |
| OOM | `oom` 0, `oom_kill` 0, `OOMKilled=false` |
| après redémarrage du conteneur (pages froides) | 16 × 200, 2 × 422 ; premières générations 0,44 à **5,79 s** (médiane 0,87 s) ; 40 en série : 0,54 / 2,62 / 4,13 s, 0 × 503 |

### 9.3 Même conteneur réduit à 1,5 Gio (2 Gio moins les 512 Mo de l'altitude)

`docker update --memory 1536m` sur le même volume (18 surcouches), redémarrage pages froides, premières générations,
40 en série, 114 à 4 clients (sans la série de villes ni les dépôts) : 0 OOM, `memory.peak` au plafond (1 611 Mo),
`anon` au plus 1 055 Mo, pages de fichier au plus bas 603 Mo ; premières générations 0,45 à 5,89 s ; 4 clients :
97 × 200, 17 × 422, **0 × 503, 0 × 504**, 1,89 / 3,59 / 7,21 s ; 1 éviction au budget, 14 au nombre.

**Marge pour l'altitude** (calculée, l'altitude n'a pas été sollicitée faute de données locales) : 2 147 − 1 309 =
838 Mo au pire relevé, soit 326 Mo de pages de fichier une fois les 512 Mo des deux caches pris. Le passage à 1,5 Gio
tient, mais il n'a pas rejoué les dépôts ni la série de villes, qui font le pic de 1 309 Mo.

### 9.4 Constats

- **Bloquant : la plus grosse équipe ne se construit plus dès que la réserve d'espaces de travail dépasse 76 Mo**
  (`windows.rs` `reserve_build` / `reserve_in`, `generate.rs` `give_workspace`). Sa construction réserve 924 Mo sur
  1 000 ; les espaces de travail gardés en réserve (jusqu'à `max_concurrent` × `workspace_keep_mb` = 320 Mo) sont
  comptés par `force_reserve`/`reserve` et ne sont rendus que quand une génération en reprend un : ni l'éviction des
  fenêtres ni celle des équipes ne les libèrent. Reproduit avec **un seul client**, de façon déterministe : dépôt de
  D seul, redémarrage, neuf découvertes sans équipe (réserve 78,6 Mo), puis deux générations de D (200 km gravel,
  puis 30 km route) : **503 `overloaded` à 8,0 s les deux fois**, et toutes les suivantes tant que la réserve reste
  au-dessus de 76 Mo. Ce sont les 2 × 503 de la série de villes (§9.2 : les deux tombent sur D). Avant ce commit, la
  découverte rendait une réponse sans surcouche ; désormais c'est un 503, en mode équipe comme en découverte.
- **Éviction en cascade des équipes résidentes** (`windows.rs` `reserve_in`, `store.rs` `evict_idle`) : une
  réservation **transitoire** de construction (403 à 924 Mo) évince toutes les équipes que personne ne tient, qui ne
  pèsent que 35 à 58 Mo ensemble ; chacune doit ensuite se reconstruire (1 à 6 s, et sa propre réservation de 400 à
  900 Mo évince les autres). Relevé : 208 évictions au budget sur la séance, 89 reconstructions pour 228 requêtes
  à 4 clients, d'où 10 × 503 et 5 × 504 par passage et un p90 collé à l'échéance de 8 s. Compter les équipes ne
  rendait que leurs 35 à 58 Mo ; les évincer pour une réservation transitoire coûte bien plus qu'il ne rend.
- **Plafond en nombre trop bas pour deux environnements** : 18 équipes (2 × 9) pour `team_cache_entries` = 16 ; en
  accès tournant, chaque requête d'une équipe évincée la reconstruit (25 évictions au nombre sur la séance, 14 sur
  114 requêtes à 1,5 Gio). 13 équipes aujourd'hui en production, mais le plafond doit suivre le nombre d'équipes des
  deux environnements, d'autant qu'une équipe ne pèse que quelques Mo.
- Mémoire : tenue sous 2 Gio sans OOM, `anon` au plus 1 309 Mo (contre 1 182 Mo au §2.2 avec 9 équipes et un budget
  de 900 Mo) ; la marge calculée pour l'altitude (326 Mo de pages de fichier) n'a pas été éprouvée sous la charge
  qui fait ce pic.

### 9.5 Non mesuré

L'altitude réelle dans le conteneur (pas de données locales) ; le service à 1,5 Gio pendant les dépôts et la série de
villes ; un disque lent ; un relevé du binaire `8a6f8d7` dans le même conteneur avec 18 équipes (les chiffres du
§2.2 sont à 9 équipes et sous `systemd-run`, pas directement comparables) ; l'arrêt pendant une attente de
`warm_team` (relu, non provoqué).

## 10. Finition 2 : construction d'équipe creuse, réserve évincée d'abord, équipes hors des réservations (17 novembre 2026)

Commit `c93b17c` (étape 5 ; paquet 1.9.1, contrat inchangé). Binaires `release` du banc (une équipe par processus,
pages chaudes, `MALLOC_ARENA_MAX=2`) et image `osm-route-details:lot6` **reconstruite** depuis le `Dockerfile` du dépôt
(40,5 Mo ; la fonctionnalité `reference` n'y est pas : l'image se construit avec `-p ord-server`). Même poste, mêmes
fichiers et mêmes neuf surcouches que le §9. Équipes lettrées A à I ; aucune coordonnée, aucun identifiant, aucune clé.

**La séance du §9.2 est reconstruite, non rejouée** : son script n'avait pas été gardé. Mêmes formes et mêmes nombres
(18 dépôts réels sous deux clés tirées au hasard et jamais écrites, 18 premières générations, 54 réseaux en série, 60
villes en série, 114 requêtes à 4 clients deux fois), mais **d'autres requêtes** : les chiffres de latence et de
refus fonctionnels (422) ne se comparent pas ligne à ligne avec le §9.2. Ce qui se compare : les 503, les 504, les
reconstructions, les évictions, la mémoire. Même configuration (`ORD_CACHE__OFFLINE=true`, IGN désactivé, aucune
altitude), `docker run --memory=… --memory-swap=…` identiques, volume neuf. Les 422 sont des refus fonctionnels du
moteur (`no_candidate` 33, `start_too_far` 27, `network_too_small` 9, `out_of_coverage` 3 sur 2 × 114 + 114 requêtes),
aucun n'est budgétaire.

### 10.1 Vérifications

| Quoi | Résultat |
|---|---|
| `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` | verts |
| `cargo test --workspace --locked` | **759 tests, 0 échec** (744 + 15 : fixtures de 200 surcouches, égalités creuse = référence = à plat, garde-fou forcé, constat A, plafonds du magasin, ticket de l'entrée) ; `window_reference.txt` inchangée ; contrat OpenAPI inchangé (test `contract`) |
| Équipes de la France (neuf), `--dump-teams`, avant / après | **18 lignes identiques** (données et contrôleur, empreinte xxh3 et octets) |
| Équipes de la région (neuf), idem | **18 lignes identiques** |
| `--dump-teams --teams-reference` sur la France, dans le même processus | neuf équipes : données et contrôleur **égaux** à la construction d'avant, octet pour octet |
| Banc région `--compare --self-check --budget-mb 1000 --rounds 0` | **969 / 969, 0 différence** ; auto-contrôle identique à la force brute ; violations du contrôleur : aucune ; parcours refusés alors que le moteur les acceptait : 0 |
| Rapports du banc région, contre ceux de `e66503d` | identiques champ à champ (hors durées) ; seule différence : le dernier bit de quelques sommes de flottants du contrôleur (l'ordre d'itération d'un `HashSet` change à chaque processus ; arrondi à 1e-5, 0 écart) |

### 10.2 Construction d'une équipe (France, une par processus)

Pic anonyme du **processus du banc** (18 Mo de base compris), temps, estimation réservée. Avant : `e66503d`.

| | données, avant | données, après | estimé | contrôleur, avant | contrôleur, après | estimé |
|---|---|---|---|---|---|---|
| A | 1,63 s, 343 Mo | 1,26 s, 61 Mo | 59 Mo | 0,99 s, 231 Mo | 1,34 s, 72 Mo | 71 Mo |
| B | 0,20 s, 55 Mo | 0,17 s, 22 Mo | 11 Mo | 0,16 s, 55 Mo | 0,22 s, 23 Mo | 13 Mo |
| C | 1,83 s, 434 Mo | 0,93 s, 41 Mo | 38 Mo | 1,28 s, 283 Mo | 1,59 s, 48 Mo | 46 Mo |
| **D** | **3,12 s, 706 Mo** | **1,58 s, 65 Mo** | **64 Mo** | 1,88 s, 418 Mo | 2,44 s, 66 Mo | 77 Mo |
| E | 0,40 s, 108 Mo | 0,24 s, 23 Mo | 12 Mo | 0,23 s, 108 Mo | 0,31 s, 23 Mo | 14 Mo |
| F à I | 0,03 à 0,07 s, 26 à 32 Mo | 0,03 à 0,04 s, 18 Mo | 3 Mo | 0,05 à 0,06 s | 0,07 à 0,09 s, 18 Mo | 3 Mo |

La réservation de D passe de **924 Mo à 141 Mo** (données 64 + contrôleur 77) ; le pic de sa construction, de 748 Mo à
65 Mo (moins la base de 18 Mo : 47 Mo). L'estimation reste au-dessus du pic mesuré moins la base pour les neuf équipes
(marge de 25 % à 100 %, plus au petit bout). **Le contrôleur est plus lent qu'avant** (D 1,88 → 2,44 s) : le balayage
unique lit la position des deux bouts de chaque arête admise pour tester la case chaude, ce que l'ancien balayage ne
faisait pas ; les deux constructions tournent en parallèle, le temps d'une équipe est celui du contrôleur (D : 3,12 s
→ 2,44 s).

### 10.3 Constat A, reproduction (conteneur 2 Gio, un seul client)

Dépôt de D seul, redémarrage du conteneur, neuf découvertes sans équipe (réserve d'espaces de travail : 149 Mo), puis
quatre générations de D (200 km gravel, 30 km route, deux fois).

| | image d'avant (`e66503d`) | image de la finition 2 |
|---|---|---|
| générations de D | **4 × 503 `overloaded` à 8,0 s** | **4 × 200** : 4,6 / 0,3 / 1,5 / 0,2 s |
| équipes construites, éviction d'équipe | — | 1 construction, 0 éviction |

### 10.4 Séance complète, 2 Gio

| | Mesure |
|---|---|
| écoute / générateur prêt | 5,5 s (pages déjà chaudes sur l'hôte) |
| 18 dépôts | 18 × 200 ; 703 Mo en 59 s, 67 Mo en 5 s, 38 Mo en 70 s, 30 Mo en 46 s, les cinq autres de 1 à 7 s ; `anon` après dépôts **477 Mo** |
| équipes résidentes | 69,6 Mo comptés (`team_cache`), 18 équipes, **0 éviction d'équipe** (le compteur `ord_generator_team_evictions_total` n'a jamais été incrémenté) |
| reconstructions de données d'équipe | **18** sur toute la séance (`ord_overlay_cache_misses_total` = 18, construites au dépôt ; 357 réussites de cache) ; avant : 262 défauts de cache |
| premières générations (18) | 18 × 200 ; médiane 0,19 s, max 0,37 s (avant : 0,88 s, max 5,58 s, 16 reconstructions) |
| réseaux, 54 en série | 31 × 200, 23 × 422 ; 0,27 / 1,41 / 3,56 s (médiane / p90 / max) |
| villes en découverte avec identifiants d'équipe, 60 en série | 59 × 200, 1 × 422 (hors couverture) ; 0,60 / 2,41 / 3,95 s ; **0 × 503** (avant : 2) |
| réseaux + villes, 114 à 4 clients, deux passages | 90 × 200, 24 × 422 chacun ; 1,14 / 3,50 / 4,48 s puis 1,32 / 2,87 / 5,32 s ; **0 × 503, 0 × 504** (avant : 10 et 10 × 503, 5 et 5 × 504) |
| évictions sur la séance | fenêtres 255, agrégats d'équipe par fenêtre 310, **espaces de travail 215** (la réserve s'évince en premier) ; équipes **0** (avant : 208 au budget, 25 au nombre) ; `team_build_escapes` et `window_escapes` : jamais incrémentés |
| `anon` | 477 Mo après les dépôts ; **1 099 Mo au plus** (au pic des réseaux en série, puis 1 090 à 1 099 Mo tant que les fenêtres tiennent le budget) ; avant : 1 309 Mo, soit **−210 Mo** |
| `memory.peak` / événements | 1 115 Mo ; `max` 0, `oom` 0, `oom_kill` 0, `OOMKilled=false` |

Les pages de fichier ne se comparent pas au §9 (3 Mo ici : le cache de pages des fichiers France était chargé dans le
groupe de contrôle d'une séance précédente) ; l'`anon` est la mesure qui compte.

### 10.5 Même séance à 1,5 Gio (2 Go moins les 512 Mo de l'altitude), dépôts et série de villes compris

18 dépôts, 18 premières générations (18 × 200), 54 réseaux (31 × 200, 23 × 422), 60 villes (59 × 200, 1 × 422),
2 × 114 à 4 clients (90 × 200, 24 × 422 chacun) : **0 × 503, 0 × 504**, 0 OOM, `memory.events` `max` 0 (le plafond de
1 536 Mo n'est jamais atteint) ; `anon` au plus **1 105 Mo**, `memory.peak` 1 128 Mo. Latences : 1,13 / 3,44 / 4,57 s puis
1,34 / 2,89 / 5,35 s.

### 10.6 Critères

| Critère | Résultat |
|---|---|
| 0 × 503 et 0 × 504 en série et à 4 clients, à 2 Gio et à 1,5 Gio | **tenu** |
| reconstructions d'équipe = nombre d'équipes | **tenu** (18 pour 18) |
| 0 éviction d'équipe | **tenu** |
| `anon` du conteneur sous 1,15 Go | **tenu** : 1,10 Go à 2 Gio, 1,105 Go à 1,5 Gio |
| 0 OOM | **tenu** |
| banc région `--compare` à 969 / 0 ; fixture figée et résultats inchangés | **tenu** |
| « pic d'anonyme du générateur nettement sous 1 Go » | **non tenu** : 1,10 Go pour un budget compté de 800 Mo ; voir ci-dessous |

### 10.7 Ce qui reste au-dessus de 1 Go

- **Le plancher n'est pas le générateur** : 477 Mo d'`anon` **avant la première génération**, au repos, après les 18
  dépôts (le budget compté du générateur n'en voit que 70, les équipes). C'est ce que retiennent le dépôt (index par
  cellule de 128 Mo, mémo géométrie → chaîne de 400 000 entrées) et l'allocateur. Les fenêtres remplissent ensuite le
  budget jusqu'à 800 Mo compté, d'où ~1,10 Go.
- Budget à **650 Mo** (`ORD_GENERATOR__MEMORY_BUDGET_MB`, sans autre changement) : mêmes résultats fonctionnels
  (0 × 503, 0 × 504, mêmes latences), `anon` au plus **975 Mo**. Le plancher théorique du budget (équipes + deux
  générations de 200 km en gravel dense + une construction : ~800 Mo, §20.9) n'est alors plus garanti dans le cas
  extrême de deux générations denses simultanées : 503 de charge, accepté au §17. À décider avec l'altitude réelle.
- `MALLOC_MMAP_THRESHOLD_=262144` (premier essai sans code du §20.8) : `anon` au plus **1 125 Mo**, plus que les
  1 099 Mo du seuil de 1 Mio : **non retenu**. `malloc_trim` reste une décision du propriétaire (§20.8).
- Non mesuré : l'altitude réelle dans le conteneur ; un disque lent ; un déploiement sur le réseau partagé.

