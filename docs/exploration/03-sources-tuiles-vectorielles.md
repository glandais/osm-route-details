# 03 — Sources de tuiles vectorielles OSM libres (revêtement + pont/tunnel)

> Exploration, octobre 2026. Objectif : savoir quelle source de tuiles vectorielles OSM
> **libre et réutilisable par un service** permet de qualifier, pour chaque segment d'un
> parcours GPS, la **structure** (pont / tunnel / gué / normal) et le **revêtement**
> (asphalte, pavés, gravier, terre…), sans base OSM locale.
>
> Méthode : docs et specs officielles et code source des générateurs, plus un **contrôle
> empirique** : une poignée de tuiles z14 décodées le 30/10/2026 (forêt de Fontainebleau
> 14/8312/5668, Paris 14/8299/5636, Nantes 14/8121/5748, Annecy 14/8470/5835) sur
> VersaTiles, vector.openstreetmap.org et OpenFreeMap. Le contrôle a été fait avec
> `mapbox-vector-tile` (Python) et n'a porté que sur quelques tuiles, pas en masse.

## 1. Résumé

- **Aucune tuile « grand public » ne fournit aujourd'hui la valeur brute de `surface`
  sur les routes**, contrairement à ce que laissent croire certaines specs :
  - le schéma **Shortbread** spécifie `surface` = « value of surface=* » (brut). Mais
    l'instance **OSMF** ne renvoie **aucun** `surface` sur `streets`, et **VersaTiles**
    le renvoie **regroupé en `paved`/`unpaved`** (constaté) ;
  - le schéma **OpenMapTiles** (OpenFreeMap, MapTiler) regroupe explicitement en
    `paved`/`unpaved` ;
  - **Protomaps** n'a aucun attribut `surface` sur `roads`.
- Pont/tunnel est disponible partout (z11–z12+). Le **gué (`ford`)** n'existe que dans
  **OpenMapTiles** (`brunnel=ford`). `layer` brut n'est exposé que dans OpenMapTiles.
- **Recommandation** : en source primaire, **générer ses propres tuiles** (Planetiler
  avec un schéma YAML sur mesure, ou tilemaker) à partir du `.osm.pbf`, en z14 et
  avec `surface`, `smoothness`, `tracktype`, `bridge`, `tunnel`, `ford`, `layer`,
  `highway` et `osm_id` en brut. En repli immédiat, prendre le planet **OpenFreeMap**
  (OpenMapTiles, hebdo, `brunnel` + `ford` + `layer` + `paved/unpaved`) ou le planet
  **VersaTiles**.
  Voir §6.

## 2. Tableau comparatif

