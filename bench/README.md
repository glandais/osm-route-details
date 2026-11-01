# Banc qualité

Corpus, métriques et seuils d'acceptation d'osm-route-details, fixés **avant le code** pour que
chaque incrément du MVP ait une cible mesurable. Le contrat mesuré est celui de
`docs/design/02-contrat-interface.md` ; les décisions de conception sont dans
`docs/design/01-conception-generale.md` (§8.5, §10, §11).

| Fichier | Contenu |
|---------|---------|
| `manifest.yaml` | Une entrée par cas : fichier, région, ce qu'il teste, référence, licence, provenance, empreinte, série |
| `criteria.yaml` | Forme machine des seuils du §4 (brouillon, gelé avant le premier passage) |
| `data/ouvrages.yaml` | Corpus de 37 ouvrages (OSM way ids, tags, longueurs, MNT/MNS/MNH IGN aux extrémités et au milieu) |
| `data/ouvrages-geom.json`, `data/ouvrages-osm/` | Géométrie et réponses brutes Overpass / IGN du corpus d'ouvrages |
| `data/deck_baro_vs_lidar.csv` | Première mesure : baromètre corrigé vs MNS LiDAR au milieu des ouvrages traversés |
| `tools/` | Scripts d'extraction et de qualification (Python 3, bibliothèque standard ; PyYAML via `uv run --with pyyaml`) |
| `private/` | **Hors git** : streams Strava, traces GPX personnelles, table pseudo → identifiant Strava |

---

## 1. Corpus

### 1.1 Bancs de vcyclist (copiés dans `data/vcyclist/`)

vcyclist 6.11.0-1 (`4dfd6c5`, à jour le 2026-10-30), `docs/deck-strava-data/`, copié une fois sans
modification dans `data/vcyclist/` (Apache-2.0 ; `LICENSE`, `PROVENANCE.md` et `SHA256SUMS` à côté) :

- `test1..4.gpx` : Saint-Nazaire 10 km, Nantes 25 km (16 ponts de Loire), Mauves 10 km, Vercors
  111 km (4 tunnels dont les Grands Goulets, balcons). Altitude `<ele>` = fond Strava.
- `analysis/truth/testN.csv` : vérité au pas de 5 m (`T` MNT, `B` médiane MNS 25 m sur tablier, `N`
  droite entre portails, `S` Strava − offset sur balcon) ; `structures.csv` : 21 ponts, 4 tunnels,
  6 balcons, 15 ponts OSM non détectés.
- Témoins : ponts `p01..p17` + négatifs `n01..n16`, tunnels `th*`, `tt*` + négatifs `hn*`.
- Hors git : `demo/public/gpx/strava.gpx` (même commit), activité enregistrée de l'auteur en 2018
  (horodatage, fréquence cardiaque) : règle des traces perso (§1.3), même si vcyclist la publie.
  Régénérée dans `private/vcyclist/` par `tools/fetch_hors_git.py` et vérifiée par son sha256
  (`manifest.yaml`, section `hors_git`). Elle sert au test de méthode du D+ (714 / 637 m) ; datée de
  la période d'altitude dégradée, son baro n'est pas une référence.

Chiffres de référence (ligne de base à égaler, pas à reproduire) :

| Mesure | Valeur vcyclist |
|--------|-----------------|
| Ponts trouvés, calibration | 18/21 (manqués : écluse sud aval, Rotonde, Échevis) |
| Ponts trouvés, hors échantillon `p09..p17` | 4/11 (235/772 stations, 30 %) |
| Tunnels trouvés, hors échantillon | 6/7, 0 faux tunnel sur 4 négatifs |
| Tablier, critère | ≤ 2 m max (ponts nantais), ≤ 5 m (Saint-Nazaire, Cheviré) |
| Tablier Saint-Nazaire avant → après correction | −65,8 → −1,5 m |
| Tunnel, critère | Grands Goulets ≤ 5 m de la droite entre portails |
| RMSE hors ouvrage, Strava vs MNT après offset | 0,75–0,85 m (Loire-Atlantique), 2,31 m (Vercors, P95 4,58) |
| D+ `test4` | Strava 2 479, MNT brut 2 837, chaussée estimée 2 362 m |
| D+ MNT vs baromètre | +12 % (714 vs 637 m) |
| Mapterhorn z12 vs MNT LiDAR | 0,46–0,58 m RMS terrain ouvert ; GLO-30 2,8 m |

**Pièges** (à garder en tête en lisant nos résultats) :

1. La vérité `test*` est dérivée du **même MNT/MNS IGN** que celui que lira le service : elle mesure
   surtout l'échantillonnage et la correction, pas l'exactitude du MNT. Les 21 « ponts vrais » sont la
   sortie du détecteur Python de vcyclist ; les 15 ponts OSM `missed` gardent une vérité au MNT,
   donc fausse. → On ne juge **jamais** un tablier sur `truth/testN.csv` seul quand une trace baro
   ou une référence `data/ouvrages.yaml` existe.
2. Les séries hors échantillon de vcyclist (`p09..p17`, `th*`, `hn*`) sont **consommées** : pour nos
   heuristiques V1, ce sont des données de calibration. Notre série tenue à l'écart est la nôtre (§2).
3. Le manifeste `tunnel-judge.sha256` ne vérifie plus que les données (6 sources ont changé depuis).
4. Toujours citer la convention de pixel (coins ou centres) avec un chiffre issu de vcyclist.
5. L'IGN (Etalab 2.0) n'est pas mentionné dans vcyclist : l'attribution est à notre charge.

### 1.2 Éval d'osm-surface (copiée dans `data/osm-surface/`)

osm-surface (`44e5da6`, pas de branche amont configurée : fraîcheur non vérifiable). Trois parcours
autour de Nantes, 185,2 km (`testdata/*.gpx` + `*.route.json`). La référence est la classe de
revêtement **Stadia/Valhalla** calculée par tribly (`tracks[0].surfaces`), pas une vérité terrain.

| Parcours | Accord classe | Hors inconnu Stadia | Revêtu / non revêtu | Inconnu (nous) | Non appariés | Latence |
|----------|---------------|---------------------|---------------------|----------------|--------------|---------|
| graymond-ride-138 (90 km) | 86,0 % | 86,5 % | 98,1 % | 0,9 % | 3 pts | 77 ms |
| np-655-gravel (52 km) | 91,6 % | 91,5 % | 99,0 % | 1,3 % | 8 pts | 71 ms |
| np677-chill-gravel (43 km) | 93,5 % | 93,9 % | 99,3 % | 0,0 % | 0 pt | 55 ms |

Écarts expliqués à 59 % par la réduction Shortbread à `paved`/`unpaved`, 14 % par l'âge des
tuiles, ≈ 12 % par la référence elle-même. Copie (Apache-2.0, licence choisie par l'utilisateur le
2026-10-30) : GPX sans modification, `.route.json` expurgés en `*.reference.json` par
`tools/expurge_route_json.py` (équipe, URL tribly, médias, waypoints, tags retirés), rapport
d'évaluation, `LICENSE`, `PROVENANCE.md`, `SHA256SUMS`.

### 1.3 Traces barométriques personnelles (hors git)

**Acquisition.** Streams Strava de l'utilisateur (MCP Strava, lecture seule, 2026-10-30) :
`location`, `altitude`, `distance`, `time` à pleine résolution. Inventaire : 2 384 activités de plus
de 15 km (2005–2026), dont 2 285 avec départ en France.

