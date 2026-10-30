# Banc qualité

Corpus, métriques et seuils d'acceptation d'osm-route-details, fixés **avant le code** pour que
chaque incrément du MVP ait une cible mesurable. Le contrat mesuré est celui de
`docs/design/02-contrat-interface.md` ; les décisions de conception sont dans
`docs/design/01-conception-generale.md` (§8.5, §10, §11).

| Fichier | Contenu |
|---------|---------|
| `manifest.yaml` | Une entrée par cas : fichier, région, ce qu'il teste, référence, licence, provenance, empreinte, série |
| `criteria.yaml` | Forme machine des seuils du §4 (brouillon, gelé avant le premier passage) |
| `data/ouvrages.yaml` | Corpus de 36 ouvrages (OSM way ids, tags, longueurs, MNT/MNS/MNH IGN aux extrémités et au milieu) |
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

`data/ouvrages.yaml`, 36 ouvrages, OSM lu via Overpass le 2026-10-30, référence IGN interrogée le même
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
- trace qui commence sur un pont : milieu du pont de l'île de Ré.

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

`data/ouvrages.yaml` porte, pour les 21 ponts français, `milieu_mns_mediane_25m` (11 points de −12,5 à
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

   **Extension avant le premier passage** (décision de l'utilisateur, 2026-10-30) : 51 ouvrages
   jamais vus par vcyclist et franchis par une trace perso fiable (46 ponts dont le MNH au milieu
   dépasse 3 m, 5 tunnels), trouvés par `tools/ouvrages/candidats_traces.py` (Overpass sur la boîte
   englobante de chaque trace rognée, sans envoyer la trace), puis sélectionnés mécaniquement : noms
   et abords (< 100 m) d'ouvrages vcyclist exclus, chaussées parallèles et ways homonymes fusionnées,
   au plus 3 ouvrages par trace (2 dans l'agglomération nantaise), les plus longs. Ils dérivent des
   traces perso et dessineraient des trajets habituels : ils restent **hors git**
   (`private/ouvrages-perso.json`, empreinte au manifest), comme les traces.

   Résultat (2026-10-30) : **22 ouvrages tenus à l'écart sur 87 (25 %)**, dont 19 nouveaux (17 ponts,
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
2. **Service** : la géométrie recalée est l'entrée. Les bornes d'entrée de l'API (200 km,
   16 000 points) s'appliquent : au-delà, l'outil la découpe en tronçons ≤ 200 km (`<id>.partN.gpx`,
   aujourd'hui `perso-06`, 205 km) ; les sorties des tronçons sont recousues par abscisse. La sortie
   n'est pas bornée.
3. **Comparaison au baro sur la géométrie recalée.** Chaque point d'origine apparié par Stadia est
   projeté sur la géométrie recalée ; son abscisse y porte l'altitude baro. Les points non appariés,
   ou à plus de 30 m de leur point apparié, sont écartés. Pourquoi la géométrie recalée plutôt que
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
4. **Référence baro** : retirer un décalage. Pour l'altitude, décalage global = médiane(baro − sortie
   du service) sur les stations `ground` ; pour un ouvrage, décalage local (1 à 3 km de part et d'autre).
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
| **E2b** | Les corrections ne dégradent pas le D+ | par groupe : médiane \|ln(D+ service / D+ baro)\| ≤ médiane \|ln(D+ service avec `structure_correction: false` / D+ baro)\| ; mêmes sources, géométrie recalée, preset `dem` et masques | idem |
| **E3** | Erreur d'altitude sur tablier | Par pont : erreur max et erreur au milieu de (service − référence) sur l'emprise du tablier | baro corrigé localement (seule référence indépendante) ; à défaut, médiane MNS 25 m (`ouvrages.yaml` `milieu_mns_mediane_25m`, vcyclist `B`), référence de **cohérence** seulement |
| **E4** | Erreur en tunnel | Par tunnel : erreur max de (service − droite entre portails) ; D+ fictif ajouté dans le tunnel | `ouvrages.yaml`, vcyclist `N` |
| **E5** | Passage sous un pont | \|service − MNT\| au croisement | `chevire-dessous`, `rotonde-dessous`, `perso-23..25` |
| **S1** | Rappel des ouvrages OSM tagués | Longueur des ponts/tunnels du corpus typés `bridge`/`tunnel` par le service ÷ longueur de référence | `ouvrages.yaml`, `structures.csv` |
| **S2** | Faux positifs d'ouvrage | Longueur typée `bridge`/`tunnel` hors de tout ouvrage de référence ÷ longueur totale ; nombre d'ouvrages fictifs | témoins négatifs vcyclist `n*`, `hn*` ; traces `dplus` |
| **S3** | Ouvrage non tagué | MVP : altitude au milieu de `stn-acces-nord` (mesurée, signalée, pas de seuil). V1 : détecté oui/non | baro `perso-20..22` |
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
| E2b | **Bloquant** : pour chaque groupe, médiane \|ln(D+ service / D+ baro)\| ≤ même médiane pour le service appelé avec `structure_correction: false` | Mesure l'apport des corrections d'ouvrage à sources égales (LiDAR HD, repli Mapterhorn). Le RGE ALTI, ≈ 7 m d'erreur en montagne, rendrait le critère presque toujours vrai (décision de l'utilisateur, 2026-10-30) |
| E3 | France, pont tagué : \|erreur au milieu\| ≤ 2 m et max ≤ 3 m (ponts < 300 m), ≤ 5 m (grands ponts) ; ≥ 90 % des ponts du corpus passent | vcyclist : ≤ 2 m Nantes, ≤ 5 m Saint-Nazaire, 18/21 = 86 % ; baro − MNS mesuré ≤ 1 m sur Saint-Nazaire, Pirmil, Mauves |
| E3 hors France | Mesuré et rapporté, pas de seuil (pont interpolé, `interpolated_structure`) | 22–40 m d'erreur attendus sur grands ponts bombés (doc 01 §9) |
| E4 | \|erreur max\| ≤ 5 m et D+ fictif ≤ 5 m par tunnel ; ≥ 6/7 des tunnels tagués | Critère Grands Goulets de vcyclist ; 6/7 hors échantillon ; Strava : +190 m d'erreur, +112 m de D+ fictif aux Grands Goulets |
| E5 | \|service − MNT\| ≤ 1 m au croisement, 100 % des cas | Erreur ≈ 50 m si le MNS est lu ; tolérance zéro |
| S1 | ≥ 95 % de la longueur des ouvrages tagués typée correctement | Le MVP se fonde sur OSM seul : seul l'appariement peut faire échouer |
| S2 | 0 ouvrage fictif sur les négatifs ; ≤ 0,5 % de longueur hors ouvrage typée pont/tunnel | Pas d'heuristique au MVP : un faux positif ne peut venir que d'un mauvais appariement |
| S3 | Pas de seuil au MVP (limite documentée, `structure: ground`) ; V1 : détecté | doc 01 §5.5 |
| R1 | ≥ 85 % par parcours (classe), ≥ 85 % hors inconnu | osm-surface 86,0–93,5 % ; ne pas régresser sous le pire cas |
| R2 | ≥ 97 % par parcours | osm-surface 98,1–99,3 % |
| R3 | Comme R1 et R2 (classe ≥ 85 %, revêtu / non revêtu ≥ 97 %) par trace perso recalée, contre Stadia `trace_attributes` | Même nature de référence que R1/R2 (Stadia/Valhalla), sur 4 600 km de régions variées |
| M1 | Non apparié ≤ 1 % de la longueur ; `unknown` ≤ 2 % | osm-surface : 0–8 points non appariés, inconnu 0–1,3 % |
| L1 | p95 ≤ 20 s pour 100 km en France à froid ; ≤ 5 s hors France | vcyclist : 130 km IGN à froid 19 s / 264 tuiles, Mapterhorn 3 s / 16 tuiles |
| L2 | p95 ≤ 500 ms pour 100 km à chaud | osm-surface : 55–77 ms pour la typologie ; marge pour l'élévation et l'assemblage |
| Q1 | 4/4 négatifs non `fiable` | Contrôle du banc |

Sur la série tenue à l'écart, un incrément est accepté si tous les seuils passent. Un échec isolé sur
un cas tenu à l'écart est consigné et analysé, mais ne se corrige pas sur ce cas (sinon il devient
calibration).

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
| `tools/ouvrages/` | Requêtes Overpass et scripts de constitution de `data/ouvrages.yaml` ; `mns_median25.py` ; `candidats_traces.py` (extension par les traces perso) |

API utilisées : Géoplateforme altimétrie (`data.geopf.fr/altimetrie`, ≤ 4 req/s, User-Agent
identifiable), Overpass (quelques requêtes), MCP Strava (lecture seule).
