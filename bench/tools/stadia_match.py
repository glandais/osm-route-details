#!/usr/bin/env python3
"""Recale une trace enregistrée sur la route avec Stadia Maps (trace_attributes, Valhalla), et garde
la réponse comme référence de revêtement (R3).

Étapes :
1. Lecture du GPX (altitude = baromètre), découpage en segments aux sauts de plus de GAP_M entre
   deux points consécutifs (transport : sinon le recalage invente une route entre les deux). Seuil
   au-dessus du plus long tunnel cyclable du corpus (1,76 km), où le GPS peut décrocher ; les sauts
   plus courts (pause avec reprise quelques centaines de mètres plus loin, point aberrant) sont
   laissés au recalage, segments de moins de MIN_SEG_KM écartés ; chaque segment est traité comme une trace
   <id>.sN ; rééchantillonnage tous les STEP_M mètres.
2. Appels trace_attributes (costing bicycle, shape_match map_snap) par tronçons de CHUNK_PTS points
   chevauchants de OVERLAP_PTS ; réponses brutes écrites telles quelles (régénérables, hors git).
3. Géométrie recalée = concaténation des `shape` (polyline6) ; chaque point d'origine apparié reçoit
   son abscisse curviligne sur la géométrie recalée (projection monotone du point apparié).
4. Écritures :
   - <id>.gpx : géométrie recalée, sans altitude (l'entrée du service) ; découpée en <id>.partN.gpx si
     elle dépasse MAX_KM ou MAX_PTS (bornes d'entrée de l'API) ;
   - <id>.baro.csv : s_recale_m, baro_m, s_origine_m, distance_au_trace_m, pour les points appariés
     (la référence d'altitude, transportée sur la géométrie recalée) ;
   - <id>.surface.csv : s_debut_m, s_fin_m, surface, road_class, use, way_id (référence R3).
La clé est lue dans STADIAMAPS_KEY, ou dans le premier .env trouvé en remontant ; elle n'est jamais
écrite ni affichée.

Usage : stadia_match.py trace.gpx id dossier_sortie
"""
import json
import math
import os
import re
import sys
import time
import urllib.error
import urllib.request

API = "https://api.stadiamaps.com/trace_attributes/v1"
UA = "osm-route-details-bench/0.1 (usage personnel)"
STEP_M = 25.0
CHUNK_PTS = 2000
OVERLAP_PTS = 40
MAX_KM = 200.0
MAX_PTS = 16000
GAP_M = 2000.0
MIN_SEG_KM = 5.0
ATTRS = ["edge.surface", "edge.road_class", "edge.use", "edge.length", "edge.way_id",
         "edge.begin_shape_index", "edge.end_shape_index", "matched.point", "matched.type",
         "matched.edge_index", "matched.distance_from_trace_point", "shape"]


def api_key():
    k = os.environ.get("STADIAMAPS_KEY")
    if k:
        return k
    d = os.path.abspath(os.getcwd())
    while True:
        p = os.path.join(d, ".env")
        if os.path.exists(p):
            for line in open(p):
                if line.startswith("STADIAMAPS_KEY="):
                    return line.split("=", 1)[1].strip().strip('"')
        if os.path.dirname(d) == d:
            raise SystemExit("STADIAMAPS_KEY introuvable (environnement ou .env)")
        d = os.path.dirname(d)


def hav(a, b):
    r = 6371008.8
    p1, p2 = math.radians(a[0]), math.radians(b[0])
    dp, dl = p2 - p1, math.radians(b[1] - a[1])
    h = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * r * math.asin(math.sqrt(h))


def decode6(s):
    i = lat = lng = 0
    out = []
    while i < len(s):
        vals = []
        for _ in range(2):
            r = sh = 0
            while True:
                b = ord(s[i]) - 63
                i += 1
                r |= (b & 31) << sh
                sh += 5
                if b < 32:
                    break
            vals.append(~(r >> 1) if r & 1 else r >> 1)
        lat += vals[0]
        lng += vals[1]
        out.append((lat / 1e6, lng / 1e6))
    return out


