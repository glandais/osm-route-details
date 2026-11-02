#!/usr/bin/env python3
"""Traces de la série V1.1 : pseudonymes et GPX, avant le recalage (README §4 ter.4, décision de
l'utilisateur, 2026-11-02).

Sorties fiables de la sélection (private/strava/qualif_v11.csv, verdict fiable) : perso-NN numérotés à
la suite de V1 par date de départ puis identifiant ; négatifs Q1 du tirage figé : neg-NN à la suite.
Écrit, hors git : private/traces/<pseudo>.gpx et <pseudo>.trim1km.gpx (stream_to_gpx.py, 1 km retiré
à chaque bout) et private/v11/perso-map-v11.csv (pseudonyme → identifiant Strava, qualification,
empreintes). Sortie standard : comptes seulement.

Usage : traces_v11.py private/
"""
import csv
import hashlib
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def sha256(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def last(prefix, B):
    n = 0
    for f in (f"{B}/perso-map.csv", f"{B}/v1/perso-map-v1.csv"):
        for r in csv.DictReader(open(f)):
            if r["pseudo"].startswith(prefix):
                n = max(n, int(r["pseudo"].split("-")[1]))
    return n


def main(B):
    B = B.rstrip("/")
    inv = {str(a["id"]): a for a in json.load(open(f"{B}/strava/inventaire.json"))}
    t = json.load(open(f"{B}/v11/tirage.json"))
    qual = {r["id"]: r for r in csv.DictReader(open(f"{B}/strava/qualif_v11.csv"))}
    for f in ("qualif_lot1.csv", "qualif_lot2.csv", "qualif_lot3.csv", "qualif_lot4.csv", "qualif_q1.csv", "qualif_v1.csv"):
        if os.path.exists(f"{B}/strava/{f}"):
            qual.update({r["id"]: r for r in csv.DictReader(open(f"{B}/strava/{f}")) if r["id"] in t["q1"]})
    start = lambda i: (inv[i]["start_local"], i)
    fiables = sorted((i for i in t["selection"] if qual.get(i, {}).get("verdict") == "fiable"), key=start)
    negs = sorted(t["q1"], key=start)
    pseudo = {i: f"perso-{k}" for k, i in enumerate(fiables, last("perso-", B) + 1)}
    pseudo.update({i: f"neg-{k:02d}" for k, i in enumerate(negs, last("neg-", B) + 1)})
    rows = []
    for i in fiables + negs:
        p = pseudo[i]
        src = f"{B}/strava/streams-v11/{i}.json"
        full, trim = f"{B}/traces/{p}.gpx", f"{B}/traces/{p}.trim1km.gpx"
        for out, extra in ((full, []), (trim, ["--trim-m", "1000"])):
            subprocess.run([sys.executable, "-I", os.path.join(HERE, "stream_to_gpx.py"), src, out, "--name", p, *extra], check=True)
        a, q = inv[i], qual[i]
        rows.append({"pseudo": p, "strava_id": i, "role": "Q1" if i in t["q1"] else "trace", "serie": "tenue_a_l_ecart",
                     "mois": a["start_local"][:7], "lieu": (a.get("location_summary") or "").split(", ")[-1],
                     "dist_km": q.get("dist_km"), "verdict": q.get("verdict"), "offset_m": q.get("offset_m"),
                     "drift_m": q.get("drift_m"), "noise_m": q.get("noise_m"), "p95_abs_m": q.get("p95_abs_m"),
                     "gain_rec_m": q.get("gain_rec_m"), "gain_dem_m": q.get("gain_dem_m"), "sha256_stream": sha256(src),
                     "sha256_gpx": sha256(full), "sha256_gpx_trim": sha256(trim)})
    with open(f"{B}/v11/perso-map-v11.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    print(json.dumps({"traces": len(fiables), "negatifs": len(negs), "premier": pseudo[fiables[0]], "dernier": pseudo[fiables[-1]]}))


if __name__ == "__main__":
    main(*sys.argv[1:])
