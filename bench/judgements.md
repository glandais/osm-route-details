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

