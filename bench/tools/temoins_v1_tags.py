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

Série : variable d'environnement SERIE (v1 par défaut ; v11 : private/v11/, base 2026-11-02T09:32Z, date
des données du recalage de V1.1 ; v12 : private/v12/, base 2026-11-03T06:24:42Z, tileset_last_modified du
serveur Valhalla au recalage de V1.2). Usage : [SERIE=v11|v12] temoins_v1_tags.py private/
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

SERIE = os.environ.get("SERIE", "v1")
BASE = {"v1": "2026-11-01T13:00:00Z", "v11": "2026-11-02T09:32:00Z", "v12": "2026-11-03T06:24:42Z"}[SERIE]
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
        P = gpx(f"{B}/{SERIE}/planifies/{t['trace']}.gpx")
        if "debut" not in t:   # témoin planifié : l'itinéraire entier
            return P
        P = densify(P)         # tronçon d'un itinéraire (E4) : entre ses extrémités
    else:
        s = json.load(open(f"{B}/strava/streams-{SERIE}/{pseudo_to_strava[t['trace']]}.json"))
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
    T = json.load(open(f"{B}/{SERIE}/temoins-{SERIE}.json"))
    p2s = {r["pseudo"]: r["strava_id"] for r in csv.DictReader(open(f"{B}/{SERIE}/perso-map-{SERIE}.csv"))}
    out, counts = {}, {}
    for t in T:
        P = densify(geometry(B, t, p2s))
        la = [p[0] for p in P]; lo = [p[1] for p in P]
        raw = f"{B}/{SERIE}/overpass/temoin-{t['id']}.json"
        if os.path.exists(raw):
            d = json.load(open(raw))
        else:
            d = overpass((min(la) - .001, min(lo) - .0015, max(la) + .001, max(lo) + .0015))
            json.dump(d, open(raw, "w")); time.sleep(2)
        ways = [(tag_of(e.get("tags", {})), [(g["lat"], g["lon"]) for g in e["geometry"]])
                for e in d.get("elements", []) if e.get("type") == "way" and "geometry" in e and tag_of(e.get("tags", {}))]
        pas = [hav(a, b) for a, b in zip(P, P[1:])] + [0.0]
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
                # longueur portée par chaque point : son pas jusqu'au suivant (décision de l'utilisateur,
                # 2026-11-02 : les pas de P ne valent pas STEP_M, de quelques mètres aux arrêts à 2 × STEP_M)
                plages.append({"tag": lab[k0], "debut": P[k0], "fin": P[k - 1], "longueur_m": round(sum(pas[k0:k]), 1)})
                counts[lab[k0]] = counts.get(lab[k0], 0) + 1
                k0 = None
            if cur and k0 is None:
                k0 = k
        out[t["id"]] = {"base_osm": BASE, "serie": t["serie"], "plages": plages,
                        "longueur_temoin_m": round(sum(pas), 1)}
    json.dump(out, open(f"{B}/{SERIE}/temoins-{SERIE}-tags.json", "w"))
    print(json.dumps({"temoins": len(out), "avec_plage": sum(1 for v in out.values() if v["plages"]), "plages_par_tag": counts}))


if __name__ == "__main__":
    main(sys.argv[1])
