# Générateur de parcours — lot 6 : relevés (France entière dans 2 Go)

> Relevés de relecture du 16 novembre 2026, binaires `release` du commit `c3bbd51` de la branche
> `feat/ord-loops-window` et du tag `v1.9.0` (moteur à plat, code identique à `develop`), Rust 1.100.0, poste de
> développement (AMD Ryzen 7 4800H, 16 fils, 30 Go, NVMe). Graphe et vues France : extrait Geofabrik du 15/11 ;
> région : Pays de la Loire, extrait du 14/11. Conception : [`ord-loops-lot6.md`](ord-loops-lot6.md). Mo et Go
> décimaux. Équipes désignées par les lettres A à I ; aucune coordonnée de départ, aucun identifiant d'équipe ni secret.
> Les seuls lieux nommés sont des centres-villes publics, sans lien avec les départs des équipes.
>
> Tout ce qui suit a été **remesuré** à la relecture, sauf la §6 (relevés du premier passage, non refaits). Les deux
> binaires du banc ont reçu, hors dépôt, la même instrumentation d'une vingtaine de lignes : par requête, l'empreinte
> FNV-1a du `Debug` complet du résultat et des diagnostics, **et la liste des arêtes de base orientées** de chaque
> parcours.

## 1. Équivalence

| Quoi | Comment | Résultat |
|---|---|---|
| Moteur fenêtré contre moteur à plat d'avant le lot, région | `ord-loops-bench --modes strict,team,discovery --rounds 0` : binaire `v1.9.0` puis binaire du lot (moteur fenêtré, cache sous 900 Mo), **toutes** les requêtes du banc, 323 × 3 modes = **969** ; fichiers d'empreintes triés et comparés octet à octet | **969 / 969 identiques**, empreinte du `Debug` complet **et** liste des arêtes de base ; 696 requêtes rendent des parcours (strict 91, équipe d'abord 300, découverte 305), 273 échouent de la même façon des deux côtés |
| Fenêtré contre fenêtre de toutes les tuiles, même processus, région | `--compare` sur les mêmes 969 requêtes | 0 différence |
| Index globaux contre la force brute, région | `--self-check` | chaînes étrangères 9 051 + 13 293, chaînes sous le plafond 9 518 + 15 782, degré admis de 820 044 + 977 849 nœuds : identiques (1,5 s) |
| Référence figée, graphe synthétique | `cargo test --workspace --locked` ; la fixture `window_reference.txt` (140 requêtes, z11 et z16) n'a pas changé depuis le commit de l'étape 0, écrit avant le moteur fenêtré | tests verts |
| France contre région, mêmes requêtes | banc fenêtré France (surcouches France, mêmes 969 requêtes) contre le banc fenêtré région | requêtes **intérieures** à la région (disque de la requête dans le polygone) : **591 / 591 même issue** ; sur les 435 qui rendent des parcours, 386 ont des géométries identiques (empreinte `geom_xxh3`), 48 des parcours de mêmes longueurs au mètre mais d'autres géométries, 1 des longueurs égales au décimètre. Requêtes **de bord** : 370 / 378 même issue, 8 échecs « aucun candidat » de la région réussissent sur la France (le graphe ne s'arrête plus à la frontière régionale) |

Les écarts de géométrie France / région ne mettent pas le moteur en cause : les deux graphes sont des extraits de dates
différentes (14 et 15 novembre) et **numérotent autrement** nœuds et arêtes ; or le moteur départage les coûts égaux par
le plus petit identifiant. Des parcours de même longueur et d'autre tracé sont la signature de tels départages.
L'équivalence exacte se prouve sur un même graphe : c'est la première ligne.

## 2. Mémoire

### 2.1 Banc France sous 2 Go, cache de pages froid

