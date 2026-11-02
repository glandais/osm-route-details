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

L1 et L2 (latences) sont mesurés à part (`ord-bench latency`).
