#!/usr/bin/env python3
"""Ouvrages V1.1 exclus à tort comme « déjà vus en calibration », ajoutés à la calibration V1.1
(décision de l'utilisateur, 2026-11-03 ; verdict V1.1 non revu).

Le contrôle de couverture V1.1 comptait les traces de la série dans la calibration (défaut de
couverture_v1.py corrigé en 6b5a1c0) : des ouvrages franchis par une trace V1.1 étaient exclus
`deja_vu_calibration` par leur propre trace. Cet outil relit private/v11/couverture-cas.json (gelé,
jamais réécrit), recalcule `deja_vu_calibration` avec calibration_refs corrigée (SERIE=v11) et
retient les ouvrages dont c'était la seule exclusion. Les ponts passent les mêmes contrôles IGN que
dans couverture_v1.py (MNH au milieu, niveau des passages baro), dans un dossier de travail, puis la
même règle de compte (hors_lidar, pas_un_tablier, passage_retire_niveau).

Les ouvrages retenus sont numérotés nv-NN à la suite de ouvrages-v11.json (par type puis longueur
décroissante, comme build_v11_corpus.py), avant tout corpus V1.2, et ajoutés à ouvrages-v11.json et
series-v11.csv (même schéma, `complement_v11: true`). Copies d'avant l'ajout :
private/v11/*.avant-complement.*. Un fichier déjà complété n'est pas complété deux fois.

Sortie standard : nombres par type et numéros, sans coordonnée ni identifiant de voie.

Usage : uv run -q --no-project python -I tools/complement_v11.py private/
"""
import csv
import importlib.util
import json
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
os.environ["SERIE"] = "v11"
spec = importlib.util.spec_from_file_location("couverture_v1", f"{HERE}/couverture_v1.py")
cv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cv)


def main(B):
    B = B.rstrip("/")
    D = f"{B}/v11"
    O = json.load(open(f"{D}/ouvrages-v11.json"))
    if any(o.get("complement_v11") for o in O):
        raise SystemExit("ouvrages-v11.json déjà complété")
    C = json.load(open(f"{D}/couverture-cas.json"))
    refs, cg = cv.calibration_refs(B)
    cand = {}
    for k, o in C["ouvrages"].items():
        if o["exclusions"] != ["deja_vu_calibration"]:
            continue
        ll = [tuple(p) for p in o["pts"].values()]
        if any(cv.hav(p, q) < cv.MASK_M for p in ll for q in refs) or cv.seen_by_track(cg, ll):
            continue
        cand[k] = dict(o, exclusions=[])
    print("deja_vu_calibration à tort :", {t: sum(1 for o in cand.values() if o["type"] == t) for t in ("bridge", "tunnel")}, file=sys.stderr)

    # contrôles IGN de couverture_v1.py (ign_step), sur les seuls candidats, dans un dossier de travail
    W = f"{D}/complement"
    os.makedirs(f"{W}/v11", exist_ok=True)
    if not os.path.exists(f"{W}/strava"):
        os.symlink(os.path.abspath(f"{B}/strava"), f"{W}/strava")
    json.dump({"ouvrages": cand}, open(f"{W}/v11/couverture-cas.json", "w"))
    cv.ign_step(W)
    G = json.load(open(f"{W}/v11/couverture-ign.json"))
    LV = json.load(open(f"{W}/v11/couverture-niveau.json")) if os.path.exists(f"{W}/v11/couverture-niveau.json") else {}

    # règle de compte de couverture_v1.py (tirage)
    compte, raisons = [], {}
    for k, o in cand.items():
        r = []
        if o["type"] == "bridge":
            g = G.get(k)
            if g is None or g["mnh_m"] is None:
                r.append("hors_lidar")
            elif g["mnh_m"] < cv.MIN_MNH_M:
                r.append("pas_un_tablier")
        ok = [p for p in o["passages"] if not p["exclusions"] and LV.get(f'{k}:{p["trace"]}', {}).get("etat") != "retire"]
        if not r and not ok:
            r.append("passage_sur_ouvrage_parallele" if any(p["exclusions"] for p in o["passages"]) else "passage_retire_niveau")
        for x in r:
            raisons[f'{o["type"]}:{x}'] = raisons.get(f'{o["type"]}:{x}', 0) + 1
        if not r:
            compte.append((k, ok))

    pseudo = {r["strava_id"]: r["pseudo"] for r in csv.DictReader(open(f"{D}/perso-map-v11.csv"))}
    n_nv = max(int(o["id"].split("-")[1]) for o in O)
    compte.sort(key=lambda x: (cand[x[0]]["type"], -cand[x[0]]["length_m"], x[0]))
    neufs = []
    for n, (k, ok) in enumerate(compte, n_nv + 1):
        o = cand[k]
        tr = [p["trace"] for p in ok]
        neufs.append({"id": f"nv-{n:02d}", "name": "", "type": o["type"], "chain": o["ways"], "extra": [], "region": "",
                      "cas": "Ouvrage V1.1 franchi par une trace de la série V1.1, exclu à tort comme déjà vu (6b5a1c0), "
                             "ajouté à la calibration V1.1 (décision de l'utilisateur, 2026-11-03)",
                      "vc": "", "notes": "", "pts_from": "osm", "length_m": round(o["length_m"]), "mid": o["pts"]["milieu"],
                      "pts": o["pts"], "tags": {str(w): o["tags"] for w in o["ways"][:1]},
                      "trace": pseudo.get(tr[0], tr[0]) if tr else "", "milieu_mns_mediane_25m": None,
                      "mnh_milieu_m": (G.get(k) or {}).get("mnh_m"), "serie": "tenue_a_l_ecart", "groupe_tirage": None,
                      "complement_v11": True})

    for f in ("ouvrages-v11.json", "series-v11.csv"):
        a, e = os.path.splitext(f)
        if not os.path.exists(f"{D}/{a}.avant-complement{e}"):
            shutil.copy2(f"{D}/{f}", f"{D}/{a}.avant-complement{e}")
    json.dump(O + neufs, open(f"{D}/ouvrages-v11.json", "w"), ensure_ascii=False)
    with open(f"{D}/series-v11.csv", "a", newline="") as f:
        w = csv.writer(f)
        for o in neufs:
            w.writerow([o["id"], "ouvrage", "", "tenue_a_l_ecart", "", ""])
    print(json.dumps({"retenus": {t: sum(1 for o in neufs if o["type"] == t) for t in ("bridge", "tunnel")},
                      "plus_de_150m": sum(1 for o in neufs if o["type"] == "bridge" and o["length_m"] > 150),
                      "mnh_10m": sum(1 for o in neufs if (o["mnh_milieu_m"] or 0) >= cv.MNH_NIVEAU_M),
                      "exclusions": raisons, "premier": neufs[0]["id"] if neufs else None,
                      "dernier": neufs[-1]["id"] if neufs else None}, ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv[1])