**Détection de la période défectueuse.** Le rapport « D+ Strava / D+ MNT » sur les polylignes réduites
(`tools/strava_scan_dplus.py`) est trop bruité pour conclure (médiane mensuelle entre 1,0 et 1,9 selon
l'appareil). La qualification se fait donc sur le stream complet (`tools/qualify_baro.py`) : altitude
enregistrée − MNT RGE ALTI tous les 50 m, puis décalage (médiane), dérive (amplitude d'une médiane
glissante sur 5 km), bruit (1,4826 × MAD après retrait de la tendance) et p95. Sur 60 activités
échantillonnées :

| Période | Constat | Exemples |
|---------|---------|----------|
| 2014-06 | Déjà bruité (bruit 2,7 m, p95 8,9 m) | une seule sortie testée avant 2014-12 |
| 2014-12 → 2015-04 | Sain (bruit 0,7–0,8 m) | 2014-12, 2015-02, 2015-04 |
| **2015-05 → 2015-07** | **Baromètre KO** : D+ enregistré à 37–38 % du MNT, dérive jusqu'à 138 m | 2015-05-31 (242/644 m), 2015-07-26 (272/723 m) |
| 2015-08 → 2020-08 | Altitude dégradée, profil d'altitude GPS : bruit 2,5–9,6 m, p95 jusqu'à 25 m, D+ souvent gonflé | Ille-sur-Têt 2016 : bruit 7,5 m contre 1,2 m sur le même départ en 2023 ; Reyrevignes 2018 : 4,2 contre 1,7 m ; Puy-de-Dôme 2017 : 6,7 contre 0,7 m |
| ≥ 2020-10 | Sain (bruit 0,3–1,7 m en plaine et moyenne montagne, ≤ 2,4 m en haute montagne) | toutes les traces retenues |

Dans 2015-05 → 2020-08, les ratios sortie par sortie alternent entre baro KO (≈ 0,5) et altitude
GPS (> 1,5) : probablement deux appareils. **Règle du corpus : aucune trace de 2015-05 à 2020-09, ni d'avant 2014-12 sans qualification individuelle.**
Quatre traces de cette période sont gardées comme **négatifs** (`neg-01..04`) pour tester le
qualificateur lui-même.

**Critère « fiable »** (tri du corpus, pas évaluation du service) : bruit ≤ 2,5 m, p95 ≤ 8 m,
dérive ≤ 60 m, couverture IGN ≥ 90 %. La dérive tolérée est large : en montagne, la météo déplace la
pression de 30 à 50 m sur une journée (42 m mesurés dans le Cantal avec un bruit de 0,9 m). Une dérive
lente déforme peu le D+ ; un capteur défaillant se voit au bruit et aux sauts. `perso-06` (205 km,
dérive 62 m, bruit 0,8 m) est gardé avec cette réserve.

**Corpus retenu** (39 traces + 4 négatifs, détail dans `manifest.yaml`) :

| Groupe | Traces | Ce qu'elles testent | Bruit médian | D+ MNT / D+ baro |
|--------|--------|---------------------|--------------|------------------|
| `dplus` | 18 | D+ et altitude hors ouvrage : Massif central (7), Pyrénées (2), Alpes (2), Préalpes/Ventoux (2), Vosges, Montagne Noire, causses du Lot, Finistère, Limousin | 1,08 m | 0,98–1,20 (méd. 1,07) |
| `plaine` | 8 | Altitude et D+ en plaine (Loire-Atlantique) | 0,61 m | 0,96–1,05 (méd. 1,00) |
| `ouvrage` | 10 | Ponts de Saint-Nazaire (×3, dont l'accès nord non tagué), Pirmil, Audibert (×5), Anne-de-Bretagne, Mauves ; passage **sous** Cheviré (×3) | 0,40 m | 0,96–1,21 |
| `hors_france` | 3 | Hors LiDAR HD : pont de la Zurriola (Espagne), Grossglockner, Highlands (Mapterhorn seul) | à qualifier | — |
| `negatif` | 4 | Baro KO et altitude GPS : le banc doit les rejeter | 3,5–9,6 m | 0,67–2,66 |

Le ratio D+ MNT / D+ baro a été calculé avec le MNT brut au pas de 50 m (hystérésis 3 m) contre le
baromètre (hystérésis 2 m). Il retrouve l'ordre de grandeur de vcyclist (+12 %) en relief, et ≈ 1 en
plaine.

**Vie privée.** Les traces révèlent le domicile et les habitudes : elles restent dans `private/`
(gitignoré) sous pseudonyme `perso-NN`. Le manifest versionné ne porte que le département, le mois,
la distance et l'empreinte sha256 (trace entière et trace rognée de 1 km à chaque bout,
`*.trim1km.gpx`). **Les traces perso et tout ce qui en dérive (parcours recalés, réponses Stadia)
restent hors git, sans exception** (décision de l'utilisateur, 2026-10-30).

**Reconstituer `private/`** : récupérer les streams par le MCP Strava (identifiants dans
`private/perso-map.csv`, à conserver hors dépôt), les ranger en `private/strava/streams/<id>.json`,
puis lancer `tools/build_perso_corpus.py` et vérifier les empreintes du manifest.

### 1.4 Corpus d'ouvrages (versionné)

`data/ouvrages.yaml`, 37 ouvrages, OSM lu via Overpass le 2026-10-30 (le 2026-10-31 pour Roselend), référence IGN interrogée le même
jour sur deux ressources : `ign_rge_alti_wld` (MNT) et `ign_lidar_hd_mnx_multi_wld` (MNT, MNS, MNH
LiDAR HD, `zonly=false&measures=true`). Cas couverts :

- grands ponts bombés : Saint-Nazaire, Normandie, Cheviré (`bicycle=no`), île de Ré ;
- **ouvrage mal tagué** : 1,06 km de l'accès nord du pont de Saint-Nazaire sans `bridge` ni `layer`
  (MNH 19,7 m au milieu) ; Mauves (`bridge:structure` sans `bridge`), Normandie (`bridge` +
  `embankment`) ;
- passage dessus / **dessous** : Rotonde (Nantes), piste sous Cheviré (sol 6,9 m, MNS 57,0 m) ;
- ponts contigus ou successifs : Pont-Rousseau puis Pirmil, Mauves, écluses de Saint-Nazaire,
  Haudaudine et Audibert ;
- ponts de Loire nantais (Bellevue, Senghor, Tabarly, Anne-de-Bretagne en travaux, Pirmil, Briand) ;
- viaduc haut cyclable : Recoumène (voie verte, 60,8 m) ;
- tunnels sous relief marqué : Petits et Grands Goulets (route actuelle et ancienne), Combe Laval,
  Moines, Arbois (Bourne), Rousset ; pont d'Échevis ; galerie pare-avalanche de l'Arly ;
- tunnels cyclables longs : Croix-Rousse (1,76 km), voie verte de Dordogne ;
- hors LiDAR HD : Liège, Venise (Ponte della Libertà), Saint-Sébastien (Zurriola), **Krk (GLO-30
  côtier inféré : tuile Mapterhorn z13 absente)** ;
- trace qui commence sur un pont : milieu du pont de l'île de Ré ;
- **route en crête de barrage** taguée `bridge=yes` : Roselend (824 m, Savoie). Le MNT LiDAR HD retire
  le barrage voûte et lit le sol 20 à 70 m plus bas, le RGE ALTI le garde. Ajouté le 2026-10-31 après
  l'écart E1 de 131 m de `perso-11` à l'incrément 2 ; déjà vu par vcyclist (`tn10-roselend`), donc en
  calibration. Baro de `perso-11` corrigé localement : +2,0 m au-dessus du MNS ponctuel de la crête.

Réserves : le MNS n'est pas la chaussée (garde-corps, véhicules, végétation, falaise : 993 contre
907 m à un portail de Combe Laval) ; RGE ALTI et MNT LiDAR divergent de 20 à 55 m au tunnel d'Arbois
(falaise) ; l'accès vélo de plusieurs ponts n'est pas vérifié.

### 1.5 Première mesure croisée : baromètre vs MNS LiDAR

`data/deck_baro_vs_lidar.csv` (`tools/deck_baro_vs_lidar.py`) : altitude baro au point de la trace le
plus proche du milieu de l'ouvrage, décalage corrigé localement (médiane baro − MNT entre 1 et 3 km de
part et d'autre).

| Ouvrage | Passages | Baro − MNS au milieu |
|---------|----------|----------------------|
| Pont de Saint-Nazaire (tagué) | 3 | −0,51 / −0,29 / +0,09 m |
| Accès nord non tagué | 3 | −1,03 / −1,02 / −0,24 m |
| Pirmil | 1 | −0,55 m |
| Anne-de-Bretagne | 1 | −0,16 m |
| Mauves | 1 | −0,92 m |
| Audibert | 5 | −2,4 à −4,5 m |
| Sous Cheviré | 3 | baro = sol (0,4–6,9 m), MNS 57–59 m |

Le MNS LiDAR est cohérent avec le baromètre sur les grands ponts (≤ 1 m). Il ne l'est pas partout :
à Audibert, il lit 2 à 4,5 m au-dessus du baromètre, et la **médiane sur 25 m le long de l'axe OSM
donne la même valeur** que la lecture ponctuelle (11,54 m, valeurs 11,2–11,7 m) : ce n'est pas un pixel
isolé mais une surface cohérente plus haute que ce que voit le baromètre. Cause non élucidée (chaussée
cyclable plus basse que la surface lue par le LiDAR, décalage local du baromètre ?). Conséquence : le
MNS, ponctuel ou médian, n'est qu'une référence de **cohérence** ; la seule référence indépendante
d'un tablier est le baromètre. Le passage sous Cheviré confirme le piège du MNS lu au croisement
(≈ 50 m d'erreur).

`data/ouvrages.yaml` porte, pour les 22 ponts français, `milieu_mns_mediane_25m` (11 points de −12,5 à
+12,5 m le long de l'axe OSM, `tools/ouvrages/mns_median25.py`) à côté de la lecture ponctuelle.

---

## 2. Protocole

**Discipline** (reprise de vcyclist, doc 01 §6.4) :

1. Les **séries** sont fixées avant toute mesure du service, par une règle écrite qui garantit
   qu'un mètre de trace ou un ouvrage n'appartient qu'à une série (`tools/series_groups.py`) :
   - **groupes d'ouvrages** : ouvrages dont des points de référence sont à moins de 150 m (même
     ouvrage, dessus/dessous, ponts contigus), plus le lien déclaré Cheviré ↔ passage dessous. Série
     du groupe : `sha256("osm-route-details:groupe:" + plus petit id)`, premier chiffre hexadécimal
     dans 0–4 → tenue à l'écart ;
   - **déjà vu ⇒ calibration** : tout groupe dont un ouvrage a un renvoi vcyclist, les traces `vc-*`
     (vcyclist a réglé dessus ses détecteurs et son constructeur de tablier, dont le service
     s'inspire), les traces `os-*` (chaîne typologie reprise d'osm-surface) et les négatifs ;
   - **traces** : `sha256("osm-route-details:trace:" + id)`, même critère ;
   - **on partitionne les mètres, pas les traces** : la fenêtre de ± 500 m autour d'un ouvrage
     franchi (passage à moins de 40 m de chacun de ses points de référence) appartient à la série de
     l'**ouvrage** ; le passage y sert aux métriques d'ouvrage (E3, E4, E5, S*). Si cette série
     diffère de celle de la trace, la fenêtre est masquée dans les métriques de la trace (E1 ; E2 et
     E2b sommés sur les tronçons restants, pour le service comme pour ses références). Chaque mètre
     n'appartient qu'à une série, et chaque ouvrage garde tous ses passages baro.
     **Pratique confirmée** (décision de l'utilisateur, 2026-11-01) : une trace de calibration est
     envoyée **entière** au service ; ses fenêtres d'ouvrages, de croisements ou de témoins tenus à
     l'écart sont calculées puis **jetées** : jamais mesurées, ni rapportées autrement que par
     l'identifiant opaque de l'élément masqué (`passages_masques`), et jamais lues pour régler quoi
     que ce soit. Les mesures de la trace portent sur le reste.
   - **passage en hauteur sur un pont** (décision de l'utilisateur, 2026-10-31, avant toute nouvelle
     mesure) : le critère horizontal ne distingue pas une trace qui longe un pont par-dessous
     (Cheviré × perso-23, au sol sous un tablier à plus de 50 m). Si le MNH au milieu (médiane MNS
     25 m − MNT au milieu) est ≥ 10 m, le passage n'est compté sur le pont que si la médiane de (baro
     corrigé − MNT) sur les stations à ± 12,5 m du milieu, au moins 3 stations baro, atteint ½ MNH.
     Sinon, le critère horizontal seul s'applique, avec la mention `niveau_non_discriminant`
     (MNH < 10 m) ou `niveau_sans_baro` (moins de 3 stations). Budget d'erreur : bruit baro ≈ 1 m,
     incertitude du décalage ≤ 2 m (au-delà, référence incertaine, §4 bis.3), retard baro jusqu'à
     ± 4 m ; soit ≈ 5 m, d'où « dessus » et « dessous » discernables seulement pour MNH ≥ 2 × 5 m. Aucun
     terme ne vient du service. Le test ne porte que sur les métriques d'ouvrage (E3, S1,
     `hors_graphe_velo`) ; la partition des fenêtres entre séries, fixée avant la mesure, ne change
     pas.

   **Extension avant le premier passage** (décision de l'utilisateur, 2026-10-30) : 51 ouvrages
   jamais vus par vcyclist et franchis par une trace perso fiable (46 ponts dont le MNH au milieu
   dépasse 3 m, 5 tunnels), trouvés par `tools/ouvrages/candidats_traces.py` (Overpass sur la boîte
   englobante de chaque trace rognée, sans envoyer la trace), puis sélectionnés mécaniquement : noms
   et abords (< 100 m) d'ouvrages vcyclist exclus, chaussées parallèles et ways homonymes fusionnées,
   au plus 3 ouvrages par trace (2 dans l'agglomération nantaise), les plus longs. Ils dérivent des
   traces perso et dessineraient des trajets habituels : ils restent **hors git**
   (`private/ouvrages-perso.json`, empreinte au manifest), comme les traces.

   Résultat (2026-10-30) : **22 ouvrages tenus à l'écart sur 87 (25 %)**, 88 avec Roselend (calibration, ajouté le 2026-10-31), dont 19 nouveaux (17 ponts,
   2 tunnels), tous avec au moins un passage baro, plus `normandie`, `recoumene` et
   `donostia-zurriola` ; **9 traces perso sur 39 (23 %)** ; 48 fenêtres de passage masquées dans les
   métriques des traces.

   Variantes écartées : grouper des traces entières (composante connexe trace × ouvrage) soude les
   traces nantaises, qui partagent de nombreux ponts, en une composante géante (28 cas, puis 36 avec
   l'extension) : tout ou rien. Sans la règle « déjà vu », on garderait des ouvrages sur lesquels
   vcyclist a réglé son constructeur de tablier : fuite.
2. Les **seuils** du §4 sont gelés dans ce fichier avant le premier passage du service. Toute
   modification est un commit séparé, motivé, antérieur à la mesure qu'elle concerne.
3. La série tenue à l'écart est jugée **une seule fois par incrément** (MVP, V1…) : on note le commit
   du service, les `data_versions` et `rules_version`, et l'empreinte du manifest. Après jugement, elle
   devient calibration et une nouvelle série doit être constituée pour l'incrément suivant.
   Un critère bloquant sans cas dans la série tenue à l'écart (sauté), ou sous son minimum de
   couverture (§4 ter.4), n'est pas vert : l'incrément est refusé. Le jugement ne se refait pas ;
   la série est complétée pour l'incrément suivant (décision de l'utilisateur, 2026-11-01, à partir
   de V1 ; le jugement du MVP appliquait déjà « sauté = refus » dans `ord-bench judge`).
4. Les données externes sont figées par empreinte (manifest) : un fichier dont le sha256 a changé
   invalide la comparaison.

### 2.1 Traces enregistrées : recalage, service, comparaison

Les traces perso sont des traces GPS enregistrées, pas des itinéraires planifiés : leur géométrie
porte l'erreur latérale du GPS. Protocole (décision de l'utilisateur, 2026-10-30) :

1. **Recalage** sur la route par Stadia Maps (`trace_attributes`, Valhalla, `costing: bicycle`,
   `shape_match: map_snap`), sur la trace rognée de 1 km à chaque bout (`*.trim1km.gpx`, pour ne pas
   envoyer les abords du domicile), rééchantillonnée à 25 m, par tronçons de 2 000 points
   chevauchants (`tools/stadia_match.py`). Sorties dans `private/recales/` : géométrie recalée
   (`<id>.gpx`, sans altitude), baro transporté (`<id>.baro.csv`), revêtement Stadia
   (`<id>.surface.csv`, référence R3) et réponses brutes (`brut/`). Une trace est d'abord coupée
   aux sauts de plus de 2 km entre deux points consécutifs (transport : le recalage inventerait une
   route) ; les segments de moins de 5 km sont écartés. Seuil au-dessus du plus long tunnel cyclable
   du corpus (1,76 km), où le GPS peut décrocher. Résultat le 2026-10-30 sur les 39 traces : 40
   parcours recalés (`perso-13` coupée en `.s1`/`.s2` après 10,8 km de transport ; `perso-12` réduite
   à son segment principal après 32,7 km de transport), 96 à 100 % des points appariés, longueur
   recalée entre 0,99 et 1,03 fois la longueur d'origine.
   **Sauts de Valhalla** (corrigé le 2026-10-31, depuis le cache, sans nouvel appel). Quand Valhalla
   ne sait pas enchaîner deux sous-parcours, son `shape` les relie par une ligne droite à travers
   champs, jusqu'à 1,5 km, portée par une arête bien plus courte. Ces lignes droites étaient passées
   au service : elles ont produit les « trous » M1 de `perso-06` et `perso-18` à l'incrément 3. Pourtant
   les ways que Stadia apparie de part et d'autre sont bien dans les tuiles VersaTiles, et ils
   existaient dans OSM avant les données VersaTiles (réplication du 2026-06-07). Une arête dont la forme dépasse sa
   longueur de plus de 50 m (excès < 25 m sur les autres arêtes du corpus) contient un saut. Le saut
   est remplacé par la trace d'origine entre le dernier point apparié avant lui et le premier après,
   si le shape retiré n'excède pas deux fois cette trace + 200 m (sinon une boucle serait coupée).
   Les points insérés gardent leur baro (marqués `insere = 1`), sous la condition du point 3. La plage n'a pas de référence de revêtement (`use` =
   `discontinuite`, surface vide), à exclure de R3. Résultat : 19 parcours touchés, 82 sauts dont 6
   restés droits (2,2 km au total, sans point apparié de part et d'autre). Les longueurs restent
   entre 0,98 et 1,03 fois celles d'origine. Sur `perso-06`, 9,0 des 9,7 km insérés suivent une voie
   VersaTiles (≤ 15 m). Sur `perso-18`, 0,8 km reste hors de toute voie : la sortie y passe hors du
   réseau cartographié, ce qui est un vrai non apparié. Ancienne version gardée dans
   `private/recales.v1/`.
   **Allers-retours** (corrigé le 2026-10-31, depuis le cache, sans nouvel appel). Sur un aller-retour,
   l'aller et le retour se superposent ; deux étapes prenaient le point le plus proche et sautaient
   sur le retour : (a) le raccord entre deux tronçons de requête, qui a retiré de `perso-23` un
   aller-retour de 1,8 km entre deux traversées de Pirmil ; (b) le transport du baro, qui empilait des
   points sur une seule abscisse ou en écartait. Les deux retiennent désormais le **premier** point ou
   segment à moins de (plus proche + 15 m, resp. 10 m), au minimum local, jamais en arrière de la
   dernière abscisse retenue. Résultat : seule la géométrie de `perso-23` change (+ 2,1 km) ; le baro
   transporté gagne 1 714 points (161 335 → 163 049), aucun n'est perdu, et les sauts d'abscisse de
   plus de 300 m sans contrepartie à l'origine passent de 47 à 24. Version précédente gardée dans
   `private/recales.v2/`.
   **Trous de la trace brute** (corrigé le 2026-10-31, depuis le cache). Le rééchantillonnage à 25 m
   tirait des points en ligne droite dans les pas bruts où le GPS est perdu (galeries, tunnels) et leur
   donnait un baro interpolé, ce que la règle du banc interdit au-delà de 100 m
   (`baro_gap_max_m`). Ces points servent encore au recalage, mais ne portent plus de baro : 176 points
   retirés sur 12 traces, géométrie inchangée. Version précédente dans `private/recales.v3/`.
   **Source** (décision de l'utilisateur, 2026-11-01) : à partir de V1, le recalage et la
   référence de revêtement (R3, et R1/R2 des parcours planifiés) passent par le **Valhalla public
   de la FOSSGIS** (`https://valhalla1.openstreetmap.de/trace_attributes`, même moteur, même
   requête), à **1 requête par seconde** au plus, en séquence, User-Agent identifiable
   (`tools/stadia_match.py`, variable `VALHALLA_URL`). Données du serveur : tuiles Valhalla du
   2026-11-01 13:01 UTC (`/status`, `tileset_last_modified`), appels du 2026-11-01. **Toutes les
   traces du banc sont recalées par cette même source**, `perso-01..39` comprises (recalées par
   Stadia en 2026-10, réponses gardées dans `private/recales.stadia/`, non relues) : les rapports
   de calibration postérieurs ne sont pas comparables aux précédents sur M1, R3 et E1.
2. **Service** : la géométrie recalée est l'entrée. Les bornes d'entrée de l'API (200 km,
   16 000 points) s'appliquent : au-delà, l'outil la découpe en tronçons ≤ 200 km (`<id>.partN.gpx`,
   aujourd'hui `perso-06`, 205 km) ; les sorties des tronçons sont recousues par abscisse. La sortie
   n'est pas bornée.
3. **Comparaison au baro sur la géométrie recalée.** Chaque point d'origine apparié par Stadia est
   projeté sur la géométrie recalée ; son abscisse y porte l'altitude baro. Les points non appariés,
   ou à plus de 30 m de leur point apparié, sont écartés.
   **Points insérés** (décision de l'utilisateur, 2026-10-31, avant toute nouvelle mesure) : un point
   d'origine inséré à la place d'un saut (`insere = 1`) n'a pas été validé par l'appariement ; sa
   position est celle du GPS, qui peut errer hors de la route (perso-10, galerie de la D921 : jusqu'à
   146 m de toute voie, au fond de la gorge). Il ne garde son baro que si sa position sur la géométrie
   recalée est à 30 m au plus d'une ligne de la couche `streets` (tuile du point et ses 8 voisines),
   même seuil que pour les points appariés. Les tuiles sont celles de la **campagne qui juge la
   trace** : ord-bench applique le test au chargement du baro, le recalage ne tranche pas. Pour une
   trace tenue à l'écart, c'est donc la campagne de jugement, cache rempli à ce moment ; une tuile
   réellement absente à la source compte comme « sans voie ». Aucun terme du service. Mesure sur la
   calibration et les traces tenues à l'écart (cache de calibration, 2026-10-31) : 501 points insérés
   portent du baro, environ 75 seraient retirés (≈ 1,9 km), dont les 29 de la sortie hors réseau de
   perso-18 ; l'alternative « aucun baro sur les points insérés » en retirait 503 (≈ 12,6 km), dont
   ≈ 10,7 km à moins de 30 m d'une voie.
   Pourquoi la géométrie recalée plutôt que
   l'originale : (a) c'est celle que lit le service, donc la seule où le service et sa référence
   sans correction (E2b) sont comparables point à point ; (b) le vélo roulait sur la route, et le baromètre mesure l'altitude de
   la route ; (c) lire le relief sur la géométrie d'origine ajouterait l'erreur latérale du GPS
   multipliée par la pente transversale (5 m d'écart sur un dévers à 30 % ≈ 1,5 m), et sur un pont la
   trace d'origine peut sortir du tablier. La géométrie d'origine ne sert qu'au transport du baro.

Le recalage a tourné le 2026-10-30 (envoi autorisé par l'utilisateur). Il est régénérable :
`tools/stadia_match.py` relit `private/recales/brut/` s'il existe, et rappelle Stadia sinon.

### 2.2 Campagne de mesure (une par incrément)

1. Vérifier les empreintes du manifest (traces privées comprises) et recaler les traces perso (§2.1).
2. Envoyer chaque parcours au service (`sampling.mode = distance`, pas 5 m, et le mode par défaut
   `input_with_breakpoints`), à froid (cache vidé) puis à chaud.
3. Rééchantillonner service et référence sur la même grille curviligne de 5 m.
   **Lecture du « MNT LiDAR » des règles** (précision, 2026-10-31, pas une règle nouvelle) : les termes
   de référence lus sur le MNT sont toujours ceux du mode `distance` au pas de 5 m, quel que soit le
   mode du service mesuré. C'est-à-dire : la sortie du service avec `structure_correction: false` dans
   ce mode, simple échantillonnage du MNT à sources égales. Sont concernés : le test de niveau
   (§2.1), le décalage d'approche E3 (§4 bis.3), les bandes de portail E4 (§4 bis.2), le MNT d'E5 et
   les décisions de passage qui en découlent. En `input_with_breakpoints`, la sortie sans correction est
   interpolée entre les points : ce n'est plus le MNT, et la référence d'un passage dépendrait du mode
   testé. La référence et les passages comptés sont donc identiques dans les deux modes ; seule la
   sortie du service mesurée change. Exception : E2b compare le service corrigé et non corrigé **dans
   le même mode**, à sources égales.
4. **Référence baro** : retirer un décalage. Pour l'altitude, décalage global = médiane(baro − sortie
   du service) sur les stations `ground` ; pour un pont (E3), décalage local pris sur les approches de
   50 à 300 m (§4 bis.3), à défaut de 1 à 3 km de part et d'autre.
   La dérive lente (> 5 km) est retirée par médiane glissante avant le calcul des résidus d'altitude ;
   jamais avant le D+.
5. Calculer les métriques (§3), comparer aux seuils (§4), consigner le rapport (commit, versions,
   empreinte du manifest, date).

---

## 3. Métriques

| Id | Métrique | Définition | Référence |
|----|----------|------------|-----------|
| **E1** | Erreur d'altitude hors ouvrage | RMSE et P95 de (service − baro), après décalage et retrait de tendance, sur les stations `ground` | traces `dplus`, `plaine` |
| **E2** | D+ comparé au baro (indicatif) | ratio D+ service (preset `dem`) / D+ baro (hystérésis 2 m, fenêtre 15 m : preset `barometric`), par trace | traces `dplus`, `plaine`, `ouvrage` |
| **E2b** | Les corrections ne dégradent pas le D+ | par groupe : médiane \|ln(D+ service / D+ baro)\| ≤ médiane \|ln(D+ service avec `structure_correction: false` / D+ baro)\| ; mêmes sources, géométrie recalée, preset `dem` **des trois côtés (baro compris, §4 bis.5)** et masques | idem |
| **E3** | Erreur d'altitude sur tablier | Par passage : erreur max et erreur au milieu de (service − référence) sur l'emprise du tablier ; par pont : **médiane sur les passages** de l'une et de l'autre (§4 bis) | baro corrigé localement (seule référence indépendante) ; à défaut, médiane MNS 25 m (`ouvrages.yaml` `milieu_mns_mediane_25m`, vcyclist `B`), référence de **cohérence** seulement |
| **E4** | Erreur en tunnel | Par tunnel **OSM**, entre ses propres portails : erreur max de (service − droite entre portails) ; D+ fictif ajouté dans le tunnel. Altitude de portail = médiane du MNT des stations entre 10 et 30 m hors du portail, sur l'approche (§4 bis) | `ouvrages.yaml`, vcyclist `N` (par tunnel OSM) |
| **E5** | Passage sous un pont | \|service − référence\| au croisement (baro corrigé ou MNT, §4 ter.7) | `chevire-dessous`, `rotonde-dessous`, `perso-23..25` |
| **S1** | Rappel des ouvrages OSM tagués | Longueur des ponts/tunnels OSM du corpus, **portés par une voie du graphe vélo**, typés `bridge`/`tunnel` par le service ÷ longueur de référence ; balcons non tagués exclus (§4 bis) ; V1 : recouvrement continu (§4 ter.1) | `ouvrages.yaml`, `structures.csv` |
| **S2** | Faux positifs d'ouvrage | Longueur typée `bridge`/`tunnel` hors de tout ouvrage de référence ÷ longueur totale ; nombre d'ouvrages fictifs | témoins négatifs vcyclist `n*`, `hn*` ; traces `dplus` |
| **S3** | Ouvrage non tagué | MVP : altitude au milieu de `stn-acces-nord`, et erreur sur les balcons non tagués entre tunnels (Petits Goulets, Combe Laval) (mesurées, signalées, pas de seuil). V1 : détecté oui/non | baro `perso-20..22` ; vcyclist `S` sur les balcons |
| **R1** | Accord du revêtement vs Stadia | Part de longueur où `surface.class` = classe Stadia ; variante hors inconnu Stadia | osm-surface |
| **R2** | Accord revêtu / non revêtu | Idem sur la partition binaire, mètres classables des deux côtés | osm-surface |
| **R3** | Accord vs Stadia sur les traces perso | Comme R1 et R2, référence = `edge.surface` de la réponse `trace_attributes` de Stadia Maps | traces perso recalées (§2.1) |
| **M1** | Taux d'appariement | 1 − (longueur non appariée ÷ longueur totale) ; part `unknown` | osm-surface, toutes traces |
| **L1** | Latence à froid | p50, p95 d'une requête de 100 km, caches disque vidés (IGN, Mapterhorn, VersaTiles) | traces de 90–130 km |
| **L2** | Latence à chaud | p50, p95 de la même requête rejouée | idem |
| **Q1** | Rejet des négatifs | Le banc lui-même : `qualify_baro.py` ne doit classer aucun de `neg-01..04` en `fiable` (aujourd'hui : 3 `suspect`, 1 `hors_ign` car en partie hors couverture IGN) | négatifs |

---

## 4. Seuils d'acceptation proposés pour le MVP

Proposés, à geler avant le premier passage du service. « France » = terrain IGN mesuré.

| Id | Seuil MVP | Justification |
|----|-----------|---------------|
| E1 | RMSE ≤ 1,5 m en plaine, ≤ 3,0 m en relief ; P95 ≤ 4 m / 8 m | Baro vs MNT au pas de 50 m : bruit médian 0,6 m (plaine), 1,1 m (relief), p95 médian 1,6 / 3,5 m ; vcyclist Strava vs MNT 0,75–0,85 m / 2,31 m |
| E2 | **Indicatif** (suivi en non-régression) : par trace dans [0,90 ; 1,20] ; médiane du groupe dans [0,95 ; 1,12] | MNT brut / baro mesuré : 0,96–1,21 (médianes 1,00 plaine, 1,07 relief) ; vcyclist +12 % ; critère vcyclist [0,8 ; 1,2] resserré car notre référence est un baromètre |
| E2b | **Bloquant** : pour chaque groupe, médiane \|ln(D+ service / D+ baro)\| ≤ même médiane pour le service appelé avec `structure_correction: false`, D+ baro en preset `dem` (§4 bis.5) | Mesure l'apport des corrections d'ouvrage à sources égales (LiDAR HD, repli Mapterhorn). Le RGE ALTI, ≈ 7 m d'erreur en montagne, rendrait le critère presque toujours vrai (décision de l'utilisateur, 2026-10-30) |
| E3 | France, pont tagué : \|erreur au milieu\| ≤ 2 m et max ≤ 3 m (ponts < 300 m), ≤ 5 m (grands ponts) ; ≥ 90 % des ponts du corpus passent | vcyclist : ≤ 2 m Nantes, ≤ 5 m Saint-Nazaire, 18/21 = 86 % ; baro − MNS mesuré ≤ 1 m sur Saint-Nazaire, Pirmil, Mauves |
| E3 hors France | Mesuré et rapporté, pas de seuil (pont interpolé, `interpolated_structure`) | 22–40 m d'erreur attendus sur grands ponts bombés (doc 01 §9) |
| E4 | \|erreur max\| ≤ 5 m et D+ fictif ≤ 5 m par tunnel ; ≥ 6/7 des tunnels tagués | Critère Grands Goulets de vcyclist ; 6/7 hors échantillon ; Strava : +190 m d'erreur, +112 m de D+ fictif aux Grands Goulets |
| E5 | \|service − référence\| au croisement ≤ 1 m (MNT), ≤ 2 m (baro corrigé localement, §4 ter.7), 100 % des cas | Erreur ≈ 50 m si le MNS est lu ; le baro corrigé porte l'erreur de son décalage, comme en E3 |
| S1 | ≥ 95 % de la longueur des ouvrages tagués typée correctement | Le MVP se fonde sur OSM seul : seul l'appariement peut faire échouer |
| S2 | 0 ouvrage fictif sur les négatifs ; ≤ 0,5 % de longueur hors ouvrage typée pont/tunnel | Pas d'heuristique au MVP : un faux positif ne peut venir que d'un mauvais appariement |
| S3 | Pas de seuil au MVP (limite documentée, `structure: ground`) ; V1 : détecté | doc 01 §5.5 |
| R1 | ≥ 85 % par parcours (classe), ≥ 85 % hors inconnu | osm-surface 86,0–93,5 % ; ne pas régresser sous le pire cas |
| R2 | ≥ 97 % par parcours | osm-surface 98,1–99,3 % |
| R3 | Comme R1 et R2 (classe ≥ 85 %, revêtu / non revêtu ≥ 97 %) par trace perso recalée, contre Stadia `trace_attributes` ; rapporté, non bloquant (§4 bis.7) | Même nature de référence que R1/R2 (Stadia/Valhalla), sur 4 600 km de régions variées |
| M1 | Non apparié ≤ 1 % de la longueur ; `unknown` ≤ 2 % | osm-surface : 0–8 points non appariés, inconnu 0–1,3 % |
| L1 | p95 ≤ 20 s pour 100 km en France à froid ; ≤ 5 s hors France | vcyclist : 130 km IGN à froid 19 s / 264 tuiles, Mapterhorn 3 s / 16 tuiles |
| L2 | p95 ≤ 500 ms pour 100 km à chaud | osm-surface : 55–77 ms pour la typologie ; marge pour l'élévation et l'assemblage |
| Q1 | 4/4 négatifs non `fiable` | Contrôle du banc |

**§4 bis. Périmètre et agrégation des critères d'ouvrage** (décision de l'utilisateur, 2026-10-31 ;
écrit avant toute nouvelle mesure et avant le jugement de la série tenue à l'écart ; seuils
inchangés). Motif : la campagne de l'incrément 4, en calibration, a montré que trois règles de
lecture étaient ambiguës ou mal posées pour un service vélo. Aucune n'est taillée sur un cas : ce
sont des règles générales, appliquées de la même façon aux deux séries.
Complété le même jour (décision de l'utilisateur, 2026-10-31, commit dédié, toujours avant toute
nouvelle mesure et avant le jugement) après l'analyse de la campagne 2026.10.5 en calibration : source
des tags du périmètre vélo (point 1), bandes de portail invalides dans une série de tunnels
(point 2), limite des données pour les galeries pare-avalanche (point 4). Complété une seconde fois
(décision de l'utilisateur, 2026-10-31, commit dédié, avant toute nouvelle mesure) après la campagne
2026.10.6-bench2 en calibration : bande sur un balcon court entre deux tunnels (point 2), contrôle
`hors_graphe_velo` quand la trace longe l'ouvrage sur une voie cyclable parallèle (point 1).
Complété une troisième fois (décision de l'utilisateur, 2026-10-31, commit dédié, avant toute nouvelle
mesure) après l'analyse des échecs E3/E4 de la campagne 2026.10.7 en calibration : décalage local
E3 pris sur les approches et référence « incertaine » (point 3), bande de portail sur un pont ou un
tunnel des tuiles (point 2) ; puis, après vérification sur la calibration, plausibilité de la pente
entre portails (point 2).
Modifié le 2026-11-01 (décision de l'utilisateur, commits dédiés, avant le gel du MVP et avant toute
nouvelle mesure) : limite des données hors décompte (point 4), plages hors réseau de M1 (point 6), R3 non bloquant (point 7).

1. **Ouvrages hors graphe vélo.** Un ouvrage dont la voie OSM est exclue du graphe vélo par la règle
   de tags du service (T03 §4.2, étape 1 : motorway, trunk en France, pedestrian, bridleway, busway ;
   `bicycle=*` prime) sort de S1, E3, E4 et E5. Exemples : Millau A75, Cheviré, Lioran, Galibier. La
   règle s'applique aux tags du way, jamais à une liste de cas. **Source des tags** : les attributs des tuiles de
   la campagne (`campagne.data_versions` du rapport, couche `streets` : `kind`, `bicycle`, `bridge`,
   `tunnel`), c'est-à-dire les données que lit le service, et non le corpus Overpass du banc, de date
   différente. Si la géométrie de l'ouvrage du corpus n'a pas d'équivalent dans ces données (way créé
   ou retracé après la date des tuiles), l'ouvrage est marqué `absent_des_donnees`, rapporté
   (`E4_non_couvert`, ou `E3_non_couvert` pour un pont) et non compté ; exemple : Anne-de-Bretagne.
   Inversement, un ouvrage interdit au vélo dans OSM récent mais franchissable dans les tuiles reste
   dans le périmètre. La voie de l'ouvrage, dans les tuiles, est l'élément de même nature (`bridge`,
   `tunnel`) qui suit le mieux sa géométrie : plus petite médiane des distances sur l'emprise,
   acceptée jusqu'à 15 m, sinon même critère sur toutes les voies. Ce n'est jamais la plus permissive
   des voies voisines : un trottoir `bicycle=no` longé par une piste cyclable reste hors graphe. Deux
   candidats à moins de 3 m d'écart de médiane, de périmètres différents : « périmètre ambigu »,
   rapporté. Pour E5, c'est la voie du passage
   dessous qui compte : la piste sous Cheviré (`chevire-dessous`) reste. Ces ouvrages sont rapportés à part
   (`hors_graphe_velo`) avec leur contrôle propre, sans seuil : le service n'apparie pas la portion
   à la voie hors graphe. Le contrôle passe si la portion est non appariée (`matched: false`, avec
   l'avertissement `unmatched_range`) **ou** appariée à une voie du graphe vélo distincte de celle de
   l'ouvrage (trace qui longe l'ouvrage sur une piste parallèle, cas `nv-49`). L'erreur d'altitude
   est rapportée quand une référence existe, sinon `null` avec sa raison. Les négatifs dont c'est la **trace** qui emprunte une voie hors graphe (`n04`, `n08`,
   `n09`) restent dans S2 et E5 : y inventer un ouvrage, en accrochant une voie qui croise, est un
   défaut du service. (À partir de V1, leurs portions hors graphe sortent de S2 : §4 ter.5.)
2. **Tunnels.**
   - E4 se mesure **par tunnel OSM**, chacun entre ses propres portails. Une série de tunnels séparés
     par des balcons non tagués (Petits Goulets, Combe Laval), qu'un span vcyclist réunit en un seul
     tunnel, donne autant de tunnels que de ways `tunnel` contigus.
   - Les balcons intermédiaires non tagués sortent d'E4 et de S1. Ils sont rapportés avec S3, contre
     la référence vcyclist `S` (Strava − décalage), sans seuil au MVP.
   - Altitude de portail de la référence = **médiane du MNT des stations entre 10 et 30 m hors du
     portail**, sur l'approche. Le MNT de la première station hors portail n'est pas la chaussée sur un
     portail en falaise ; la bande reste distincte de la sonde ponctuelle du service à 10 m, ce qui
     évite une référence circulaire.
   - **Bande de portail invalide** : la bande 10–30 m d'un portail est invalide si elle chevauche un
     autre ouvrage (tunnel, galerie, balcon S3, ou tout pont ou tunnel des tuiles de la campagne,
     `bridge=true` / `tunnel=true`, qu'il soit ou non du corpus : sous un pont voisin du portail, le
     MNT lit le fond de vallée), si elle compte moins de 3 stations hors ouvrage, ou
     si elle est située sur un tronçon à l'air libre de moins de 120 m entre deux tunnels OSM (balcon
     non tagué : sur ces routes en falaise, le MNT lit la paroi, pas la chaussée ; les tronçons entre
     tunnels d'une même série du corpus mesurent 15 à 116 m).
     Elle est alors remplacée par la première bande valide plus loin dans la série, du même côté (en
     pratique le portail extérieur de la série). Sans bande valide à moins de 1 km de ce côté, le
     tunnel est noté « référence indisponible », rapporté et non compté. L'erreur reste mesurée sur
     le seul tunnel OSM (droite entre les deux altitudes de portail retenues) ; les balcons restent
     en S3.
   - **Plausibilité** : une pente implicite entre les portails retenus, |z sortie − z entrée| ÷
     distance curviligne entre eux, supérieure à 25 % est impossible pour une route ouverte : la
     référence est « indisponible », rapportée et non comptée. La borne vient de la géométrie
     routière, pas du paramètre du service (`max_tunnel_chord_grade`), qui la rendrait circulaire.
     Vérifié avant adoption (2026.10.7) : les tunnels qui passent vont jusqu'à 9,3 %, les galeries
     en échec jusqu'à 15,7 % ; seul combe-laval#0 (120,6 %) dépasse la borne.
   - Sensibilité rapportée : E4 recalculé avec un portail pris à 5, 10, 20 et 30 m. Un tunnel dont le
     verdict bascule entre ces distances est marqué « référence ambiguë » ; son verdict reste celui de
     la règle ci-dessus.
3. **E3 par pont = médiane des passages.** Pour un pont franchi plusieurs fois, l'écart au milieu et
   l'écart max sont les médianes, sur ses passages, des valeurs mesurées passage par passage. Le
   bruit baro d'un passage (≈ 1 m) est du même ordre que la tolérance de 2 m : exiger que tous les
   passages passent pénalisait les ponts les mieux documentés. La référence reste le baro corrigé
   localement, y compris à Audibert (5 passages concordants, 2,4–4,5 m sous le MNS du tablier) et à
   Anne-de-Bretagne.
   - **Décalage local d'un passage** = médiane de (baro − MNT LiDAR) sur les **approches de 50 à
     300 m** de chaque côté du pont, hors stations d'un autre ouvrage et des fenêtres masquées. Le
     décalage pris à 1–3 km ne vaut pas toujours au pont : le baro s'y décale localement (dérive,
     retard), alors que le tablier est raccord avec la route. Le calcul ne fait intervenir que le
     baro et le MNT, jamais le service : pas de référence circulaire.
   - Au moins 20 stations par côté ; sinon, repli sur le décalage à 1–3 km, avec la mention
     `decalage_repli_1_3km`.
   - Si les décalages des deux côtés diffèrent de plus de 2 m (la tolérance E3), la référence du
     passage est « incertaine » : rapportée, non comptée. Un saut de baro entre les deux approches
     est ainsi écarté sans détecteur séparé.
4. **Limite des données, hors décompte** (décision de l'utilisateur, 2026-11-01, avant le gel du MVP ;
   remplace la règle « compter, avec la limite » du 2026-10-31). Un ouvrage du corpus que la source
   du service ne type pas comme tunnel ou pont **à cause de son schéma** (l'attribut qui le décrit
   n'existe pas dans la couche lue par le service, et non un tag absent ou faux dans OSM) sort
   d'E4 et de S1 (d'E3 et de S1 pour un pont). Il est rapporté à part, comme limite des données du
   MVP, à la manière de S3 : erreur d'altitude mesurée contre sa référence, sans seuil, avec la note
   `limite_donnees` qui nomme l'attribut manquant. Motif : le MVP lit les tuiles telles qu'elles
   sont ; un ouvrage que leur schéma ne peut pas exprimer mesure le choix de la source, pas le
   service, et le compter ferait échouer E4 sur un défaut qu'aucune correction du service ne peut
   lever. La règle porte sur le schéma, jamais sur une liste de cas ; un ouvrage absent de la
   géométrie des tuiles reste `absent_des_donnees` (point 1). Cas connus : les galeries
   pare-avalanche (`tunnel=avalanche_protector`), que le schéma Shortbread de VersaTiles rend en
   voie ordinaire sans `tunnel` : `arly-galerie`, `nv-30`, `nv-37`.
5. **E2b à traitement égal** (décision de l'utilisateur, 2026-10-31, avant toute nouvelle mesure).
   Le D+ baro d'E2b est calculé avec le preset `dem` (hystérésis 3 m, lissage 30 m), le même que pour
   le service et sa référence sans correction ; E2, indicatif, reste en `barometric` (2 m, 15 m),
   vue produit. Motif : E2b mesure l'apport des corrections à sources **et à traitement** égaux. Avec
   le baro en `barometric`, la seule différence de preset donnait au baro 2,6 à 5,0 % de D+ de plus en
   plaine ; les faux D+ des creux sous les tabliers compensaient ce biais, et retirer ces creux
   faisait échouer une correction physiquement juste. Médianes |ln(D+ / D+ baro)|, service / sans
   correction, calibration 2026.10.9-bench1 (27 traces, D+ recalculé à < 1 % du rapport) :
   | groupe | baro `barometric` | baro `dem` |
   |---|---|---|
   | dplus (14) | 0,0325 / 0,0514 | 0,0474 / 0,0645 |
   | ouvrage (8) | 0,0272 / 0,0404 | 0,0570 / 0,0713 |
   | plaine (5) | 0,0374 / 0,0240 (échec) | 0,0120 / 0,0367 |
   Les trois groupes passent à traitement égal. Les marges de dplus et d'ouvrage se resserrent
   (rapport service / sans correction 0,63 → 0,73 et 0,67 → 0,80) : le service y est déjà au-dessus
   du baro, et le preset `dem` abaisse le D+ baro. Le critère reste discriminant partout. En plaine,
   chaque correction insérée seule retire du D+ (62 corrections, −109 m), sauf trois (+0,8 à +3,2 m).
   Valeurs ci-dessus : recalcul de contrôle hors ord-bench (lissage et accumulateur réimplémentés,
   écart ≤ 3,6 m par trace). Valeurs d'ord-bench, qui font foi (2026.10.9-bench2, mode distance,
   baro `dem`) : dplus 0,0472 / 0,0635, ouvrage 0,0565 / 0,0703, plaine 0,0165 / 0,0360 ; mêmes
   verdicts. Le baro garde un seuil fixe de 3 m : l'adaptation du seuil sur GLO-30 est une propriété
   de la source MNT, sans objet pour le baro (et sans plage adaptée sur les traces plaine).
6. **M1 hors réseau** (décision de l'utilisateur, 2026-11-01, avant le gel du MVP). Sur une trace
   recalée, une plage dont aucun point n'a de voie de la couche `streets` à moins de 30 m, dans les
   tuiles de la campagne qui juge la trace (tuile du point et ses 8 voisines, comme pour les points
   insérés, §2.1 ; tuile absente à la source = sans voie), sort de M1 : ni dans la longueur non
   appariée, ni dans la longueur totale, ni dans la part `unknown`. Elle est rapportée à part
   (`hors_reseau`, longueur par trace). Motif : M1 mesure les échecs du matcher là où une voie
   existe ; une sortie réellement hors du réseau cartographié ne peut pas être appariée, quel que
   soit le service. Le test ne lit que les tuiles et la trace, jamais la réponse du service. Cas
   déclencheur : `perso-18`.
7. **R3 rapporté, non bloquant** (décision de l'utilisateur, 2026-11-01, avant le gel du MVP). R3
   reste mesuré avec les mêmes seuils (classe ≥ 85 %, revêtu / non revêtu ≥ 97 %), mais un échec
   est rapporté sans refuser l'incrément. Motif : R3 compare deux dérivés d'OSM de dates
   différentes, les tuiles du service et le graphe de Stadia Maps ; un retaguage OSM entre ces deux
   dates n'est pas un défaut du service. R1 et R2, sur osm-surface, restent bloquants ; R1 garde son
   seuil de 85 %. Cas déclencheur : `perso-17`.

**Lecture de R1–R3 et M1 au MVP** (incrément 3, 2026-10-31 ; seuils inchangés). `surface.class` est
gelée à `paved` / `unpaved` / `unknown` pour la v1 (doc 01). La classe Stadia est donc ramenée aux
mêmes trois valeurs : revêtu = `paved_smooth`, `paved`, `paved_rough` ; non revêtu = toute surface
pire que `paved_rough` (`compacted` compris), comme osm-surface (T03 §5). Conséquences :
- R1 « classe » compte 3 classes. Il n'est pas comparable aux 86–94 % d'osm-surface sur 5 classes, et
  son seuil de 85 % ne discrimine plus guère : R1 ne diffère de R2 que par les mètres inconnus. Les
  gardes effectives sont R2 et M1 ; R3 (revêtu / non revêtu ≥ 97 %) est rapporté (point 7). Un R1 sur la classe détaillée
  attendra `SurfaceDetail` (V2).
- La part `unknown` de M1 est mesurée sur `surface.class`. Une portion appariée reçoit toujours une
  classe (repli sur `highway`, T03 §5) : `unknown` ≈ non apparié, et le seuil de 1 % sur le non
  apparié est le plus serré des deux.

Sur la série tenue à l'écart, un incrément est accepté si tous les seuils passent. Un échec isolé sur
un cas tenu à l'écart est consigné et analysé, mais ne se corrige pas sur ce cas (sinon il devient
calibration).

**§4 ter. Règles pour V1** (décisions de l'utilisateur, 2026-11-01, après le jugement du MVP,
`7f171d2`, refusé ; commits dédiés, avant toute nouvelle mesure). Le MVP reste refusé : ces règles
ne s'appliquent qu'à partir de V1, en calibration comme sur la série tenue à l'écart V1. Seuils
inchangés.

1. **Grain de S1.** La longueur typée d'un passage est la longueur du recouvrement de l'emprise
   [s_A, s_B] (abscisses de A et de B sur la trace recalée) par les segments structure du service
   (`bridge` / `tunnel`), mesurée en continu sur leurs abscisses, et non plus le nombre de stations
   de 5 m typées entre les stations les plus proches de A et de B, bornes comprises. Tolérance d'un
   demi-pas (2,5 m) à chaque borne : un segment structure qui s'arrête à 2,5 m au plus de s_A ou de
   s_B est prolongé jusqu'à la borne. Rappel du passage = longueur typée ÷ (s_B − s_A). Motif : au
   jugement du MVP, presque chaque pont perdait exactement une station de borne (rappels
   (n − 1)/n, 7 à 8 % sur un pont de 54 à 74 m), soit ≈ 80 m sur 155 m de déficit ; l'effet
   dépendait de la longueur des ponts de la série, pas du service. E3 et E4 ne changent pas : E3 lit
   ses stations entre A et B et le milieu, E4 ses bandes de portail et la droite entre portails ;
   aucun ne compte de longueur typée.
2. **Attribution d'un passage.** Un passage (trace à moins de 40 m de A, du milieu et de B, §2.1)
   n'est attribué à un pont ou à un tunnel du corpus que si la **trace brute** (GPS d'origine, pas
   la trace recalée) suit sa voie mieux que tout autre pont ou tunnel des tuiles de la campagne :
   sur l'emprise, la médiane des distances des points bruts à la voie de l'ouvrage (élément de même
   nature qui suit le mieux sa géométrie, §4 bis.1) est la plus petite de celles des voies
   `bridge=true` / `tunnel=true` des tuiles. Sinon le passage est rapporté en
   `passage_sur_ouvrage_parallele` et n'est compté ni dans E3, E4, S1, ni dans `hors_graphe_velo`.
   Motif : au jugement du MVP, deux passages sur un pont routier ont été attribués à la passerelle
   parallèle à ≈ 8 m (`nv-40`) ; le service typait le bon pont, et le banc le jugeait sur un autre.
   La règle ne lit que la trace brute et les tuiles, jamais le service. Passage dessous (E5) : non
   concerné.
   **Un tablier, un ouvrage** (décision de l'utilisateur, 2026-11-01, après la première calibration
   V1, règles 2026.11.2). Les voies `bridge=true` d'un même pont (piste, trottoir, tram cartographiés
   à part sur le même tablier) forment un seul ouvrage : la règle ne départage que des ponts
   distincts, et la médiane de l'ouvrage est la plus petite de celles de ses voies. Une voie
   concurrente est du **même pont** que la voie de l'ouvrage si, dans les tuiles de la campagne :
   (a) leurs points les plus proches du milieu de l'ouvrage sont dans un même polygone de la couche
   `bridges` (`man_made=bridge` ; les morceaux d'un polygone coupés aux bords de tuile comptent
   pour un) ; deux polygones différents font deux ponts ; (b) sinon, si l'une au moins des deux n'est
   dans aucun polygone `bridges`, quand la concurrente reste à 10 m au plus de la voie de l'ouvrage
   sur toute l'emprise (21 points de A à B, extrémités comprises). Tunnels : inchangés (pas de
   couche d'emprise). Vérifié sur les tuiles du banc (`versatiles-shortbread-z14-d2e31ed09602913b`) :
   `nantes-pirmil` (route, trottoir à 8 m, tram à 18 m, piste à 23 m : même polygone),
   `nantes-audibert` (route, trottoir, tram, piste à 23 m : même polygone), `mauves` (route et piste :
   même polygone), `nv-32` (piste et route à 4 m : même polygone) et `nv-45` (aucun polygone,
   trottoir à 4,7–4,8 m sur toute l'emprise : (b)) redeviennent un seul ouvrage ; `nv-40`
   (passerelle hors de tout polygone, pont routier dans un polygone, à 8 m au milieu mais à 34 m
   d'une extrémité) reste un pont distinct, et ses passages restent `passage_sur_ouvrage_parallele`.
   Motif : au rapport 2026.11.2, ces ponts sortaient d'E3 et `nantes-audibert` perdait la moitié de
   ses passages, attribués à la piste du même tablier.
3. **M1 hors graphe vélo.** Une station de la trace recalée (pas de 5 m) dont la voie la plus proche
   de la couche `streets`, à 30 m au plus (tuiles de la campagne qui juge la trace, tuile et ses
   8 voisines, comme §4 bis.6), est fermée au vélo par la règle de tags du service (T03 §4.2,
   étape 1 : `trunk` en France, `bicycle=no`, motorway, pedestrian, bridleway, busway ; `bicycle=*`
   prime) sort de M1 (non apparié, longueur totale, `unknown`). Elle est rapportée à part
   (`hors_graphe_velo_m` par trace), avec un **contrôle** : le service doit y rendre
   `matched: false` avec l'avertissement `unmatched_range` ; une station de cette plage appariée
   est un échec de M1. Les tags sont lus dans les tuiles, jamais dans la réponse du service. Une
   station déjà `hors_reseau` (§4 bis.6) y reste. **Risque de circularité, accepté par
   l'utilisateur** : la règle reprend la règle de tags du service ; un service qui fermerait à tort
   une voie la voit sortir de M1 au lieu d'échouer. La règle de tags est fixée par T03, pas réglée
   sur le banc, et le contrôle garde l'autre sens (une voie fermée ne doit pas être appariée).
   Motif : au jugement du MVP, 4,6 km de `perso-23` suivaient des voies `trunk` (dont
   `bicycle=no`), GPS brut à 2–4 m, toute autre voie à plus de 40 m ; le matcher y faisait ce que
   la règle prescrit.
   **Resserrée** (décision de l'utilisateur, 2026-11-01, après la première calibration V1, règles
   2026.11.2) : la règle ne vise que les **routes motorisées fermées au vélo**, `motorway`, `trunk`
   et les routes `bicycle=no` (`kind` de route motorisée, comme l'état hors graphe de T03 §5.3,
   étape 6), sur des **plages continues d'au moins 50 m** de la trace recalée. Rues piétonnes,
   trottoirs et sentiers `bicycle=no`, `busway`, escaliers et simples traversées (plage de moins de
   50 m) restent jugés par M1. La « voie la plus proche » ne compte que les voies de la couche
   `streets` qui ne sont pas des rails. Motif : la règle précédente contredisait T03 §5.3, où le
   matcher apparie volontairement ces portions non motorisées à la voie cyclable voisine, et faisait
   échouer M1 sur 16 traces de calibration, souvent sur un carrefour.
4. **Couverture de la série tenue à l'écart V1.** Constat au jugement du MVP : 1 seul tunnel E4
   compté, 15 ponts E3, aucun croisement E5, aucun témoin négatif S2, aucun parcours R1/R2. Les deux
   derniers étaient structurels : la règle 2 de `tools/series_groups.py` met d'office en
   calibration les parcours d'osm-surface et les négatifs. Minimums, comptés **après** les
   exclusions du banc (hors graphe vélo, `limite_donnees`, `absent_des_donnees`, référence
   indisponible ou incertaine, passage retiré au niveau, passage sur ouvrage parallèle) :
   | Critère | Minimum sur la série tenue à l'écart | Source possible |
   |---|---|---|
   | E1 | ≥ 8 traces France fiables, ≥ 800 km comparés | perso (baro) |
   | E2b | ≥ 3 traces par groupe (dplus, plaine, ouvrage) | perso |
   | E3 | ≥ 12 ponts comptés, dont ≥ 3 à MNH ≥ 10 m et ≥ 3 de plus de 150 m | perso |
   | E4 | ≥ 7 tunnels OSM comptés (le seuil 6/7 n'a pas de sens en deçà), dont ≥ 2 de plus de 300 m | MNT seul |
   | E5 | ≥ 8 croisements sous un pont | MNT seul |
   | S1 | ≥ 15 ouvrages, ≥ 2 km d'emprise | OSM seul |
   | S2 | ≥ 4 témoins négatifs, ≥ 40 km, ≥ 3 natures (remblai / déblai, couvert arboré, bâti ou falaise, digue) | OSM et MNT |
   | R1 / R2 | ≥ 4 parcours, ≥ 400 km, ≥ 10 % non revêtu, régions hors osm-surface | parcours planifiés |
   | M1 | toutes les traces, ≥ 800 km | — |
   | Q1 | ≥ 2 traces négatives (baro défaillant) | perso |
   « MNT seul » / « OSM seul » : la référence ne demande pas de baro ; ces cas peuvent être des
   itinéraires planifiés, composés de ways OSM à travers des ouvrages choisis.
   Constitution :
   - **Données nouvelles seulement** : rien de la calibration (toute la série jugée au MVP
     comprise), de vcyclist ni d'osm-surface ; ouvrages absents de la calibration, fenêtres de
     ± 500 m comprises. Sources : activités Strava non encore qualifiées (`qualify_baro.py`, période
     du GPS défectueux toujours exclue), sorties récentes, itinéraires planifiés.
   - **Groupe E2b d'une trace nouvelle** (décision de l'utilisateur, 2026-11-01) : `dplus` si le
     D+ MNT (`qualify_baro.py`, `gain_dem_m`) est d'au moins 11 m/km ; sinon `ouvrage` si la trace
     passe sur au moins 2 ouvrages comptés (après exclusions) ; sinon `plaine`. Calculé avant le
     tirage, sans le service. Au MVP, le groupe était choisi à la main par l'intention de la sortie ;
     une règle est nécessaire pour un tirage figé sur des sorties non choisies.
   - **Négatif Q1 d'une série nouvelle** (décision de l'utilisateur, 2026-11-01) : sortie de la
     période du GPS défectueux (2015-05 → 2020-09) dont le défaut est attesté **hors du
     qualificateur** : D+ Strava inférieur à 60 % du D+ MNT au scan des polylignes
     (`strava_scan_dplus.py`), confirmé sur le stream complet (D+ enregistré < 60 % du D+ MNT de
     `qualify_baro.py`). Une sortie de la période sans défaut attesté n'est pas un négatif : sur 6
     tirées par empreinte en V1, 5 étaient saines (D+ conforme, bruit 1,1–1,8 m). Motif : choisir
     les négatifs par le verdict du qualificateur rendrait Q1 circulaire.
     **Complément** (décision de l'utilisateur, 2026-11-01) : le défaut inverse, altitude GPS en
     excès, est aussi un négatif : D+ Strava supérieur à 1,6 × D+ MNT au scan **et** confirmé sur le
     stream complet (D+ enregistré > 1,6 × D+ MNT de `qualify_baro.py`), comme `neg-03` et `neg-04`.
   - **Tirage stratifié, figé avant toute mesure.** Strates, dans cet ordre : E4, E5, S2, R1/R2, E3,
     Q1, puis le reste. Dans chaque strate, tri par sha256(`osm-route-details:v1:` + id) ; le cas va
     à la série tenue à l'écart jusqu'au minimum et jusqu'à au moins 1/3 de la strate. Un cas
     affecté par une strate précédente garde son affectation.
     **Marge du tirage** (décision de l'utilisateur, 2026-11-01) : la strate est remplie jusqu'à son
     minimum **majoré de 25 %**, arrondi à l'entier supérieur sur chaque composante du minimum
     (nombre de cas, km, sous-comptes comme « dont > 300 m » ou « dont > 150 m », part non revêtue,
     nombre de natures), ou jusqu'à la strate entière si elle est plus petite ; la règle du tiers
     s'applique aussi quand elle donne plus. Le seuil de **jugement** reste le minimum de la table
     ci-dessus. Motif : remplie jusqu'au minimum exact, une strate s'arrête juste au-dessus
     (S2 à 40,3 km pour 40 au contrôle du 2026-11-01), et une seule exclusion découverte à la
     campagne la ferait passer dessous, ce qui vaut refus.
   - **Nature d'un témoin S2 planifié** (décision de l'utilisateur, 2026-11-01). Un témoin planifié est
     un cas, de nature déclarée ; sa longueur comptée exclut les abords d'ouvrage (100 m) et les plages
     hors graphe vélo (point 5). La nature déclarée n'est retenue que si OSM la confirme sur au moins
     50 % de la longueur comptée ; sinon le témoin prend la nature que ses tags confirment sur au
     moins 50 %, et il est retiré si aucune ne l'est. Confirmation, à chaque station : couvert arboré,
     dans un polygone `landuse=forest` / `natural=wood` ou à 20 m au plus de son bord (forêts en
     parcelles bordées par la route) ; bâti ou falaise, `natural=cliff` à 100 m au plus (falaise
     au-dessus de la route, gorges) ; digue, `man_made=dyke` à 30 m au plus ; remblai / déblai,
     `embankment=yes`, `cutting=yes` ou `man_made=embankment` à 30 m au plus. Le bâti n'est pas lu.
     Cas du 2026-11-01 : la « Digue à la Mer » de Camargue, taguée `embankment=yes`, devient
     remblai / déblai ; le témoin du Tarn en amont (falaise confirmée sur 11 %) est reclassé selon
     ses tags, ou retiré.
   - **Contrôle de couverture sans le service** (corpus, tuiles, MNT), écrit dans le manifest. Un
     minimum non atteint impose de collecter d'autres cas, jamais de changer le sel.
   - **Au jugement**, un critère sous son minimum (exclusions découvertes à la campagne) vaut refus,
     comme un critère sauté (§2, point 3).
   - **R1 / R2** : référence Stadia Maps `trace_attributes` des parcours planifiés (aucune donnée
     perso), demandée la même semaine que la date des tuiles de la campagne, pour limiter l'écart
     de version qui a rendu R3 non bloquant. Réponses hors git, régénérables (conditions de Stadia).
     **Source** (décision de l'utilisateur, 2026-11-01) : Valhalla FOSSGIS (§2.1), 1 requête par
     seconde ; données du serveur du 2026-11-01, comme les tuiles V1. Réponses hors git,
     régénérables.
5. **S2 hors graphe vélo** (décision de l'utilisateur, 2026-11-01, après la première calibration
   V1, règles 2026.11.2). Les portions d'un témoin négatif où la trace suit une voie fermée au vélo
   ne comptent pas dans S2 (ni ouvrage fictif, ni longueur typée, ni longueur totale) : ce sont les
   plages hors graphe vélo de M1 (point 3, version resserrée : route motorisée fermée au vélo, plage
   continue ≥ 50 m, tuiles de la campagne qui juge la trace, jamais la réponse du service). Elles
   sont rapportées à part par témoin (`hors_graphe_velo_m`). E5 ne change pas : c'est la voie du
   passage dessous qui compte (§4 bis.1). Motif : en V1, sur une voie fermée au vélo, le service rend
   les ponts OSM réels de cette voie (avertissement `bicycle_forbidden`, T03 §5.3 étape 6) ; ce ne
   sont pas des ouvrages fictifs. Cas déclencheurs : `n08-nantes-centre` (1 ouvrage, 30 m) et
   `n09-peage-lancon` (2 ouvrages, 45 m), S2 en échec au rapport 2026.11.2.
6. **Ouvrage réel sur la voie d'un témoin négatif** (décision de l'utilisateur, 2026-11-01). Un
   ouvrage réel tagué dans OSM sur la voie même d'un témoin négatif (`bridge`, `tunnel`, y compris
   `tunnel=avalanche_protector`, ou `covered=yes`), même absent du schéma de la source du service,
   n'est jamais un ouvrage fictif : sa plage (de A à B, plus la marge `x` de 35 m) est retirée de S2
   (ouvrages fictifs, longueur typée, longueur totale) et rapportée à part, en `limite_donnees`
   (§4 bis.4) si le schéma de la source ne le porte pas (`avalanche_protector`, `covered`), sinon
   comme ouvrage. Le témoin ne sort entièrement de S2 que si l'ouvrage réel en est l'objet même
   (catégorie du témoin) ou en couvre plus de la moitié de la longueur. Cas : `tn12-arly-galleries`
   (galeries pare-avalanche, objet du témoin) sort de S2, ses galeries rejoignent les ouvrages
   mesurés en limite des données ; `tn10-roselend` (couronnement de barrage tagué `bridge`, 46 % du
   témoin) reste, la plage du couronnement masquée. Vérification du 2026-11-01 (tags OSM actuels de
   347 voies, API OSM) : ouvrages tagués aussi sur la voie de `tn08` (dont `avalanche_protector`),
   `tn02`, `tn07`, `tn09`, `tn11`, `hn01`, `hn03`, `hn06`, `n04`, `n08` (dont `covered`), `n09`
   (`covered`), `n12`, `n14` : déjà en plages X ou O, ils restent dans S2, plages masquées.
   **Réserve** : ces tags sont ceux du 2026-11-01, pas ceux de la base OSM des témoins vcyclist
   (2026-10-13) ; la règle s'applique aux tags de la base des témoins. Motif : détecter une galerie
   n'est pas inventer un ouvrage ; seul un ouvrage absent d'OSM peut être fictif.
   **Précisions de lecture** (rédaction, pas une nouvelle décision ; relevées à l'implémentation,
   rapport 2026.11.4) :
   - *voie du témoin* : l'identifiant de way porté par chaque station des fichiers de stations des
     témoins (`analysis/truth/controls/<id>.csv`, `analysis/truth/tunnels/<id>.csv`, colonne `way`),
     plages X et O comprises ; les plages de `spans.csv` ne suffisent pas. `covered=yes` est donc
     lisible : stations O de `n08` (6) et `n09` (8) au 2026-11-01. Les tags viennent de la base OSM
     du témoin (réserve ci-dessus), pas de la copie vcyclist, qui ne les porte pas ;
   - *objet du témoin* : la catégorie vcyclist du témoin nomme la nature d'un ouvrage que le témoin
     emprunte (galerie, tunnel, pont, viaduc) ; un ouvrage que le témoin longe ou passe dessous
     (`under-viaduct-approach`, `quay-under-bridges`) n'en fait pas l'objet. `dam-crest-road`
     (`tn10`) nomme un barrage, pas un pont : `tn10` reste ;
   - *rapportée en `limite_donnees`* : avec son erreur d'altitude mesurée contre sa référence,
     comme au §4 bis.4, sans seuil ; pour une plage `covered=yes` (voie couverte, au niveau du sol),
     la référence est le MNT aux stations de la plage ; une galerie (`tunnel=avalanche_protector`)
     est mesurée comme un tunnel ;
   - *plus de la moitié* : ouvrage par ouvrage, sans additionner les ouvrages d'un même témoin.
     Sans effet sur les témoins actuels : la somme ne dépasse pas non plus la moitié (au plus 46 %
     pour `tn10`, 14 % pour `tn12`, sorti par sa catégorie).
7. **Référence d'E5 au croisement** (décision de l'utilisateur, 2026-11-01, après la calibration
   2026.11.5-bench1 : E5 221/240). Pour chaque passage sous un pont, dans cet ordre, sans lire le
   service :
   - **retiré** (rapporté `e5_trace_sur_ouvrage`, non compté) si la trace est elle-même sur un
     ouvrage au croisement : une voie `bridge` ou `tunnel` de la couche `streets` des tuiles de la
     campagne passe à 10 m au plus du croisement en faisant moins de 20° avec la trace (le pont du
     dessus la croise à 20° au moins par construction) ;
   - **référence = baro corrigé localement** (décalage = médiane baro − MNT de 200 à 1 500 m de part
     et d'autre, comme au §2.1) au croisement, interpolé sur la grille de 5 m, si la trace est une
     trace perso fiable dont le baro existe des deux côtés du croisement sans pas brut de plus de
     100 m (`baro_gap_max_m`). Tolérance **2 m** avec cette référence, comme E3 (décision de
     l'utilisateur, 2026-11-01, après la calibration 2026.11.6) ; référence **incertaine**
     (rapportée `e5_reference_incertaine`, non comptée) si les deux décalages, médiane baro − MNT
     de 200 à 1 500 m de chaque côté pris à part, diffèrent de plus de **2 m**. Effet sur la
     calibration 2026.11.6, sans le service (MNT du cache ; 153 des 174 passages au baro mesurables
     ainsi, les 21 autres sur des traces en plusieurs parties ou sans pas brut au croisement) :
     14 passages incertains, dont 6 sans baro sur l'un des côtés ; des 17 échecs au baro, 2 deviennent
     incertains (`nv-328`, `nv-432`), 12 passent à 2 m, 3 restent (`nv-268`, `nv-269`, `nv-352`,
     écart de 2,1 à 2,9 m, côtés à 0,4 m au plus l'un de l'autre) ;
   - **sinon référence = MNT LiDAR**, **indisponible** (rapporté `e5_reference_indisponible`, non
     compté) si le MNT est sans donnée à 10 m au plus du croisement, ou s'il lit le tablier :
     MNT au croisement − moyenne des médianes du MNT entre 20 et 50 m de part et d'autre, le long de
     la voie du dessous, > **2,0 m** (deux fois la tolérance MNT d'E5 ; un gabarit sous tablier fait
     au moins 4,5 m). Bosse entre **1,0 et 2,0 m** : référence **incertaine** (rapportée
     `e5_reference_incertaine`, non comptée ; décision de l'utilisateur, 2026-11-01, après la
     calibration 2026.11.6) : une bosse de l'ordre de la tolérance ne distingue pas le tablier du sol.
     Une cuvette (écart négatif) reste une référence : les passages dessous creusent. Effet sur la
     calibration 2026.11.6, sans le service : 3 des 29 passages au MNT deviennent incertains
     (`nv-361` sur `perso-06`, bosse 1,9 m ; `nv-461`, 1,8 m ; `n05-canal-midi-platanes#1`, 1,5 m),
     dont les 2 échecs au MNT.
   Lecture de la règle (précisions de rédaction, 2026-11-01) :
   - « trace perso fiable » = verdict `fiable` de `qualify_baro.py` ; une trace en plusieurs parties
     se lit partie par partie ;
   - « trace sur un ouvrage » : couche `streets` de la tuile du croisement et de ses 8 voisines
     (voisinage du §4 bis.6), lignes `bridge` ou `tunnel` ; la couche `bridges` (polygones) n'y
     entre pas ;
   - « MNT sans donnée » : no-data ou hors de la couverture IGN LiDAR HD, sans repli sur une
     autre source ;
   - une approche (20–50 m d'un côté) sans aucune station de MNT : bosse non mesurable, référence
     indisponible ;
   - plages `O` des témoins vcyclist (sans point de croisement, sans baro) : croisement au milieu de
     la plage, référence MNT, écart retenu = écart max sur la plage. Les approches 20–50 m du milieu
     tombent en partie dans la plage quand elle dépasse 40 m (25 à 80 m sur ces témoins) : point
     ouvert, à trancher par l'utilisateur.
   Le « trou comblé » du MNT (gap_filled du service) n'est pas lisible dans la donnée : le MNT
   LiDAR HD de l'IGN est déjà interpolé sous les tabliers (aucun no-data sur 218 croisements de
   calibration) ; c'est la bosse qui le trahit. Vérification sur la calibration 2026.11.5-bench1
   (218 croisements des traces perso, MNT du cache, seuils fixés avant lecture des cas) :
   34 retirés (dont 12 des 19 échecs) ; 17 au MNT qui lit le tablier (dont 6 échecs : la bosse,
   de 2,5 à 8,6 m, explique l'écart du service) ; aux croisements à bosse, le baro corrigé suit les
   approches à 1 m près, ce qui confirme que le MNT lit le tablier. Effet attendu avec la référence
   baro : les 6 échecs à bosse passent si le service suit le sol ; à l'inverse, 11 croisements
   aujourd'hui réussis à écart nul (service = MNT qui lit le tablier, bosse de 2,5 à 11 m) échoueront :
   le service y lit aussi le tablier. Échec restant au MNT seul : `nv-416` (bosse 1,7 m, sous le
   seuil). Série tenue à l'écart (MNT par l'API IGN, tuiles V1, sans le service) : 105 croisements,
   78 gardent une référence (tous au baro), au-dessus du minimum (8) et de la cible (10).
   Recompté après la calibration 2026.11.6 en testant aussi le verdict `fiable` et le pas brut au
   croisement (le premier comptage ne les testait pas) : 77 croisements gardent une référence
   (59 au baro, 18 au MNT), 27 sont retirés (trace sur un ouvrage), 1 est indisponible. Avec la
   tolérance baro et les deux références incertaines du 2026-11-01 : **65** croisements gardent une
   référence (48 au baro, 17 au MNT), 10 ont un baro incertain (côtés à plus de 2 m), 1 une bosse
   MNT de 1 à 2 m, 1 n'a de baro que d'un côté du croisement ; toujours au-dessus du minimum (8) et
   de la cible (10).
8. **Référence incertaine en E1 plaine** (décision de l'utilisateur, 2026-11-01, après la
   calibration 2026.11.5-bench1). Une trace du groupe `plaine` dont le bruit baro mesuré par
   `qualify_baro.py` (`noise_m`, 1,4826 × MAD du résidu baro − MNT après retrait de tendance)
   dépasse **1,0 m** a une référence incertaine en E1 : rapportée, non jugée. Motif : le seuil
   plaine (RMSE ≤ 1,5 m) est du même ordre que ce bruit, qui passe dans l'erreur mesurée
   (`perso-58`, bruit 1,44 m → RMSE 1,77 m ; `perso-76`, 1,05 m → 1,22 m), alors que le critère
   « fiable » du corpus admet jusqu'à 2,5 m. Le groupe `dplus` (seuil 3,0 m) n'est pas concerné ;
   E2b non plus. Effet : 2 traces plaine de calibration V1, 2 du MVP ; série tenue à l'écart V1 :
   2 sur 8, il reste 28 traces fiables et 3 083 km pour E1 (minimum 8 / 800, cible 10 / 1 000).

---

## 5. Protocole : MNS lu sur la trace GPS ou sur la géométrie OSM du pont (§11.2 du doc 01)

**Question.** Quand la trace GPS passe à côté du tablier, le profil du tablier est-il meilleur lu dans
le MNS aux points de la trace (méthode **A**, celle de vcyclist) ou aux points projetés sur la
géométrie OSM du pont (méthode **B**) ?

**Cas.** Ponts tagués du corpus traversés par une trace baro fiable : Saint-Nazaire (`perso-20..22`),
Pirmil (`perso-26`), Anne-de-Bretagne (`perso-29`), Audibert (`perso-25..29`), Mauves (`perso-27`).
Complément sans baro : ponts de `test1..3` de vcyclist (référence MNS seule, voir piège 1).

**Mesure.** Pour chaque passage :

1. Emprise du tablier : way(s) OSM `bridge=*` de `data/ouvrages-osm/`, projetées sur la trace.
2. Écart latéral trace–OSM : distance de chaque station à l'axe de la way ; on retient la médiane et
   le P95 par passage, et on classe le passage en 0–5 m, 5–15 m, > 15 m.
3. Méthode A : MNS (`ign_lidar_hd_mnx_multi_wld`) aux stations de la trace (pas 5 m), médiane
   glissante 25 m.
4. Méthode B : stations projetées orthogonalement sur l'axe OSM densifié à 5 m, puis MNS et même
   médiane 25 m.
5. Référence : baro corrigé du décalage local (§1.5) aux mêmes stations.
6. Métriques par passage : RMSE et erreur max (méthode − référence) sur le tablier, erreur aux deux
   culées (premiers et derniers 25 m).
7. Test de robustesse : décaler artificiellement la trace de 5, 10 et 15 m latéralement (bruit GPS
   simulé) et recalculer A et B.

**Règle de décision, écrite avant la mesure.** B est retenue si, sur la calibration :
RMSE médiane(B) ≤ RMSE médiane(A) − 0,3 m, aucun pont n'est dégradé de plus de 0,5 m en erreur max,
et B gagne dans la classe > 5 m d'écart latéral. Puis une seule vérification sur la série tenue à
l'écart : B ne doit pas y perdre. Sinon A reste (doc 01 §11.2). Avec l'extension du §2, la série
tenue à l'écart compte 17 ponts français franchis par une trace baro (de 54 à 571 m).

---

## 6. Reste à faire, et à fournir par l'utilisateur

- Qualifier les 3 traces hors France contre Mapterhorn (pas de MNT IGN).
- Conditions de Stadia Maps sur le stockage des réponses : dans le doute, hors git et régénérables.
- Vérifier l'accès vélo des ponts du corpus (Krk, Venise, Cheviré `bicycle=no`), la source réelle
  GLO-30 de Krk, et trancher RGE ALTI vs MNT LiDAR au tunnel d'Arbois.
- Constituer une nouvelle série tenue à l'écart après le jugement du MVP.

## 6 bis. Taille du cache de banc

Comptage des tuiles distinctes (union de tous les cas, tampon de 2 px) le 2026-10-30, pour les
traces perso (43 traces, 4 663 km), `test1..4` (156 km), osm-surface (185 km), les 65 témoins de
vcyclist et les 36 ouvrages :

| Source | Tuiles | Taille unitaire | Taille |
|--------|--------|-----------------|--------|
| IGN MNT z15 (BIL32 256 px, France) | 4 667 (dont 4 234 pour les traces perso ; 190 pour `test1..4`, comme vcyclist) | 256 Ko | ≈ 1,2 Go |
| IGN MNS z15 (ponts seulement) | ≤ 4 667 | 256 Ko | 0,1 à 1,2 Go (borne basse : un pont toutes les ~10 tuiles) |
| Mapterhorn z12 | 463 | ≈ 144 Ko (vcyclist : 2,3 Mo / 16) | ≈ 67 Mo |
| VersaTiles z14 | 2 214 | 50 à 130 Ko (osm-surface : 11 Mo / 84 tuiles, urbain) | 0,1 à 0,3 Go |
| **Total** | | | **≈ 1,5 à 2,8 Go, probablement ≈ 1,7 Go** |

Sans les traces perso : ≈ 650 tuiles IGN, soit ≈ 250–350 Mo comme prévu. Les traces perso
représentent 90 % du cache ; il croît à peu près linéairement avec les kilomètres de zones nouvelles
(les boucles nantaises se recouvrent). Mesure exacte au premier `fill-cache`.

## 7. Outils

| Script | Rôle |
|--------|------|
| `tools/strava_scan_dplus.py` | Balayage grossier de l'inventaire : D+ Strava vs D+ RGE ALTI sur polyligne réduite |
| `tools/qualify_baro.py` | Qualification d'une altitude enregistrée contre le MNT (décalage, dérive, bruit, D+) |
| `tools/stream_to_gpx.py` | Stream Strava → GPX, rognage optionnel des extrémités |
| `tools/expurge_route_json.py` | Expurge un export tribly d'osm-surface (géométrie et revêtement seulement) |
| `tools/stadia_match.py` | Recalage Stadia, baro transporté, revêtement de référence, découpage ≤ 200 km (§2.1) |
| `tools/build_perso_corpus.py` | Sélection → GPX, empreintes, séries, fragment du manifest |
| `tools/deck_baro_vs_lidar.py` | Baro corrigé vs MNS LiDAR au milieu des ouvrages |
| `tools/series_groups.py` | Groupes d'ouvrages, séries, passages masqués (§2) |
| `tools/fetch_hors_git.py` | Régénère les fixtures `hors_git` du manifest depuis leur dépôt et commit, vérifie le sha256 |
| `tools/ouvrages/` | Requêtes Overpass et scripts de constitution de `data/ouvrages.yaml` ; `mns_median25.py` ; `candidats_traces.py` (extension par les traces perso) |

API utilisées : Géoplateforme altimétrie (`data.geopf.fr/altimetrie`, ≤ 4 req/s, User-Agent
identifiable), Overpass (quelques requêtes), MCP Strava (lecture seule).
