#!/usr/bin/env python3
"""Construit le corpus de traces barométriques personnelles à partir de la sélection.

Pour chaque ligne de selection.csv (pseudo, strava_id, groupe, cas) :
- écrit <pseudo>.gpx (trace entière) et <pseudo>.trim1km.gpx (1 km retiré à chaque bout) ;
- calcule leur sha256 ;
- attribue la série (calibration ou tenue à l'écart) par une règle fixée d'avance :
  tenue à l'écart si le premier chiffre hexadécimal de sha256("osm-route-details:" + pseudo)
  est dans 0–4 (environ 5/16 des cas). Les négatifs et le hors France restent en calibration ;
- reprend les indicateurs de qualify_baro.py.

Sorties : <dir>/perso-map.csv (avec l'identifiant Strava : reste hors git) et, sur la sortie
standard, le fragment YAML du manifest (sans identifiant Strava, date au mois, département).

Usage : build_perso_corpus.py selection.csv inventaire.json streams_dir traces_dir qualif1.csv [...]
"""
import csv
import hashlib
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 16), b""):
            h.update(b)
    return h.hexdigest()


def serie(pseudo, groupe):
    if groupe in ("negatif", "hors_france"):
        return "calibration"
    d = hashlib.sha256(f"osm-route-details:{pseudo}".encode()).hexdigest()[0]
    return "tenue_a_l_ecart" if d in "01234" else "calibration"


def main(sel_path, inv_path, streams_dir, traces_dir, *qualif_paths):
    inv = {a["id"]: a for a in json.load(open(inv_path))}
    q = {}
    for p in qualif_paths:
        for r in csv.DictReader(open(p)):
            q[r["id"]] = r
    os.makedirs(traces_dir, exist_ok=True)
    rows = []
    for s in csv.DictReader(open(sel_path)):
        sid, pseudo = s["strava_id"], s["pseudo"]
        src = os.path.join(streams_dir, f"{sid}.json")
        full = os.path.join(traces_dir, f"{pseudo}.gpx")
        trim = os.path.join(traces_dir, f"{pseudo}.trim1km.gpx")
        for out, extra in ((full, []), (trim, ["--trim-m", "1000"])):
            subprocess.run([sys.executable, "-I", os.path.join(HERE, "stream_to_gpx.py"), src, out,
                            "--name", pseudo, *extra], check=True)
        a = inv.get(sid, {})
        qq = q.get(sid, {})
        rows.append(dict(
            pseudo=pseudo, strava_id=sid, groupe=s["groupe"], cas=s["cas"],
            mois=a.get("start_local", "")[:7],
            lieu=(a.get("location_summary") or "").split(", ")[-1],
            dist_km=qq.get("dist_km") or round(a.get("summary", {}).get("distance", 0) / 1000, 1),
            serie=serie(pseudo, s["groupe"]),
            verdict=qq.get("verdict", ""), offset_m=qq.get("offset_m", ""),
            drift_m=qq.get("drift_m", ""), noise_m=qq.get("noise_m", ""),
            p95_abs_m=qq.get("p95_abs_m", ""), gain_rec_m=qq.get("gain_rec_m", ""),
            gain_dem_m=qq.get("gain_dem_m", ""),
            sha256_stream=sha256(src), sha256_gpx=sha256(full), sha256_gpx_trim=sha256(trim),
            taille_gpx_o=os.path.getsize(full),
        ))
    with open(os.path.join(os.path.dirname(traces_dir.rstrip("/")), "perso-map.csv"), "w",
              newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    for r in rows:
        print(f"  - id: {r['pseudo']}")
        print(f"    groupe: {r['groupe']}")
        print(f"    serie: {r['serie']}")
        print(f"    cas: \"{r['cas']}\"")
        print(f"    region: \"{r['lieu']}\"")
        print(f"    mois: \"{r['mois']}\"")
        print(f"    dist_km: {r['dist_km']}")
        print(f"    fichier: private/traces/{r['pseudo']}.gpx")
        print(f"    sha256: {r['sha256_gpx']}")
        print(f"    sha256_trim1km: {r['sha256_gpx_trim']}")
        if r["verdict"]:
            print(f"    qualification: {{verdict: {r['verdict']}, offset_m: {r['offset_m'] or 'null'}, "
                  f"drift_m: {r['drift_m'] or 'null'}, noise_m: {r['noise_m'] or 'null'}, "
                  f"p95_abs_m: {r['p95_abs_m'] or 'null'}, gain_baro_m: {r['gain_rec_m'] or 'null'}, "
                  f"gain_mnt_m: {r['gain_dem_m'] or 'null'}}}")


if __name__ == "__main__":
    main(*sys.argv[1:])
