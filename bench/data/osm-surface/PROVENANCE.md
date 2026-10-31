# Provenance

Copie faite le 2026-10-30 depuis le dépôt osm-surface (dépôt local de l'utilisateur), commit
`44e5da6`, pour le banc d'osm-route-details.

- Licence : Apache-2.0, choisie par l'utilisateur le 2026-10-30. `LICENSE` vient du commit `53b441b`
  d'osm-surface (« Licence Apache 2.0 », qui déclare aussi `license = "Apache-2.0"` dans `Cargo.toml`) ;
  contenu identique à la copie initiale (sha256 dans `bench/manifest.yaml`). Les autres fichiers restent
  ceux du commit `44e5da6`.
- `*.gpx` : `testdata/*.gpx`, copiés sans modification. Parcours de groupe ; géométrie
  © contributeurs OpenStreetMap (ODbL).
- `*.reference.json` : `testdata/*.route.json` expurgés par `bench/tools/expurge_route_json.py`
  (identifiants, équipe, médias et URL tribly, waypoints, tags, dates retirés). Restent la géométrie et
  `tracks[0].surfaces`, la référence de revêtement Stadia/Valhalla calculée par tribly.
- `eval-report.md` : `eval/report.md`, rapport d'évaluation d'osm-surface (chiffres R1/R2/M1 de
  référence).
- Les empreintes sha256 de chaque fichier sont dans `SHA256SUMS`.
