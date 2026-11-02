#!/usr/bin/env python3
"""Réponses Overpass du contrôle de couverture d'une série (README §4 ter.4), par emprise de trace.

Pour chaque sortie de la sélection (streams private/strava/streams-<serie>/<id>.json) et chaque
itinéraire planifié (private/<serie>/planifies/<id>.gpx), emprise = boîte de la trace élargie de
0,002° en latitude et 0,003° en longitude :
- <id>.json : ways highway ou railway `bridge`, highway `tunnel`, `embankment=yes`, `cutting=yes`,
  `man_made=dyke` (ouvrages, croisements, natures S2) ;
- <id>.cliff.json : `natural=cliff`, `man_made=embankment` ;
- <id>.foret.json (itinéraires S2 seulement) : forêts et bois, ways et relations.
Mêmes requêtes que la série V1 (outils de session, scratchpad). Base OSM courante (non datée), comme
V1. En séquence, PAUSE_S secondes entre deux requêtes, User-Agent du banc ; une réponse déjà en cache
n'est pas redemandée. Cache, hors git : private/<serie>/overpass/.
Sortie standard : nombre de requêtes faites, en cache, en échec.

Usage : overpass_couverture.py private/ serie
"""
import glob
import json
import os
import re
import sys
import time
import urllib.parse
import urllib.request

UA = "osm-route-details-bench/0.1 (usage personnel)"
URL = "https://overpass-api.de/api/interpreter"
PAUSE_S = 2.0


def full(b):
    return (f"[out:json][timeout:180];(way[highway][bridge]({b});way[railway][bridge]({b});way[highway][tunnel]({b});"
            f"way[embankment=yes]({b});way[cutting=yes]({b});way[man_made=dyke]({b}););out tags geom;")


def cliff(b):
    return f"[out:json][timeout:180];(way[natural=cliff]({b});way[man_made=embankment]({b}););out tags geom;"


def foret(b):
    return (f"[out:json][timeout:180];(way[landuse=forest]({b});way[natural=wood]({b});relation[landuse=forest]({b});"
            f"relation[natural=wood]({b}););out geom;")


def bbox(pts):
    la = [p[0] for p in pts]
    lo = [p[1] for p in pts]
    return ",".join(f"{x:.5f}" for x in (min(la) - .002, min(lo) - .003, max(la) + .002, max(lo) + .003))


def main(B, serie):
    B = B.rstrip("/")
    out = f"{B}/{serie}/overpass"
    os.makedirs(out, exist_ok=True)
    jobs = []
    for f in sorted(glob.glob(f"{B}/strava/streams-{serie}/*.json")):
        i = os.path.basename(f)[:-5]
        b = bbox([p for p in json.load(open(f))["location"] if p])
        jobs += [(f"{i}.json", full(b)), (f"{i}.cliff.json", cliff(b))]
    res = f"{B}/{serie}/planifies/resume.json"
    for i, r in (json.load(open(res)).items() if os.path.exists(res) else []):
        txt = open(f"{B}/{serie}/planifies/{i}.gpx").read()
        b = bbox([(float(a), float(c)) for a, c in re.findall(r'lat="([-\d.]+)" lon="([-\d.]+)"', txt)])
        jobs += [(f"{i}.json", full(b)), (f"{i}.cliff.json", cliff(b))] + ([(f"{i}.foret.json", foret(b))] if r["role"] == "S2" else [])
    n = {"faites": 0, "en_cache": 0, "echecs": 0}
    for name, q in jobs:
        f = f"{out}/{name}"
        if os.path.exists(f):
            n["en_cache"] += 1
            continue
        for attempt in range(5):
            try:
                req = urllib.request.Request(URL, data=urllib.parse.urlencode({"data": q}).encode(), headers={"User-Agent": UA})
                with urllib.request.urlopen(req, timeout=240) as r:
                    x = r.read()
                json.loads(x)
                open(f, "wb").write(x)
                n["faites"] += 1
                break
            except Exception as e:  # noqa: BLE001
                print(f"nouvel essai ({type(e).__name__})", file=sys.stderr, flush=True)
                time.sleep(30 * (attempt + 1))
        else:
            n["echecs"] += 1
        time.sleep(PAUSE_S)
    print(json.dumps(n))


if __name__ == "__main__":
    main(*sys.argv[1:])
