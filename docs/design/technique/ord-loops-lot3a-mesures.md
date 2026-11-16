# Générateur de parcours — lot 3a : vérification de bout en bout sur la vraie région

> Relevé du 16 novembre 2026, binaire `release` de la branche `feat/ord-graph` (arbre non commité), sur le graphe et
> les vues du relevé du lot 2, les surcouches **déposées par HTTP** depuis l'export privé du banc et les 323 requêtes
> de la campagne du lot 2. Conception : [ord-loops-lot3a.md](ord-loops-lot3a.md). Tous les chiffres viennent d'une
> commande lancée pour ce relevé ; sorties brutes (privées, hors dépôt) sous `~/.cache/osm-route-details/graph/lot3a/verify/`.
> Équipes lettrées A à I comme aux relevés précédents ; ni coordonnée, ni identifiant d'équipe, ni secret ici.

## 0. Verdict

**Le lot 3a tient sur la vraie région.** Les neuf surcouches déposées par HTTP sont **identiques en contenu** à celles de
l'outil du lot 1 ; les 1 292 générations rejouées par HTTP (323 requêtes × 4 valeurs du curseur) rendent **exactement**
les mêmes issues et les mêmes parcours (géométrie au millionième de degré comprise) que le moteur en processus avec les
mêmes paramètres ; la découverte est identique au lot 2 sur les 323 requêtes ; aucun parcours n'a été refusé par le
contrôleur dans le service ; le cloisonnement tient (corps identiques octet pour octet hors `instance`, durées
indiscernables) ; les surcouches survivent au redémarrage ; un nouveau graphe les rend `stale` puis la purge les retire ;
rien de privé dans les journaux ni les métriques ; le service est resté sous 1,1 Gio dans une limite cgroup de 2 Gio.

Réserves mesurées, aucune bloquante pour le lot 3b : (1) le contrôleur coûte **≈ 120 ms par génération stricte réussie**
et ≈ 25 ms pour les autres, parce qu'un contrôleur neuf par requête reconstruit ses tables sur tout le graphe (§4.3) ;
(2) un environnement **mesure la durée d'un dépôt en cours d'un autre** (son propre dépôt attend 42,8 s derrière celui de
A, §5.3) ; (3) un client qui ne respecte pas `Retry-After` fait écrire **une ligne `ERROR` par 503** (110 115 lignes en
60 s, §7) ; (4) aux crans 0 et « équipe d'abord », les issues diffèrent du lot 2 **uniquement** parce que le curseur fixe
la familiarité et λ (prouvé au §3.2), ce qui était voulu.

## 1. Montage

| Élément | Valeur |
|---|---|
| Machine | AMD Ryzen 7 4800H, 16 fils, 30 Gio ; aucune autre charge lourde pendant les mesures |
| Limite mémoire | `systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0` (cgroup v2, `memory.max` = 2 147 483 648 relu) ; **aucune limite CPU** |
| Service | `osm-route-details serve`, `MALLOC_ARENA_MAX=2`, journal JSON, défauts de configuration du générateur (2 places de calcul, file de 8, budget 8 s ; dépôt : une place, file de 120 s, budget 900 s) |
| Graphe, vues, polygone | ceux du relevé du lot 2 (`graph_id` `4a8ab3fc75a87044`) |
| Corps de dépôt | NDJSON produit depuis l'export par un script local, une ligne par trace comme le collerait le backend (texte du document tel que stocké, aucun document ré-encodé, aucun document vide) |
| Environnements | deux secrets K tirés au hasard (32 octets) : env. 1 = A, C, E, G, I ; env. 2 = B, D, F, H ; `Ord-Team` = l'identifiant d'équipe du backend |
| Altitude | cache rempli une fois en ligne par un parcours **synthétique** de 30 km sans lien avec une équipe, puis service relancé `offline` |

Six lancements : (1) en ligne, dépôts, banc HTTP, cloisonnement ; (2) redémarrage hors ligne sur le même volume, charge
d'altitude et de génération, file de dépôt ; (3) graphe reconstruit (autre identité) ; (4) même graphe, purge sans grâce ;
(5) fichier de graphe absent ; (6) retour au graphe d'origine, dépôt compressé.

## 2. Dépôts

### 2.1 Durées et mémoire (lancement 1, dépôts successifs dans l'ordre du tableau)

