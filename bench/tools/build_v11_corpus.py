#!/usr/bin/env python3
"""Construit les cas de la série tenue à l'écart V1.1 (décision de l'utilisateur, 2026-11-02 ; tout
tenu à l'écart, MVP et V1 en calibration) à partir du tirage figé et du contrôle de couverture.

Entrées (bench/private/) : v11/couverture-cas.json, couverture-ign.json, affectation.json
(couverture_v1.py, SERIE=v11), v11/perso-map-v11.csv (traces_v11.py : pseudonymes et GPX déjà
écrits, empreintes revérifiées ici), v11/planifies/, v11/r12/ ; éléments de calibration :
data/ouvrages.yaml, ouvrages-perso.json, v1/ouvrages-v1.json, v1/temoins-v1.json.

Sorties, hors git (bench/private/v11/) :
- ouvrages-v11.json : ouvrages comptés, nv-NN numérotés à la suite de V1 (par type puis longueur
  décroissante), puis croisements E5 en `passage sous pont`, même schéma que ouvrages-v1.json ;
- temoins-v11.json : témoins S2 (tronçons de sorties : extrémités ; itinéraires planifiés : entiers) ;
- series-v11.csv : id, type, groupe, serie, passages_masques, fenetres_calibration.
  passages_masques : éléments V1.1 d'une autre série que la trace (README §2, règle 4 ; aucun, tout
  est tenu à l'écart). fenetres_calibration : éléments de calibration (MVP, V1) dont tous les points
  sont à moins de TOL_M de la trace, fenêtres ± 500 m de la série calibration au sens du README §2 ;
  pour information, que le banc ne lit pas : aucun masque au jugement V1.1 et aucun extrait de
  calibration tiré des traces V1.1 (décisions de l'utilisateur, 2026-11-02 ; README §4 ter.10).

Sortie standard : fragment YAML du manifest (section perso_v11), sans identifiant Strava, de voie
ni coordonnée ; département et mois seulement.

Série : variable d'environnement SERIE (v11 par défaut ; v12 : V1.2, décision de l'utilisateur,
2026-11-03, MVP, V1 et V1.1 en calibration, nv-NN à la suite de V1.1).

Usage : [SERIE=v12] uv run --with pyyaml tools/build_v11_corpus.py private/
"""
import csv
import hashlib
import json
import math
import os
import sys

import yaml

HERE = os.path.dirname(os.path.abspath(__file__))
TOL_M = 40.0
SERIE = os.environ.get("SERIE", "v11")
CALIB = {"v11": ["v1"], "v12": ["v1", "v11"]}[SERIE]   # séries passées en calibration


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 16), b""):
            h.update(b)
    return h.hexdigest()


def hav(a, b):
    k = 111320.0
    return math.hypot((a[0] - b[0]) * k, (a[1] - b[1]) * k * math.cos(math.radians(a[0])))


def near_track(track, p, tol):
    d = tol / 111320.0 * 2
    return any(abs(q[0] - p[0]) < d and abs(q[1] - p[1]) < d * 2 and hav(q, p) < tol for q in track)


def calibration_elements(B):
    """(id, points) des éléments de calibration : corpus versionné, MVP, V1 (toute la série V1)."""
    out = []
    for o in yaml.safe_load(open(f"{B}/../data/ouvrages.yaml"))["ouvrages"]:
        if o.get("points"):
            out.append((o["id"], [tuple(p) for p in o["points"].values()]))
    for f in [f"{B}/ouvrages-perso.json"] + [f"{B}/{d}/ouvrages-{d}.json" for d in CALIB]:
        out += [(o["id"], [tuple(p) for p in o["pts"].values()]) for o in json.load(open(f))]
    for d in CALIB:
        out += [(t["id"], [tuple(t["debut"]), tuple(t["fin"])]) for t in json.load(open(f"{B}/{d}/temoins-{d}.json"))
                if not t["planifie"] and "debut" in t]
    return out


