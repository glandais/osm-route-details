# 02 — Contrat d'interface (grosse maille)

> Statut : conception générale. Ce document fixe la **forme** du contrat v1 : style, endpoints, entrée,
> sortie, erreurs, versionnage, limites. Ce n'est ni un schéma exhaustif ni un OpenAPI. Les exemples
> JSON sont **illustratifs** : les noms peuvent encore bouger, les valeurs ne sont pas des défauts
> calibrés. La sémantique des enums et des modes dégradés est dans
> [01-conception-generale.md](01-conception-generale.md) (§5 et §6).

## 1. Style d'API

- HTTP/JSON, **un appel synchrone** par parcours (jusqu'à 200 km ; pas d'endpoint asynchrone en v1).
  Aucun état côté serveur entre deux appels.
- Version majeure dans le chemin (`/v1/...`). On ne casse jamais le contrat à l'intérieur d'une
  version majeure.
- Corps compressés acceptés et renvoyés (gzip, brotli).
- Le service renvoie des **codes, jamais de texte localisé** : les clients traduisent. Le champ
  `message` des avertissements et des erreurs sert au diagnostic.
- Distances en mètres, altitudes en mètres, pentes en pourcentage, coordonnées en WGS84 (lat, lon).
- Pas d'authentification intégrée. TLS, clés et quotas sont gérés par le reverse proxy.

## 2. Endpoints

| Endpoint | Rôle |
|---|---|
| `POST /v1/route-details` | Calcul principal. |
| `GET /v1/capabilities` | Bornes de requête, valeurs d'enum (dont les raisons d'insertion des points), **valeurs par défaut appliquées** (dont les seuils de sélection des points de rupture), zone de qualité maximale (France), capacités des données actives (gué détectable ? `surface.detail` disponible ? MNS des ponts actif ? heuristiques actives ?), sources d'élévation (IGN LiDAR HD, Mapterhorn), leur zoom et leur repli, versions. |
| `GET /v1/attribution` | Liste complète des attributions des sources configurées : OSM (ODbL), ESA WorldCover (CC BY 4.0, variante VersaTiles servie), IGN LiDAR HD (Etalab 2.0), sources Mapterhorn, OpenMapTiles si actif. Accessible sans authentification : les clients y renvoient leurs utilisateurs. |
| `GET /health/live` | Le processus répond. |
| `GET /health/ready` | Les données configurées sont lisibles. |
| `GET /metrics` | Métriques, sur un **port interne** seulement. |

Pas de `/structures/{osm_id}` avant les tuiles maison (V2) : sans `osm_id`, il n'y a rien à exposer.

## 3. Entrée

### 3.1 Formats de parcours

Le parcours est donné sous l'une des deux formes suivantes, et une seule :

- `points` : liste de `{lat, lon}`. Une altitude éventuelle est ignorée ;
- `polyline` : polyline encodée, avec sa précision (`polyline_precision` : 5 ou 6).

L'ordre des points est le sens de parcours. Un parcours peut repasser par les mêmes coordonnées
(aller-retour, boucle) : seuls les doublons **consécutifs** sont fusionnés.

En v1, le parcours est supposé être un **tracé planifié à vélo**. Il n'y a ni paramètre de mode ni
paramètre de type de trace. Ces paramètres arriveront en V2, en option et avec une valeur par défaut
qui conserve le comportement actuel.

Le GeoJSON (LineString) en entrée et en sortie est prévu en V1.x.

### 3.2 Options

Toutes les options sont facultatives et ont une valeur par défaut documentée dans
`/v1/capabilities`, qui publie aussi les bornes ci-dessous. Une valeur hors bornes donne un 400
`invalid_input`.

