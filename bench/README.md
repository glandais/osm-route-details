# Banc qualité

Corpus, métriques et seuils d'acceptation d'osm-route-details, fixés **avant le code** pour que
chaque incrément du MVP ait une cible mesurable. Le contrat mesuré est celui de
`docs/design/02-contrat-interface.md` ; les décisions de conception sont dans
`docs/design/01-conception-generale.md` (§8.5, §10, §11).

| Fichier | Contenu |
|---------|---------|
| `manifest.yaml` | Une entrée par cas : fichier, région, ce qu'il teste, référence, licence, provenance, empreinte, série |
| `data/ouvrages.yaml` | Corpus de 36 ouvrages (OSM way ids, tags, longueurs, MNT/MNS/MNH IGN aux extrémités et au milieu) |
| `data/ouvrages-geom.json`, `data/ouvrages-osm/` | Géométrie et réponses brutes Overpass / IGN du corpus d'ouvrages |
| `data/deck_baro_vs_lidar.csv` | Première mesure : baromètre corrigé vs MNS LiDAR au milieu des ouvrages traversés |
| `tools/` | Scripts d'extraction et de qualification (Python 3, bibliothèque standard ; PyYAML via `uv run --with pyyaml`) |
| `private/` | **Hors git** : streams Strava, traces GPX personnelles, table pseudo → identifiant Strava |

---

## 1. Corpus

### 1.1 Bancs de vcyclist (référencés, non copiés)

vcyclist 6.11.0-1 (`4dfd6c5`, à jour le 2026-10-30), `docs/deck-strava-data/` :

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

### 1.2 Éval d'osm-surface (référencée, non copiée)

osm-surface (`44e5da6`, pas de branche amont configurée : fraîcheur non vérifiable). Trois parcours
autour de Nantes, 185,2 km (`testdata/*.gpx` + `*.route.json`). La référence est la classe de
revêtement **Stadia/Valhalla** calculée par tribly (`tracks[0].surfaces`), pas une vérité terrain.

| Parcours | Accord classe | Hors inconnu Stadia | Revêtu / non revêtu | Inconnu (nous) | Non appariés | Latence |
|----------|---------------|---------------------|---------------------|----------------|--------------|---------|
| graymond-ride-138 (90 km) | 86,0 % | 86,5 % | 98,1 % | 0,9 % | 3 pts | 77 ms |
| np-655-gravel (52 km) | 91,6 % | 91,5 % | 99,0 % | 1,3 % | 8 pts | 71 ms |
| np677-chill-gravel (43 km) | 93,5 % | 93,9 % | 99,3 % | 0,0 % | 0 pt | 55 ms |

Écarts expliqués à 59 % par la réduction Shortbread à `paved`/`unpaved`, 14 % par l'âge des
tuiles, ≈ 12 % par la référence elle-même. Réserves : conditions de réutilisation de Stadia à
vérifier ; `.route.json` à expurger (métadonnées d'équipe, URL tribly) avant toute copie ; pas de
licence de code déclarée.

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
`*.trim1km.gpx`). Aucune trace n'est versionnée sans accord explicite de l'utilisateur.

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

