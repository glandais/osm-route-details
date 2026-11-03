#!/usr/bin/env python3
"""Référence R1/R2 d'un itinéraire planifié, au format des parcours d'osm-surface (`*.reference.json`).

`tracks[0].line` : l'itinéraire planifié tel quel (lon, lat, 0, abscisse en m), l'entrée du service ;
`tracks[0].surfaces` : plages de revêtement de la réponse trace_attributes (Valhalla, stadia_match.py),
ramenées de l'abscisse recalée à celle de l'itinéraire par les points appariés (<id>.baro.csv :
s_recale_m → s_origine_m, interpolation monotone) ; classe Valhalla écrite comme tribly
(PAVED, PAVED_ROUGH, COMPACTED, GRAVEL, DIRT, PATH, IMPASSABLE ; plage sans référence : UNKNOWN).

Règles de V1.2 (README §4 quater.1, décision de l'utilisateur, 2026-11-03) : l'abscisse de
l'itinéraire est calculée avec la haversine de stadia_match.py (R = 6 371 008,8 m), celle de
s_origine_m ; entre deux points appariés consécutifs où s_origine avance de plus de SAUT_M de plus
que s_recale (aller-retour sans point apparié), la plage [s_origine(i), s_origine(i + 1)] est UNKNOWN
au lieu de la plage recalée étirée. Les références V1 et V1.1 ont été écrites avant ces règles
(distance équirectangulaire, pas de plage UNKNOWN aux sauts).

Sortie standard : longueur et part non revêtue selon la référence, sans coordonnées.

Usage : reference_r12_v1.py itineraire.gpx dossier_recale id dossier_sortie
"""
import bisect
import csv
import json
import math
import re
import sys

TRIBLY = {"paved_smooth": "PAVED", "paved": "PAVED", "paved_rough": "PAVED_ROUGH", "compacted": "COMPACTED",
          "gravel": "GRAVEL", "dirt": "DIRT", "path": "PATH", "impassable": "IMPASSABLE"}
UNPAVED = {"COMPACTED", "GRAVEL", "DIRT", "PATH", "IMPASSABLE"}
SAUT_M = 100.0  # = baro_gap_max_m (criteria serie_tenue_a_l_ecart.reference_R1_R2_v12.saut_s_origine)


def hav(a, b):
    r = 6371008.8  # comme stadia_match.py
    p1, p2 = math.radians(a[0]), math.radians(b[0])
    dp, dl = p2 - p1, math.radians(b[1] - a[1])
    h = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * r * math.asin(math.sqrt(h))


def cut(surf, f, t):
    """Retire [f, t] des plages et y met une plage UNKNOWN."""
    out = []
    for x in surf:
        if x["to"] <= f or x["from"] >= t:
            out.append(x)
            continue
        if x["from"] < f:
            out.append({"from": x["from"], "to": round(f, 1), "surface": x["surface"]})
        if x["to"] > t:
            out.append({"from": round(t, 1), "to": x["to"], "surface": x["surface"]})
    out.append({"from": round(f, 1), "to": round(t, 1), "surface": "UNKNOWN"})
    out.sort(key=lambda x: x["from"])
    merged = []
    for x in out:
        if x["to"] <= x["from"]:
            continue
        if merged and merged[-1]["surface"] == x["surface"] and abs(merged[-1]["to"] - x["from"]) < 1e-6:
            merged[-1]["to"] = x["to"]
        else:
            merged.append(x)
    return merged


def main(gpx, rec, tid, out):
    pts = [(float(a), float(b)) for a, b in re.findall(r'lat="([-\d.]+)"\s+lon="([-\d.]+)"', open(gpx).read())]
    s = [0.0]
    for a, b in zip(pts, pts[1:]):
        s.append(s[-1] + hav(a, b))
    m = [(float(r["s_recale_m"]), float(r["s_origine_m"])) for r in csv.DictReader(open(f"{rec}/{tid}.baro.csv"))
         if r["insere"] == "0"]
    m.sort()
    xr = [a for a, _ in m]; xo = [b for _, b in m]

    def orig(x):
        i = bisect.bisect_left(xr, x)
        if i <= 0:
            return xo[0]
        if i >= len(xr):
            return xo[-1]
        a, b = xr[i - 1], xr[i]
        t = (x - a) / (b - a) if b > a else 0.0
        return xo[i - 1] + t * (xo[i] - xo[i - 1])

    surf = []
    for r in csv.DictReader(open(f"{rec}/{tid}.surface.csv")):
        f, t = orig(float(r["s_debut_m"])), orig(float(r["s_fin_m"]))
        if t <= f:
            continue
        c = TRIBLY.get(r["surface"], "UNKNOWN") if r["use"] != "discontinuite" else "UNKNOWN"
        if surf and surf[-1]["surface"] == c and abs(surf[-1]["to"] - f) < 1e-6:
            surf[-1]["to"] = round(t, 1)
        else:
            surf.append({"from": round(f, 1), "to": round(t, 1), "surface": c})
    sauts = [(b, d) for (a, b), (c, d) in zip(m, m[1:]) if (d - b) - (c - a) > SAUT_M]
    for f, t in sauts:
        surf = cut(surf, f, t)
    ref = {"name": tid, "slug": tid, "distance": round(s[-1], 1), "source": "itinéraire planifié V1 (BRouter) ; revêtement Valhalla FOSSGIS trace_attributes",
           "tracks": [{"line": {"type": "LineString", "coordinates": [[p[1], p[0], 0.0, round(d, 1)] for p, d in zip(pts, s)]},
                       "surfaces": surf, "surfaceState": "valhalla_fossgis"}]}
    json.dump(ref, open(f"{out}/{tid}.reference.json", "w"))
    L = sum(x["to"] - x["from"] for x in surf)
    nr = sum(x["to"] - x["from"] for x in surf if x["surface"] in UNPAVED)
    unk = sum(x["to"] - x["from"] for x in surf if x["surface"] == "UNKNOWN")
    print(json.dumps({"id": tid, "km": round(s[-1] / 1000, 1), "km_reference": round(L / 1000, 1),
                      "part_non_revetu": round(nr / L, 3) if L else None, "part_inconnu": round(unk / L, 3) if L else None,
                      "sauts_s_origine": len(sauts), "sauts_m": round(sum(t - f for f, t in sauts), 1)}))


if __name__ == "__main__":
    main(*sys.argv[1:])
