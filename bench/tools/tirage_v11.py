#!/usr/bin/env python3
"""Tirage de la série tenue à l'écart V1.1 (décision de l'utilisateur, 2026-11-02), sans téléchargement.

Vivier, lu dans l'inventaire local (private/strava/inventaire.json, polylignes réduites) :
- sorties vélo (Ride, GravelRide, MountainBikeRide) parties depuis le 2014-01-01, en France ;
- jamais passées dans le service : ni dans private/perso-map.csv (MVP) ni dans
  private/v1/perso-map-v1.csv (V1), négatifs compris ;
- au moins MIN_INEDIT_KM km inédits : longueur de la polyligne réduite (densifiée tous les 100 m)
  hors des cellules de CELL_M m (et de leurs 8 voisines) touchées par une trace déjà utilisée.
Ordre : sha256(SEL + id) croissant. Sélection = les N premières ; le reste de l'ordre est la réserve,
lue dans cet ordre si un minimum de couverture n'est pas atteint (jamais changer le sel).
Q1 : sorties déjà qualifiées suspectes (private/strava/qualif*.csv), jamais utilisées, ordre
sha256(SEL + "q1:" + id) ; un négatif compte si son défaut est attesté (D+ enregistré < 0,6 ou > 1,6 ×
D+ MNT, 930f6e8) ; les Q1 premiers attestés.

Sortie (hors git, identifiants Strava) : private/v11/tirage.json. Sortie standard : comptes et km, sans
identifiant ni coordonnée, et l'empreinte du fichier.

Série : variable d'environnement SERIE (v11 par défaut ; v12 : V1.2, décision de l'utilisateur,
2026-11-03, sel osm-route-details:v1.2:, jamais utilisées ni MVP, ni V1, ni V1.1, sortie
private/v12/tirage.json ; Q1 rapporté sur V1.2, README §4 quinquies.1 : Q1 = 0 par défaut).

Usage : [SERIE=v12] tirage_v11.py private/ [N] [Q1]
"""
import csv
import glob
import hashlib
import json
import math
import os
import sys

SERIE = os.environ.get("SERIE", "v11")
SEL = {"v11": "osm-route-details:v1.1:", "v12": "osm-route-details:v1.2:"}[SERIE]
DEJA = {"v11": ["perso-map.csv", "v1/perso-map-v1.csv"],
        "v12": ["perso-map.csv", "v1/perso-map-v1.csv", "v11/perso-map-v11.csv"]}[SERIE]
DEBUT = "2014-01-01"
TYPES = {"Ride", "GravelRide", "MountainBikeRide"}
MIN_INEDIT_KM = 50.0
CELL_M = 200.0
STEP_M = 100.0
ETRANGER = {"United Kingdom", "Kyrgyzstan", "Spain", "Italy", "Austria", "Ireland", "Germany", "Switzerland",
            "Portugal", "Belgium", "Netherlands", "Luxembourg", "Croatia", "Slovenia", "Norway", "Denmark", "Greece",
            "Morocco", "Iceland", "Czechia", "Poland", "Andorra", "Monaco", "Japan", "United States", "Canada",
            "Sweden", "Hungary", "Slovakia", "Romania", "Bulgaria", "Turkey", "Montenegro", "Albania",
            "Bosnia and Herzegovina", "Serbia", "Finland", "Estonia", "Latvia", "Lithuania", "Tunisia"}


def h(x):
    return hashlib.sha256((SEL + x).encode()).hexdigest()


def decode(s):
    i = lat = lng = 0
    out = []
    while i < len(s):
        vals = []
        for _ in (0, 1):
            sh = r = 0
            while True:
                b = ord(s[i]) - 63
                i += 1
                r |= (b & 0x1f) << sh
                sh += 5
                if b < 0x20:
                    break
            vals.append(~(r >> 1) if r & 1 else r >> 1)
        lat += vals[0]
        lng += vals[1]
        out.append((lat / 1e5, lng / 1e5))
    return out


