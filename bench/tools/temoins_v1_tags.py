#!/usr/bin/env python3
"""Ouvrages réels sur la voie des témoins S2 de la série V1 (README §4 ter.6), sans le service.

Les témoins V1 (private/v1/temoins-v1.json) n'ont pas de fichier de stations à ways comme ceux de
vcyclist : leurs voies sont lues par géométrie. Pour chaque témoin, une requête Overpass datée de la
base des témoins V1 (BASE) sur son emprise : ways `covered=yes`, `tunnel=avalanche_protector`, et
`highway` portant `bridge` ou `tunnel`. Une station du témoin (pas de STEP_M) est sur une telle voie
si la voie passe à TOL_M au plus et fait moins de ANGLE_DEG avec le témoin ; les stations
consécutives forment une plage.

Sortie (hors git) : private/v1/temoins-v1-tags.json, {id: {base_osm, plages: [{tag, debut: [lat, lon],
fin: [lat, lon], longueur_m}]}} ; tag ∈ covered, avalanche_protector, bridge, tunnel. Réponses brutes
dans private/v1/overpass/temoin-<id>.json. Sortie standard : comptes par tag, sans coordonnées.

Usage : temoins_v1_tags.py private/
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

BASE = "2026-11-01T13:00:00Z"
UA = "osm-route-details-bench/0.1 (usage personnel)"
STEP_M = 5.0
TOL_M = 10.0
ANGLE_DEG = 20.0


def hav(a, b):
    k = 111320.0
    return math.hypot((a[0] - b[0]) * k, (a[1] - b[1]) * k * math.cos(math.radians(a[0])))


def gpx(path):
    return [(float(a), float(b)) for a, b in re.findall(r'lat="([-\d.]+)"\s+lon="([-\d.]+)"', open(path).read())]


def geometry(B, t, pseudo_to_strava):
    if t["planifie"]:
        P = gpx(f"{B}/v1/planifies/{t['trace']}.gpx")
        if "debut" not in t:   # témoin planifié : l'itinéraire entier
            return P
        P = densify(P)         # tronçon d'un itinéraire (E4) : entre ses extrémités
    else:
        s = json.load(open(f"{B}/strava/streams-v1/{pseudo_to_strava[t['trace']]}.json"))
        P = [tuple(p) for p in s["location"] if p]
    i0 = min(range(len(P)), key=lambda k: hav(P[k], t["debut"]))
    i1 = min(range(len(P)), key=lambda k: hav(P[k], t["fin"]))
    return P[min(i0, i1):max(i0, i1) + 1]


def densify(P):
    out = [P[0]]
    for a, b in zip(P, P[1:]):
        n = max(1, int(hav(a, b) // STEP_M))
        out += [(a[0] + (b[0] - a[0]) * k / n, a[1] + (b[1] - a[1]) * k / n) for k in range(1, n + 1)]
    return out


def overpass(bbox):
    b = ",".join(f"{x:.5f}" for x in bbox)
    q = (f'[out:json][timeout:180][date:"{BASE}"];(way[covered=yes]({b});way[tunnel=avalanche_protector]({b});'
         f'way[highway][bridge]({b});way[highway][tunnel]({b}););out tags geom;')
    for attempt in range(5):
        try:
            r = urllib.request.urlopen(urllib.request.Request("https://overpass-api.de/api/interpreter",
                                       data=urllib.parse.urlencode({"data": q}).encode(), headers={"User-Agent": UA}), timeout=240)
            return json.load(r)
        except Exception:  # noqa: BLE001
            time.sleep(30)
    raise RuntimeError("Overpass indisponible")


def tag_of(t):
    if t.get("covered") == "yes":
        return "covered"
    if t.get("tunnel") == "avalanche_protector":
        return "avalanche_protector"
    if t.get("tunnel", "no") != "no":
        return "tunnel"
    if t.get("bridge", "no") != "no":
        return "bridge"
    return None


def main(B):
    B = B.rstrip("/")
    T = json.load(open(f"{B}/v1/temoins-v1.json"))
    p2s = {r["pseudo"]: r["strava_id"] for r in csv.DictReader(open(f"{B}/v1/perso-map-v1.csv"))}
    out, counts = {}, {}
    for t in T:
        P = densify(geometry(B, t, p2s))
        la = [p[0] for p in P]; lo = [p[1] for p in P]
        raw = f"{B}/v1/overpass/temoin-{t['id']}.json"
        if os.path.exists(raw):
            d = json.load(open(raw))
        else:
            d = overpass((min(la) - .001, min(lo) - .0015, max(la) + .001, max(lo) + .0015))
            json.dump(d, open(raw, "w")); time.sleep(2)
        ways = [(tag_of(e.get("tags", {})), [(g["lat"], g["lon"]) for g in e["geometry"]])
                for e in d.get("elements", []) if e.get("type") == "way" and "geometry" in e and tag_of(e.get("tags", {}))]
        lab = []
        for k, p in enumerate(P):
            q = P[min(k + 1, len(P) - 1)] if k + 1 < len(P) else P[k - 1]
            hd = math.atan2(q[0] - p[0], (q[1] - p[1]) * math.cos(math.radians(p[0])))
            hit = None
            for tg, g in ways:
                for a, b in zip(g, g[1:]):
                    # distance point-segment en plan local
                    kx = 111320 * math.cos(math.radians(p[0])); ky = 111320
                    ax, ay, bx, by = (a[1] - p[1]) * kx, (a[0] - p[0]) * ky, (b[1] - p[1]) * kx, (b[0] - p[0]) * ky
                    dx, dy = bx - ax, by - ay; L = dx * dx + dy * dy
                    u = 0 if L == 0 else max(0, min(1, -(ax * dx + ay * dy) / L))
                    if math.hypot(ax + u * dx, ay + u * dy) > TOL_M:
                        continue
                    ang = abs(math.degrees(math.atan2(dy, dx) - hd)) % 180
                    if min(ang, 180 - ang) < ANGLE_DEG:
                        hit = tg; break
                if hit:
                    break
            lab.append(hit)
        plages = []; k0 = None
        for k in range(len(lab) + 1):
            cur = lab[k] if k < len(lab) else None
            if k0 is not None and cur != lab[k0]:
                plages.append({"tag": lab[k0], "debut": P[k0], "fin": P[k - 1], "longueur_m": round(STEP_M * (k - k0), 1)})
                counts[lab[k0]] = counts.get(lab[k0], 0) + 1
                k0 = None
            if cur and k0 is None:
                k0 = k
        out[t["id"]] = {"base_osm": BASE, "serie": t["serie"], "plages": plages,
                        "longueur_temoin_m": round(STEP_M * (len(P) - 1), 1)}
    json.dump(out, open(f"{B}/v1/temoins-v1-tags.json", "w"))
    print(json.dumps({"temoins": len(out), "avec_plage": sum(1 for v in out.values() if v["plages"]), "plages_par_tag": counts}))


if __name__ == "__main__":
    main(sys.argv[1])
