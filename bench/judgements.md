# Jugements de la série tenue à l'écart

Un jugement par incrément, consigné qu'il passe ou non (bench §2.3, T05 §3.3).

## Jugement MVP (2026-11-01)

- Commit du service : `f1f9c9e` ; commit de gel : `f1f9c9e`
- `rules_version` : 2026.10.10 ; `api_version` : 1.0
- Révision du banc (`revision_banc`) : aucune
- `data_versions` : `{"elevation":[{"date":"2026-11-01","indicative":true,"products":["mnt","mns"],"role":"primary","source":"ign-lidar-hd","zoom":15}],"typology":{"indicative":true,"role":"primary","source":"versatiles-shortbread","zoom":14}}`
- Empreintes : manifest `c5d04b265cbbeed807dda525d44dafdb3a4a420a5a1cbdd4847acad00c66e470`, critères `caf05c9781754d171a57c599ee9ad87a3d320e5a7c79154c308bb0edc2afacb3`, règles du banc `ea7c9f203dab75744e65f211d4534f1e076576ac55262685d684a0ea5b73fb85`, données `16cd7587dd2798027ef7dc4cd51ec6607793036b7cdfa680c79633f885638a10`
- Rapport : `reports/2026-11-01-2026.10.10-jugement-MVP/results.json`
- Résultat : **refusé** (en échec ou sautés : E5, M1, R1_R2, S1, S2)

- Non bloquants en échec ou sautés, rapportés sans refuser : R3, S3, hors_graphe_velo

| Id | bloquant | distance | input_with_breakpoints |
|---|---|---|---|
| E1 | oui | pass | pass |
| E1_echecs | non | [] | [] |
| E2 | non | reported | reported |
| E2_indicatif | non | [{"groupe":"dplus","mediane":0.9861,"mediane_dans_bornes":true,"traces_hors_bornes":[]},{"groupe":"ouvrage","mediane":0.9943,"mediane_dans_bornes":true,"traces_hors_bornes":[]},{"groupe":"plaine","mediane":0.9757,"mediane_dans_bornes":true,"traces_hors_bornes":[]}] | [{"groupe":"dplus","mediane":0.9854,"mediane_dans_bornes":true,"traces_hors_bornes":[]},{"groupe":"ouvrage","mediane":0.9877,"mediane_dans_bornes":true,"traces_hors_bornes":[]},{"groupe":"plaine","mediane":0.976,"mediane_dans_bornes":true,"traces_hors_bornes":[]}] |
| E2b | oui | pass | pass |
| E2b_groupes_en_echec | non | [] | [] |
| E3 | oui | pass | pass |
| E3_echecs | non | [] | [] |
| E3_hors_france | non | reported | reported |
| E4 | oui | pass | pass |
| E4_echecs | non | [] | [] |
| E5 | oui | skipped | skipped |
| E5_echecs | non | [] | [] |
| M1 | oui | fail | fail |
| M1_echecs | non | ["perso-23"] | ["perso-23"] |
| Q1 | oui | pass | pass |
| R1_R2 | oui | skipped | skipped |
| R1_R2_echecs | non | [] | [] |
| R3 | non | fail | fail |
| R3_echecs | non | ["perso-13.s1"] | ["perso-13.s1"] |
| S1 | oui | fail | fail |
| S2 | oui | skipped | skipped |
| S3 | non | skipped | skipped |
| hors_graphe_velo | non | skipped | skipped |

L1 et L2 (latences) sont mesurés à part (`ord-bench latency`).

## Jugement V1 (2026-11-02)

