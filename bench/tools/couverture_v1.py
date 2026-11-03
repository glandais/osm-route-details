#!/usr/bin/env python3
"""Contrôle de couverture et tirage de la série tenue à l'écart V1, sans le service (README §4 ter.4).

Lit les streams Strava qualifiés, les itinéraires planifiés (planifier_v1.py), les réponses Overpass
par emprise, les tuiles de la campagne V1 dans le cache du banc (tuiles_v1.py), le corpus de
calibration et l'API altimétrie de l'IGN ; ne lit jamais le service. Trois étapes :

1. cas : pour chaque trace (sorties fiables, itinéraires planifiés E4 et S2), ouvrages franchis
   (chaînes de ways `highway` bridge/tunnel contiguës, ≥ MIN_LEN_M, trace à moins de TOL_M de A, du
   milieu et de B), croisements sous un pont (E5), témoins S2. Exclusions du banc :
   - déjà vu : à moins de MASK_M d'un ouvrage de calibration, ou franchi par une trace de
     calibration, de vcyclist ou d'osm-surface ;
   - tuiles : voie de même nature à 10 m au plus du milieu, sinon `absent_des_donnees` ; hors graphe
     vélo par la règle de tags (T03 §4.2 étape 1) sur `kind` / `bicycle` ;
   - `limite_donnees` (tunnel=avalanche_protector) ;
   - attribution (§4 ter.2) sur la trace brute, voies concurrentes des tuiles, « un tablier, un
     ouvrage » (même polygone `bridges`, sinon ≤ 10 m sur toute l'emprise) ;
   - S2 : stations à plus de TEMOIN_LIBRE_M de tout pont, tunnel ou croisement, hors plages hors
     graphe vélo (§4 ter.5 : route motorisée fermée au vélo, voie non ferrée la plus proche à 30 m,
     plage ≥ 50 m) ; témoin d'une sortie = tronçon ≥ TEMOIN_MIN_M d'une même nature OSM ; témoin
     planifié = un cas, nature déclarée confirmée par OSM sur ≥ 50 % (00e500f).
2. IGN : MNH LiDAR au milieu des ponts (médiane MNS sur 25 m − MNT), pont gardé si MNH ≥ MIN_MNH_M
   (règle du corpus MVP) ; contrôle de niveau (§2.1) des ponts à MNH ≥ 10 m sur le baro recalé
   localement au MNT (décalage = médiane baro − MNT de 200 à 1 500 m de part et d'autre).
3. tirage : strates E4, E5, S2, R1_R2, E3, Q1, reste ; ordre sha256('osm-route-details:v1:' + id) ;
   un cas va à la série tenue à l'écart tant que la cible de la strate (minimum majoré de 25 %,
   64b7067) n'est pas atteinte ou que moins d'un tiers de la strate y est ; un groupe d'ouvrages
   (composante à moins de NEAR_M) est un seul cas ; un cas affecté garde son affectation. E3 ne
   compte que les ponts passés par une trace baro fiable ; Q1 les négatifs au défaut attesté hors
   du qualificateur (930f6e8, 305abd7) ; groupe E2b par 13e0ed1.
   À partir de V1.2 (SERIE hors FIGES), E5 se compte en sites (README §4 quater.4) : croisements
   d'une même trace à SITE_M au plus l'un de l'autre, de proche en proche ; V1 et V1.1 gardent le
   compte en croisements de leur tirage figé.

Non contrôlé ici : bandes de portail E4, référence incertaine ; la campagne peut encore exclure.

Sorties dans private/v1/ (hors git : géométries, identifiants de way et de sortie) ; le résumé
imprimé ne porte que des comptes.

Usage : couverture_v1.py cas private/ ; couverture_v1.py ign private/ ; couverture_v1.py tirage private/
"""
import csv
import glob
import hashlib
import json
import math
import os
import re
import statistics
import sys
import time
import urllib.parse
import urllib.request

TOL_M = 40.0
NEAR_M = 150.0
SITE_M = 40.0   # E5 par site (README §4 quater.4, à partir de V1.2) : croisements d'une même trace à 40 m au plus
FIGES = ("v1", "v11")   # tirages figés : E5 compté en croisements, comme à leur tirage
MASK_M = 500.0
MIN_LEN_M = 50.0
MIN_MNH_M = 3.0
MNH_NIVEAU_M = 10.0
LONG_PONT_M = 150.0
LONG_TUNNEL_M = 300.0
TEMOIN_MIN_M = 300.0
TEMOIN_LIBRE_M = 100.0   # pas d'ouvrage ni de croisement à moins de cette distance
STEP_M = 10.0
# Série contrôlée (README §4 ter.4) : v1 (tirage stratifié) ou v11 (V1.1, décision de l'utilisateur,
# 2026-11-02 : tout le nouveau matériau tenu à l'écart, MVP et V1 en calibration).
SERIE = os.environ.get("SERIE", "v1")
SEL = {"v1": "osm-route-details:v1:", "v11": "osm-route-details:v1.1:"}[SERIE]
ALTI = "https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json"
UA = "osm-route-details-bench/0.1 (usage personnel)"
FERMES = {"motorway", "motorway_link", "trunk", "trunk_link", "pedestrian", "bridleway", "busway"}
OUVERT_VELO = {"yes", "designated", "permissive", "destination", "official"}
MINIMUMS = {
    "E1": {"traces": 8, "km": 800}, "E2b": {"par_groupe": 3}, "E3": {"ponts": 12, "mnh_10m": 3, "plus_de_150m": 3},
    "E4": {"tunnels": 7, "plus_de_300m": 2}, "E5": {"croisements": 8}, "S1": {"ouvrages": 15, "emprise_m": 2000},   # E5 : sites à partir de V1.2 (§4 quater.4)
    "S2": {"temoins": 4, "km": 40, "natures": 3}, "R1_R2": {"parcours": 4, "km": 400}, "M1": {"km": 800},
    "Q1": {"traces": 2},
}


# --- géométrie (plan local en mètres) ---
class Plan:
    def __init__(self, lat0):
        self.ky = 111320.0
        self.kx = 111320.0 * math.cos(math.radians(lat0))

    def xy(self, p):
        return (p[1] * self.kx, p[0] * self.ky)


def seg_dist(p, a, b):
    ax, ay = a; bx, by = b; px, py = p
    dx, dy = bx - ax, by - ay
    L = dx * dx + dy * dy
    t = 0.0 if L == 0 else max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / L))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def poly_dist(p, poly):
    return min(seg_dist(p, poly[i], poly[i + 1]) for i in range(len(poly) - 1)) if len(poly) > 1 else math.hypot(p[0] - poly[0][0], p[1] - poly[0][1])


def length(poly):
    return sum(math.hypot(poly[i + 1][0] - poly[i][0], poly[i + 1][1] - poly[i][1]) for i in range(len(poly) - 1))


