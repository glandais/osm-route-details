#!/usr/bin/env python3
"""Tire les séries du banc pour qu'aucune donnée ne serve aux deux séries.

Règle (bench/README.md §2) :
1. Groupes d'ouvrages : composantes connexes des ouvrages dont des points de référence sont à moins
   de NEAR_M (même ouvrage en plusieurs entrées, dessus/dessous, ponts contigus, départ sur un pont).
   Série d'un groupe : tenue à l'écart si le premier chiffre hexadécimal de
   sha256("osm-route-details:groupe:" + plus petit identifiant du groupe) est dans 0–4.
2. Passage : une trace franchit un ouvrage si elle passe à moins de TOL_M de chacun de ses points de
   référence (A, milieu, B ; ou le point de croisement pour un passage dessous).
3. Série d'une trace : celle des groupes d'ouvrages qu'elle franchit s'ils sont tous de la même série ;
   sinon (aucun ouvrage, ou séries mélangées) sha256("osm-route-details:trace:" + id), même critère.
   Les négatifs (contrôle du banc) restent en calibration.
5. Déjà vu par un projet dont le service reprend les réglages ⇒ calibration : les traces vc-*
   (vcyclist : détecteurs et constructeur de tablier), os-* (osm-surface : chaîne typologie) et tout
   groupe d'ouvrages dont un membre a un renvoi vcyclist. Les traces qui les franchissent suivent les points 3 et 4.
4. Passage mixte (série de la trace ≠ série de l'ouvrage) : exclu des métriques d'ouvrage (E3, E4,
   E5, S*) des deux séries, et masqué (± MASK_M autour de l'ouvrage) dans les métriques de la trace
   (E1, E2 : D+ sommé sur les tronçons restants, pour le service comme pour la référence).
Ainsi un mètre de trace et un ouvrage n'appartiennent qu'à une série.

Avec --composantes, applique à la place la règle « composante connexe trace × ouvrage » (pour
comparaison seulement : elle soude ici 28 cas de Loire-Atlantique en un seul groupe).

Entrées : ouvrages-geom.json, une liste de traces « id chemin.gpx|chemin.json ».
Sortie : CSV id,type,groupe,serie,passages_masques sur la sortie standard.

Usage : series_groups.py ouvrages-geom.json traces.txt [--composantes]
"""
import hashlib
import json
import math
import re
import sys

TOL_M = 40.0
NEAR_M = 150.0
MASK_M = 500.0
# Même ouvrage, points de référence trop éloignés pour NEAR_M (croisement sous un long viaduc).
LIENS_DECLARES = [("chevire", "chevire-dessous")]


def dist_m(a, b):
    k = 111320.0
    return math.hypot((a[0] - b[0]) * k, (a[1] - b[1]) * k * math.cos(math.radians(a[0])))


def load_track(path):
    if path.endswith(".json"):
        return [tuple(p) for p in json.load(open(path))["location"]]
    txt = open(path).read()
    return [(float(a), float(b)) for a, b in re.findall(r'lat="([-\d.]+)"\s+lon="([-\d.]+)"', txt)]


def near(track, p, tol):
    d = tol / 111320.0 * 2
    return any(abs(q[0] - p[0]) < d and abs(q[1] - p[1]) < d * 2 and dist_m(q, p) < tol for q in track)


def serie_of(key):
    h = hashlib.sha256(f"osm-route-details:{key}".encode()).hexdigest()[0]
    return "tenue_a_l_ecart" if h in "01234" else "calibration"


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
        ra, rb = self.find(a), self.find(b)
        if ra != rb:
            self.p[max(ra, rb)] = min(ra, rb)

    def groups(self):
        g = {}
        for x in self.p:
            g.setdefault(self.find(x), []).append(x)
        return {min(v): sorted(v) for v in g.values()}


def main(geom_path, traces_path, mode=""):
    geom = json.load(open(geom_path))
    opts = {o["id"]: list(o["pts"].values()) for o in geom}
    vu_vcyclist = {o["id"] for o in geom if o.get("vc")}
    traces = [l.split() for l in open(traces_path) if l.strip() and not l.startswith("#")]
    uf = UF()
    ids = list(opts)
    for i, a in enumerate(ids):
        uf.find(a)
        for b in ids[i + 1:]:
            if any(dist_m(p, q) < NEAR_M for p in opts[a] for q in opts[b]):
                uf.union(a, b)
    for a, b in LIENS_DECLARES:
        uf.union(a, b)
    crossings = {}
    for tid, path in traces:
        tr = load_track(path)
        crossings[tid] = [o for o, pts in opts.items() if all(near(tr, p, TOL_M) for p in pts)]

    print("id,type,groupe,serie,passages_masques")
    if mode == "--composantes":
        for tid, cs in crossings.items():
            uf.find(tid)
            for o in cs:
                uf.union(tid, o)
        for gid, ms in sorted(uf.groups().items()):
            s = serie_of(f"groupe:{gid}")
            for m in ms:
                kind = "ouvrage" if m in opts else "trace"
                print(f"{m},{kind},{gid},{'calibration' if m.startswith('neg-') else s},")
        return

    ogroups = uf.groups()
    og_of = {m: gid for gid, ms in ogroups.items() for m in ms}
    oserie = {gid: "calibration" if vu_vcyclist & set(ms) else serie_of(f"groupe:{gid}")
              for gid, ms in ogroups.items()}
    for gid, ms in sorted(ogroups.items()):
        for m in ms:
            print(f"{m},ouvrage,{gid},{oserie[gid]},")
    for tid, cs in crossings.items():
        series = {oserie[og_of[o]] for o in cs}
        if tid.startswith(("neg-", "vc-", "os-")):
            s = "calibration"
        elif len(series) == 1:
            s = series.pop()
        else:
            s = serie_of(f"trace:{tid}")
        masked = [o for o in cs if oserie[og_of[o]] != s]
        print(f"{tid},trace,{tid},{s},{' '.join(masked)}")


if __name__ == "__main__":
    main(*sys.argv[1:4])
