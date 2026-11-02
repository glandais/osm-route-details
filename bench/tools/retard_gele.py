#!/usr/bin/env python3
"""Retard du baro gelé de la calibration V1 (README §4 ter.9, extraits au jugement ; décision de
l'utilisateur, 2026-11-02).

Lit `cas[].retard_baro` d'un rapport de calibration commité et écrit, pour chaque cas qui en porte
un, le δ estimé sur la trace entière : au jugement, les extraits d'une trace de calibration reçoivent
ce δ, sans réestimation. Le fichier ne contient ni coordonnée ni identifiant Strava : pseudonyme,
δ et éléments de l'estimation seulement.

Sortie : JSON {source: {rapport, sha256, commit, criteria_sha256, manifest_sha256},
cas: {id: {retard_m, minimum_m, ecart_robuste_0_m, ecart_robuste_min_m, stations, non_estimable}}}.
Sortie standard : nombre de cas, estimés et non estimables, empreinte du fichier écrit.

Usage : retard_gele.py reports/<campagne>/results.json data/retard-baro-v1.json
"""
import hashlib
import json
import sys


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def main(rapport, sortie):
    d = json.load(open(rapport))
    c = d["campagne"]
    cas = {}
    for x in d["cas"]:
        r = x.get("retard_baro")
        if r is None:
            continue
        cas[x["id"]] = {k: r.get(k) for k in ("retard_m", "minimum_m", "ecart_robuste_0_m", "ecart_robuste_min_m",
                                              "stations", "non_estimable")}
    out = {"source": {"rapport": rapport, "sha256": sha256(rapport), "commit": c["commit"],
                      "revision_banc": c.get("revision_banc"), "criteria_sha256": c["criteria_sha256"],
                      "manifest_sha256": c["manifest_sha256"]},
           "cas": dict(sorted(cas.items()))}
    with open(sortie, "w") as f:
        json.dump(out, f, ensure_ascii=False, indent=1)
        f.write("\n")
    n_est = sum(1 for v in cas.values() if v["retard_m"] is not None)
    print(json.dumps({"cas": len(cas), "estimes": n_est, "non_estimables": len(cas) - n_est,
                      "sha256": sha256(sortie)}, ensure_ascii=False))


if __name__ == "__main__":
    main(*sys.argv[1:])