def at(poly, frac):
    tgt = length(poly) * frac; acc = 0.0
    for i in range(len(poly) - 1):
        s = math.hypot(poly[i + 1][0] - poly[i][0], poly[i + 1][1] - poly[i][1])
        if acc + s >= tgt and s > 0:
            t = (tgt - acc) / s
            return (poly[i][0] + t * (poly[i + 1][0] - poly[i][0]), poly[i][1] + t * (poly[i + 1][1] - poly[i][1]))
        acc += s
    return poly[-1]


def inter(a, b, c, d):
    (x1, y1), (x2, y2), (x3, y3), (x4, y4) = a, b, c, d
    den = (x2 - x1) * (y4 - y3) - (y2 - y1) * (x4 - x3)
    if den == 0:
        return None
    u = ((x3 - x1) * (y4 - y3) - (y3 - y1) * (x4 - x3)) / den
    v = ((x3 - x1) * (y2 - y1) - (y3 - y1) * (x2 - x1)) / den
    if 0 <= u <= 1 and 0 <= v <= 1:
        ang = abs(math.degrees(math.atan2(y2 - y1, x2 - x1) - math.atan2(y4 - y3, x4 - x3))) % 180
        return (x1 + u * (x2 - x1), y1 + u * (y2 - y1)), min(ang, 180 - ang)
    return None