| Option | Valeurs | Défaut | Rôle |
|---|---|---|---|
| `sections` | sous-ensemble de `elevation`, `typology`, `summary` | toutes | Parties de la réponse à renvoyer. La typologie **tourne quand même** en interne pour corriger les ouvrages. |
| `sampling.mode` | `input_with_breakpoints` \| `input` \| `distance` | `input_with_breakpoints` | `input_with_breakpoints` : tous les points d'entrée, plus les points de rupture insérés par le service (sommet, creux, rupture de pente, extrémité d'ouvrage, changement de revêtement ou de source de terrain), chacun avec ses raisons (doc 01 §5.6). `input` : une valeur par point d'entrée, sans insertion. `distance` : un profil à pas métrique. |
| `sampling.step_m` | [5 ; 1 000] m | 25 m | Pas du profil en mode `distance`. |
| `elevation.smoothing_m` | `0`, ou [10 ; 1 000] m | `0` (désactivé) | Ajoute une altitude lissée. |
| `elevation.gain_preset` | `dem`, `raw`, … | `dem` | Seuil et échelle du D+. Sur les plages GLO-30 (MNS), le seuil est **adapté automatiquement**. Les valeurs effectives, plage par plage, sont renvoyées dans la réponse (`gain_params`). |
| `structure_correction` | booléen | `true` | `false` : MNT brut sur les ouvrages, signalé comme non corrigé. Le client ne choisit ni la source d'élévation ni la méthode : elles sont décidées par le service et exposées dans la réponse. |
| `accept_partial` | booléen | `false` | Accepte une réponse partielle quand une source est **en panne**, au lieu d'un 503. |
| `debug` | booléen | `false` | Attributs bruts par segment. **Réservé à un usage interne** : activé par configuration du déploiement, sinon refusé (400). |

### 3.3 Exemple de requête

```json
{
  "points": [
    {"lat": 47.2184, "lon": -1.5536},
    {"lat": 47.2201, "lon": -1.5490},
    {"lat": 47.2263, "lon": -1.5411}
  ],
  "options": {
    "sections": ["elevation", "typology", "summary"],
    "sampling": {"mode": "input_with_breakpoints"},
    "accept_partial": false
  }
}
```

## 4. Sortie

### 4.1 Structure générale

| Bloc | Contenu |
|---|---|
| `meta` | `api_version`, `rules_version`, `data_versions` (identifiant et date par source), identifiant de requête, durée. |
| `status` | `complete` ou `partial`. `partial` signifie qu'une **source** n'a pas fourni de donnée sur une partie du parcours : panne acceptée par `accept_partial`, ou plage hors de l'emprise des données configurées (`elevation_missing`, `typology_missing`). Une portion non appariée alors que les tuiles sont lues ne rend **pas** la réponse partielle : elle est signalée dans `quality`. |
| `points[]` | Par défaut, les points d'entrée et les points de rupture insérés (`input_with_breakpoints`) ; un élément par point d'entrée (`input`) ou par pas (`distance`) sur demande. Section `elevation`. |
| `segments[]` | Tronçons contigus et homogènes qui couvrent **tout** le parcours. Section `typology`. |
| `summary` | Agrégats du parcours. Section `summary`. |
| `quality` | Provenance factuelle (dont le terrain lu par tronçon), statut des corrections et avertissements. Toujours présent. |
| `attribution` | Mentions obligatoires et lien vers `/v1/attribution`. **Toujours présent**, quelles que soient les sections (§4.7). |

`meta`, `status`, `quality` et `attribution` sont présents dans toute réponse 200.

Les deux granularités se complètent :

- `points[]` porte l'altitude, pour enrichir une trace point par point ;
- `segments[]` porte la typologie, sous forme de suites homogènes.

Les deux portent **la distance cumulée et l'indice du point d'entrée**. Un client peut ainsi remettre
les résultats en face de son parcours sans recalcul géométrique.

### 4.2 `points[]`

| Champ | Sens |
|---|---|
| `d_m` | Distance curviligne cumulée depuis le début. |
| `index` | Indice du point d'entrée pour un point d'entrée ; sinon, indice du dernier point d'entrée qui précède (point inséré, mode `distance` ; même convention que `from_index`). |
| `origin` | `input` (point du client) ou `inserted` (point ajouté par le service). Mode `input_with_breakpoints` ; vaut toujours `input` en mode `input`, absent en mode `distance`. |
| `reasons[]` | Raisons d'insertion, codes stables, liste extensible : `local_max`, `local_min`, `grade_change`, `structure_start`, `structure_end`, `surface_change`, `terrain_source_change`, `elevation_gap_start`, `elevation_gap_end`. Obligatoire sur un point inséré ; présent sur un point d'entrée seulement si une rupture tombe dessus. |
| `lat`, `lon` | Sur un point inséré et en mode `distance`. Jamais sur un point d'entrée, qui n'est pas déplacé. |
| `ele` | Altitude **après correction des ouvrages**, non lissée. `null` si l'altitude est absente. |
| `ele_smoothed` | Présent seulement si un lissage est demandé. |
| `ele_method` | `dem` (MNT), `dsm_deck` (tablier de pont lu dans le MNS LiDAR), `interpolated_structure` (droite entre culées ou portails : tunnel, ou pont sans MNS exploitable), `gap_filled`, `none` (altitude absente). |

