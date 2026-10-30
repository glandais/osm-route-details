# Strava routes vs IGN LiDAR HD

Analysis behind [`docs/archive/tasks/elevation-bridge-decks.md`](../../archive/tasks/elevation-bridge-decks.md):
how Strava's elevation on four planned routes (`../test1.gpx` … `../test4.gpx`, author metadata
removed) compares with the IGN LiDAR HD terrain (MNT), surface (MNS) and height (MNH) models over
bridges and tunnels. Findings are in [`rapport.md`](rapport.md), figures in `fig/`, every figure in
`results.json`.

Not part of the build. To reproduce (Python 3, numpy, matplotlib; network to `data.geopf.fr` and
Overpass, bbox requests only):

```bash
python3 -I fetch.py     # resample GPX at 5 m, sample MNT/MNS — tiles cached in cache/
python3 -I analyze.py   # bridges, tunnels, estimated roadway, figures, results.json
python3 -I report.py    # rapport_tables.md
python3 -I export_truth.py  # truth/*.csv: frozen roadway, MNS samples and structure spans for ElevationBenchProbeTest
python3 -I controls.py      # E2 bis: negative controls and held-out positives, from OSM only
python3 -I controls.py --tunnels   # E4: tunnel controls (tuning, steep negatives, frozen held-out series)
python3 -I export_demo.py   # the demo's validation tracks + their reference, into demo/public/gpx/validation/
```

`controls.py` (stdlib + numpy, network to Overpass only, bbox queries one at a time; raw answers
cached in `osm/controls/`, so a rerun on that cache is offline and deterministic — but the cache is
gitignored: from a fresh clone the script queries current OSM and may not reproduce the committed
truth, whose reference is `CONTROL_GUARDS` in `ElevationBenchProbeTest`) builds the 33 sites of its
`SITES` table — 16 negative controls `n01..n16` (roads under or beside viaducts and overpasses,
toll canopy, covered carriageway, forest, plane trees, levee, wooded ravine and valley, a street
under a motorway, a quay under bridges), 8 held-out positives `p01..p08` (bridges outside the four
Strava routes; since E2 ter, used or seen in calibration) and the 9 bridges `p09..p17` of the second
held-out series (role `holdout2`, frozen by the rule of the task doc's E2 quinquies before any
detector run, and read only by `deckHoldoutJudge` under `INTEGRATION=1 DECK_JUDGE=1`). A
`holdout2` deck is picked by name or by a "longest bridge that crosses ..." rule, never by hand
(the deck ways of the named bridges were looked up with by-hand Overpass queries before being
coded here; see the task doc's amendments). The 150 m guard measures each counted `D` span's
distance to the route ends only, not the ground between two `D` spans (`p17` has 145 m). Each
site is a Dijkstra route over OSM ways, resampled at 5 m exactly like `fetch.py`, with one kind per station: `G`
ground, `O` structure overhead, `L` bridge or railway viaduct alongside (≤ 20 m), `X` own structure
on a negative, or a tunnel/culvert crossing under the route (an untagged deck), and `x` its 35 m
abutment margin (both excluded from scoring), `D` own bridge deck on a positive. Every site carries
self-checks (expected names, overhead refs, deck lengths), plus no U-turn or doubling back on every
site and 150 m of route beyond every counted `D` span on a positive; a failed check stops the
script without writing. `python3 -I controls.py n04 p08` is a dry run on a subset.

Outputs, read by `ElevationBenchProbeTest` (`deck_controls_parse_and_hold_their_guards`, and
`deckControls` under `INTEGRATION=1`):

- `../controls/<id>.gpx`: the routed OSM nodes, no `<ele>` (© OpenStreetMap contributors, ODbL);
- `truth/controls/<id>.csv`: `s,lat,lon,kind,way` at 5 m, header with role, category, OSM base date;
- `truth/controls/index.csv`: one row per site, station counts per kind;
- `truth/controls/spans.csv`: every maximal run of `D`, `X`, `O`, `L` with, for `D`/`X`, its OSM way
  and structure (`bridge`, `tunnel`, `layer`, `underpass`), and its name.

`controls.py --tunnels` (E4) builds a separate series into `../controls/tunnels/<id>.gpx` and
`truth/tunnels/` (same files and columns, with `N` — the site's own tunnel, truth = line between the
MNT at the stations just outside it — in place of `D`). Roles: `tunnel-tuning` (8 OSM tunnels; the four
Vercors tunnels of `test4` complete the tuning set), `tunnel-negative` (real steep roads: hairpins,
summit, urban ramps, balconies, dam crests, avalanche galleries, a cutting), and the held-out
`tunnel-holdout` / `tunnel-holdout-negative`, frozen by the rule of the task doc's E4 *Témoins tunnels*
before any OSM query for them. Their `TUNNEL_SITES` entries are *cells*: candidates tried in the rule's
order, the first that builds and passes the checks kept, one site per department. Read by
`TunnelBenchProbeTest`: the parse test on every `jvmTest`, `tunnelControls` under `INTEGRATION=1`
(calibration rows only, IGN MNT z15 and Mapterhorn z12), and the held-out rows only by
`tunnelHoldoutTunnelJudge` under `INTEGRATION=1 TUNNEL_JUDGE=1`.
