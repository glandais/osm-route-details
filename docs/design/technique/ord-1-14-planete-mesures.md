# 1.14.0 — la planète dans le service : relevés et budget de génération

> Relevés du 20 novembre 2026, préparation de la 1.14.0. Tout est **[mesuré]** sauf mention. Suite de
> `ord-loops-planete.md` §11.3, qui laissait ouverts « Tokyo à pages froides » (504 à 100 km), la série des villes et la
> concurrence sur la planète.

## 1. Question

Le budget d'une génération (`generator.budget_ms`, 8 000 ms, attente comprise) a été réglé sur la France. Sur la
planète, les zones denses (Tokyo en tête) le dépassent. Il faut savoir de combien, si c'est le disque ou le calcul, si
le moteur lui-même abandonne (`NoRouteDetail::Budget`, `astar_budget`), et fixer le budget de la 1.14.0 sans rien
changer en France.

## 2. Méthode

- **Binaire** : `osm-route-details` de `6e4d579` (`develop`, 1.13.1 plus le fichier `.ordi`), `cargo build --release
  -p ord-server`, lancé par `serve`.
- **Poste** : AMD Ryzen 7 4800H (8 cœurs, 16 fils), 30 Gio, Samsung SSD 980 NVMe ; aucune autre charge lourde.
- **Fichiers** : `planet-261116-20261119-z11.{ordg,road.ordv,gravel.ordv,ordi,poly}` ; pas de `.orda` (attracteurs
  `off`, comme en production).
- **Cadre** : `systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=0`, environnement de l'image
  (`MALLOC_ARENA_MAX=2`, `MALLOC_MMAP_THRESHOLD_=1048576`, `ORD_CACHE__MEMORY_ELEVATION_MB=256`,
  `ORD_CACHE__MEMORY_STREETS_MB=256`), sans altitude (`ORD_CACHE__OFFLINE=true`, IGN désactivé), réglages par défaut
  sauf **`ORD_GENERATOR__BUDGET_MS=120000`** pour mesurer les vraies durées. Prêt en 4,7 à 4,9 s.
- **Pages froides** : `posix_fadvise(DONTNEED)` sur les quatre fichiers (contrôlé à 0 Mo par `fincore`) **avant chaque
  requête** de la passe froide ; puis la passe chaude rejoue les mêmes requêtes, lieu par lieu.
- **Requêtes** : boucles en découverte (`cursor` 1, aucune équipe), graine 1, figures par défaut (organiques) ;
  par lieu : route 60, 100 et 150 km, gravel 40 et 80 km. Durée : `duration_ms` du journal `route generation`
  (égale au temps HTTP à 10 ms près). Part de construction des fenêtres : delta de
  `ord_generator_window_build_seconds_sum` autour de chaque requête (série en série seulement).
- **Lieux** (villes du monde, publiques) :
  - **zones denses** (18) : Tokyo, Osaka, Séoul, Shanghai, Pékin, Hong Kong (Kowloon), Jakarta, Bombay, Delhi, Le Caire,
    Istanbul (péninsule historique), Moscou, Londres, Paris, Mexico, São Paulo, New York (Midtown), Los Angeles ;
  - **série des 14 lieux** du §10.4 de `ord-graph-views-hors-memoire.md` : la liste exacte n'est pas écrite ; reconstituée
    avec les dix lieux que nomment les notes (Denver, Le Cap, Nairobi, Reykjavik, Ushuaia, plein Atlantique, Suva,
    Taveuni, Tokyo, Anchorage) et quatre ajoutés (Sydney, Buenos Aires, Lima, Tromsø) ;
  - **France** : Nantes, Lyon, Paris, Marseille, Toulouse, Lille ;
  - **200 km** (le plafond de Pédalons, `GENERATOR_MAX_KM`) : route 200, gravel 150 et 200, VTT 60 km sur Tokyo,
    Shinjuku, Yokohama, Osaka, Nagoya, Delhi, Jakarta, Londres, New York, Taksim, Kadıköy, et Nantes, Lyon, Paris, Lille ;
  - **allers simples avec cap** (correctif 1.13.1) : Tokyo, Séoul, Londres, Paris, New York, São Paulo ; arrivée à
    5,5 km au nord en 60 km cap est, et à 25 km à l'est en 80 km cap sud.
