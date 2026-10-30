#!/usr/bin/env python3
"""Médiane du MNS LiDAR HD sur ~25 m au milieu du tablier de chaque pont du corpus.

Pour chaque ouvrage de type pont (hors passages dessous) : segment OSM de la chaîne le plus proche
du milieu, direction de ce segment, 11 points de −12,5 à +12,5 m le long de l'axe (pas 2,5 m),
lecture MNS de la ressource ign_lidar_hd_mnx_multi_wld (zonly=false&measures=true), médiane des
valeurs valides. Sert de référence de COHÉRENCE (même famille de données que le service) ; la seule
référence indépendante d'un tablier est le baromètre.

Entrées : ouvrages-geom.json, dossier des réponses Overpass (q*.json).
Sortie : JSON {id: {mns_mediane_25m, n, pas_m, mns_min, mns_max}} sur la sortie standard.

Usage : mns_median25.py ouvrages-geom.json ouvrages-osm/
"""
import glob
import json
import math
import statistics
import sys
import time
import urllib.parse
import urllib.request

API = "https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json"
UA = "osm-route-details-bench/0.1 (usage personnel)"
OFFSETS = [-12.5 + 2.5 * i for i in range(11)]


def mns(points):
    q = urllib.parse.urlencode({
        "lon": "|".join(f"{p[1]:.7f}" for p in points),
        "lat": "|".join(f"{p[0]:.7f}" for p in points),
        "resource": "ign_lidar_hd_mnx_multi_wld", "zonly": "false", "measures": "true",
    })
    req = urllib.request.Request(f"{API}?{q}", headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as r:
        out = []
        for e in json.load(r)["elevations"]:
            v = [m["z"] for m in e.get("measures", []) if " MNS " in m.get("title", "")]
            out.append(v[0] if v and v[0] > -9000 else None)
        return out


def main(geom_path, osm_dir):
    ways = {}
    for f in sorted(glob.glob(f"{osm_dir}/*.json")):
        for e in json.load(open(f)).get("elements", []):
            if e.get("type") == "way" and "geometry" in e:
                ways[e["id"]] = [(p["lat"], p["lon"]) for p in e["geometry"]]
    res = {}
    for o in json.load(open(geom_path)):
        if "bridge" not in o["type"] or "sous" in o["type"]:
            continue
        mid = o["mid"]
        c = math.cos(math.radians(mid[0]))
        best = None
        for w in o["chain"]:
            g = ways.get(w, [])
            for a, b in zip(g, g[1:]):
                ax, ay = (a[1] - mid[1]) * c, a[0] - mid[0]
                bx, by = (b[1] - mid[1]) * c, b[0] - mid[0]
                dx, dy = bx - ax, by - ay
                L = dx * dx + dy * dy
                t = 0 if L == 0 else max(0.0, min(1.0, -(ax * dx + ay * dy) / L))
                d = math.hypot(ax + t * dx, ay + t * dy)
                if L and (best is None or d < best[0]):
                    best = (d, dx / math.sqrt(L), dy / math.sqrt(L))
        if best is None:
            continue
        _, ux, uy = best
        k = 111320.0
        pts = [(mid[0] + uy * s / k, mid[1] + ux * s / (k * c)) for s in OFFSETS]
        vals = [v for v in mns(pts) if v is not None]
        time.sleep(0.5)
        if vals:
            res[o["id"]] = dict(mns_mediane_25m=round(statistics.median(vals), 2), n=len(vals),
                                pas_m=2.5, mns_min=min(vals), mns_max=max(vals))
        else:
            res[o["id"]] = dict(mns_mediane_25m=None, n=0, pas_m=2.5)
    json.dump(res, sys.stdout, ensure_ascii=False, indent=1)


if __name__ == "__main__":
    main(*sys.argv[1:3])