| | **VersaTiles** (`osm.versatiles`) | **OSMF** vector.openstreetmap.org | **OpenFreeMap** | **Protomaps** basemap v4 | **OpenMapTiles self-host / MapTiler** | **Tuiles maison** (Planetiler / tilemaker) |
|---|---|---|---|---|---|---|
| Schéma | Shortbread (tiles.json : `shortbread@1.0` ; docs : 1.1) + extensions VersaTiles | Shortbread v1 (`shortbread_v1`) | OpenMapTiles, non modifié | Protomaps basemap v4 | OpenMapTiles | Libre (YAML / Lua) |
| Générateur | Planetiler + profil Shortbread Java | Tilekiln + osm2pgsql-themepark (PostGIS) | Planetiler (profil OMT) | Planetiler (profil Java protomaps) | OpenMapTiles (imposm + PostGIS) ou Planetiler | Planetiler / tilemaker |
| Zoom max | 14 | 14 | 14 | 15 | 14 | Au choix (14 conseillé) |
| Couche routes | `streets` (+ `street_polygons`, `bridges`) | `streets` (+ `street_polygons`, `bridges`) | `transportation` | `roads` | `transportation` | Au choix |
| Chemins / pistes / cyclables | track, path, footway, cycleway, steps dès z13 | idem (z13) | `class=path/track`, `subclass` = highway (footway, cycleway…) | path z12, footway/cycleway/steps z13, sidewalk z14 | idem OpenFreeMap | Tout |
| `surface` | **Constaté : `paved`/`unpaved` seulement** (spec : brut) | **Absent des tuiles** (déclaré dans tilejson ; brut seulement sur `street_polygons`) | `paved`/`unpaved` (liste fixe) | **Absent** | `paved`/`unpaved` | **Brut** |
| Pont / tunnel | `bridge`, `tunnel` booléens z11+ | `bridge`, `tunnel` booléens z11+ | `brunnel` = bridge/tunnel/ford | `is_bridge`, `is_tunnel` z12+ | `brunnel` | Brut |
| Gué (`ford`) | Non | Non | **Oui** (`brunnel=ford`) | Non | Oui | Oui |
| `layer` | Non (sert seulement au tri) | Non (sert seulement au tri) | **Oui** (brut) | Non (`sort_rank`) | Oui | Oui |
| `tracktype` | Oui (brut, z11+) | Champ présent mais **valeurs incohérentes** (`yes`/`no` sur des `residential`) | Non | Non | Non | Oui |
| `smoothness` | Non | Non | Non | Non | Non | Oui |
| Classe highway détaillée | `kind` = highway brut | `kind` = highway brut | `class` regroupé + `subclass` (paths) | `kind` + `kind_detail` (≈ highway) | `class` + `subclass` | Brut |
| Accès / usages | `bicycle`, `horse` (z14) | `bicycle`, `foot`, `horse`, `motorcar` normalisés yes/limited/no (z13) | `bicycle`, `foot`, `horse`, `mtb_scale`, `access` | `access` (z15) | idem OFM | Brut |
| Généralisation au zoom max | Fusion des lignes à attributs identiques (Paris : 116 entités vs 2842 chez OSMF) | Faible (1 entité ≈ 1 way) | Fusion OMT (`update_transportation_merge`) | `mergeLineStrings` à tous les zooms (tolérance 0,1 px) | Fusion OMT | Paramétrable (`tolerance_at_max_zoom`, pas de merge) |
| Accès | Fichier planet `.versatiles` (aussi `.pmtiles`, `.mbtiles`) **62,1 Go** ; serveur Rust `versatiles serve` ; HTTP z/x/y public | HTTP z/x/y uniquement (`/shortbread_v1/{z}/{x}/{y}.mvt`) | Planet MBTiles **103 Go**, PMTiles **87 Go**, image Btrfs ; HTTP z/x/y public | Planet PMTiles **139 Go** (z0–15), range requests, `pmtiles extract` | MBTiles (commercial chez MapTiler) ou génération | Fichier local (MBTiles/PMTiles) |
| Mise à jour | Planet tous les 2 à 4 mois (20261027, 20260608, 20260413…) | Continue (Tilekiln, base osm2pgsql mise à jour) | Planet hebdomadaire | Build quotidien | Selon fournisseur | Selon notre cron |
| Licence données | ODbL (OSM) | ODbL | ODbL + attribution OpenMapTiles (CC-BY 4.0) ; code MIT | ODbL Produced Work ; code BSD-3 | ODbL + CC-BY OpenMapTiles | ODbL |
| Usage serveur / bulk | Téléchargement du fichier prévu et autorisé : self-host OK | **Bulk interdit**, proxy-cache déconseillé, best effort | Planet téléchargeable : self-host OK ; API publique : collecte automatisée **interdite** (sauf offline ≤ 3000 tuiles / 50 Mo) | Téléchargement OK, hotlink déconseillé (copier dans son stockage) | Self-host libre (OMT) ; données MapTiler sous licence | Aucune contrainte |

## 3. Détail par source

### 3.1 VersaTiles

