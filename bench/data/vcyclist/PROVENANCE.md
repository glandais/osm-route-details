# Provenance

Copie sans modification de `docs/deck-strava-data/` (sous-ensemble) du dépôt vcyclist, version
6.11.0-1, commit `4dfd6c5`, faite le 2026-10-30 pour le banc d'osm-route-details.

- Licence du dépôt : Apache-2.0 (`LICENSE`, copié ici).
- `test1..4.gpx` : itinéraires planifiés dans Strava par l'auteur de vcyclist ; géométrie
  © contributeurs OpenStreetMap (ODbL).
- `controls/`, `analysis/truth/controls/`, `analysis/truth/tunnels/` : témoins tirés d'OSM (ODbL).
- `analysis/truth/test*.csv`, `*_mns.csv`, `structures.csv` : vérité dérivée de l'IGN LiDAR HD
  (Licence Ouverte Etalab 2.0, « © IGN — LiDAR HD »).
- Les empreintes sha256 de chaque fichier sont dans `bench/manifest.yaml`.