`posix_fadvise(DONTNEED)` des trois fichiers avant le lancement (sinon les pages déjà en cache ne sont pas comptées au
groupe de contrôle et la limite n'est pas éprouvée), puis
`systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 ord-loops-bench … --modes strict,team,discovery
--rounds 1 --budget-mb 900`, neuf équipes, surcouches France, **1 644 requêtes** :

- code 0, sans mise à mort (le groupe reste à 2 Go, les pages de fichier sont reprises au fil de l'eau) ; `VmHWM`
  2 031 Mo, dont 853 Mo de pages de fichier à la fin ;
- 55 fenêtres construites, 1 589 servies du cache, 8 évincées, **0 échappement** ; fenêtres de 244 464 chaînes en
  médiane, 590 262 au plus ; mémoire comptée au plus 900 Mo ;
- `RssAnon` : 1 050 Mo à la fin ; **1 459 Mo au plus**, pendant la construction des données de l'équipe D. Le banc ne
  réserve pas la construction d'une équipe dans son budget (le service, si) : les fenêtres en cache (jusqu'à 900 Mo) et
  la construction de D s'additionnent. La ligne « pic au total » du résumé du banc (1 050 Mo) ne couvre pas ces
  constructions.

### 2.2 Le service sur la France (`osm-route-details`, configuration par `ORD_GENERATOR__*`)

Lancé sur `127.0.0.1`, sans altitude (`ORD_CACHE__OFFLINE=true`, IGN désactivé), sous
`systemd-run --user --scope -p MemoryMax=… -p MemorySwapMax=0`, `MALLOC_ARENA_MAX=2`, budget de 900 Mo, cache de pages
des trois fichiers évincé au départ, polygone de couverture rectangulaire autour des nœuds (voir §4). Clé d'espace de
noms tirée au hasard par le pilote, jamais écrite. Déroulé : cinq dépôts réels (703, 67, 38, 30 et 6 Mo de NDJSON),
puis 70 générations en série dans dix villes (Paris, Lyon, Marseille, Toulouse, Bordeaux, Lille, Strasbourg, Brest,
Grenoble, Nantes : boucles de 50, 100 et 200 km en route et en gravel, et un aller simple de 200 km par ville, en
découverte), deux fois ; 60 générations en série depuis les réseaux déposés (30 à 200 km, trois types, curseurs 0,
0,25 et 1) ; puis 150 générations (réseaux et villes) à 4 clients et à 12 clients : 500 générations par séance.
`anon` du groupe échantillonné toutes les 0,25 à 0,5 s, `memory.events` relu à la fin. La colonne « 1,5 Go » retire de
la limite les 512 Mo des deux caches de l'altitude, qui partage le conteneur en production.

| | 2 Go, `MALLOC_MMAP_THRESHOLD_=1048576` | 2 Go, sans ce réglage | 1,5 Go, avec le réglage |
|---|---|---|---|
| écoute / `load_ms` (pages froides) | 7,9 s / 7 605 ms | 8,1 s / 7 725 ms | 9,2 s / 8 875 ms |
| `anon` au démarrage (index), au plus | 286 Mo | 358 Mo | 305 Mo |
| `anon` après les cinq dépôts | 210 Mo | 267 Mo | 210 Mo |
| `anon` au plus, sur la séance | **1 101 Mo** | **1 743 Mo** | **1 106 Mo** |
| pages de fichier au plus bas | 1 026 Mo | 540 Mo | 476 Mo |
| `oom` / `oom_kill` | 0 / 0 | 0 / 0 | 0 / 0 |
| villes, 2 × 70 en série (méd. / p90 / max, 2e passage) | 140 × 200 ; 0,96 / 2,72 / 5,28 s | 140 × 200 ; 0,92 / 2,41 / 5,05 s | 140 × 200 ; 0,98 / 2,50 / 5,30 s |
| réseaux, 60 en série | 38 × 200, 21 × 422, **1 × 504** (8,04 s) | 38 × 200, 21 × 422, **1 × 504** (8,60 s) | 38 × 200, 21 × 422, **1 × 504** (8,91 s) |
| 150 à 4 clients | 123 × 200, 27 × 422 ; 1,39 / 3,20 / 7,42 s | 123 × 200, 27 × 422 ; 1,32 / 3,20 / 6,62 s | 123 × 200, 27 × 422 ; 1,35 / 3,38 / 7,23 s |
| 150 à 12 clients | 100 × 200, 22 × 422, 23 × 503, **5 × 504** | 102 × 200, 24 × 422, 23 × 503, **1 × 504** | 101 × 200, 23 × 422, 23 × 503, **3 × 504** |

Les 422 sont les échecs ordinaires de ces tirages (départ hors couverture, trop loin du réseau strict, réseau trop
petit, aucun candidat). Les 503 `overloaded` à 12 clients sont la file pleine (2 calculs, 8 en attente), comme au
lot 3a. Les **504** sont des générations qui dépassent le budget de 8 s ; celle de la série tombe sur une des premières
requêtes d'un réseau tout juste déposé (voir §3.3).

Métriques à la fin (2 Go, avec le réglage) : `ord_generator_memory_bytes` fenêtres 418 Mo, agrégats d'équipe 120 Mo,
espaces de travail en réserve 259 Mo ; 318 fenêtres construites (146 route, 172 gravel) pour 149 servies du cache, 309
évincées : dix villes à trois distances et deux vues ne tiennent pas ensemble dans 900 Mo, le cache tourne ;
0 échappement, 0 parcours refusé par le contrôleur indépendant.

**Le réglage `MALLOC_MMAP_THRESHOLD_` est indispensable** : sans lui, le tas garde 640 Mo de plus que ce que le budget
compte, et les pages de fichier tombent à 540 Mo ; avec les 512 Mo de caches de l'altitude dans le même conteneur, le
service sans réglage dépasserait 2 Go. Avec le réglage, le générateur tient dans 1,5 Go sans perte de temps mesurable.
Le réglage n'est posé ni dans le `Dockerfile` du dépôt ni dans le code.

## 3. Temps

### 3.1 Génération à chaud, région : moteur fenêtré contre moteur à plat

`ord-loops-bench --modes strict,team,discovery --rounds 3 --limit 25` (129 requêtes × 3 modes), un fil, l'un après
l'autre sur la machine peu chargée : binaire du lot, binaire `v1.9.0`, binaire du lot une seconde fois (pour la
dispersion). Médiane des trois tours chauds par requête, puis **médiane / p90 / maximum en ms** :