- **Concurrence** : quatre clients en parallèle sur les 18 zones denses, cache évincé au départ, graines 101 et plus ;
  72 requêtes (60–150 km) à budget levé, puis 84 requêtes avec route et gravel 200 km en plus, à 8 s et à 30 s.

Scripts et relevés bruts hors dépôt, sous `~/.cache/osm-route-details/graph/world-1.14/` (`drive.py`, `analyze.py`,
`*.jsonl`, `*.cg`, journaux du service).

## 3. Relevés en série, budget levé

Durées en secondes. « Fen. » : construction des fenêtres, médiane / max.

| Série | Passe | n | Médiane | p90 | p99 | Max | Fen. | Issues |
|---|---|---|---|---|---|---|---|---|
| Zones denses, 60–150 km | froide | 90 | 2,01 | 5,49 | 11,73 | **11,73** | 0,27 / 0,93 | 83 × 200 ; 422 : 5 `network_too_small`, 2 `no_candidate` |
| | chaude | 90 | 1,48 | 4,75 | 10,59 | 10,59 | 0 | idem |
| Zones denses, 200 km | froide | 44 | 5,14 | 14,70 | 25,73 | **25,73** | 0,58 / 1,28 | 42 × 200 ; 2 `no_candidate` |
| | chaude | 44 | 4,18 | 14,19 | 24,77 | 24,77 | 0 / 1,16 | idem |
| Allers simples avec cap | froide | 12 | 1,21 | 4,07 | 4,08 | 4,08 | 0,22 / 0,74 | 12 × 200 |
| 14 lieux | froide | 70 | 0,40 | 1,65 | 11,04 | 11,04 | 0,05 / 0,93 | 41 × 200 ; 422 : 19 `no_candidate`, 10 `start_too_far` |
| France, 40–150 km | froide | 30 | 1,01 | 1,94 | 2,19 | **2,19** | 0,18 / 0,35 | 30 × 200 |
| | chaude | 30 | 0,68 | 1,43 | 1,75 | 1,75 | 0 | idem |
| France, 200 km | froide | 16 | 2,95 | 4,84 | 6,42 | **6,42** | 0,46 / 0,64 | 16 × 200 |

Aucun 503, aucun 504, aucun `budget`, aucun `warn` (ni échappement de fenêtre, ni parcours refusé par le contrôleur).
Pic anonyme 988 Mo au plus (série la plus longue) ; 0 `oom`, 0 `oom_kill`. `memory.peak` du groupe touche 3 Gio dès le
démarrage : ce sont des pages de fichier (cache des `mmap`), récupérables ; l'anonyme reste sous 1 Gio.

**Les plus lentes** (froid) : Shinjuku gravel 200 km 25,7 s ; Yokohama gravel 200 km 23,3 s, gravel 150 km 21,3 s ;
Tokyo gravel 200 km 15,5 s, route 200 km 13,7 s, route 150 km 11,7 s, route 100 km 9,7 s, gravel 80 km 8,2 s ;
Nagoya gravel 200 km 14,0 s ; Osaka route 200 km 11,5 s, route 150 km 8,3 s ; Delhi route 100 km 7,5 s.
Hors du Japon, rien ne dépasse 9,5 s (New York route 200 km). Par ville, le maximum froid des 60–150 km :

| Tokyo | Osaka | Delhi | Jakarta | Londres | New York | Le Caire | Mexico | Los Angeles | Séoul | Paris | Moscou | São Paulo | Pékin | Shanghai | Bombay | Hong Kong |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 11,7 | 8,3 | 7,5 | 6,6 | 6,4 | 5,8 | 5,1 | 3,4 | 3,2 | 2,9 | 2,3 | 2,7 | 1,8 | 1,2 | 1,0 | 1,1 | 0,4 |

**Part des générations réussies (200) sous un budget donné**, pages froides, en série :

