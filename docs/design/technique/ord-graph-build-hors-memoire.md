# Graphe vélo OSM — construction hors mémoire : conception

> Statut : conception technique, **implémentée** le 17 novembre 2026 (§13 : écarts et vérifications ; les mesures
> du §10 restent à faire). Suite de
> [`ord-graph-lot0.md`](ord-graph-lot0.md) §2, dont elle reprend le dernier levier (« tableaux de travail sur fichier
> temporaire ») et le généralise. Motif : sur le poste de 15 Gio (12 Gio libres, 4 Gio de swap, 559 Go de disque
> libre, 16 fils), le constructeur actuel plafonne vers **2,5 × la France** ; l'Europe et la planète n'entrent pas.
> **[mesuré]** : relevé déjà publié, référencé ; **[calculé]** : dérivé de mesures ; **[estimé]** : à confirmer au §10.

## 0. Périmètre

- **Dans la note** : `ord-graph-build build`, c'est-à-dire PBF → graphe de base `.ordg`. Un mode **hors mémoire** dont
  le pic de mémoire est **borné par un paramètre**, indépendant de la couverture, et qui écrit **les mêmes octets** que
  le constructeur en mémoire (hors en-tête de 4 Kio et section `STATS`, comme le contrôle de déterminisme du lot 0).
- **Hors de la note, mais nommé** (§9) : `ord-graph-build views`, qui est lui aussi proportionnel au graphe et bloque
  l'Europe au même titre ; le service et les outils de surcouche, qui lisent par `mmap` et ne sont pas en cause ici.
- **Inchangés** : le format (1.2), les attributs (`ATTR_LAYOUT`), le filtre vélo, la règle de jonction, l'ordre des
  sommets et des arêtes, le lecteur, le moteur.
- Le constructeur en mémoire **reste** : il est la référence des tests d'équivalence (§8) et le chemin le plus rapide
  pour les petits extraits.

## 1. Constat : ce que le constructeur actuel tient en mémoire

`build_graph` (`crates/ord-graph/src/build.rs:358`) puis `write_graph` (`crates/ord-graph/src/format.rs:892`).
Tailles par élément, d'après les types du code :

| Phase | Structures vivantes | Octets par élément |
|---|---|---|
| passe 1 : ways, relations | `refs: Vec<i64>`, `ways: Vec<WayRec>` | 8 par référence ; 24 par way gardée |
| nœuds utilisés | + copie triée de `refs`, `uniq: Vec<i64>`, `flags`, puis `refs: Vec<u32>` | + 8 par référence (copie), 9 par nœud utilisé |
| passe 2 : nœuds | + `coords: Vec<AtomicU64>` | 8 par nœud utilisé |
| découpe | + `segs`, `raw: Vec<RawEdge>` | 12 par segment, 24 par arête |
| sommets, cellules | + `used`, `verts` (32 o), `gid_of`, `nodes`, `edges` (40 o) | ≈ 37 par nœud utilisé, 48 par sommet, 40 par arête |
| écriture | + `dir` (12 o × arêtes orientées), **toute la géométrie** (`geoms`), `ids`, `eids`, puis **le fichier entier relu en `Vec`** pour `verify()` | ≈ 24 par arête, + taille de la section `GEOMETRY`, + taille du fichier |

Trois étapes demandent une vue d'ensemble du réseau, et c'est pour elles que tout est monté :

1. **Jonctions** : un nœud est jonction s'il apparaît au moins deux fois dans l'ensemble des ways gardées ; les ways
   d'un même lieu sont dispersées dans le fichier (triées par id, pas par position).
2. **Coordonnées** : dans un PBF `Type_then_ID`, les nœuds précèdent les ways ; il faut connaître les nœuds utiles
   (passe 1) avant de lire leurs coordonnées (passe 2).
3. **Numérotation** : le format range sommets et arêtes par cellule z11 en ordre de Morton, avec des ids globaux ; il
   faut trier tous les sommets, puis toutes les arêtes.

**[mesuré]** Pays de la Loire : pic 336 Mo à 1 fil, 439 Mo à 16 fils (lot 0, relevé §3). **[calculé]** France :
≈ 4,8 Go à 1 fil, jusqu'à 6,3 Go à 16 fils (même relevé). **[estimé]** Europe ≈ 6 à 7 × la France, soit 30 Go ou
plus ; planète ≈ 20 × la France, soit 100 Go ou plus.

