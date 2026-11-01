#!/usr/bin/env python3
"""Compose les itinéraires planifiés de la série tenue à l'écart V1 (README §4 ter.4).

Un itinéraire planifié ne contient aucune donnée personnelle : des points de passage choisis sur la
carte, routés sur OSM par BRouter (serveur public brouter.de, profil vélo). Pour chaque itinéraire
de specs.json ({id, role, nature (témoin S2), profil, points: [[lat, lon], ...]}) : GPX écrit dans le dossier de sortie,
puis longueur et part non revêtue lues dans les tags OSM que BRouter renvoie (WayTags).
Non revêtu : surface de la liste UNPAVED, ou à défaut highway=track (tracktype ≠ grade1) ou path.

Sortie : GPX <id>.gpx et résumé JSON (longueurs, parts) dans le dossier ; résumé sans coordonnées
sur la sortie standard. Le dossier reste hors git tant que la série n'est pas jugée.

Usage : planifier_v1.py specs.json dossier
"""
import json
import os
import sys
import time
import urllib.parse
import urllib.request

API = "https://brouter.de/brouter"
UA = "osm-route-details-bench/0.1 (usage personnel)"
UNPAVED = {"unpaved", "gravel", "fine_gravel", "compacted", "dirt", "earth", "ground", "grass", "sand", "mud",
           "pebblestone", "woodchips", "rock", "grass_paver"}
PAVED = {"asphalt", "paved", "concrete", "concrete:plates", "concrete:lanes", "paving_stones", "sett", "chipseal",
         "cobblestone", "metal", "wood"}


def route(points, profil):
    q = urllib.parse.urlencode({"lonlats": "|".join(f"{lo:.6f},{la:.6f}" for la, lo in points), "profile": profil,
                                "alternativeidx": 0, "format": "geojson"})
    for attempt in range(4):
        try:
            with urllib.request.urlopen(urllib.request.Request(f"{API}?{q}", headers={"User-Agent": UA}), timeout=120) as r:
                return json.load(r)["features"][0]
        except Exception:  # noqa: BLE001
            time.sleep(10 * (attempt + 1))
    raise RuntimeError("BRouter indisponible")


def non_revetu(tags):
    t = dict(x.split("=", 1) for x in tags.split() if "=" in x)
    s = t.get("surface")
    if s in UNPAVED:
        return True
    if s in PAVED:
        return False
    hw = t.get("highway")
    if hw == "track":
        return t.get("tracktype") != "grade1"
    return hw == "path"


def main(specs_path, out):
    os.makedirs(out, exist_ok=True)
    res = {}
    for s in json.load(open(specs_path)):
        f = route(s["points"], s.get("profil", "trekking"))
        coords = f["geometry"]["coordinates"]
        msgs = f["properties"]["messages"]
        hdr, rows = msgs[0], msgs[1:]
        di, ti = hdr.index("Distance"), hdr.index("WayTags")
        L = sum(float(r[di]) for r in rows)
        nr = sum(float(r[di]) for r in rows if non_revetu(r[ti]))
        with open(f"{out}/{s['id']}.gpx", "w") as g:
            g.write('<?xml version="1.0" encoding="UTF-8"?>\n<gpx version="1.1" creator="osm-route-details bench (BRouter)" '
                    'xmlns="http://www.topografix.com/GPX/1/1">\n<trk><name>' + s["id"] + "</name><trkseg>\n")
            for c in coords:
                g.write(f'<trkpt lat="{c[1]:.6f}" lon="{c[0]:.6f}"></trkpt>\n')
            g.write("</trkseg></trk></gpx>\n")
        res[s["id"]] = {"role": s["role"], "nature": s.get("nature"), "profil": s.get("profil", "trekking"), "km": round(L / 1000, 2),
                        "part_non_revetu": round(nr / L, 3) if L else None}
        time.sleep(2)
    json.dump(res, open(f"{out}/resume.json", "w"), indent=1, ensure_ascii=False)
    print(json.dumps(res, indent=1, ensure_ascii=False))


if __name__ == "__main__":
    main(*sys.argv[1:])