- Commit du code mesuré (`crates/`, `tools/ord-bench/`, `Cargo.*`) : `059fd4e`
- HEAD au jugement : `945ed0a` ; commit de gel : `945ed0a`
- `rules_version` : 2026.11.6 ; `api_version` : 1.3
- Révision du banc (`revision_banc`) : `bench5`
- `data_versions` : `{"elevation":[{"date":"2026-11-02","indicative":true,"products":["mnt","mns"],"role":"primary","source":"ign-lidar-hd","zoom":15}],"typology":{"indicative":true,"role":"primary","source":"versatiles-shortbread","zoom":14}}`
- Empreintes : manifest `37c8b725f707d652e92e9eed4d35c6dfc948e3af538e912f05130013a366445b`, critères `d1e679031876a3109204be0e41a40ecae0c62b5f991465e5559970cd98618153`, règles du banc `ea7c9f203dab75744e65f211d4534f1e076576ac55262685d684a0ea5b73fb85`, données `717eb025c69dcd8c6a964804ac88c0e66690104089d23a9fd6ebca92d9ec605f`
- Rapport : `reports/2026-11-02-2026.11.6-bench5-jugement-V1/results.json`
- Résultat : **refusé** (en échec, sautés ou sous leur minimum de couverture : E1, E5, R1_R2)
- Non bloquants en échec ou sautés, rapportés sans refuser : E3_hors_france, S3

| Critère | bloquant | distance | input_with_breakpoints | ids en échec |
|---|---|---|---|---|
| E1 | oui | fail | fail | distance : perso-53 (dplus, RMSE 6,58 m, p95 1,90 m) ; input_with_breakpoints : perso-53 (RMSE 42,36 m, p95 1,96 m) ; références incertaines, non jugées (plaine, bruit baro > 1,0 m) : perso-45 (indicatif en échec), perso-80 (indicatif réussi) |
| E2 | non | reported | reported | distance : médianes dplus 1,048, ouvrage 1,016, plaine 0,951, toutes dans les bornes ; hors bornes : perso-78 (ouvrage, 0,858), perso-43, perso-45, perso-124 (plaine, 0,814 à 0,900) ; input_with_breakpoints : médianes dplus 1,052, ouvrage 1,015, plaine 0,949 (hors bornes) ; hors bornes : perso-53 (dplus, 1,552), perso-78 (ouvrage, 0,858), perso-43, perso-45, perso-124 (plaine, 0,808 à 0,900) |
| E2b | oui | pass | pass | — |
| E3 | oui | pass | pass | distance : nv-105, nv-210, nv-58, nv-79 ; input_with_breakpoints : nv-105, nv-58, nv-79 |
| E3_hors_france | non | skipped | skipped | — |
| E4 | oui | pass | pass | nv-228 |
| E5 | oui | pass | fail | distance : — ; input_with_breakpoints : nv-423 |
| M1 | oui | pass | pass | — |
| Q1 | oui | pass | pass | — |
| R1_R2 | oui | fail | fail | pl-r-vosges-nord |
| R3 | non | pass | pass | — |
| S1 | oui | pass | pass | — |
| S2 | oui | pass | pass | — |
| S3 | non | skipped | skipped | — |
| hors_graphe_velo | non | reported | reported | — |