Aucune des trois étapes n'exige la mémoire : ce sont deux **tris globaux** et une **jointure** entre deux listes
triées par id de nœud. Elles se font sur disque.

## 2. Référence : Valhalla (`mjolnir`)

Valhalla construit la planète sur ce poste (tuiles du 3 novembre dans `~/valhalla/world/custom_files/valhalla_tiles`,
depuis `planet-261026.osm.pbf`, 96 Go). Son constructeur (`../valhalla`) :

- **tableaux sur fichier** : `sequence<T>` (`valhalla/midgard/sequence.h`), enregistrements de taille fixe projetés
  par `mmap`, petit tampon d'écriture en mémoire ; `sort` trie par blocs de taille bornée puis fusionne dans un
  fichier temporaire ;
- **ids utiles en bitset** : `UnorderedIdTable` (`src/mjolnir/idtable.h`), un bit par id OSM, mots non vides seuls ;
- **jointure par fusion** des nœuds : `way_nodes.bin` trié par id de nœud, parcouru avec le flux des nœuds du PBF
  (`src/mjolnir/pbfgraphparser.cc`, autour de la ligne 2074), puis retrié par (way, rang) ;
- **arêtes en flux** (`ConstructEdges`), **sommets triés par tuile** sur disque (`SortGraph`), puis **une tuile à la
  fois** en parallèle (`BuildLocalTiles`).

On reprend les trois premiers principes. Le dernier ne s'applique pas tel quel : notre format a des ids globaux et
un seul fichier, pas une tuile par fichier (§5).

## 3. Principe

Chaque grande structure du §1 devient un **fichier d'enregistrements de taille fixe**. On ne la parcourt qu'en
**flux séquentiel** ou à travers un **tri externe** (§4). La mémoire ne tient que :

- les tampons des tris, sous le budget `--mem-budget-mb` ;
- le lot de blobs PBF décodés (≈ 210 Mio de surcoût à 16 fils, **[mesuré]** lot 0) ;
- la table des cellules (16 o par cellule ; quelques Mo même pour la planète, **[estimé]**) ;
- les compteurs de `BuildStats`.

**Déterminisme** : chaque clé de tri est **totale** sur l'enregistrement. Deux enregistrements de même clé sont
identiques octet pour octet. L'ordre de sortie ne dépend donc ni du budget (qui change le découpage en séries), ni du
nombre de fils.

## 4. Tri externe

Un seul composant générique, `ExtSort<R>`, dans `crates/ord-graph/src/extsort.rs` (nouveau module, feature `pbf`
ou nouvelle feature `extsort` ; pas de `unsafe`, `zerocopy` pour les enregistrements) :

1. **Séries** : les enregistrements arrivent par `push`, dans un tampon de `budget` octets. Tampon plein : tri
   parallèle (`par_sort_unstable_by_key`, clé totale), écriture séquentielle dans `tmp/<étape>-<n>.run`.
2. **Fusion** : tas de `k` têtes de série, lecture par `BufReader` de `budget / k` octets chacun. Au-delà de
   `k_max = 64` séries, fusion à plusieurs niveaux.
3. **Sortie en flux** : la dernière fusion n'est pas écrite. Elle est consommée par l'étape suivante (itérateur), ce
   qui épargne une écriture et une lecture complètes.
4. **Tout tient** : si la série unique tient dans le budget, aucun fichier n'est écrit.

Entrées-sorties **par lecture et écriture tamponnées**, pas par `mmap` : comme pour le PBF (lot 0 §2), un `mmap`
gonflerait le RSS de pages de fichier et brouillerait la mesure du pic. Le cache de pages du noyau fait le reste.

Les fichiers temporaires vont sous `--tmp-dir` (défaut : `<out-dir>/.ordg-build-<pid>/`). Ils sont supprimés en fin de
construction, et à l'échec. Un dossier de construction laissé par un processus mort est signalé, jamais réutilisé.

## 5. Étapes

Les noms d'enregistrements sont ceux du code à venir ; `u32 way` est l'**indice** de la way gardée (rang dans l'ordre
des ids), pas son id OSM.

