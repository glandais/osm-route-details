#!/usr/bin/env python3
"""Balayage grossier de l'inventaire Strava : D+ annoncé par Strava face au D+ du RGE ALTI (IGN)
le long de la polyligne réduite de chaque activité.

Sert à repérer une période où l'altitude enregistrée n'est pas fiable (baromètre KO) : le rapport
D+ Strava / D+ MNT y décroche de sa tendance. Ce n'est pas une mesure de qualité, seulement un tri.

Entrée : inventaire JSON (liste d'activités de search_activities, avec reduced_polyline).
Sortie : CSV id,date,lat,lon,dist_km,strava_gain,dem_gain,n_pts,ratio.
Seules les activités dont le départ est en France métropolitaine sont interrogées.

Usage : strava_scan_dplus.py inventaire.json sortie.csv
"""
import csv
import json
import sys
import time
import urllib.parse
import urllib.request

API = "https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json"
UA = "osm-route-details-bench/0.1 (usage personnel)"
MIN_INTERVAL_S = 0.25  # 4 req/s, sous la limite de 5 req/s
GAIN_THRESHOLD_M = 5.0  # hystérésis du D+ sur la polyligne réduite


def decode_polyline(s):
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
        out.append((lat / 1e5, lng / 1e5))
    return out


def in_france(lat, lon):
    return 41.3 <= lat <= 51.2 and -5.3 <= lon <= 9.7


def ign_elevations(points):
    q = urllib.parse.urlencode({
        "lon": "|".join(f"{p[1]:.5f}" for p in points),
        "lat": "|".join(f"{p[0]:.5f}" for p in points),
        "resource": "ign_rge_alti_wld",
        "zonly": "true",
    })
    req = urllib.request.Request(f"{API}?{q}", headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)["elevations"]


def hysteresis_gain(z, threshold):
    gain = 0.0
    ref = None
    for v in z:
        if v is None or v <= -9000:
            continue
        if ref is None:
            ref = v
        elif v - ref >= threshold:
            gain += v - ref
            ref = v
        elif v < ref:
            ref = v
    return gain


def main(inv_path, out_path):
    acts = json.load(open(inv_path))
    done = set()
    try:
        with open(out_path) as f:
            done = {row["id"] for row in csv.DictReader(f)}
    except FileNotFoundError:
        pass
    new = not done
    with open(out_path, "a", newline="") as f:
        w = csv.writer(f)
        if new:
            w.writerow(["id", "date", "lat", "lon", "dist_km", "strava_gain", "dem_gain", "n_pts", "ratio"])
        last = 0.0
        for a in acts:
            if a["id"] in done or not a.get("reduced_polyline"):
                continue
            pts = decode_polyline(a["reduced_polyline"])
            if len(pts) < 10 or not in_france(*pts[0]):
                continue
            # Une requête GET reste courte sous ~400 points.
            step = max(1, len(pts) // 400 + 1)
            pts = pts[::step]
            wait = MIN_INTERVAL_S - (time.monotonic() - last)
            if wait > 0:
                time.sleep(wait)
            last = time.monotonic()
            try:
                z = ign_elevations(pts)
            except Exception as e:  # noqa: BLE001 — on note et on continue
                print(f"{a['id']}: {e}", file=sys.stderr)
                continue
            s = a["summary"]
            dem = hysteresis_gain(z, GAIN_THRESHOLD_M)
            ratio = s["elevation_gain"] / dem if dem > 0 else ""
            w.writerow([a["id"], a["start_local"][:10], f"{pts[0][0]:.3f}", f"{pts[0][1]:.3f}",
                        round(s["distance"] / 1000, 1), s["elevation_gain"], round(dem), len(pts),
                        round(ratio, 3) if ratio != "" else ""])
            f.flush()


if __name__ == "__main__":
    main(*sys.argv[1:3])
