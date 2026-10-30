# Conception technique — index et plan de réalisation

> Cette conception technique applique la conception générale ([doc 01](../01-conception-generale.md))
> et le contrat d'interface ([doc 02](../02-contrat-interface.md)). Elle ne les modifie pas. Quand une
> exigence pose problème, elle est gardée et signalée dans la section « Points de tension » du
> fichier concerné. Aucun code n'existe encore.

## 1. Sections

| # | Fichier | Composants (doc 01 §4) | À y chercher |
|---|---|---|---|
| T01 | [01-architecture.md](01-architecture.md) | API HTTP, Orchestrateur, Observabilité | **Carte des types partagés (§0)** et hiérarchie d'erreurs ; workspace et crates ; runtime, admission, budget de 90 s ; validation et RFC 9457 ; pipeline des étapes 0 à 10 ; décision 200/503 (règle stricte) ; observabilité ; versions et déterminisme ; packaging et déploiement en une seule instance |
| T02 | [02-acces-donnees.md](02-acces-donnees.md) | Accès données, Caches, Adaptateur élévation (couche basse), Adaptateur typologie (décodage), Référentiel de couverture | Port `TileSource` / `TileStore`, quatre issues de lecture, statuts HTTP par source, cadence par hôte, relances, cache disque autoréparé, TTL ; BIL et Terrarium, bilinéaire au centre du pixel, repli point par point ; contour IGN, zone France, GLO-30, couture, #340 ; attributions |
| T03 | [03-typologie.md](03-typologie.md) | Adaptateur typologie (schéma), Chaîne typologie, Classification | Modèle neutre, Shortbread, graphe par niveaux, HMM, ouvrages et extrémités (`Step::Need`), passages et doutes, table de règles, `reconcile`, segments homogènes |
| T04 | [04-elevation.md](04-elevation.md) | Chaîne élévation, Fusion du profil, Détecteurs heuristiques, Sélection des points de rupture | Stations à 5 m, MNT et MNS, tablier MNS, interpolation, tunnels, heuristiques V1, comblement, lissage, D+ à hystérésis et adaptation GLO-30, points de rupture |
| T05 | [05-qualite-tests.md](05-qualite-tests.md) | (hors service) Qualité, bancs, CI | Niveaux de tests, golden files, corpus et vérités, discipline des séries tenues à l'écart, traces barométriques, **seuils A1–A13**, empreintes de règles et de données, `ord-bench` et cache de banc local, CI |
| T06 | [06-assembleur.md](06-assembleur.md) | Assembleur | Calcul de chaque champ de `segments[]`, `summary` et `quality` (dont `summary.structures[]` : `kept_ground`, ouvrage mixte en entrées contiguës) ; `status` ; producteur de chaque avertissement |
| T07 | [07-configuration.md](07-configuration.md) | API HTTP (configuration) | Sources et priorité de la configuration, table des clés et défauts, bornes des options, une seule instance, `/v1/capabilities` |

## 2. Ordre de lecture

1. **T01 §0** (carte des types) et **T01 §5** (pipeline, décision), puis **T06** (assembleur) et
   **T07** (configuration) : le squelette, et le nom de chaque type et de son propriétaire.
2. **T02** : ce que le cœur reçoit (tuiles décodées, `TerrainSample`, couverture) et les règles de
   panne qui alimentent la décision.
3. **T03**, puis **T04** : les deux chaînes, dans l'ordre du pipeline. T04 consomme les
   `Structure`/`StructurePassage` de T03, et T03 reçoit en retour les détections par `reconcile`.
4. **T05** : comment chaque exigence est vérifiée, et les seuils d'acceptation.
5. Pour finir, les « Points de tension » de chaque fichier, qui sont les questions encore ouvertes.

Conventions : **[M]**/[mesuré]/[vc]/[os] désigne une valeur reprise d'un projet qui l'a mesurée,
**[C]**/[à calibrer]/[cal] une valeur de départ fixée sur la série de calibration. Les références
`VC/`, `vc:` (vcyclist 6.11) et `OS/`, `os:` (osm-surface `44e5da6`) sont des `chemin:ligne`.