| Mode | à plat (`v1.9.0`) | fenêtré, 1er passage | fenêtré, 2e passage | écart de médiane |
|---|---|---|---|---|
| strict | 2 / 68 / 199 | 2 / 72 / 207 | 2 / 71 / 207 | égale |
| équipe d'abord | 247 / 772 / 1 886 | 256 / 791 / 1 945 | 257 / 790 / 1 938 | **+ 4 %** |
| découverte | 175 / 559 / 1 006 | 178 / 569 / 1 008 | 181 / 562 / 1 007 | **+ 2 à + 3 %** |
| tout | 111 / 618 / 1 886 | 116 / 629 / 1 945 | 115 / 629 / 1 938 | + 4 % |

RSS maximal : 644 à 652 Mo fenêtré, 723 Mo à plat. Durée du banc : 5 min 57 s fenêtré, 5 min 40 s à plat (la
construction des données d'équipe sur zone et des tables du contrôleur compte dedans).

### 3.2 France

| | Mesure |
|---|---|
| Ouverture du moteur, banc, pages froides | 5,9 s (index : route 2,9 + 0,66 s, gravel 4,8 + 1,1 s, une vue par fil) ; 8,5 Mo tenus |
| Démarrage du service, pages froides | écoute à 7,9 à 9,2 s, `load_ms` 7,6 à 8,9 s (§2.2) |
| Génération, banc France (1 tour chaud) | strict 2 / 92 / 271 ms, équipe d'abord 258 / 886 / 2 058 ms, découverte 171 / 496 / 1 177 ms (médiane / p90 / max) ; même ordre que la région au même banc (4 / 108 / 326, 255 / 865 / 1 942, 162 / 495 / 1 187) |
| Fenêtre à froid, banc France | 88 / 198 / 464 ms (médiane / p90 / max, 55 constructions) |
| Fenêtre à froid, service France (2 Go) | gravel 0,30 / 0,52 / 0,72 s, route 0,11 / 0,28 / 0,37 s (médiane / p90 / max, 172 et 146 constructions, fenêtres jusqu'à 929 417 chaînes en route) |
| Dépôts, service France | 703 Mo en 72 s, 67 Mo en 5,5 s, 38 Mo en 88 s, 30 Mo en 58 s, 6 Mo en 8 s (mêmes durées aux trois séances) |

Critère « fenêtre à froid ≤ 2 s pour 100 km » : tenu (0,72 s au plus, toutes distances jusqu'à 200 km confondues).

### 3.3 Données d'une équipe sur la France : la première requête

Le banc France sous 2 Go, pages froides, relève par équipe la construction de `TeamData` puis des tables du
contrôleur (`TeamCheck`) : A 2,4 + 1,0 s, C 3,4 + 1,5 s, **D 6,3 + 2,5 s, soit 8,8 s**, les autres sous 0,5 s.
Le service fait ces deux constructions **dans la première génération** qui suit un dépôt ou un redémarrage, sous son
budget de 8 s : pour D, elles dépassent à elles seules le budget quand les pages sont froides, et pour A et C une
génération de quelques secondes suffit à le dépasser. Les 504 de la série du §2.2 (un par séance, toujours parmi les
premières requêtes des réseaux déposés) sont de cette nature.

## 4. Ce que les relevés ont trouvé hors du moteur

- **Le polygone `france.poly` de Geofabrik éteint le générateur** : au démarrage, « generator coverage polygon does not
  match the graph extent » (sa boîte déborde de 1,8° à l'ouest de celle des nœuds, au-delà des 15 km admis). Les relevés
  ont tourné avec un rectangle autour des nœuds. Il faut un polygone de couverture à la mesure du graphe avant tout
  déploiement de la France.
- **Dépôts lents** : 38 Mo en 88 s et 30 Mo en 58 s, plus lents que 703 Mo en 72 s ; non comparé à la région.

## 5. Ce qui n'a pas été mesuré

- Le service **avec l'altitude** dans le même conteneur : seulement approché en abaissant la limite du groupe à 1,5 Go
  (§2.2).
- **Docker** : relevés sous `systemd-run` seulement.
- Un **disque lent** (réseau) : fichiers sur NVMe ; une fenêtre froide coûte surtout de la lecture de pages.
- Le chemin « échappement, puis fenêtre élargie » du service : aucun échappement aux relevés (0 sur 969 + 1 644
  requêtes au banc et 1 500 générations au service).
- Les distances au-delà de 200 km par HTTP (le contrat s'arrête à 200 km) ; le pire aller simple de 300 km n'est relevé
  qu'en §6.

## 6. Relevés du premier passage, non refaits

Repris de la version précédente de ces relevés, sans les avoir rejoués :

- sonde `window_probe` (fenêtres de classe autour de Paris, Lyon, Nantes et Clermont-Ferrand) : boucle de 200 km en
  gravel à Paris, 1 382 579 chaînes, 96 Mo de fenêtre et 188 Mo d'espace de travail ; pire aller simple de 300 km
  (Lyon, gravel) 2 234 108 chaînes, 158 Mo et 316 Mo, construit en 0,68 s ; espace de travail mesuré à 142 à 146 o par
  chaîne ;
- pic de mémoire transitoire de la construction des données d'équipe, une à une : D 704 Mo, C 434 Mo, A 347 Mo
  (estimation réservée à 210 o par chaîne : 749, 468, 321 Mo) ;
- comparaison des empreintes `--dump` du binaire `v1.9.0` et du binaire du lot sur 1 292 requêtes de la région (quatre
  crans du curseur), et `--compare` sur 688 requêtes de la France : 0 différence.

## 7. Écarts à la note et au compte rendu

- La note (§19 point 7) dit l'espace de travail réservé à 118 o par chaîne ; le code et le point 12 disent 150 o.
- La note (§10.1) annonce un budget unique qui borne ce que le générateur alloue ; mesuré, il ne le borne qu'avec
  `MALLOC_MMAP_THRESHOLD_` (§2.2), et les agrégats d'équipe d'une fenêtre neuve ne sont comptés qu'après coup.
- Le compte rendu annonçait 0 réponse 5xx hors 503 au service : la relecture relève un 504 en série à chaque séance et
  1 à 5 à 12 clients (§3.3).
- Le compte rendu donnait 4,5 à 5,9 s d'ouverture du service : c'est pages chaudes ; pages froides, 7,6 à 8,9 s.
- Les relevés mémoire du premier passage au service ont été pris pages déjà en cache : celles-ci n'étaient pas comptées
  au groupe de contrôle (« file » à 2 Mo), si bien que la limite de 2 Go n'éprouvait que l'anonyme. Pages froides, le
  groupe se remplit de pages de fichier jusqu'à la limite dès l'ouverture et les rend sous pression, sans mise à mort.