### E1 — Passe 1 : ways et relations (flux)

Même filtre (`classify`), même dédoublonnage des références consécutives, même rejet des ways de moins de deux nœuds
distincts. Sorties :

- `ways.bin` : `(id i64, attrs u64, n_refs u32, _ u32)`, 24 o, déjà dans l'ordre des ids (le PBF est trié) ;
- `ExtSort<WayNode>` : `(node i64, way u32, rank u32)`, 16 o par référence, clé `(node, way, rank)` ;
- `ExtSort<RouteBits>` : `(way_id i64, bits u64)`, 16 o par membre way d'une relation d'itinéraire, clé totale.

Les bits d'itinéraire ne sont plus appliqués par dichotomie dans `ways`. Ils le sont en E4, par fusion de
`RouteBits` (trié par `way_id`) avec `ways.bin`. Le compteur `relation_way_members_not_in_graph` se calcule dans la
même fusion.

### E2 — Passe 2 : coordonnées par jointure (parallèle par blob)

`WayNode` trié (E1) est écrit une fois en `wn.bin`. Les nœuds du PBF sont lus **par lots de blobs, en parallèle**
(comme aujourd'hui), avec une différence : chaque blob décodé donne son intervalle d'ids `[min, max]`. Le PBF étant
trié, ces intervalles sont croissants et disjoints. Le blob ne traite que la **tranche** de `wn.bin` dont les ids sont
dans cet intervalle (deux dichotomies), par fusion de deux listes triées.

Il écrit `coords.bin`, **aligné sur `wn.bin`** : `(lat i32, lon i32)` par référence, `MISSING` pour un nœud absent de
l'extrait. Les tranches des blobs d'un lot sont disjointes. On les écrit dans l'ordre du fichier : sortie séquentielle,
identique quel que soit le nombre de fils.

Une référence dont l'id tombe entre deux blobs, ou après le dernier, reste `MISSING`. Cette règle est la même
qu'aujourd'hui : nœud référencé absent, way coupée.

La passe a besoin d'une nouvelle méthode de `OsmSource`, `nodes_by_blob`, qui livre les nœuds blob par blob avec leur
intervalle. `MemSource` l'implémente en un seul blob.

### E3 — Jonctions et retour à l'ordre des ways

On relit `wn.bin` et `coords.bin` en parallèle, séquentiellement. La série des références d'un même id donne
`multi = (longueur ≥ 2)`, ce qui est exactement `FLAG_MULTI` aujourd'hui (occurrences comptées après dédoublonnage,
toutes ways confondues, la même way comprise).

Les références vont dans `ExtSort<WayNodeC>` : `(way u32, rank u32, node i64, lat i32, lon i32, multi u8, _)`, 24 o,
clé `(way, rank)`, unique.

**Pourquoi les autres drapeaux restent locaux.** `FLAG_END` (bout de segment) et `FLAG_LOOP` (milieu d'une boucle
promu jonction) sont aujourd'hui posés sur le nœud, donc vus par toutes les ways. Un nœud qui apparaît dans une autre
way, ou deux fois dans la même, est déjà `multi`. Pour un nœud qui n'apparaît qu'une fois, le drapeau ne sert qu'à sa
propre way. E4 les traite donc way par way, sans état global : même découpe que le code actuel. Un test le vérifie
(§8).

### E4 — Arêtes (flux, dans l'ordre des ways)

On consomme la sortie de `WayNodeC`, way par way, en fusion avec `ways.bin` et `RouteBits`. Le travail de chaque way :

- segments de nœuds présents ;
- coupe aux nœuds `multi`, aux bouts de segment et au milieu des boucles ;
- longueur haversine en dm, même fonction et même arrondi.

Sorties :

- `geom0.bin` : points **intérieurs** de chaque arête, `(lat i32, lon i32)`, à la suite ;
- `ExtSort<Edge0>` : `(a i64, way_id i64, b i64, rank_lo u32, rank_hi u32, attrs u64, len_dm u32, n_int u32,
  geom_off u64)`, 56 o, ids OSM pour `a` et `b`, clé `(a, way_id, b, rank_lo)` provisoire ;
