# Graphe vélo OSM en cellules — lot 0 : construction, format, mesure

> Statut : conception technique, **implémentée** (§9 résultats, §10 écarts). Exigences : plan Pédalons `docs/plans/2026-11-15-generateur-parcours-osm.md`
> (§1, §3, §4, §11, §12 ; ledger `API-204`). Ce lot **mesure** : ses chiffres décident du reste du plan et passent
> avant les fonctions. **[mesuré]** : relevé du 15 novembre 2026 sur l'extrait ci-dessous ; **[estimé]** : à confirmer.

## 0. Périmètre

- Crate `crates/ord-graph` (Dijkstra par arêtes orientées **pour la mesure seulement**) : modules `attrs`, `build`
  (depuis une source abstraite `OsmSource`), `pbf` (feature), `format`, `mmap` (feature, le seul `unsafe`), `search`,
  `stats` ; aucune dépendance `ord-*`, compilable en WASM sans `pbf` ni `mmap`. Outil `tools/ord-graph-build` (clap).
- Hors lot 0 : dénivelé, attracteurs, génération, HTTP, index « point → nœud », restrictions de tourner, barrières,
  `sac_scale` / `width` / `maxspeed` / `lanes` (bits réservés). Aucun crate existant ne change.
- Donnée : Geofabrik `pays-de-la-loire-latest.osm.pbf` dans `~/.cache/osm-route-details/graph/`, md5 vérifié.
  **[mesuré]** 374 Mo, réplication `2026-11-14T21:20:41Z`, trié `Type_then_ID` ; 39,2 M nœuds, 5,41 M ways, 84 823
  relations ; ids max : nœud 14 378 997 708, way 1 579 501 475 (64 bits) ; classes gardées : 854 036 ways, 5,84 M nœuds.

## 1. Filtre vélo et attributs

**Classes gardées** : `motorway`, `trunk`, `primary`, `secondary`, `tertiary` (et leurs `_link`), `unclassified`,
`residential`, `living_street`, `service`, `track`, `path`, `cycleway`, `footway`, `bridleway`, `steps`,
`pedestrian`, `road`, `busway` : `ROUTABLE_KINDS` de T03 (`sidewalk`, `crossing` sont ici des `footway=*`). Tout
le reste est **écarté** et compté par motif (`construction`, `proposed`, `platform`, `corridor`, `raceway`…), comme
toute way `area=yes`.

**Accès résolu** : la valeur la plus précise de `bicycle` > `vehicle` > `access` (valeur inconnue : clé suivante) ;
sans tag, défaut de classe de T03 §4.2 (fermés : `motorway`, `trunk`, `pedestrian`, `bridleway`, `busway`).

| État | Valeurs |
|---|---|
| `Yes` | `yes`, `permissive`, `designated`, `official`, `public`, `allowed`, `use_sidepath`, `dismount`, `discouraged`, `mtb` |
| `Destination` | `destination`, `delivery`, `customers` |
| `Private` | `private`, `permit`, `residents`, `restricted` |
| `No` | `no`, `agricultural`, `forestry`, `emergency`, `psv`, `bus` |

**Règle d'entrée** : `motorway` et `trunk` (et `_link`) n'entrent que si l'accès n'est pas `No` ; **tout le reste
entre**, avec son état. Le graphe est un sur-ensemble : un `footway` `bicycle=no` servira à l'appariement (plan
§4.2), le moteur l'exclura par son coût (plan §6.5). Écart voulu à T03 §4.2 : shortbread@1.1 confondait
`bicycle=no` et `access=private`, les tags bruts non. **[mesuré]** `access=private` sur 56 130 ways, `bicycle=no`
sur 5 076, `oneway:bicycle` sur 7 314.

**Sens** : voiture `oneway=yes|true|1` → avant, `-1|reverse` → arrière, `reversible|alternating` → double sens
(compté) ; implicite sur `junction=roundabout|circular` et `motorway`. Le vélo suit, sauf `oneway:bicycle=no|yes|-1`,
`cycleway=opposite*`, `cycleway:{left,right,both}=opposite*`, `cycleway:{left,right,both}:oneway=no|-1`. Sens ouvert
au vélo et fermé aux voitures : bit `contraflow`. Ignorés, comptés : `*:conditional`, `bicycle:forward|backward`,
`motor_vehicle`. **Relations** `type=route`, `route=bicycle|mtb`, hors `state=proposed`, membres way : `icn|ncn` →
national, `rcn` → régional, `lcn` ou sans `network` → local, `mtb` → VTT ; super-relations non suivies.

**Un `u64` par arête non orientée** :