- **Données** : [download.versatiles.org](https://download.versatiles.org/). `osm.versatiles`
  pèse 62,1 Go, z0–14, schéma Shortbread, ODbL. Il existe aussi des versions datées :
  20261027, 20260608, 20260413, 20260105, 20251006… soit **une release tous les 2 à
  4 mois**. Le fichier est aussi téléchargeable en `.pmtiles` / `.mbtiles` / `.tar`
  via `versatiles convert`.
  La page précise « use this download service only to download these files ». Le
  téléchargement complet est donc l'usage prévu.
- **Génération** : Planetiler + profil Shortbread Java ([extensions VersaTiles](https://docs.versatiles.org/compendium/specification_shortbread_extensions.html)).
  Le [tiles.json public](https://tiles.versatiles.org/tiles/osm/tiles.json) annonce
  `shortbread@1.0` et Planetiler 0.10.3-SNAPSHOT. Les extensions n'ajoutent rien d'utile
  ici côté `streets` : `link` dès z5 et `service` dès z10.
- **Serveur** : CLI Rust `versatiles` (convert / serve), serveur Node, images Docker
  ([versatiles.org](https://versatiles.org/)). Le format `.versatiles` est lisible en
  local ou par range HTTP. Un client Rust existe donc nativement, ce qui est intéressant
  pour un microservice Rust.
- **Attributs `streets`** (spec [Shortbread 1.0](https://shortbread-tiles.org/schema/1.0/) /
  [1.1](https://shortbread-tiles.org/schema/1.1/)) : `kind` (= highway / railway /
  aeroway brut), `bridge` et `tunnel` booléens z11+, `tracktype` z11+, `surface` z11+,
  `service`, `bicycle`, `horse`. La spec 1.1 ajoute aussi `foot` et `motorcar`.
  `bridge=true` couvre aussi `low_water_crossing` (passage à gué bétonné).
  `tunnel=true` couvre aussi `building_passage` et `covered=yes`. Pas de `ford`, pas
  de `layer`, pas de `smoothness`.
- **Point clé `surface`** : la spec dit « value of surface=* ». **Dans les tuiles
  réelles, on n'observe que `paved` et `unpaved`.** Sur 4 tuiles, aucune valeur brute
  (`asphalt`, `sett`, `gravel`…) n'apparaît, alors que les mêmes tuiles chez OSMF
  (`street_polygons`) contiennent `paving_stones`, `sett`, `asphalt`, `compacted`.
  Le profil Planetiler de VersaTiles normalise donc le revêtement. À noter : le
  [sample YAML Shortbread de Planetiler](https://github.com/onthegomap/planetiler/blob/main/planetiler-custommap/src/main/resources/samples/shortbread.yml)
  porte le commentaire `- key: surface # TODO canonicalize?`.
- **Généralisation** : à z14, les lignes aux attributs identiques sont fusionnées. Sur
  la tuile de Paris, on compte 116 entités `streets` contre 2842 chez OSMF. Ce n'est pas
  gênant pour du map-matching, mais il n'y a pas d'id de way OSM.
- **Repo générateur** `versatiles-generator` archivé le 22/06/2026
  ([GitHub](https://github.com/versatiles-org/versatiles-generator)). La génération passe
  désormais par l'image Docker `versatiles-planetiler`. Il y a un léger risque sur la
  pérennité du pipeline.

### 3.2 OSMF — vector.openstreetmap.org

- **Déploiement** : juillet 2025 ([blog OSM](https://blog.openstreetmap.org/2025/07/22/vector-tiles-are-deployed-on-openstreetmap-org)).
  Génération par [Tilekiln](https://github.com/pnorman/tilekiln) depuis PostGIS, avec
  le thème Shortbread de [osm2pgsql-themepark](https://github.com/osm2pgsql-dev/osm2pgsql-themepark).
- **TileJSON** ([shortbread_v1](https://vector.openstreetmap.org/shortbread_v1/tilejson.json)) :
  z0–14. `streets` déclare `bicycle, bridge, foot, horse, kind, link, motorcar, oneway,
  oneway_reverse, rail, service, surface, tracktype, tunnel`.
- **Code themepark** (`themes/shortbread_v1/topics/streets.lua`) : `a.surface = t.surface`
  (brut, sans filtre). `tracktype` n'est rempli que pour `kind == 'track'`. Les accès sont
  normalisés en yes / limited / no.
- **Constat empirique** : sur les 4 tuiles testées (8445 entités `streets`), **aucune
  n'a de `surface`**. Le champ `tracktype` vaut `yes`/`no` sur des `residential`,
  `primary`, `cycleway`… Cela ne correspond ni à la spec ni au code. Il s'agit
  vraisemblablement d'un décalage de colonnes ou d'un bug de déploiement. Seul
  `street_polygons` porte un `surface` brut. **Source inutilisable en l'état pour le
  revêtement** (à re-vérifier plus tard).
- **Politique d'usage** ([Vector Tile Usage Policy](https://operations.osmfoundation.org/policies/vector/)) :
  - « Bulk downloading ("scraping") is the downloading of tiles in advance » : **interdit** ;
  - User-Agent identifiant obligatoire ;
  - cache local d'au moins 7 jours ;
  - proxy-cache « not generally recommended » ;
  - aucun SLA, blocage possible sans préavis.

  **Conclusion** : inadapté comme backend d'un service serveur. Utile au mieux comme
  référence de comparaison ponctuelle.
- **Avantage unique** : la fraîcheur. La base est mise à jour en continu, donc les tuiles
  sont proches du temps réel.

### 3.3 OpenFreeMap

- **Offre** ([openfreemap.org](https://openfreemap.org/)) : schéma OpenMapTiles « unmodified »,
  planet **hebdomadaire** en MBTiles et Btrfs (et PMTiles). Code MIT.
  Génération : Planetiler ([GitHub](https://github.com/hyperknot/openfreemap)).
- **Tailles mesurées** (version `20261025_080012_pt`, HEAD HTTP) :
  - `tiles.mbtiles` : 103,3 Go ;
  - `tiles.pmtiles` : 87,2 Go.

  URL : `https://btrfs.openfreemap.com/areas/planet/{version}/tiles.{mbtiles,pmtiles,btrfs}`,
  index dans `files.txt`. Rétention : les 4 derniers runs, plus le premier run de chacun
  des 6 derniers mois.
- **TileJSON** : `transportation` en z4–14, champs `access, bicycle, brunnel, class,
  expressway, foot, horse, indoor, layer, level, mtb_scale, network, official, oneway,
  ramp, service, subclass, surface, toll`.
- **`surface`** ([schéma OMT](https://openmaptiles.org/schema/),
  [`class.sql`](https://github.com/openmaptiles/openmaptiles/blob/master/layers/transportation/class.sql)) :
  - `paved` regroupe paved, asphalt, cobblestone, concrete, concrete:lanes,
    concrete:plates, metal, paving_stones, sett, unhewn_cobblestone, wood, grade1 ;
  - `unpaved` regroupe unpaved, compacted, dirt, earth, fine_gravel, grass, grass_paver,
    gravel, gravel_turf, ground, ice, mud, pebblestone, salt, sand, snow, woodchips,
    rock, clay, laterite ;
  - **toute autre valeur donne NULL**.

  On perd donc la distinction asphalte / pavés / gravier. Constaté sur les tuiles :
  uniquement `paved`/`unpaved`.
- **`brunnel`** = `bridge` / `tunnel` / **`ford`**. `layer` est brut (valeurs observées
  de -7 à 1 à Paris). Ce sont les meilleurs attributs « structure » parmi les sources
  prêtes à l'emploi.
- **Généralisation** : OMT fusionne les tronçons (`update_transportation_merge.sql`).
  `class=path` couvre footway, cycleway, path, steps, avec le détail dans `subclass`.
  `track` a sa propre `class`.
- **API publique** ([ToS](https://openfreemap.org/tos/)) : il est interdit de « Collect
  data from the service in automated ways », sauf offline ≤ 3000 tuiles / 50 Mo,
  4 requêtes parallèles et un User-Agent unique.
  **Pour un service, il faut télécharger le planet et self-host** (autorisé, c'est
  l'objet des downloads).
- **Attribution** : © OpenMapTiles (CC-BY 4.0, [LICENSE](https://github.com/openmaptiles/openmaptiles/blob/master/LICENSE.md))
  + © OpenStreetMap.

### 3.4 Protomaps / PMTiles

- **Builds** ([docs downloads](https://docs.protomaps.com/basemaps/downloads)) : un build
  planet PMTiles quotidien en z0–15. La [liste des builds](https://build-metadata.protomaps.dev/builds.json)
  donne 20261030.pmtiles, **139,2 Go**, v4.15.2. La doc annonce « ~120 GB », chiffre
  daté. Rétention : 1 semaine de builds quotidiens, puis les lundis du dernier mois, puis
  un build par version. `pmtiles extract` permet un extrait régional ou un `--maxzoom`.
  « Hotlinking … discouraged » : il faut copier le fichier dans son propre stockage.
  Licence : ODbL Produced Work, code BSD-3, format PMTiles domaine public.
- **Couche `roads`** ([doc layers](https://docs.protomaps.com/basemaps/layers),
  [`Roads.java`](https://github.com/protomaps/basemaps/blob/main/tiles/src/main/java/com/protomaps/basemap/layers/Roads.java)) :
  - `kind` (highway / major_road / minor_road / path…) et `kind_detail` (≈ valeur
    highway : track, path, cycleway, bridleway, steps, sidewalk…) ;
  - `is_bridge` et `is_tunnel` dès z12, `is_link`, `oneway` (z14), `access` (z15) ;
  - **pas de `surface`, pas de `ford`, pas de `layer`** ;
  - `mergeLineStrings` à tous les zooms.
- **Verdict** : excellent format (PMTiles, range requests, un seul fichier) et le seul
  planet en z15. Mais **aucune information de revêtement**, donc il ne couvre que la
  moitié du besoin.

### 3.5 OpenMapTiles self-host / MapTiler

- Même schéma qu'OpenFreeMap (voir §3.3). Pipeline officiel imposm3 + PostGIS
  ([openmaptiles](https://github.com/openmaptiles/openmaptiles)), ou plus simplement
  Planetiler avec son profil OMT intégré.
- MapTiler vend ses données / son planet sous licence commerciale. Cela n'a aucun intérêt
  ici, puisque OpenFreeMap fournit déjà un planet OMT hebdomadaire gratuit.
- Contrainte de licence : attribution « © OpenMapTiles » obligatoire (CC-BY 4.0 sur le
  design). Des exceptions sont possibles via MapTiler.

### 3.6 Générer ses propres tuiles (Planetiler / tilemaker)

- **Planetiler** ([GitHub](https://github.com/onthegomap/planetiler), Apache-2.0) :
  - profil YAML sur mesure ([custommap](https://github.com/onthegomap/planetiler/blob/main/planetiler-custommap/README.md))
    avec `include_when`, attributs `tag_value`, `min_zoom` par attribut,
    `tolerance_at_max_zoom` et `tile_post_process.merge_line_strings` optionnel ;
  - benchmarks planet (profil OMT, plus lourd qu'un profil « routes seules ») : 29 min
    sur 64 CPU / 128 Go, **3 h 35 sur 8 CPU / 16 Go**, 18 h sur 2 CPU / 8 Go ;
  - pré-requis : RAM libre ≈ 0,5 × taille du PBF (~92 Go en 2026), disque SSD ≈ 5 à
    10 × PBF ;
  - un profil limité aux `highway=*` produira un fichier bien plus petit que les
    70–140 Go des basemaps.
- **tilemaker** ([GitHub](https://github.com/systemed/tilemaker), FTWPL) : profil Lua +
  config JSON, sortie MBTiles/PMTiles, sans base. Option `--store` (SSD) pour les grosses
  zones. Le profil OMT fourni (`process-openmaptiles.lua`) regroupe aussi `surface` en
  paved/unpaved (`surfaceMinzoom = 12`) et gère `brunnel=ford`.
  Le [profil Shortbread tilemaker](https://github.com/shortbread-tiles/shortbread-tilemaker)
  écrit `surface` brut (`Attribute("surface", surface, 11)`), mais simplifie sous z14
  (`simplify_below: 14`) et combine les lignes (`combine_below: 15`).
- **Schéma minimal proposé** (couche unique `ways`, z14 uniquement, sans simplification
  à z14, sans merge) :
  - `osm_id` ;
  - `highway` brut ;
  - `surface` brut ;
  - `smoothness`, `tracktype` ;
  - `bridge`, `tunnel`, `ford`, `covered`, `layer` ;
  - `bicycle` / `foot` ;
  - `mtb:scale`, `sac_scale` (éventuellement) ;
  - et, à part, les nœuds `ford=yes` (points).

  La normalisation vers notre enum (asphalte, pavés, gravier, terre…) se fait dans le
  service Rust, pas dans les tuiles. On garde ainsi la donnée brute et une logique
  versionnée côté code (cf. [02-osm-surface-typologie.md](02-osm-surface-typologie.md)).
- **Coût** : il faut une machine de build et un cron, et on prend en charge les mises à
  jour. En échange, on maîtrise totalement le schéma et la fraîcheur : on peut rebuilder
  chaque semaine depuis planet.osm.org ou un extrait Geofabrik.

### 3.7 Autres pistes (hors tuiles)

- **Overture Maps — transportation** ([guide](https://docs.overturemaps.org/guides/transportation/),
  [segment](https://docs.overturemaps.org/schema/reference/transportation/segment/)) :
  - fichiers Parquet sur S3/Azure, release mensuelle, ODbL ;
  - champs `road_surface`, `road_flags` et `level_rules`, avec référencement linéaire
    (`between`) le long du segment ;
  - les valeurs exactes de `road_surface` et `road_flags` n'ont pas pu être confirmées
    dans la doc consultée.

  Alternative crédible, mais ce ne sont pas des tuiles. Il faudrait un index spatial
  maison (par ex. GeoParquet partitionné + R-tree en Rust).
- **Mapbox Streets / MapTiler Cloud / Stadia** : services commerciaux, avec des CGU qui
  interdisent généralement l'extraction de données. Écartés.

## 4. Focus : l'attribut `surface`

| Source | Présent sur la couche routes ? | Valeurs | Zoom |
|---|---|---|---|
| Spec Shortbread 1.0 / 1.1 | Oui | brutes (« value of surface=* ») | 11+ |
| VersaTiles (constaté) | Oui | **`paved` / `unpaved`** | 11+ |
| OSMF (constaté) | **Non** (seulement `street_polygons`, brut) | — | — |
| OpenMapTiles / OpenFreeMap | Oui | `paved` / `unpaved` (liste fixe, autres valeurs → NULL) | 12+ (tilemaker) |
| Protomaps v4 | Non | — | — |
| Tuiles maison | Oui | brutes (`asphalt`, `sett`, `gravel`, `compacted`…) | au choix |

Taux de remplissage observé à z14, part des entités routes avec un `surface` non nul :
- OpenFreeMap : 51 % à Paris, 76 % à Nantes, 76 % à Annecy, 40 % à Fontainebleau ;
- VersaTiles : environ 70 %, mais sur des lignes fusionnées.

Un `surface` absent reste fréquent sur les tracks et paths. Le service devra donc
**inférer** le revêtement depuis `highway` + `tracktype` (cf. doc 02), d'où l'intérêt
d'avoir `tracktype` et `highway` bruts.

## 5. Focus : structure (pont / tunnel / gué)

- **Pont / tunnel** : présents dans toutes les sources (booléens ou `brunnel`), dès z11
  (Shortbread) ou z12 (Protomaps). Suffisant pour taguer un segment comme « sur un pont ».
- **Gué** : seul OpenMapTiles a `brunnel=ford`, et seulement pour les **ways** taguées
  `ford=yes`. Or `ford=yes` est très souvent porté par un **nœud**, à l'intersection
  chemin × cours d'eau. Aucun schéma de basemap n'expose ces nœuds sur les routes. Les
  tuiles maison peuvent le faire.
- **`layer`** : uniquement OpenMapTiles (brut). Utile pour distinguer, lors du
  map-matching, la route sur le pont de la route dessous. Shortbread et Protomaps ne s'en
  servent que pour le tri. Sans `layer`, il faut se reposer sur `bridge`/`tunnel` et sur
  la cohérence de la trace.
- **Fusion des lignes** (VersaTiles, Protomaps, OMT) : un pont reste une entité séparée,
  puisque ses attributs diffèrent. La fusion ne fait donc pas perdre l'information de
  structure, mais elle supprime le lien vers l'id de way OSM.

## 6. Recommandation

**Source primaire : tuiles maison générées avec Planetiler** (profil YAML dédié
« route-details »), en z14 uniquement, au format PMTiles ou MBTiles, servies en local
au microservice Rust :

1. C'est la seule option qui donne **`surface` brut** et donc l'enum fine (asphalte /
   pavés / gravier / terre). Toutes les sources prêtes à l'emploi plafonnent à
   paved/unpaved, ou n'ont rien.
2. Elle permet d'ajouter `ford` (ways **et** nœuds), `layer`, `smoothness`, `tracktype`
   et `osm_id`, sans simplification ni fusion à z14 : une géométrie fidèle pour le
   map-matching.
3. Elle reste conforme à la contrainte « pas de base OSM locale » : Planetiler lit le
   `.osm.pbf` directement, sans PostGIS. Le service ne lit que des tuiles.
4. Coût raisonnable : un build hebdo ou mensuel sur une machine 8 CPU / 16–32 Go (ordre
   de quelques heures pour la planète, bien moins pour un extrait Europe ou France).
   Commencer par un extrait Geofabrik pour itérer sur le schéma.

**Alternative 1, prête à l'emploi : OpenFreeMap planet (OpenMapTiles)**. On télécharge
le PMTiles (87 Go) chaque semaine et on le lit localement :
- `brunnel` (bridge/tunnel/**ford**), `layer`, `class`/`subclass`, `paved`/`unpaved` ;
- c'est la meilleure source « structure » sans build ;
- l'enum revêtement se limite à 2 classes (+ inférence par `class`/`subclass`) ;
- attribution OpenMapTiles obligatoire.

**Alternative 2 : VersaTiles `osm.versatiles`** (62 Go, lecture native via l'écosystème
Rust VersaTiles) : `bridge`/`tunnel`, `kind` = highway brut, `tracktype` brut, `surface`
regroupé. Elle n'a ni `ford` ni `layer`, et les mises à jour sont rares (2 à 4 mois).

**À écarter pour le backend** :
- **OSMF** : bulk interdit, pas de `surface` dans les tuiles, `tracktype` incohérent ;
- **API publique OpenFreeMap** : collecte automatisée interdite ;
- **Protomaps** : pas de `surface`. Reste un bon choix si un jour seul pont/tunnel compte,
  ou comme fond de carte.

**Approche hybride possible** : démarrer le prototype sur le PMTiles OpenFreeMap, avec un
lecteur PMTiles et un décodeur MVT en Rust. Le format est identique pour les tuiles
maison, donc on bascule ensuite sur un PMTiles maison en ne changeant que le mapping
d'attributs.

## 7. Risques

- **Écart spec / réalité** : VersaTiles et OSMF ne respectent pas, en pratique, la spec
  Shortbread pour `surface`. Il ne faut pas se fier à la seule spec : écrire des tests
  d'intégration sur des tuiles réelles connues.
- **Généralisation** : même à z14, la quantification MVT (extent 4096, soit ≈ 2,4 m par
  unité à 45° de latitude) et les fusions de lignes dégradent la géométrie. Le
  map-matching d'une trace GPS doit être tolérant (buffer de quelques mètres). Gare aux
  segments courts (ponts courts, gués) et aux ways parallèles (route + piste cyclable
  séparée).
- **Couverture `surface`** : 25 à 60 % de ways sans `surface` selon les zones. Une
  inférence par `highway`/`tracktype` sera nécessaire, avec un niveau de confiance à
  exposer dans l'API.
- **Fraîcheur** : un planet hebdo (OFM), tous les 2 à 4 mois (VersaTiles), ou au rythme
  de notre cron (maison). Les corrections OSM récentes ne sont pas visibles
  immédiatement.
- **Licences** :
  - ODbL : le résultat du service (typologie par segment) est vraisemblablement un
    *Produced Work*, avec attribution « © OpenStreetMap contributors » ;
  - si on redistribue une base dérivée (ex. un cache de segments qualifiés), la clause
    share-alike de l'ODbL s'applique ;
  - avec OpenMapTiles, ajouter « © OpenMapTiles ».
- **Pérennité** : repo `versatiles-generator` archivé ; les URLs de builds Protomaps
  peuvent changer ; OpenFreeMap est un projet porté par une seule personne
  (financement par dons). La génération maison réduit cette dépendance.
- **Volumétrie** : planet de 60 à 140 Go selon la source. Prévoir le stockage, ou ne
  garder que la zone utile avec `pmtiles extract` sur une bbox ou un extrait Geofabrik.

## Sources

- Shortbread : [schéma 1.0](https://shortbread-tiles.org/schema/1.0/), [schéma 1.1](https://shortbread-tiles.org/schema/1.1/), [shortbread-tilemaker process.lua / config.json](https://github.com/shortbread-tiles/shortbread-tilemaker)
- VersaTiles : [site](https://versatiles.org/), [téléchargements](https://download.versatiles.org/), [extensions Shortbread](https://docs.versatiles.org/compendium/specification_shortbread_extensions.html), [tiles.json](https://tiles.versatiles.org/tiles/osm/tiles.json), [versatiles-generator](https://github.com/versatiles-org/versatiles-generator)
- OSMF : [Vector Tile Usage Policy](https://operations.osmfoundation.org/policies/vector/), [annonce juillet 2025](https://blog.openstreetmap.org/2025/07/22/vector-tiles-are-deployed-on-openstreetmap-org), [tilejson shortbread_v1](https://vector.openstreetmap.org/shortbread_v1/tilejson.json), [osm2pgsql-themepark](https://github.com/osm2pgsql-dev/osm2pgsql-themepark), [Tilekiln](https://github.com/pnorman/tilekiln)
- OpenFreeMap : [site](https://openfreemap.org/), [ToS](https://openfreemap.org/tos/), [GitHub](https://github.com/hyperknot/openfreemap), [tilejson planet](https://tiles.openfreemap.org/planet)
- OpenMapTiles : [schéma](https://openmaptiles.org/schema/), [transportation.yaml](https://github.com/openmaptiles/openmaptiles/blob/master/layers/transportation/transportation.yaml), [class.sql](https://github.com/openmaptiles/openmaptiles/blob/master/layers/transportation/class.sql), [LICENSE](https://github.com/openmaptiles/openmaptiles/blob/master/LICENSE.md)
- Protomaps : [layers](https://docs.protomaps.com/basemaps/layers), [downloads](https://docs.protomaps.com/basemaps/downloads), [Roads.java](https://github.com/protomaps/basemaps/blob/main/tiles/src/main/java/com/protomaps/basemap/layers/Roads.java), [builds.json](https://build-metadata.protomaps.dev/builds.json)
- Générateurs : [Planetiler](https://github.com/onthegomap/planetiler), [Planetiler custommap](https://github.com/onthegomap/planetiler/blob/main/planetiler-custommap/README.md), [tilemaker](https://github.com/systemed/tilemaker), [process-openmaptiles.lua](https://github.com/systemed/tilemaker/blob/master/resources/process-openmaptiles.lua)
- Overture : [transportation guide](https://docs.overturemaps.org/guides/transportation/), [segment schema](https://docs.overturemaps.org/schema/reference/transportation/segment/)