- `ExtSort<Vert>` : `(osm i64, lat i32, lon i32)`, 16 o, deux par arête ;
- compteurs de `BuildStats` (coupes, boucles, nœuds manquants), dans le même ordre qu'aujourd'hui.

`rank_lo` remplace `lo` (indice dans le tableau global `refs`). À way égale, `lo` croît avec le rang, et `lo` ne sert
qu'à départager deux arêtes de même `(a, way_id, b)`. L'ordre est donc le même.

### E5 — Sommets et cellules

`Vert` est trié par `(Morton de la cellule z11, Morton du point, osm)`, clé calculée à la volée avec
`grid::tile_of_e7`, `grid::morton` et `grid::morton_e7`. Les doublons d'un même id OSM sont adjacents, puisque les
coordonnées sont égales, et on n'en garde qu'un. Le rang de sortie est l'**id global** `gid`.

En flux, on écrit :

- `nodes.bin` : `(lat, lon)` par `gid`, futur `NodeRec` ;
- `nids.bin` : l'id OSM par `gid`, futur `ID_NODES` ;
- la table des cellules (`CellSpec`, en mémoire, petite) ;
- `ExtSort<OsmGid>` : `(osm i64, gid u32)`, clé `osm`.

### E6 — Renumérotation des arêtes

Trois tris externes de `Edge0` :

1. par `a` (OSM), fusionné avec `OsmGid` : `a` devient un `gid` ;
2. par `b` (OSM), même fusion : `b` devient un `gid` ;
3. par la **clé finale** `(a gid, way_id, b gid, rank_lo)`, celle du code actuel. Le rang de sortie est l'**id
   d'arête**.

Le dernier tri alimente E7 en flux. `duplicate_identities` se compte sur les voisins de cette sortie, comme
aujourd'hui.

### E7 — Écriture

Même disposition, mêmes octets, mais le fichier est produit en flux.

- **Arêtes orientées** : pendant la sortie de E6, chaque arête pousse dans `ExtSort<Dir>`
  `(source u32, cible u32, arête|sens u32)`, 12 o, clé totale, celle du `dir.sort_unstable()` actuel. Elle écrit aussi
  `edges.bin` (attrs, `len_dm`, `n_int`, `geom_off`, `way_id`, `a`, `b`), dans l'ordre des ids d'arête.
- **Blocs de cellule** : cellule par cellule, on lit en flux les tranches de `nodes.bin`, de `Dir` trié et de
  `edges.bin`. Le bloc est construit en mémoire, comme aujourd'hui : un bloc reste petit. La cellule d'une cible
  (`BorderRec`) se trouve par dichotomie dans la table des cellules.
- **Géométrie** : les points intérieurs sont dans `geom0.bin`, **dans l'ordre des ways**. Une lecture aléatoire, arête
  par arête, battrait le cache de pages dès que `geom0.bin` dépasse la mémoire. On fait donc une **distribution par
  paquets** :
  1. on parcourt `edges.bin` et on trie `(geom_off, id d'arête)` par `geom_off` ;
  2. on relit `geom0.bin` séquentiellement ;
  3. chaque géométrie, déjà encodée (varints zigzag en delta depuis le nœud `a`, encodage actuel), va dans le paquet de
     sa **plage de cellules** ; les paquets sont dimensionnés pour tenir sous le budget ;
  4. chaque paquet est relu, rangé par id d'arête, et concaténé dans `geom.bin`, avec un décalage par cellule.

  La section `GEOMETRY` est ensuite copiée à sa place après les blocs (`std::io::copy`, ou `copy_file_range` sous
  Linux), avec les mêmes bourrages de 4 Kio.
- **`ID_NODES`, `ID_EDGES`** : copiés de `nids.bin` et de `edges.bin`, par flux.
- **Sommes** : `xxh3` calculé en flux (`Xxh3::update`). Un test d'équivalence le confirme au premier lot (§8) : la
  somme en flux doit donner la même valeur que `xxh3_64` en un coup.
- **Relecture** : `GraphFile::open_mmap` puis `verify()`, au lieu de `std::fs::read` (§1). `verify()` ne parcourt que
  les sections et les cellules, une à une ; `validate_structure` n'alloue que par cellule (`format.rs:721`, `:756`).
