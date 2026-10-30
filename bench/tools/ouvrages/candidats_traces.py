#!/usr/bin/env python3
"""Cherche des ouvrages OSM franchis par des traces perso fiables, jamais vus par vcyclist.

Pour chaque trace (GPX rogné) : une requête Overpass sur sa boîte englobante (la trace elle-même
n'est pas envoyée), ways `highway` avec `bridge` ou `tunnel`, longueur > MIN_LEN_M. Un ouvrage est
retenu si la trace passe à moins de TOL_M de ses deux extrémités et de son milieu, et s'il n'est ni
déjà au corpus, ni franchi par test1..4 de vcyclist, ni dans les témoins de vcyclist. Les ways
contiguës d'un même ouvrage (même nom ou extrémités communes) sont fusionnées.
Puis lecture IGN au milieu (MNT RGE ALTI, MNT/MNS/MNH LiDAR HD) ; un pont n'est gardé que si le
MNH au milieu dépasse MIN_MNH_M (vrai tablier en hauteur).

Sortie : JSON des candidats sur la sortie standard ; réponses Overpass brutes dans le dossier donné.

Usage : candidats_traces.py traces.txt ouvrages-geom.json dossier_vcyclist dossier_brut
        (traces.txt : lignes « id chemin.gpx »)
"""
import csv
import glob
import json
import math
import os
import re
import sys
import time
import urllib.parse
import urllib.request

OVERPASS = "https://overpass-api.de/api/interpreter"
ALTI = "https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json"
UA = "osm-route-details-bench/0.1 (usage personnel)"
MIN_LEN_M = 50
TOL_M = 20.0
MIN_MNH_M = 3.0


def dist_m(a, b):
    k = 111320.0
    return math.hypot((a[0] - b[0]) * k, (a[1] - b[1]) * k * math.cos(math.radians(a[0])))


def load_gpx(path):
    txt = open(path).read()
    return [(float(a), float(b)) for a, b in re.findall(r'lat="([-\d.]+)"\s+lon="([-\d.]+)"', txt)]


def near(track, p, tol):
    d = tol / 111320.0 * 2
    return any(abs(q[0] - p[0]) < d and abs(q[1] - p[1]) < d * 2 and dist_m(q, p) < tol for q in track)


def length(g):
    return sum(dist_m(a, b) for a, b in zip(g, g[1:]))


def mid(g):
    L, acc = length(g) / 2, 0.0
    for a, b in zip(g, g[1:]):
        s = dist_m(a, b)
        if acc + s >= L:
            t = (L - acc) / s if s else 0
            return (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]))
        acc += s
    return g[-1]


def overpass(bbox, raw_path):
    if os.path.exists(raw_path):
        return json.load(open(raw_path))
    s, w, n, e = bbox
    q = (f'[out:json][timeout:120];(way["highway"]["bridge"~"^(yes|viaduct)$"](if: length() > {MIN_LEN_M})'
         f'({s},{w},{n},{e});way["highway"]["tunnel"~"^(yes|building_passage|avalanche_protector)$"]'
         f'(if: length() > {MIN_LEN_M})({s},{w},{n},{e}););out tags geom;')
    for attempt in range(5):
        req = urllib.request.Request(OVERPASS, data=urllib.parse.urlencode({"data": q}).encode(),
                                     headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=180) as r:
                d = json.load(r)
            json.dump(d, open(raw_path, "w"))
            time.sleep(5)
            return d
        except Exception:  # noqa: BLE001 — serveur saturé : attente croissante
            time.sleep(20 * (attempt + 1))
    print(f"Overpass en échec pour {raw_path} : trace ignorée", file=sys.stderr)
    return None


