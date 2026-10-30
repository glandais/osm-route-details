#!/usr/bin/env python3
"""Expurge un export tribly (.route.json) d'osm-surface pour le banc.

Garde la géométrie et la référence de revêtement Stadia/Valhalla (tracks[0].line, tracks[0].surfaces),
plus distance, D+/D− et état ; retire identifiants, équipe, médias et URL de téléchargement tribly,
waypoints, tags, visibilité et dates.

Usage : expurge_route_json.py entree.route.json sortie.json
"""
import json
import sys

GARDER = ["name", "slug", "distance", "elevationGain", "elevationLoss", "elevationState",
          "surfaceType", "start", "end"]
GARDER_TRACK = ["line", "surfaces", "surfaceState"]


def main(src, dst):
    d = json.load(open(src))
    out = {k: d[k] for k in GARDER if k in d}
    out["tracks"] = [{k: t[k] for k in GARDER_TRACK if k in t} for t in d.get("tracks", [])]
    json.dump(out, open(dst, "w"), ensure_ascii=False, separators=(",", ":"))


if __name__ == "__main__":
    main(*sys.argv[1:3])
