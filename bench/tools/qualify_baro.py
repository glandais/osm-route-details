#!/usr/bin/env python3
"""Qualifie l'altitude enregistrée d'une activité (stream Strava) contre le MNT de l'IGN.

Pour chaque activité : rééchantillonnage tous les STEP_M mètres, lecture du MNT (RGE ALTI, API alti
de la Géoplateforme), résidu = altitude enregistrée − MNT. On en tire :
- offset_m : médiane du résidu (décalage de datum ou d'étalonnage du baromètre) ;
- drift_m : amplitude de la tendance lente du résidu (médiane glissante sur 5 km), signe d'un
  baromètre qui dérive avec la météo ou d'un capteur défaillant ;
- noise_m : écart robuste (1,4826 × MAD) du résidu après retrait de la tendance ;
- p95_abs_m : 95e centile de |résidu − tendance| ;
- gain_rec_m / gain_dem_m : D+ de l'enregistrement (hystérésis 2 m, preset « barometric ») et du
  MNT (hystérésis 3 m, preset « dem »), sur la même grille.
Hors couverture IGN (valeurs ≤ −9000), la station est ignorée ; la part couverte est rapportée.

Un enregistrement est dit « fiable » si noise_m ≤ 2,5, p95_abs_m ≤ 8, drift_m ≤ 60 et la couverture
≥ 90 %. La dérive tolérée est large : sur une journée en montagne, la météo déplace la pression de
quelques hPa, soit 30 à 50 m (42 m mesurés sur une sortie Cantal saine, bruit 0,9 m). Une dérive lente
déforme peu le D+ ; un baromètre défaillant se voit surtout au bruit et aux sauts.
Ces bornes servent au tri du corpus, pas à l'évaluation du service.

Entrée : fichiers JSON de streams (clés location, altitude, distance) nommés <activity_id>.json.
Sortie : une ligne CSV par activité sur la sortie standard.

Usage : qualify_baro.py streams/*.json > qualif.csv
"""
import json
import math
import os
import statistics
import sys
import time
import urllib.parse
import urllib.request

API = "https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json"
UA = "osm-route-details-bench/0.1 (usage personnel)"
STEP_M = 50.0
BATCH = 150
MIN_INTERVAL_S = 0.25
TREND_WINDOW_M = 5000.0
_last = 0.0


def ign(points):
    global _last
    out = []
    for i in range(0, len(points), BATCH):
        chunk = points[i:i + BATCH]
        q = urllib.parse.urlencode({
            "lon": "|".join(f"{p[1]:.6f}" for p in chunk),
            "lat": "|".join(f"{p[0]:.6f}" for p in chunk),
            "resource": "ign_rge_alti_wld",
            "zonly": "true",
        })
        for attempt in range(4):
            wait = MIN_INTERVAL_S - (time.monotonic() - _last)
            if wait > 0:
                time.sleep(wait)
            _last = time.monotonic()
            try:
                req = urllib.request.Request(f"{API}?{q}", headers={"User-Agent": UA})
                with urllib.request.urlopen(req, timeout=60) as r:
                    out += json.load(r)["elevations"]
                break
            except Exception:  # noqa: BLE001 — relance avec attente croissante
                if attempt == 3:
                    raise
                time.sleep(2 ** attempt)
    return out


def resample(dist, loc, alt, step):
    res = []
    j = 0
    s = dist[0]
    while s <= dist[-1]:
        while j + 1 < len(dist) and dist[j + 1] < s:
            j += 1
        if j + 1 >= len(dist):
            break
        d0, d1 = dist[j], dist[j + 1]
        t = 0.0 if d1 == d0 else (s - d0) / (d1 - d0)
        lat = loc[j][0] + t * (loc[j + 1][0] - loc[j][0])
        lon = loc[j][1] + t * (loc[j + 1][1] - loc[j][1])
        z = alt[j] + t * (alt[j + 1] - alt[j])
        res.append((s, lat, lon, z))
        s += step
    return res


def hysteresis_gain(z, threshold):
    gain = 0.0
    ref = None
    for v in z:
        if ref is None:
            ref = v
        elif v - ref >= threshold:
            gain += v - ref
            ref = v
        elif v < ref:
            ref = v
    return gain


def rolling_median(values, half):
    out = []
    for i in range(len(values)):
        out.append(statistics.median(values[max(0, i - half):i + half + 1]))
    return out


def qualify(path):
    d = json.load(open(path))
    loc, alt, dist = d.get("location"), d.get("altitude"), d.get("distance")
    if not (loc and alt and dist):
        return None
    pts = resample(dist, loc, alt, STEP_M)
    dem = ign([(p[1], p[2]) for p in pts])
    pairs = [(p, z) for p, z in zip(pts, dem) if z is not None and z > -9000]
    cov = len(pairs) / len(pts) if pts else 0.0
    if len(pairs) < 50:
        return dict(n=len(pts), coverage=round(cov, 3))
    rec = [p[3] for p, _ in pairs]
    mnt = [z for _, z in pairs]
    resid = [a - b for a, b in zip(rec, mnt)]
    half = int(TREND_WINDOW_M / STEP_M / 2)
    trend = rolling_median(resid, half)
    detr = [r - t for r, t in zip(resid, trend)]
    med = statistics.median(detr)
    mad = statistics.median(abs(x - med) for x in detr)
    absd = sorted(abs(x) for x in detr)
    return dict(
        n=len(pts),
        coverage=round(cov, 3),
        offset_m=round(statistics.median(resid), 2),
        drift_m=round(max(trend) - min(trend), 1),
        noise_m=round(1.4826 * mad, 2),
        p95_abs_m=round(absd[int(0.95 * (len(absd) - 1))], 1),
        max_abs_m=round(absd[-1], 1),
        gain_rec_m=round(hysteresis_gain(rec, 2.0)),
        gain_dem_m=round(hysteresis_gain(mnt, 3.0)),
        dist_km=round(dist[-1] / 1000, 1),
        flat_frac=round(sum(1 for a, b in zip(rec, rec[1:]) if a == b) / max(1, len(rec) - 1), 3),
    )


def main(paths):
    cols = ["id", "n", "coverage", "offset_m", "drift_m", "noise_m", "p95_abs_m", "max_abs_m",
            "gain_rec_m", "gain_dem_m", "dist_km", "flat_frac", "verdict"]
    print(",".join(cols))
    for p in paths:
        aid = os.path.splitext(os.path.basename(p))[0]
        try:
            q = qualify(p)
        except Exception as e:  # noqa: BLE001
            print(f"{aid}: {e}", file=sys.stderr)
            continue
        if q is None:
            continue
        ok = (q.get("coverage", 0) >= 0.9 and q.get("noise_m", math.inf) <= 2.5
              and q.get("p95_abs_m", math.inf) <= 8 and q.get("drift_m", math.inf) <= 60)
        q["verdict"] = "fiable" if ok else ("hors_ign" if q.get("coverage", 0) < 0.9 else "suspect")
        print(",".join(str(q.get(c, aid if c == "id" else "")) for c in cols), flush=True)


if __name__ == "__main__":
    main(sys.argv[1:])
