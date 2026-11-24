# Calcul d'itinéraire de A à B — lot 1 : le moteur

> Statut : **codé le 23 novembre 2026** dans `ord-loops` (`route.rs`, `tests_route.rs`, exemple `route_region`),
> depuis `develop` à `288d0d3` (1.15.1). Le service (`ord-server`, contrat api 1.15) est le lot 2, §7. Plan appelant :
> Pédalons `docs/plans/2026-11-23-itineraire-velo-osm.md` (ledger `API-217`), §3 à §5 et §10.
>
> Marques : **[lu]** relu dans le code ; **[mesuré]** relevé de cette note (§5) ; **[proposé]** réglage de départ, que le
> banc du lot 1 bis confirme ou change. Ni coordonnée d'équipe ni identifiant ici ; les points d'essai sont des centres
> de villes.

## 1. Périmètre

- **Dans le lot** : `route_in` et `route` (un itinéraire d'un point à un autre, pour la route, le gravel et le VTT),
  `Engine::plan_route` (la fenêtre d'une zone), `route_geometry_spans` (géométrie et plages non revêtues).
- **Hors du lot** : l'altitude et la pente (jamais : le moteur n'a pas l'altitude), les points de passage multiples (le
  planificateur de Pédalons route tronçon par tronçon), les alternatives, le guidage, les données de nœud (barrières,
  feux), les restrictions de virage et les bacs (lot 6 du plan).
- **Invariant** : une génération rend les mêmes parcours qu'avant, au bit près. La génération n'appelle rien de
  `route.rs` ; `search.rs` ne gagne que la visibilité `pub(crate)` de trois constantes de virage.

## 2. Ce que l'itinéraire reprend du générateur [lu]

| Brique | Reprise telle quelle |
|---|---|
| Vue | `road` pour la route, `gravel` pour le gravel et le VTT (`RideType::view_index`) |
| Poids | `Weighter` : `K(classe, revêtement, accès)`, goût commun ou de l'équipe, familiarité et λ, roulabilité du lot 5 |
| États | chaînes orientées de la fenêtre ; morceaux partiels de départ et d'arrivée (`make_anchor`) |
| Virages, première passe | `turn_cost` et `retro_ok` : gratuit sous 45°, coût jusqu'à 150°, refus au-delà aux carrefours, demi-tour refusé |
| Rendu | un `Route` : `route_geometry`, `trim_piece` et le contrôleur indépendant le lisent comme un parcours généré |

Aucun poids n'est changé. Ce que le plan prend à Valhalla (aménagement cyclable, montée de classe, entrée sur une voie
« destination ») attend le banc.

## 3. La recherche

- **A\*** du morceau partiel de départ (deux états, un par sens ouvert) au morceau partiel d'arrivée. Les états d'arrivée
  ont leurs propres cases (`2 n + sens`) : la chaîne d'arrivée peut être celle du départ (arrivée derrière le départ
  sur un sens unique : on fait le tour), ou être traversée en entier.
- **Heuristique** : vol d'oiseau jusqu'à la position réelle de l'arrivée × le plus petit coût par décimètre des chaînes
  de la fenêtre (un balayage par requête). Minorant, sauf pour le morceau partiel d'arrivée d'une chaîne qui mêle des
  arêtes de poids différents (écart borné par ce morceau). **Faible** : le goût et l'équipe descendent le minimum bien
  sous le coût courant, la recherche fixe l'essentiel de la zone (§5).
- **Zone** : ellipse de foyers départ et arrivée, de somme `max(1,5 d ; d + 4 km)`, puis `max(2,5 d ; d + 15 km)` si
  rien n'est trouvé (`RouteTune::reach`) **[proposé]**. La fenêtre est celle d'un aller simple de cette distance
  (`plan_with`) : le cache de fenêtres du service la partage avec les générations.