L'origine du terrain n'est pas répétée à chaque point : elle est donnée par plages dans
`quality.terrain` (§4.5).

En mode `input_with_breakpoints`, un ouvrage, un sommet ou un creux qui tombe entre deux points
d'entrée espacés est représenté par des points insérés. En mode `input`, il peut n'être porté par
aucun point. Dans tous les modes, le D+ et les pentes du `summary` et des `segments[]` sont calculés
sur le profil interne densifié : ils ne se recalculent pas à partir de `points[]`.

### 4.3 `segments[]`

| Champ | Sens |
|---|---|
| `from_m`, `to_m` | Plage en distance. |
| `from_index`, `to_index` | Points d'entrée qui encadrent la plage. |
| `structure` | `ground`, `bridge`, `tunnel`, `unknown`. `ford` est réservé. `unknown` : portion non appariée **sans** ouvrage détecté ni voie connue. Sur une voie fermée au vélo (api 1.1), celle de cette voie. |
| `structure_source` | Sur un ouvrage : `osm`, `heuristic` (V1, détecté hors de tout way OSM), `osm_heuristic` (V1, les deux concordent). Absent sur `ground` et `unknown`. |
| `surface.class` | `paved`, `unpaved`, `unknown`. Gelé en v1. |
| `surface.detail` | **Absent en v1.** Extensible, rempli en V2. |
| `surface.source` | `tagged`, `inferred_tracktype`, `inferred_highway`, `none`. |
| `way_kind` | `major_road`, `minor_road`, `track`, `path`, `cycleway`, `footway`, `steps`, `unknown`. |
| `matched` | `false` sur une portion non appariée. En V1, un ouvrage détecté par l'heuristique seule sur une portion non appariée est un segment `structure: bridge` ou `tunnel`, `structure_source: heuristic`, `matched: false`, revêtement et `way_kind` à `unknown` : `matched: false` n'implique donc pas `structure: unknown`. Depuis l'api 1.1, une portion qui suit une voie **fermée au vélo** (autoroute, trunk, route `bicycle=no`…) reste `matched: false` (elle n'est pas sur le réseau vélo) mais porte `structure`, `surface` et `way_kind` de cette voie, avec les avertissements `unmatched_range` et `bicycle_forbidden` ; un pont ou un tunnel de cette voie n'est pas corrigé. |
| `ele_method` | Méthode d'élévation dominante sur le tronçon. |
| `ele_start`, `ele_end`, `grade_pct` | Altitudes aux bornes et pente moyenne. |
| `osm` | Seulement avec `debug` : attributs bruts de la source. |

Un nouveau segment commence dès que l'un de ces attributs change : `structure`, `structure_source`,
`surface.class`, `surface.source`, `way_kind`, `matched`, ainsi qu'à chaque extrémité d'ouvrage (deux
ponts contigus restent deux segments, mais forment une seule entrée de `summary.structures[]`). Un
changement de `ele_method` ou d'origine du terrain seul ne coupe pas.

### 4.4 `summary`

- `length_m` ;
- `gain_m` et `loss_m`, **toujours accompagnés de leurs paramètres effectifs** (`gain_params` :
  preset, seuil, échelle de lissage, et `adapted[]`, les plages GLO-30 où le seuil a été adapté
  automatiquement, avec le seuil appliqué) et d'un indicateur `partial` si des plages d'altitude
  manquent ;
