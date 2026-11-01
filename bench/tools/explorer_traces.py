#!/usr/bin/env python3
"""Explorateur local des traces du banc : site HTML statique et autonome, hors git.

Entrées (relatives à bench/) : manifest.yaml (perso, perso_v1, vcyclist, osm_surface) ; traces brutes
private/traces/<id>.trim1km.gpx (à défaut <id>.gpx) ; recalages private/recales/<id>[.sN][.partM].gpx,
<id>[.sN].baro.csv et <id>[.sN].surface.csv ; ouvrages data/ouvrages-geom.json,
private/ouvrages-perso.json, private/v1/ouvrages-v1.json ; témoins S2 private/v1/temoins-v1.json ;
séries private/v1/series-v1.csv ; itinéraires planifiés private/v1/planifies/<id>.gpx ; parcours
publics data/vcyclist/ (essais et contrôles) et data/osm-surface/ (référence de revêtement
<nom>.reference.json). Ne lit pas les tables pseudonyme → Strava ; aucun appel réseau.

Série tenue à l'écart V1 (discipline de la série) : écartée avant toute lecture de fichier — traces
perso_v1 hors `calibration`, ouvrages et témoins V1 hors `calibration`, itinéraires planifiés sans
série `calibration` dans series-v1.csv (tenus, `par_cas` ou absents). Sur les traces affichées, les
fenêtres ± MASK_M autour des éléments tenus V1 cités dans `passages_masques` ou à moins de TOL_M de
la trace perso sont retirées de la carte, du profil et de la bande de revêtement. La série tenue du MVP,
déjà jugée, est affichée et étiquetée « tenue MVP (jugée) ». `--inclure-tenues-v1` lève ces deux
filtres (avertissement imprimé).

Sorties (bench/private/explorateur/, gitignoré, vidé à chaque passage) : index.html (tableau
triable et filtrable) et t/<id>.html (carte SVG brut/recalé, ouvrages A–milieu–B, témoins S2,
profil baro brut et transporté sur le recalé, bande de revêtement, métadonnées). Aucune ressource
externe. La sortie standard ne porte que des comptes et le chemin de l'index.

Usage (depuis bench/) : uv run --with pyyaml python -I tools/explorer_traces.py [--inclure-tenues-v1] [--ids a,b,…]
"""
import argparse
import csv
import glob
import html
import json
import math
import os
import re
import shutil
import sys
import time

import yaml

TOL_M = 40.0      # un ouvrage concerne une trace à moins de TOL_M de chacun de ses points
MASK_M = 500.0    # fenêtre masquée autour d'un élément tenu V1 (README §2, règle 4)
MARGE_M = 250.0   # marge ajoutée aux fenêtres masquées le long de l'abscisse recalée
EPS_M = 4.0       # Douglas–Peucker des géométries
DENS_M = 25.0     # densification du recalé pour le masquage
OUT = "private/explorateur"
TENUE = "tenue_a_l_ecart"
COUL = {"pont": "#2a9d3a", "tunnel": "#8c564b", "passage dessous": "#8e44ad"}
SURF = {"paved": "#4a4a4a", "unpaved": "#d08a2c", "inconnu": "#d9d9d9", "masqué": "#f2c6e6"}
PAVED = {"paved_smooth", "paved", "paved_rough", "PAVED", "PAVED_SMOOTH", "PAVED_ROUGH"}
UNPAVED = {"compacted", "dirt", "gravel", "path", "impassable", "COMPACTED", "DIRT", "GRAVEL", "PATH", "IMPASSABLE"}
E = html.escape


# --- lecture ---
def read_gpx(path):
    pts = []
    txt = open(path, encoding="utf-8").read()
    for m in re.finditer(r"<(trkpt|rtept)\b([^>]*?)(/>|>(.*?)</\1>)", txt, re.S):
        a = dict(re.findall(r'(lat|lon)="([^"]+)"', m.group(2)))
        ele = re.search(r"<ele>([^<]+)</ele>", m.group(4) or "")
        pts.append((float(a["lat"]), float(a["lon"]), float(ele.group(1)) if ele else None))
    return pts


def sans_way(nom):
    """Nom d'ouvrage sans identifiant de way."""
    return re.sub(r"\bways?\s*[\d ,/+]+", "", nom or "").strip()


def surf_classe(s):
    return "paved" if s in PAVED else "unpaved" if s in UNPAVED else "inconnu"


# --- géométrie (plan local équirectangulaire, mètres) ---
class Plan:
    def __init__(self, lat0, lon0):
        self.lat0, self.lon0 = lat0, lon0
        self.ky = 111320.0
        self.kx = 111320.0 * math.cos(math.radians(lat0))

    def xy(self, p):
        return ((p[1] - self.lon0) * self.kx, (p[0] - self.lat0) * self.ky)