def steps(pts):
    """Points tous les STEP_M m au plus, avec la longueur qu'ils portent."""
    for a, b in zip(pts, pts[1:]):
        d = math.hypot((b[0] - a[0]) * 111320, (b[1] - a[1]) * 111320 * math.cos(math.radians(a[0])))
        n = max(1, int(d // STEP_M))
        for k in range(n):
            yield (a[0] + (b[0] - a[0]) * k / n, a[1] + (b[1] - a[1]) * k / n), d / n


def cell(p):
    c = CELL_M / 111320
    return int(p[0] / c), int(p[1] / (c / math.cos(math.radians(p[0]))))


def en_france(a, pts):
    pays = (a.get("location_summary") or "").split(", ")[-1]
    return bool(a.get("location_summary")) and pays not in ETRANGER and 41.0 <= pts[0][0] <= 51.6 and -5.5 <= pts[0][1] <= 9.8


def main(B, n=45, q1=2 if SERIE == "v11" else 0):
    B = B.rstrip("/")
    n, q1 = int(n), int(q1)
    inv = json.load(open(f"{B}/strava/inventaire.json"))
    used = set()
    for f in (f"{B}/{x}" for x in DEJA):
        used |= {r["strava_id"] for r in csv.DictReader(open(f))}
    grid = set()
    for a in inv:
        if str(a["id"]) in used and a.get("reduced_polyline"):
            for p, _ in steps(decode(a["reduced_polyline"])):
                c = cell(p)
                grid |= {(c[0] + i, c[1] + j) for i in (-1, 0, 1) for j in (-1, 0, 1)}
    vivier = []
    for a in inv:
        i = str(a["id"])
        if i in used or a.get("sport_type") not in TYPES or a["start_local"][:10] < DEBUT or not a.get("reduced_polyline"):
            continue
        pts = decode(a["reduced_polyline"])
        if len(pts) < 2 or not en_france(a, pts):
            continue
        inedit = sum(d for p, d in steps(pts) if cell(p) not in grid) / 1000
        if inedit >= MIN_INEDIT_KM:
            vivier.append({"id": i, "inedit_km": round(inedit, 1), "km": round(a["summary"]["distance"] / 1000, 1)})
    vivier.sort(key=lambda v: h(v["id"]))
    qual = {}
    for f in sorted(glob.glob(f"{B}/strava/qualif*.csv")):
        qual.update({r["id"]: r for r in csv.DictReader(open(f))})
    sus = sorted((i for i, r in qual.items() if r["verdict"] == "suspect" and i not in used), key=lambda i: h("q1:" + i))

    def atteste(r):
        try:
            x = float(r["gain_rec_m"]) / float(r["gain_dem_m"])
        except (KeyError, ValueError, ZeroDivisionError):
            return False
        return x < 0.6 or x > 1.6
    negatifs = [i for i in sus if atteste(qual[i])][:q1]
    out = {"sel": SEL, "criteres": {"debut": DEBUT, "types": sorted(TYPES), "france": True, "min_inedit_km": MIN_INEDIT_KM,
                                    "cellule_m": CELL_M, "jamais_utilisees": DEJA},
           "ordre": [v["id"] for v in vivier], "vivier": vivier, "selection": [v["id"] for v in vivier[:n]],
           "q1_ordre": sus, "q1": negatifs}
    os.makedirs(f"{B}/{SERIE}", exist_ok=True)
    path = f"{B}/{SERIE}/tirage.json"
    assert not os.path.exists(path), f"{path} existe : tirage figé, jamais réécrit"
    json.dump(out, open(path, "w"), indent=1)
    sel = vivier[:n]
    print(json.dumps({"vivier": len(vivier), "vivier_km": round(sum(v["km"] for v in vivier)),
                      "selection": len(sel), "selection_km": round(sum(v["km"] for v in sel)),
                      "selection_inedit_km": round(sum(v["inedit_km"] for v in sel)),
                      "q1_suspectes": len(sus), "q1_attestes_retenus": len(negatifs),
                      "sha256": hashlib.sha256(open(path, "rb").read()).hexdigest()}, ensure_ascii=False))


if __name__ == "__main__":
    main(*sys.argv[1:])