| Équipe | Env. | Traces | Corps | Durée (client) | Arêtes stockées | Fichier stocké |
|---|---|---|---|---|---|---|
| A | 1 | 2 726 | 703,1 Mo | **44,7 s** | 140 766 | 662 831 o |
| B | 2 | 446 | 66,9 Mo | 4,5 s | 27 053 | 136 061 o |
| C | 1 | 108 | 30,2 Mo | 6,7 s | 19 196 | 81 901 o |
| D | 2 | 71 | 38,3 Mo | 8,7 s | 20 316 | 84 578 o |
| E | 1 | 10 | 6,1 Mo | 6,4 s | 15 601 | 65 180 o |
| F | 2 | 1 | 0,38 Mo | 0,95 s | 1 216 | 5 424 o |
| G | 1 | 1 | 0,45 Mo | 1,07 s | 1 588 | 6 935 o |
| H | 2 | 7 | 0,99 Mo | 0,02 s | 0 (hors région) | 470 o |
| I | 1 | 7 | 0,99 Mo | 0,02 s | 0 (hors région) | 470 o |

- Tous en 200 ; 73,9 s cumulés (`ord_overlay_deposit_seconds_sum`). Débit de A : 15,7 Mo/s. Le coût suit les géométries
  **nouvelles** et l'index par cellule construit à la demande, pas les octets : E (10 traces, 15 304 géométries à
  apparier) prend 6,4 s, F et G (une trace) ≈ 1 s chacune, l'index par cellule repartant de zéro à chaque dépôt.
- **Mémoire du dépôt de A, processus neuf** : `VmRSS` au départ 469 308 ko (232 884 anonymes, 236 424 de fichiers) ;
  `VmHWM` après le dépôt **684 664 ko** (+215 356 ko) ; pic du cgroup 645 172 ko. La mémoire n'est pas rendue : `VmRSS`
  reste à 668 880 ko après le dépôt (glibc).
- **Pire cas relevé** : `VmHWM` **1 070 540 ko** après le banc HTTP complet (trois équipes en cache, 5 168 générations),
  pic du cgroup 1 033 416 ko, `memory.events` : `oom 0`. Au lancement 2, dépôt de A avec trois équipes déjà en cache :
  `VmHWM` 1 055 488 ko, pic du cgroup 937 568 ko. Sous 4 et 12 clients de génération : 855 072 ko. Aucune requête
  d'altitude **à froid** n'a tourné en même temps (non mesuré).
- **Compressé** (lancement 6) : A en gzip (161,5 Mo envoyés, `Content-Encoding: gzip`) : 44,7 s, `VmHWM` 683 832 ko,
  contenu identique.

### 2.2 Les surcouches stockées contre celles de l'outil du lot 1

Comparaison par un programme local qui dérive les noms et la clé de chiffrement de K (comme le service), déchiffre le
fichier, décode l'`.ordo` et le compare à celui de `tools/ord-graph-overlay` :

- **Neuf sur neuf : mêmes arêtes, mêmes comptes, même pas (`stride` 8), dans le même ordre.** Mêmes `graph`,
  `params_hash`, `types`.
- Différences de métadonnées, toutes voulues : `team_id` = 0 (l'équipe n'est connue que par son nom dérivé),
  `export_xxh3` = 0 et `export_rows` = nombre de traces de l'équipe (l'outil met l'empreinte et le nombre de lignes de
  tout l'export), `tool_version` et `build_epoch` du service.
- Volume : un dossier `graph-<id>`, deux dossiers d'espace de 32 hexadécimaux, neuf fichiers `<64 hex>.ordo.enc` ;
  aucun autre nom.

## 3. Génération par HTTP contre la campagne du lot 2

### 3.1 Ce que le banc a gagné

`ord-loops-bench` reçoit un **mode HTTP** : `--http <url> --keys <fichier local> --cursors 0,0.01,0.25,1`. Chaque
requête du banc part en `POST /v1/generator/routes` avec la clé de l'environnement de l'équipe et sa référence, un tour
froid puis 3 tours chauds (médiane), comme en processus. `--cursors` vaut aussi **en processus** : le mode, la familiarité
et λ viennent alors de `ord_server::generator::cursor` (le même code que le service). Chaque parcours porte l'empreinte de
sa géométrie arrondie au millionième (`geom_xxh3`, la précision de la polyligne rendue), en processus comme en HTTP.

