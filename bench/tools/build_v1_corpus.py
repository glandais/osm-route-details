#!/usr/bin/env python3
"""Construit les cas de la série V1 (calibration V1 et série tenue à l'écart) à partir du tirage figé.

Entrées (bench/private/v1/, produites par couverture_v1.py) : couverture-cas.json, couverture-ign.json,
couverture-niveau.json, affectation.json, tirage.json, planifies/resume.json ; streams Strava
(strava/streams-v1/), qualifications (strava/qualif_v1.csv, qualif_q1.csv), inventaire.

Sorties, toutes hors git (bench/private/) :
- traces/<pseudo>.gpx et <pseudo>.trim1km.gpx : perso-NN (sorties fiables, numérotées à la suite du
  MVP par date de départ) et neg-NN (négatifs Q1), via stream_to_gpx.py ;
- v1/perso-map-v1.csv : pseudonyme → identifiant Strava (ne quitte jamais private/) ;
- v1/ouvrages-v1.json : ouvrages comptés, nv-NN numérotés à la suite du MVP (par type puis
  longueur décroissante), même schéma qu'ouvrages-perso.json, avec `serie` ; croisements E5 en
  `passage sous pont` (point de croisement, pont du dessus) ;
- v1/temoins-v1.json : témoins S2 (tronçons de sorties : extrémités ; itinéraires planifiés : entiers) ;
- v1/series-v1.csv : id, type, groupe, serie, passages_masques (fenêtres ± MASK_M des ouvrages,
  croisements et témoins d'une autre série que la trace : README §2, règle 4).

Sortie standard : fragment YAML du manifest (section perso_v1), sans identifiant Strava ni
coordonnée ; département et mois seulement.

Usage : build_v1_corpus.py private/
"""
import csv
import hashlib
import json
import math
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MASK_M = 500.0
TOL_M = 40.0


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