Le MNS LiDAR est une bonne vérité de tablier sur les grands ponts (≤ 1 m). Il ne l'est pas partout :
à Audibert, le MNS ponctuel lit 2 à 4,5 m au-dessus du baromètre (superstructure, lampadaires ou
véhicules) : la médiane sur 25 m est indispensable et le baromètre prime quand il existe. Le passage
sous Cheviré confirme le piège du MNS lu au croisement (≈ 50 m d'erreur).

---

## 2. Protocole

**Discipline** (reprise de vcyclist, doc 01 §6.4) :

1. Les **séries** sont fixées avant toute mesure du service, par une règle écrite :
   `sha256("osm-route-details:" + id)`, premier chiffre hexadécimal dans 0–4 → tenue à l'écart
   (≈ 5/16). Traces perso : 8 tenues à l'écart (7 `dplus`, 1 `ouvrage`). Ouvrages : 12 tenus à l'écart
   (liste dans `manifest.yaml`). Négatifs et hors France : calibration.
2. Les **seuils** du §4 sont gelés dans ce fichier avant le premier passage du service. Toute
   modification est un commit séparé, motivé, antérieur à la mesure qu'elle concerne.
3. La série tenue à l'écart est jugée **une seule fois par incrément** (MVP, V1…) : on note le commit
   du service, les `data_versions` et `rules_version`, et l'empreinte du manifest. Après jugement, elle
   devient calibration et une nouvelle série doit être constituée pour l'incrément suivant.
4. Les données externes sont figées par empreinte (manifest) : un fichier dont le sha256 a changé
   invalide la comparaison.

**Campagne de mesure** (une par incrément) :

1. Vérifier les empreintes du manifest (traces privées comprises).
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
| **E2** | D+ comparé au baro | ratio D+ service (preset `dem`) / D+ baro (hystérésis 2 m, fenêtre 15 m : preset `barometric`), par trace | traces `dplus`, `plaine`, `ouvrage` |
| **E3** | Erreur d'altitude sur tablier | Par pont : erreur max et erreur au milieu de (service − référence) sur l'emprise du tablier | baro corrigé localement ; à défaut, médiane MNS 25 m (`ouvrages.yaml`, vcyclist `B`) |
| **E4** | Erreur en tunnel | Par tunnel : erreur max de (service − droite entre portails) ; D+ fictif ajouté dans le tunnel | `ouvrages.yaml`, vcyclist `N` |
| **E5** | Passage sous un pont | \|service − MNT\| au croisement | `chevire-dessous`, `rotonde-dessous`, `perso-23..25` |
| **S1** | Rappel des ouvrages OSM tagués | Longueur des ponts/tunnels du corpus typés `bridge`/`tunnel` par le service ÷ longueur de référence | `ouvrages.yaml`, `structures.csv` |
| **S2** | Faux positifs d'ouvrage | Longueur typée `bridge`/`tunnel` hors de tout ouvrage de référence ÷ longueur totale ; nombre d'ouvrages fictifs | témoins négatifs vcyclist `n*`, `hn*` ; traces `dplus` |
| **S3** | Ouvrage non tagué | MVP : altitude au milieu de `stn-acces-nord` (mesurée, signalée, pas de seuil). V1 : détecté oui/non | baro `perso-20..22` |
| **R1** | Accord du revêtement vs Stadia | Part de longueur où `surface.class` = classe Stadia ; variante hors inconnu Stadia | osm-surface |
| **R2** | Accord revêtu / non revêtu | Idem sur la partition binaire, mètres classables des deux côtés | osm-surface |
| **R3** | Accord vs vérité terrain | Idem contre un relevé terrain, quand il existe (aucun pour l'instant) | à constituer |
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
| E2 | Par trace, D+ service / D+ baro dans [0,90 ; 1,20] ; médiane du groupe dans [0,95 ; 1,12] | MNT brut / baro mesuré : 0,96–1,21 (médianes 1,00 plaine, 1,07 relief) ; vcyclist +12 % ; critère vcyclist [0,8 ; 1,2] resserré car notre référence est un baromètre |
| E3 | France, pont tagué : \|erreur au milieu\| ≤ 2 m et max ≤ 3 m (ponts < 300 m), ≤ 5 m (grands ponts) ; ≥ 90 % des ponts du corpus passent | vcyclist : ≤ 2 m Nantes, ≤ 5 m Saint-Nazaire, 18/21 = 86 % ; baro − MNS mesuré ≤ 1 m sur Saint-Nazaire, Pirmil, Mauves |
| E3 hors France | Mesuré et rapporté, pas de seuil (pont interpolé, `interpolated_structure`) | 22–40 m d'erreur attendus sur grands ponts bombés (doc 01 §9) |
| E4 | \|erreur max\| ≤ 5 m et D+ fictif ≤ 5 m par tunnel ; ≥ 6/7 des tunnels tagués | Critère Grands Goulets de vcyclist ; 6/7 hors échantillon ; Strava : +190 m d'erreur, +112 m de D+ fictif aux Grands Goulets |
| E5 | \|service − MNT\| ≤ 1 m au croisement, 100 % des cas | Erreur ≈ 50 m si le MNS est lu ; tolérance zéro |
| S1 | ≥ 95 % de la longueur des ouvrages tagués typée correctement | Le MVP se fonde sur OSM seul : seul l'appariement peut faire échouer |
| S2 | 0 ouvrage fictif sur les négatifs ; ≤ 0,5 % de longueur hors ouvrage typée pont/tunnel | Pas d'heuristique au MVP : un faux positif ne peut venir que d'un mauvais appariement |
| S3 | Pas de seuil au MVP (limite documentée, `structure: ground`) ; V1 : détecté | doc 01 §5.5 |
| R1 | ≥ 85 % par parcours (classe), ≥ 85 % hors inconnu | osm-surface 86,0–93,5 % ; ne pas régresser sous le pire cas |
| R2 | ≥ 97 % par parcours | osm-surface 98,1–99,3 % |
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
l'écart (Saint-Nazaire, Mauves) : B ne doit pas y perdre. Sinon A reste (doc 01 §11.2).

---

## 6. Reste à faire, et à fournir par l'utilisateur

- **Accord de l'utilisateur** pour versionner tout ou partie des traces (entières ou rognées de 1 km),
  ou pour garder les identifiants Strava hors dépôt. Par défaut : rien de personnel n'est versionné.
- Qualifier les 3 traces hors France contre Mapterhorn (pas de MNT IGN).
- Vérité terrain du revêtement (R3) : aucune pour l'instant (relevé sur un parcours connu ?).
- Vérifier les conditions de réutilisation de la référence Stadia et l'absence de licence
  d'osm-surface avant toute copie de `testdata/`.
- Vérifier l'accès vélo des ponts du corpus (Krk, Venise, Cheviré `bicycle=no`), la source réelle
  GLO-30 de Krk, et trancher RGE ALTI vs MNT LiDAR au tunnel d'Arbois.
- Constituer une nouvelle série tenue à l'écart après le jugement du MVP.

## 7. Outils

| Script | Rôle |
|--------|------|
| `tools/strava_scan_dplus.py` | Balayage grossier de l'inventaire : D+ Strava vs D+ RGE ALTI sur polyligne réduite |
| `tools/qualify_baro.py` | Qualification d'une altitude enregistrée contre le MNT (décalage, dérive, bruit, D+) |
| `tools/stream_to_gpx.py` | Stream Strava → GPX, rognage optionnel des extrémités |
| `tools/build_perso_corpus.py` | Sélection → GPX, empreintes, séries, fragment du manifest |
| `tools/deck_baro_vs_lidar.py` | Baro corrigé vs MNS LiDAR au milieu des ouvrages |
| `tools/ouvrages/` | Requêtes Overpass et scripts de constitution de `data/ouvrages.yaml` |

API utilisées : Géoplateforme altimétrie (`data.geopf.fr/altimetrie`, ≤ 4 req/s, User-Agent
identifiable), Overpass (quelques requêtes), MCP Strava (lecture seule).
