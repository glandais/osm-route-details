#!/usr/bin/env python3
"""Remplit le cache du banc avec les tuiles VersaTiles z14 (Shortbread) de l'emprise d'une série,
sans lancer le service (README §4 ter.4 : tuiles de la campagne V1, datées ici).

Tuiles : celle de chaque point des traces données (rééchantillonnées tous les STEP_M m) et ses
8 voisines, comme le voisinage lu par le banc (§4 bis.6). Une tuile déjà en cache n'est pas
retéléchargée. Écriture au format du cache disque du service (crates/ord-data/src/disk.rs) :
`ORDT` + version 1 + longueur (u32 LE) + en-tête JSON + corps MVT décompressé, somme xxh3 ;
200 → body, 204 → empty (politique VERSATILES), 404 et erreurs → rien d'écrit.
TTL : 7 jours, comme les tuiles écrites par le service.

Sortie standard : nombre de tuiles (présentes, téléchargées, vides, en échec), dates de
téléchargement et ETag distincts (sans coordonnées).

Usage : uv run --with xxhash tools/tuiles_v1.py cache_dir trace.gpx|stream.json ...
"""
import gzip
import json
import math
import os
import re
import sys
import tempfile
import time
import urllib.error
import urllib.request

import xxhash

URL = "https://tiles.versatiles.org/tiles/osm/{z}/{x}/{y}"
NAMESPACE = "versatiles-shortbread-z14-d2e31ed09602913b"   # espace de noms du service pour cette URL
UA = "osm-route-details-bench/0.1 (usage personnel)"
Z = 14
STEP_M = 50.0
TTL_S = 604800


def points(path):
    if path.endswith(".json"):
        return [tuple(p) for p in json.load(open(path))["location"] if p]
    return [(float(a), float(b)) for a, b in re.findall(r'lat="([-\d.]+)"\s+lon="([-\d.]+)"', open(path).read())]


def tile_of(lat, lon):
    n = 2 ** Z
    return int((lon + 180) / 360 * n), int((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n)


def tiles(paths):
    out = set()
    for p in paths:
        last = None
        for q in points(p):
            if last and math.hypot((q[0] - last[0]) * 111320, (q[1] - last[1]) * 111320 * math.cos(math.radians(q[0]))) < STEP_M:
                continue
            last = q
            x, y = tile_of(*q)
            out |= {(x + dx, y + dy) for dx in (-1, 0, 1) for dy in (-1, 0, 1)}
    return out


def write(path, issue, etag, last_modified, fetched_at, max_age, body):
    header = json.dumps({"issue": issue, "etag": etag, "last_modified": last_modified, "fetched_at_s": fetched_at,
                         "server_max_age_s": max_age, "ttl_s": TTL_S, "body_len": len(body),
                         "body_xxh3": xxhash.xxh3_64_intdigest(body)}, separators=(",", ":")).encode()
    os.makedirs(os.path.dirname(path), exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path))
    with os.fdopen(fd, "wb") as f:
        f.write(b"ORDT" + bytes([1]) + len(header).to_bytes(4, "little") + header + body)
    os.replace(tmp, path)


def main(cache, *paths):
    root = os.path.join(cache, NAMESPACE, str(Z))
    todo = sorted(tiles(paths))
    stats = {"tuiles": len(todo), "presentes": 0, "telechargees": 0, "vides": 0, "echecs": 0}
    etags, dates = set(), set()
    for x, y in todo:
        path = os.path.join(root, str(x), f"{y}.tile")
        if os.path.exists(path):
            stats["presentes"] += 1
            continue
        req = urllib.request.Request(URL.format(z=Z, x=x, y=y), headers={"User-Agent": UA, "Accept-Encoding": "gzip"})
        for attempt in range(4):
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    body = r.read()
                    if r.headers.get("Content-Encoding") == "gzip" or body[:2] == b"\x1f\x8b":
                        body = gzip.decompress(body)
                    cc = r.headers.get("Cache-Control") or ""
                    m = re.search(r"max-age=(\d+)", cc)
                    now = int(time.time())
                    if r.status == 204 or not body:
                        write(path, "empty", r.headers.get("ETag"), r.headers.get("Last-Modified"), now, int(m.group(1)) if m else None, b"")
                        stats["vides"] += 1
                    else:
                        write(path, "body", r.headers.get("ETag"), r.headers.get("Last-Modified"), now, int(m.group(1)) if m else None, body)
                        stats["telechargees"] += 1
                    etags.add(r.headers.get("ETag")); dates.add(time.strftime("%Y-%m-%d", time.gmtime(now)))
                break
            except urllib.error.HTTPError as e:
                if e.code == 404:
                    stats["echecs"] += 1; break
                time.sleep(5 * (attempt + 1))
            except Exception:  # noqa: BLE001
                time.sleep(5 * (attempt + 1))
        else:
            stats["echecs"] += 1
        time.sleep(0.05)
    stats["dates_telechargement"] = sorted(dates)
    stats["etags_distincts"] = len(etags)
    print(json.dumps(stats, ensure_ascii=False))


if __name__ == "__main__":
    main(*sys.argv[1:])