- En-tête en dernier, `fsync`, `chmod 0444`, `rename` : inchangé.

## 6. Ressources attendues

| | En mémoire (actuel) | Hors mémoire |
|---|---|---|
| Pic de mémoire | ∝ graphe (France ≈ 4,8 à 6,3 Go **[calculé]**) | **budget + ≈ 0,3 Go**, quelle que soit la couverture **[estimé]** ; défaut proposé : `--mem-budget-mb 1024` |
| Disque temporaire | 0 | **France ≈ 5 Go**, Europe ≈ 35 Go, planète ≈ 120 Go **[estimé]** |
| Lectures du PBF | 3 (xxh3, passe 1, passe 2) | 3, inchangé |
| Mur | référence | **≤ 2 ×** la référence sur la France (critère, §10) |

Calcul du disque temporaire **[estimé]**. Les références sont estimées à ≈ 1,1 × les nœuds utilisés (5,84 M pour la
région, lot 0 §0), soit ≈ 95 M pour la France. Au pire moment (E3 à E4), on a :

| Fichier | Calcul | Taille |
|---|---|---|
| `wn.bin` | 16 o × 95 M | 1,5 Go |
| `coords.bin` | 8 o × 95 M | 0,8 Go |
| séries de `WayNodeC` | 24 o × 95 M | 2,3 Go |
| `geom0.bin` | — | 0,5 Go |
| **Total** | | **≈ 5 Go** |

`wn.bin` et `coords.bin` sont supprimés dès E3 consommé, ce qui ramène E5 et E6 sous ce chiffre. L'Europe est
comptée à ×6,5 et la planète à ×23, par le nombre de ways `highway` (taginfo, à relever au §10).

**Limites du format** pour la planète **[estimé]** : ≈ 550 M arêtes, sous la limite de 2^31 ; sommets sous 2^32 ;
coordonnées de cellule sur 16 bits à z11 (2^11). La limite actuelle de 2^32 références (`Pass1`) disparaît, puisque
les références ne sont plus indexées en `u32`.

## 7. Interface

```
ord-graph-build build --pbf <extrait> [--out-dir D] [--zoom 11] [--threads N] [--report R]
                      [--external] [--mem-budget-mb 1024] [--tmp-dir T]
```

- `--external` choisit le chemin hors mémoire.
- Le rapport (`build.json`) garde ses phases, nommées E1 à E7, et ajoute `tmp_peak_bytes` et `runs` (nombre de séries
  par tri).
- Sans `--external`, rien ne change.
- Décision ultérieure, après les mesures du §10 : rendre `--external` implicite au-delà d'une taille de PBF.
- Le nom de sortie (`<région>-<date>-z11.ordg`) est inchangé : `europe-latest.osm.pbf` donne
  `europe-<date>-z11.ordg`.

## 8. Tests

1. **Équivalence de bout en bout** (`tools/ord-graph-build`, e2e) : sur le mini-PBF des tests
   (`crates/ord-graph/tests/data/mini.osm.pbf`, régénéré par `scripts/ord-graph-mini-pbf.sh`), construction en mémoire contre construction hors mémoire avec un **budget
   minuscule**, qui force des centaines de séries et une fusion à plusieurs niveaux. Fichiers **identiques** hors
   en-tête et `STATS` ; compteurs de `STATS` égaux, hors phases et fils.
2. **`MemSource` aléatoires** (propriété) : graphes générés avec auto-intersections, ways repassant sur un nœud,
   boucles, nœuds manquants, relations vers des ways absentes, nœuds isolés entre deux blobs. Mêmes `GraphData` par
   les deux chemins. Ce test garde le raisonnement de E3 sur les drapeaux locaux.
3. **Déterminisme** : 1 fil contre 16 fils, et trois budgets (minuscule, moyen, tout tient), donnent les mêmes octets.
4. **`ExtSort`** : unitaires (vide, une série, `k_max + 1` séries, clés égales), et somme `xxh3` en flux égale à la
   somme en un coup.
5. **Échecs** : PBF tronqué en E2, disque plein simulé (écriture refusée) et interruption ne laissent ni fichier
   `.ordg` ni dossier temporaire ; dossier de construction préexistant signalé.