class Grid:
    """Index de segments par cellules de CELL m."""
    CELL = 100.0

    def __init__(self):
        self.g = {}

    def add(self, key, poly):
        for i in range(len(poly) - 1):
            a, b = poly[i], poly[i + 1]
            for cx in range(int(min(a[0], b[0]) // self.CELL), int(max(a[0], b[0]) // self.CELL) + 1):
                for cy in range(int(min(a[1], b[1]) // self.CELL), int(max(a[1], b[1]) // self.CELL) + 1):
                    self.g.setdefault((cx, cy), set()).add((key, i))

    def near(self, p, r):
        out = set()
        for cx in range(int((p[0] - r) // self.CELL), int((p[0] + r) // self.CELL) + 1):
            for cy in range(int((p[1] - r) // self.CELL), int((p[1] + r) // self.CELL) + 1):
                out |= self.g.get((cx, cy), set())
        return out


def track_dist(p, tpoly, tgrid, r=TOL_M):
    best = float("inf")
    for _, i in tgrid.near(p, r):
        best = min(best, seg_dist(p, tpoly[i], tpoly[i + 1]))
    return best


class UF:
    def __init__(self):
        self.p = {}

    def find(self, x):
        self.p.setdefault(x, x)
        while self.p[x] != x:
            self.p[x] = self.p[self.p[x]]
            x = self.p[x]
        return x

    def union(self, a, b):
        self.p[self.find(a)] = self.find(b)


def gpx_points(path):
    return [(float(a), float(b)) for a, b in re.findall(r'lat="([-\d.]+)"\s+lon="([-\d.]+)"', open(path).read())]


def hav(a, b):
    k = 111320.0
    return math.hypot((a[0] - b[0]) * k, (a[1] - b[1]) * k * math.cos(math.radians(a[0])))


def kind_of(t):
    if t.get("tunnel", "no") != "no":
        return "tunnel"
    if t.get("bridge", "no") != "no":
        return "bridge"
    return None


def hors_graphe(t):
    b = t.get("bicycle")
    if b in OUVERT_VELO:
        return False
    if b in ("no", "use_sidepath"):
        return True
    return t.get("highway") in FERMES


# --- étape 1 : cas ---
def calibration_refs(B):
    refs = []
    for f in [f"{B}/ouvrages-tous-geom.json", f"{B}/ouvrages-perso-geom.json"]:
        for o in json.load(open(f)):
            refs += [tuple(p) for p in o["pts"].values()]
    tracks = sorted(glob.glob(f"{B}/traces/*.gpx")) + sorted(glob.glob(f"{B}/../data/vcyclist/**/*.gpx", recursive=True)) \
        + sorted(glob.glob(f"{B}/../data/osm-surface/*.gpx"))
    if SERIE != "v1":
        # V1 est en calibration : ses ouvrages, croisements et itinéraires planifiés
        for o in json.load(open(f"{B}/v1/ouvrages-v1.json")):
            refs += [tuple(p) for p in o["pts"].values()]
        tracks += sorted(glob.glob(f"{B}/v1/planifies/*.gpx"))
    tracks = [t for t in tracks if ".trim1km." not in t]
    cg = {}
    for n, t in enumerate(tracks):
        for q in gpx_points(t):
            cg.setdefault((round(q[0] * 1000), round(q[1] * 1000)), []).append((n, q))
    return refs, cg


def seen_by_track(cg, pts):
    """Index des traces de calibration passant à moins de TOL_M de chacun des points."""
    sets = []
    for p in pts:
        c = (round(p[0] * 1000), round(p[1] * 1000)); s = set()
        for dx in (-1, 0, 1):
            for dy in (-1, 0, 1):
                s |= {n for n, q in cg.get((c[0] + dx, c[1] + dy), []) if hav(p, q) < TOL_M}
        sets.append(s)
    return set.intersection(*sets) if sets else set()


def chains(ways):
    """Chaînes d'un même type partageant une extrémité ; renvoie [(type, [way ids], poly latlon)]."""
    uf = UF(); ends = {}
    for wid, (k, t, g) in ways.items():
        for e in (g[0], g[-1]):
            ends.setdefault((k, e), []).append(wid)
    for lst in ends.values():
        for w in lst[1:]:
            uf.union(lst[0], w)
    comp = {}
    for wid in ways:
        comp.setdefault(uf.find(wid), []).append(wid)
    out = []
    for ws in comp.values():
        deg = {}
        for w in ws:
            for e in (ways[w][2][0], ways[w][2][-1]):
                deg[e] = deg.get(e, 0) + 1
        if len(ws) > 1 and (max(deg.values()) > 2 or sum(1 for v in deg.values() if v == 1) != 2):
            out += [(ways[w][0], [w], ways[w][2]) for w in ws]   # ramifié : ways séparées
            continue
        if len(ws) == 1:
            out.append((ways[ws[0]][0], ws, ways[ws[0]][2])); continue
        start = next(e for e, v in deg.items() if v == 1)
        poly = [start]; left = set(ws); order = []
        while left:
            w = next((w for w in left if ways[w][2][0] == poly[-1] or ways[w][2][-1] == poly[-1]), None)
            if w is None:
                break
            g = ways[w][2] if ways[w][2][0] == poly[-1] else ways[w][2][::-1]
            poly += g[1:]; left.discard(w); order.append(w)
        if left:
            out += [(ways[w][0], [w], ways[w][2]) for w in ws]
        else:
            out.append((ways[ws[0]][0], order, poly))
    return out


def load_overpass(B, i):
    E = []
    for f in [f"{B}/{SERIE}/overpass/{i}.json", f"{B}/{SERIE}/overpass/{i}.cliff.json"]:
        if os.path.exists(f):
            E += [e for e in json.load(open(f)).get("elements", []) if e.get("type") == "way" and "geometry" in e]
    return E


def load_forets(B, i):
    """Polygones forêt/bois (ways fermées et anneaux extérieurs des relations) : listes de [lat, lon]."""
    f = f"{B}/{SERIE}/overpass/{i}.foret.json"
    if not os.path.exists(f):
        return []
    out = []
    for e in json.load(open(f)).get("elements", []):
        if e.get("type") == "way" and "geometry" in e:
            out.append([(p["lat"], p["lon"]) for p in e["geometry"]])
        elif e.get("type") == "relation":
            # anneaux extérieurs : ways membres raboutées bout à bout
            segs = [[(p["lat"], p["lon"]) for p in m["geometry"]] for m in e.get("members", [])
                    if m.get("role") == "outer" and "geometry" in m and m["geometry"]]
            while segs:
                ring = segs.pop(0)
                grown = True
                while grown and ring[0] != ring[-1]:
                    grown = False
                    for j, sg in enumerate(segs):
                        if sg[0] == ring[-1]:
                            ring += sg[1:]
                        elif sg[-1] == ring[-1]:
                            ring += sg[::-1][1:]
                        elif sg[-1] == ring[0]:
                            ring = sg[:-1] + ring
                        elif sg[0] == ring[0]:
                            ring = sg[::-1][:-1] + ring
                        else:
                            continue
                        segs.pop(j); grown = True; break
                out.append(ring)
    return out


# --- tuiles de la campagne (cache du banc, Shortbread z14) ---
TILE_NS = "versatiles-shortbread-z14-d2e31ed09602913b"
TILE_CACHE = os.path.expanduser(f"~/.cache/osm-route-details/bench/{TILE_NS}/14")
MOTORISEES = {"motorway", "trunk", "primary", "secondary", "tertiary", "unclassified", "residential", "living_street", "service"}
_tiles = {}


def _varint(b, i):
    r = s = 0
    while True:
        x = b[i]; i += 1; r |= (x & 0x7F) << s; s += 7
        if x < 0x80:
            return r, i


def _fields(b):
    i = 0
    while i < len(b):
        k, i = _varint(b, i); f, t = k >> 3, k & 7
        if t == 0:
            v, i = _varint(b, i)
        elif t == 2:
            n, i = _varint(b, i); v = b[i:i + n]; i += n
        elif t == 5:
            v = b[i:i + 4]; i += 4
        else:
            v = b[i:i + 8]; i += 8
        yield f, t, v


def _packed(b):
    out = []; i = 0
    while i < len(b):
        v, i = _varint(b, i); out.append(v)
    return out


def _value(b):
    import struct
    for f, t, v in _fields(b):
        if f == 1: return v.decode()
        if f == 2: return struct.unpack("<f", v)[0]
        if f == 3: return struct.unpack("<d", v)[0]
        if f in (4, 5): return v
        if f == 6: return (v >> 1) ^ -(v & 1)
        if f == 7: return bool(v)


def tile(x, y):
    """Entités `streets` (tags, lignes lat/lon) et polygones `bridges` d'une tuile ; None si absente du cache."""
    if (x, y) in _tiles:
        return _tiles[(x, y)]
    f = f"{TILE_CACHE}/{x}/{y}.tile"
    res = None
    if os.path.exists(f):
        import struct
        d = open(f, "rb").read()
        n = struct.unpack("<I", d[5:9])[0]
        body = d[9 + n:]
        res = {"streets": [], "bridges": []}
        N = 2 ** 14
        for fl, _, v in _fields(body):
            if fl != 3:
                continue
            name, keys, vals, feats, ext = None, [], [], [], 4096
            for g, _, w in _fields(v):
                if g == 1: name = w.decode()
                elif g == 3: keys.append(w.decode())
                elif g == 4: vals.append(_value(w))
                elif g == 2: feats.append(w)
                elif g == 5: ext = w
            if name not in ("streets", "bridges"):
                continue
            for fb in feats:
                tags = {}; cmds = []
                for k, _, w in _fields(fb):
                    if k == 2:
                        a = _packed(w); tags.update({keys[a[i]]: vals[a[i + 1]] for i in range(0, len(a), 2)})
                    elif k == 4:
                        cmds = _packed(w)
                lines = []; cur = []; i = 0; cx = cy = 0
                while i < len(cmds):
                    c = cmds[i]; i += 1; cid, cnt = c & 7, c >> 3
                    if cid == 7:
                        if cur: cur.append(cur[0])
                        continue
                    for _ in range(cnt):
                        dx, dy = cmds[i], cmds[i + 1]; i += 2
                        cx += (dx >> 1) ^ -(dx & 1); cy += (dy >> 1) ^ -(dy & 1)
                        if cid == 1 and cur: lines.append(cur); cur = []
                        cur.append((math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * (y + cy / ext) / N)))), (x + cx / ext) / N * 360 - 180))
                if cur: lines.append(cur)
                res[name].append((tags, lines))
    _tiles[(x, y)] = res
    return res


def tiles_near(p):
    n = 2 ** 14
    x = int((p[1] + 180) / 360 * n); y = int((1 - math.asinh(math.tan(math.radians(p[0]))) / math.pi) / 2 * n)
    out = {"streets": [], "bridges": [], "manquantes": 0}
    for dx in (-1, 0, 1):
        for dy in (-1, 0, 1):
            t = tile(x + dx, y + dy)
            if t is None:
                out["manquantes"] += 1; continue
            out["streets"] += t["streets"]; out["bridges"] += t["bridges"]
    return out


def ferme_motorise(tags):
    """Route motorisée fermée au vélo (§4 ter.3 resserrée) : motorway, trunk, ou route motorisée bicycle=no."""
    b = tags.get("bicycle")
    if b in OUVERT_VELO:
        return False
    k = tags.get("kind")
    return k in ("motorway", "trunk") or (k in MOTORISEES and b == "no")


def hors_graphe_tuile(tags):
    """Règle de tags du service (T03 §4.2 étape 1) sur les attributs des tuiles."""
    b = tags.get("bicycle")
    if b in OUVERT_VELO:
        return False
    if b == "no":
        return True
    return tags.get("kind") in ("motorway", "trunk", "pedestrian", "bridleway", "busway")


def in_poly(p, ring):
    c = False
    for i in range(len(ring)):
        a, b = ring[i], ring[i - 1]
        if (a[1] > p[1]) != (b[1] > p[1]) and p[0] < (b[0] - a[0]) * (p[1] - a[1]) / ((b[1] - a[1]) or 1e-12) + a[0]:
            c = not c
    return c


def trace_list(B):
    """Traces du contrôle : sorties fiables, négatifs Q1 (sans extraction), itinéraires planifiés."""
    qual = {r["id"]: r for r in csv.DictReader(open(f"{B}/strava/qualif_{SERIE}.csv"))}
    if SERIE == "v1":
        for f in ("qualif_q1.csv",):
            if os.path.exists(f"{B}/strava/{f}"):
                qual.update({r["id"]: dict(r, q1=True) for r in csv.DictReader(open(f"{B}/strava/{f}"))})
    else:
        # négatifs Q1 du tirage figé (tools/tirage_v11.py), déjà qualifiés
        q1 = set(json.load(open(f"{B}/{SERIE}/tirage.json"))["q1"])
        for f in sorted(glob.glob(f"{B}/strava/qualif*.csv")):
            qual.update({r["id"]: dict(r, q1=True) for r in csv.DictReader(open(f)) if r["id"] in q1})
    sel = {i: (z, k) for i, z, k in json.load(open(f"{B}/{SERIE}/selection.json"))}
    inv = {str(a["id"]): a for a in json.load(open(f"{B}/strava/inventaire.json"))}
    out = []
    for i in sorted(qual):
        r = qual[i]
        if r.get("q1"):
            # négatif Q1 : défaut attesté hors qualificateur, scan et stream (930f6e8, 305abd7) :
            # D+ enregistré < 60 % du D+ MNT (baro KO) ou > 1,6 × (altitude GPS en excès)
            g = float(r["gain_rec_m"] or 0); d = float(r["gain_dem_m"] or 0)
            neg = d > 0 and (g < 0.6 * d or g > 1.6 * d)
            out.append({"id": i, "zone": "q1", "verdict": r["verdict"], "km": float(r["dist_km"] or sel[i][1]),
                        "gain_dem_m": float(r["gain_dem_m"] or 0), "role": "Q1" if neg else "hors_serie"})
            continue
        out.append({"id": i, "zone": sel[i][0], "verdict": r["verdict"], "km": float(r["dist_km"] or sel[i][1]),
                    "gain_dem_m": float(r["gain_dem_m"] or 0), "role": "trace" if r["verdict"] == "fiable" else "hors_serie"})
    res = f"{B}/{SERIE}/planifies/resume.json"
    if os.path.exists(res):
        for i, r in sorted(json.load(open(res)).items()):
            out.append({"id": i, "zone": "planifie", "verdict": "planifie", "km": r["km"], "gain_dem_m": 0.0,
                        "role": r["role"], "nature": r.get("nature"), "part_non_revetu": r["part_non_revetu"]})
    return out


def trace_points(B, t):
    if t["zone"] == "planifie":
        return gpx_points(f"{B}/{SERIE}/planifies/{t['id']}.gpx")
    return [tuple(p) for p in json.load(open(f"{B}/strava/streams-{SERIE}/{t['id']}.json"))["location"] if p]


def cas(B):
    refs, cg = calibration_refs(B)
    out = {"traces": trace_list(B), "ouvrages": {}, "croisements": {}, "temoins": [], "tuiles_manquantes": 0}
    for tr in out["traces"]:
        if tr["role"] in ("hors_serie", "Q1", "R1_R2"):
            continue
        i = tr["id"]
        raw = trace_points(B, tr)
        plan = Plan(raw[0][0])
        rxy = [plan.xy(p) for p in raw]
        dec = [rxy[0]]
        for p in rxy[1:]:
            d = math.hypot(p[0] - dec[-1][0], p[1] - dec[-1][1])
            if d >= STEP_M:
                # points planifiés espacés : densifier
                n = int(d // STEP_M); q0 = dec[-1]
                for k in range(1, n):
                    dec.append((q0[0] + (p[0] - q0[0]) * k / n, q0[1] + (p[1] - q0[1]) * k / n))
                dec.append(p)
        pas = [math.hypot(b[0] - a[0], b[1] - a[1]) for a, b in zip(dec, dec[1:])] + [0.0]   # longueur portée par chaque point décimé
        tg = Grid(); tg.add("t", dec)
        E = load_overpass(B, i)
        ways = {}; others = []
        for e in E:
            t = e.get("tags", {}); g = [(p["lat"], p["lon"]) for p in e["geometry"]]
            k = kind_of(t)
            if k and "highway" in t:
                ways[e["id"]] = (k, t, g)
            others.append((e["id"], t, g))
        allstruct = Grid(); sxy = {}
        for wid, t, g in others:
            if kind_of(t):
                sxy[wid] = [plan.xy(p) for p in g]; allstruct.add(wid, sxy[wid])
        passages = []
        for k, ws, poly in chains(ways):
            pxy = [plan.xy(p) for p in poly]
            L = length(pxy)
            if L < MIN_LEN_M:
                continue
            pts = {"A": pxy[0], "milieu": at(pxy, 0.5), "B": pxy[-1]}
            if not all(track_dist(p, dec, tg) < TOL_M for p in pts.values()):
                continue
            t0 = ways[ws[0]][1]
            excl = []
            mid_ll = (pts["milieu"][1] / plan.ky, pts["milieu"][0] / plan.kx)
            ll = {"A": poly[0], "milieu": mid_ll, "B": poly[-1]}
            if any(hav(p, q) < MASK_M for p in ll.values() for q in refs) or seen_by_track(cg, list(ll.values())):
                excl.append("deja_vu_calibration")
            if k == "tunnel" and any(ways[w][1].get("tunnel") == "avalanche_protector" for w in ws):
                excl.append("limite_donnees")
            # tuiles : voie de l'ouvrage (même nature, la plus proche du milieu), hors graphe, absence
            T = tiles_near(mid_ll)
            out["tuiles_manquantes"] += T["manquantes"]
            feats = [(tg2, [[plan.xy(q) for q in l] for l in lines]) for tg2, lines in T["streets"] if tg2.get(k)]
            def fdist(p, f):
                return min((poly_dist(p, l) for l in f[1] if len(l) > 1), default=1e9)
            own_f = [f for f in feats if fdist(pts["milieu"], f) <= 10.0]
            if not own_f:
                excl.append("absent_des_donnees")
            else:
                best = min(own_f, key=lambda f: fdist(pts["milieu"], f))
                if hors_graphe_tuile(best[0]):
                    excl.append("hors_graphe_velo")
            # attribution (§4 ter.2) avec « un tablier, un ouvrage » (ponts)
            mx, my = pts["milieu"]
            src = dec if tr["zone"] == "planifie" else rxy   # itinéraire planifié : géométrie densifiée
            emp = [p for p in src if math.hypot(p[0] - mx, p[1] - my) <= L / 2 + 5 and poly_dist(p, pxy) < TOL_M]
            own = None
            if emp and own_f:
                polys = [[plan.xy(q) for q in l] for _, lines in T["bridges"] for l in lines]
                samp = [at(pxy, s / 20) for s in range(21)]
                def nearest_pt(f, p):
                    best = (1e9, None)
                    for l in f[1]:
                        for j in range(len(l) - 1):
                            a, b = l[j], l[j + 1]
                            dx, dy = b[0] - a[0], b[1] - a[1]; LL = dx * dx + dy * dy
                            tt = 0.0 if LL == 0 else max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / LL))
                            q = (a[0] + tt * dx, a[1] + tt * dy)
                            best = min(best, (math.hypot(p[0] - q[0], p[1] - q[1]), q))
                    return best[1]
                def covered(q1, q2):
                    return all(any(in_poly((q1[0] + (q2[0] - q1[0]) * s / 6, q1[1] + (q2[1] - q1[1]) * s / 6), r) for r in polys) for s in range(7))
                def in_any(q):
                    return any(in_poly(q, r) for r in polys)
                ref_f = min(own_f, key=lambda f: fdist(pts["milieu"], f))
                q0 = nearest_pt(ref_f, pts["milieu"])
                same = list(own_f); distinct = []
                for f in feats:
                    if f in own_f or fdist(pts["milieu"], f) > 60.0:
                        continue
                    q1 = nearest_pt(f, pts["milieu"])
                    if k == "bridge":
                        if in_any(q0) and in_any(q1):
                            (same if covered(q0, q1) else distinct).append(f)
                            continue
                        if max(fdist(s, f) for s in samp) <= 10.0:
                            same.append(f); continue
                    distinct.append(f)
                own = min(statistics.median(fdist(p, f) for p in emp) for f in same)
                if any(statistics.median(fdist(p, f) for p in emp) < own for f in distinct):
                    excl.append("passage_sur_ouvrage_parallele")
            oid = f"w{min(ws)}"
            perm = {"deja_vu_calibration", "hors_graphe_velo", "limite_donnees", "absent_des_donnees"}
            o = out["ouvrages"].setdefault(oid, {"id": oid, "type": k, "ways": ws, "length_m": round(L, 1),
                                                 "pts": {"A": poly[0], "milieu": mid_ll, "B": poly[-1]},
                                                 "tags": {x: t0.get(x) for x in ("highway", "bicycle", "bridge", "tunnel") if x in t0},
                                                 "passages": [], "exclusions": []})
            o["passages"].append({"trace": i, "planifie": tr["zone"] == "planifie", "fiable": tr["verdict"] == "fiable",
                                  "mediane_brute_m": None if own is None else round(own, 2),
                                  "exclusions": sorted(set(excl) - perm)})
            o["exclusions"] = sorted(set(o["exclusions"]) | (set(excl) & perm))
            passages.append((oid, pxy))
        # E5 : la trace croise la géométrie d'un pont qu'elle ne franchit pas
        own_ways = {w for oid, _ in passages for w in out["ouvrages"][oid]["ways"]}
        xs = []
        for wid, t, g in others:
            if kind_of(t) != "bridge" or wid in own_ways:
                continue
            bxy = sxy[wid]; hit = None
            for j in range(len(bxy) - 1):
                for _, ti in tg.near(bxy[j], 100.0 + math.hypot(bxy[j + 1][0] - bxy[j][0], bxy[j + 1][1] - bxy[j][1])):
                    x = inter(bxy[j], bxy[j + 1], dec[ti], dec[ti + 1])
                    if x and x[1] >= 20:
                        hit = x[0]; break
                if hit:
                    break
            if not hit or any(poly_dist(hit, pxy) < 20.0 for _, pxy in passages):
                continue
            hl = (hit[1] / plan.ky, hit[0] / plan.kx)
            excl = ["deja_vu_calibration"] if any(hav(hl, q) < MASK_M for q in refs) else []
            cid = f"x{wid}"
            c = out["croisements"].setdefault(cid, {"id": cid, "way": wid, "pt": hl, "rail": "railway" in t, "traces": [], "exclusions": excl})
            c["traces"].append(i); xs.append(hit)
        # S2 : stations libres d'ouvrage ; plages hors graphe (§4 ter.5) retirées ; nature par OSM
        nat = Grid(); nkind = {}
        for wid, t, g in others:
            n = None
            if t.get("man_made") == "dyke":
                n = "digue"
            elif t.get("natural") == "cliff":
                n = "bati_ou_falaise"
            elif t.get("embankment") == "yes" or t.get("cutting") == "yes" or t.get("man_made") == "embankment":
                n = "remblai_deblai"
            if n:
                nkind[wid] = (n, [plan.xy(p) for p in g]); nat.add(wid, nkind[wid][1])
        forets = [[plan.xy(q) for q in r] for r in load_forets(B, i)]
        fg = Grid()
        for a, r in enumerate(forets):
            fg.add(a, r)
        RAD = {"digue": 30.0, "bati_ou_falaise": 30.0, "remblai_deblai": 15.0}
        # plages hors graphe : voie streets non ferrée la plus proche (≤ 30 m) route motorisée fermée au vélo
        ferme = []
        for p in dec[::5]:
            ll = (p[1] / plan.ky, p[0] / plan.kx)
            T = tiles_near(ll)
            best = (31.0, None)
            for tg2, lines in T["streets"]:
                if tg2.get("rail") or tg2.get("kind") in ("rail", "tram", "light_rail", "subway", "narrow_gauge", "funicular", "monorail"):
                    continue
                for l in lines:
                    if len(l) < 2:
                        continue
                    dd = poly_dist(p, [plan.xy(q) for q in l])
                    if dd < best[0]:
                        best = (dd, tg2)
            ferme += [best[1] is not None and ferme_motorise(best[1])] * 5
        ferme = ferme[:len(dec)] + [False] * max(0, len(dec) - len(ferme))
        # plages continues ≥ 50 m seulement
        k0 = None
        for idx in range(len(ferme) + 1):
            on = idx < len(ferme) and ferme[idx]
            if on and k0 is None:
                k0 = idx
            if not on and k0 is not None:
                if (idx - k0) * STEP_M < 50.0:
                    for j in range(k0, idx):
                        ferme[j] = False
                k0 = None
        lab = []
        for idx, p in enumerate(dec):
            if any(seg_dist(p, sxy[w][j], sxy[w][j + 1]) < TEMOIN_LIBRE_M for w, j in allstruct.near(p, TEMOIN_LIBRE_M)) \
               or any(math.hypot(p[0] - x[0], p[1] - x[1]) < TEMOIN_LIBRE_M for x in xs) or ferme[idx]:
                lab.append(None); continue
            n = None
            for w, j in nat.near(p, 30.0):
                k2, g2 = nkind[w]
                if seg_dist(p, g2[j], g2[j + 1]) < RAD[k2]:
                    n = k2 if n is None or k2 == "digue" else n
            if n is None and forets and (any(seg_dist(p, forets[a][j], forets[a][j + 1]) <= 20.0 for a, j in fg.near(p, 20.0))
                                         or any(in_poly(p, forets[a]) for a in {a for a, _ in fg.near(p, 1500.0)})):
                n = "couvert_arbore"   # dans une forêt OSM ou à 20 m au plus de son bord (parcelles bordées par la route)
            lab.append(n)
        if tr["zone"] == "planifie" and tr["role"] == "S2":
            # témoin planifié : un cas, nature déclarée, longueur hors abords d'ouvrage et hors plages fermées
            libres = [idx for idx, p in enumerate(dec) if not ferme[idx] and not (
                any(seg_dist(p, sxy[w][j], sxy[w][j + 1]) < TEMOIN_LIBRE_M for w, j in allstruct.near(p, TEMOIN_LIBRE_M))
                or any(math.hypot(p[0] - x[0], p[1] - x[1]) < TEMOIN_LIBRE_M for x in xs))]
            # nature (§4 ter.4, 00e500f) : déclarée si OSM la confirme sur ≥ 50 % de la longueur comptée,
            # sinon celle que les tags confirment à 50 %, sinon retiré
            RCONF = {"bati_ou_falaise": 100.0, "digue": 30.0, "remblai_deblai": 30.0}
            def conf_of(n):
                if n == "couvert_arbore":
                    return sum(1 for idx in libres if lab[idx] == "couvert_arbore" or (forets and (
                        any(seg_dist(dec[idx], forets[a][j], forets[a][j + 1]) <= 20.0 for a, j in fg.near(dec[idx], 20.0))
                        or any(in_poly(dec[idx], forets[a]) for a in {a for a, _ in fg.near(dec[idx], 1500.0)}))))
                gs = [g for w, (k2, g) in nkind.items() if k2 == n and len(g) > 1]
                return sum(1 for idx in libres if any(poly_dist(dec[idx], g) <= RCONF[n] for g in gs))
            parts = {n: round(conf_of(n) / max(1, len(libres)), 2) for n in ("couvert_arbore", "bati_ou_falaise", "digue", "remblai_deblai")}
            declaree = tr.get("nature")
            if parts.get(declaree, 0) >= 0.5:
                nature = declaree
            else:
                best = max(parts, key=parts.get)
                nature = best if parts[best] >= 0.5 else None
            out["temoins"].append({"id": i, "trace": i, "planifie": True, "nature": nature, "nature_declaree": declaree,
                                   "length_m": round(sum(pas[idx] for idx in libres), 1), "parts_osm": parts})
            tr["hors_graphe_velo_m"] = round(sum(ferme) * STEP_M)
            print(i, "passages", len(passages), "croisements", len(xs), file=sys.stderr, flush=True)
            continue
        run = None; k = 0
        for idx, n in enumerate(lab + [None]):
            if run and n == run[0]:
                run[2] = idx; continue
            if run:
                # seuil de construction de la série figée (ee3cb76) : STEP_M par point décimé ; la
                # longueur écrite est celle du tronçon, de debut à fin (décision de l'utilisateur,
                # 2026-11-02 : les points décimés sont espacés de STEP_M à 2 × STEP_M)
                Lr = round(sum(pas[j] for j in range(run[1], run[2])), 1)
                if STEP_M * (run[2] - run[1] + 1) < TEMOIN_MIN_M <= Lr:
                    print(i, "témoin éligible à sa longueur, hors série figée :", run[0], Lr, file=sys.stderr, flush=True)
                if STEP_M * (run[2] - run[1] + 1) >= TEMOIN_MIN_M:
                    ends = [(dec[j][1] / plan.ky, dec[j][0] / plan.kx) for j in (run[1], run[2])]
                    out["temoins"].append({"id": f"{i}-s{k}", "trace": i, "planifie": tr["zone"] == "planifie",
                                           "nature": run[0], "length_m": Lr, "debut": ends[0], "fin": ends[1]})
                    k += 1
                run = None
            if n:
                run = [n, idx, idx]
        tr["hors_graphe_velo_m"] = round(sum(ferme) * STEP_M)
        print(i, "passages", len(passages), "croisements", len(xs), file=sys.stderr, flush=True)
    json.dump(out, open(f"{B}/{SERIE}/couverture-cas.json", "w"))


# --- étape 2 : IGN ---
def ign(points, lidar):
    res = "ign_lidar_hd_mnx_multi_wld" if lidar else "ign_rge_alti_wld"
    q = urllib.parse.urlencode({"lon": "|".join(f"{p[1]:.7f}" for p in points), "lat": "|".join(f"{p[0]:.7f}" for p in points),
                                "resource": res, "zonly": "false" if lidar else "true", **({"measures": "true"} if lidar else {})})
    for attempt in range(5):
        try:
            with urllib.request.urlopen(urllib.request.Request(f"{ALTI}?{q}", headers={"User-Agent": UA}), timeout=90) as r:
                d = json.load(r)
            time.sleep(0.4)
            if not lidar:
                return [z if z > -9000 else None for z in d["elevations"]]
            out = []
            for e in d["elevations"]:
                m = {}
                for x in e.get("measures", []):
                    if x.get("title", "").count(" - ") >= 2 and x["z"] > -9000:
                        m.setdefault(x["title"].split(" - ")[2].strip(), x["z"])
                out.append(m)
            return out
        except Exception:  # noqa: BLE001
            time.sleep(10 * (attempt + 1))
    raise RuntimeError("IGN indisponible")


def batched(points, lidar, n=40):
    out = []
    for k in range(0, len(points), n):
        out += ign(points[k:k + n], lidar)
    return out


def ign_step(B):
    C = json.load(open(f"{B}/{SERIE}/couverture-cas.json"))
    cache_f = f"{B}/{SERIE}/couverture-ign.json"
    cache = json.load(open(cache_f)) if os.path.exists(cache_f) else {}
    todo = [o for o in C["ouvrages"].values() if o["type"] == "bridge" and o["id"] not in cache and not o["exclusions"]]
    for o in todo:
        a, b = o["pts"]["A"], o["pts"]["B"]; m = o["pts"]["milieu"]
        d = hav(a, b) or 1.0
        ux, uy = (b[0] - a[0]) / d, (b[1] - a[1]) / d
        line = [(m[0] + ux * s, m[1] + uy * s) for s in [-12.5 + 2.5 * k for k in range(11)]]
        v = batched(line, True)
        mns = [x["MNS"] for x in v if "MNS" in x]
        mnt = v[5].get("MNT")
        o_ign = {"mnh_m": round(statistics.median(mns) - mnt, 2) if mns and mnt is not None else None, "mnt_milieu": mnt}
        cache[o["id"]] = o_ign
        json.dump(cache, open(cache_f, "w"))
    # contrôle de niveau des ponts à MNH ≥ 10 m, par passage
    lev_f = f"{B}/{SERIE}/couverture-niveau.json"
    lev = json.load(open(lev_f)) if os.path.exists(lev_f) else {}
    for o in C["ouvrages"].values():
        g = cache.get(o["id"])
        if not g or g["mnh_m"] is None or g["mnh_m"] < MNH_NIVEAU_M:
            continue
        for p in o["passages"]:
            key = f'{o["id"]}:{p["trace"]}'
            if key in lev or p["exclusions"] or not p.get("fiable"):   # niveau : passages baro seulement
                continue
            s = json.load(open(f'{B}/strava/streams-{SERIE}/{p["trace"]}.json'))
            L = [(tuple(x), z, dd) for x, z, dd in zip(s["location"], s["altitude"], s["distance"]) if x]
            m = o["pts"]["milieu"]
            k0 = min(range(len(L)), key=lambda k: hav(L[k][0], m))
            s0 = L[k0][2]
            near_mid = [z for x, z, dd in L if abs(dd - s0) <= 12.5]
            ring = [(x, z) for x, z, dd in L if 200 <= abs(dd - s0) <= 1500]
            ring = ring[::max(1, len(ring) // 60)]
            mnt_ring = batched([x for x, _ in ring], False)
            res = [z - t for (x, z), t in zip(ring, mnt_ring) if t is not None]
            if len(near_mid) < 3 or len(res) < 10 or g["mnt_milieu"] is None:
                lev[key] = {"etat": "niveau_sans_baro"}
            else:
                off = statistics.median(res)
                mes = statistics.median(near_mid) - off - g["mnt_milieu"]
                lev[key] = {"etat": "retire" if mes < 0.5 * g["mnh_m"] else "garde", "mesure_m": round(mes, 2)}
            json.dump(lev, open(lev_f, "w"))
    print("ponts lus", len(cache), "contrôles de niveau", len(lev))


# --- étape 3 : tirage et comptes ---
def h(x):
    return hashlib.sha256((SEL + x).encode()).hexdigest()


def cible(minimum):
    """Remplissage d'une strate (§4 ter.4, 64b7067) : minimum majoré de 25 %, arrondi au supérieur."""
    return math.ceil(minimum * 1.25)


def tirage(B):
    C = json.load(open(f"{B}/{SERIE}/couverture-cas.json"))
    G = json.load(open(f"{B}/{SERIE}/couverture-ign.json"))
    LV = json.load(open(f"{B}/{SERIE}/couverture-niveau.json")) if os.path.exists(f"{B}/{SERIE}/couverture-niveau.json") else {}
    fiables = [t for t in C["traces"] if t["role"] == "trace"]
    planifies = {t["id"]: t for t in C["traces"] if t["zone"] == "planifie"}
    # ouvrages comptés
    compte = {}
    raisons = {}
    for o in C["ouvrages"].values():
        r = list(o["exclusions"])
        if o["type"] == "bridge":
            g = G.get(o["id"])
            if not r and (g is None or g["mnh_m"] is None):
                r.append("hors_lidar")       # gardé hors MNH connu ? non : règle du corpus MVP
            elif not r and g["mnh_m"] < MIN_MNH_M:
                r.append("pas_un_tablier")
        ok = [p for p in o["passages"] if not p["exclusions"] and LV.get(f'{o["id"]}:{p["trace"]}', {}).get("etat") != "retire"]
        if not r and not ok:
            r.append("passage_sur_ouvrage_parallele" if any(p["exclusions"] for p in o["passages"]) else "passage_retire_niveau")
        for x in r:
            raisons[(o["type"], x)] = raisons.get((o["type"], x), 0) + 1
        if not r:
            compte[o["id"]] = {"type": o["type"], "L": o["length_m"], "mnh": (G.get(o["id"]) or {}).get("mnh_m"),
                               "pts": o["pts"], "traces": sorted({p["trace"] for p in ok}),
                               "baro": any(p.get("fiable") for p in ok)}   # E3 : référence baro
    xs = {c["id"]: c for c in C["croisements"].values() if not c["exclusions"]}
    # groupes : ouvrages et croisements à moins de NEAR_M
    uf = UF(); items = {k: [tuple(p) for p in v["pts"].values()] for k, v in compte.items()}
    items.update({k: [tuple(v["pt"])] for k, v in xs.items()})
    keys = sorted(items)
    for a_i, a in enumerate(keys):
        for b in keys[a_i + 1:]:
            if any(hav(p, q) < NEAR_M for p in items[a] for q in items[b]):
                uf.union(a, b)
    groupe = {k: uf.find(k) for k in keys}
    gid = {}
    for k in keys:
        gid.setdefault(groupe[k], []).append(k)
    gname = {g: min(m) for g, m in gid.items()}
    # membres d'un cas, par son nom (plus petit id du groupe) : la racine de l'union-find n'est pas ce nom.
    # V1 garde le défaut : son tirage figé (comptes sous-estimés, donc plus de cas tenus à l'écart) se reproduit.
    if SERIE != "v1":
        gid = {gname[g]: m for g, m in gid.items()}
    serie = {}

    def assign(stratum, cases, measure, enough):
        """cases : liste d'id de cas ; measure(ids en tenue) -> dict ; enough(dict) -> bool."""
        cases = sorted(set(cases), key=h)
        n = len(cases); held = [c for c in cases if serie.get(c) == "tenue_a_l_ecart"]
        for c in cases:
            if c in serie:
                continue
            if SERIE != "v1" or not enough(measure(held)) or len(held) < math.ceil(n / 3):
                serie[c] = "tenue_a_l_ecart"; held.append(c)
            else:
                serie[c] = "calibration"
        return measure(held), n

    def members(case_ids):
        return [k for c in case_ids for k in gid.get(c, [])]

    res = {}
    tun_cases = sorted({gname[groupe[k]] for k, v in compte.items() if v["type"] == "tunnel"})
    mE4 = lambda held: {"tunnels": sum(1 for k in members(held) if k in compte and compte[k]["type"] == "tunnel"),
                        "plus_de_300m": sum(1 for k in members(held) if k in compte and compte[k]["type"] == "tunnel" and compte[k]["L"] > LONG_TUNNEL_M)}
    res["E4"] = assign("E4", tun_cases, mE4, lambda m: m["tunnels"] >= cible(7) and m["plus_de_300m"] >= cible(2))
    x_cases = sorted({gname[groupe[k]] for k in xs})
    if SERIE in FIGES:
        mE5 = lambda held: {"croisements": sum(1 for k in members(held) if k in xs)}
        res["E5"] = assign("E5", x_cases, mE5, lambda m: m["croisements"] >= cible(8))
    else:
        # un site physique = un cas (§4 quater.4) : croisements d'une même trace à SITE_M au plus, de proche en proche
        us = UF(); xk = sorted(xs)
        for a_i, a in enumerate(xk):
            for b in xk[a_i + 1:]:
                if set(xs[a]["traces"]) & set(xs[b]["traces"]) and hav(xs[a]["pt"], xs[b]["pt"]) <= SITE_M:
                    us.union(a, b)
        site = {k: us.find(k) for k in xk}
        mE5 = lambda held: {"sites": len({site[k] for k in members(held) if k in xs}),
                            "croisements": sum(1 for k in members(held) if k in xs)}
        res["E5"] = assign("E5", x_cases, mE5, lambda m: m["sites"] >= cible(8))
    tem = [t for t in C["temoins"] if t["nature"]]   # témoin planifié sans nature confirmée : retiré
    temd = {t["id"]: t for t in tem}
    mS2 = lambda held: {"temoins": len(held), "km": round(sum(temd[c]["length_m"] for c in held) / 1000, 1),
                        "natures": len({temd[c]["nature"] for c in held})}
    res["S2"] = assign("S2", [t["id"] for t in tem], mS2, lambda m: m["temoins"] >= cible(4) and m["km"] >= cible(40) and m["natures"] >= cible(3))
    rr = {i: t for i, t in planifies.items() if t["role"] == "R1_R2"}
    mR12 = lambda held: {"parcours": len(held), "km": round(sum(rr[c]["km"] for c in held)),
                         "part_non_revetu": round(sum(rr[c]["km"] * rr[c]["part_non_revetu"] for c in held) / max(1e-9, sum(rr[c]["km"] for c in held)), 3)}
    res["R1_R2"] = assign("R1_R2", list(rr), mR12, lambda m: m["parcours"] >= cible(4) and m["km"] >= cible(400) and m["part_non_revetu"] >= 0.125)
    br_cases = sorted({gname[groupe[k]] for k, v in compte.items() if v["type"] == "bridge" and v["baro"]})
    e3 = lambda k: k in compte and compte[k]["type"] == "bridge" and compte[k]["baro"]
    mE3 = lambda held: {"ponts": sum(1 for k in members(held) if e3(k)),
                        "mnh_10m": sum(1 for k in members(held) if e3(k) and (compte[k]["mnh"] or 0) >= MNH_NIVEAU_M),
                        "plus_de_150m": sum(1 for k in members(held) if e3(k) and compte[k]["L"] > LONG_PONT_M)}
    res["E3"] = assign("E3", br_cases, mE3, lambda m: m["ponts"] >= cible(12) and m["mnh_10m"] >= cible(3) and m["plus_de_150m"] >= cible(3))
    q1 = [t["id"] for t in C["traces"] if t["role"] == "Q1"]
    res["Q1"] = assign("Q1", q1, lambda held: {"traces": len(held)}, lambda m: m["traces"] >= cible(2))
    # traces : groupe E2b provisoire (D+ MNT au km et ouvrages comptés)
    nb_ouv = {}
    for k, v in compte.items():
        for t in v["traces"]:
            nb_ouv[t] = nb_ouv.get(t, 0) + 1
    grp = {}
    for t in fiables:
        dpk = t["gain_dem_m"] / t["km"]
        grp[t["id"]] = "dplus" if dpk >= 11 else ("ouvrage" if nb_ouv.get(t["id"], 0) >= 2 else "plaine")
    td = {t["id"]: t for t in fiables}
    mR = lambda held: {"traces": len(held), "km": round(sum(td[c]["km"] for c in held)),
                       **{g: sum(1 for c in held if grp[c] == g) for g in ("dplus", "plaine", "ouvrage")}}
    res["reste"] = assign("reste", [t["id"] for t in fiables], mR,
                          lambda m: m["traces"] >= cible(8) and m["km"] >= cible(800) and all(m[g] >= cible(3) for g in ("dplus", "plaine", "ouvrage")))
    # S1 sur l'ensemble des ouvrages tenus à l'écart (toutes strates)
    held_ouv = [k for c, s in serie.items() if s == "tenue_a_l_ecart" for k in gid.get(c, []) if k in compte]
    if SERIE != "v1":   # tout est tenu à l'écart, ponts sans baro des itinéraires planifiés compris
        held_ouv = list(compte)
    S1 = {"ouvrages": len(held_ouv), "emprise_m": round(sum(compte[k]["L"] for k in held_ouv))}
    # affectation de chaque élément (ouvrage, croisement : série de son groupe ; témoin, trace, itinéraire) ;
    # fichier à part, tirage.json inchangé
    hors = "calibration" if SERIE == "v1" else "tenue_a_l_ecart"   # élément hors strate (pont sans baro d'un itinéraire)
    aff = {"ouvrages": {k: serie.get(gname[groupe[k]], hors) for k in compte},
           "croisements": {k: serie.get(gname[groupe[k]], hors) for k in xs},
           "temoins": {t["id"]: serie.get(t["id"], hors) for t in tem},
           "traces": {t["id"]: serie.get(t["id"], hors) for t in C["traces"] if t["role"] in ("trace", "Q1")},
           "groupes_ouvrages": {k: gname[groupe[k]] for k in keys},
           "groupes_traces": grp}
    json.dump(aff, open(f"{B}/{SERIE}/affectation.json", "w"), sort_keys=True)
    tir = {"sel": SEL, "serie": serie, "groupes_traces": grp}
    # V1.1 : le tirage figé (tools/tirage_v11.py) n'est jamais réécrit ; tout est tenu à l'écart
    tir_f = f"{B}/{SERIE}/tirage.json" if SERIE == "v1" else f"{B}/{SERIE}/affectation-tirage.json"
    json.dump(tir, open(tir_f, "w"), sort_keys=True)
    sha = hashlib.sha256(open(tir_f, "rb").read()).hexdigest()
    held_tr = [c for c in td if serie.get(c) == "tenue_a_l_ecart"]
    held_cases = {c for c, x in serie.items() if x == "tenue_a_l_ecart"}
    pl_held = [p for p in planifies if p in held_cases or any(t["trace"] == p and t["id"] in held_cases for t in tem)
               or any(p in compte[k]["traces"] for c in held_cases for k in gid.get(c, []) if k in compte)]
    summary = {
        "traces": {"telechargees": len(C["traces"]), "fiables": len(fiables),
                   "verdicts": {v: sum(1 for t in C["traces"] if t["verdict"] == v) for v in sorted({t["verdict"] for t in C["traces"]})},
                   "km_fiables": round(sum(t["km"] for t in fiables))},
        "candidats": {"ouvrages_comptes": {"bridge": sum(1 for v in compte.values() if v["type"] == "bridge"),
                                           "tunnel": sum(1 for v in compte.values() if v["type"] == "tunnel")},
                      "exclusions": {f"{a}:{b}": n for (a, b), n in sorted(raisons.items())},
                      "croisements_comptes": len(xs), "croisements_exclus": len(C["croisements"]) - len(xs),
                      **({} if SERIE in FIGES else {"sites_e5": len(set(site.values()))}),
                      "temoins": {n: {"nb": sum(1 for t in tem if t["nature"] == n), "km": round(sum(t["length_m"] for t in tem if t["nature"] == n) / 1000, 1)}
                                  for n in ("remblai_deblai", "couvert_arbore", "bati_ou_falaise", "digue")}},
        "tenue_a_l_ecart": {s: {"atteint": v[0], "cas_de_la_strate": v[1]} for s, v in res.items()},
        "S1": S1, "E1": {"traces": len(held_tr), "km": round(sum(td[c]["km"] for c in held_tr))},
        "M1": {"km": round(sum(td[c]["km"] for c in held_tr) + sum(planifies[p]["km"] for p in pl_held)), "traces": len(held_tr) + len(pl_held),
               "hors_graphe_velo_km_des_temoins": round(sum(t.get("hors_graphe_velo_m", 0) for t in C["traces"]) / 1000, 1)},
        "planifies_tenus_a_l_ecart": sorted(pl_held),
        "tirage_sha256": sha,
    }
    print(json.dumps(summary, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    {"cas": cas, "ign": ign_step, "tirage": tirage}[sys.argv[1]](sys.argv[2].rstrip("/"))