def ign(points):
    q = urllib.parse.urlencode({"lon": "|".join(f"{p[1]:.6f}" for p in points),
                                "lat": "|".join(f"{p[0]:.6f}" for p in points),
                                "resource": "ign_lidar_hd_mnx_multi_wld", "zonly": "false",
                                "measures": "true"})
    req = urllib.request.Request(f"{ALTI}?{q}", headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as r:
        out = []
        for e in json.load(r)["elevations"]:
            m = {}
            for x in e.get("measures", []):  # aussi des mesures hors métropole à −99999
                if x.get("title", "").count(" - ") >= 2 and x["z"] > -9000:
                    m.setdefault(x["title"].split(" - ")[2].strip(), x["z"])
            out.append({"lidar_mnt": m.get("MNT"), "lidar_mns": m.get("MNS"), "lidar_mnh": m.get("MNH")})
        return out


def main(traces_path, geom_path, vc_dir, raw_dir):
    os.makedirs(raw_dir, exist_ok=True)
    corpus_ways = set()
    for o in json.load(open(geom_path)):
        corpus_ways |= set(o["chain"]) | set(o.get("extra", []))
    vc_ways = set()
    for f in glob.glob(f"{vc_dir}/analysis/truth/*/spans.csv"):
        for r in csv.DictReader(open(f)):
            for w in re.findall(r"\d{5,}", r.get("way", "")):
                vc_ways.add(int(w))
    vc_tracks = [load_gpx(f) for f in sorted(glob.glob(f"{vc_dir}/test*.gpx"))]
    found = {}
    for line in open(traces_path):
        if not line.strip():
            continue
        tid, path = line.split()
        tr = load_gpx(path)
        lats, lons = [p[0] for p in tr], [p[1] for p in tr]
        bbox = (round(min(lats), 3) - 0.002, round(min(lons), 3) - 0.002,
                round(max(lats), 3) + 0.002, round(max(lons), 3) + 0.002)
        d = overpass(bbox, os.path.join(raw_dir, f"{tid}.json"))
        if d is None:
            continue
        for e in d.get("elements", []):
            if e["type"] != "way" or e["id"] in corpus_ways or e["id"] in vc_ways or e["id"] in found:
                continue
            g = [(p["lat"], p["lon"]) for p in e["geometry"]]
            pts = [g[0], mid(g), g[-1]]
            if not all(near(tr, p, TOL_M) for p in pts):
                continue
            if any(all(near(v, p, TOL_M) for p in pts) for v in vc_tracks):
                continue
            t = e.get("tags", {})
            found[e["id"]] = dict(way=e["id"], trace=tid, geom=g, tags=t, longueur_m=round(length(g)),
                                  type="tunnel" if "tunnel" in t else "bridge")
    # fusion des ways contiguës (extrémités communes) en un ouvrage
    ways = list(found.values())
    groups, used = [], set()
    for w in ways:
        if w["way"] in used:
            continue
        grp, frontier = [w], [w]
        used.add(w["way"])
        while frontier:
            cur = frontier.pop()
            for o in ways:
                if o["way"] in used or o["type"] != cur["type"]:
                    continue
                ends = (cur["geom"][0], cur["geom"][-1])
                if any(dist_m(a, b) < 2 for a in ends for b in (o["geom"][0], o["geom"][-1])):
                    used.add(o["way"])
                    grp.append(o)
                    frontier.append(o)
        groups.append(grp)
    out = []
    for grp in groups:
        g0 = max(grp, key=lambda w: w["longueur_m"])
        allpts = [p for w in grp for p in w["geom"]]
        A = min(allpts, key=lambda p: (p[0], p[1]))
        B = max(allpts, key=lambda p: dist_m(p, A))
        m = mid(g0["geom"])
        vals = ign([A, m, B])
        time.sleep(0.5)
        if g0["type"] == "bridge" and (vals[1]["lidar_mnh"] is None or vals[1]["lidar_mnh"] < MIN_MNH_M):
            continue
        out.append(dict(type=g0["type"], chain=sorted(w["way"] for w in grp), trace=g0["trace"],
                        nom=g0["tags"].get("name", ""), longueur_m=sum(w["longueur_m"] for w in grp),
                        tags={w["way"]: w["tags"] for w in grp},
                        pts={"A": list(A), "milieu": list(m), "B": list(B)},
                        valeurs={"A": vals[0], "milieu": vals[1], "B": vals[2]}))
    json.dump(out, sys.stdout, ensure_ascii=False, indent=1)


if __name__ == "__main__":
    main(*sys.argv[1:5])