6. **Région réelle** (manuel, relevé) : Pays de la Loire, `cmp -i 4096` contre `pays-de-la-loire-20261114-z11.ordg`
   reconstruit avec le même extrait.

## 9. Ce qui reste proportionnel au graphe

- **`views`** (`crates/ord-graph/src/view.rs`, `build_view`) monte `BaseEdges` (extrémités, attributs, longueurs par
  arête de base), l'incidence et les tableaux de contraction. **[mesuré]** 618 Mo pour la région, six vues (lot 0 bis
  §4) ; **[calculé]** ≈ 9 Go pour la France par la même règle, en partie du cache de pages du `mmap`. Il faut un lot à
  part pour l'Europe : la contraction ne dépend que des degrés et des sens, ce qui se prête à un traitement par bandes
  de cellules avec reprise des chaînes qui traversent une bande. Rien n'est conçu ici.
- **Démarrage du service** : l'index de départ se construit en parcourant le graphe. **[mesuré]** 5,8 s et 8,5 Mo
  tenus pour la France (lot 6, relevé). La durée croît avec le graphe, la mémoire à peine.
- **`ord-graph-overlay`** : non examiné ici.

## 10. Protocole de mesure

Sur ce poste (15 Gio), binaire `release`, `/usr/bin/time -v`, `VmHWM` interne, taille du dossier temporaire
échantillonnée (`du -sb` chaque seconde). Le relevé va dans `ord-graph-build-hors-memoire-mesures.md`.

1. **Région** : en mémoire contre `--external` (budgets 16 Mo, 256 Mo, 1 Go) : mur, pic, disque temporaire,
   identité des octets.
2. **France** (Geofabrik `france-latest.osm.pbf`) : **en mémoire d'abord**. C'est la première mesure de ce pic qui
   n'était que calculé, et elle doit tenir sur ce poste. Puis `--external` à 1 Go. Identité des octets, rapport des
   murs (critère ≤ 2 ×), pic sous budget + 0,3 Go.
3. **Europe** (Geofabrik `europe-latest.osm.pbf`) : `--external` seul ; pic, mur, disque temporaire. Le relevé
   confirme ou corrige les estimations du §6.
4. **Planète** (`planet-261026.osm.pbf`, déjà sur le disque) : seulement si l'Europe tient ses estimations. On vérifie
   d'abord que l'en-tête déclare `Sort.Type_then_ID` ; le constructeur refuse sinon.

## 11. Ordre de réalisation

1. `ExtSort<R>` et ses tests (§8, point 4).
2. E1 à E4 derrière `--external`, `GraphData` encore produit en mémoire à la fin. Ce n'est pas encore hors mémoire,
   mais cela prouve l'équivalence de la découpe (tests 1 et 2) au plus tôt.
3. E5 à E7 : écriture en flux, relecture par `mmap`. Le pic devient borné.
4. Relevés du §10, points 1 à 3.
5. Lot suivant, à concevoir : `views` hors mémoire (§9).

## 12. Points de tension

- **Clés totales** : la moindre clé partielle rend la sortie dépendante du budget. Chaque `ExtSort` déclare sa clé,
  et le test 3 du §8 le vérifie.
- **Somme `xxh3`** : l'égalité entre la somme en flux et la somme en un coup est attendue de la bibliothèque. Elle est
  testée, pas supposée.
- **Mur** : trois tris externes de `Edge0` (56 o) plus la distribution de la géométrie représentent le plus gros
  volume d'entrées-sorties. Si le critère de ×2 n'est pas tenu, le premier levier est d'élaguer `Edge0` : porter
  `attrs` et `geom_off` dans un fichier à part, indexé par rang de sortie de E4.
- **Disque temporaire** : environ 120 Go pour la planète, à côté du PBF de 96 Go. Il faut un contrôle d'espace libre
  au départ, avec une estimation tirée de la taille du PBF.
- **Pas d'`osmium`** sur ce poste : une couverture sur mesure (France et voisins) passe par un extrait Geofabrik ou par
  l'installation d'`osmium`. Découper selon un `.poly` dans le constructeur sort du périmètre.

## 13. Réalisation (17 novembre 2026)

