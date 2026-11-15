#!/usr/bin/env python3
"""Sert la visionneuse des parcours générés par ord-loops-bench, en local uniquement.

    python3 tools/ord-loops-viewer/serve.py [dossier-geojson] [--port 8765]

`/` rend la page, `/data/index.json` la liste des fichiers `<équipe>-<requête>-<mode>.geojson` du
dossier, `/data/<fichier>` un fichier. Le dossier contient des départs réels d'équipes : le serveur
n'écoute que sur 127.0.0.1 et rien n'en est copié dans le dépôt.
"""

import argparse
import json
import re
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_DIR = Path.home() / ".cache/osm-route-details/graph/lot2/geojson"
NAME = re.compile(r"^([A-Za-z0-9]+)-(\d+)-([a-z]+)\.geojson$")


def listing(data_dir: Path):
    requests = {}
    for path in sorted(data_dir.glob("*.geojson")):
        m = NAME.match(path.name)
        if not m:
            continue
        team, number, mode = m.group(1), int(m.group(2)), m.group(3)
        entry = requests.setdefault((team, number), {"team": team, "request": number, "modes": {}})
        entry["modes"][mode] = path.name
    return [requests[key] for key in sorted(requests)]


def handler(data_dir: Path):
    class Handler(BaseHTTPRequestHandler):
        def send(self, status, body: bytes, content_type: str):
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            path = self.path.split("?", 1)[0]
            if path in ("/", "/index.html"):
                self.send(200, (HERE / "index.html").read_bytes(), "text/html; charset=utf-8")
            elif path == "/data/index.json":
                self.send(200, json.dumps(listing(data_dir)).encode(), "application/json")
            elif path.startswith("/data/") and NAME.match(path[6:]):
                target = data_dir / path[6:]
                if target.is_file():
                    self.send(200, target.read_bytes(), "application/geo+json")
                else:
                    self.send(404, b"absent", "text/plain")
            else:
                self.send(404, b"absent", "text/plain")

        def log_message(self, *_):
            pass

    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("data_dir", nargs="?", default=str(DEFAULT_DIR))
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    data_dir = Path(args.data_dir).expanduser().resolve()
    if not data_dir.is_dir():
        raise SystemExit(f"dossier introuvable : {data_dir}")
    count = len(listing(data_dir))
    print(f"{count} requêtes dans {data_dir}")
    print(f"http://127.0.0.1:{args.port}/")
    ThreadingHTTPServer(("127.0.0.1", args.port), handler(data_dir)).serve_forever()


if __name__ == "__main__":
    main()
