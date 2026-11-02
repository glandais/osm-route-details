#!/usr/bin/env python3
"""Instantané des tags OSM des voies d'une référence de revêtement Valhalla, à la date de ses données
(README §4 bis.8 : erreur de la référence, décision de l'utilisateur, 2026-11-02).

Lit les fichiers <id>.surface.csv donnés (sortie de stadia_match.py) et retient la colonne way_id
des seules plages où Valhalla rend une classe non revêtue (NON_REVETU : la règle ne regarde que
celles-là), puis demande à Overpass, à la date DATE des données du serveur Valhalla, les tags `surface`, `highway` et `tracktype`
de ces voies : `[date:"DATE"];way(id:…);out tags;`, par lots de BATCH, en séquence, à PAUSE_S
secondes d'écart, User-Agent du banc. Une voie absente à cette date n'a pas d'entrée.

Sortie, hors git (identifiants de voies) : JSON {date, source, voies, tags: {way_id: {surface,
highway, tracktype}}}. Le banc la lit sans appel réseau ; son empreinte va au manifest. Un fichier de
sortie existant est complété (voies déjà présentes non redemandées).
Sortie standard : nombre de voies demandées, trouvées, et de voies en revêtement dur sans ambiguïté.

Usage : osm_tags_reference.py DATE sortie.json fichier.surface.csv ...
"""
import csv
import json
import os
import sys
import time
import urllib.parse
import urllib.request

UA = "osm-route-details-bench/0.1 (usage personnel)"
URL = "https://overpass-api.de/api/interpreter"
BATCH = 250
PAUSE_S = 5.0
KEYS = ("surface", "highway", "tracktype")
NON_REVETU = {"compacted", "dirt", "gravel", "path", "impassable"}
DUR = {"asphalt", "concrete", "concrete:lanes", "concrete:plates", "paving_stones"}   # criteria serie_tenue_a_l_ecart.erreur_reference_R


def query(date, ids):
    q = f'[out:json][timeout:900][date:"{date}"];way(id:{",".join(map(str, ids))});out tags;'
    for attempt in range(6):
        try:
            req = urllib.request.Request(URL, data=urllib.parse.urlencode({"data": q}).encode(), headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=960) as r:
                return json.load(r)["elements"]
        except Exception as e:  # noqa: BLE001
            print(f"lot de {len(ids)} : {type(e).__name__}, nouvel essai", file=sys.stderr, flush=True)
            time.sleep(60 * (attempt + 1))
    raise RuntimeError("Overpass indisponible")


def main(date, out, *paths):
    ids = set()
    for p in paths:
        for r in csv.DictReader(open(p)):
            if r.get("way_id") and r.get("surface") in NON_REVETU:
                ids.add(int(float(r["way_id"])))
    doc = json.load(open(out)) if os.path.exists(out) else {"date": date, "source": URL, "tags": {}}
    if doc["date"] != date:
        raise SystemExit(f"{out} est daté {doc['date']}, pas {date}")
    asked = set(map(int, doc.get("demandees", [])))
    todo = sorted(ids - asked)
    for k in range(0, len(todo), BATCH):
        lot = todo[k:k + BATCH]
        for e in query(date, lot):
            t = e.get("tags", {})
            doc["tags"][str(e["id"])] = {key: t[key] for key in KEYS if key in t}
        asked |= set(lot)
        doc["demandees"] = sorted(asked)
        doc["voies"] = len(doc["tags"])
        json.dump(doc, open(out, "w"), separators=(",", ":"))
        print(f"{min(k + BATCH, len(todo))}/{len(todo)}", file=sys.stderr, flush=True)
        time.sleep(PAUSE_S)
    dur = sum(1 for v in doc["tags"].values() if v.get("surface") in DUR)
    print(json.dumps({"demandees": len(asked), "trouvees": len(doc["tags"]), "revetement_dur": dur}))


if __name__ == "__main__":
    main(*sys.argv[1:])