Lignes E1 et E2 complétées à la main d'après `results.json` : l'affichage de `judge` (`aux_summary`)
y écrit le groupe au lieu de l'id et omet les ids hors bornes d'E2. Couvertures toutes au-dessus de
leur minimum, aucun cas sauté (43 cas). Premier passage de ce gel : le jugement du gel `243b7a6`
s'était interrompu sans verdict (témoin `perso-44-s0` sauté), avant les corrections `c47114d`,
`059fd4e` et `affc041` (décision de l'utilisateur, 2026-11-02).

Note (2026-11-02, après la calibration 2026.11.10-bench1) : défaut de l'outil, sans effet sur le
refus. Les témoins S2 `pl-e4-cajarc-s0` (342 m, `bati_ou_falaise`) et `pl-e4-cajarc-s1` (493 m,
`remblai_deblai`), tronçons de l'itinéraire E4 `pl-e4-cajarc` avec extrémités, ont été mesurés sur
tout l'itinéraire hors abords d'ouvrages (`longueur_m` 10 352,89 m chacun) au lieu de leur tronçon.
Aucun faux positif sur l'itinéraire entier (`faux_positif_m` 0), donc aucun sur les tronçons : le
verdict S2 (pass) reste juste ; seules les longueurs S2 sont gonflées (≈ 19,5 km). Un témoin avec
extrémités se mesure entre elles, même sur un itinéraire planifié (§4 ter.6) ; correction dans
ord-bench (bench2), après V1.

L1 et L2 (latences) sont mesurés à part (`ord-bench latency`).

## Jugement V1.1 (2026-11-02)

- Commit du code mesuré (`crates/`, `tools/ord-bench/`, `Cargo.*`) : `44ac2e3`
- HEAD au jugement : `c00c180` ; commit de gel : `c00c180`
- `rules_version` : 2026.11.10 ; `api_version` : 1.4
- Révision du banc (`revision_banc`) : `bench2`
- `data_versions` : `{"elevation":[{"date":"2026-10-31","indicative":true,"products":["mnt","mns"],"role":"primary","source":"ign-lidar-hd","zoom":15}],"typology":{"indicative":true,"role":"primary","source":"versatiles-shortbread","zoom":14}}`
- Empreintes : manifest `33855a1e5540df6774a3ae37c137c26711bec0e51679b17b7d596d1d3ed20dd6`, critères `d409048f54d168ac6b3c951b4f26ebda8bf9889c6d9528625f084435892a4292`, règles du banc `ea7c9f203dab75744e65f211d4534f1e076576ac55262685d684a0ea5b73fb85`, données `8aecbda074c7e7268471a47be81ea0cf98de1c57b7195336c382e4e1544d715a`
- Rapport : `reports/2026-11-02-2026.11.10-bench2-jugement-V1.1/results.json`
- Résultat : **refusé** (en échec, sautés ou sous leur minimum de couverture : E1, E5, R1_R2, S2)
- Non bloquants en échec ou sautés, rapportés sans refuser : E3, E3_hors_france, S3, hors_graphe_velo

| Critère | bloquant | distance | input_with_breakpoints | ids en échec |
|---|---|---|---|---|
| E1 | oui | fail | fail | distance : perso-136 (dplus, RMSE 3,28 m, p95 6,58 m) ; input_with_breakpoints : perso-136 (RMSE 3,25 m, p95 6,51 m) ; références incertaines, non jugées (15 cas, 14 traces), mêmes dans les deux modes : marche du baro (§4 ter.11) perso-135 (dplus, +10,2 m), perso-137 (dplus, −31,0 m, indicatif en échec), perso-144 (dplus, −12,4 m), perso-145 (dplus, +16,3 m, indicatif en échec), perso-143 (plaine, −11,3 m, indicatif en échec), perso-147 (plaine, +17,3 m) ; bruit baro > 1,0 m (plaine) : perso-131 et perso-138 (indicatif en échec), perso-139, perso-140, perso-141, perso-142.s1, perso-142.s2, perso-146, perso-148 |
| E2 | non | reported | reported | distance : médianes dplus 1,018, plaine 0,995, dans les bornes ; hors bornes (plaine) : perso-131 (0,762), perso-139, perso-142.s2, perso-143, perso-148, perso-149 (0,818 à 0,896) ; input_with_breakpoints : médianes dplus 1,015, plaine 0,997 ; mêmes traces hors bornes |
| E2b | oui | pass | pass | — |
| E3 | non | sous_minimum | sous_minimum | ponts 0 < 12, ponts_mnh_10m_min 0 < 3, ponts_plus_de_150m 0 < 3 |
| E3_hors_france | non | skipped | skipped | — |
| E4 | oui | pass | pass | — |
| E5 | oui | fail | fail | référence baro corrigé local pour tous ; nv-799 et nv-800 deux fois ; distance (20) : nv-664, nv-704, nv-795, nv-836, nv-694, nv-751, nv-798, nv-799, nv-799, nv-800, nv-800, nv-693, nv-719, nv-741, nv-754, nv-795, nv-683, nv-772, nv-804, nv-805 ; input_with_breakpoints (19, sans le second nv-795) : nv-664, nv-704, nv-795, nv-836, nv-694, nv-751, nv-798, nv-799, nv-799, nv-800, nv-800, nv-693, nv-719, nv-741, nv-754, nv-683, nv-772, nv-804, nv-805 |
| M1 | oui | pass | pass | — |
| Q1 | oui | pass | pass | — |
| R1_R2 | oui | fail | fail | pl11-r-millevaches (revêtu / non revêtu 0,9667 < 0,97 ; classe 0,9667 ; aucune erreur de la référence exclue) |
| R3 | non | pass | pass | — |
| S1 | oui | pass | pass | — |
| S2 | oui | fail | fail | deux modes : perso-157-s1 (bati_ou_falaise, 1 ouvrage fictif, 85 m), pl11-s2-verdon (couvert_arbore, 1 ouvrage fictif, 115 m) |
| S3 | non | skipped | skipped | — |
| hors_graphe_velo | non | skipped | skipped | — |

Lignes E1, E2, E5, R1_R2 et S2 complétées à la main d'après `results.json` : l'affichage de `judge`
écrit le groupe au lieu de l'id (E1) et ne nomme pas les témoins en échec (S2). Aucun cas sauté ;
gel `c00c180`, code mesuré `44ac2e3` (bench2), un seul passage, jusqu'au bout. Couvertures du
rapport : E1 23 traces, 3 912 km ; E2b dplus 7, plaine 31, ouvrage 0 (non exigé, §4 ter.10) ; E3
0 pont (sous le minimum, rapporté, §4 ter.10) ; E4 7 tunnels dont 2 de plus de 300 m ; E5 140
croisements ; S1 24 ouvrages, 3 484 m ; S2 38 témoins, 81,5 km, 4 natures ; R1_R2 5 parcours,
510 km ; M1 38 traces, 6 030 km ; Q1 2.

Anomalies constatées sans analyse de la série (en-têtes du rapport et du cache seulement) :
1. 60 tuiles VersaTiles téléchargées le 2026-11-02 entre 20:21Z et 20:31Z par le fill-cache du
   jugement, hors de la campagne datée (11:21Z–15:32Z, `tuiles_2026_11_02`) ; même jour, mais leur
   rattachement au build du 2026-10-27 n'est pas vérifié.
2. Quatre itinéraires V1 (`pl-e4-boisclair`, `pl-e4-cajarc`, `pl-e4-velay`, `pl-s2-tarn-amont`),
   calibration depuis le jugement V1, sont dans les cas du jugement V1.1 avec la série
   `tenue_a_l_ecart`, pour M1 seulement (série `par_cas` dans `series-v1.csv`, sortie tenue à
   l'écart dans le rapport) : défaut de sélection de l'outil ; M1 passe.
3. Références E1 incertaines : 15 cas (14 traces), dont 6 par marche du baro
   (§4 ter.11), contre 1 en calibration (`perso-44`) ; 2 marches à moins de 1 m du seuil de 10 m
   (`perso-135`, +10,2 / +10,1 m ; `perso-144`, Δ long −10,8 m).

Note (2026-11-03, diagnostic des échecs en lecture seule, après accord de l'utilisateur) : le
refus reste acquis ; V1.1 n'est pas rejugée et passe en calibration pour V1.2. Rien n'a été corrigé
sur un cas tenu à l'écart ; les règles qui suivent valent à partir de V1.2 (README §4 quater).
- **R1_R2, `pl11-r-millevaches` : la référence a tort.** Abscisse de l'itinéraire en distance
  équirectangulaire contre la haversine de `stadia_match.py` (rapport 1,00112, + 125 m au km 107 ;
  corrigée seule, accord estimé 0,978) ; aller-retour vers le km 31,6 sans point apparié, plage
  revêtue étirée sur environ 490 m ; piste des km 17,9 à 19,7 `grade1` dans les tuiles (OSM du
  2026-10-19) et `grade2` chez Valhalla et dans l'instantané du 2026-11-02. Règles : §4 quater.1.
- **S2, `perso-157-s1` et `pl11-s2-verdon` : le service a tort.** Tunnel heuristique sur le MNT
  là où le terrain ne monte que d'un côté de l'axe ; les témoins sont bien posés. Correction du
  service (tunnel heuristique seulement si le terrain monte des deux côtés), nouvelle version de
  règles.
- **E1, `perso-136` : surtout la référence.** Baro clairsemé, en avance d'environ 300 m au pied
  des montées (+ 20 à + 35 m en 150 m) ; sans deux zones, RMSE 2,86 m. `perso-137` montre le même
  motif mais a été écartée par la marche du baro : traitement incohérent de deux cas semblables.
  Règle : §4 quater.3.
- **E5 : 20 lignes en échec, 11 sites au diagnostic (10 selon §4 quater.4), 7 imputés au service
  et 13 à la référence.** Service : correction de l'overpass absente hors LiDAR (repli
  Mapterhorn), et fenêtre rejetée près d'une rampe au lieu d'être rognée (correction du service,
  nouvelle version de règles). Référence : baro bruité (6 ; règle §4 quater.2) ; retard résiduel
  du baro (1) ; et un groupe de 6 (`perso-131` / `nv-704`, `perso-140` / `nv-798`, `nv-799` et
  `nv-800` à ses deux passages) où la référence présente une bosse de + 3 à + 4 m sur un MNT plat,
  et où les deux passages de `perso-140` aux mêmes ponts diffèrent de 3 m alors que le MNT concorde
  à 0,8 m : un point GPS aberrant isolé par croisement (un point à plus de 30 m de la trace recalée,
  deux sur `perso-131` ; sur `perso-140`, son s_origine recule), pas un trajet parallèle. Ces 6 références
  restent suspectes, confiance moyenne ; la garde géométrique « trace brute / trace recalée »
  envisagée a été abandonnée (décision de l'utilisateur, 2026-11-03). Doublons : croisements
  comptés sous plusieurs ids ; règle §4 quater.4.
- **Anomalie 2, défaut de sélection** : les itinéraires `par_cas` de V1 ne passent pas en
  calibration une fois V1 jugée (`v1.rs`) ; correction dans ord-bench, hors banc.
- **Anomalie 1, 60 tuiles hors campagne** : l'emprise de `tuiles_v1.py` a été calculée à 14:23Z,
  avant la replanification de `pl11-r-millevaches`, `pl11-r-ardennes` et `pl11-r-larzac` (14:24Z à
  14:25Z) ; le fill-cache du jugement a complété. Impact jugé faible ; rattachement au build non
  vérifiable hors ligne. Corrections d'outil : campagne de tuiles verrouillée (GPX modifié après
  le calcul de l'emprise refusé ; téléchargement hors campagne refusé ou signalé au jugement).

L1 et L2 (latences) sont mesurés à part (`ord-bench latency`).
## Jugement V1.2 (2026-11-03)

- Commit du code mesuré (`crates/`, `tools/ord-bench/`, `Cargo.*`) : `04467ae`
- HEAD au jugement : `68aee72` ; commit de gel : `68aee72`
- `rules_version` : 2026.11.11 ; `api_version` : 1.4
- Révision du banc (`revision_banc`) : `bench4`
- `data_versions` : `{"elevation":[{"date":"2026-10-31","indicative":true,"products":["mnt","mns"],"role":"primary","source":"ign-lidar-hd","zoom":15}],"typology":{"indicative":true,"role":"primary","source":"versatiles-shortbread","zoom":14}}`
- Empreintes : manifest `83205d1761caaacfe0405552f48e814a62566be4697f483d42ba60f73237c615`, critères `dfcbe5f384cd45a955fdba53c343e6a72ea8aad5a31d09c2b325c3b5a8d50d37`, règles du banc `ea7c9f203dab75744e65f211d4534f1e076576ac55262685d684a0ea5b73fb85`, données `c12049c8e5368df31a5a8b2e7ac99491846e1d23d448edb91f662a61e6392cd8`
- Rapport : `reports/2026-11-03-2026.11.11-bench4-jugement-V1.2/results.json`
- Résultat : **refusé** (en échec, sautés ou sous leur minimum de couverture : E2b, E3)
- Non bloquants en échec ou sautés, rapportés sans refuser : E3_hors_france, Q1, S3, hors_graphe_velo

| Critère | bloquant | distance | input_with_breakpoints | ids en échec |
|---|---|---|---|---|
| E1 | oui | pass | pass | references incertaines : perso-173 ; perso-166 ; perso-171 ; perso-182 ; perso-184 ; perso-185 ; perso-186 ; perso-194 |
| E2 | non | reported | reported | distance : indicatif : dplus 1,003 ; ouvrage 0,982 ; plaine 1,021 ; input_with_breakpoints : indicatif : dplus 1,004 ; ouvrage 0,980 ; plaine 1,016 |
| E2b | oui | fail | fail | ouvrage |
| E3 | oui | fail | fail | distance : nv-1001, nv-1013, nv-1022, nv-1023, nv-1024, nv-1031, nv-1037, nv-1038, nv-1039, nv-1040, nv-1041, nv-1042, nv-1043, nv-1051, nv-1060, nv-1070, nv-966, nv-968, nv-970, nv-971, nv-973, nv-974, nv-983, nv-984, nv-985, nv-986, nv-989, nv-992 ; input_with_breakpoints : nv-1001, nv-1013, nv-1022, nv-1024, nv-1031, nv-1037, nv-1038, nv-1039, nv-1040, nv-1041, nv-1042, nv-1043, nv-1051, nv-1060, nv-1070, nv-966, nv-968, nv-970, nv-971, nv-973, nv-974, nv-984, nv-985, nv-986, nv-989 |
| E3_hors_france | non | skipped | skipped | — |
| E4 | oui | pass | pass | — |
| E5 | oui | pass | pass | — |
| M1 | oui | pass | pass | — |
| Q1 | non | sous_minimum | sous_minimum | traces_negatives 0 < 2 |
| R1_R2 | oui | pass | pass | — |
| R3 | non | pass | pass | — |
| S1 | oui | pass | pass | — |
| S2 | oui | pass | pass | — |
| S3 | non | skipped | skipped | — |
| hors_graphe_velo | non | skipped | skipped | — |

Aucun cas sauté ; gel `68aee72`, code mesuré `04467ae` (bench4), un seul passage mesuré, jusqu'au
bout (2026-11-03, 20:34:20Z à 20:43:40Z). Lignes du tableau telles qu'écrites par `judge`.

Anomalies constatées sans analyse de la série (en-têtes du rapport, du cache et des outils) :
1. **fill-cache refusé sur le premier gel** (`d944376`, 20:01Z) : 10 tuiles de rues absentes de la
   campagne `tuiles_2026_11_03` (toutes de `perso-168`), aucune téléchargée, `judge` non lancé.
   Cause : emprise de `tools/tuiles_v1.py` sur des points espacés de 50 m, sans les segments
   (trous de trace de plus d'une tuile) ; 20 tuiles manquaient sur la série (11 cas), aucune sur
   les traces de calibration. Emprise corrigée (`67906e6`), campagne complétée de 43 tuiles
   (20:12Z, même build, `meta.json` identique avant et après), gel refait (`98aef65`, décision de
   l'utilisateur). Avant le refus, environ 290 tuiles IGN MNS et autant de MNT écrites pour
   `perso-166` et `perso-167` ; aucune tuile Mapterhorn ni de rues.
2. **judge arrêté au démarrage sur le deuxième gel** (`98aef65`, 20:26:42Z, 5 s, rien mesuré ni
   consigné, aucun rapport) : instantané des tags OSM de référence attendu sous
   `2026-11-03T06-24-42Z.json` (date des données du serveur Valhalla à la seconde), nommé au
   manifest `2026-11-03T06-24Z.json`. Fichier renommé hors git, contenu inchangé, chemins du
   manifest mis à jour après contrôle en lecture seule de tous les fichiers chargés par `judge`
   (un seul écart), gel refait (`68aee72`, décision de l'utilisateur). Le seul passage mesuré est
   celui de cette entrée.
3. **Témoins S2 planifiés retirés mesurés** : `pl12-s2-isere` et `pl12-s2-nesque`, retirés au
   manifest (`retire:`, §4 quinquies.4), sont remplis par `fill-cache`, mesurés par `judge` et
   présents dans le rapport (série `tenue_a_l_ecart`) ; S2 passe, le verdict n'en dépend donc pas.
   Couverture S2 du rapport : 30 témoins, 62,6 km (contrôle de couverture d'avant mesure :
   30 témoins, 61,2 km) ; l'écart n'est pas analysé.

L1 et L2 (latences) sont mesurés à part (`ord-bench latency`).

