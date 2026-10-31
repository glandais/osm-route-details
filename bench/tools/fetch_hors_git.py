#!/usr/bin/env python3
"""Régénère les fixtures `hors_git` du manifest tirées d'un dépôt externe, et vérifie leur empreinte.

Pour chaque entrée de la section `hors_git` de manifest.yaml : `git show <commit>:<chemin>` dans le
dépôt local donné, écriture dans `fichier` (relatif à bench/), contrôle du sha256. Rien n'est
téléchargé : le dépôt doit contenir le commit (git fetch au besoin).

Usage : fetch_hors_git.py manifest.yaml depot=chemin_local [depot=chemin_local ...]
        (ex. vcyclist=../vcyclist ; nécessite PyYAML : uv run --with pyyaml)
"""
import hashlib
import os
import subprocess
import sys

import yaml


def main(manifest_path, *repos):
    roots = dict(r.split("=", 1) for r in repos)
    bench = os.path.dirname(os.path.abspath(manifest_path))
    ok = True
    for e in yaml.safe_load(open(manifest_path)).get("hors_git", []):
        src = e["provenance_depot"]
        if src["depot"] not in roots:
            print(f"{e['id']} : dépôt {src['depot']} non fourni, ignoré", file=sys.stderr)
            continue
        data = subprocess.run(["git", "-C", roots[src["depot"]], "show", f"{src['commit']}:{src['chemin']}"],
                              check=True, capture_output=True).stdout
        sha = hashlib.sha256(data).hexdigest()
        if sha != e["sha256"]:
            print(f"{e['id']} : empreinte {sha} ≠ {e['sha256']}", file=sys.stderr)
            ok = False
            continue
        out = os.path.join(bench, e["fichier"])
        os.makedirs(os.path.dirname(out), exist_ok=True)
        open(out, "wb").write(data)
        print(f"{e['id']} : {e['fichier']} ({len(data)} octets)")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main(*sys.argv[1:])
