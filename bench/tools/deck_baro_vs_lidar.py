#!/usr/bin/env python3
"""Compare, au milieu d'un ouvrage du corpus, l'altitude barométrique d'une trace qui le traverse
à la référence LiDAR (MNS et MNT) du corpus d'ouvrages.

Le décalage du baromètre est corrigé localement : médiane de (baro − MNT RGE ALTI) sur les
stations situées entre 1 et 3 km de part et d'autre du milieu de l'ouvrage, hors résidus > 5 m.
Le point de la trace retenu est le plus proche du milieu (distance ≤ 30 m, sinon pas de mesure).

Entrées : ouvrages.yaml (champs milieu et reference.valeurs.milieu, ou la première valeur), streams Strava.
Sortie CSV : ouvrage,activite,dist_milieu_m,offset_local_m,baro_corr_m,lidar_mns_m,lidar_mnt_m,
baro_moins_mns_m.

Usage : uv run --with pyyaml tools/deck_baro_vs_lidar.py ouvrages.yaml paires.csv
        (paires.csv : ouvrage,stream_json)
"""
import csv
import json
import math
import statistics
import sys

import yaml  # PyYAML

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from qualify_baro import ign  # noqa: E402


def dist_m(a, b):
    k = 111320.0
    return math.hypot((a[0] - b[0]) * k, (a[1] - b[1]) * k * math.cos(math.radians(a[0])))


def main(ouvrages_path, pairs_path):
    ouv = {o["id"]: o for o in yaml.safe_load(open(ouvrages_path))["ouvrages"]}
    w = csv.writer(sys.stdout)
    w.writerow(["ouvrage", "activite", "dist_milieu_m", "offset_local_m", "baro_corr_m",
                "lidar_mns_m", "lidar_mnt_m", "baro_moins_mns_m"])
    for row in csv.reader(open(pairs_path)):
        oid, path = row
        o = ouv[oid]
        mid = [o["milieu"]["lat"], o["milieu"]["lon"]]
        vals = o["reference"]["valeurs"]
        ref = vals.get("milieu") or next(iter(vals.values()))
        s = json.load(open(path))
        loc, alt, dist = s["location"], s["altitude"], s["distance"]
        i = min(range(len(loc)), key=lambda k: dist_m(loc[k], mid))
        d0 = dist_m(loc[i], mid)
        if d0 > 30:
            w.writerow([oid, path, round(d0), "", "", ref.get("lidar_mns"), ref.get("lidar_mnt"), ""])
            continue
        idx = [k for k in range(len(dist)) if 1000 <= abs(dist[k] - dist[i]) <= 3000][::5]
        dem = ign([loc[k] for k in idx])
        res = [alt[k] - z for k, z in zip(idx, dem) if z is not None and z > -9000]
        med = statistics.median(res)
        res = [r for r in res if abs(r - med) <= 5]
        off = statistics.median(res)
        baro = alt[i] - off
        mns = ref.get("lidar_mns")
        w.writerow([oid, path.rsplit("/", 1)[-1].split(".")[0], round(d0, 1), round(off, 2),
                    round(baro, 2), mns, ref.get("lidar_mnt"),
                    round(baro - mns, 2) if mns is not None else ""])


if __name__ == "__main__":
    main(*sys.argv[1:3])
