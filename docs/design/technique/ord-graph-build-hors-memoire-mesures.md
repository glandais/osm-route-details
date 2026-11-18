# Graphe vélo OSM — construction hors mémoire : relevés

> Relevés du 17 novembre 2026, protocole de [`ord-graph-build-hors-memoire.md`](ord-graph-build-hors-memoire.md) §10.
> Binaire `release` de `4b08e99`, plus le plafond des lots en octets pour la seconde planète (§5.1). **[mesuré]** partout, sauf mention.

## 1. En bref

| Critère (§10) | Relevé | Verdict |
|---|---|---|
| Mêmes octets qu'en mémoire | Région à 16, 256 et 1024 Mo ; France à 1024 Mo : **identiques** de la fin de l'en-tête au début de `STATS` | **Tenu** |
| Pic ≤ budget + 0,3 Go | Budget 1 Gio : France **1,10 Gio**, Europe **1,25 Gio**, planète **2,86 Gio**, puis **1,49 Gio** avec le plafond des lots (§5.1), avant la relecture | Tenu jusqu'à l'Europe ; planète **+ 0,19 Gio** au-delà |
| Mur ≤ 2 × en mémoire (France) | 104 s contre 69 s : **× 1,50** | **Tenu** |
| Disque temporaire (§6) | France 5,67 Gio (estimé ≈ 5 Go) ; Europe **46,4 Gio** (estimé ≈ 35 Go) ; planète **139,6 Gio** | Europe **+ 33 %** : réseau 8,0 × la France, et non 6,5 × |
| France en mémoire (calculé au lot 0 : 4,8 à 6,3 Go) | **4,53 Gio** à 16 fils | Sous la fourchette |

La construction hors mémoire tient l'Europe dans **1,25 Gio** sur ce poste de 15 Gio, là où la construction en mémoire en
demanderait ≈ 36 Go **[calculé : 4,53 Gio × 191,2 / 24,0 M arêtes]**. La planète passe en 1 h 42 sous 1,49 Gio, une fois
les lots de blobs plafonnés en octets (§5.1). En mémoire, il faudrait ≈ 104 Go **[calculé, même règle]**.

## 2. Conditions

| | |
|---|---|
| Poste | AMD Ryzen 7 3800X (8 cœurs, 16 fils), 15 Gio de RAM, 4 Gio de swap ; NVMe Kingston SNV2S 1 To (ext4) ; Linux 7.3.0-10 |
| Charge | d'autres processus tenaient ≈ 3 à 4 Gio pendant les relevés ; cache de pages non vidé (pas de `sudo`) : le PBF d'une construction a pu rester en cache de la précédente |
| Commande | `/usr/bin/time -v ord-graph-build build --pbf … --out-dir … --report …` (16 fils), avec `--external --mem-budget-mb N` |
| Pic | `time -v` (pic du processus entier) et `peak_rss_before_verify_kb` (`VmHWM` juste avant la relecture par `mmap`, §13.3 point 6) |
| Comparaison | octets identiques hors `build_epoch`, somme d'en-tête, entrée `STATS` de la table et section `STATS`, dont les compteurs (hors phases et fils) sont comparés à part |

**Extraits** :

| Extrait | Taille | Réplication | Remarque |
|---|---|---|---|
| `pays_de_la_loire.osm.pbf` | 0,43 Go | 2026-11-17 | mis à jour par `pyosmium-up-to-date` 4.0.2, qui n'écrit pas `Sort.Type_then_ID` dans l'en-tête |
| `france.osm.pbf` | 5,9 Go | 2026-11-17 | idem |
| `europe-latest.osm.pbf` (Geofabrik) | 35,5 Go | 2026-11-16 | trié, déclaré |
| `planet-261026.osm.pbf` | 95,6 Go | 2026-10-26 | trié, déclaré (`planet-dump-ng`) |

Les deux extraits mis à jour sont bien triés (`osmium fileinfo -e` : « Objects ordered (by type and id): yes »). Le
constructeur exige la déclaration : on a construit sur une **copie** dont seul l'en-tête porte en plus
`Sort.Type_then_ID` (le reste des octets recopié tel quel). Les deux constructions contrôlent en plus l'ordre des ways et
des relations, et la construction hors mémoire celui des nœuds.

## 3. Relevés

