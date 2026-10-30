# 04 — Sources d'élévation libres (exploration)

> Statut : exploration, aucune ligne de code. Recherche faite le 30/10/2026 sur les docs officielles,
> les dépôts de code et des requêtes directes aux services (Mapterhorn, Géoplateforme IGN).
> Les points notés **(non vérifié)** viennent d'une connaissance générale et doivent être confirmés
> dans la doc du producteur avant d'être utilisés.

## 0. Rappels utiles

- **MNT** (modèle numérique de terrain, *DTM*) : sol nu. Sur un pont, il donne le sol, la rivière ou la route
  *sous* le tablier. Sur un tunnel, il donne le relief *au-dessus*.
- **MNS** (modèle numérique de surface, *DSM*) : la première surface rencontrée (toits, canopée, tabliers de ponts).
  Il est correct sur un pont, mais faux sous les arbres (sentier en forêt) et en ville.
- **MNH** = MNS − MNT (hauteur du sursol).
- **Référentiel vertical** : les altitudes nationales sont « orthométriques » et rattachées à un marégraphe
  (NGF-IGN69 = Marseille, NAP = Amsterdam, TAW = Ostende…). Les produits globaux utilisent un géoïde
  (EGM96, EGM2008). L'écart entre l'ellipsoïde WGS84 et le géoïde fait environ +45 à +55 m en France
  (en Espagne, l'ondulation « oscille autour de 50 m » selon l'[IGN espagnol](https://datos-geodesia.ign.es/geoide/EGM08-REDNAP_Spain_IGN.pdf)).
  Il ne faut donc **jamais** mélanger des hauteurs ellipsoïdales GPS brutes avec un MNT. Entre deux
  référentiels orthométriques européens, l'écart va de quelques cm à quelques dm. Exception notable :
  la Belgique TAW est **2,33 m** sous le NAP néerlandais ([VLIZ](https://vliz.be/imisdocs/publications/259880.pdf)).

---

## 1. Mapterhorn (agrégateur mondial)

Site : <https://mapterhorn.com> · code : <https://github.com/mapterhorn/mapterhorn> (BSD-3) ·
financement NLnet/NGI0, hébergement soutenu par Cloudflare (R2, Workers).

| Aspect | Constat |
|---|---|
| Nature | **MNT uniquement** pour les sources nationales (DGM, MNT, DTM). Le fond mondial est **Copernicus GLO-30, qui est un MNS**. |
| Sources | 151 entrées dans [attribution.json](https://download.mapterhorn.com/attribution.json) : GLO-30 (30 m) monde ; **France : MNT LiDAR HD 0,5 m + RGE ALTI 1 m/5 m** (métropole, Corse, DROM) ; swissALTI3D 0,5 m ; tous les Länder allemands en DGM1 1 m (BW 0,25 m) ; Autriche BEV 1 m ; Flandre 1 m, Wallonie 0,5 m ; Luxembourg 0,5 m ; Pays-Bas AHN5 5 m ; Espagne MDT02/MDT05 et un MDT 0,5 m ; Portugal 0,5 m ; Italie régionale 1 à 10 m + TINITALY 10 m ; UK (Angleterre, Écosse, Pays de Galles) 1 m ; Scandinavie, Pologne, Tchéquie, Slovénie, Slovaquie, Roumanie ; USGS 3DEP 1 m/10 m ; etc. |
| Format | Tuiles raster **WebP 512 px, encodage Terrarium**, `h = R·256 + G + B/256 − 32768`. Reprojetées en Web Mercator ([pipelines/README](https://github.com/mapterhorn/mapterhorn/blob/main/pipelines/README.md)). |
| Zooms | `planet.pmtiles` couvre z0–z12. Les archives régionales `6-x-y.pmtiles` couvrent z13 et au-delà. Le zoom max est ajusté localement à la résolution de la source : z17 à z18 en France (pixel ≈ 0,6 m / 0,3 m). |
| Quantification verticale | Arrondi qui dépend du zoom : 1 m jusqu'à z11, 50 cm à z12, 25 cm à z13, 6 cm à z15, 1,6 cm à z17. Il faut donc lire au zoom natif pour profiter de la précision. |
| Fusion | Priorité à la source au maxzoom le plus élevé. Les nodata sont comblés par la source suivante, avec un **flou gaussien le long de la couture**. |
| Accès | API XYZ `https://tiles.mapterhorn.com/{z}/{x}/{y}.webp` ([TileJSON](https://tiles.mapterhorn.com/tilejson.json)). Téléchargement PMTiles complet ou extrait par bbox (`pmtiles extract`, `--dry-run` pour connaître la taille) ([data-access](https://mapterhorn.com/data-access)). Pas de quota publié. |
| Volumétrie (v0.0.13, [download_urls.json](https://download.mapterhorn.com/download_urls.json)) | Total ≈ **9,9 To**. `planet.pmtiles` (z0–12) ≈ **356 Go**. Les 9 archives z13+ qui touchent la France ≈ **2,1 To** (elles débordent sur les voisins). |
| Licence | Code BSD-3. Données : chaque source garde sa licence (Etalab 2.0 pour l'IGN, Copernicus, CC BY 4.0, OGL, DL-DE…). L'attribution est obligatoire et la liste est longue. |
| Référentiel vertical | **Aucune conversion** : chaque source reste dans son datum national, et GLO-30 en EGM2008. Le mainteneur le confirme dans [l'issue #255 « Vertical references »](https://github.com/mapterhorn/mapterhorn/issues/255). Les marches aux frontières sont seulement adoucies par le flou. |
| Mises à jour | Recalcul incrémental (seules les tuiles d'agrégation modifiées sont refaites) et versionné dans le CHANGELOG. Pas de cadence fixe. |

**Conclusion Mapterhorn** : c'est le meilleur « tout-en-un » libre aujourd'hui. Il intègre déjà le LiDAR HD
français, et le format Terrarium/PMTiles se lit très bien depuis Rust (crate `pmtiles` + décodage WebP).
Ses limites : pas de MNS, pas de correction de datum, une quantification qui dépend du zoom, et un fond
mondial qui est un MNS (GLO-30).

---

## 2. IGN France

### 2.1 LiDAR HD (programme national, 2020 → 2026)

- **Produits dérivés** :
  - **MNT** au pas de 50 cm, calculé à partir des points sol, eau et des « points virtuels ».
  - **MNS** au pas de 50 cm, qui inclut végétation, bâti et **classe « tablier de pont »** (code 17).
  - **MNH** au pas de 50 cm.
  - Une version 5 m est annoncée « prochainement ».
  - Sources : [MNT](https://www.data.gouv.fr/datasets/mnt-lidar-hd/informations),
    [MNS](https://www.data.gouv.fr/datasets/mns-lidar-hd/informations),
    [MNH](https://www.data.gouv.fr/datasets/mnh-lidar-hd/informations).
- **Nuage de points** : LAS/LAZ classé ASPRS, densité ≥ 10 impulsions/m². Depuis le descriptif de 2023,
  la classe 17 est devenue « Tablier de pont » ([DC LiDAR HD](https://geoservices.ign.fr/sites/default/files/2024-09/DC_LiDAR_HD_1-0.pdf)).
- **Précision** : visée de 10 cm en altimétrie et 50 cm en planimétrie (fiches data.gouv).
  Le référentiel est **Lambert 93 / NGF-IGN69** en métropole (fiche : « RGF93-CC49 / IGN69 »).
- **Format** : GeoTIFF float32, dalles de 1 km × 1 km. Une dalle MNS de 50 cm pèse environ 24 Mo.
  Les zones **ZICAD** (interdites à la prise de vue) sont en nodata.
- **Couverture** : au 6 septembre 2026, le nuage publié couvre **78 % de la surface communale** et 89 %
  des communes à ≥ 99 % ([carte par commune](https://www.data.gouv.fr/datasets/carte-du-lidar-hd-de-lign-par-commune-date-du-vol-et-couverture-en-france)).
  La Guyane n'est pas dans le programme. Les MNx suivent le nuage avec du retard.
  La fiche indique « mis à jour le 30/09/2026 ». Le programme n'est **pas terminé** : il faut un repli.
- **Volumétrie France métro (estimation)** : environ 550 000 dalles de 1 km². À environ 16 à 24 Mo par dalle,
  cela fait **≈ 9 à 13 To par produit** (MNT ou MNS). Ce n'est pas réaliste à embarquer tel quel.
- **Licence** : Licence Ouverte Etalab 2.0 (réutilisation commerciale permise, attribution IGN).

### 2.2 RGE ALTI® (1 m, 5 m) et BD ALTI® (25 m)

- **RGE ALTI** : MNT à 1 m, dérivé à 5 m. Il mélange plusieurs sources, d'où une **précision hétérogène**
  ([fiche GEE](https://developers.google.com/earth-engine/datasets/catalog/IGN_RGE_ALTI_1M_2_0),
  [DC RGE ALTI 2.0](https://geoservices.ign.fr/sites/default/files/2021-07/DC_RGEALTI_2-0.pdf)) :
  - LiDAR (zones inondables, littoral, grandes forêts) : 0,2 à 0,5 m ;
  - corrélation d'images aériennes : 0,5 à 0,7 m ;
  - **radar en montagne (Alpes, Pyrénées, Cévennes, Corse) : erreur moyenne de l'ordre de 7 m sur les fortes pentes** ;
  - bandes `SRC` (source) et `DST` (distance au point mesuré) disponibles pour qualifier chaque pixel.
- Format : ASCII Grid, 1000 × 1000 nœuds par dalle, système LAMB93-IGN69 (IGN78 en Corse).
  Téléchargement par département. Licence Etalab 2.0. Le produit est figé (2009–2021) et le LiDAR HD le remplace.
- **BD ALTI** : pas de 25 m (et 75 m, 250 m), ASCII Grid ([DC BD ALTI](https://geoservices.ign.fr/sites/default/files/2021-07/DC_BDALTI_2-0.pdf)).
  Il n'a plus d'intérêt face au RGE ALTI 5 m.

### 2.3 Accès par services (Géoplateforme)

- **Service de calcul altimétrique REST** ([doc](https://cartes.gouv.fr/aide/fr/guides-utilisateur/utiliser-les-services-de-la-geoplateforme/calcul-altimetrique/)) :
  - Base : `https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/`.
  - Endpoints : `elevation.json` (points) et `elevationLine.json` (profil, `sampling` de 2 à 5000,
    renvoie aussi le dénivelé positif et négatif).
  - Limites : **5 000 points par requête, 5 requêtes/s par IP**, pas de clé requise (testé).
    Nodata = `-99999`, valeurs arrondies au cm.
  - Ressources listées par `/altimetrie/resources` (testé) : `ign_rge_alti_wld`,
    `ign_rge_alti_par_territoires`, `ign_lidar_hd_mnt_{mono|multi}_wld`, `ign_lidar_hd_mnx_{mono|multi}_wld`.
  - **Point intéressant** : avec `resource=ign_lidar_hd_mnx_multi_wld&measures=true`, une seule requête
    renvoie **MNT, MNS et MNH** au même point. Par exemple à Paris : MNT 35,67 m, MNS 60,88 m, MNH 25,2 m.
  - Cela convient au prototypage et à de faibles volumes. Un service de production dépendrait alors
    d'un tiers limité à 5 req/s.
- **WMS-R / WMTS** (`data.geopf.fr/wms-r`, `/wmts`) :
  - couches `IGNF_LIDAR-HD_MNT|MNS|MNH_ELEVATION.ELEVATIONGRIDCOVERAGE.{LAMB93,WGS84G}` et
    `ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES` (RGE ALTI) ;
  - certaines couches sont servies en `image/x-bil;bits=32` (float brut), donc exploitables comme
    tuiles de données et pas seulement d'affichage (vérifié dans les GetCapabilities).
- **Téléchargement des dalles** : interface cartes.gouv.fr, ou index de dalles WFS/WCS. Le dépôt
  [wipfli/frhd-download](https://github.com/wipfli/frhd-download), utilisé par Mapterhorn, prévient que
  certaines dalles reviennent avec des rectangles nodata et doivent être retéléchargées.

---

## 3. Autres MNT nationaux libres haute résolution (Europe, non exhaustif)

Les résolutions et licences viennent du catalogue Mapterhorn
([attribution.json](https://download.mapterhorn.com/attribution.json)), recoupé avec les sites producteurs.

| Pays | Produit | Rés. | Précision Z | Réf. vertical | Format / accès | Licence |
|---|---|---|---|---|---|---|
| Suisse | [swissALTI3D](https://www.swisstopo.admin.ch/en/height-model-swissalti3d) (MNT, sans végétation ni bâti) | 0,5 / 2 m | 0,3 m (LiDAR récent), 1 à 3 m au-dessus de 2000 m | LN02 | **COG** en dalles de 1 km (≈ 43 500), XYZ. Mis à jour tous les 6 ans | OGD swisstopo |
| Espagne | MDT02 / MDT05 / MDT 0,5 m (PNOA, CNIG) | 0,5 à 5 m | (non vérifié) | Orthométrique REDNAP (géoïde EGM08-REDNAP) | GeoTIFF par feuille, [WCS INSPIRE](https://inspire-geoportal.ec.europa.eu/srv/api/records/spaignMDT05) | CC BY 4.0 |
| Pays-Bas | [AHN](https://www.ahn.nl/kwaliteitsbeschrijving) MNT (« maaiveld ») et MNS (« ruw ») | 0,5 m (AHN5 5 m dans Mapterhorn) | ≤ 5 cm syst. + 5 cm stoch. (95 % < 15 cm) | NAP (≈ EVRF, −5 mm) | GeoTIFF, LAZ, WMS/WCS/WMTS | CC BY 4.0 (catalogue Mapterhorn) |
| Belgique | DHM Vlaanderen II (1 m) ; MNT Wallonie (0,5 m) | 0,5 à 1 m | (non vérifié) | **TAW/DNG, −2,33 m par rapport au NAP** | GeoTIFF | Licence libre Flandre ; CC BY 4.0 Wallonie |
| Luxembourg | LiDAR 2019 MNT | 0,5 m | (non vérifié) | (non vérifié) | GeoTIFF | CC0 |
| Allemagne | DGM1 des 16 Länder | 1 m (BW 0,25 m) | < ±0,3 m ([BKG](https://sg.geodatenzentrum.de/web_public/gdz/dokumentation/deu/dgm1.pdf)) | DHHN2016 (DHHN92 selon le Land) | GeoTIFF/XYZ par Land | DL-DE Zero ou BY 2.0, CC BY 4.0 (varie selon le Land) |
| Autriche | BEV ALS-DGM | 1 m (Burgenland 5 m) | (non vérifié) | GHA (Adria, non vérifié) | GeoTIFF | CC BY 4.0 |
| Italie | Pas de LiDAR national libre : DTM régionaux (Bolzano, Trento, Aoste, Lombardie, Piémont, Sicile…) + TINITALY | 1 à 10 m | hétérogène | Genova 1942 (non vérifié) | GeoTIFF | CC BY / CC0 selon la région |
| Royaume-Uni | EA LiDAR Composite DTM, Scotland, Wales | 1 m | (non vérifié) | ODN Newlyn (non vérifié) | GeoTIFF | OGL |

Agrégat tiers à signaler : [Sonny's LiDAR DTM of Europe](https://sonny.4lima.de/). Il fournit des `.hgt`
à 1″ et 3″ (et du 20 m / 50 m) par pays, en **CC BY 4.0, usage commercial autorisé**. BRouter l'utilise.
C'est pratique comme fond européen « LiDAR rééchantillonné » (≈ 4,8 Go à 1″ pour l'Europe), mais pas
toujours issu du LiDAR et de datum non documenté.

---

## 4. Modèles globaux

| Source | MNT/MNS | Rés. | Précision Z | Réf. vertical | Accès | Licence |
|---|---|---|---|---|---|---|
| [Copernicus GLO-30 / GLO-90](https://dataspace.copernicus.eu/explore-data/data-collections/copernicus-contributing-missions/collections-description/COP-DEM) | **MNS** (bâti, végétation) | 1″ / 3″ | abs. < 4 m (LE90), rel. < 2 m (pente ≤ 20 %) | **EGM2008** | **COG sur AWS** `s3://copernicus-dem-30m`, `-90m` ([registry](https://registry.opendata.aws/copernicus-dem/)). Dalles de 1°. Release 2024_1 | Licence Copernicus libre, commerciale OK, mention de source obligatoire |
| SRTM v3 / CGIAR v4.1 / NASADEM | **MNS** (bande C, pénétration partielle du couvert) | 1″ (≈ 30 m), 60°N à 56°S | ≈ 6 à 16 m (LE90, selon les études) | **EGM96** | `.hgt` (USGS EarthExplorer, CGIAR) | Domaine public US (SRTM). CGIAR a ses propres conditions |
| [AW3D30](https://www.eorc.jaxa.jp/ALOS/en/dataset/aw3d30/aw3d30_e.htm) (JAXA) v4.1 | **MNS** | 1″ | (validations externes) | « hauteur au-dessus du niveau de la mer » (EGM96, non vérifié) | GeoTIFF par dalle de 1°, **inscription requise** | Gratuit, usage commercial autorisé |
| [FABDEM](https://gee-community-catalog.org/projects/fabdem/) V1-2 | **MNT** (GLO-30 sans bâti ni forêt par apprentissage automatique) | 1″ | meilleur que GLO-30 en forêt et en ville | EGM2008 (hérité) | GeoTIFF par dalle de 1° | ⚠️ **CC BY-NC-SA 4.0 : usage commercial interdit**, copyleft. Contact Fathom pour un usage commercial |
| [Terrain Tiles AWS (Mapzen/Joerd)](https://registry.opendata.aws/terrain-tiles/) | Mixte (SRTM et GMTED = MNS ; NED, EUDEM, UK 2 m, Autriche 10 m…) | z0 à z15, ≈ 30 m en Europe (EUDEM) | hétérogène | mixte, non harmonisé | `s3://elevation-tiles-prod(-eu)`, formats terrarium / normal / geotiff / skadi (`.hgt`) | Attributions multiples ([joerd](https://github.com/tilezen/joerd/blob/master/docs/data-sources.md)). **Plus maintenu**, artefacts aux côtes et sur le bâti. Mapterhorn le remplace |

Point clé : **tous les globaux libres et commerciaux sont des MNS**. Le seul MNT global (FABDEM) est NC.
Hors des pays qui ont un MNT LiDAR, une route en forêt peut ressortir 10 à 20 m trop haut, et une
traversée de forêt peut ajouter du faux dénivelé.

---

## 5. Ponts et tunnels : MNT vs MNS en pratique

| Cas | MNT | MNS | Ce qui marche |
|---|---|---|---|
| Pont (viaduc, passerelle) | sol ou rivière sous le tablier : creux artificiel de 10 à 100 m | tablier correct, mais bruité par les garde-corps, le trafic, les arbres aux abords | interpolation entre les culées. Ou MNS **restreint au tracé du pont** |
| Tunnel | relief au-dessus : bosse artificielle (parfois des centaines de m) | idem, et pire | **interpolation linéaire entre les portails** (seule option raisonnable) |
| Passage inférieur (la route passe *sous* un pont) | correct | faux (on lit le tablier du dessus) | MNT |
| Forêt, ville | correct | faux de +10 à +30 m | MNT |

Pratiques observées :

- **GraphHopper** : `EdgeElevationInterpolator`
  ([javadoc](https://javadoc.io/static/com.graphhopper/graphhopper-core/stable/com/graphhopper/reader/dem/EdgeElevationInterpolator.html)).
  Les nœuds qui ne touchent que des arêtes pont ou tunnel sont « internes ». Leur altitude est interpolée
  depuis les nœuds « externes », et les nœuds pilier linéairement selon la distance. C'est activé par
  défaut depuis la 0.13. Exemple publié : un dénivelé ramené de 1395 m à 142 m
  ([blog 2019](https://www.graphhopper.com/blog/2019/07)).
- **Valhalla** : `elevationbuilder.cc`
  ([source](https://github.com/valhalla/valhalla/blob/master/src/mjolnir/elevationbuilder.cc)).
  - Sur les arêtes `bridge`, `tunnel` ou ferry, il n'échantillonne que **le début et la fin**. L'altitude
    est « interpolée linéairement entre les nœuds » au runtime.
  - La pente des ponts et tunnels est **bornée à ±3 %**.
  - L'échantillonnage se fait tous les 60 m.
  - Les données viennent des tuiles Skadi (`.hgt`) de Terrain Tiles, soit ≈ 1,6 To pour le monde.
- **BRouter** ([elevation](https://brouter.de/brouter/elevation.html)) : SRTM CGIAR v4.1, LiDAR de Sonny
  au-delà de 60°N, puis rasters 1″ « BEF » avec LiDAR de Sonny et repli SRTM 3″
  ([PR #644](https://github.com/abrensch/brouter/pull/644)). Le dénivelé est filtré par une hystérésis
  dont le seuil dépend de la source : environ 10 m pour SRTM, environ 5 m pour le LiDAR. Le traitement
  des ponts et tunnels n'est pas documenté. Une discussion mkgmap indique que BRouter, cycle.travel et
  OsmAnd ignorent le relief au-dessus des tunnels.
- **geOps** ([blog](https://geops.com/en/blog/interpolation-of-height-profiles)) : SRTM en bilinéaire.
  Les ponts et tunnels OSM sont regroupés en « réseaux ». Le tracé est projeté sur le réseau puis
  interpolé linéairement entre les extrémités connues. Si aucune extrémité n'est connue, un système
  linéaire minimise les pentes. Des conflits (passage sous un pont, polygones qui se chevauchent) sont
  résolus par la distance aux entrées.
- **Strava, Komoot** : rien d'officiel trouvé. Les forums décrivent des profils faux sur les ponts et
  tunnels. RideWithGPS propose un outil manuel « Flatten Elevation ».

**Idée propre à ce projet** : en France, le LiDAR HD offre le MNS (avec la classe tablier de pont) **et** le MNT.
Sur un segment OSM `bridge=*`, on peut prendre le **MNS échantillonné sur l'axe** et le comparer à
l'interpolation des culées (prises sur le MNT). Si l'écart est faible, on garde le MNS (cela capture le
bombé et les rampes). Sinon, on interpole. Pour `tunnel=*`, `covered=yes` et `layer<0`, on interpole
toujours. Partout ailleurs, on lit le MNT.

---

## 6. Stratégie multi-résolution

1. **Pile de sources par priorité** (la plus fine d'abord) :
   LiDAR HD MNT 0,5 m → RGE ALTI 1 m → MNT nationaux voisins → GLO-30 → GLO-90.
   C'est exactement ce que fait Mapterhorn.
2. **Harmonisation verticale vers un référentiel unique**, de préférence EGM2008 ou EVRF2007 pour l'Europe :
   - on applique une grille de conversion par source (PROJ : `IGN69 → EGM2008` via RAF20 et la grille EGM) ;
   - **à défaut**, on documente les biais, de quelques dm partout sauf en Belgique TAW (−2,33 m) ;
   - Mapterhorn ne le fait pas : il faut le savoir si on l'adopte.
3. **Transitions entre sources** :
   - un saut de 0,3 à 2 m à une couture crée un faux D+ et une fausse pente sur 1 pixel ;
   - remèdes : le flou de couture (Mapterhorn), le lissage du profil final (moyenne glissante ou
     Savitzky-Golay sur 20 à 50 m), et un D+ filtré par hystérésis à seuil adapté à la source
     (5 m LiDAR / 10 m SRTM, comme BRouter) ;
   - **renvoyer la source et la précision par point** dans la réponse, à la manière du champ `acc`
     de l'API IGN.
4. **Échantillonnage** : rééchantillonner le tracé à un pas proche de la résolution de la source
   (≈ 1 à 5 m en LiDAR, ≈ 30 m en GLO-30) avec une interpolation bilinéaire. Valhalla échantillonne à 60 m,
   ce qui est trop grossier pour le vélo et la rando sur LiDAR.

---

## 7. Recommandation

**Primaire (France et Europe de l'Ouest) : Mapterhorn auto-hébergé**
- Extraits PMTiles par bbox : France ≈ 2 To à z17/18, ou beaucoup moins si on s'arrête à z15 ou z16.
  À z15, le pixel fait ≈ 2,4 m et la quantification 6 cm, ce qui suffit pour le vélo et la rando.
- Il intègre déjà le LiDAR HD MNT, le RGE ALTI et les MNT nationaux voisins, avec un format unique
  (Terrarium WebP) lisible directement en Rust.
- L'API publique `tiles.mapterhorn.com` sert au développement uniquement.

**Repli mondial** : déjà dans Mapterhorn (GLO-30). Sinon, des COG Copernicus GLO-30 lus en *range requests*
sur AWS. On accepte que ce soit un MNS : on l'indique dans la réponse et on augmente le seuil de filtrage du D+.

**MNS LiDAR HD (France) pour les ponts uniquement**
- Prototype : l'API `alti` IGN (`ign_lidar_hd_mnx_multi_wld`, MNT et MNS en un appel, 5 req/s) suffit.
- Ensuite : télécharger à la demande les dalles MNS 1 km qui intersectent les `bridge=*` OSM, ou utiliser
  le WMTS BIL 32 bits. Le MNS complet n'est **pas** à embarquer (≈ 10 To).

**Règles d'usage MNT/MNS**
- MNT par défaut.
- Tunnels : interpolation entre les portails.
- Ponts : interpolation entre les culées. Si le MNS est disponible et cohérent, on le prend sur l'axe.
- Bornage des pentes interpolées à quelques pourcents, comme Valhalla.

**À exclure** : FABDEM (non commercial et copyleft), Terrain Tiles AWS (plus maintenu), BD ALTI 25 m (obsolète).

## 8. Risques

- **Volumétrie** :
  - Mapterhorn monde ≈ 9,9 To, France z13+ ≈ 2,1 To ;
  - LiDAR HD brut ≈ 10 To par produit ;
  - il faut prévoir un plafond de zoom, des extraits par bbox et un cache disque. Les *range requests*
    HTTP sur PMTiles et COG évitent de tout stocker.
- **Licences** :
  - attribution multiple obligatoire (≈ 150 sources Mapterhorn : Etalab, Copernicus, CC BY, OGL, DL-DE…).
    Il faut exposer un endpoint `/attribution` ;
  - FABDEM est NC ;
  - AW3D30 demande une inscription ;
  - les licences allemandes varient selon le Land.
- **Référentiels** :
  - mélange NGF-IGN69 / IGN78 / NAP / TAW / LN02 / DHHN / EGM2008 / EGM96 sans conversion dans Mapterhorn ;
  - des marches aux frontières (Belgique ≈ 2,3 m) ;
  - si le service compare à des altitudes GPS, se méfier de l'ellipsoïdal (≈ +50 m).
- **Couverture LiDAR HD incomplète** (≈ 78 % de la surface publiée en septembre 2026), zones ZICAD en nodata,
  et un RGE ALTI de repli à **±7 m en montagne (radar)** : les cols alpins et pyrénéens risquent d'être
  les moins bons là où ils comptent le plus. Il faut vérifier, tuile par tuile, quelle source Mapterhorn
  a utilisée ([issue #350](https://github.com/mapterhorn/mapterhorn/issues/350) sur la provenance par pixel).
- **Dépendances externes** : l'API IGN (5 req/s, pas de SLA) et l'API Mapterhorn (pas de quota publié)
  ne doivent pas être des dépendances de production.
- **Qualité des tags OSM** : la logique ponts et tunnels dépend de `bridge`, `tunnel`, `layer` et du découpage
  des ways. Un pont mal tagué donne un creux de plusieurs dizaines de mètres dans le profil.