| Bits | Champ | Valeurs |
|---|---|---|
| 0–4, 5 | classe, `_link` | 18 classes, 0 réservé |
| 6–10 | `surface` brute | 0 absent, 1–30 table fixe de valeurs exactes (`asphalt` … `grass_paver`, les plus fréquentes de l'extrait), 31 autre. Revêtu / non revêtu reste **calculé** par la table de T03 §7 |
| 11–13 | `tracktype` | 0 absent, 1–5, 7 autre |
| 14–17 | `smoothness` | 0 absent, 1–8 `excellent` … `impassable`, 15 autre |
| 18–20 | `mtb:scale` | 0 absent, 1–7 = 0–6 (`+` / `-` retirés) |
| 21–22 | sens voiture | aucun, avant, arrière, réversible |
| 23–25 | vélo | `fwd_ok`, `bwd_ok`, `contraflow` |
| 26–29 | accès résolu ; tag `bicycle` | 4 états ci-dessus ; absent, ouvrant, `designated`, `dismount`/`discouraged` |
| 30–31 | `service=*` | autre, `driveway`, `parking_aisle`, `drive-through`/`emergency_access` |
| 32–35 | itinéraires | local, régional, national, VTT |
| 36–37 | aménagement `cycleway*` | aucun, `shared_lane`, `lane`, `track` |
| 38–39 | `bridge`, `tunnel` | pour le dénivelé du lot suivant |
| 40–63 | réservés | `sac_scale`, `maxspeed`, `lanes`, `width` (version mineure de disposition) |

`fwd_ok` / `bwd_ok` doublent les arêtes orientées (§3.3) : un sens fermé laisse l'arête visible. `measure` compte les
valeurs distinctes du `u64` : sur 16 bits, un dictionnaire gagnerait 6 o par arête (décision du lot 6).

## 2. Construction en mémoire bornée : deux passes

osmpbf `BlobReader` **en lecture** (sa variante `mmap` gonflerait le RSS) ; blobs décodés en parallèle (rayon), retriés
par id : **mêmes octets quel que soit le nombre de fils**. Un PBF non trié `Type_then_ID` est refusé.

1. **Passe 1, ways puis relations.** Ways filtrées → `ways: Vec<(id i64, attrs u64, début u32)>` et `refs: Vec<i64>`
   à plat. Les relations, qui suivent dans la même passe, OR-ent leurs bits d'itinéraire par dichotomie dans `ways`.
2. **Nœuds utilisés.** Copie de `refs`, `par_sort_unstable`, parcours des séries → `U: Vec<i64>` trié et un compteur
   d'occurrences saturé à 2 ; extrémités de way marquées ; `refs` devient des indices `u32` dans `U`, la version
   `i64` est libérée. **Jonction** = occurrence ≥ 2 (croisement, couture entre deux ways, auto-intersection) ou
   extrémité de way. Les tags d'une way sont constants et toute frontière de way est un nœud : il n'y a pas d'autre
   « changement de tags en cours de way », et une arête n'appartient qu'à une way (identité stable).
3. **Passe 2, nœuds.** Dichotomie de chaque nœud dans `U` ; coordonnée E7 (`decimicro_lat/lon`, i32) écrite dans
   `coords: Vec<AtomicU64>` (écritures parallèles sans `unsafe`). **Seuls les nœuds utilisés ont une coordonnée en
   mémoire.** Nœud référencé absent de l'extrait : la way est coupée (compté).
4. **Découpe**, sans relire le PBF : arêtes `(way, a, b, attrs, longueur haversine en dm, plage de refs)` ; la
   géométrie est une plage de `refs`, sans copie. Boucle `a = b` coupée à son nœud intérieur médian, promu jonction.
   Identités `(way, nœud a, nœud b)` en double : comptées.
5. **Numérotation et écriture** : jonctions triées par (cellule en ordre de Morton, Morton des coordonnées, id OSM)
   → id global `u32` contigu par cellule ; arêtes par (cellule de `a`, `a`, way, `b`). Écriture dans `<nom>.tmp`,
   en-tête en dernier, `fsync`, relecture et `verify()`, `chmod 0444`, `rename` : jamais de réécriture sur place.

| Moment | Structures dominantes | Région [estimé] | France [estimé, ×12,6 au poids du PBF] |
|---|---|---|---|
| fin passe 1 | `refs` i64, `ways` | ≈ 70 Mo | ≈ 0,95 Go |
| tri de `U` | + copie triée | ≈ 120 Mo | ≈ 1,6 Go |
| fin passe 2 | `U`, `coords`, `refs` u32, `ways` | ≈ 140 Mo | ≈ 1,8 Go |
| découpe | + arêtes (≈ 40 o) | ≈ 220 Mo | ≈ 2,3 Go (**pic**) |

**[Estimation dépassée par la mesure, §9.1 : pic structurel 293 Mo pour la région, soit ≈ 3,7 Go pour la France, et non 2,5 à 3 Go.]**
Cible de **2 Go** non tenue sans les leviers ci-dessous.
Leviers, dans l'ordre : tri radix en place au lieu de la copie, `U` réduit aux jonctions après la passe 2, pré-filtre
`osmium tags-filter` (66 Mo au lieu de 374 **[mesuré]**), tableaux de travail sur fichier temporaire.

## 3. Format de fichier

### 3.1 Cellules : grille fixe de tuiles Web Mercator z11

Cellule = tuile z11 (≈ 13,3 km de côté à 47° N, 178 km² ; zoom en paramètre de `build`) :
1. cellule d'un point en O(1), cellules d'un disque par arithmétique, sans index ;
2. indépendante du graphe : stable d'un extrait à l'autre, alignée sur les tuiles z14 de l'appariement (8 × 8) ;
3. une partition par coupes (CRP, plan §4.4) attend le lot 5 ; l'index étant une table, elle remplacera la grille.

Cellules vides absentes, cellules en ordre de Morton (x, y) : voisines proches sur disque. **Arête à cheval** : jamais coupée au bord (pas de nœud artificiel, identité intacte). L'arête non
orientée appartient à la cellule de `a` (premier nœud dans l'ordre de la way), géométrie entière. L'orientée avant
est dans le CSR de la cellule de `a`, l'arrière dans celui de `b` ; toutes deux portent l'id global d'arête et sont
dans la table de bord de leur cellule. Une arête qui traverse une cellule sans y avoir de nœud ne gêne pas le lot 0.

### 3.2 Disposition

Petit-boutiste ; lecture par `zerocopy` avec `little_endian::U32/U64` (alignement 1) : ni transmutation, ni hypothèse
sur l'hôte. Ids globaux `u32` (France ≈ 26 M orientées **[estimé]**) ; l'id d'arête garde 31 bits, le bit 31 = sens.

| Partie | Contenu | Alignement |
|---|---|---|
| En-tête (256 o) | magie `ORDGRAPH`, `format_major` / `format_minor` u16, `attr_layout` u16, marque `0x01020304`, schéma de cellule (1 = slippy) et zoom, comptes (nœuds, arêtes, orientées, cellules), extrait (horodatage et séquence de réplication, taille et xxh3 du PBF), version de l'outil, date, position de la table des sections, **xxh3 de l'en-tête** | 0 |
| Table des sections | par section : type, drapeaux, position, longueur, xxh3 | 64 |
| `CELL_INDEX` | 64 o par cellule : x, y ; premier et nombre de nœuds, d'arêtes, d'orientées ; arêtes de bord ; position et longueur du bloc et de sa géométrie ; xxh3 du bloc | 64 |
| `CELL_BLOCKS` | un bloc par cellule (§3.3) | 4096 par bloc |
| `GEOMETRY` | par cellule, dans l'ordre des arêtes | 4096 par cellule |
| `ID_NODES`, `ID_EDGES` | id OSM `u64` par nœud, id de way `u64` par arête (froids) | 64 |
| `STATS` | JSON de construction (rejets par motif, durées, RSS), lu par `measure` | 64 |

### 3.3 Bloc de cellule

Tableaux alignés sur 8 o, dans l'ordre : **nœuds** (`lat_e7`, `lon_e7` i32 : 8 o) ; **décalages CSR** (`n + 1` × u32,
relatifs à la première orientée de la cellule) ; **arêtes orientées** (cible u32 = nœud global, arête u32 avec bit de
sens : 8 o), triées par (source, cible, arête), présentes seulement si le sens est ouvert au vélo ; **arêtes
possédées** (`attrs` u64, longueur en dm u32, décalage de géométrie u32 : 16 o) ; **table de bord** (orientée locale
u32, cellule cible u32 : 8 o). Géométrie d'une arête : nombre de points intérieurs, puis deltas E7 zigzag-varint
depuis `a`, sens avant, extrémités exclues.

### 3.4 Versions et intégrité

`format_major` change à toute incompatibilité (refus), `format_minor` à tout ajout ignorable ; `attr_layout` est
vérifié à part. Nom : `<région>-<AAAAMMJJ de réplication>-z<zoom>.ordg`. xxh3-64 (`xxhash-rust`, déjà au workspace)
sur l'en-tête, chaque section, chaque bloc. À l'ouverture : magie, versions, marque, xxh3 de l'en-tête et de l'index,
positions dans la longueur du fichier. Les blocs ne sont vérifiés que par `verify()` (par `build` avant `rename`, par
`measure`, chronométré à part) : une lecture paresseuse ne vérifie pas, elle fauterait tout le bloc.

## 4. Lecteur

`GraphFile<B: AsRef<[u8]>>` lit un `Mmap` (serveur) ou un `Vec<u8>` (tests, WASM). **Résident** : en-tête, table des
sections, index décodé (France ≈ 3 500 cellules × 64 o, moins de 1 Mo), tableaux `premier_nœud` / `première_arête`
par cellule. Le reste est le cache de pages du noyau (chargé à la première lecture d'une cellule, rendu sans GC, borné
par le conteneur). Accès : `cell(i)` (tranches `zerocopy`), `node`, `out`, `edge`, `geometry`, `identity`. Cellule
d'un nœud : « même cellule » en une comparaison, sinon dichotomie sur `premier_nœud` (arêtes de bord seulement).
Implémenté dans `search` (un compte de dichotomies est rapporté : 0,036 par état fixé) ; le tas porte la cellule de la cible, pour ne pas la chercher au retrait.
**Fichier non vérifié** : `open()` ne regarde pas l'intérieur des blocs ; `validate_structure()` (appelée par `verify()`) contrôle décalages CSR, cibles, arêtes, table de bord et géométrie. Sur un fichier corrompu qui n'a pas passé `verify()`, `cell`, `out` et `node_cell` peuvent paniquer (jamais d'accès non sûr : tout est tranche vérifiée) ; `geometry` ne panique pas. Un serveur lit après `verify()`.

**Le seul `unsafe`** : `ord_graph::mmap::map_readonly(&File)`, un appel à `memmap2::Mmap::map`. SAFETY : ouvert en
lecture, jamais modifié sur place (écrit à côté, renommé, `0444`) ; une troncature par un tiers donnerait `SIGBUS`,
hors contrat, documenté pour l'exploitation. Le workspace passe `unsafe_code = "forbid"` à rustc en `-F`, qu'aucun
`allow` ne relâche (**vérifié** : erreur E0453). `ord-graph` ne prend donc pas `[lints] workspace = true` : il recopie
la table avec `unsafe_code = "deny"` et `clippy::undocumented_unsafe_blocks = "deny"` ; seul `mod mmap` porte
`#[allow(unsafe_code)]`. Tout autre `unsafe` du crate échoue à la compilation.

## 5. Protocole de mesure

Linux (`/proc/self`), `release`, machine au repos et décrite (CPU, RAM, disque, `rustc`, commit, md5 du PBF).
**`build`** : durée par phase, temps CPU et mur, fils ; pic RSS par `VmHWM` en fin, `VmRSS` à chaque phase, et la
référence externe `/usr/bin/time -v` (« Maximum resident set size »). Trois constructions, médiane (la première, PBF
froid, à part) ; une fois chacune : z10, z12, PBF pré-filtré par osmium.

**`measure --graph F --starts 50 --seed 20261115 --radii 15,25,50,75 [--poly P] [--state map|dense] --out R.json`** :

1. **Tailles** : fichier, sections, tableaux des blocs, remplissage ; octets par nœud, arête, orientée, km.
   **Comptes** : ways gardées et rejetées par motif ; nœuds par degré (1, 2 « coutures », ≥ 3), dont coutures entre
   attributs identiques ; arêtes, orientées, sens uniques, contresens, arêtes de bord ; arêtes par cellule
   p50/p90/p99/max ; longueur d'arête p50/p90/max ; **km** total, par classe, accès, `service`, itinéraire ; valeurs
   distinctes d'`attrs` ; part de la plus grande composante ; identités en double ; temps d'`open` et de `verify()`.
2. **Départs** : SplitMix64 écrit dans l'outil (pas d'algorithme tiers qui pourrait changer) ; candidats = nœuds de la
   plus grande composante incidents à une arête `residential` ou `tertiary` d'accès `Yes`, triés par id ; 50 tirages
   sans remise, les mêmes pour tous les rayons.
3. **Recherche** : état = arête orientée (distance, prédécesseur), tas binaire à suppression paresseuse ; départ =
   arêtes sortantes du nœud ; demi-tour sur la même arête interdit sauf en cul-de-sac (aucune **autre sortie utilisable** : ni `No`, ni cible hors du disque) ; `No` exclue ; une arête n'est
   poussée que si sa cible est dans le disque (équirectangulaire) ; jusqu'à épuisement du disque (l'« arbre depuis le
   départ », plan §4.3). Poids = longueur × facteur fixe (classe, revêtu, accès) décodé d'`attrs` à chaque relâchement,
   pour être dans le temps mesuré (pas une préférence). État par défaut **`--state dense`** (tableau à estampilles,
   12 o par orientée, alloué une fois) ; `--state map` (`FxHashMap`) pour comparaison. **`--flat`** ajoute la
   **référence à plat** : même recherche sur des tableaux globaux (aucune cellule ni dichotomie), mêmes états
   fixés exigés ; l'écart avec le lecteur est le coût du format.