def main(B):
    B = B.rstrip("/")
    C = json.load(open(f"{B}/{SERIE}/couverture-cas.json"))
    G = json.load(open(f"{B}/{SERIE}/couverture-ign.json"))
    A = json.load(open(f"{B}/{SERIE}/affectation.json"))
    pm = list(csv.DictReader(open(f"{B}/{SERIE}/perso-map-{SERIE}.csv")))
    pseudo = {r["strava_id"]: r["pseudo"] for r in pm}
    n_nv = max(int(o["id"].split("-")[1]) for o in json.load(open(f"{B}/{CALIB[-1]}/ouvrages-{CALIB[-1]}.json")))
    resume = json.load(open(f"{B}/{SERIE}/planifies/resume.json"))
    planifies = {t["id"]: t for t in C["traces"] if t["zone"] == "planifie" and t["id"] in resume}

    # ouvrages comptés → nv-NN, à la suite de V1
    comptes = sorted(A["ouvrages"], key=lambda k: (C["ouvrages"][k]["type"], -C["ouvrages"][k]["length_m"], k))
    nv = {}
    for k, oid in enumerate(comptes, n_nv + 1):
        nv[oid] = f"nv-{k:02d}"
    xs = sorted(A["croisements"], key=lambda k: C["croisements"][k]["way"])
    for k, cid in enumerate(xs, n_nv + len(comptes) + 1):
        nv[cid] = f"nv-{k:02d}"
    ouv = []
    for oid in comptes:
        o = C["ouvrages"][oid]
        tr = [p["trace"] for p in o["passages"] if not p["exclusions"]]
        ouv.append({"id": nv[oid], "name": "", "type": o["type"], "chain": o["ways"], "extra": [], "region": "",
                    "cas": "Ouvrage V1.1 franchi par une trace de la série V1.1 (perso ou planifiée)", "vc": "", "notes": "",
                    "pts_from": "osm", "length_m": round(o["length_m"]), "mid": o["pts"]["milieu"], "pts": o["pts"],
                    "tags": {str(w): o["tags"] for w in o["ways"][:1]},
                    "trace": pseudo.get(tr[0], tr[0]) if tr else "",
                    "milieu_mns_mediane_25m": None, "mnh_milieu_m": (G.get(oid) or {}).get("mnh_m"),
                    "serie": A["ouvrages"][oid], "groupe_tirage": A["groupes_ouvrages"].get(oid)})
    for cid in xs:
        c = C["croisements"][cid]
        ouv.append({"id": nv[cid], "name": "", "type": "passage sous pont", "chain": [], "extra": [c["way"]],
                    "region": "", "cas": "Croisement V1.1 sous un pont (E5)", "vc": "", "notes": "voie du dessous à lire dans les tuiles",
                    "cross": [c["way"], None], "pts_from": [], "length_m": 0, "mid": c["pt"], "pts": {"croisement": c["pt"]},
                    "tags": {}, "rail": c["rail"], "trace": pseudo.get(c["traces"][0], c["traces"][0]),
                    "serie": A["croisements"][cid], "groupe_tirage": A["groupes_ouvrages"].get(cid)})
    json.dump(ouv, open(f"{B}/{SERIE}/ouvrages-{SERIE}.json", "w"), ensure_ascii=False)
    tem = [dict(t, trace=pseudo.get(t["trace"], t["trace"]), serie=A["temoins"][t["id"]],
                id=(t["id"] if t["planifie"] else t["id"].replace(t["trace"], pseudo.get(t["trace"], t["trace"]))))
           for t in C["temoins"] if t["id"] in A["temoins"]]
    json.dump(tem, open(f"{B}/{SERIE}/temoins-{SERIE}.json", "w"), ensure_ascii=False)

    # traces : GPX de traces_v11.py, empreintes revérifiées
    rows = []
    for r in pm:
        for k, f in (("sha256_gpx", f"{B}/traces/{r['pseudo']}.gpx"), ("sha256_gpx_trim", f"{B}/traces/{r['pseudo']}.trim1km.gpx"),
                     ("sha256_stream", f"{B}/strava/streams-{SERIE}/{r['strava_id']}.json")):
            if sha256(f) != r[k]:
                raise SystemExit(f"{r['pseudo']} : empreinte {k} différente de perso-map-{SERIE}.csv")
        rows.append(dict(r, groupe=A["groupes_traces"].get(r["strava_id"], "negatif"), serie=A["traces"][r["strava_id"]]))

    # fenêtres : éléments à moins de TOL_M de tous leurs points
    elems = [(o["id"], o["serie"], [tuple(x) for x in o["pts"].values()]) for o in ouv]
    elems += [(t["id"], t["serie"], [tuple(t[k]) for k in ("debut", "fin")] if "debut" in t else []) for t in tem if not t["planifie"]]
    cal = calibration_elements(B)
    series_rows = []
    for r in rows:
        track = [tuple(p) for p in json.load(open(f"{B}/strava/streams-{SERIE}/{r['strava_id']}.json"))["location"] if p]
        own = [t["id"] for t in tem if t["trace"] == r["pseudo"]]
        r["masques"] = sorted(e for e, s, pts in elems if s != r["serie"] and pts and
                              (e in own or all(near_track(track, p, TOL_M) for p in pts)))
        r["fenetres_calibration"] = sorted(e for e, pts in cal if pts and all(near_track(track, p, TOL_M) for p in pts))
        series_rows.append({"id": r["pseudo"], "type": "trace", "groupe": r["groupe"], "serie": r["serie"],
                            "passages_masques": " ".join(r["masques"]), "fenetres_calibration": " ".join(r["fenetres_calibration"])})
    tir = json.load(open(f"{B}/{SERIE}/affectation-tirage.json"))["serie"]
    for i, t in sorted(planifies.items()):
        series_rows.append({"id": i, "type": "planifie", "groupe": t["role"], "serie": tir.get(i, "tenue_a_l_ecart"),
                            "passages_masques": "", "fenetres_calibration": ""})
    for o in ouv:
        series_rows.append({"id": o["id"], "type": "ouvrage", "groupe": o["groupe_tirage"] and nv.get(o["groupe_tirage"], o["groupe_tirage"]),
                            "serie": o["serie"], "passages_masques": "", "fenetres_calibration": ""})
    for t in tem:
        series_rows.append({"id": t["id"], "type": "temoin", "groupe": t["nature"], "serie": t["serie"],
                            "passages_masques": "", "fenetres_calibration": ""})
    with open(f"{B}/{SERIE}/series-{SERIE}.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(series_rows[0])); w.writeheader(); w.writerows(series_rows)

    # fragment de manifest : sans identifiant Strava ni coordonnée
    n_x = len(xs)
    print(f"perso_{SERIE}:")
    print(f"  ouvrages_{SERIE}: {{fichier: private/{SERIE}/ouvrages-{SERIE}.json, sha256: {sha256(f'{B}/{SERIE}/ouvrages-{SERIE}.json')}, nombre: {len(ouv)}, "
          f"ponts_tunnels: {len(comptes)}, croisements_e5: {n_x}, premier: nv-{n_nv + 1}, dernier: nv-{n_nv + len(ouv)}}}")
    print(f"  temoins_{SERIE}: {{fichier: private/{SERIE}/temoins-{SERIE}.json, sha256: {sha256(f'{B}/{SERIE}/temoins-{SERIE}.json')}, nombre: {len(tem)}}}")
    print(f"  series_{SERIE}: {{fichier: private/{SERIE}/series-{SERIE}.csv, sha256: {sha256(f'{B}/{SERIE}/series-{SERIE}.csv')}}}")
    print("  planifies:")
    for i, t in sorted(planifies.items()):
        g = f"{B}/{SERIE}/planifies/{i}.gpx"
        extra = f", nature: {t['nature']}" if t["role"] == "S2" else ""
        retenue = {x["nature"] for x in tem if x["id"] == i and x["nature"]}
        if retenue and retenue != {t["nature"]}:
            extra += f", nature_retenue: {retenue.pop()}"   # nature des tags confirmée (00e500f)
        if t["role"] == "S2" and not any(x["id"] == i and x["nature"] for x in tem):
            extra += ", retire: \"nature non confirmée par OSM (00e500f)\""
        if t["role"] == "R1_R2":
            ref, sf, bo = (f"{B}/{SERIE}/r12/reference/{i}.reference.json", f"{B}/{SERIE}/r12/{i}.surface.csv", f"{B}/{SERIE}/r12/{i}.baro.csv")
            extra += (f", reference: private/{SERIE}/r12/reference/{i}.reference.json, sha256_reference: {sha256(ref)}, "
                      f"sha256_surface: {sha256(sf)}, sha256_baro: {sha256(bo)}")
        print(f"    - {{id: {i}, role: {t['role']}, km: {t['km']}, fichier: private/{SERIE}/planifies/{i}.gpx, sha256: {sha256(g)}{extra}}}")
    print("  traces:")
    for r in rows:
        print(f"    - id: {r['pseudo']}")
        print(f"      groupe: {r['groupe']}")
        print(f"      serie: {r['serie']}")
        if r["masques"]:
            print(f"      passages_masques: [{', '.join(r['masques'])}]")
        print(f"      fenetres_calibration: {len(r['fenetres_calibration'])}   # series-{SERIE}.csv")
        print(f"      region: \"{r['lieu']}\"")
        print(f"      mois: \"{r['mois']}\"")
        print(f"      dist_km: {r['dist_km']}")
        print(f"      fichier: private/traces/{r['pseudo']}.gpx")
        print(f"      sha256: {r['sha256_gpx']}")
        print(f"      sha256_trim1km: {r['sha256_gpx_trim']}")
        print(f"      qualification: {{verdict: {r['verdict']}, offset_m: {r['offset_m'] or 'null'}, drift_m: {r['drift_m'] or 'null'}, "
              f"noise_m: {r['noise_m'] or 'null'}, p95_abs_m: {r['p95_abs_m'] or 'null'}, gain_baro_m: {r['gain_rec_m'] or 'null'}, "
              f"gain_mnt_m: {r['gain_dem_m'] or 'null'}}}")


if __name__ == "__main__":
    main(sys.argv[1])