| Construction | Mur | CPU | Pic `time -v` | Pic avant relecture | Disque temporaire | Arêtes | Sommets | Fichier |
|---|---|---|---|---|---|---|---|---|
| Région, en mémoire | 5,2 s | 23 s | 0,41 Gio | — | — | 1 695 641 | 1 323 871 | 128 Mo |
| Région, `--external` 16 Mo | 6,7 s | 24 s | 0,31 Gio | 0,31 Gio | 0,38 Gio | idem | idem | idem |
| Région, `--external` 256 Mo | 6,7 s | 24 s | 0,35 Gio | 0,35 Gio | 0,37 Gio | idem | idem | idem |
| Région, `--external` 1024 Mo | 5,9 s | 24 s | 0,46 Gio | 0,46 Gio | 0,10 Gio | idem | idem | idem |
| France, en mémoire | 69 s | 320 s | **4,53 Gio** | — | — | 24 003 142 | 18 761 877 | 1,89 Go |
| France, `--external` 1024 Mo | 104 s | 355 s | 1,94 Gio | **1,10 Gio** | 5,67 Gio | idem | idem | idem |
| Europe, `--external` 1024 Mo | 17 min 16 s | 2 412 s | 9,38 Gio | **1,25 Gio** | **46,4 Gio** | 191 170 878 | 149 647 667 | 15,2 Go |
| Planète, `--external` 1024 Mo | 1 h 47 min 43 s | 7 797 s | 13,23 Gio | **2,86 Gio** | **139,6 Gio** | 553 241 291 | 430 003 277 | 45,8 Go |

**Séries écrites par tri** (0 : tout a tenu dans le tampon) :

| Tri | Région 16 Mo | Région 256 Mo | France 1 Gio | Europe 1 Gio | Planète 1 Gio |
|---|---|---|---|---|---|
| `waynode` | 14 | 0 | 4 | 28 | 87 |
| `waynodec` | 22 | 2 | 6 | 44 | 137 |
| `verts` | 35 | 3 | 8 | 61 | 178 |
| `edges-a` / `-b` / `-final` | 17 / 11 / 11 | 2 / 0 / 0 | 4 / 3 / 3 | 30 / 19 / 19 | 87 / 54 / 54 |
| `dir` | 7 | 0 | 2 | 12 | 32 |
| `geomreq` | 20 | 2 | 5 | 35 | 100 |
| `osmgid` | 2 | 0 | 0 | 4 | 10 |

Jusqu'à l'Europe, aucune fusion à plusieurs niveaux (au plus 61 séries, sous `K_MAX` = 64). La planète en fait
sur six tris (de 87 à 178 séries).

**Phases** (s) :

| Phase | France en mémoire | France `--external` | Europe `--external` | Planète `--external` |
|---|---|---|---|---|
| ouverture du PBF (xxh3) | 2,6 | 2,5 | 33,3 | 89,7 |
| passe 1 / E1 | 36,4 | 38,8 | 278,6 | 981,6 |
| nœuds utilisés + passe 2 / E2–E3 | 3,5 + 8,6 | 19,7 | 162,0 | 1 454,5 |
| découpe / E4 | 6,0 | 19,8 | 154,9 | 1 643,7 |
| sommets, tri des arêtes / E5 + E6 | 4,3 | 3,7 + 6,7 | 39,4 + 60,9 | 455,8 + 700,8 |
| écriture, relecture / E7 + relecture | 7,7 | 11,9 + 0,9 | 264,3 + 42,9 | 1 013,6 + 124,1 |

## 4. Lecture

- **Le budget tient.** Le pic avant relecture est le budget plus ≈ 0,1 à 0,25 Gio (décodage des blobs, table des
  cellules), y compris sur l'Europe. Il ne grandit presque pas de la France à l'Europe (1,10 → 1,25 Gio) quand le
  graphe est multiplié par 8.
- **Le pic `time -v` n'est pas la mémoire de la construction** : la relecture finale projette le fichier par `mmap`, et
  ses pages de fichier (récupérables par le noyau) comptent dans `VmHWM`. Pour l'Europe, 9,4 Gio pour un fichier de
  15 Go. Dans un conteneur à mémoire limitée, ce sont des pages de cache que le noyau reprend sous pression ; à
  vérifier en conteneur avant d'y construire l'Europe.
- **Petits budgets** : à 16 Mo, la région prend 6,7 s pour 5,2 s en mémoire, et son pic (0,31 Gio) est sous celui de
  la construction en mémoire (0,41 Gio).
- **Mur** : × 1,5 sur la France. La passe 1 domine (décodage des ways, comme en mémoire) ; E7 pèse plus sur l'Europe
  (264 s, 25 %) que sur la France (12 s, 11 %) : la redistribution de la géométrie relit `geom0`, et l'Europe ne
  tient plus en cache de pages.
- **Estimations du §6 à corriger** : l'Europe fait **8,0 ×** la France en arêtes (6,5 × estimés), et le disque
  temporaire suit (46,4 / 5,67 = 8,2 ×). Règle plus sûre que le poids du PBF : le pic de temporaire vaut **≈ 3,2 × le
  fichier écrit** (en octets : France 6,09 / 1,89 Go = 3,2 ; Europe 49,8 / 15,2 Go = 3,3).
- **Ce qui reste proportionnel** : `views` (§9), à construire pour l'Europe ; non relevé ici.

## 5. Planète

`planet-261026.osm.pbf` (95,6 Go), `--external --mem-budget-mb 1024`, 16 fils. Sortie de la construction : code 0 ;
`verify()` passé. Pas de construction en mémoire pour comparer les octets : elle demanderait ≈ 104 Go **[calculé]**.