Code : `crates/ord-graph/src/extsort.rs` (tri externe, dossier temporaire), `crates/ord-graph/src/external.rs`
(`build_external`, E1 à E7), `StreamWriter` dans `format.rs`, option `--external` de `ord-graph-build build`
(`--mem-budget-mb`, `--tmp-dir`). Feature `external` du crate (`parallel`), dans les features par défaut.

### 13.1 Écarts à la conception

1. **E2 et E3 fusionnés, sans `wn.bin` ni `coords.bin`.** La sortie triée des références est consommée en flux, en
   même temps que les nœuds du PBF (nouvelle méthode `OsmSource::nodes_sorted`). Le parallélisme reste celui du
   décodage, blob par blob ; la jointure est séquentielle, et elle pousse directement les références complétées
   dans le tri `(way, rang)`. Cela économise deux fichiers (≈ 2,3 Go pour la France, §6) et une passe de lecture.
   La source doit livrer ses nœuds par id **strictement croissant** : sinon, erreur `NotSorted`. La construction en
   mémoire, elle, tolère le désordre.
2. **Géométrie codée dès E4.** Les varints (deltas depuis le nœud `a`) sont écrits dans `geom0` au moment de la
   découpe. `Edge0` porte le décalage et la longueur. La redistribution (E7) ne déplace que des octets : elle trie
   `GeomReq (source, destination, longueur)` par source, relit `geom0` séquentiellement, répartit en au plus 256
   paquets par destination, puis range chaque paquet en mémoire.
3. **Enregistrements** : `WayFile` 20 o (id, attributs, nombre de références) ; `RouteBits` 9 o ; `WayNodeC` 25 o ;
   `Edge0` 52 o ; `Vert` 32 o (les deux clés de Morton sont stockées) ; `EdgeRec` 20 o, plus `eids` (16 o par arête,
   la future section `ID_EDGES`).
4. **Budget** : chaque tri reçoit une part fixe du budget (moitié pour les gros tris, 1/8 à 5/16 quand plusieurs
   tris se remplissent à la fois). Pendant qu'une sortie triée est lue et qu'un autre tri se remplit, le pic des
   tampons peut aller jusqu'à environ 1,5 × le budget. **[estimé]**, à mesurer au §10.
5. **Écriture** : les deux écrivains partagent `encode_block`, `encode_geometry` et `write_header`. Le refactor de
   `write_graph` ne change aucun octet (§13.2).
6. **Pas encore fait** : le contrôle d'espace disque libre au départ (§12) ; les mesures du §10.

### 13.2 Vérifications

| Vérification | Résultat |
|---|---|
| Tests §8 (`crates/ord-graph/tests/external.rs`, `extsort` unitaires, e2e `ord-graph-build`) | 60 graphes aléatoires, à z11, z13 et z15, avec trois budgets (1 o, 4 Kio, 1 Gio) et 1 ou 4 fils ; fusion à plusieurs niveaux (plus de 64 séries) ; graphe vide ; mini-PBF par lots de 1 blob ; échec en passe 2 sans fichier restant ; dossier de construction laissé refusé ; somme `xxh3` en flux égale à la somme en un coup. Tous verts, `clippy -D warnings` propre |
| Les tests voient-ils un écart ? | Une coupe en trop introduite dans E4 : 3 tests sur 6 échouent |
| Andorre (`andorra-latest.osm.pbf` du 1er novembre, 15 240 arêtes, 11 773 sommets, 1,5 Mo) | Le binaire de `develop` (en mémoire) et la branche, en mémoire puis `--external --mem-budget-mb 1` à 1 et 16 fils, donnent des fichiers **identiques octet pour octet**, hors `build_epoch`, somme d'en-tête et `STATS`. Les compteurs de `STATS` sont égaux, hors phases et fils. De 2 à 10 séries par tri ; 6,4 Mo de disque temporaire au pic ; dossier temporaire supprimé |
| Vues d'Andorre | Les six vues construites depuis les deux bases sont identiques, sauf `base_build_epoch` dans `VIEW_META`, qui recopie la date de construction de la base |

Andorre est trop petite pour mesurer le pic mémoire : 58 Mo en `--external`, contre 52 Mo en mémoire, l'essentiel
étant le processus lui-même. Les mesures de pic, de durée et de disque temporaire restent celles du §10 (région,
France, Europe).