- **Virages** : le coût du générateur sans son refus. Le virage serré (150° et plus à un carrefour) coûte le maximum
  (`TURN_COST_MAX` à `TURN_COST_MAX + 500`, 200 à 250 m équivalents) au lieu d'être refusé, et le demi-tour sur la
  même chaîne n'est permis qu'au bout d'une impasse. `Routed::sharp_relaxed` dit si l'itinéraire en contient un.
  Jusqu'à 1.17.0 le refus du générateur s'appliquait d'abord, et le coût seulement sans autre chemin : un itinéraire
  faisait alors plusieurs fois la distance pour éviter un seul virage (2,9 km au lieu de 1,4 km sur un cas réel de
  900 m à vol d'oiseau). Une génération peut dessiner une autre boucle, un itinéraire doit relier deux points donnés.
- **Même chaîne** : arrivée devant le départ dans un sens ouvert → le morceau direct, sans recherche.
- **Accroche** : la voie roulable la plus proche, à 500 m au plus **[proposé]** (2 km pour une génération), dans une
  composante d'au moins `min(0,9 d ; 5 km)`.
- **Bornes** : vol d'oiseau de 100 km au plus **[proposé]** (`TooLong`) ; échappement de fenêtre rejoué une fois dans
  une fenêtre élargie, comme une génération.

Échecs : `StartTooFar`, `EndTooFar`, `TooLong`, `NoRoute`, `NoTeamData` (mode d'équipe sans surcouche), `Stopped`,
`WindowEscape`.

## 4. Tests (`tests_route.rs`)

1. Le coût rendu est celui d'un Dijkstra de référence (l'arbre aller du générateur sur toute la grille, puis le dernier
   pas vers l'arrivée) ; longueur au plus court sur une grille.
2. Deux points d'une même rue, dans les deux ordres : morceau direct, aucune recherche.
3. Sens unique, arrivée derrière le départ : le tour du pâté de maisons ; dans l'ordre : le morceau direct.
4. Un virage de 160° sans autre chemin, puis face à un tour de pâté de 1,9 km : pris dans les deux cas (`sharp_relaxed`).
5. Équipe d'abord suit la rue de l'équipe (plus longue), la découverte le plus court ; mode d'équipe sans surcouche refusé.
6. Point à plus de 500 m, tronçon trop long, coupure par l'appelant.
7. Deux réseaux disjoints : `NoRoute` après les deux zones.
8. Plages non revêtues : exactement la piste, dans les deux sens ; sur la vue route, pas d'itinéraire par la piste.

Les 123 tests existants d'`ord-loops`, références figées des générations comprises, passent inchangés.

## 5. Premier relevé [mesuré]

Pays de la Loire du 14 novembre 2026, `route_region` en `--release`, un fil, découverte sauf mention. Temps : fenêtre
à froid, puis recherche à froid et à chaud.

| Tronçon | Type | Fenêtre | Recherche (froid / chaud) | Longueur | Détour | États fixés / chaînes |
|---|---|---|---|---|---|---|
| Nantes → Carquefou (9,9 km) | route | 45 ms | 81 / 16 ms | 11,1 km | × 1,12 | 26 776 / 88 801 |
| | gravel | 49 ms | 99 / 20 ms | 12,2 km | × 1,24 | 36 948 / 131 938 |
| Nantes → Ancenis (32,8 km) | route | 40 ms | 92 / 33 ms | 39,7 km | × 1,21 | 95 060 / 120 214 |
| | VTT | 55 ms | 121 / 46 ms | 41,1 km | × 1,25 | 124 701 / 180 757 |
| Nantes → Angers (80,0 km) | route | 121 ms | 157 / 77 ms | 94,2 km | × 1,18 | 252 355 / 294 019 |
| | gravel | 142 ms | 210 / 117 ms | 99,6 km | × 1,24 | 380 889 / 426 754 |
| Saint-Nazaire → Saint-Brevin (4,7 km, la Loire) | route | 13 + 8 ms | 7 + 7 ms | 12,0 km | × 2,54 | zone 0 sans chemin, zone 1 par le pont |

Nantes → Ancenis en équipe d'abord (familiarité 0,75), trois surcouches réelles : 99 % et 95 % de la longueur sur les
routes de l'équipe pour les deux équipes qui roulent là (41,3 et 40,9 km contre 39,7 km en découverte), 0 % pour la
troisième, qui roule ailleurs. Temps inchangés (30 à 40 ms à chaud).

Lecture : les temps tiennent la cible du plan (300 ms à chaud pour 20 km) avec une marge d'un ordre de grandeur ; la
part non revêtue des itinéraires gravel et VTT (27 à 54 %) suit le coût du générateur sans consigne ; l'heuristique
ne coupe presque rien au-delà de 30 km (les états fixés approchent le nombre de chaînes de la zone).

## 6. Ce qui reste

- **Lot 1 bis** : le banc sur des tronçons de traces réelles (part de la trace retrouvée, détour, temps), et la
  comparaison avec Stadia ; les facteurs repris de Valhalla s'il les justifie.
- **Heuristique** : des repères (ALT) ou une recherche bidirectionnelle si le P90 du banc le demande ; rien ne le
  demande aujourd'hui.
- **Pour la retouche** (plan Pédalons `API-212`) : un cap imposé au départ et des chaînes interdites (`KeepOut`, sous
  `probe` aujourd'hui).

## 7. Lot 2 : le service (1.16.0, api 1.15)

`POST /v1/router/route` dans `ord-server` (`generator/router.rs`), sans rien changer à `ord-loops` ni aux générations.

- **Équipe** : en-têtes `Ord-Namespace-Key` et `Ord-Team` facultatifs (400 si un seul est là ou s'il est mal formé).
  Avec eux, `cursor::apply_route` : le curseur de la génération, le strict remplacé par l'équipe d'abord à λ = 3 ; sans
  `cursor`, la découverte (part d'équipe mesurée). Surcouche absente ou périmée : découverte, `overlay: null`. Mémoire
  de l'équipe introuvable dans le budget : 503, comme une génération (la réponse ne doit pas dépendre de la charge).
- **Zones** : `reach_m(tune, 0)`, puis le pas 1 sur `NoRoute` seulement ; chaque essai prend sa fenêtre par
  `WindowManager::acquire` avec le plan de `plan_route` (pas de rayon du service), rejoue une fois élargi sur
  `WindowEscape` ; un second échappement est un 500.
- **File** : sémaphore et compteur d'attente à part (`router_max_concurrent` 2, `router_max_queued` 6), budget
  `router_budget_ms` (5 000, attente comprise). Une file plus courte que celle des générations (8) : à quelques dizaines
  de ms le tronçon, six en attente derrière deux en calcul tiennent bien dans le budget.
- **Erreurs** : vol d'oiseau au-delà de `crow_max_m` vérifié dès la validation (400 `invalid_input`, pointeur `/end`, la
  limite dans le message), avant la couverture (422 `out_of_coverage`) ; `StartTooFar`, `EndTooFar`, `NoRoute` en 422
  `generation_failed` (raisons `start_too_far`, `end_too_far`, `no_route`, la dernière ajoutée à `GenerationReason`).
- **Tests** : `crates/ord-server/tests/router.rs` (grille synthétique des tests du générateur) et `router_sentinel.rs`
  (aucune coordonnée, clé ni équipe dans les logs et les métriques).

## 8. Préférence de voie (curseur routes / pistes) (1.17.0, api 1.16)

Deux champs facultatifs de `POST /v1/router/route` : `use_roads`, le réglage demandé, et `use_roads_neutral`, celui du
profil de l'appelant, tous deux dans [0 ; 1] (0 : pistes et petites voies, 1 : routes). Le moteur reçoit
`RouteRequest::way_q8 = way_factors(ride, use_roads, use_roads_neutral)`, un multiplicateur en 256es par case de voie
(`WayQ8`, `WAYS` = 7 cases). Un troisième champ, `avoid_main_roads`, renchérit les grandes routes par-dessus (plus bas).
Sans ces champs, `WAY_NEUTRAL` (256 partout) : rien n'est multiplié, l'itinéraire est celui de 1.16.0 au bit près.
`use_roads` sans son neutre est un 400 (pointeur `/use_roads_neutral`) : le neutre dépend du profil, le service ne le
devine pas ; le neutre seul est sans effet. Le réglage vaut avec ou sans équipe, dans tous les modes ; les générations
ne le voient pas.

- **Cases** : les six voies du goût (`taste_way` : 0 grande route, 1 `tertiary`, 2 petite route, 3 rue, 4 voie cyclable
  ou verte, 5 chemin), plus une case 6 (`WAY_SECONDARY`) pour `secondary` et `secondary_link`, sortis de la case 0. Le
  goût commun met `secondary` au prix d'une rue (0,77 × 88/64 en route) : l'éviter sans toucher à `primary` / `trunk`,
  ou l'éviter plus qu'eux, demande de la séparer.
- **Formule** (doc de `way_factors`), transposée du `road_factor` de Valhalla (`bicyclecost.cc`) :
  `f(u) = 1,5 − u` si `u ≥ 0,5`, sinon `2 − 2u` ; `g = f(use_roads) / f(neutral)` ; le multiplicateur d'une case vaut
  `g^α`, avec α = 1 (grande route, case 0), 0,6 (`tertiary`), 0,4 (petite route), 0,3 (rue), −0,5 (voie cyclable ou
  verte), pour le chemin 0 en route et −0,5 en gravel et VTT, et 1 pour `secondary` (case 6, qui suit les autres
  grandes routes) ; borné à [64 ; 1 024] comme une table de goût. Au neutre, `g = 1` : 256 partout.
- **Par chaîne** (`WayScale`) : le multiplicateur d'une chaîne est la moyenne, pondérée par la longueur, de ceux de ses
  arêtes de base, calculée à la première lecture de la chaîne et gardée pour la requête ; le poids du générateur est
  multiplié par lui (arrondi, jamais nul, `INF` reste `INF`).
- **Morceaux partiels** : les coûts d'accroche du départ et de l'arrivée (`out_cost`, `in_cost`) suivent la moyenne de
  leur chaîne ; l'écart à la voie exacte du morceau est borné par sa longueur.
- **Heuristique** : le plus petit coût par décimètre (`min_factor`) est recalculé avec les poids modifiés, pour que
  l'A* reste admissible quand une voie devient moins chère que le meilleur facteur d'avant.
- **Journaux et métriques** : la valeur du réglage n'en sort pas ; une étiquette `use_roads` = `none|less|same|more`
  (le réglage comparé à son neutre) et une étiquette `avoid_main_roads` = `true|false` s'ajoutent à
  `ord_router_requests_total` et à la ligne `route from A to B`.

Premier relevé [mesuré], Pays de la Loire, Nantes → Ancenis (32,8 km à vol d'oiseau), découverte, un fil :

| Type, neutre du profil | 0 | 0,25 | 0,5 | 0,75 | 1 |
|---|---|---|---|---|---|
| route, neutre 0,25 | 39,9 km | 39,7 km | 39,6 km | 39,6 km | 39,3 km |
| route, neutre 1 | 43,6 km | 41,8 km | 41,8 km | 40,2 km | 39,7 km |
| gravel, neutre 0,25 | 41,7 km, 64 % non revêtu | 38,4 km, 46 % | — | — | 39,8 km, 0 % |

Temps à chaud de 33 à 64 ms, comme sans réglage.

**Éviter les grandes routes** (`avoid_main_roads: true`, `ord_loops::avoid_main_roads`) : par-dessus la préférence
issue de `use_roads` (ou `WAY_NEUTRAL`), la case `secondary` est doublée et la case 0 (`primary`, `trunk`) multipliée
par 1,5, bornées à 1 024. Doublée, `secondary` passe derrière toute petite route, et `primary` reste derrière elle. Les
grandes routes ne sont pas interdites : elles restent prises quand rien d'autre ne passe. Absent, `null` ou `false` :
aucun effet. Indépendant de `use_roads` et cumulable, avec ou sans équipe. Relevé [mesuré], Pays de la Loire,
découverte, un fil, sans réglage `use_roads` :

| Trajet, type | Sans | Avec `avoid_main_roads` |
|---|---|---|
| Nantes → Ancenis, route | 39,7 km | 41,4 km |
| Nantes → Ancenis, gravel | 38,4 km | 38,6 km |
| Nantes → Angers, route | 94,2 km | 96,2 km |
| Nantes → Angers, gravel | 99,6 km | 101,1 km |

Temps à chaud de 45 à 140 ms, contre 34 à 115 ms sans.

**Limites** : les α et les pénalités de `avoid_main_roads` (× 2, × 1,5) sont choisis à la main ; aucune mesure sur des
traces réelles ; la longueur parcourue sur `secondary` avant et après `avoid_main_roads` n'a pas été mesurée ; `infra`
(l'aménagement cyclable) est hors de la table. À caler sur le banc du lot 1 bis (§6).
