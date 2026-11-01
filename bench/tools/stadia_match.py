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
   Quand Valhalla ne sait pas enchaîner deux sous-parcours (points `unmatched` entre les deux), son
   `shape` les relie par une ligne droite, à travers champs, portée par une arête bien plus courte
   que ce segment. Une arête dont la forme dépasse sa longueur de plus de JUMP_EXCESS_M contient un
   tel saut : son plus long segment est remplacé par les points d'origine de la trace entre les deux
   sous-parcours (ils gardent leur baro, marqués insere = 1), et cette plage n'a pas de référence de revêtement
   (`use` = `discontinuite`, surface vide, à exclure de R3).
4. Écritures :
   - <id>.gpx : géométrie recalée, sans altitude (l'entrée du service) ; découpée en <id>.partN.gpx si
     elle dépasse MAX_KM ou MAX_PTS (bornes d'entrée de l'API) ;
   - <id>.baro.csv : s_recale_m, baro_m, s_origine_m, distance_au_trace_m, insere, pour les points
     appariés et les points d'origine insérés à la place d'un saut (insere = 1) : la référence
     d'altitude, transportée sur la géométrie recalée. Un point inséré ne garde son baro qu'à 30 m au
     plus d'une voie des tuiles de la campagne qui juge la trace : ce test est fait par ord-bench, pas
     ici (README §2.1) ;
   - <id>.surface.csv : s_debut_m, s_fin_m, surface, road_class, use, way_id (référence R3).
La clé est lue dans STADIAMAPS_KEY, ou dans le premier .env trouvé en remontant ; elle n'est jamais
écrite ni affichée. Avec VALHALLA_URL (ex. https://valhalla1.openstreetmap.de/trace_attributes, serveur
public de la FOSSGIS), les appels vont à ce Valhalla sans clé, à MIN_INTERVAL_S secondes d'écart au
moins (1 requête par seconde, consigne de l'utilisateur du 2026-11-01) ; la source est écrite dans
chaque réponse brute (clé `_source`).

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
RACCORD_TOL_M = 15.0
PROJ_TOL_M = 10.0  # transport du baro : premier segment à moins de (plus proche + 10 m)  # raccord des tronçons : premier point à moins de (plus proche + 15 m)
MAX_KM = 200.0
MAX_PTS = 16000
GAP_M = 2000.0
BARO_GAP_M = 100.0  # = baro_gap_max_m d'ord-bench : pas brut au-delà duquel le baro n'est pas interpolé
MIN_SEG_KM = 5.0
JUMP_EXCESS_M = 50.0  # 39 000 arêtes du corpus : excès < 25 m sauf sur les sauts
JUMP_SNAP_M = 50.0  # point apparié encadrant ↔ sommet du shape avant ou après le saut
JUMP_WINDOW = 400  # sommets examinés de part et d'autre du saut
ATTRS = ["edge.surface", "edge.road_class", "edge.use", "edge.length", "edge.way_id",
         "edge.begin_shape_index", "edge.end_shape_index", "matched.point", "matched.type",
         "matched.edge_index", "matched.distance_from_trace_point", "shape"]


VALHALLA_URL = os.environ.get("VALHALLA_URL")
MIN_INTERVAL_S = 1.0
_last_call = [0.0]


def api_key():
    if VALHALLA_URL:
        return None
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
    """Points tous les `step` m : (s, lat, lon, baro, dans_un_trou). Un point tiré à l'intérieur d'un pas
    brut de plus de BARO_GAP_M (GPS perdu, en tunnel ou galerie) sert au recalage mais ne porte pas de
    baro : sa position est une ligne droite et son baro une interpolation (README §2.1, trou de
    référence)."""
    out = [(0.0, *pts[0], False)]
    s = 0.0
    nxt = step
    for a, b in zip(pts, pts[1:]):
        d = hav(a, b)
        while d > 0 and s + d >= nxt:
            t = (nxt - s) / d
            out.append((nxt, a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2]),
                        d > BARO_GAP_M))
            nxt += step
        s += d
    return out


