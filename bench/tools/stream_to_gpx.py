#!/usr/bin/env python3
"""Convertit un stream Strava (JSON : location, altitude, distance, time) en GPX 1.1.

L'altitude écrite est celle de l'enregistrement (baromètre), brute : aucune correction de décalage.
--trim-m N retire les N premiers et N derniers mètres (domicile, habitudes) ; la distance
curviligne de la trace exportée repart alors de 0.

Usage : stream_to_gpx.py stream.json sortie.gpx [--trim-m 1000] [--name NOM]
"""
import argparse
import json
from xml.sax.saxutils import escape


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("stream")
    ap.add_argument("out")
    ap.add_argument("--trim-m", type=float, default=0.0)
    ap.add_argument("--name", default="")
    a = ap.parse_args()
    s = json.load(open(a.stream))
    loc, alt, dist = s["location"], s["altitude"], s["distance"]
    end = dist[-1] - a.trim_m
    keep = [i for i, d in enumerate(dist) if a.trim_m <= d <= end]
    with open(a.out, "w") as f:
        f.write('<?xml version="1.0" encoding="UTF-8"?>\n')
        f.write('<gpx version="1.1" creator="osm-route-details bench" '
                'xmlns="http://www.topografix.com/GPX/1/1">\n')
        f.write(f"<trk><name>{escape(a.name)}</name><trkseg>\n")
        for i in keep:
            f.write(f'<trkpt lat="{loc[i][0]:.6f}" lon="{loc[i][1]:.6f}"><ele>{alt[i]:.1f}</ele></trkpt>\n')
        f.write("</trkseg></trk></gpx>\n")


if __name__ == "__main__":
    main()
