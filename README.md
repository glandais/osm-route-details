# osm-route-details

Microservice Rust (à venir) : en entrée un parcours, en sortie l'**élévation** et la
**typologie** (structure : sol / pont / tunnel ; revêtement : enum simple) le long du parcours.

Aucun code pour l'instant : phase d'exploration puis de conception.

## Exploration

| Doc | Sujet |
|-----|-------|
| [01-vcyclist-elevation](docs/exploration/01-vcyclist-elevation.md) | Chaîne d'élévation de vcyclist (Mapterhorn, bilinéaire, lissage, D+) |
| [02-osm-surface-typologie](docs/exploration/02-osm-surface-typologie.md) | osm-surface : tuiles VersaTiles, map-matching HMM tiles-only, attributs |
| [03-sources-tuiles-vectorielles](docs/exploration/03-sources-tuiles-vectorielles.md) | VersaTiles, OSMF, OpenFreeMap, Protomaps, Planetiler sur mesure |
| [04-sources-elevation](docs/exploration/04-sources-elevation.md) | Mapterhorn, IGN (LiDAR HD, RGE ALTI, API alti), MNT nationaux, globaux |

### Synthèse

- **Élévation** : Mapterhorn (MNT, agrège déjà LiDAR HD / RGE ALTI en France, Terrarium WebP)
  est la source principale naturelle ; repli global Copernicus GLO-30 (MNS). Référentiels
  verticaux non harmonisés entre pays. vcyclist ne traite ni ponts/tunnels ni no-data.
- **Ponts/tunnels** : un MNT donne le sol sous le pont / le relief au-dessus du tunnel ;
  pratique courante (GraphHopper, Valhalla) = interpolation entre extrémités. Piste France :
  MNS LiDAR HD sur l'axe des ponts. La typologie OSM sert donc aussi à corriger l'élévation.
- **Typologie** : aucune source de tuiles publique ne fournit `surface` brut
  (VersaTiles/OpenFreeMap : paved/unpaved ; OSMF et Protomaps : rien). Pont/tunnel disponibles
  partout ; gué et `layer` seulement dans OpenMapTiles. Pour un revêtement fin : générer ses
  propres tuiles (Planetiler, schéma dédié). 25–60 % des ways sans `surface` → inférence par
  `highway`/`tracktype` nécessaire.
- **Map-matching** : le HMM tiles-only d'osm-surface est réutilisable ; les niveaux
  sol/pont/tunnel déconnectés évitent d'apparier la route sous un pont.
- **Licences** : ODbL (OSM), ~150 attributions Mapterhorn, FABDEM exclu (NC).