| Budget | 8 s | 10 s | 12 s | 15 s | 20 s | 25 s | 30 s |
|---|---|---|---|---|---|---|---|
| Zones denses 60–150 km (83) | 79 | 82 | 83 | 83 | 83 | 83 | 83 |
| Zones denses 200 km (42) | 27 | 30 | 33 | 38 | 39 | 41 | 42 |
| France, toutes distances (46) | 46 | 46 | 46 | 46 | 46 | 46 | 46 |

### 3.1 Le calcul, pas le disque

- Pages froides contre chaudes : **+0,5 s en médiane, +1,1 s au plus** ; la construction des fenêtres pèse 11 % du
  temps froid des zones denses (17 % en France). Sur ce disque NVMe, ce n'est pas le disque qui fait durer Tokyo.
- **Le moteur cherche longtemps** : Tokyo route 100 km dure 8,8 s fenêtre déjà en mémoire. Le temps croît avec la
  densité et avec D (Shinjuku : 11,6 s en route 200 km, 25,7 s en gravel 200 km).
- **Les figures organiques en sont la plus grosse part dans ces zones** : même requête en `classic`, Tokyo route
  150 km 4,8 s contre 11,4 s, route 100 km 3,4 s contre 8,7 s, Shinjuku gravel 200 km 11,7 s contre 24,7 s, Yokohama
  gravel 150 km 10,3 s contre 19,9 s, Osaka route 150 km 2,7 s contre 7,8 s : ×2,0 à ×2,8, quand le banc français du
  lot 4c relevait ×1,25 à ×1,40.

### 3.2 Le moteur n'abandonne pas

Sur les 616 générations à budget levé (et les 228 à 8 et 30 s), **aucune** n'a fini en `budget` (`NoRouteDetail::Budget`, budget `astar_budget`
épuisé). Les 422 sont les échecs ordinaires des tirages : `start_too_far` en mer (plein Atlantique, Taveuni),
`no_candidate` sur les réseaux trop maigres ou coupés pour la distance (Ushuaia, Suva, Tromsø en route, Anchorage et
Lima à 100–150 km, Hong Kong à 60 et 150 km, Reykjavik à 60 km, Nairobi à 150 km), et `network_too_small` à Istanbul
(point de la péninsule historique : les cinq types ; Taksim et Kadıköy rendent leurs boucles, sauf route 200 km à
Taksim et gravel 200 km à Kadıköy). Le budget de génération ne change aucun de ces refus, et relever `astar_budget` n'a
pas de cas à corriger : non mesuré, faute d'objet.

### 3.3 Le budget ne change pas les parcours

Le moteur est déterministe à graine égale et le budget ne fait que l'arrêter. Vérifié : les 262 paires froid / chaud
rendent les mêmes issues et les mêmes longueurs, et la série France rejouée à **8 s** rend exactement les 60 mêmes
réponses qu'à 120 s. Un budget relevé ne change que des 504 en 200.

## 4. Concurrence (quatre clients, zones denses, pages froides au départ)

Durée vue par le client (attente de la file comprise), en secondes.

| Mélange | Budget | n | Médiane | p90 | p99 | Max | 200 | 422 | 504 | 503 |
|---|---|---|---|---|---|---|---|---|---|---|
| 60–150 km | 120 s | 72 | 4,71 | 7,76 | 14,55 | 14,55 | 65 | 7 | 0 | 0 |
| 60–150 + 200 km | **8 s** (1.13) | 84 | 5,21 | 8,02 | 8,11 | 8,11 | 61 | 7 | **16** | 0 |
| 60–150 + 200 km | **30 s** | 84 | 5,72 | 16,49 | 30,30 | 30,30 | **77** | 7 | **0** | 0 |

- Avec deux places de calcul et quatre clients, l'attente double à peu près la durée vue : la médiane passe de 1,3–2 s en
  série à 4,7–5,7 s. Aucun 503 : la file (`max_queued` 8) n'a jamais plus de deux demandes.
- À 8 s, **19 %** des générations denses finissent en 504 sous cette charge, pas seulement à Tokyo (Séoul route 60 et
  100 km, attente comprise).