4. **Par requête** : temps, orientées fixées et poussées, tas max, octets de l'état, orientées du disque (statique),
   km/km² du disque, **cellules touchées** (après coup), défauts majeurs (`/proc/self/stat`), `RssFile`, `RssAnon`.
5. **Froid** : par requête, vue fermée, `posix_fadvise(DONTNEED)` sur le fichier (`rustix`, sûr, sans root),
   réouverture, une exécution ; sans défaut majeur, « froid non garanti ». **Chaud** : un tour jeté, trois tours,
   médiane par départ.
6. **Rapport** par rayon, froid et chaud : médiane, p90, max du temps (ms), des orientées fixées, des cellules
   touchées ; max de `RssFile` (pages vues) et de `RssAnon` (état et tas). Pour tous les départs et pour ceux dont le
   disque est **intérieur** (32 points du cercle dans le `.poly` Geofabrik, quelques Ko) ; moins de 10 intérieurs :
   « peu fiable ». Repères du plan : 180 000 arêtes (≈ 360 000 orientées) à 50 km, « quelques dizaines de ms »,
   0,5 Go de topologie France.

**Extrapolation à la France.** Taille : octets par arête × arêtes France, encadrées par le poids du PBF (×12,6) et la
surface (≈ 544 000 / 32 000 km², ×17). Requête : une recherche bornée dépend de la densité locale, pas de la taille
totale : on transpose le p90 des départs intérieurs lu contre leur km/km². Construction : linéaire, plus un tri.
**Faiblesses** : ni montagne ni densité parisienne (Nantes est le plus dense de l'extrait) ; bords à 75 km ; NVMe
local ; cache de pages partagé non simulé ; un seul fil, sans charge ; poids synthétique, quand les pénalités réelles
changent l'étendue explorée ; mémoire de construction France calculée, pas mesurée (lot 6).

## 6. Tests

Entrées faites à la main par `OsmSource`, plus **un** PBF minuscule commis (< 10 Ko, `osmium cat` d'un `mini.osm`
commis, commande dans `scripts/`) pour le vrai décodage.

1. Filtre : chaque classe, `area=yes`, `construction` ; `access=no` + `bicycle=yes` → `Yes` ; `access=private` →
   `Private` ; `trunk` nu absent, `trunk` + `bicycle=yes` présent ; valeur inconnue ignorée.
2. Empaquetage : aller-retour de chaque champ à son maximum ; `mtb:scale=2+` ; surface inconnue → 31.
3. Contraction : A–B–C–D isolée → une arête, géométrie B, C, longueur somme ; ways bout à bout → deux arêtes, couture
   comptée ; croisement intérieur ; rond-point à une connexion, sans boucle ; auto-intersection.
4. Sens : `oneway=yes`, `-1`, `+ oneway:bicycle=no` (arrière `contraflow`), `cycleway=opposite_lane`,
   `cycleway:left:oneway=-1`, rond-point implicite, `oneway:bicycle=yes` sur double sens. Relations : `rcn`, `ncn`,
   VTT, membre hors graphe, `state=proposed`.
5. Aller-retour écriture / lecture d'un graphe pseudo-aléatoire, par `Vec<u8>` et par `mmap` : coordonnées, voisins,
   attributs, longueurs, géométrie, identités. Octet modifié → `verify()` nomme la cellule ; magie, `format_major`,
   `attr_layout` faux ou fichier tronqué → refus à l'ouverture.
6. Bords : arête entre deux cellules (possession, CSR des deux côtés, deux tables de bord, attributs lus d'une
   cellule voisine) ; nœud sur une limite de tuile ; arête traversant une troisième cellule ; formule de tuile égale
   à `ord_domain::geo::Grid` (dépendance de test seulement).
7. Déterminisme (1 et 4 fils → mêmes octets ; même graine → mêmes départs). Recherche : sans interdiction de
   demi-tour, distances d'un Dijkstra par nœuds de référence ; avec, détour attendu ; rien hors disque ni en `No`.

## 7. Crates (vérifiés le 15 novembre 2026, `cargo info` et `cargo tree`)

| Besoin | Choix | Version, licence | Contre `deny.toml` |
|---|---|---|---|
| PBF | **`osmpbf`** | 0.3.8, MIT OR Apache-2.0, MSRV 1.88 | licences de l'arbre admises (`protobuf` 3.7 MIT, `rayon`, `byteorder`…) ; doublons en **avertissement** (`multiple-versions = "warn"`) : `memmap2` 0.5 à côté de 0.9, `thiserror` 1 à côté de 2 ; `protobuf-codegen` en pur Rust, sans `protoc` |
| écartés | `osmpbfreader` 0.19.1 ; `osm-pbf-reader` 0.1.1 | WTFPL ; MIT | WTFPL **refusée** (hors liste) ; le second trop jeune |
| `mmap` | **`memmap2`** | 0.9.12, MIT OR Apache-2.0 | admis ; seul `Mmap::map` est `unsafe`, `advise` est sûr |
| vues d'octets | **`zerocopy`** (`derive`) | 0.8.63, BSD-2-Clause OR Apache-2.0 OR MIT | déjà au `Cargo.lock` ; 0.9 seulement en alpha |
| sommes, parallélisme | `xxhash-rust` (xxh3) ; `rayon` 1.12 | déjà au workspace ; MIT OR Apache-2.0 | admis |
| froid (outil) | `rustix` (`fs`) | 1.1.6, Apache-2.0 (± LLVM-exception) OR MIT, au `Cargo.lock` | `fadvise` est sûr |

Repli si les doublons gênent : décoder le PBF avec `prost` et `flate2` (déjà au workspace), ≈ 300 lignes, hors lot 0.
`cargo deny` n'est pas installé sur la machine de conception : le job `deny` de la CI tranche.

## 8. Points de tension

- **Sur-ensemble** : `No`, `Private`, `parking_aisle` coûtent de la taille ; les km par état et `service` le diront.
- **Grille contre densité** : si les cellules touchées à 50 km dépassent la centaine, ou si le p99 d'arêtes par
  cellule écrase le reste, la partition par coupes remonte avant le lot 5.
- **Froid honnête** : `DONTNEED` ne chasse pas une page projetée ailleurs (d'où le contrôle des défauts majeurs) ; le
  froid sur NVMe est optimiste. **Identités en double** et arêtes traversant une cellule sans nœud : comptées ici,
  traitées aux lots 1 et 2.
- Résultats : §9, écarts à la conception : §10 (rapport JSON de `measure` hors dépôt, régénérable).

## 9. Résultats du lot 0 (15 novembre 2026)

> Tout est **[mesuré]** sauf mention. Machine : AMD Ryzen 7 4800H, 16 fils, 31 Go, `rustc 1.97.0`, Linux 7.0, `release`.
> Extrait : Pays de la Loire du 2026-11-14 (md5 vérifié), fichier `pays-de-la-loire-20261114-z11.ordg`. Le rapport
> JSON complet de `measure` est dans `~/.cache/osm-route-details/graph/measure-pdl-z11.json` (hors dépôt, régénérable).
> **Relevé de référence** : [ord-graph-lot0-mesures.md](ord-graph-lot0-mesures.md) (binaire actuel, trois passes par
> recherche, rapports dans `~/.cache/osm-route-details/graph/lot0-runs/`, extrapolation France et coût de l'état dense) ;
> en cas d'écart avec cette section, il prime.

### 9.1 Construction

| Mesure | Résultat |
|---|---|
| 3 constructions z11, 16 fils, PBF en cache de pages | 4,76 / 4,74 / 4,66 s mur (médiane **4,7 s**), ≈ 20 s de CPU |
| Pic RSS (`/usr/bin/time -v`) sur ces trois | 434 / 419 / 448 Mo (médiane **434 Mo**) ; `VmHWM` interne 419 Mo |
| 1 fil | 14,6 s, 328 Mo |
| z10, z12 (une fois) | 4,7 s, 442 Mo et 427 Mo |
| Phases (première construction, mur) | ouverture + xxh3 du PBF 0,05 s ; passe 1 (ways, relations) 2,78 s ; nœuds utilisés 0,19 s ; passe 2 (nœuds) 0,55 s ; découpe 0,42 s ; sommets et tri 0,28 s ; écriture, relecture, `verify()` 0,51 s |
| Déterminisme | 1 fil contre 16 fils : **124 541 750 octets identiques** (tout sauf l'en-tête de 4 Kio et `STATS`) ; deux tests le gardent : `output_does_not_depend_on_thread_count` (source en mémoire, tri et tableaux parallèles) et `pbf_output_does_not_depend_on_threads_or_batch_size` (vrai décodage parallèle du PBF commis, 1 à 4 fils, lots de 1 à 3 blobs ; ce PBF n'a que trois blobs, la preuve sur la région reste la comparaison ci-dessus) |

**Le pic est le double de l'estimation de §2 (220 Mo), et la part « structurelle » est elle-même au-dessus.** Deux effets
distincts, séparés par une construction à **1 fil** (`VmHWM` par phase) :

| Phase | 1 fil | 16 fils |
|---|---|---|
| passe 1 (ways, relations) | 112 Mo | 323 Mo |
| nœuds utilisés (tri, jonctions) | 196 Mo | 323 Mo |
| découpe en arêtes | 208 Mo | 323 Mo |
| **sommets, cellules, tri des arêtes** | **293 Mo** | **388 Mo** |
| écriture, relecture, `verify()` | 328 Mo | 419 Mo |

- **Pic structurel : ≈ 293 Mo à 1 fil**, atteint dans la phase « sommets, cellules, tri des arêtes » : le tableau `verts`
  (tuples de 32 o par jonction) et `gid_of` (4 o par nœud utilisé) coexistent avec `raw` et `edges` (≈ 40 o par arête),
  qui ne sont libérés qu'après. §2 prévoyait 220 Mo en fin de découpe et ne comptait pas cette phase : l'estimation
  était fausse, pas seulement l'implémentation.
- **Surcoût dépendant des fils : ≈ 100 Mo** (323 contre 112 Mo dès la passe 1) : lots de 4 blobs par fil décodés d'un coup
  (dont les blobs de nœuds, que la passe 1 décode aussi) et arènes de l'allocateur par fil. Un lot plus petit le réduit ;
  il ne grossit pas avec le graphe.
- **Relecture de `verify()`** : +35 Mo (1 fil), le fichier relu entier dans un `Vec` ; une relecture par `mmap` le retire.

France **non mesurée** : base retenue = le pic **structurel mesuré** (293 Mo × 12,6 ≈ **3,7 Go**), non l'estimation de §2
(2,3 à 3 Go, dépassée). Avec les surcoûts actuels (16 fils, relecture) la même règle donne 5,3 Go (borne haute : les lots
ne grossissent pas avec le graphe, donc la vraie valeur est entre les deux). L'objectif de 2 Go **n'est pas tenu** sans
levier : libérer `raw` avant les sommets, tri radix en place, `U` réduit aux jonctions (§2) et pré-filtre `osmium`.

### 9.2 Taille et contenu

| | Valeur |
|---|---|
| Fichier | **124,5 Mo** = `CELL_BLOCKS` 67,9 (topologie) + `GEOMETRY` 19,9 + `ID_EDGES` 26,5 + `ID_NODES` 10,3 |
| Nœuds, arêtes, orientées, cellules (z11) | 1 290 856 ; 1 653 822 ; 3 145 589 ; 245 (75 en z10, 870 en z12) |
| Octets par nœud / arête / orientée / km | 96,5 / 75,3 / 39,6 / 754 ; **topologie seule 40,7 par arête** |
| Longueur | 165 290 km (accès `Yes` 157 380 ; `Private` 5 809 ; `No` 1 618 ; `Destination` 484) : le sur-ensemble coûte ≈ 5 % |
| Ways | 5 411 428 lues, **843 626 gardées** ; rejets : sans `highway` 4 555 245, `motorway`/`trunk` fermées 8 693, `area=yes` 1 718, `construction` 972, `proposed` 136 |
| Classes (km) | `track` 38 155, `unclassified` 40 445, `service` 20 541, `residential` 16 758, `tertiary` 15 830, `path` 11 129, `footway` 8 773, `secondary` 7 401 |
| Sens | 162 055 arêtes à sens unique, 8 974 contresens vélo, 0 arête fermée aux deux sens |
| Degrés | 1 : 260 532 ; 2 (coutures) : 177 834 dont 77 492 entre attributs identiques ; ≥ 3 : 852 490 |
| Arêtes par cellule (z11) | p50 4 450, p90 13 245, p99 57 216, **max 102 741** (z10 : max 188 930 ; z12 : 57 118) |
| Longueur d'arête | p50 43,6 m, p90 263 m, p99 774 m, max 5,6 km |
| `attrs` distincts | **8 228** (tient sur 16 bits : un dictionnaire `u16` gagnerait 6 o par arête, soit ≈ 10 Mo ici) |
| Plus grande composante | 98,6 % des nœuds |
| Arêtes de bord (z11) | 14 888 (0,9 %) ; 459 arêtes dont un point intérieur sort des tuiles de leurs deux extrémités ; 3 identités en double ; 5 550 boucles coupées ; 0 way coupée par nœud manquant |
| Relations | 550 relations de route, 41 569 membres appariés, 5 800 membres hors graphe ; 12 380 ways avec `motor_vehicle` ignoré, 107 avec `*:conditional` ignoré |
| `open` / `verify()` / comptes | 0,05 ms / 50 ms (cache chaud, contrôle de structure compris ; 8 ms sans) / 0,85 s |

### 9.3 Recherche bornée (50 départs, graine 20261115, `forbid_uturn`, **état dense = référence**)

Médiane / p90 en ms ; « intérieurs » = disque entièrement dans le `.poly` de la région. Le froid est une vue neuve après
`posix_fadvise(DONTNEED)` ; **chaque** requête froide a eu des défauts majeurs (18, 35, 94, 161 en médiane à 15 / 25 / 50 /
75 km). Les chiffres de cette section remplacent ceux d'une première version, qui avait deux défauts de mesure
(corrigés après relecture indépendante) : le lecteur faisait **trois dichotomies par relâchement** (`edge_cell`, `node_cell`
de la cible, `node_cell` au retrait du tas) au lieu du raccourci « même cellule » de §4, et le tableau dense était alloué
dans la zone chronométrée du froid. Le rapport de `measure --flat` mesure maintenant le coût du lecteur contre une
**référence à plat** (mêmes algorithme, états et poids ; tableaux globaux, aucune cellule), et exige les mêmes états fixés.

| Rayon | Départs intérieurs | **À plat**, tous | **Lecteur par cellules**, tous (dense) | Lecteur / plat | Lecteur, intérieurs | Lecteur, état en table de hachage, tous | Froid (dense), tous | États fixés (médiane) | Cellules (méd. / p90) | `RssFile` froid max |
|---|---|---|---|---|---|---|---|---|---|---|
| 15 km | 36 | 9,8 / 52 | 11,5 / 63 | 1,21 | 18,8 / 65 | 15,1 / 96 | 18,8 / 78 | 85 523 | 9 / 10 | 14 Mo |
| 25 km | 28 | 21 / 67 | 26 / 83 | 1,20 | 61 / 84 | 39,6 / 155 | 44,6 / 130 | 184 768 | 19 / 21 | 18 Mo |
| 50 km | 8 (**peu fiable**) | 107 / 171 | 128 / 207 | 1,20 | 183 / 236 | 198 / 363 | 158 / 232 | 727 989 | 58 / 61 | 30 Mo |
| 75 km | **0** (aucun) | 187 / 263 | 222 / 319 | 1,19 | — | 400 / 594 | 293 / 411 | 1 398 534 | 101 / 113 | 47 Mo |

- **Contenu du disque** (médiane) : 45 548 / 99 483 / 377 866 / 731 401 arêtes, soit 85 492 / 187 567 / 716 726 / 1 388 975 orientées, à 15 / 25 / 50 / 75 km ; 3 810 / 10 110 / 35 990 / 69 205 km de voies ; densité 5,4 / 5,2 / 4,6 / 3,9 km/km². **Les orientées du disque sont presque toutes fixées** (fixées / orientées = 0,985) : l'arbre est un parcours exhaustif du disque.
- **Où va le temps** (50 km, médiane) : **107 ms à plat** (≈ 0,14 µs par état fixé, tas binaire et décodage du poids à chaque relâchement compris), **+20 % pour le lecteur par cellules** (128 ms ; la dichotomie n'est plus faite que pour 0,036 par état fixé, aux arêtes de bord), **+31 % à +80 % pour la table de hachage** à la place du tableau dense (198 ms à 50 km, +55 % ; 1,6 à 2,2 fois le plat selon le rayon). La première version (table de hachage, trois dichotomies par relâchement) donnait 199 ms et attribuait tout au calcul : en réalité **≈ 45 % de ces 199 ms étaient du lecteur et de la table de hachage**, pas du Dijkstra. Le coût du Dijkstra lui-même est ≈ 0,12 à 0,14 µs par état, soit **proportionnel au nombre d'états**.
- **Froid** : +27 % à +66 % sur le chaud selon le rayon (rapport des médianes par départ : 1,56 / 1,66 / 1,27 / 1,34), sur NVMe local, dans le tableau dense déjà alloué (un serveur l'alloue une fois). Les défauts de page pèsent plus aux petits rayons, où le calcul est court.
- **États poussés / fixés = 1,00 est structurel, pas une chance.** Sans coût de virage, le poids d'une arête orientée ne dépend que d'elle ; Dijkstra fixe ses prédécesseurs par distance croissante, donc **la première relaxation d'une orientée vient toujours de son prédécesseur le moins cher** et rien ne la réduira ensuite : l'entrée du tas n'est jamais périmée. Le chiffre ne dit donc rien de la suppression paresseuse ; il cessera de valoir dès qu'un coût de virage ou une pénalité dépendant du prédécesseur apparaîtra (lot 3).
- **Ce que coûte l'état par arêtes orientées** : il multiplie les états par ≈ 2,4 par rapport aux nœuds (orientées / nœuds = 3 145 589 / 1 290 856 = 2,44 sur le graphe) **[estimé, non mesuré]** : un Dijkstra par nœuds ferait ≈ 2,4 fois moins d'états et de relaxations pour les mêmes distances (le test `edge_search_without_uturn_ban_equals_node_dijkstra` vérifie l'égalité des distances), mais ne sait ni interdire le demi-tour ni porter un coût de virage. Le mesurer est un point de départ du lot 3, pas du lot 0.
- À 75 km, un départ froid fait 161 défauts majeurs en médiane (max 210) : **la topologie lue à 75 km tient en ≈ 47 Mo de pages** (101 cellules sur 245), loin de la taille du fichier.
- **Écart avec la première version, hors lecteur** : le demi-tour ne compte plus comme « autre sortie » les arêtes fermées (`No`) ou hors du disque ; les états fixés changent de ≈ 0,1 % (727 057 → 727 989 à 50 km).

### 9.4 Lecture pour la suite du plan

1. **Taille** : 40,7 o de topologie par arête mesurés, soit **≈ 0,85 Go pour la France** (×12,6 sur les arêtes, hypothèse de §5) contre 0,5 Go au plan ; avec géométrie et identités, ≈ 1,6 Go de fichier. Le dictionnaire d'`attrs` (≈ −10 Mo ici) et un tri des identités hors du chemin chaud sont les leviers évidents ; rien n'est bloquant.
2. **Latence** : un arbre complet à 50 km coûte **≈ 107 ms à plat** (référence), **≈ 128 ms avec le lecteur par cellules** (+20 %) et ≈ 200 ms avec une table de hachage, pour « quelques dizaines de ms » au plan : **facteur 2 à 4 sur la référence, et non 5 à 10** (la première version comptait le lecteur et la table de hachage comme du Dijkstra). Le coût est **proportionnel aux états** (≈ 0,14 µs par état fixé, à plat) ; l'écart au plan vient donc du **nombre d'arêtes** (§9.4 point 4), pas d'un Dijkstra lent : à 180 000 arêtes, soit ≈ 340 000 orientées, la même recherche coûterait ≈ 50 ms, « quelques dizaines de ms » au bas de la fourchette. L'arbre par requête n'est tenable que si l'espace des états diminue (arêtes plus longues, rayon, états par nœuds) : **c'est le résultat qui pèse sur les lots 3 et 4**, plus que la taille. Le lecteur par cellules, lui, ne pèse que 20 % : le format n'est pas en cause.
3. **Grille z11** : 58 cellules à 50 km, max 102 741 arêtes dans une cellule (Nantes), p99 57 216 : le déséquilibre est réel (rapport max/p50 = 23) mais les blocs restent de 7 Mo ; pas de raison de faire remonter la partition par coupes avant le lot 5.
4. **Le repère de 180 000 arêtes est dépassé de 2,1 fois (378 000 médiane à 50 km) par la finesse du graphe, non par la surface ni la densité.** Le disque à 50 km porte **35 990 km** de voies (médiane) contre 39 000 km supposés au plan, à 4,6 km/km² contre 5 : la **longueur est au repère, légèrement en dessous**. Le surplus d'arêtes vient de leur longueur : **91 m par arête** dans le disque médian (99,9 m sur tout le graphe : 165 290 km / 1 653 822 arêtes) contre 216 m au plan (39 000 km / 180 000). Les leviers sont donc des arêtes à retirer ou à fondre, pas une région moins étendue. En arêtes (sur 1 653 822) :

   | Levier | Arêtes | Part |
   |---|---|---|
   | coutures entre attributs identiques (degré 2), à fondre | 77 492 | 4,7 % |
   | `service=driveway` 89 677, `parking_aisle` 82 987, `drive-through` / `emergency_access` 1 032, à écarter ou replier | 173 696 | 10,5 % |
   | `footway` (traversées, trottoirs : 33 m par arête en moyenne), à replier | 263 161 | 15,9 % |
   | `service` hors des trois sous-types ci-dessus (≈ 182 000) et `track` (163 643) | non chiffrés ici | — |

   Coutures et allées ensemble retirent ≈ 251 000 arêtes (15 %) : **ne ramènent pas** le graphe au repère (il faudrait en retirer la moitié). Le premier gain est dans la fusion de coutures ; le second, dans le repliement des `footway` courts (trottoirs, traversées) qui ne servent pas la génération. **Extrapolation à la France** : reprise sur les arêtes (×12,6 sur le poids du PBF, §5) : 1 653 822 × 12,6 ≈ **20,8 M d'arêtes** ; ×17 sur la surface donne 28 M comme borne haute ; l'extrapolation « longueur ÷ longueur par arête » redonne le même nombre, car les km sont mis à l'échelle du PBF de la même façon.
5. **Mesure incomplète à 50 et 75 km** : la région est trop petite pour des disques intérieurs (8 à 50 km, 0 à 75 km). Les p90 « tous » incluent des disques tronqués par la mer ou la frontière : ils sont **optimistes** pour la France. Une région plus large (Bretagne + Pays de la Loire + Centre-Val de Loire) est nécessaire pour trancher à 75 km.

## 10. Écarts à la conception

Ce qui a été fait autrement que décrit plus haut, et ce qui n'a pas été fait.

**Écarts de format et de construction**

1. **`ID_EDGES` : 16 o par arête** (`way` u64, `a` u32, `b` u32) au lieu de la seule way (§3.2). L'enregistrement possédé (§3.3) n'a pas les extrémités et une arête fermée aux deux sens n'a aucune orientée : l'identité `(way, nœud a, nœud b)` et la géométrie (qui part de `a`) en ont besoin.
2. **Table de bord** (§3.1) : elle liste les arêtes orientées **sortant** de la cellule (cible dans une autre). Une arête à cheval ouverte dans les deux sens est bien dans les deux tables ; une arête à sens unique n'est que dans celle de sa source.
3. **Octets identiques quel que soit le nombre de fils** (§2) : vrai pour tout sauf l'en-tête (date de construction, table des sections) et `STATS` (durées, RSS, nombre de fils). Le test et la vérification sur la région comparent les sections hors `STATS` (`GraphFile::section_digests`).
4. **Sommes de contrôle** (§3.4) : `CELL_BLOCKS` n'a pas de somme de section (drapeau « une par entrée », somme du bloc dans l'index) ; `GEOMETRY` a une somme pour toute la section, pas par cellule : `verify()` nomme la cellule d'un bloc corrompu, pas celle d'une géométrie. Ordre des sections dans le fichier : en-tête, blocs, géométrie, index, identifiants, `STATS` (la table de §3.2 donne les types, pas l'ordre).
5. **Coordonnées de tuile sur 16 bits** dans l'index (64 o par entrée) : zoom borné à 1..=16, refusé sinon.
6. **Table de `surface`** : les 30 valeurs sont fixées de mémoire, **pas triées par fréquence sur l'extrait** (§1 le disait). `grass_paver` est la dernière ; `unhewn_cobblestone` compte comme non revêtu dans `is_unpaved` (approximation de T03 §7, qui n'a pas été relue).
7. **Features** : `ord-graph` a `pbf` (qui implique `parallel`, donc `rayon`) et `mmap`, **actives par défaut** ; la dépendance du workspace les désactive (`default-features = false`) et l'outil les réactive. `cargo check --no-default-features` compile (modules `pbf` et `mmap` absents) ; la cible `wasm32` n'est pas installée sur la machine et n'a **pas** été essayée.
8. **Passe 1** : les blobs de nœuds sont décodés aussi (le format ne dit pas le type d'un blob sans le décompresser) ; la passe 2 s'arrête au premier lot qui contient des ways.
9. **Filtre** : 843 626 ways gardées, pas 854 036 (§0) : ce dernier comptait avant le rejet des `motorway`/`trunk` fermées (8 693) et des `area=yes`.

**Écarts de mesure**

10. **Froid** : une première version gardait la vue du graphe ouverte pendant le froid, `DONTNEED` n'évinçait rien et le contrôle des défauts majeurs l'a signalé (50 départs sur 50 « froid non garanti »). `measure` ferme maintenant toute vue avant la phase froide et la fait pour tous les rayons avant la phase chaude. Les rapports z10 et z12 de §9 ont été écrits dans un `tmpfs` (`/tmp`) : leur « froid » **n'est pas froid** et n'est pas rapporté ; seuls leurs temps chauds valent.
11. **`RssAnon` en chaud** n'est pas isolé par requête (le même processus a déjà alloué les tables des rayons précédents) ; seul `RssAnon` en froid est lisible, et encore sans processus neuf.
12. **Temps CPU** lu dans `/proc/self/stat` en supposant `CLK_TCK` = 100 (Linux).

**Ajouts après relecture indépendante**

13. **Raccourci « même cellule »** de §4 : absent de la première version (trois dichotomies par relâchement), mis en place ; le tas porte la cellule de la cible.
14. **Référence à plat** (`measure --flat`) pour séparer le coût du Dijkstra de celui du lecteur ; état dense par défaut (`--state map` pour comparaison) ; le tableau dense du froid est alloué hors de la zone chronométrée.
15. **Demi-tour en cul-de-sac** : « autre sortie » = sortie utilisable (accès non `No`, cible dans le disque) ; la première version comptait les sorties fermées ou hors du disque.
16. **Contrôle de structure** (`validate_structure`, appelé par `verify()`) ; `verify()` passe de 8 à ≈ 50 ms sur la région.
17. **`PbfSource::with_batch_len`** : réglage de la taille des lots de blobs, pour que le test de déterminisme traverse des frontières de lot.
18. **Rapports JSON** : `measure-pdl-z11-dense.json` et `measure-pdl-z11-map.json` (hors dépôt ; mêmes graine et départs que la première version, `measure-pdl-z11.json`).

**Non fait**

- La première construction « PBF froid » à part (§5) : le PBF venait d'être téléchargé, donc en cache de pages. Les trois mesures de §9.1 sont à PBF chaud.
- La variante « PBF pré-filtré par `osmium tags-filter` » (§5) et la mesure de `measure` en z10/z12 avec 50 départs (20 seulement, chaud seul).
- `cargo deny` (non installé) : le job CI tranche, comme annoncé en §7. La durée de 3 constructions n'a pas été prise sur machine « au repos » au sens strict (navigation et agents tournaient en parallèle).
- La mémoire de construction **France** : extrapolée, jamais mesurée.
