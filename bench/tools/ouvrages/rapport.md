# Corpus d'ouvrages : rapport (30/10/2026)

Livrable : `ouvrages.yaml`, 36 entrées. Dans le même dossier : les requêtes (`q*.overpass`), les réponses brutes (`raw/`) et les scripts (`geo.py`, `cross.py`, `build.py`, `ign.py`, `emit.py`).

## Ouvrages

- **Grands ponts bombés** : stn-pont (Saint-Nazaire), normandie, chevire (non cyclable, bicycle=no), re-pont (île de Ré, côtier).
- **Mal tagués** : stn-acces-nord (≈1,06 km nommé « Pont de Saint-Nazaire », sans bridge ni layer ; MNH LiDAR 19,7 m au milieu, donc l'ouvrage est bien en hauteur). mauves : ways 1371341883/4 avec bridge:structure=truss mais sans bridge. Autres incohérences relevées : normandie 220122726 porte à la fois bridge et embankment ; stn-pont est tagué suspension alors que le pont est haubané ; combe-laval 1279044208 et voie-verte-24 668765879 sont tunnel=yes sans layer.
- **Passage supérieur / passage dessous** : rotonde-dessus et rotonde-dessous (Allée Baco). chevire-dessous : au croisement, MNT 6,9 m contre MNS 57,0 m.
- **Ponts contigus ou successifs** : reze-ponts-successifs (Sèvre puis Pirmil, à environ 300 m), mauves (2 ponts séparés par une île), stn-ecluses (3 ponts mobiles), nantes-haudaudine et nantes-audibert (ways contiguës), krk (2 arcs).
- **Ponts de Nantes** : bellevue, senghor, tabarly (layer=2), anne-de-bretagne (highway=construction), pirmil, briand.
- **Viaduc haut cyclable** : recoumene (voie verte, Haute-Loire, MNH 60,8 m). La Souleuvre est écartée : la requête n'y trouve aucune way highway sur le viaduc.
- **Tunnels Vercors** : petits-goulets (série), grands-goulets-tunnel (route actuelle, 1,74 km), grands-goulets-ancienne (route fermée, access=no), combe-laval, moines, bourne-arbois (avec une galerie pare-avalanche à côté), rousset, echevis (pont).
- **Ouvrage couvert** : arly-galerie (tunnel=avalanche_protector).
- **Tunnels cyclables longs** : croix-rousse (1,76 km), voie-verte-24 (≈470 m ; la way 668765888 de vcyclist a été scindée, la suite est 668765879).
- **Hors LiDAR HD** : liege-belle-liegeoise (BE), venise-liberta (IT), donostia-zurriola (ES), krk (HR).
- **Trace qui commence sur un pont** : re-depart-sur-pont, scénario synthétique au milieu du tablier.

Les renvois vers vcyclist (`truth/structures.csv` tests 1 à 4, `truth/tunnels/spans.csv`) sont dans `renvoi_vcyclist`. Leurs valeurs n'ont pas été recopiées.

## Ce qui a été vérifié (interrogé)

- **OSM** : tags et géométrie de chaque way citée (Overpass, base du 30/10/2026). Les longueurs et les milieux sont calculés à partir de cette géométrie. Les croisements « dessous » viennent d'un calcul d'intersection de segments sur la géométrie Overpass, sans nœud commun.
- **IGN** : 115 points interrogés sur `ign_rge_alti_wld` et `ign_lidar_hd_mnx_multi_wld` (measures=true : MNT, MNS et MNH LiDAR HD à 50 cm). Hors France, tous les points renvoient -99999. Au milieu du pont de Ré, le RGE ALTI renvoie -99999 (mer) mais le LiDAR HD répond.
- **MNS** : il existe bien sur la même API. `GET https://data.geopf.fr/altimetrie/resources` liste 9 ressources, dont `ign_lidar_hd_mnx_mono_wld` et `ign_lidar_hd_mnx_multi_wld` (« MNT, MNS et MNH »). Le MNS se lit avec `zonly=false&measures=true`. La valeur `z` par défaut est le MNT. `/altimetrie/1.0/calcul/resources` renvoie 405.
- **Mapterhorn** : requêtes HEAD sur les tuiles z12/z13. Krk : z13 en 404 aux deux milieux, donc GLO-30 seul selon la règle de l'exploration 05. Ré, Venise, Saint-Sébastien, Liège et Saint-Nazaire : z13 en 200, ce qui n'établit pas la source pixel par pixel.

## Ce qui reste incertain

- **Le MNS n'est pas la chaussée.** Il mesure aussi les garde-corps, les véhicules, la végétation et les falaises (combe-laval B : MNS 993 m, MNT 907 m). Les valeurs servent de références indicatives, pas de vérité terrain.
- **Écarts entre RGE ALTI et LiDAR MNT** de 20 à 55 m aux portails de bourne-arbois (falaise). Aucune des deux sources n'a été validée.
- **Emprise de `ign_lidar_hd_mnx_mono_wld`** : ses métadonnées déclarent une bbox au sud limitée à 45,88° N, ce qui exclurait le Vercors. Je n'ai interrogé que la ressource `multi`, qui répond bien dans le Vercors. Le comportement de `mono` au sud n'a pas été testé.
- **Accès vélo réel** non vérifié : Tancarville est écarté ; Krk n'a pas de tag bicycle ; Venise est cyclable seulement par la piste 340300416, la chaussée SR11 porte bicycle=no. Le pont de l'Abîme (way 38220155 : bridge sans highway, access=no) est écarté.
- **Cas côtier GLO-30** : Venise et Saint-Sébastien ne sont pas confirmés (z13 en 200). Krk est le seul cas fort, et il reste une inférence.
- **Longueur totale de Saint-Nazaire (3 356 m)** : chiffre de connaissance générale, non vérifié ici. Même chose pour la structure haubanée.
- **Points des chaînes discontinues** (petits-goulets, combe-laval, grands-goulets-ancienne) : le milieu peut tomber à ciel ouvert, entre deux tunnels.
- **Croisement de la Rotonde** : MNS de 14,2 m, alors que le milieu du tablier donne 10,4 m. L'écart n'est pas expliqué.

## Requêtes faites

**Overpass**, avec le User-Agent `osm-route-details-bench/0.1 (contact)`. Les requêtes en 502 (serveur saturé) ont été relancées après 20 à 90 s.

- q1 : ponts de Loire-Atlantique (3 bbox nommées + 13 ids vcyclist).
- q2 : ways à moins de 3 m de 8 ponts (calcul des croisements).
- q3 : Normandie, Tancarville, Ré, Vercors, Abîme, Caille, Souleuvre, Venise, Liège, Hérault + 7 ids vcyclist.
- q4 : Dordogne, Abîme, Liège, Venise, Saint-Sébastien.
- q5 : pont de l'Abîme + cycleway/path avec bridge=viaduct de plus de 250 m (bbox France).
- q6 : ways sur le contour de la Belle Liégeoise.
- q7 : Krk et Rio–Antirrio (ce dernier est écarté : autoroute, trottoirs bicycle=no).

**IGN** :

- `/altimetrie/resources`, `/altimetrie/resources/ign_lidar_hd_mnx_mono_wld`, `/1.0/calcul/resources` (405), `/1.0/calcul/resources/ign_rge_alti_wld` (500).
- 4 tests ponctuels, puis 8 requêtes `elevation.json` par lots de 30 points au plus, espacées de 0,6 s.

**Mapterhorn** : 15 requêtes HEAD de tuiles.

Aucune écriture hors de ce dossier, aucun dépôt modifié.