### 3.2 Issues et parcours, requête par requête

| Curseur (service) | Mode, λ, familiarité | Réussites HTTP | En processus, mêmes paramètres | Écarts HTTP / processus | Lot 2 (`camp3`) | Identiques au lot 2 |
|---|---|---|---|---|---|---|
| 0 | strict, —, 1 | 90 | 90 | **0** | strict : 91 | 252 / 323 |
| 0,01 | équipe d'abord, 2,98, 0,5 | 293 | 293 | **0** | équipe d'abord (λ 3) : 299 | 103 / 323 |
| 0,25 | équipe d'abord, 2,50, 0,5 | 301 | 301 | **0** | équipe d'abord : 299 | 29 / 323 |
| 1 | découverte, 1, 0,5 | 305 | 305 | **0** | découverte : 305 | **323 / 323** |

- **HTTP = processus** sur les 1 292 requêtes × curseurs : même issue, même nombre de parcours, même géométrie, mêmes
  longueur, part d'équipe, part comblée, recouvrement et croisements (tolérance 0,06 m, l'arrondi au décimètre du
  service), même famille. Mode et λ rendus par le service : ceux attendus à chaque cran. 2 520 parcours rendus, **0
  retiré** par le contrôleur du service (`dropped` = 0, aucune ligne `route rejected`), 0 violation en processus.
- **Le moteur actuel rejoue `camp3` à l'identique** avec les paramètres du lot 2 (familiarité de la requête Java,
  λ = 3) : 323 / 323 en strict, 323 / 323 en équipe d'abord (même issue, mêmes parcours). Les écarts au lot 2 ne viennent
  donc **que** des paramètres que le curseur fixe :
  - **cran 0** : les 90 requêtes Java à familiarité 1 sont identiques (23 réussites des deux côtés). Les 233 autres
    passent de la familiarité de la requête (0,5 ou 0) à 1 : 138 à 0,5 → 45 réussites contre 42, 95 à 0 → 22 contre 26 ;
    3 gagnées, 3 perdues (`NO_CANDIDATE`), 1 perdue au budget interne de recherche (`NO_ROUTE:BUDGET`) ; 64 réussites
    des deux côtés n'ont pas les mêmes parcours (51 longueurs, 13 nombres de parcours).
  - **équipe d'abord** : aucun cran ne redonne le λ = 3 du lot 2 (le cran 0 est le strict, 0,01 donne 2,98) et la
    familiarité est fixée à 0,5. À 0,01 : 10 perdues, 4 gagnées ; à 0,25 : 4 perdues, 6 gagnées, 1 échec qui change de
    cause. Le cran 0,01 réussit **moins** que 0,25 (293 contre 301) : la réussite n'est pas monotone en λ.