def call(key, shape):
    body = {"shape": [{"lat": p[1], "lon": p[2]} for p in shape], "costing": "bicycle",
            "shape_match": "map_snap", "filters": {"attributes": ATTRS, "action": "include"}}
    url = VALHALLA_URL or f"{API}?api_key={key}"
    for attempt in range(4):
        wait = MIN_INTERVAL_S - (time.monotonic() - _last_call[0])
        if wait > 0:
            time.sleep(wait)
        _last_call[0] = time.monotonic()
        req = urllib.request.Request(url, data=json.dumps(body).encode(),
                                     headers={"Content-Type": "application/json", "User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                d = json.load(r)
                d["_source"] = VALHALLA_URL or API
                return d
        except urllib.error.HTTPError as e:
            if e.code in (429, 500, 502, 503, 504) and attempt < 3:
                time.sleep(5 * (attempt + 1))
                continue
            raise SystemExit(f"Valhalla HTTP {e.code} : {e.read()[:200]!r}")


def project_forward(poly, cum, p, start, min_s=-math.inf):
    """Projection de p sur poly à partir du segment start (recherche bornée vers l'avant). On retient le
    PREMIER segment à moins de (plus proche + PROJ_TOL_M) : sur un aller-retour, l'aller et le retour
    se superposent, et le plus proche peut être sur le retour."""
    cand = []
    c = math.cos(math.radians(p[0]))
    for i in range(start, min(len(poly) - 1, start + 400)):
        a, b = poly[i], poly[i + 1]
        ax, ay = (a[1] - p[1]) * c, a[0] - p[0]
        bx, by = (b[1] - p[1]) * c, b[0] - p[0]
        dx, dy = bx - ax, by - ay
        L = dx * dx + dy * dy
        t = 0.0 if L == 0 else max(0.0, min(1.0, -(ax * dx + ay * dy) / L))
        if cum[i] + t * (cum[i + 1] - cum[i]) >= min_s:  # jamais en arrière de la dernière abscisse retenue
            cand.append((math.hypot(ax + t * dx, ay + t * dy), i, t))
    if not cand:
        return None, start
    tol = min(x[0] for x in cand) + PROJ_TOL_M / 111320.0
    k = next(k for k, x in enumerate(cand) if x[0] <= tol)
    while k + 1 < len(cand) and cand[k + 1][0] <= cand[k][0]:
        k += 1  # minimum local sur ce passage
    _, i, t = cand[k]
    return cum[i] + t * (cum[i + 1] - cum[i]), i


def write_gpx(path, pts, name):
    with open(path, "w") as f:
        f.write('<?xml version="1.0" encoding="UTF-8"?>\n<gpx version="1.1" creator="osm-route-details bench" '
                'xmlns="http://www.topografix.com/GPX/1/1">\n')
        f.write(f"<trk><name>{name}</name><trkseg>\n")
        for la, lo in pts:
            f.write(f'<trkpt lat="{la:.6f}" lon="{lo:.6f}"/>\n')
        f.write("</trkseg></trk></gpx>\n")


def fill_jumps(shape, d, chunk):
    """Remplace les sauts du shape par les points d'origine. Renvoie (shape, indice ancien → nouveau,
    plages [début, fin] des sauts dans le nouveau shape, indices des points d'origine insérés)."""
    pos = {j: (m["lat"], m["lon"]) for j, m in enumerate(d["matched_points"]) if m.get("type") != "unmatched"}
    n = len(shape)

    def near(p, lo, hi):
        """(distance, v) de p au plus proche segment shape[v]–shape[v + 1], lo ≤ v < hi."""
        return min(((seg_dist(p, shape[v], shape[v + 1]), v) for v in range(max(0, lo), min(n - 1, hi))),
                   default=(math.inf, None))

    jumps = []  # (va, vb, ja, jb) : shape[va + 1 .. vb] remplacé par P(ja), points d'origine, P(jb)
    for e in d["edges"]:
        b, en = e.get("begin_shape_index"), e.get("end_shape_index")
        if b is None or en is None or en <= b:
            continue
        segs = [hav(shape[i], shape[i + 1]) for i in range(b, en)]
        if sum(segs) - e.get("length", 0) * 1000 <= JUMP_EXCESS_M:
            continue
        i = b + max(range(len(segs)), key=segs.__getitem__)
        cand = [j for j, p in pos.items() if min(hav(p, shape[i]), hav(p, shape[i + 1])) < 2000]
        found = None
        for w in (JUMP_WINDOW, 40, 8):  # fenêtre resserrée si la première enjambe une boucle
            # A : points appariés sur le shape avant le saut ; B : après. Dernier de A avant le premier de B.
            A = {j: near(pos[j], i - w, i) for j in cand}
            B = {j: near(pos[j], i + 1, i + 1 + w) for j in cand}
            A = {j: v for j, (dd, v) in A.items() if dd <= JUMP_SNAP_M}
            B = {j: v for j, (dd, v) in B.items() if dd <= JUMP_SNAP_M}
            jb = min((j for j in B if A and j > min(A)), default=None)
            ja = max((j for j in A if jb is not None and j < jb), default=None)
            if ja is None:
                continue
            # le shape retiré, saut compris, ne doit pas dépasser de beaucoup la trace d'origine qui le remplace
            cut = sum(hav(shape[v], shape[v + 1]) for v in range(A[ja], B[jb] + 1)) - hav(shape[i], shape[i + 1])
            if cut <= 2 * (jb - ja) * STEP_M + 200:
                found = (A[ja], B[jb], ja, jb)
                break
        # sinon le saut reste droit, mais sans référence de revêtement
        jumps.append(found or (i, i, None, None))
    jumps.sort()
    out, imap, gaps, filled = [], [], [], set()
    v = 0
    for va, vb, ja, jb in jumps:
        if va < v:
            continue  # chevauche un saut déjà traité
        while v <= va:
            imap.append(len(out))
            out.append(shape[v])
            v += 1
        if ja is None:
            gaps.append((len(out) - 1, len(out)))  # segment droit shape[va]–shape[va + 1]
            continue
        start = len(out)
        out.append(pos[ja])
        for j in range(ja + 1, jb):
            out.append((chunk[j][1], chunk[j][2]))
            filled.add(j)
        out.append(pos[jb])
        while v <= vb:  # sommets du saut, retirés
            imap.append(start)
            v += 1
        gaps.append((start, len(out) - 1))
    while v < n:
        imap.append(len(out))
        out.append(shape[v])
        v += 1
    return out, imap, gaps, filled


def seg_dist(p, a, b):
    """Distance (m) du point p au segment a–b, en projection locale."""
    c = math.cos(math.radians(p[0])) * 111195.0
    ax, ay = (a[1] - p[1]) * c, (a[0] - p[0]) * 111195.0
    bx, by = (b[1] - p[1]) * c, (b[0] - p[0]) * 111195.0
    dx, dy = bx - ax, by - ay
    L = dx * dx + dy * dy
    t = 0.0 if L == 0 else max(0.0, min(1.0, -(ax * dx + ay * dy) / L))
    return math.hypot(ax + t * dx, ay + t * dy)


def split_gaps(row, gaps):
    """Découpe une ligne de revêtement (début, fin, …) autour des sauts."""
    b, en, *rest = row
    out = []
    for g0, g1 in gaps:
        if g1 <= b or g0 >= en:
            continue
        if g0 > b:
            out.append((b, g0, *rest))
        out.append((max(b, g0), min(en, g1), "", "", "discontinuite", ""))
        b = min(en, g1)
    if b < en:
        out.append((b, en, *rest))
    return out


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
    n_jumps = 0
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
        shape, imap, gaps, filled = fill_jumps(decode6(d["shape"]), d, chunk)
        n_jumps += len(gaps)
        # raccord : on reprend au PREMIER point recalé (dans l'ordre de la géométrie) proche de la fin du
        # tronçon précédent, et non au plus proche : sur un aller-retour, le plus proche peut être sur le
        # retour, et tout l'aller-retour serait sauté.
        first = 0
        if poly and k > 0:
            dist = [hav(p, poly[-1]) for p in shape]
            first = next(i for i, x in enumerate(dist) if x <= min(dist) + RACCORD_TOL_M)
            while first + 1 < len(dist) and dist[first + 1] <= dist[first]:
                first += 1  # minimum local sur ce passage : pas de recul de quelques mètres
        dup = 1 if poly and shape[first] == poly[-1] else 0
        start_i = first + dup
        base = len(poly)
        poly += shape[start_i:]
        offset = base - start_i  # indice dans shape → indice dans poly
        floor = max(0, base - 1)
        gaps = [(g0 + offset, g1 + offset) for g0, g1 in gaps if g1 + offset > floor]
        for e in d["edges"]:
            b, en = e.get("begin_shape_index"), e.get("end_shape_index")
            if b is None or en is None:
                continue
            b, en = imap[b], imap[en]
            if en + offset <= floor and base:
                continue  # arête du chevauchement, déjà couverte par le tronçon précédent
            surf_rows += split_gaps((max(floor, b + offset), en + offset, e.get("surface", ""),
                                     e.get("road_class", ""), e.get("use", ""), e.get("way_id", "")), gaps)
        # chevauchement partagé à mi-chemin : chaque point d'origine n'est gardé qu'une fois
        skip = OVERLAP_PTS // 2 if k > 0 else 0
        last = len(chunk) if st + len(chunk) >= len(pts) else len(chunk) - OVERLAP_PTS // 2
        for j, (p, m) in enumerate(zip(chunk, d["matched_points"])):
            if j < skip or j >= last:
                continue
            if p[4]:
                continue  # dans un trou de la trace brute : pas de baro transporté
            if m.get("type") == "matched":
                baro_rows.append((p[0], p[3], (m["lat"], m["lon"]), m.get("distance_from_trace_point", ""), 0))
            elif j in filled:  # point d'origine inséré à la place d'un saut : il est sur la géométrie
                baro_rows.append((p[0], p[3], (p[1], p[2]), 0.0, 1))
    cum = [0.0]
    for a, b in zip(poly, poly[1:]):
        cum.append(cum[-1] + hav(a, b))
    with open(os.path.join(outdir, f"{tid}.baro.csv"), "w") as f:
        f.write("s_recale_m,baro_m,s_origine_m,distance_au_trace_m,insere\n")
        i = 0
        last_s = -math.inf
        for s0, z, mp, dd, ins in baro_rows:
            s, j = project_forward(poly, cum, mp, max(0, i - 5), last_s - 20)
            if s is None:
                continue  # projection ambiguë (lacet, boucle) : point écarté plutôt qu'un recul
            i = j
            last_s = max(last_s, s)
            f.write(f"{s:.1f},{z:.2f},{s0:.1f},{dd},{ins}\n")
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
    print(f"{tid},{len(pts)},{len(baro_rows)},{len(poly)},{total_km:.1f},{nparts},{n_jumps}")


if __name__ == "__main__":
    main(*sys.argv[1:4])