- À 30 s, toutes passent ; la plus longue (Osaka gravel 200 km, 30,3 s, rendue juste après l'échéance par le
  contrôleur) dit que 30 s est tenu de justesse dans le pire cas mesuré : quatre requêtes japonaises de 200 km en même
  temps.
- Pic anonyme 917 à 972 Mo, 0 `oom` : le budget de mémoire (800 Mo) ne bouge pas.

## 5. Décision

1. **`generator.budget_ms` passe de 8 000 à 30 000** (un seul chiffre global). Il couvre 100 % des générations mesurées
   en série, 200 km dans la région de Tokyo compris (25,7 s au plus, 15 % de marge), toutes les zones denses de 60 à
   150 km avec une marge de 2,5 (11,7 s au plus) et la France avec une marge de 4,7 (6,4 s au plus à 200 km). Un budget
   par densité de fenêtre n'est pas retenu : le budget ne pèse que sur les requêtes qui dépassent, les autres finissent
   au même instant, et un seul chiffre reste lisible pour l'appelant (`/v1/capabilities` le publie déjà dans
   `generator.limits.budget_ms`).
   - À modifier : `crates/ord-server/src/config.rs`, `impl Default for GeneratorConfig` (`budget_ms: 30_000`) et le
     commentaire du champ ; journal des changements (section « Changé », avec ces chiffres) ; version 1.14.0.
   - Tests à ajouter : un test de configuration qui fixe le défaut (`Config::default().generator.budget_ms == 30_000`)
     et que `ORD_GENERATOR__BUDGET_MS` le remplace ; un test du service qui vérifie que `/v1/capabilities` publie
     `generator.limits.budget_ms` = 30 000 par défaut.
   - **France inchangée** : mêmes parcours (§3.3), mêmes durées (≤ 6,4 s, toutes sous l'ancien budget).
2. **`astar_budget` et les autres réglages du moteur ne bougent pas** : aucun abandon `budget` en zone dense (§3.2). Les
   références figées et les parcours français restent identiques au bit près. Piste pour plus tard, hors 1.14.0 : le coût
   des figures organiques en ville très dense (×2 à ×2,8, §3.1) ; le réduire changerait des parcours.
3. **`max_concurrent` 2 et `max_queued` 8 ne bougent pas.** Avec 30 s, une demande en file attend plus longtemps
   derrière deux générations japonaises de 200 km, mais elle a le même budget ; aucun 503 sous quatre clients, mémoire
   inchangée. Relever `max_concurrent` dépend des cœurs du serveur, non mesurés ici.
4. **Délai du backend Pédalons** (`pedalons.route-generator.generate-timeout`, aujourd'hui `PT10S`) : **`PT35S`** =
   budget 30 s + altitude des propositions 2,5 s (`ascent-budget`) + 2,5 s de marge (réseau, contrôleur qui finit
   juste après l'échéance : +0,3 s relevé). Le proxy entre les deux (Traefik) ne doit pas couper avant.

## 6. Ce qui n'a pas été mesuré

- **Le serveur de production** : processeur et disque inconnus d'ici (aucun accès). Les durées sont celles d'un Ryzen
  7 4800H sur NVMe ; un cœur plus lent allonge d'autant le calcul, et un disque plus lent la part des fenêtres (0,5 à
  1,3 s ici). À suivre après déploiement par l'histogramme `ord_generator_duration_seconds` et le compteur
  `ord_generator_requests_total{outcome="timeout"}`.
- **Les dépôts d'équipe sur la planète** et les modes `team_first` / `strict` : aucune surcouche déposée (données
  privées) ; la construction d'une équipe se fait dans le même budget, qui ne peut que l'aider.
- **La liste exacte des 14 lieux** du §10.4 : reconstituée (§2).
- **Le cap des allers simples** : durées et issues relevées (12 × 200, 4,1 s au plus), pas l'écart au cap.
- **Istanbul, péninsule historique** : `network_too_small` pour les cinq types ; non expliqué ici (accroche dans une
  composante courte), sans lien avec le budget.