def cumul(P):
    s = [0.0]
    for i in range(1, len(P)):
        s.append(s[-1] + math.hypot(P[i][0] - P[i - 1][0], P[i][1] - P[i - 1][1]))
    return s


def seg_dist(p, a, b):
    dx, dy = b[0] - a[0], b[1] - a[1]
    L = dx * dx + dy * dy
    t = 0.0 if L == 0 else max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / L))
    return math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)


def dp(P, eps=EPS_M, bloc=256):
    """Douglas–Peucker itératif par blocs (coût linéaire en pratique) ; renvoie les indices gardés."""
    n = len(P)
    if n < 3:
        return list(range(n))
    keep = bytearray(n)
    for a0 in range(0, n - 1, bloc):
        b0 = min(a0 + bloc, n - 1)
        keep[a0] = keep[b0] = 1
        st = [(a0, b0)]
        while st:
            a, b = st.pop()
            dmax, im = -1.0, -1
            pa, pb = P[a], P[b]
            for i in range(a + 1, b):
                d = seg_dist(P[i], pa, pb)
                if d > dmax:
                    dmax, im = d, i
            if dmax > eps:
                keep[im] = 1
                st += [(a, im), (im, b)]
    return [i for i in range(n) if keep[i]]


def densifie(P, pas=DENS_M):
    out = [P[0]] if P else []
    for i in range(1, len(P)):
        (x0, y0), (x1, y1) = P[i - 1], P[i]
        k = int(math.hypot(x1 - x0, y1 - y0) // pas)
        out += [(x0 + (x1 - x0) * j / (k + 1), y0 + (y1 - y0) * j / (k + 1)) for j in range(1, k + 1)]
        out.append(P[i])
    return out


class Grille:
    def __init__(self, P, cell=TOL_M):
        self.c, self.g = cell, {}
        for p in P:
            self.g.setdefault((int(p[0] // cell), int(p[1] // cell)), []).append(p)

    def proche(self, p, r=TOL_M):
        cx, cy, k = int(p[0] // self.c), int(p[1] // self.c), int(r // self.c) + 1
        return any(math.hypot(q[0] - p[0], q[1] - p[1]) < r
                   for i in range(cx - k, cx + k + 1) for j in range(cy - k, cy + k + 1) for q in self.g.get((i, j), ()))


def runs(flags):
    """Plages [i, j) consécutives où flags est vrai."""
    out, i = [], None
    for k, f in enumerate(flags + [False]):
        if f and i is None:
            i = k
        elif not f and i is not None:
            out.append((i, k)); i = None
    return out


def fusion(iv):
    out = []
    for a, b in sorted(iv):
        if out and a <= out[-1][1]:
            out[-1][1] = max(out[-1][1], b)
        else:
            out.append([a, b])
    return out


def dans(s, iv):
    return any(a <= s <= b for a, b in iv)


# --- corpus ---
def ouvrage_type(t):
    t = t.lower()
    return "passage dessous" if "passage sous" in t else "tunnel" if ("tunnel" in t or "covered" in t) else "pont"


def corpus(M, inclure):
    """Entrées affichables, ouvrages et témoins affichables, éléments tenus V1 (points seulement) et comptes exclus."""
    exclus = {"traces": 0, "planifiés": 0, "ouvrages": 0, "témoins": 0}
    lab = lambda s, mvp: ("tenue MVP (jugée)" if mvp else "tenue V1") if s == TENUE else s
    T = []
    for e in M["perso"]["traces"]:
        T.append(dict(e, source="perso MVP", serie=lab(e["serie"], True), _f=e["fichier"]))
    for e in M["perso_v1"]["traces"]:
        if e.get("serie") != "calibration" and not inclure:
            exclus["traces"] += 1; continue
        T.append(dict(e, source="perso V1", serie=lab(e.get("serie"), False), _f=e["fichier"]))
    S = {r["id"]: r["serie"] for r in csv.DictReader(open("private/v1/series-v1.csv")) if r["type"] == "planifie"}
    for e in M["perso_v1"]["planifies"]:
        s = S.get(e["id"])
        if s != "calibration" and not inclure:
            exclus["planifiés"] += 1; continue
        T.append(dict(e, source="planifié", groupe=e.get("role"), serie=lab(s or "par_cas", False), dist_km=e.get("km"), _f=e["fichier"]))
    for sec, src in (("vcyclist", "vcyclist"), ("osm_surface", "osm-surface")):
        for e in M.get(sec, []):
            f = e["fichier"].split(" + ")[0]
            serie = lab(e.get("serie"), True)
            if f.endswith(".gpx"):
                T.append(dict(e, source=src, groupe="parcours", serie=serie, _f=f))
            elif f.endswith("/"):
                for g in sorted(glob.glob(f + "*.gpx")):
                    nom = os.path.basename(g)[:-4]
                    T.append({"id": f"{e['id']}-{nom}", "source": src, "groupe": e["id"].replace("vc-", ""), "serie": serie,
                              "teste": e.get("teste", ""), "licence": e.get("licence", ""), "fichier": g, "_f": g})
    O, cache = [], set(M["ouvrages"].get("serie_tenue_a_l_ecart") or [])
    for o in json.load(open("data/ouvrages-geom.json")):
        O.append(dict(o, serie=lab(TENUE if o["id"] in cache else "calibration", True), _src="public"))
    for o in json.load(open("private/ouvrages-perso.json")):
        O.append(dict(o, serie=lab(o.get("serie"), True), _src="perso MVP"))
    cache = []   # éléments tenus V1 : identifiant et points, jamais affichés
    for o in json.load(open("private/v1/ouvrages-v1.json")):
        if o.get("serie") != "calibration" and not inclure:
            exclus["ouvrages"] += 1; cache.append((o["id"], list(o.get("pts", {}).values()))); continue
        O.append(dict(o, serie=lab(o.get("serie"), False), _src="perso V1"))
    W = []
    for t in json.load(open("private/v1/temoins-v1.json")):
        if t.get("serie") != "calibration" and not inclure:
            exclus["témoins"] += 1; cache.append((t["id"], [t[k] for k in ("debut", "fin") if k in t])); continue
        W.append(dict(t, serie=lab(t.get("serie"), False)))
    for o in O:
        o["_cat"] = ouvrage_type(o.get("type", ""))
        o["_pts"] = [(k, tuple(v)) for k, v in o.get("pts", {}).items()]
    return T, O, W, cache, exclus


def recalages(i, noms):
    """Segments recalés : liste de (nom du segment, [fichiers GPX des parties], baro.csv, surface.csv)."""
    seg = {}
    for f in noms:
        m = re.match(rf"^{re.escape(i)}(?:\.s(\d+))?(?:\.part(\d+))?\.gpx$", f)
        if m:
            seg.setdefault(int(m.group(1) or 0), []).append((int(m.group(2) or 0), f))
    out = []
    for k in sorted(seg):
        base = f"{i}.s{k}" if k else i
        out.append((base, [f"private/recales/{f}" for _, f in sorted(seg[k])],
                    f"private/recales/{base}.baro.csv", f"private/recales/{base}.surface.csv"))
    return out


# --- rendu SVG ---
def poly_svg(P, H, cls, eps=EPS_M):
    if len(P) < 2:
        return ""
    idx = dp(P, eps)
    return f'<polyline class="{cls}" points="{" ".join(f"{P[k][0]:.0f},{H - P[k][1]:.0f}" for k in idx)}"/>'


def decime(xs, ys, n=1500):
    """Garde min et max par case d'abscisse : profil léger, pics conservés."""
    if len(xs) <= 2 * n:
        return list(zip(xs, ys))
    x0, x1 = xs[0], xs[-1] or 1
    cases = {}
    for x, y in zip(xs, ys):
        k = int((x - x0) / (x1 - x0 + 1e-9) * n)
        c = cases.get(k)
        if c is None:
            cases[k] = [(x, y), (x, y)]
        else:
            if y < c[0][1]: c[0] = (x, y)
            if y > c[1][1]: c[1] = (x, y)
    out = []
    for k in sorted(cases):
        out += sorted(set(cases[k]))
    return out


def page(t, O, W, cache, noms, inclure):
    R = {"ouvrages": 0, "segments": 0, "masques": 0}
    brut = t["_f"].replace(".gpx", ".trim1km.gpx") if t["source"].startswith("perso") else t["_f"]
    if not os.path.exists(brut):
        brut = t["_f"]
    raw = read_gpx(brut) if os.path.exists(brut) else []
    segs = recalages(t["id"], noms) if t["source"].startswith("perso") else []
    rec = [(b, [p for f in fs for p in read_gpx(f)], bc, sc) for b, fs, bc, sc in segs]
    rec = [r for r in rec if len(r[1]) > 1]
    R["segments"] = len(rec)
    tous = raw + [p for r in rec for p in r[1]]
    if not tous:
        return None, R
    la = [p[0] for p in tous]; lo = [p[1] for p in tous]
    pl = Plan((min(la) + max(la)) / 2, min(lo))
    X = [pl.xy(p) for p in raw]
    G = Grille(X or [pl.xy(p) for p in tous])
    sX = cumul(X)

    # fenêtres masquées : éléments tenus V1 cités ou à moins de TOL_M de la trace
    mpts = []
    if not inclure and t["source"].startswith("perso"):   # parcours publics : rien à cacher, un trou trahirait l'élément
        cites = set(t.get("passages_masques") or [])
        for oid, pts in cache:
            q = [pl.xy(p) for p in pts]
            if oid in cites or any(G.proche(p) for p in q):
                mpts += q; R["masques"] += 1
    proche_m = lambda p: any(abs(p[0] - m[0]) < MASK_M and abs(p[1] - m[1]) < MASK_M and math.hypot(p[0] - m[0], p[1] - m[1]) < MASK_M for m in mpts)
    okX = [not proche_m(p) for p in X] if mpts else [True] * len(X)
    recs = []   # (nom, xy densifié, flags, s, intervalles masqués, longueur, baro, surface)
    for b, P, bc, sc in rec:
        Q = densifie([pl.xy(p) for p in P])
        ok = [not proche_m(p) for p in Q] if mpts else [True] * len(Q)
        sQ = cumul(Q)
        marge = MARGE_M + (MASK_M if ".part" in "".join(segs[[s[0] for s in segs].index(b)][1]) else 0)
        iv = fusion([[sQ[i] - marge, sQ[j - 1] + marge] for i, j in runs([not f for f in ok])])
        recs.append((b, Q, ok, sQ, iv, sQ[-1], bc, sc))

    # ouvrages et témoins concernés
    bb = (min(la) - 0.01, max(la) + 0.01, min(lo) - 0.01, max(lo) + 0.01)
    cites = set(t.get("passages_masques") or [])
    ouv = [o for o in O if o["_pts"] and (o["id"] in cites or (all(bb[0] < p[0] < bb[1] and bb[2] < p[1] < bb[3] for _, p in o["_pts"])
                                                                and all(G.proche(pl.xy(p)) for _, p in o["_pts"])))]
    R["ouvrages"] = len(ouv)
    tem = [w for w in W if w.get("trace") == t["id"]]

    # carte
    allxy = X + [p for r in recs for p in r[1]]
    x0, x1 = min(p[0] for p in allxy), max(p[0] for p in allxy)
    y0, y1 = min(p[1] for p in allxy), max(p[1] for p in allxy)
    pad = max(x1 - x0, y1 - y0, 200) * 0.04
    H = y1 + pad
    w, h = x1 - x0 + 2 * pad, y1 - y0 + 2 * pad
    r0 = max(w, h) / 300
    svg = [f'<svg id="carte" viewBox="{x0 - pad:.0f} 0 {w:.0f} {h:.0f}" xmlns="http://www.w3.org/2000/svg">']
    svg += [poly_svg(X[i:j], H, "brut") for i, j in runs(okX)]
    for _, Q, ok, *_ in recs:
        svg += [poly_svg(Q[i:j], H, "rec", EPS_M / 2) for i, j in runs(ok)]
    for wt in tem:
        if X and "debut" in wt and "fin" in wt:
            ia, ib = (min(range(len(X)), key=lambda k: math.hypot(X[k][0] - q[0], X[k][1] - q[1])) for q in (pl.xy(wt["debut"]), pl.xy(wt["fin"])))
            ia, ib = min(ia, ib), max(ia, ib)
            svg += [poly_svg(X[i:j], H, "tem") for i, j in runs([okX[k] and ia <= k <= ib for k in range(len(X))])]
            for q in (wt["debut"], wt["fin"]):
                x, y = pl.xy(q)
                svg.append(f'<circle class="m tm" data-r="5" cx="{x:.0f}" cy="{H - y:.0f}" r="{r0:.0f}"><title>témoin {E(wt["id"])} ({E(wt.get("nature", ""))})</title></circle>')
    for o in ouv:
        c = COUL[o["_cat"]]
        q = [pl.xy(p) for _, p in o["_pts"]]
        ab = [pl.xy(dict(o["_pts"])[k]) for k in ("A", "milieu", "B") if k in dict(o["_pts"])]
        if len(ab) > 1:
            svg.append(f'<polyline class="ouv" stroke="{c}" points="{" ".join(f"{x:.0f},{H - y:.0f}" for x, y in ab)}"/>')
        tit = f'{o["id"]} · {o.get("type", "")} · {o.get("length_m", "")} m · {o["serie"]}'
        for (k, _), (x, y) in zip(o["_pts"], q):
            big = k in ("milieu", "croisement")
            svg.append(f'<circle class="m" data-r="{6 if big else 4}" cx="{x:.0f}" cy="{H - y:.0f}" r="{r0:.0f}" fill="{c if big else "#fff"}" stroke="{c}"><title>{E(tit)} · {E(k)}</title></circle>')
        x, y = pl.xy(dict(o["_pts"]).get("milieu") or dict(o["_pts"]).get("croisement") or o["_pts"][0][1])
        svg.append(f'<text class="l" x="{x:.0f}" y="{H - y:.0f}" dx="0" fill="{c}">{E(o["id"])}</text>')
    svg.append("</svg>")

    # profil et bande de revêtement
    off, prof_rec, ins, bande = 0.0, [], [], []
    for b, Q, ok, sQ, iv, L, bc, sc in recs:
        sm = 0.0
        if os.path.exists(bc):
            rows = [(float(r["s_recale_m"]), float(r["baro_m"]), r["insere"] == "1") for r in csv.DictReader(open(bc)) if r["baro_m"]]
            rows = [r for r in rows if not dans(r[0], iv)]
            prof_rec.append([(off + s, z) for s, z, _ in rows])
            ins += [(off + s, z) for s, z, k in rows if k]
            sm = max((r[0] for r in rows), default=0.0)
        if os.path.exists(sc):
            for r in csv.DictReader(open(sc)):
                a, z = float(r["s_debut_m"]), float(r["s_fin_m"])
                cl = surf_classe(r["surface"])
                cl = "masqué" if any(a < q1 and z > q0 for q0, q1 in iv) else cl
                det = f'{r["surface"]} · {r["road_class"]} · {r["use"]}' if cl != "masqué" else ""
                bande.append([off + a, off + z, cl, det])
        off += max(L, sm) + 200.0
    if t["source"] == "osm-surface":
        ref = t["_f"].replace(".gpx", ".reference.json")
        if os.path.exists(ref):
            for tr in json.load(open(ref))["tracks"]:
                bande += [[float(s["from"]), float(s["to"]), surf_classe(s["surface"]), s["surface"]] for s in tr["surfaces"]]
    fus = []
    for a, z, cl, det in bande:
        if fus and fus[-1][2] == cl and abs(fus[-1][1] - a) < 1:
            fus[-1][1] = z
            if det and det not in fus[-1][3]: fus[-1][3] = (fus[-1][3] + " ; " + det)[:200]
        else:
            fus.append([a, z, cl, det])
    prof_brut = [[(sX[k], raw[k][2]) for k in range(i, j) if raw[k][2] is not None] for i, j in runs(okX)]
    prof_brut = [c for c in prof_brut if c]
    courbes = prof_brut + prof_rec
    psvg = ""
    if courbes or fus:
        smax = max([c[-1][0] for c in courbes if c] + [f[1] for f in fus] + [1.0])
        zs = [z for c in courbes for _, z in c]
        zmin, zmax = (min(zs), max(zs)) if zs else (0, 1)
        zmax = zmax if zmax > zmin + 10 else zmin + 10
        fx = lambda s: 50 + s / smax * 940
        fy = lambda z: 200 - (z - zmin) / (zmax - zmin) * 190
        p = ['<svg id="profil" viewBox="0 0 1000 262" xmlns="http://www.w3.org/2000/svg">']
        pas = next(k for k in (1, 2, 5, 10, 20, 25, 50, 100, 200, 500) if smax / 1000 / k <= 12)
        for k in range(0, int(smax / 1000 / pas) + 1):
            x = fx(k * pas * 1000)
            p.append(f'<line class="g" x1="{x:.1f}" y1="10" x2="{x:.1f}" y2="200"/><text class="ax" x="{x:.1f}" y="254" text-anchor="middle">{k * pas}</text>')
        for z in (zmin, (zmin + zmax) / 2, zmax):
            p.append(f'<text class="ax" x="46" y="{fy(z) + 4:.1f}" text-anchor="end">{z:.0f}</text><line class="g" x1="50" y1="{fy(z):.1f}" x2="990" y2="{fy(z):.1f}"/>')
        for cls, cs in (("pb", prof_brut), ("pr", prof_rec)):
            for c in cs:
                d = decime([s for s, _ in c], [z for _, z in c])
                if len(d) > 1:
                    p.append(f'<polyline class="{cls}" points="{" ".join(f"{fx(s):.1f},{fy(z):.1f}" for s, z in d)}"/>')
        vus = set()
        for s, z in ins:
            k = (round(fx(s) * 2), round(fy(z)))
            if k not in vus:
                vus.add(k); p.append(f'<circle class="ins" cx="{fx(s):.1f}" cy="{fy(z):.1f}" r="1.6"/>')
        for a, z, cl, det in fus:
            tit = f"{cl} {a / 1000:.2f}–{z / 1000:.2f} km" + (f" ({det})" if det else "")
            p.append(f'<rect x="{fx(a):.2f}" y="212" width="{max(fx(z) - fx(a), 0.3):.2f}" height="16" fill="{SURF[cl]}"><title>{E(tit)}</title></rect>')
        p.append('<text class="ax" x="46" y="224" text-anchor="end">revêt.</text><text class="ax" x="990" y="242" text-anchor="end">km</text></svg>')
        psvg = "".join(p)

    # métadonnées
    meta = {k: v for k, v in t.items() if not k.startswith("_") and k not in ("sha256", "sha256_trim1km", "sha256_reference", "provenance_depot")}
    qual = meta.pop("qualification", None) or {}
    if not inclure and meta.get("passages_masques"):
        noms_o = {o["id"] for o in O} | {w["id"] for w in W}
        n_c = sum(1 for m in meta["passages_masques"] if m not in noms_o)
        meta["passages_masques"] = [m for m in meta["passages_masques"] if m in noms_o] + ([f"+ {n_c} élément(s) tenu(s) à l'écart V1"] if n_c else [])
    lignes = "".join(f"<tr><th>{E(k)}</th><td>{E(', '.join(map(str, v)) if isinstance(v, list) else str(v))}</td></tr>" for k, v in meta.items())
    lignes += "".join(f"<tr><th>qualification.{E(k)}</th><td>{E(str(v))}</td></tr>" for k, v in qual.items())
    lignes += f"<tr><th>fichier affiché</th><td>{E(brut)}</td></tr>"
    lignes += f"<tr><th>segments recalés</th><td>{E(', '.join(r[0] for r in recs) or 'aucun')}</td></tr>"
    if ins:
        lignes += f"<tr><th>points insérés (baro)</th><td>{len(ins)}</td></tr>"
    if R["masques"]:
        lignes += f"<tr><th>fenêtres masquées</th><td>{R['masques']} (± {MASK_M:.0f} m autour d'éléments tenus à l'écart V1)</td></tr>"
    louv = "".join(f'<tr><td style="color:{COUL[o["_cat"]]}">■</td><td>{E(o["id"])}</td><td>{E(sans_way(o.get("name")))}</td><td>{E(o.get("type", ""))}</td>'
                   f'<td>{E(str(o.get("length_m", "")))}</td><td>{E(o["serie"])}</td><td>{E(o["_src"])}</td></tr>' for o in ouv)
    ltem = "".join(f"<tr><td>{E(w['id'])}</td><td>{E(w.get('nature', ''))}</td><td>{w.get('length_m', 0):.0f}</td><td>{E(w['serie'])}</td></tr>" for w in tem)
    doc = PAGE.format(id=E(t["id"]), sous=E(f"{t['source']} · {t.get('groupe') or ''} · {t['serie']}"), carte="".join(svg),
                      profil=psvg or "<p>Pas d'altitude.</p>", meta=lignes,
                      ouv=f"<table><tr><th></th><th>id</th><th>nom</th><th>type</th><th>m</th><th>série</th><th>source</th></tr>{louv}</table>" if ouv else "<p>Aucun.</p>",
                      tem=f"<h2>Témoins S2</h2><table><tr><th>id</th><th>nature</th><th>m</th><th>série</th></tr>{ltem}</table>" if tem else "",
                      css=CSS, js=JS_PAGE, coul_pont=COUL["pont"], coul_tun=COUL["tunnel"], coul_pd=COUL["passage dessous"],
                      s_p=SURF["paved"], s_u=SURF["unpaved"], s_i=SURF["inconnu"], s_m=SURF["masqué"])
    R["km"] = t.get("dist_km") or round(sX[-1] / 1000, 1) if sX else t.get("dist_km")
    return doc, R


CSS = """body{font:14px/1.4 -apple-system,system-ui,sans-serif;margin:16px;color:#222}h1{font-size:20px;margin:0}h2{font-size:16px;margin:18px 0 6px}
table{border-collapse:collapse}td,th{padding:2px 6px;border-bottom:1px solid #eee;text-align:left;vertical-align:top}th{font-weight:600}
.leg span{display:inline-block;margin-right:14px}.sw{display:inline-block;width:18px;height:4px;vertical-align:middle;margin-right:4px}
#carte{width:100%;height:68vh;border:1px solid #ccc;background:#fbfbf8;cursor:grab;touch-action:none}
.brut{fill:none;stroke:#e0473c;stroke-opacity:.55;stroke-width:4px;vector-effect:non-scaling-stroke;stroke-linejoin:round}
.rec{fill:none;stroke:#1f5fbf;stroke-width:1.4px;vector-effect:non-scaling-stroke;stroke-linejoin:round}
.tem{fill:none;stroke:#ff9f1c;stroke-width:8px;stroke-opacity:.6;vector-effect:non-scaling-stroke}
.ouv{fill:none;stroke-width:3px;vector-effect:non-scaling-stroke}.m{stroke-width:1.5px;vector-effect:non-scaling-stroke}.tm{fill:#ff9f1c;stroke:#7a4a00}
.l{font-size:11px;paint-order:stroke;stroke:#fff;stroke-width:3px;vector-effect:non-scaling-stroke;font-weight:600}.sans-l .l{display:none}
#profil{width:100%;border:1px solid #ccc;background:#fff}.pb{fill:none;stroke:#999;stroke-width:1}.pr{fill:none;stroke:#1f5fbf;stroke-width:1}
.ins{fill:#e36c0a}.g{stroke:#eee}.ax{font-size:10px;fill:#666}a{color:#1f5fbf}"""

JS_PAGE = """const s=document.getElementById('carte');if(s){const vb=s.viewBox.baseVal,v0=[vb.x,vb.y,vb.width,vb.height];
const k=()=>{const r=s.getBoundingClientRect();return Math.max(vb.width/r.width,vb.height/r.height)};
const maj=()=>{const f=k();s.querySelectorAll('.m').forEach(c=>c.setAttribute('r',c.dataset.r*f));s.querySelectorAll('.l').forEach(t=>{t.setAttribute('font-size',11*f);t.setAttribute('dx',8*f)})};
s.addEventListener('wheel',e=>{e.preventDefault();const r=s.getBoundingClientRect(),f=e.deltaY>0?1.25:0.8,c=k();
const mx=vb.x+vb.width/2+(e.clientX-r.left-r.width/2)*c,my=vb.y+vb.height/2+(e.clientY-r.top-r.height/2)*c;
vb.x=mx-(mx-vb.x)*f;vb.y=my-(my-vb.y)*f;vb.width*=f;vb.height*=f;maj()},{passive:false});
let d=null;s.addEventListener('pointerdown',e=>{d=[e.clientX,e.clientY];s.setPointerCapture(e.pointerId)});
s.addEventListener('pointermove',e=>{if(!d)return;const c=k();vb.x-=(e.clientX-d[0])*c;vb.y-=(e.clientY-d[1])*c;d=[e.clientX,e.clientY]});
s.addEventListener('pointerup',()=>d=null);document.getElementById('reinit').onclick=()=>{[vb.x,vb.y,vb.width,vb.height]=v0;maj()};
document.getElementById('lab').onchange=e=>s.classList.toggle('sans-l',!e.target.checked);addEventListener('resize',maj);maj()}"""

PAGE = """<!DOCTYPE html><html lang="fr"><head><meta charset="utf-8"><title>{id}</title><style>{css}</style></head><body>
<p><a href="../index.html">← index</a></p><h1>{id}</h1><p>{sous}</p>
<div class="leg"><span><i class="sw" style="background:#e0473c;opacity:.55;height:6px"></i>trace brute (rognée)</span><span><i class="sw" style="background:#1f5fbf"></i>géométrie recalée</span>
<span><i class="sw" style="background:{coul_pont}"></i>pont</span><span><i class="sw" style="background:{coul_tun}"></i>tunnel</span><span><i class="sw" style="background:{coul_pd}"></i>passage dessous</span>
<span><i class="sw" style="background:#ff9f1c"></i>témoin S2</span><label><input type="checkbox" id="lab" checked> étiquettes</label> <button id="reinit">recentrer</button> <small>molette : zoom, glisser : déplacer</small></div>
{carte}
<h2>Profil d'altitude (baro)</h2><div class="leg"><span><i class="sw" style="background:#999"></i>brut (abscisse de la trace)</span><span><i class="sw" style="background:#1f5fbf"></i>transporté sur le recalé</span>
<span><i class="sw" style="background:#e36c0a;height:6px;width:6px;border-radius:3px"></i>point inséré</span>
<span><i class="sw" style="background:{s_p};height:10px"></i>paved</span><span><i class="sw" style="background:{s_u};height:10px"></i>unpaved</span><span><i class="sw" style="background:{s_i};height:10px"></i>inconnu</span><span><i class="sw" style="background:{s_m};height:10px"></i>masqué</span></div>
{profil}
<h2>Ouvrages concernés</h2>{ouv}{tem}<h2>Métadonnées</h2><table>{meta}</table><script>{js}</script></body></html>"""

INDEX = """<!DOCTYPE html><html lang="fr"><head><meta charset="utf-8"><title>Explorateur des traces du banc</title><style>{css}
#t th{{cursor:pointer;background:#f4f4f4;position:sticky;top:0}}#t td.n{{text-align:right}}.avert{{background:#fde2e1;padding:6px 10px;border:1px solid #e0473c}}</style></head><body>
<h1>Explorateur des traces du banc</h1><p>{resume}</p>{avert}
<p><input id="q" placeholder="filtrer…" size="30"> <select id="src"><option value="">toutes sources</option>{opts}</select> {case} <span id="n"></span></p>
<table id="t"><thead><tr>{tete}</tr></thead><tbody>{corps}</tbody></table>
<script>const T=document.getElementById('t'),B=T.tBodies[0],q=document.getElementById('q'),src=document.getElementById('src'),tv=document.getElementById('tv');
function filtre(){{let n=0;for(const r of B.rows){{const ok=r.textContent.toLowerCase().includes(q.value.toLowerCase())&&(!src.value||r.dataset.src==src.value)&&(!r.dataset.tv||(tv&&tv.checked));r.hidden=!ok;n+=ok}}document.getElementById('n').textContent=n+' trace(s)'}}
[q,src,tv].forEach(e=>e&&e.addEventListener('input',filtre));
T.tHead.querySelectorAll('th').forEach((th,i)=>th.onclick=()=>{{const asc=th.dataset.o!='a';th.dataset.o=asc?'a':'d';
const v=r=>{{const t=r.cells[i].textContent,x=parseFloat(t);return isNaN(x)?t:x}};
[...B.rows].sort((a,b)=>{{const x=v(a),y=v(b);return (typeof x==typeof y?(x<y?-1:x>y?1:0):(typeof x=='number'?-1:1))*(asc?1:-1)}}).forEach(r=>B.appendChild(r))}});filtre()</script></body></html>"""


def main():
    ap = argparse.ArgumentParser(description="Explorateur HTML local des traces du banc (sortie private/explorateur/).")
    ap.add_argument("--inclure-tenues-v1", action="store_true", help="inclut la série tenue à l'écart V1 (à éviter en développement)")
    ap.add_argument("--ids", help="liste d'identifiants séparés par des virgules")
    a = ap.parse_args()
    t0 = time.time()
    if a.inclure_tenues_v1:
        print("AVERTISSEMENT : la série tenue à l'écart V1 est incluse ; ne pas ouvrir la sortie en session de développement.", file=sys.stderr)
    M = yaml.safe_load(open("manifest.yaml"))
    T, O, W, cache, exclus = corpus(M, a.inclure_tenues_v1)
    if a.ids:
        voulus = set(a.ids.split(","))
        T = [t for t in T if t["id"] in voulus]
    noms = sorted(os.listdir("private/recales"))
    if os.path.isdir(OUT):
        shutil.rmtree(OUT)
    os.makedirs(f"{OUT}/t")
    rows, n_masq, n_vide, par_src = [], 0, 0, {}
    for t in T:
        doc, R = page(t, O, W, cache, noms, a.inclure_tenues_v1)
        if doc is None:
            n_vide += 1; continue
        nom = re.sub(r"[^A-Za-z0-9_.-]", "_", t["id"]) + ".html"
        open(f"{OUT}/t/{nom}", "w", encoding="utf-8").write(doc)
        n_masq += R["masques"]
        par_src[t["source"]] = par_src.get(t["source"], 0) + 1
        q = t.get("qualification") or {}
        rows.append((t, nom, R, q))
    cols = ["id", "source", "groupe", "série", "mois", "région", "km", "verdict", "noise_m", "p95_abs_m", "drift_m",
            "gain_baro_m", "gain_mnt_m", "ouvrages", "recalés"]
    num = lambda v: "" if v is None else str(v)
    corps = []
    for t, nom, R, q in rows:
        tv = ' data-tv="1"' if t["serie"] == "tenue V1" else ""
        cel = [f'<a href="t/{E(nom)}">{E(t["id"])}</a>', E(t["source"]), E(str(t.get("groupe") or "")), E(t["serie"]), E(str(t.get("mois") or "")),
               E(str(t.get("region") or "")), num(R.get("km"))] + [E(num(q.get(k))) for k in cols[7:13]] + [str(R["ouvrages"]), str(R["segments"])]
        corps.append(f'<tr data-src="{E(t["source"])}"{tv}>' + "".join(f'<td{" class=n" if i >= 6 else ""}>{c}</td>' for i, c in enumerate(cel)) + "</tr>")
    resume = ", ".join(f"{k} : {v}" for k, v in par_src.items()) + f" ; éléments tenus à l'écart V1 exclus : {sum(exclus.values())}" * (not a.inclure_tenues_v1)
    avert = '<p class="avert">Série tenue à l\'écart V1 incluse (--inclure-tenues-v1) : masquée par défaut, case à cocher ci-dessous.</p>' if a.inclure_tenues_v1 else ""
    case = '<label><input type="checkbox" id="tv"> montrer la série tenue à l\'écart V1</label>' if a.inclure_tenues_v1 else ""
    open(f"{OUT}/index.html", "w", encoding="utf-8").write(INDEX.format(
        css=CSS, resume=E(resume), avert=avert, case=case, opts="".join(f'<option>{E(k)}</option>' for k in par_src),
        tete="".join(f"<th>{c}</th>" for c in cols), corps="\n".join(corps)))
    taille = sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(OUT) for f in fs)
    print(f"traces affichées : {len(rows)} (" + ", ".join(f"{k} {v}" for k, v in par_src.items()) + ")")
    if not a.inclure_tenues_v1:
        print("tenus à l'écart V1 exclus : " + ", ".join(f"{k} {v}" for k, v in exclus.items()) + f" ; fenêtres masquées : {n_masq}")
    if n_vide:
        print(f"sans géométrie : {n_vide}")
    print(f"sortie : {taille / 1e6:.1f} Mo en {time.time() - t0:.0f} s ; index : {OUT}/index.html")


if __name__ == "__main__":
    main()
