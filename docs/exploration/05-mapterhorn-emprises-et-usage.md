# 05 — Mapterhorn : emprises des sources et conditions d'usage de l'API

> Recherche du 30 octobre 2026. Elle complète la section Mapterhorn de
> [04-sources-elevation.md](04-sources-elevation.md) sans la répéter.
> Sources : code du dépôt [mapterhorn/mapterhorn](https://github.com/mapterhorn/mapterhorn) (commit
> `ccfd5b5` du 26/10/2026), issues GitHub, site mapterhorn.com et requêtes directes aux serveurs.
> **[V]** = fait vérifié (code lu, fichier téléchargé ou requête HTTP faite).
> **[I]** = inférence tirée de ces faits, à confirmer.

---

## 1. Emprises des sources : réponse

**Oui, en grande partie.** Mapterhorn publie deux fichiers d'emprises par source, en plus d'`attribution.json`.
Il n'existe pas de couche de provenance par pixel. On peut cependant la reconstituer de façon fiable avec
la règle de fusion, qui est déterministe.

### 1.1 Ce qui est publié

| Fichier | Contenu | Taille | Usage |
|---|---|---|---|
| [`coverage-index.pmtiles`](https://download.mapterhorn.com/coverage-index.pmtiles) | Pour chaque tuile z4 à z12, un JSON `{source_id: metadata.json}` qui liste toutes les sources présentes dans la tuile, avec `name`, `resolution`, `license`, `producer`… | **1,5 Mo** | Pré-tri grossier : savoir si une tuile z12 contient autre chose que `glo30` |
| [`coverage.pmtiles`](https://download.mapterhorn.com/coverage.pmtiles), servi en MVT par [single-archive-tiles.mapterhorn.com/coverage.json](https://single-archive-tiles.mapterhorn.com/coverage.json) | Tuiles vectorielles z0 à z14, couche `coverage`, un polygone par source avec l'attribut `source` | **342 Mo** | Test précis point-dans-polygone, à l'échelle du mètre à z14 |
| Tarball par source `download.mapterhorn.com/sources/{source}.tar` | En tête de l'archive : `metadata.json`, `bounds.csv` (bbox EPSG:3857 de chaque GeoTIFF), `coverage.gpkg` (polygone de données valides), puis les GeoTIFF | variable | Provenance fine, au niveau du fichier source |

Toutes ces informations sont **[V]**. Les deux PMTiles datent du 14/09/2026 (en-tête `last-modified`) et
sont envoyés par [`upload.py`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/upload.py).
Le contenu des tarballs se lit dans
[`source_create_tarball.py`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/source_create_tarball.py).
La page [mapterhorn.com/coverage](https://mapterhorn.com/coverage) affiche `coverage.pmtiles`. L'exemple
[attribute-visible-sources](https://mapterhorn.com/examples/attribute-visible-sources) se sert de
`coverage-index.pmtiles` : au-delà de z12, il remonte à la tuile parente z12.

### 1.2 Comment les emprises sont construites (pipeline)

- [`source_bounds.py`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/source_bounds.py) écrit
  `bounds.csv`, avec la bbox de chaque GeoTIFF. **[V]**
- [`source_polygonize.py`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/source_polygonize.py)
  procède en deux temps. `gdal_calc "A*0+1"` produit un masque qui garde le nodata, puis
  `gdal_polygonize` vectorise chaque fichier. Les polygones sont enfin réunis par `ST_Union`.
  **Le polygone couvre donc les données valides, pas la bbox.** **[V]**
- [`create_coverage_index.py`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/create_coverage_index.py)
  simplifie le polygone (100 m), lui ajoute un **tampon de 1 km** et le rastérise en tuiles z12. Il agrège
  ensuite vers z4. L'index **surestime** donc la couverture d'environ 1 km. **[V]**
- [`Coverage.java`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/Coverage.java) (Planetiler)
  écrit les polygones dans `coverage.pmtiles`, avec une tolérance de simplification faible. **[V]**

### 1.3 Règle de fusion, qui sert à reconstituer la provenance

- [`aggregation_covering.py`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/aggregation_covering.py)
  calcule le `maxzoom` de chaque GeoTIFF : c'est le plus petit z dont le pixel Mercator est plus fin que
  celui du fichier. Ce maxzoom ne descend jamais sous 12, donc **GLO-30 est fixé à z12**. **[V]**
- [`aggregation_reproject.py`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/aggregation_reproject.py)
  trie les sources par `maxzoom` décroissant, puis par nom de source. [`aggregation_merge.py`](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/aggregation_merge.py)
  part de la première source, puis comble uniquement les pixels encore nodata avec les suivantes. Les
  coutures sont lissées par un flou gaussien sur une bande d'environ 150 m (`MACROTILE_BUFFER_3857`). **[V]**
- Le mainteneur a décrit la méthode à suivre dans
  l'[issue #350](https://github.com/mapterhorn/mapterhorn/issues/350) : charger les `coverage.gpkg` et
  `bounds.csv` des tarballs, garder les sources dont le polygone contient le point, puis prendre la plus haute
  résolution. Le demandeur a validé cette méthode sur l'US 3DEP : la hauteur servie correspond au fichier
  prédit à quelques mm près. Pour un autre fichier candidat, l'écart va de 22 à 105 mm. La couche
  `provenance.pmtiles` par pixel proposée dans cette issue **n'existe pas** (issue ouverte). **[V]**

### 1.4 Vérifications faites sur les données publiées

Le `coverage-index.pmtiles` a été téléchargé et décodé (Python `pmtiles`). Résultats à z12 **[V]** :

| Point | Sources listées |
|---|---|
| Grenoble, Lyon | `frhd2154d` (0,5 m), `frrgealti1metro`, `frrgealti5metro`, `glo30` |
| Paris | `frhd2154a`, RGE ALTI 1 m et 5 m, `glo30` |
| Zurich | `chzh` (0,25 m), `swissalti3d`, `glo30` |
| Bruxelles | `beflanders` (1 m), `glo30` |
| Casablanca | `glo30` seul |

Comptage des tuiles z12 : `frrgealti1metro` en a 12 721, `frrgealti5metro` 12 722, et les `frhd2154a…e`
11 544 au total. Une tuile MVT z14 de `coverage.pmtiles` près de Grenoble contient 4 polygones : glo30,
frhd2154d, frrgealti1metro et frrgealti5metro. **[V]**

**Conséquence [I]** : en France métropolitaine, à terre, le RGE ALTI 1 m couvre pratiquement tout le
territoire. Il passe devant GLO-30, dont le maxzoom est 12 contre ≈ 16. Le fond Copernicus (MNS) ne sert donc,
en métropole, que sur le littoral, en mer et dans les trous de nodata. La question « MNT national ou
GLO-30 ? » se pose surtout hors des pays couverts.

### 1.5 Indicateur indirect par le zoom max : fiable dans un seul sens

Tests HTTP **[V]** :
- `tiles.mapterhorn.com/17/…` à Grenoble renvoie 200 et z18 renvoie 404 ;
- au Maroc, z12 renvoie 200 et z13 renvoie 404 (`Tile not found`).

Le maxzoom d'une tuile d'agrégation est **le maximum des sources présentes dans la macro-tuile z12 et son
tampon**. Pour un pixel donné, on en tire deux conclusions :
- **404 à z13 ⇒ seulement GLO-30** (ou des sources ≤ z12). C'est fiable. **[I fort]**
- **200 à z13 et au-delà ⇒ une source fine existe à proximité**. Le pixel lui-même peut quand même venir
  de GLO-30 rééchantillonné, par exemple dans un trou de LiDAR ou à 500 m d'une frontière. C'est donc **non
  fiable** au niveau du pixel. **[I]**

### 1.6 Limites et risques

- **Égalités de maxzoom** : à maxzoom égal, l'ordre alphabétique du nom de source tranche. C'est le cas
  d'une source nationale de 20 à 30 m face à `glo30`, toutes deux à z12. Il faut appliquer exactement la
  même règle. **[V code / I effet]**
- **[Issue #340](https://github.com/mapterhorn/mapterhorn/issues/340)**, ouverte, avec la PR #341 non
  fusionnée : l'étape de fusion de `source_polygonize.py` ne garderait qu'un enregistrement, à cause d'un
  conflit de FID dans le GPKG. Ce n'est pas visible sur les sources françaises testées, qui couvrent des
  milliers de tuiles et non un seul GeoTIFF. Il faut quand même **contrôler chaque source utilisée**, par
  exemple en comparant son étendue au `bounds.csv`. **[I]**
- `source_fill_nodata.py` (v0.0.13) comble les petits trous avant la vectorisation : le polygone peut
  inclure des pixels interpolés. **[V code / I effet]**
- Une couture lissée sur environ 150 m mélange deux sources. Il faut marquer ces points « mixte ». **[V]**
- Les PMTiles de couverture ne sont pas forcément régénérés à chaque version. Il faut comparer l'`etag`
  avec `download_urls.json` / `attribution.json`. **[I]**

### 1.7 Exploitation recommandée pour le service

1. Au démarrage, ou chaque semaine, télécharger `coverage-index.pmtiles` (1,5 Mo, en mémoire). Pour chaque
   tuile z12 du parcours :
   - si la tuile contient seulement `glo30`, renvoyer `source=glo30` et `type=MNS` ;
   - sinon, passer à l'étape 2.
2. Récupérer les tuiles MVT z14 de `coverage` qui touchent le parcours, avec le même cache disque que les
   tuiles d'élévation (≈ 200 o par tuile). Faire le test point-dans-polygone pour chaque source. Prendre la
   source de plus forte résolution (`metadata.resolution`, à défaut le maxzoom calculé), puis l'ordre
   alphabétique. Marquer « couture » à moins de 150 m d'un bord de polygone de priorité supérieure.
3. L'option fine (fichier source, utile pour le datum par projet) consiste à lire en *range request*
   l'en-tête des tarballs (`coverage.gpkg`, `bounds.csv`). Elle n'est pas nécessaire pour distinguer
   MNT et MNS.

Il n'y a pas de carte à maintenir soi-même. **Si on veut zéro dépendance** à ces fichiers, l'alternative la
moins coûteuse est la suivante : une table statique « source → type (MNT/MNS) » dérivée
d'`attribution.json`, complétée par le test 404 à z13. Cela garantit de détecter le cas « GLO-30 seul »,
mais pas les trous à l'intérieur d'une zone couverte.

---

## 2. Conditions d'usage de `tiles.mapterhorn.com`

| Point | Constat |
|---|---|
| CGU / quotas | **Aucune CGU ni quota publié** : rien sur [data-access](https://mapterhorn.com/data-access), sur la page d'accueil, dans le README ou dans les issues (recherches « rate limit », « terms », « usage policy », « user agent » sans résultat pertinent). **[V]** |
| Usage serveur avec cache | Question posée dans l'[issue #317](https://github.com/mapterhorn/mapterhorn/issues/317) : relais serveur, 1 000 requêtes par jour, 1 requête par seconde, cache privé. Le mainteneur (O. Wipfli) répond : *« you are allowed to store and redistribute the tiles indefinitely, just make sure that you attribute the original sources »*. Il ajoute : *« Ideally you download the data you need as PMTiles archives »*. Aucun tarif. **[V]** |
| Self-host | **Recommandé** pour les usages qui ne sont pas de la cartographie interactive (même issue). Extraits par bbox avec `pmtiles extract`, et [miroirs](https://download.mapterhorn.com/mirrorstatus.json). **[V]** |
| User-Agent | Pas exigé : les requêtes avec un UA `curl` passent. Le serveur journalise l'IP, l'UA, le referrer et l'ASN pendant 30 jours ([privacy policy](https://mapterhorn.com/privacy-policy)). Il faut envoyer un UA identifiable avec un contact, par courtoisie. **[V / I]** |
| Cache HTTP | `cache-control: public, max-age=604800` (7 jours), `last-modified` présent, servi par Cloudflare (`cf-cache-status: HIT`). Un cache disque long, revalidé par `If-Modified-Since` après expiration, est cohérent avec ces en-têtes. **[V / I]** |
| Comportement hors zoom | Au-delà du maxzoom local, le serveur renvoie **404** (et non la tuile parente). Le client doit sur-zoomer depuis la tuile parente et mettre le 404 en cache. **[V]** |
| Attribution | Lien « © Mapterhorn » vers [mapterhorn.com/attribution](https://mapterhorn.com/attribution), jugé suffisant par le mainteneur pour l'affichage (#317). Il faut aussi respecter les licences sources qui l'exigent (swissALTI3D, CC BY, OGL…) : liste et PDF dans [attribution.json](https://download.mapterhorn.com/attribution.json). L'endpoint `/attribution` du service peut se limiter aux sources effectivement rencontrées (via §1). **[V]** |
| Financement / pérennité | Code BSD-3. Projet financé à l'origine par NLnet NGI0 Core. Stockage, Workers et bande passante offerts par Cloudflare. [Open Collective](https://opencollective.com/mapterhorn) : budget annuel estimé ≈ 27 k$, solde ≈ 12,9 k$. Un seul mainteneur actif ; les droits d'admin sont partagés avec B. Liu (Protomaps) et S. Brawer. Pas de SLA. **[V]** Le risque principal est une dépendance au sponsoring de Cloudflare et à une seule personne. **[I]** |

**Conclusion [I]** : pour un service personnel à faible volume, l'appel à `tiles.mapterhorn.com` avec un cache
disque persistant est **toléré, et même explicitement permis** (stockage et redistribution autorisés). Il faut
quatre précautions :
- un UA identifiable ;
- une requête à la fois et peu de requêtes par seconde ;
- l'attribution ;
- ne pas compter sur une disponibilité garantie.

Pour la France, un extrait PMTiles local (z ≤ 15 ou 16) reste la solution recommandée par le projet. L'API
publique sert alors de repli pour le reste du monde.