## 3. Plan de réalisation du MVP (incréments verticaux)

Chaque incrément traverse toute la pile, du HTTP jusqu'au JSON, et finit par un service qui répond.
Son critère d'acceptation se mesure par `ord-bench run --offline` sur le cache de banc local (T05 §5.3),
ou par un test nommé. Les seuils A* sont ceux de T05 §3.5. Ils sont écrits dans
`bench/criteria.md` **avant** l'incrément qui les vise.

| # | Périmètre | Modules touchés | Critère d'acceptation |
|---|---|---|---|
| 0 | **Squelette contractuel** : workspace, types de T01 §0, `StrictJson` et validation, `Problem` RFC 9457, `/health/*`, `/v1/capabilities` et `/v1/attribution` précalculés, `contract/openapi.json`, CI (`fmt`, `clippy`, `test`, `deny`, `contract`). `POST` renvoie 500 `internal`. | `ord-domain`, `ord-server`, `ord-testkit` (squelette) | Tests T05 §2.5, points 1 à 3 et 5 ; option inconnue → 400 qui liste **toutes** les clés ; coordonnée hors Web Mercator → 400 avec pointeur et sans valeur ; 201 km → 413 `details.limit = "length_m"` |
| 1 | **Profil Mapterhorn seul**, mode `input` et `distance` : `prepare`, `XyzHttpSource`, cache disque et LRU, `TerrariumCodec`, `TerrainReader` limité à Mapterhorn (origine `mapterhorn`), `sample_raw`, `derive` (D+ des presets, pentes), assembleur partiel (`summary.length_m`, `gain_*`, `quality.terrain`), `meta`, `attribution` | `ord-core::{prepare, elevation::derive, assemble}`, `ord-adapters::terrarium`, `ord-data::{port, http, disk, memory, elevation}`, `ord-pipeline` (séquentiel), `tools/ord-bench` (`fill-cache`, `run`) | Correction du demi-pixel vérifiée sur `test4` : RMS au sol `T` ≤ 1,0 m (vcyclist 0,58 m après correction, 2,63 m avant). Points de contrôle live à ± 50 m. Tests de cache de T05 §2.1 (page d'erreur en 200 jamais cachée, entrée corrompue purgée). Même requête deux fois → réponses égales |
| 2 | **IGN LiDAR HD + repli + couverture** : `WmsGetMapSource`, `BilCodec`, `HostBudget` (20 req/s, relances, cache négatif), repli point par point, contour IGN, zone France, `coverage-index.pmtiles` et polygones (GLO-30, couture, contrôle #340), `TerrainRange`, adaptation du seuil GLO-30, `outside_primary_zone`, `terrain_fallback`, `dsm_source` | `ord-adapters::bil`, `ord-data::{http (WMS), coverage}`, `ord-core::{coverage, elevation::derive::gain}`, assembleur (`terrain_m`) | **A8** (RMS `T` ≤ 1,0 m sur `test1..3`, ≤ 1,5 m sur `test4`). Scénarios `ScriptedServer` : IGN en 503 → 200 `terrain_fallback` ; hors contour → 0 requête IGN. Golden hors France et GLO-30 (`adapted[]` non vide). Live : **A13** ≤ 25 s sur 130 km à froid |
| 3 | **Typologie et segments** : `StreetsCodec<Shortbread>`, couloir, graphe par niveaux, HMM, `runs`, `classify`, `build_segments` (verdicts vides), `segments[]`, `by_*_m`, `matched_ratio`, `inferred_surface_ratio`, `unmatched_range`, `typology_missing`. Typologie et MNT en parallèle (`tokio::join!`) | `ord-adapters::streets`, `ord-core::{typology::{corridor, graph, matcher, runs, segments}, classification}`, `ord-pipeline::typology` | **A10** (accord revêtu ≥ 97,5 % par parcours `os-stadia`), **A11** (`matched_ratio` ≥ 98 %). Non-régression des 3 parcours d'osm-surface malgré les trois écarts de graphe (T03 §12.3). Typologie à chaud < 160 ms sur 180 km |
| 4 | **Ouvrages OSM** : `StructureGraph`, extension `Step::Need` (4 tours, 32 tuiles, 20 km), passages et doutes, `sample_structures` (MNS et MNT des extensions), `fuse` (tablier MNS, interpolation, tunnels, comblement), `reconcile` avec `detected = []`, rapport `structure_correction`, `summary.structures[]`, avertissements d'ouvrage | `ord-core::{typology::structures, elevation::fusion, classification::reconcile}`, `ord-pipeline::elevation::sample_structures` | **A1** (Loire, max ≤ 2 m), **A2** (Saint-Nazaire ≤ 5 m), **A3** (21/21), **A5** (Vercors, 4/4 et ≤ 2 m), **A7** (0 station déplacée sur les négatifs, dont les passages sous un pont `n02`, `n15`), **A9** (D+ / vérité ∈ [0,90 ; 1,15]). Golden : trace qui commence sur le tablier, pont parcouru deux fois, demi-tour sur un pont |
| 5 | **Sortie par défaut** : `select_breakpoints` (classes A/B/C, fusion, rattachement, plafond), mode `input_with_breakpoints`, lissage optionnel `ele_smoothed`, `sections` | `ord-core::elevation::{breakpoints, derive::smooth}`, `ord-server` (DTO, filtrage) | Sur `vc-routes` : 100 % des bornes d'ouvrage et de trou portées par un point ; les deux extrémités de chaque jambe comptée dans `gain_m` présentes ; ≤ 20 points insérés par km ; somme des dénivelés entre points = `gain_m` à 2 % près. Propriétés T05 §2.2 (`d_m` croissant, points d'entrée jamais déplacés) |
| 6 | **Durcissement MVP** : `decide_outcome` complet et `accept_partial`, `Retry-After`, admission et 503 `overloaded`, budget et 504, annulation, observabilité sans coordonnées, Docker, `bench-offline` en local et job `bench-report` en CI (T05 §6), jugement unique des séries tenues à l'écart | `ord-pipeline::{admission, decision}`, `ord-server` (observabilité), CI | Tous les scénarios de pannes de T05 §2.4. Test du parcours sentinelle : aucune coordonnée dans les logs. **A12** (p95 à chaud ≤ 500 ms sur 180 km). Jugement unique publié de **A4** (≥ 9/11) et **A6** (≥ 6/7) dans `bench/judgements.md`, qu'il passe ou non |

**Après le MVP** :
- **V1** : heuristiques (`ElevationRules::heuristics = true`, `DsmScope::LidarHull`, détecteurs,
  `reconcile` non vide, `structure_untagged`). Elles sont acceptées sur A4, A6 et A7 rejugés avec de
  nouvelles séries tenues à l'écart, et sur le coût à froid du MNS partout, mesuré sous le budget
  de 90 s (T02 §10).
- **`.versatiles` local** au premier `429`, ou au passage en production.
- **V1.x** : mesure `dsm-placement` (T05 §7) avant toute lecture du MNS sur la géométrie OSM.

## 4. Tensions transverses (renvois)

Les tensions transverses de la révision précédente (vocabulaire du contrat, budget à froid, panne
d'une seule tuile, `mapterhorn_measured` sur GLO-30, zone France, déploiement, cache de banc) sont
tranchées : doc 01 §10. Restent ouvertes, outre les points propres à chaque fichier :

| Sujet | Où |
|---|---|
| Vérité circulaire des tabliers (vérité `B` = MNS IGN) | T05 §10.1 |
| Taille des fichiers : T01 ramené à ≈ 560 lignes après extraction de T06 et T07, encore au-delà de la cible de 500 | — |