- `ele_min_m`, `ele_max_m` ;
- répartitions en mètres par `structure`, `surface.class`, `surface.source` et `way_kind` ;
- `structures[]` : liste des ponts et tunnels rencontrés sur le réseau vélo (pas ceux d'une voie
  fermée au vélo, portés par `segments[]` seulement), triée par distance : `kind` (`bridge` ou
  `tunnel`), plage, longueur, `structure_source`, méthode appliquée : `dsm_deck`,
  `interpolated_structure`, `kept_ground` (pont OSM sans tablier dans le MNS, au ras du sol :
  l'altitude du sol est gardée, ce qui compte comme une correction), ou `none` (non corrigé). Un
  ouvrage mixte (pont enchaîné à un tunnel) donne **une entrée par type, contiguës** (la fin de
  l'une est le début de la suivante), chacune avec sa méthode ; il n'y a pas de liste d'éléments ;
- `terrain_m` : longueur par origine du terrain (`ign_lidar_hd`, `mapterhorn_measured`, `mapterhorn`,
  `none`) ;
- `unknown_m` : longueur non appariée.

### 4.5 `quality`

| Champ | Sens |
|---|---|
| `matched_ratio` | Part de la longueur appariée. |
| `inferred_surface_ratio` | Part de la longueur dont le revêtement est déduit. |
| `elevation_coverage` | Part de la longueur avec une altitude. |
| `structure_correction` | Un statut **par type d'ouvrage** (`bridges`, `tunnels`), vocabulaire repris de vcyclist : `applied` (tous les ouvrages rencontrés sont corrigés, `kept_ground` compris), `partial` (au moins un non corrigé), `none_found` (aucun ouvrage), `disabled` (option client), `unavailable` (typologie absente : panne acceptée ou hors emprise). Plus des compteurs `counts` par méthode (`dsm_deck`, `interpolated_structure`, `kept_ground`, `uncorrected`), un par entrée de `summary.structures[]`, et par `structure_source`. |
| `terrain` | Provenance du terrain **par tronçon** : plages `{from_m, to_m, origin, measured, fallback_after_error}`. `origin` ∈ `ign_lidar_hd`, `mapterhorn_measured` (Mapterhorn en zone France, **hors** plages GLO-30 et leurs coutures), `mapterhorn` (ailleurs, GLO-30 et coutures compris), `none` ; `measured` dit si l'origine a été mesurée par un banc ; `fallback_after_error` distingue un repli subi (IGN en échec) d'un repli normal (hors LiDAR). |
| `warnings[]` | `{code, message, from_m?, to_m?}`. Les codes sont stables. Un avertissement signale une sortie **sciemment dégradée ou dépendante du réseau**, jamais le cas normal. |

Codes d'avertissement prévus en v1 : `unmatched_range`, `structure_uncorrected`,
`structure_slope_implausible`, `elevation_gap_filled`, `elevation_missing`, `typology_missing`,
`terrain_fallback` (tuiles IGN en échec, relues dans Mapterhorn), `dsm_unavailable` (MNS illisible :
pont interpolé au lieu d'être lu dans le MNS), `dsm_source` (plage lue sur GLO-30, un MNS : seuil de
D+ adapté), `outside_primary_zone` (plage hors France : Mapterhorn seul, mode dégradé), et
`typology_unavailable` / `elevation_unavailable` (seulement avec `accept_partial`). En V1 :
`structure_untagged` (ouvrage détecté par heuristique hors de tout way OSM, corrigé) et, depuis
l'api 1.1, `bicycle_forbidden` (le parcours suit une voie fermée au vélo : typologie de cette voie
rendue, portion non appariée au réseau vélo, ouvrages non corrigés).

Aucun référentiel vertical n'est déclaré ni signalé : les écarts entre référentiels nationaux sont
jugés négligeables pour l'usage (doc 01 §2.2). Les altitudes sont des altitudes au-dessus du niveau
de la mer, jamais des hauteurs ellipsoïdales : un client ne doit pas les comparer à l'altitude GPS
brute (écart d'environ 50 m en France).

Il n'y a **pas** de score de confiance agrégé (`high` / `medium` / `low`) en v1, car il ne pourrait
pas être calibré sans vérité terrain. La confiance se lit dans les facteurs bruts : `surface.source`,
`matched`, `structure_source`, `ele_method`, `terrain`, avertissements.

### 4.6 Exemple de réponse (abrégé)

```json
{
  "meta": {
    "api_version": "1.1",
    "rules_version": "2026.11.1",
    "data_versions": {
      "typology": {"source": "versatiles-shortbread", "date": "2026-10-27"},
      "elevation": [
        {"source": "ign-lidar-hd", "products": ["mnt", "mns"], "zoom": 15, "read_at": "2026-10-30"},
        {"source": "mapterhorn", "version": "0.0.13", "zoom": 12, "role": "fallback"}
      ]
    },
    "request_id": "c1f…",
    "duration_ms": 84
  },
  "status": "complete",
  "points": [
    {"d_m": 0,    "index": 0, "origin": "input",    "ele": 12.5, "ele_method": "dem"},
    {"d_m": 350,  "index": 0, "origin": "inserted", "reasons": ["structure_start"],
     "lat": 47.2196, "lon": -1.5503, "ele": 16.8, "ele_method": "dsm_deck"},
    {"d_m": 412,  "index": 1, "origin": "input",    "ele": 19.8, "ele_method": "dsm_deck"},
    {"d_m": 455,  "index": 1, "origin": "inserted", "reasons": ["local_max"],
     "lat": 47.2205, "lon": -1.5484, "ele": 20.2, "ele_method": "dsm_deck"},
    {"d_m": 520,  "index": 1, "origin": "inserted", "reasons": ["structure_end"],
     "lat": 47.2210, "lon": -1.5476, "ele": 19.1, "ele_method": "dsm_deck"},
    {"d_m": 1180, "index": 2, "origin": "input",    "ele": 21.5, "ele_method": "dem"}
  ],
  "segments": [
    {"from_m": 0, "to_m": 350, "from_index": 0, "to_index": 1,
     "structure": "ground", "surface": {"class": "paved", "source": "tagged"},
     "way_kind": "cycleway", "matched": true,
     "ele_method": "dem", "ele_start": 12.5, "ele_end": 16.8, "grade_pct": 1.2},
    {"from_m": 350, "to_m": 520, "from_index": 0, "to_index": 2,
     "structure": "bridge", "structure_source": "osm",
     "surface": {"class": "paved", "source": "inferred_highway"},
     "way_kind": "minor_road", "matched": true,
     "ele_method": "dsm_deck", "ele_start": 16.8, "ele_end": 19.1, "grade_pct": 1.4}
  ],
  "summary": {
    "length_m": 1180,
    "gain_m": 9, "loss_m": 1,
    "gain_params": {"preset": "dem", "threshold_m": 3, "smoothing_m": 30,
                    "adapted": [], "partial": false},
    "ele_min_m": 12.5, "ele_max_m": 21.5,
    "by_structure_m": {"ground": 1010, "bridge": 170},
    "by_surface_class_m": {"paved": 1180},
    "structures": [{"kind": "bridge", "from_m": 350, "to_m": 520,
                    "structure_source": "osm", "method": "dsm_deck"}],
    "terrain_m": {"ign_lidar_hd": 1180},
    "unknown_m": 0
  },
  "quality": {
    "matched_ratio": 1.0, "inferred_surface_ratio": 0.14, "elevation_coverage": 1.0,
    "structure_correction": {"bridges": "applied", "tunnels": "none_found",
                             "counts": {"dsm_deck": 1}},
    "terrain": [{"from_m": 0, "to_m": 1180, "origin": "ign_lidar_hd",
                 "measured": true, "fallback_after_error": false}],
    "warnings": []
  },
  "attribution": {
    "text": "© OpenStreetMap contributors (ODbL) · © IGN — LiDAR HD (Etalab 2.0) · Mapterhorn and its sources",
    "url": "/v1/attribution"
  }
}
```

Exemple d'avertissement localisé :

```json
{"code": "structure_uncorrected", "message": "bridge end has no elevation",
 "from_m": 5210, "to_m": 5480}
{"code": "dsm_unavailable", "message": "surface model tiles failed, bridge interpolated",
 "from_m": 8120, "to_m": 8790}
{"code": "outside_primary_zone", "message": "outside France, Mapterhorn only",
 "from_m": 41200, "to_m": 63750}
```

Exemple de plage GLO-30 dans `gain_params.adapted[]` (seuil illustratif, non calibré) :

```json
{"from_m": 41200, "to_m": 52300, "dsm": "glo30", "threshold_m": 10}
```

### 4.7 `attribution`

- Présent dans **toute** réponse 200, même avec `sections: ["elevation"]` : les données OSM servent
  toujours à corriger les ouvrages.
- `text` : mention courte des sources configurées (« © OpenStreetMap contributors », « © IGN — LiDAR
  HD », Mapterhorn et ses sources, « © OpenMapTiles » si l'adaptateur OpenFreeMap est actif). Comme
  dans vcyclist, la source d'élévation **et** son repli sont mentionnés. `url` : `/v1/attribution`.
- Un client qui affiche ces données doit relayer la mention. Un client qui **accumule** les réponses
  pour constituer une base de segments qualifiés tombe sous la clause de partage à l'identique de
  l'ODbL (doc 01 §8.6).
- `quality.terrain` dit si l'IGN ou Mapterhorn ont servi. À l'intérieur de Mapterhorn, la mention
  n'est pas filtrée : il faudrait une provenance par pixel qu'il ne fournit pas.

## 5. Erreurs et cas limites

### 5.1 Format d'erreur

Le corps d'erreur suit la **RFC 9457** (`application/problem+json`), comme le service `:api` de
vcyclist : `type` = `urn:osm-route-details:problem:<code>`, `title`, `status`, `detail` (diagnostic),
plus l'extension `code` (le même code machine stable, pour les clients qui ne lisent pas l'URN) et
d'éventuels `details`. Il n'y a pas de codes Valhalla. Un avertissement n'est jamais une erreur.
Exemple :

```json
{"type": "urn:osm-route-details:problem:data_unavailable", "title": "Data unavailable",
 "status": 503, "detail": "elevation sources unreachable", "code": "data_unavailable",
 "details": {"source": "elevation"}}
```

Une option ou une clé inconnue donne un 400 **avant** tout calcul.

### 5.2 Codes HTTP

| HTTP | `code` | Cas |
|---|---|---|
| 400 | `invalid_input` | Format invalide, moins de 2 points distincts, coordonnée hors bornes (y compris hors plage Web Mercator), option hors bornes, `points` et `polyline` fournis ensemble, `debug` demandé sans y avoir droit. |
| 413 | `route_too_large` | Trop de points d'entrée, ou parcours de plus de 200 km (bornes d'entrée seulement, §7). Les bornes sont publiées dans `/v1/capabilities`. Le client découpe. |
| 422 | `out_of_coverage` | **Aucune** donnée d'**aucune** source sur toute l'emprise (cas des extraits par bbox). Si une seule source couvre, ou si la couverture est partielle, la réponse est un 200 `partial`. |
| 503 | `data_unavailable` | Une source attendue est en panne, même après essai du cache périmé **et du repli** (IGN en échec mais Mapterhorn disponible : 200 avec `terrain_fallback`), et `accept_partial` n'est pas activé. Règle stricte, sans seuil de tolérance : une seule tuile en panne non rattrapée suffit, quelle que soit la longueur touchée. Les deux sources en panne donnent toujours un 503, même avec `accept_partial`. Toujours avec `Retry-After`. |
| 503 | `overloaded` | File saturée. |
| 504 | `timeout` | Budget de temps dépassé. Jamais de réponse tronquée en silence. |
| 500 | `internal` | Erreur inattendue. |
| 404 | `not_found` | Route inconnue. |
| 405 | `method_not_allowed` | Méthode non prise en charge par la route (ex. `GET /v1/route-details`). |
| 415 | `unsupported_media_type` | `Content-Type` autre que `application/json` sur `POST /v1/route-details`. |

Un corps trop gros reste un 413 `route_too_large` (`details.limit = "body_bytes"`). Toutes ces
erreurs, protocolaires comprises, suivent le format du §5.1.

Le 429 (quota) est renvoyé par le reverse proxy, pas par le service.

### 5.3 Cas limites

| Cas | Comportement |
|---|---|
| Points consécutifs identiques | Dédoublonnés. Les indices d'entrée restent ceux du client. |
| Points identiques non consécutifs (aller-retour, boucle) | Conservés. Une même portion reçoit les mêmes valeurs dans les deux sens. |
| Parcours qui commence ou finit sur un pont | Corrigé avec les extrémités **de l'ouvrage**, pas avec le premier point de la trace. |
| Passage sous un pont | `structure: ground`, altitude MNT. |
| Passage sous un pont puis sur ce même pont | Chaque passage a son niveau, décidé par la continuité de l'appariement. |
| Portion hors réseau (bridleway, chemin absent des tuiles, champ, plage, bac) | Segment `unknown`, `matched: false`, avertissement `unmatched_range`. `status` reste `complete`. |
| Portion sur une route motorisée fermée au vélo (autoroute, trunk en France, route `bicycle=no`) (api 1.1) | `matched: false`, `structure`, `surface` et `way_kind` de cette route, avertissements `unmatched_range` et `bicycle_forbidden` ; un pont ou un tunnel de cette route n'est pas corrigé (MNT) et n'entre pas dans `summary.structures[]`. `status` reste `complete`. Une rue piétonne ou un sentier `bicycle=no` restent appariés à la voie cyclable voisine. |
| Points d'entrée trop espacés pour suivre la voie | Ligne droite entre les points : appariement souvent impossible, donc `unknown` ; altitude lue sur la ligne droite. |
| Pont ou tunnel non tagué ou absent d'OSM | MVP : `structure: ground`, MNT brut, aucun avertissement. V1, sur terrain mesuré : ouvrage détecté par heuristique et **corrigé**, `structure_source: heuristic`, avertissement `structure_untagged`. |
| Pont sur le terrain LiDAR, MNS illisible | Interpolation entre culées, `dsm_unavailable`. Réponse 200. |
| Pont OSM sans tablier dans le MNS (au ras du sol, gué bétonné classé `bridge`) | Altitude du sol gardée, `ele_method: dem`, `summary.structures[].method: kept_ground`, compté comme corrigé. |
| Pont enchaîné à un tunnel | Un seul ouvrage corrigé, deux entrées contiguës dans `summary.structures[]` (une par type). |
| Ouvrage détecté par l'heuristique seule sur une portion non appariée (V1) | `structure: bridge` ou `tunnel`, `structure_source: heuristic`, `matched: false`, corrigé, `structure_untagged` et `unmatched_range`. |
| Tuiles IGN en échec | Relues dans Mapterhorn, `terrain_fallback`, `quality.terrain[].fallback_after_error`. Réponse 200. |
| Parcours partiellement hors emprise des données | Plages concernées en `elevation_missing` ou `typology_missing`, `status: partial`. |
| Parcours au-delà des bornes (plus de 200 km) | 413. Le client découpe le parcours ; un ouvrage coupé reste bien corrigé, les D+ des morceaux ne s'additionnent pas exactement. |
| Trou de no-data | Court : comblé (`gap_filled`). Long : `ele: null`, D+ marqué partiel. |
| Parcours hors France, en tout ou partie | Mapterhorn seul : pas de MNS, ponts interpolés. 200, avertissement `outside_primary_zone` sur les plages concernées. |
| Parcours transfrontalier hors France | Réponse normale, sans avertissement de référentiel vertical (non traité). |
| Points d'entrée espacés autour d'un ouvrage ou d'un sommet | Mode par défaut : points insérés aux extrémités de l'ouvrage et au sommet, avec leurs raisons. |
| `sections: ["elevation"]` | Pas de `segments[]` en sortie, mais la correction des ouvrages s'applique quand même. |
| `structure_correction: false` | `quality.structure_correction` à `disabled` pour les ponts et les tunnels, méthode `dem` partout. |

## 6. Versionnage et évolution

Trois versions toujours distinctes, toujours exposées dans `meta` :

- `api_version` : le contrat ;
- `rules_version` : traduction attributs → enums, règles de correction des ouvrages, réglages des
  heuristiques, adaptation du seuil de D+ sur GLO-30, seuils de sélection des points de rupture,
  valeurs par défaut ;
- `data_versions` : identifiant et date de chaque source.

Elles permettent d'expliquer pourquoi deux réponses diffèrent : nouvelles règles, ou nouvelles données.

**Idempotence** : `POST /v1/route-details` n'a aucun effet de bord. À requête identique et à
`api_version`, `rules_version` et `data_versions` identiques, la réponse est identique, hors
`request_id` et `duration_ms`. Un client ou le reverse proxy peut la mettre en cache avec ce triplet
dans la clé. Pour toute source lue en ligne (tuiles distantes en dev, IGN dans tous les modes), `data_versions`
n'est qu'indicatif (doc 01 §8.3) : deux réponses peuvent différer si l'IGN publie de nouvelles zones.

Historique : `1.1` (2026-11-01) ajoute l'avertissement `bicycle_forbidden` et la typologie d'une
portion qui suit une voie fermée au vélo (§4.3, §5.3), sans changer les champs existants.

Règles d'évolution à l'intérieur de la v1 :

- ajouter un champ ou un code d'avertissement **ne casse pas** le contrat. Les clients ignorent ce
  qu'ils ne connaissent pas ;
- les valeurs de `surface.class` sont **gelées** ;
- `surface.detail` est **ouvert** : un client doit tolérer une valeur inconnue et se replier sur
  `class` ;
- `structure` ne recevra pas de nouvelle valeur en v1 : `ford` est déjà réservé (produit en V2) ;
- la liste des raisons d'insertion (`reasons[]`) est **ouverte** : un client ignore une raison qu'il
  ne connaît pas ;
- les paramètres de mode et de type de trace (V2) arriveront en options facultatives, avec un défaut
  identique au comportement v1 ;
- une rupture donne une `/v2`, avec en-têtes `Deprecation` et `Sunset` et une période de
  recouvrement.

## 7. Limites

| Limite | Valeur v1 |
|---|---|
| Nombre de points, longueur du parcours | Bornes publiées dans `/v1/capabilities` : **200 km** et 16 000 points (bornes d'osm-surface). Elles ne portent que sur l'**entrée** (points d'entrée après dédoublonnage, longueur) : au-delà, 413, découpage par le client ; un endpoint asynchrone pourra venir plus tard, hors v1. |
| Taille de la sortie | **Non bornée en points** : le mode `distance` au pas minimal de 5 m dépasse 16 000 points (≈ 40 000 sur 200 km) sans 413. La densification interne à 5 m découle des bornes d'entrée et n'est pas une borne. |
| Taille du corps de requête | Bornée. Les corps compressés sont acceptés. |
| Budget de temps par requête | **90 s** ; au-delà, 504 `timeout`. Dimensionné large sur une lecture IGN à froid (de l'ordre de 20 s pour 130 km sans cache) : usage personnel, latence à froid secondaire. Le délai du reverse proxy doit le dépasser. |
| Sources tierces | Aucune URL de tuile ni de source fournie par le client. |
| Concurrence | File d'attente bornée ; au-delà, 503 `overloaded`. |
| Couverture | France en qualité maximale ; ailleurs, Mapterhorn en mode dégradé signalé. Limitée par les sources configurées (extraits par bbox éventuels), décrite dans `/v1/capabilities`. |
| Sources publiques | VersaTiles et Mapterhorn publics : MVP et usage personnel, derrière un cache ; VersaTiles en fichier local en production. IGN Géoplateforme : en ligne dans tous les modes, derrière un cache (doc 01 §8.1). |

## 8. Usage par type de client

Le service n'est conçu pour aucun client particulier ; vcyclist en est indépendant (doc 01 §1).

- **Apps vélo et rando, usage personnel d'abord** : le mode par défaut (`input_with_breakpoints`)
  donne un profil fidèle sans pas fixe, avec les sommets, creux et extrémités d'ouvrage déjà placés ;
  `summary` et `segments[]` servent à l'affichage (barre de revêtement, profil avec les ouvrages
  signalés). Les codes sont traduits par le client. La sortie GeoJSON pour la carte arrive en V1.x.
- **Client qui enrichit sa trace point par point** : `sampling=input`, une valeur par point d'entrée,
  indices conservés, sans insertion.
- **Client qui trace un graphique à pas régulier** : `sampling=distance`.