def main(B):
    B = B.rstrip("/")
    C = json.load(open(f"{B}/v1/couverture-cas.json"))
    G = json.load(open(f"{B}/v1/couverture-ign.json"))
    A = json.load(open(f"{B}/v1/affectation.json"))
    inv = {str(a["id"]): a for a in json.load(open(f"{B}/strava/inventaire.json"))}
    qual = {}
    for f in ("qualif_v1.csv", "qualif_q1.csv"):
        qual.update({r["id"]: r for r in csv.DictReader(open(f"{B}/strava/{f}"))})
    old = list(csv.DictReader(open(f"{B}/perso-map.csv")))
    n_perso = max(int(r["pseudo"].split("-")[1]) for r in old if r["pseudo"].startswith("perso-"))
    n_neg = max(int(r["pseudo"].split("-")[1]) for r in old if r["pseudo"].startswith("neg-"))
    nv_old = json.load(open(f"{B}/ouvrages-perso.json"))
    n_nv = max(int(o["id"].split("-")[1]) for o in nv_old)

    # pseudonymes, par date de départ puis identifiant
    def start(i):
        return (inv[i]["start_local"], i)
    traces = sorted((i for i, s in A["traces"].items() if i in A["groupes_traces"]), key=start)
    negs = sorted((t["id"] for t in C["traces"] if t["role"] == "Q1"), key=start)
    pseudo = {}
    for k, i in enumerate(traces, n_perso + 1):
        pseudo[i] = f"perso-{k:02d}"
    for k, i in enumerate(negs, n_neg + 1):
        pseudo[i] = f"neg-{k:02d}"
    planifies = {t["id"]: t for t in C["traces"] if t["zone"] == "planifie" and t["id"] in
                 json.load(open(f"{B}/v1/planifies/resume.json"))}

    # ouvrages comptés → nv-NN
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
                    "cas": "Ouvrage V1 franchi par une trace de la série V1 (perso ou planifiée)", "vc": "", "notes": "",
                    "pts_from": "osm", "length_m": round(o["length_m"]), "mid": o["pts"]["milieu"], "pts": o["pts"],
                    "tags": {str(w): o["tags"] for w in o["ways"][:1]},
                    "trace": pseudo.get(tr[0], tr[0]) if tr else "",
                    "milieu_mns_mediane_25m": None, "mnh_milieu_m": (G.get(oid) or {}).get("mnh_m"),
                    "serie": A["ouvrages"][oid], "groupe_tirage": A["groupes_ouvrages"].get(oid)})
    for cid in xs:
        c = C["croisements"][cid]
        ouv.append({"id": nv[cid], "name": "", "type": "passage sous pont", "chain": [], "extra": [c["way"]],
                    "region": "", "cas": "Croisement V1 sous un pont (E5)", "vc": "", "notes": "voie du dessous à lire dans les tuiles",
                    "cross": [c["way"], None], "pts_from": [], "length_m": 0, "mid": c["pt"], "pts": {"croisement": c["pt"]},
                    "tags": {}, "rail": c["rail"], "trace": pseudo.get(c["traces"][0], c["traces"][0]),
                    "serie": A["croisements"][cid], "groupe_tirage": A["groupes_ouvrages"].get(cid)})
    json.dump(ouv, open(f"{B}/v1/ouvrages-v1.json", "w"), ensure_ascii=False)
    tem = [dict(t, trace=pseudo.get(t["trace"], t["trace"]), serie=A["temoins"][t["id"]],
                id=(t["id"] if t["planifie"] else t["id"].replace(t["trace"], pseudo.get(t["trace"], t["trace"]))))
           for t in C["temoins"] if t["id"] in A["temoins"]]
    json.dump(tem, open(f"{B}/v1/temoins-v1.json", "w"), ensure_ascii=False)

    # GPX et empreintes
    rows = []
    for i in traces + negs:
        p = pseudo[i]
        src = f"{B}/strava/streams-v1/{i}.json"
        full, trim = f"{B}/traces/{p}.gpx", f"{B}/traces/{p}.trim1km.gpx"
        for out, extra in ((full, []), (trim, ["--trim-m", "1000"])):
            subprocess.run([sys.executable, "-I", os.path.join(HERE, "stream_to_gpx.py"), src, out, "--name", p, *extra], check=True)
        a = inv[i]; q = qual.get(i, {})
        rows.append({"pseudo": p, "strava_id": i, "groupe": A["groupes_traces"].get(i, "negatif"),
                     "serie": A["traces"][i], "mois": a["start_local"][:7],
                     "lieu": (a.get("location_summary") or "").split(", ")[-1], "dist_km": q.get("dist_km"),
                     "verdict": q.get("verdict"), "offset_m": q.get("offset_m"), "drift_m": q.get("drift_m"),
                     "noise_m": q.get("noise_m"), "p95_abs_m": q.get("p95_abs_m"), "gain_rec_m": q.get("gain_rec_m"),
                     "gain_dem_m": q.get("gain_dem_m"), "sha256_stream": sha256(src), "sha256_gpx": sha256(full),
                     "sha256_gpx_trim": sha256(trim)})
    with open(f"{B}/v1/perso-map-v1.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0])); w.writeheader(); w.writerows(rows)

    # fenêtres masquées : éléments d'une autre série que la trace, à moins de TOL_M de ses points
    elems = [(o["id"], o["serie"], [tuple(x) for x in o["pts"].values()]) for o in ouv]
    elems += [(t["id"], t["serie"], [tuple(t[k]) for k in ("debut", "fin")] if "debut" in t else []) for t in tem if not t["planifie"]]
    series_rows = []
    for r in rows:
        track = [tuple(p) for p in json.load(open(f"{B}/strava/streams-v1/{r['strava_id']}.json"))["location"] if p]
        own = [t["id"] for t in tem if t["trace"] == r["pseudo"]]
        masks = sorted(e for e, s, pts in elems if s != r["serie"] and pts and
                       (e in own or all(near_track(track, p, TOL_M) for p in pts)))
        r["masques"] = masks
        series_rows.append({"id": r["pseudo"], "type": "trace", "groupe": r["groupe"], "serie": r["serie"], "passages_masques": " ".join(masks)})
    tir = json.load(open(f"{B}/v1/tirage.json"))["serie"]
    for i, t in sorted(planifies.items()):
        # cas du tirage (R1/R2, témoin S2) : sa série ; itinéraire E4 : chaque tunnel porte la sienne
        series_rows.append({"id": i, "type": "planifie", "groupe": t["role"], "serie": tir.get(i, "par_cas"), "passages_masques": ""})
    for o in ouv:
        series_rows.append({"id": o["id"], "type": "ouvrage", "groupe": o["groupe_tirage"] and nv.get(o["groupe_tirage"], o["groupe_tirage"]),
                            "serie": o["serie"], "passages_masques": ""})
    for t in tem:
        series_rows.append({"id": t["id"], "type": "temoin", "groupe": t["nature"], "serie": t["serie"], "passages_masques": ""})
    with open(f"{B}/v1/series-v1.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(series_rows[0])); w.writeheader(); w.writerows(series_rows)

    # fragment de manifest : sans identifiant Strava ni coordonnée
    print("  traces:")
    for r in rows:
        print(f"    - id: {r['pseudo']}")
        print(f"      groupe: {r['groupe']}")
        print(f"      serie: {r['serie']}")
        if r["masques"]:
            print(f"      passages_masques: [{', '.join(r['masques'])}]")
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
