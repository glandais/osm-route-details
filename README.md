# osm-route-details

Microservice Rust (à venir) : en entrée un parcours, en sortie l'**élévation** et la
**typologie** (structure : sol / pont / tunnel ; revêtement : enum simple) le long du parcours.

Aucun code pour l'instant : phase d'exploration puis de conception.

## Conception

| Doc | Sujet |
|-----|-------|
| [01-conception-generale](docs/design/01-conception-generale.md) | Vision, périmètre, composants, stratégie de données, enums, modes dégradés, trajectoire, exploitation, questions ouvertes |
| [02-contrat-interface](docs/design/02-contrat-interface.md) | Contrat d'API grosse maille : endpoints, entrée, sortie (points/segments/résumé/qualité/attribution), erreurs, versionnage |

## Exploration

| Doc | Sujet |
|-----|-------|
| [01-vcyclist-elevation](docs/exploration/01-vcyclist-elevation.md) | vcyclist 6.11 : IGN LiDAR HD + repli Mapterhorn, détection ponts (MNS) et tunnels, provenance, D+ |
| [02-osm-surface-typologie](docs/exploration/02-osm-surface-typologie.md) | osm-surface : tuiles VersaTiles, map-matching HMM tiles-only, attributs |
| [03-sources-tuiles-vectorielles](docs/exploration/03-sources-tuiles-vectorielles.md) | VersaTiles, OSMF, OpenFreeMap, Protomaps, Planetiler sur mesure |
| [04-sources-elevation](docs/exploration/04-sources-elevation.md) | Mapterhorn, IGN (LiDAR HD, RGE ALTI, API alti), MNT nationaux, globaux |

### Synthèse

- **Élévation** : en France, IGN LiDAR HD en direct (MNT par la Géoplateforme WMS-R, BIL
  float32, z15), comme vcyclist 6.11 ; Mapterhorn (Terrarium, qui sert d'ailleurs du LiDAR HD
  en France jusqu'à z17) en repli point par point et partout ailleurs, avec GLO-30 (MNS) comme
  fond mondial. Provenance du terrain exposée par tronçon. Référentiels verticaux non
  harmonisés entre pays. Dépendance au WMS-R (40 req/s/IP, pas de SLA) : cache disque et repli.
- **Ponts/tunnels** : un MNT donne le sol sous le pont / le relief au-dessus du tunnel.
  vcyclist les corrige déjà par heuristiques sur le LiDAR seul (MNS pour les tabliers, forme du
  MNT pour les tunnels), mais en rate beaucoup (hors échantillon : 4 ponts sur 11). Ici : OSM
  pour l'emprise et le type, LiDAR pour l'altitude (MNS sur le tablier dès le MVP, droite entre
  portails pour les tunnels) ; l'interpolation entre culées (GraphHopper, Valhalla) n'est qu'un
  repli, 22–40 m d'erreur sur les grands ponts bombés. Heuristiques portées en contrôle croisé
  et pour les ouvrages absents d'OSM (V1).
- **Typologie** : aucune source de tuiles publique ne fournit `surface` brut
  (VersaTiles/OpenFreeMap : paved/unpaved ; OSMF et Protomaps : rien). Pont/tunnel disponibles
  partout ; gué et `layer` seulement dans OpenMapTiles. Pour un revêtement fin : générer ses
  propres tuiles (Planetiler, schéma dédié). 25–60 % des ways sans `surface` → inférence par
  `highway`/`tracktype` nécessaire.
- **Map-matching** : le HMM tiles-only d'osm-surface est réutilisable ; les niveaux
  sol/pont/tunnel déconnectés évitent d'apparier la route sous un pont.
- **Licences** : ODbL (OSM), IGN LiDAR HD (Licence Ouverte Etalab 2.0), ~150 attributions
  Mapterhorn, FABDEM exclu (NC).