- **Réseau** : 553,2 M arêtes, 430,0 M sommets, 262,9 M ways retenus sur 1 227,6 M. C'est **2,9 ×** l'Europe et
  **23 ×** la France. Le fichier fait 45,8 Go.
- **Disque** : le pic de temporaire est de 139,6 Gio. En octets, cela fait 149,9 / 45,8 Go = **3,3 × le fichier**, ce
  qui confirme la règle du §4. Pendant la construction, le disque est descendu de 684 à 641 Go libres une fois le
  fichier écrit. Il faut prévoir ≈ 200 Go libres : temporaire + fichier.
- **Mur** : 1 h 48, contre 17 min pour l'Europe. Le temps croît de **6,2 ×** pour 2,9 × d'arêtes. E1 croît comme le
  PBF (× 3,5). E2–E4 croissent bien plus vite (× 9 à × 10,6). Dans E4, le CPU n'occupe plus que 873 s sur 1 644 s :
  le reste est de l'attente d'entrées-sorties. Les flux à fusionner ne tiennent plus en cache de pages, et six tris
  font une fusion à deux niveaux.
- **Mémoire : budget dépassé, cause trouvée (§5.1).** Le pic avant la relecture est de **2,86 Gio**, contre 1,25 Gio
  pour l'Europe. Le dépassement apparaît dès E1 : 2,78 Gio de pic, et 2,59 Gio encore résidents en fin de phase,
  contre 0,78 Gio pour l'Europe.
- **Pic `time -v`** : 13,2 Gio. Ce sont les pages du fichier projeté par `mmap` lors de la relecture, comme pour
  l'Europe (§4).

### 5.1 Cause et correctif : lots de blobs plafonnés en octets

- **`MALLOC_ARENA_MAX=2` est écarté.** La construction ainsi relancée donne le même graphe en 1 h 57. Son pic avant
  relecture est de 3,07 Gio, et 2,57 Gio restent résidents en fin d'E1 : l'allocateur n'y est pour rien.
- **Les blobs de la planète sont 27 × plus gros.** `planet-dump-ng` écrit 53 331 blobs de 1,79 Mo compressés en
  moyenne (3,98 Mo au plus). L'extrait Europe de Geofabrik en compte 545 883, de 65 Ko en moyenne.
- **Le lot de blobs n'était borné qu'en nombre.** Il comptait 64 blobs (4 par fil), soit ≈ 115 Mo compressés pour la
  planète, et ≈ 1,5 Gio une fois décodés, hors de tout budget.
- **La sonde le confirme.** Sur les 4 premières minutes de la planète, avant que les tampons de tri ne se
  remplissent, le pic est de 1 790 Mio avec 16 fils et de 432 Mio avec 4 fils (lots de 16 blobs).
- **Le correctif** (`PbfSource::with_batch_bytes`) plafonne aussi le lot en octets du fichier, avec au moins un blob
  par lot. La construction hors mémoire fixe ce plafond à 1/64 du budget, soit 16 Mo pour 1 Gio. Pour les extraits
  Geofabrik, rien ne change (64 blobs ≈ 4 Mo). Andorre construite avec le plafond reste identique à develop.

**Planète avec le plafond** (même PBF, budget 1 Gio, 16 fils) :

| | Sans plafond | Avec plafond |
|---|---|---|
| Mur | 1 h 47 min 43 s | **1 h 42 min 10 s** |
| CPU (utilisateur + système) | 7 798 s | 6 887 s |
| Pic avant relecture | 2,86 Gio | **1,49 Gio** |
| Pic en E1 / en fin d'E1 | 2,78 / 2,59 Gio | 1,21 / 0,84 Gio |
| Arêtes, sommets, disque temporaire | 553 241 291, 430 003 277, 139,6 Gio | identiques |

Le pic restant (1,49 Gio) est atteint en E2–E3. La fusion de `waynode` y tient la moitié du budget, et le tri de
`waynodec` l'autre moitié, comme pour l'Europe (1,25 Gio). S'y ajoutent les nœuds décodés du lot : 16 Mo compressés,
contre ≈ 4 Mo pour Geofabrik. Le critère « budget + 0,3 Go » (1,3 Gio) est donc dépassé de **0,19 Gio**. Deux façons
de le tenir : un plafond de 1/256 du budget (4 Mo, autant qu'un lot Geofabrik), ou compter le lot décodé dans le
budget des tris d'E2. La première ne réduit que le parallélisme du décodage. **[à mesurer]**

Le plafond ne ralentit rien. Le mur baisse même de 5 %, et le CPU de 12 %. Moins de blobs décodés d'avance, cela
veut dire moins de pages à toucher.

Sur ce poste (15 Gio), la planète se construit en 1 h 42 sous 1,5 Gio, avec ≈ 200 Go de disque libre.