- Part d'équipe (équipe + comblé) du premier parcours, p10 / p50 / p90 : cran 0,01 : 0,507 / 0,810 / 0,994 ; 0,25 :
  0,510 / 0,799 / 0,995 ; découverte : 0,056 / 0,303 / 0,947 (lot 2, équipe d'abord : 0,509 / 0,807 / 0,993).

### 3.3 Latence ajoutée par HTTP

Générations réussies, un client, une requête à la fois ; médiane des 3 tours chauds ; « serveur » = `duration_ms` du
journal (de la validation à la réponse prête, contrôleur compris), aligné requête par requête ; « moteur » = la même
requête en processus (moteur seul, sans contrôleur).

| Curseur | n | Client HTTP p50 / p90 | Moteur en processus p50 / p90 | Client − serveur p50 / p90 / max | Serveur − moteur p50 / p90 |
|---|---|---|---|---|---|
| 0 | 90 | 165 / 312 ms | 47 / 174 ms | 1,9 / 3,1 / 3,8 ms | **120 / 131 ms** |
| 0,01 | 293 | 263 / 822 ms | 242 / 788 ms | 2,1 / 3,3 / 4,9 ms | 25 / 35 ms |
| 0,25 | 301 | 255 / 826 ms | 227 / 791 ms | 2,1 / 3,3 / 4,7 ms | 25 / 35 ms |
| 1 | 305 | 197 / 540 ms | 172 / 504 ms | 2,2 / 3,3 / 4,5 ms | 26 / 37 ms |

- **HTTP, JSON et polyligne : 2 ms** à la médiane, moins de 5 ms au maximum, sur la boucle locale.
- **Le contrôleur et le rendu : 25 ms**, et **120 ms en strict**. Sur les générations **sans** parcours, serveur − moteur
  vaut 0,0 ms (strict) à 1,4 ms : le surcoût est entièrement après le moteur. Cause **probable, non mesurée isolément** :
  `Checker::new` par requête (§6 de la conception) et ses tables paresseuses sur **tout** le graphe (degrés admis,
  incidence triée, degrés d'équipe ; en strict, en plus, l'ensemble des arêtes comblées recalculé sur toute la
  surcouche), reconstruites à chaque requête ; le banc du lot 2 gardait un contrôleur par équipe, d'où des caches chauds.

## 4. Charge, altitude, mémoire (lancement 2, hors ligne)

### 4.1 Altitude pendant les générations

`POST /v1/route-details` sur le parcours synthétique (600 points, 30 km), cache disque et mémoire chauds, 15 requêtes par
série ; générations en boucle sur A, curseur 0,25, 100 à 150 km.

| Série | `fast` p50 / p90 / max | `full` p50 / p90 / max |
|---|---|---|
| seule | 27,1 / 29,5 / 60,0 ms | 31,2 / 32,9 / 35,1 ms |
| 4 clients de génération (2 places occupées, file non vide) | 28,6 / 33,9 / 46,6 ms | 19,7 / 27,5 / 28,1 ms |
| 12 clients de génération, sans attente entre deux 503 | 27,6 / 49,5 / 51,6 ms | 17,3 / 25,7 / 26,6 ms |

L'altitude en cache ne souffre pas des générations **sur cette machine** (16 fils, pas de limite CPU) : les générations
sont bornées à 2 fils. Non mesuré : altitude à froid (tuiles à télécharger) pendant les générations, conteneur limité en CPU.

### 4.2 Générations sous charge

- 4 clients pendant 60 s : 198 réussites, 8 `422`, **aucun 503 ni 504** ; p50 1 183 ms, p90 1 738 ms, max 2 372 ms
  (100 à 150 km, deux places).
- 12 clients qui relancent aussitôt : 199 réussites et 6 `422` (le débit des deux places ne change pas), **110 113 réponses
  503 `overloaded` immédiates** (≈ 1 ms), max 4 573 ms pour une acceptée (attente dans la file, dans le budget de 8 s),
  aucun 504.

## 5. Cloisonnement, sur le service avec les vraies surcouches

### 5.1 Contre une équipe de l'environnement 1 (A)

| Appelant | `GET …/overlay` | `POST …/routes` curseur 0,5 | `PUT` | `DELETE …/overlay` | `DELETE …/overlays` |
|---|---|---|---|---|---|
| clé de l'env. 2, référence de A | 200 `absent` | 409 `overlay_required`, `state: absent` | 200 : crée **la surcouche de l'env. 2** sous cette référence ; celle de A ne change pas (état relu identique) | 204 ; A reste `ready` | — |
| clé inconnue, référence de A | 200 `absent` | 409 idem | — | — | 204 ; A reste `ready` |
| clé de l'env. 1, équipe jamais déposée | 200 `absent` | 409 idem | — | 204 | — |
| clé de l'env. 2, équipe jamais déposée | 200 `absent` | 409 idem | — | — | — |
| sans clé (référence de A, d'une équipe absente, ou aucune) | 400 `/headers/Ord-Namespace-Key` | 400 idem | 400 idem | 400 idem | 400 idem |

- **Corps identiques octet pour octet**, hors `instance`, entre les quatre premières lignes : 200 tours × 2 opérations, un
  seul corps distinct par opération et par cas, le même pour tous les cas.
- **Durées indiscernables** (ordre tiré au hasard à chaque tour, connexion gardée, 200 mesures par cas) : `GET` p50 0,277
  à 0,281 ms, p90 0,424 à 0,468 ms ; `POST` refusé p50 0,395 à 0,399 ms, p90 0,581 à 0,620 ms. Le propriétaire, lui, lit
  et déchiffre son fichier : `GET` p50 7,2 ms (voulu : seul l'environnement propriétaire le voit).
- Curseur 1 : sans clé, 200 sans surcouche ; avec la clé de l'env. 2 et la référence de A, 200, `overlay: null`, parts
  d'équipe nulles : rien de A ne transparaît. Avec la clé de l'env. 1 : surcouche utilisée, parts d'équipe positives.
- Après les tentatives, A génère toujours (200, 3 parcours) ; la surcouche que l'env. 2 avait créée sous la référence de
  A est bien effacée (`absent`).

### 5.2 Redémarrage

Lancement 2 sur le même volume : les neuf équipes `ready`, même `deposited_at` et même nombre d'arêtes qu'au dépôt ;
une génération à 50 km par équipe : 200 pour A à G (2 ou 3 parcours, 272 à 576 ms, chargement de l'équipe compris),
`422 out_of_coverage` (`coverage: graph`) pour H et I, dont les départs sont hors région.

### 5.3 Ce que le cloisonnement ne couvre pas, mesuré

Pendant le dépôt de A (env. 1, 44,9 s) : un second dépôt de l'env. 1 reçoit **503 en 0,02 s** ; un dépôt de l'env. 2
(F) **attend 42,8 s** puis réussit. L'env. 2 apprend donc qu'un dépôt d'un autre environnement est en cours et combien de
temps il dure, soit à peu près la taille de l'équipe. C'est le prix de la place de dépôt unique, connu de la conception
(§2, disponibilité) ; il est maintenant chiffré.

### 5.4 Changement de graphe

Graphe reconstruit depuis le même PBF au zoom 12 (autre identité : `20cbfc8172a7677c`), vues reconstruites, même dossier
des surcouches :

- `capabilities.generator.graph.id` change ; les neuf équipes sont `stale`, `stale_reason: graph`, `deposited_at` = date
  du fichier, `edges` absent ; curseur 0,25 → **409 `overlay_required`, `state: stale`** ; curseur 1 → 200, `overlay:
  null`, nouveau `graph_id` ; H et I → 422 `out_of_coverage`.
- **Ce que fait le backend** : il voit l'identifiant changer (ou `stale` équipe par équipe) et redépose. F, G, B
  redéposés : 0,69 s, 0,76 s, 4,12 s, aussitôt `ready` ; les six autres restent `stale` jusqu'à leur dépôt.
- Relancé avec `stale_grace_h = 0` : la purge de démarrage retire l'ancien dossier (`stale_graph` 9,
  `ord_overlay_purged_total{reason="stale_graph"}` 9, `ord_overlays_stored` 3) ; les six non redéposées passent `absent`.
  Retour au graphe d'origine (lancement 6) avec la grâce par défaut : le dossier du zoom 12 est gardé.

### 5.5 Sans graphe

Fichier de graphe absent : démarrage, `/health/live` et `/health/ready` 200, `capabilities.generator` `enabled: false`,
`reason: graph_unreadable` ; génération 503 `data_unavailable` (`source: graph`) ; `/v1/route-details` 200 (24 ms). Une
ligne `WARN` qui cite le chemin configuré.

## 6. Journaux et métriques de la session

13 fichiers (journaux JSON des six lancements et `/metrics` de chacun), 118 Mo :

- **Identifiants d'équipe** (les neuf) : 0 occurrence. **Secrets K** : 0 ; aucune chaîne de 43 caractères base64url.
- **Noms dérivés** recalculés par HMAC pour les neuf équipes sous les deux clés (dossier d'espace complet, nom de fichier) :
  0 occurrence. Seule l'étiquette `ns8` (non secrète) apparaît, comme prévu.
- **Coordonnées** des départs du banc, écrites à 4, 5, 6 ou 7 décimales ou en écriture complète : 35 occurrences, **toutes
  dans des horodatages** (chiffres des secondes). Aucun champ de journal ne porte de coordonnée ni de polyligne (champs
  relevés : comptes, durées, modes, tranches de distance, causes, `ns8`).
- Métriques : aucune étiquette d'équipe ni d'espace.

## 7. Changements faits pendant la vérification

1. `tools/ord-loops-bench` : mode HTTP (`--http`, `--keys`), `--cursors` en processus et en HTTP, empreinte de géométrie
   `geom_xxh3`, champs `cursor`, `http_status`, `dropped`, `applied` au rapport (`src/http.rs`, `main.rs`, `record.rs`) ;
   dépendances `ord-server` (la correspondance du curseur), `reqwest` et `tokio` (client asynchrone sur un runtime d'un fil),
   `xxhash-rust` : **aucune caisse nouvelle** dans `Cargo.lock`. Tests : décodage de polyligne, codes d'échec.
2. `crates/ord-server` : `/v1/capabilities` publie `limits.storage.max_namespace_mb` (le quota qui décide d'un 507 depuis
   la revue, qui n'était pas publié) et `limits.deposit.queue_wait_s` ; contrat régénéré ; assertions ajoutées au test
   `coverage_is_explicit_in_errors_and_in_capabilities`. Ajouts seulement, dans l'api 1.9 de ce lot.
3. `crates/ord-server` : `team_m` valait `-0.0` dans le JSON pour un parcours sans arête d'équipe (relevé au §5.1) ;
   l'arrondi rend désormais `0.0`. Test `rounding_never_renders_a_negative_zero`. Les mesures des §2 à §5.1 sont faites
   avec le binaire d'avant ces deux changements, celles des lancements 2 à 6 avec le binaire d'après.

`cargo test -p ord-server -p ord-loops-bench`, `cargo clippy --all-targets` et `cargo fmt --check` passent.

## 8. Questions au propriétaire, avec ce que les mesures en disent

1. **Coût du contrôleur dans le service** : +120 ms par génération stricte réussie, +25 ms sinon (§3.3), et des tables sur
   tout le graphe allouées à chaque requête. Garder un contrôleur neuf par requête (simple, déjà dans la cible de temps),
   ou garder ses tables par graphe et par équipe en cache (change l'API de `ord-loops-check`, à faire avant ou pendant
   le lot 3b) ?
2. **Cran par défaut du curseur** : 0,25 réussit 301 fois sur 323 avec une part d'équipe médiane de 0,799 ; 0,01 réussit
   moins (293) pour 0,810. Fixer le défaut à 0,25, et garder un cran visible à 0 (strict : 90 réussites) ?
3. **Fuite de calendrier par la place de dépôt** (§5.3) : l'accepter (seuls des environnements à nous partagent le réseau),
   ou payer une place par environnement (un fil et ≈ 200 Mio de plus par dépôt simultané), ou une instance par environnement ?
4. **503 journalisés en `ERROR`** par la couche de traces : 110 115 lignes en une minute avec un client qui ne respecte pas
   `Retry-After`. Rétrograder les 503 `overloaded` (et 409 `overlay_required` ?) en `warn` ou les échantillonner ? Le
   client Java devra de toute façon respecter `Retry-After`.
5. **Équipes hors région** : leur dépôt réussit avec **0 arête** et l'état dit `ready`, mais toute génération rend 422. Le
   backend doit-il cesser de déposer une équipe dont le rapport dit `edges: 0`, et masquer le générateur pour elle ?
6. **Après un changement de graphe**, tout le monde est en 409 jusqu'au redépôt (≈ 75 s pour les neuf équipes, en série).
   Le backend redépose-t-il dès qu'il voit `graph.id` changer (sans attendre la nuit) et bascule-t-il en découverte
   (curseur 1) entre-temps, ou affiche-t-il une erreur ?
7. **Mémoire** : 1,07 Gio au pire relevé, sans altitude à froid ni limite CPU. Rester à 2 Go, avec une mesure dans le vrai
   conteneur au lot 3b ?
8. **`storage.allowed_namespaces`** : l'activer en production avec les préfixes des environnements connus, pour qu'aucun
   inconnu du réseau partagé ne prenne une des 16 places d'espace ?
9. Toujours ouvertes (conception §14) : réseau partagé chiffré pour que K n'y circule pas en clair (question 8), distance
   maximale de 200 km contre 300 km dans le contrat Pédalons (question 6), D+ des propositions (question 4).

## 9. Reproduction (hors dépôt)

Scripts locaux non versionnés : conversion de l'export en NDJSON par équipe, deux secrets tirés au hasard et le fichier des
clés, lancement sous `systemd-run`, dépôts par `curl -T`, comparaison des surcouches (programme Rust qui utilise
`ord_server::generator::crypto` et `ord_graph::overlay`), cloisonnement et durées (`urllib`, `http.client`), altitude
sous charge, recherche des données privées dans les journaux. Banc :

```sh
ord-loops-bench --http http://127.0.0.1:28080 --keys <clés> --cursors 0,0.01,0.25,1 --rounds 3 --label http …
ord-loops-bench --cursors 0,0.01,0.25,1 --rounds 3 --label inproc …
ord-loops-bench --modes strict,team --rounds 0 --label lot2replay …
```