def read_gpx(path):
    txt = open(path).read()
    pts = re.findall(r'<trkpt lat="([-\d.]+)" lon="([-\d.]+)">\s*<ele>([-\d.]+)</ele>', txt)
    return [(float(a), float(b), float(c)) for a, b, c in pts]


def resample(pts, step):
    out = [(0.0, *pts[0])]
    s = 0.0
    nxt = step
    for a, b in zip(pts, pts[1:]):
        d = hav(a, b)
        while d > 0 and s + d >= nxt:
            t = (nxt - s) / d
            out.append((nxt, a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])))
            nxt += step
        s += d
    return out


def call(key, shape):
    body = {"shape": [{"lat": p[1], "lon": p[2]} for p in shape], "costing": "bicycle",
            "shape_match": "map_snap", "filters": {"attributes": ATTRS, "action": "include"}}
    for attempt in range(4):
        req = urllib.request.Request(f"{API}?api_key={key}", data=json.dumps(body).encode(),
                                     headers={"Content-Type": "application/json", "User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code in (429, 500, 502, 503, 504) and attempt < 3:
                time.sleep(5 * (attempt + 1))
                continue
            raise SystemExit(f"Stadia HTTP {e.code} : {e.read()[:200]!r}")


def project_forward(poly, cum, p, start):
    """Projection de p sur poly à partir du segment start (recherche bornée vers l'avant)."""
    best = (math.inf, start, 0.0)
    c = math.cos(math.radians(p[0]))
    for i in range(start, min(len(poly) - 1, start + 400)):
        a, b = poly[i], poly[i + 1]
        ax, ay = (a[1] - p[1]) * c, a[0] - p[0]
        bx, by = (b[1] - p[1]) * c, b[0] - p[0]
        dx, dy = bx - ax, by - ay
        L = dx * dx + dy * dy
        t = 0.0 if L == 0 else max(0.0, min(1.0, -(ax * dx + ay * dy) / L))
        d = math.hypot(ax + t * dx, ay + t * dy)
        if d < best[0]:
            best = (d, i, t)
    _, i, t = best
    return cum[i] + t * (cum[i + 1] - cum[i]), i


def write_gpx(path, pts, name):
    with open(path, "w") as f:
        f.write('<?xml version="1.0" encoding="UTF-8"?>\n<gpx version="1.1" creator="osm-route-details bench" '
                'xmlns="http://www.topografix.com/GPX/1/1">\n')
        f.write(f"<trk><name>{name}</name><trkseg>\n")
        for la, lo in pts:
            f.write(f'<trkpt lat="{la:.6f}" lon="{lo:.6f}"/>\n')
        f.write("</trkseg></trk></gpx>\n")


def segments(raw):
    segs, cur = [], [raw[0]]
    for a, b in zip(raw, raw[1:]):
        if hav(a, b) > GAP_M:
            segs.append(cur)
            cur = []
        cur.append(b)
    segs.append(cur)
    return [g for g in segs if len(g) > 1 and sum(hav(a, b) for a, b in zip(g, g[1:])) >= MIN_SEG_KM * 1000]


def main(gpx, tid, outdir):
    key = api_key()
    os.makedirs(os.path.join(outdir, "brut"), exist_ok=True)
    segs = segments(read_gpx(gpx))
    if len(segs) == 1:
        process(key, segs[0], tid, outdir)
    else:
        for k, seg in enumerate(segs):
            process(key, seg, f"{tid}.s{k + 1}", outdir)


def process(key, raw_pts, tid, outdir):
    pts = resample(raw_pts, STEP_M)
    poly, baro_rows, surf_rows = [], [], []
    starts = list(range(0, len(pts), CHUNK_PTS - OVERLAP_PTS))
    for k, st in enumerate(starts):
        chunk = pts[st:st + CHUNK_PTS]
        if len(chunk) < 2 or (k > 0 and starts[k - 1] + CHUNK_PTS >= len(pts)):
            break  # le tronçon précédent atteignait déjà la fin
        raw = os.path.join(outdir, "brut", f"{tid}.{k:02d}.json")
        if os.path.exists(raw):
            d = json.load(open(raw))
        else:
            d = call(key, chunk)
            json.dump(d, open(raw, "w"))
            time.sleep(0.5)
        shape = decode6(d["shape"])
        # raccord : on reprend après le point recalé le plus proche de la fin du tronçon précédent
        first = 0
        if poly and k > 0:
            first = min(range(len(shape)), key=lambda i: hav(shape[i], poly[-1]))
        dup = 1 if poly and shape[first] == poly[-1] else 0
        start_i = first + dup
        base = len(poly)
        poly += shape[start_i:]
        offset = base - start_i  # indice dans shape → indice dans poly
        floor = max(0, base - 1)
        for e in d["edges"]:
            b, en = e.get("begin_shape_index"), e.get("end_shape_index")
            if b is None or en is None or en + offset <= floor and base:
                continue  # arête du chevauchement, déjà couverte par le tronçon précédent
            surf_rows.append((max(floor, b + offset), en + offset, e.get("surface", ""),
                              e.get("road_class", ""), e.get("use", ""), e.get("way_id", "")))
        # chevauchement partagé à mi-chemin : chaque point d'origine n'est gardé qu'une fois
        skip = OVERLAP_PTS // 2 if k > 0 else 0
        last = len(chunk) if st + len(chunk) >= len(pts) else len(chunk) - OVERLAP_PTS // 2
        for j, (p, m) in enumerate(zip(chunk, d["matched_points"])):
            if j < skip or j >= last or m.get("type") != "matched":
                continue
            baro_rows.append((p[0], p[3], (m["lat"], m["lon"]), m.get("distance_from_trace_point", "")))
    cum = [0.0]
    for a, b in zip(poly, poly[1:]):
        cum.append(cum[-1] + hav(a, b))
    with open(os.path.join(outdir, f"{tid}.baro.csv"), "w") as f:
        f.write("s_recale_m,baro_m,s_origine_m,distance_au_trace_m\n")
        i = 0
        last_s = -math.inf
        for s0, z, mp, dd in baro_rows:
            s, i = project_forward(poly, cum, mp, max(0, i - 5))
            if s < last_s - 20:
                continue  # projection ambiguë (lacet, boucle) : point écarté plutôt qu'un recul
            last_s = max(last_s, s)
            f.write(f"{s:.1f},{z:.2f},{s0:.1f},{dd}\n")
    with open(os.path.join(outdir, f"{tid}.surface.csv"), "w") as f:
        f.write("s_debut_m,s_fin_m,surface,road_class,use,way_id\n")
        for b, en, *rest in surf_rows:
            if b < len(cum) and en < len(cum):
                f.write(f"{cum[b]:.1f},{cum[en]:.1f}," + ",".join(str(x) for x in rest) + "\n")
    total_km = cum[-1] / 1000
    nparts = max(math.ceil(total_km / MAX_KM), math.ceil(len(poly) / MAX_PTS))
    if nparts <= 1:
        write_gpx(os.path.join(outdir, f"{tid}.gpx"), poly, tid)
    else:
        size = cum[-1] / nparts
        idx = 0
        for n in range(nparts):
            lim = size * (n + 1)
            part = []
            while idx < len(poly) and (cum[idx] <= lim or n == nparts - 1):
                part.append(poly[idx])
                idx += 1
            if idx < len(poly):
                part.append(poly[idx])  # raccord : le tronçon suivant repart de ce point
            write_gpx(os.path.join(outdir, f"{tid}.part{n + 1}.gpx"), part, f"{tid} {n + 1}/{nparts}")
    print(f"{tid},{len(pts)},{len(baro_rows)},{len(poly)},{total_km:.1f},{nparts}")


if __name__ == "__main__":
    main(*sys.argv[1:4])
