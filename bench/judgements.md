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

L1 et L2 (latences) sont mesurés à part (`ord-bench latency`).

