# Générateur de parcours — lot 3a : le moteur servi par osm-route-details, conception

> Statut : **codé** (api 1.9) ; la conception ci-dessous est celle d'origine, les écarts du codage sont au §17 et les
> relevés sur la vraie région au §18. Exigences : plan Pédalons `docs/plans/2026-11-15-generateur-parcours-osm.md`
> (§11, §12 quater et décisions du 16 novembre 2026). Socle : [lot 1](ord-graph-lot1.md), [lot 2](ord-loops-lot2.md) et leurs
> relevés. **[mesuré]** : sonde jetable hors dépôt du 16 novembre 2026 (§16), même graphe, mêmes vues, mêmes surcouches et
> mêmes 323 requêtes que le relevé du lot 2 ; **[calculé]** depuis des mesures citées ; **[estimé]** : à relever au codage.
> Équipes lettrées comme aux relevés précédents. Ni coordonnée ni identifiant d'équipe ici.

## 0. Ce que le lot livre

Pour le backend Pédalons (client Java, lot 3b) : **déposer** la surcouche d'une équipe (le service apparie les documents
`network_edges`), **demander des parcours**, **effacer**, et l'**état** d'une surcouche (faut-il redéposer ?). Le contrôleur
indépendant tourne dans le service sur chaque parcours. api **1.9**, ajouts seulement.

## 1. Routes et corps

| Route | Rôle | En-têtes | Réponse |
|---|---|---|---|
| `PUT /v1/generator/overlay` | dépôt en flux (§4) | `Ord-Namespace-Key`, `Ord-Team`, `Ord-Source-Digest` (facultatif) | 200 rapport de dépôt |
| `GET /v1/generator/overlay` | état | `Ord-Namespace-Key`, `Ord-Team` | 200 toujours (`state`) |
| `DELETE /v1/generator/overlay` | efface une équipe | idem | 204 toujours (idempotent) |
| `DELETE /v1/generator/overlays` | efface tout l'espace (rotation de clé, poste abandonné) | `Ord-Namespace-Key` | 204 |
| `POST /v1/generator/routes` | génération | `Ord-Namespace-Key`, `Ord-Team` (facultatifs si `cursor` = 1) | 200 parcours |

**Aucun identifiant dans le chemin ni dans la requête** : `TraceLayer` journalise `path`, les métriques le gabarit de route ;
les en-têtes ne sont jamais journalisés. `Ord-Namespace-Key` : 32 octets aléatoires en base64url sans remplissage
(43 caractères, autre longueur → 400). `Ord-Team` : référence opaque du backend (son TSID d'équipe), `[A-Za-z0-9_-]{1,64}`.

**Cadrage du dépôt** : `Content-Type: application/x-ndjson`, `Content-Encoding: gzip` admis (couche de décompression
existante, bornes comptées **après** décompression). Une ligne JSON par trace (`\n`, CRLF toléré ; clés inconnues refusées
sur la ligne, tolérées dans le document comme au lot 1) : `{"type":"ROAD","excluded":false,"network_edges":{…tel que
stocké…}}`, `type` = `surface_type` (`ROAD`, `GRAVEL`, `MTB`, autre → `MIXED`), `network_edges` objet ou `null` (illisible).
**Dernière ligne obligatoire** `{"end":{"tracks":N}}` : sans elle ou si N est faux, 400 et la surcouche précédente reste.
Le backend colle le texte stocké (jsonb, sans saut de ligne) entre `…"network_edges":` et `}`.

**Génération**, corps JSON strict (`StrictJson`, 64 Kio) :
`{"start":{"lat","lon"},"end":{…}|null,"bearing":"N"…"NW"|null,"distance_m","ride_type":"road|gravel|mtb","cursor":0..1,
"seed":u64 (défaut 0)}`. Réponse : `graph_id`, `mode` et `lambda` appliqués (§5), `coverage` (`inside` ou `partial` :
disque D/2 qui sort du polygone, `Poly::contains_disc`), `overlay` (`deposited_at` ou `null`), `dropped` (parcours refusés
par le contrôleur), `routes[]` (au plus 3, meilleur d'abord, §6).

**État** (`GET`) : `state` (`absent` | `ready` | `stale`), `stale_reason` (`graph` | `matcher`), `graph_id`, `deposited_at`,
`expires_at`, `source_digest`, `edges`, `team_km`. Redépôt si `state ≠ ready`, `source_digest` différent ou `expires_at` proche.

## 2. Cloisonnement entre environnements

**Schéma.** Chaque backend détient un secret d'environnement K (32 octets, `openssl rand`, jamais partagé entre
environnements). Le service n'en garde **aucun** et n'a aucun registre : tout est dérivé de K à chaque requête, par
HMAC-SHA256 à séparation de domaine (`ring`, déjà dans `Cargo.lock` : aucune caisse nouvelle) :
`ns = hex(HMAC(K, "ord/ns/v1")[..16])` (dossier), `team = hex(HMAC(K, "ord/team/v1\0" ‖ Ord-Team))` (fichier),
`enc = HMAC(K, "ord/enc/v1")` (clé ChaCha20-Poly1305 du fichier). Un appelant ne peut **nommer** que des fichiers dérivés de sa
propre clé : il ne peut ni lire, ni utiliser, ni écraser, ni effacer, ni sonder ceux d'un autre, faute de pouvoir en
former le nom.

| Menace | Réponse | Reste |
|---|---|---|
| Autre environnement sur le réseau (préproduction compromise, poste) | noms dérivés de K ; aucune route de liste ; `GET` rend `absent` et `DELETE` 204 sans distinguer « jamais déposé », « autre clé » ou « autre espace » | il peut saturer le calcul (§8) ; le dépôt est équitable (§4), les quotas sont par espace |
| Journal qui fuit | ni K, ni `Ord-Team`, ni nom de fichier, ni coordonnée ; seule l'étiquette `ns8` (8 premiers hex de `ns`, non secrète) | `ns8` relie les requêtes d'un même environnement |
| Sauvegarde ou copie du volume | fichiers chiffrés (AEAD, nonce aléatoire de 96 bits par écriture, données associées = magie ‖ `graph_id` ‖ `ns` ‖ `team`) : contenu illisible, permutation de fichiers détectée | métadonnées : nombre d'espaces, d'équipes, tailles, dates. Volume + K (sauvegarde du `.env` du backend) = tout |
| Traversée de chemin | noms = hex calculés par le service, jamais une chaîne du client ; format des en-têtes validé avant tout | — |
| Temps ou codes qui trahissent l'existence | entre environnements, rien à mesurer : la requête ne peut pas atteindre le fichier d'un autre. Dans un même environnement, `absent` et `ready` diffèrent, c'est voulu | les quotas sont **par espace** (octets, surcouches) : un 507 ne dépend que de soi. Seul le nombre d'espaces (16) est partagé ; `storage.allowed_namespaces` (préfixes `ns8` de l'opérateur) empêche un inconnu d'en prendre un |

**Non couvert** : le secret circule **en clair** sur `pedalons-shared` (réseau overlay non chiffré) ; qui écoute le réseau ou
lit la configuration d'un backend **est** cet environnement ; root sur l'hôte voit la mémoire (surcouches déchiffrées en cache) ;
la **disponibilité** n'est protégée que par des bornes (un pair hostile peut encore occuper le calcul : file de 8, 2 places ;
un dépôt est borné à `budget_s` **dur**, le fil s'arrête de lui-même, et la place de dépôt est une file équitable ; sans
`allowed_namespaces`, il peut aussi prendre les espaces libres) ; le port `/metrics` ne doit pas être publié sur le réseau
partagé (la jauge `ord_overlays_stored` compte les surcouches de tous les environnements, la durée des dépôts trahit leur
taille) ; un `panic` ne journalise que son endroit (`set_hook`) ; deux environnements configurés avec le **même** K partagent leurs surcouches sans que le service le
voie ; `/metrics` (port 9090, même réseau) montre des comptes agrégés. Rotation : nouveau K = nouvel espace ; le backend
redépose tout, l'ancien espace est effacé par `DELETE /v1/generator/overlays` ou par la purge.

## 3. Stockage

```
<generator.overlay_dir>/                 (défaut /var/cache/osm-route-details/generator : le volume existant)
  graph-<graph_id>/<ns>/<team>.ordo.enc  surcouche chiffrée ; mtime = date du dépôt
  graph-<graph_id>/<ns>/<team>.tmp-<aléa> écriture en cours
```

`graph_id` = xxh3 hexadécimal des champs de `GraphIdentity::same_ids` (format, attributs, zoom, comptes, empreintes des ids) :
un graphe reconstruit à l'identique garde ses surcouches, un nouveau graphe les rend `stale`. Fichier : magie `ORDOENC1`,
version, nonce, puis chiffré : métadonnées JSON (`deposited_at`, `source_digest`, `matcher_hash` = `params_hash` du lot 1,
statistiques) et l'`.ordo` du lot 1 dont le champ `team_id` vaut **0**. Un fichier qui ne se déchiffre pas est traité
comme absent, effacé, compté (`ord_overlay_corrupt_total`).

**Remplacement atomique** : écriture du `.tmp` dans le même dossier, `fsync`, `rename`, `fsync` du dossier, puis seulement
remplacement de l'entrée du cache mémoire. Jusqu'au `rename`, l'ancienne surcouche sert ; le `rename` est le point de validation (un `fsync` du dossier qui échoue après lui est journalisé, le dépôt reste réussi) ; un dépôt qui échoue (flux, budget,
disque) efface son `.tmp` et laisse tout en l'état. Un verrou par `(ns, team)` en mémoire (instance unique) ; pendant la minute
d'un déploiement roulant, deux instances pourraient écrire la même équipe : le dernier `rename` gagne, jamais un fichier mêlé.

**Purge** (tâche horaire, sans clé, sur les `mtime`) : surcouche non redéposée depuis `overlay_ttl_days` (14) ; dossiers
`graph-*` d'un autre graphe après `stale_grace_h` (48 h, pour qu'une ancienne instance encore en vie serve pendant la
bascule) ; `.tmp` de plus d'une heure (au démarrage : de plus de 5 minutes, pour ne pas effacer l'écriture que finit l'ancienne instance d'un déploiement « start-first ») ; dossiers vides. Au démarrage, `state = stale` (raison `graph`)
quand le fichier n'existe que sous un autre `graph-*`.

**Cache mémoire** : `(ns, team)` → `Arc<(Overlay, TeamData)>`, LRU de `team_cache_entries` (3). `TeamData` est **dense** :
52 à 56 Mio pour A **[mesuré]**, du même ordre pour toute équipe puisque les listes sont denses (lot 2 §12.2, point 3) ;
construit en 141 à 212 ms, lecture de l'`.ordo` 3 ms **[mesuré]**. Une construction à la fois par équipe : une seconde demande
sur la même équipe froide attend la première et réutilise son entrée (pas de seconde copie de ≈ 55 Mio).

## 4. Dépôt : flux, mémoire bornée, apparieur en bibliothèque

Taille réelle **[mesuré, export du banc]** : A 2 726 traces, **703 Mo** de documents, 1 967 881 arêtes d'équipe mais
**198 985 géométries distinctes** (4,8 Mo de polylignes) ; plus gros document 4,2 Mo (D). Le texte se jette ligne par ligne ;
ce qui reste, c'est la mémoïsation par géométrie et le compte par arête de base.

**Flux** : un seul fil (`spawn_blocking`) lit le corps par trames (64 Kio), découpe les lignes (une ligne ≤ 16 Mio), lit le
document en serde sur les seuls champs utiles (`g`, `trv`, `len`, `ow`, `wk`, `fn`, `tn`, `fm`, `tm`, `fo`, `to`), apparie
chaque géométrie nouvelle, agrège la trace dans l'ordre d'arrivée, et lâche le texte. Agrégation séquentielle, donc
déterministe et identique à l'outil. Durée de A en un fil **[calculé]** : appariement 49 µs × 198 985 ≈ 10 s, lecture
≈ 4,5 s (6,3 s à 1 fil pour 981 Mo au lot 1), plus le transfert.

| Poste | Borne |
|---|---|
| ligne en cours + document lu | `deposit.max_document_bytes` (16 Mio) × 2 |
| mémo géométrie → chaîne | `deposit.memo_entries` (400 000) ; au-delà, éviction : seulement du calcul en plus, même résultat |
| compte par arête (`EdgeAcc`, 36 o) | arêtes de la région touchées : ≈ 140 000 pour A, ≈ 5 Mio **[calculé]** |
| **index spatial** | l'index entier de l'outil fait **402 Mio** pour la région **[mesuré]** (estimé à 150–200 Mo au lot 1), 0,4 s à 16 fils, 3 s à un : **interdit** dans le service. Index **par cellule, à la demande**, LRU de `deposit.index_cache_mb` (128) |

L'objection du lot 1 à l'index à la demande (un segment doit être connu des cellules voisines qu'il touche) se lève en
construisant une cellule depuis ses **9 cellules voisines** et en gardant les segments dont la boîte la touche : correct tant
qu'aucune arête n'est plus large qu'une cellule (5,6 km au plus contre ≈ 13 km pour une cellule z11 à cette latitude) ; le
service vérifie cette borne au chargement et sinon construit l'index entier. Un dépôt à la fois **pour tout le service**
(`deposit.max_concurrent` = 1) : l'index et le fil sont partagés. Total d'un dépôt ≈ 200 Mio **[estimé]**.

**Sans dupliquer le code** : nouveau crate `crates/ord-graph-match` qui reçoit de `tools/ord-graph-overlay` `geo`, `index`
(mode entier pour l'outil, par cellule pour le service), `matcher`, la lecture du document et ses codes (`export.rs` sans
le CSV), le cœur de `aggregate` (comptes et compteurs de causes ; ensembles et échantillons du rapport restent à l'outil),
`overlay_arrays`, `params_hash`, et une API `Deposit::push_track(ty, excluded, doc) / finish() -> (edges, counts, stats)`,
générique sur `GraphFile<B>`. L'outil garde le CSV, le rapport, la re-vérification et la CLI, et **passe par
`Deposit`** : un seul chemin d'appariement et d'agrégation. Les tests de l'outil restent la non-régression.

## 5. Curseur « routes de l'équipe » ↔ « découverte »

`cursor` ∈ [0, 1], arrondi au centième. **0 → strict** (familiarité 1) ; **1 → découverte** ; entre les deux **équipe d'abord**,
familiarité fixée à 0,5 (neutre : le réglage « rarement ↔ souvent » disparaît, plan §12 quater décision 4) et coût hors
équipe **λ(c) = 1 + 2 (1 − c)**, de 3 (le λ du lot 2) à 1 (le coût OSM seul, découverte sans le mode). Moteur : λ passe
de `Params` à la requête, en 256es (entiers, même poids en natif et en WASM). Publié tel quel dans `/v1/capabilities`.

**Mesure [mesuré]** : les 323 requêtes, un tour, 12 fils, contrôleur indépendant sur chaque parcours ; part d'équipe =
(`team_m` + `bridged_m`) / L du **premier parcours accepté** par le contrôleur, sur les requêtes réussies.

| c | 0 | 0,10 | 0,25 | 0,40 | 0,50 | 0,60 | 0,75 | 0,90 | 1 |
|---|---|---|---|---|---|---|---|---|---|
| λ | strict | 2,80 | 2,50 | 2,20 | 2,00 | 1,80 | 1,50 | 1,20 | découverte |
| réussites / 323 | 90 | 297 | 301 | 304 | 304 | 305 | 305 | 305 | 305 |
| part d'équipe, médiane | 1,000 | **0,811** | 0,799 | 0,771 | 0,748 | 0,721 | 0,677 | 0,572 | 0,303 |
| p10 / p90 | 1 / 1 | 0,513 / 0,990 | 0,510 / 0,995 | 0,450 / 0,983 | 0,475 / 0,982 | 0,454 / 0,982 | 0,405 / 0,980 | 0,276 / 0,963 | 0,056 / 0,947 |

- **La médiane décroît strictement** sur les neuf points ; p10 presque (0,450 → 0,475 entre 0,40 et 0,50). **Par requête, pas
  de monotonie** : entre deux crans voisins de l'équipe d'abord, 37 à 101 requêtes sur 297 à 305 voient leur part monter
  (11 à 53 de plus de 5 points), parce que l'ensemble des candidats change. Promesse tenable : « en moyenne », pas « pour cette requête ».
- **Saut 0 → 0+ structurel** : le strict est un filtre (90 réussites), l'équipe d'abord un coût (297) ; cran 0 à marquer.
- **Écartées, mesurées** : λ = 9^(1−c), familiarité 1 − c/2 : parts plates de 0,10 à 0,50 (0,876 → 0,850, remontée 0,837 →
  0,850), 275 réussites et p50 405 ms à 0,10 ; λ = 1 + 2 (1 − c), familiarité 1 − c/2 : 0,826 à 0,10 puis remontée à 0,830.
  Strict à familiarité 0,5 : 84 réussites contre 90 à 1, le strict garde 1.
- Sur les ≈ 18 500 parcours rendus des trois passes, **1** refusé par le contrôleur (passe « λ et familiarité », c = 0,90).
  Temps à 12 fils en parallèle : équipe d'abord p50 222 à 326 ms, p90 803 à 1 270 ms (à un fil, lot 2 : 163 / 442 ms).

## 6. Le contrôleur dans le chemin de réponse, ce qui est rendu

Pour chaque parcours du moteur : `Checker::new(graph, overlay, Config::default()).check(req, route)` ; refusé → retiré,
`ord_generator_routes_rejected_total{violation}` (17 codes fixes), une ligne `warn` avec les codes, le mode, le type et la
tranche de distance, **sans coordonnée**. Tous refusés → 422 `generation_failed`, `reason = checker_rejected`. Coût
**[mesuré]** : `Checker::new` < 1 ms, premier contrôle 23–24 ms (caches paresseux), suivants 7–8 ms pour 150 km ; p50
4,3 à 5,0 ms, p90 9,3 à 11,3 ms par parcours sur le banc (12 fils). Un contrôleur neuf par requête suffit.

| Champ de `routes[]` | Source | Aujourd'hui |
|---|---|---|
| `polyline` (précision **6**, celle que le backend envoie à `/v1/route-details`) | `route_geometry` | oui |
| `distance_m`, `team_m`, `bridged_m`, `team_share`, `tail_m` (= `shared_m`, longueur prise deux fois), `crossings` | **contrôleur** (`Report`) | oui |
| `unpaved_m`, `unpaved_share`, `presumed_surface_m`, `bearing_deviation_deg`, `family` | moteur, non revérifié | oui |
| `start_snap_m`, `end_snap_m` | `Diagnostics` du moteur | oui |
| `ascent_m`, `descent_m` | — | **manquent** : le moteur n'a pas d'altitude ; `null`, et `capabilities.generator.ascent = false` (question 4) |
| réseau d'équipe proche (`RouteGeneratorNetworkDto` du backend) | `TeamStats` existe, par disque non | **manque** |

## 7. Erreurs (RFC 9457, `urn:osm-route-details:problem:<code>`)

| Cas | Statut, code | `details` |
|---|---|---|
| corps, en-tête, borne (`cursor`, `distance_m`, clé, équipe) | 400 `invalid_input` | pointeurs, jamais la valeur |
| flux sans fin, compte faux, ligne illisible | 400 `invalid_input` | `/lines/<n>` |
| corps, document, nombre de traces au-delà des bornes | 413 `route_too_large` | `limit` ∈ `body_bytes`, `document_bytes`, `tracks` |
| départ ou arrivée hors du polygone | 422 `out_of_coverage` | `coverage = graph` |
| `StartTooFar`, `EndTooFar`, `DistanceTooShort`, `NetworkTooSmall`, `NoRoute(EndUnreachable \| NoWayBack \| TailTooLong \| NoCandidate \| Budget)`, tout refusé | 422 `generation_failed` | `reason` : `start_too_far`… `budget`, `checker_rejected` |
| `NoTeamData` : `cursor` < 1 sans surcouche prête | 409 `overlay_required` | `state` = `absent` \| `stale` |
| même équipe déjà en dépôt | 409 `deposit_in_progress` | — |
| quota de stockage | 507 `insufficient_storage` | — |
| file de génération pleine, dépôt de l'environnement déjà en cours, ou place de dépôt non obtenue en `queue_wait_s` | 503 `overloaded` + `Retry-After` | — |
| graphe non configuré ou illisible | 503 `data_unavailable` + `Retry-After: 3600` | `source = graph` |
| `Stopped` (budget), client parti | 504 `timeout` | jamais de parcours partiel |

## 8. Concurrence, budget, limites

Génération : CPU pur, `spawn_blocking` sous un sémaphore `generator.max_concurrent` (2), file `max_queued` (8), budget
`generator.budget_ms` (8 000) qui inclut l'attente ; le `stop` du moteur lit l'échéance et un drapeau levé quand le client
part. Un `Workspace` par place (≈ 19 Mio après 150 km **[mesuré]**), réutilisé. Distance `[distance_m_min, distance_m_max]`
= [5 000, 200 000] (le moteur n'est mesuré que jusqu'à 150 km : 1,0 s à un fil, question 6). Dépôt : un à la fois **sur le
service** et un seul par environnement (en cours ou en attente, sinon 503 tout de suite) ; la place est une file **équitable**
(`deposit.queue_wait_s`, 120 s, puis 503 `Retry-After`) : un environnement qui finit repasse derrière ceux qui attendent.
`deposit.budget_s` (900) est un budget **dur**, attente de la place comprise : il borne la lecture des trames, les envois au
fil et le fil lui-même (échéance relue entre deux lignes et avant l'écriture ; 504 `timeout`, rien d'écrit) ; 30 s sans octet → abandon ;
la découpe des lignes ne reparcourt que les octets nouveaux (coût linéaire, même en trames de 4 Kio) ; corps ≤ 2 Gio décompressés, document ≤ 16 Mio, traces ≤ 20 000.
Stockage : `max_namespaces` (16), `max_overlays_per_namespace` (2 000), `max_namespace_mb` (64) **par espace**, `max_total_mb` (1 024, le budget du volume : la configuration est refusée si `max_namespaces × max_namespace_mb` le dépasse), `allowed_namespaces` (vide : ouvert).

**Mémoire dans les 2 Go du conteneur [mesuré sauf mention]** : caches de tuiles 2 × 256 Mio (configuration partagée) ;
moteur 219 Mio anonymes + ≈ 200 Mio de pages du fichier (récupérables) ; 3 `TeamData` ≈ 165 Mio ; 2 `Workspace` ≈ 38 Mio ;
un dépôt ≈ 200 Mio **[estimé]** ; plus une requête d'altitude à froid (jusqu'à ≈ 370 Mo, commentaire du
`docker-compose.shared.yml` de Pédalons). ≈ 1,5 Gio anonymes au pire **[calculé]** : **serré** (question 5). La sonde à
12 fils et 9 équipes chargées a culminé à 3,2 Gio (`time -v`) : le nombre de fils et d'équipes en mémoire doit rester borné ; `MALLOC_ARENA_MAX=2` est déjà posé.

## 9. Configuration et mode dégradé

Section `generator` (figment, `ORD_GENERATOR__…`) : `graph`, `view_road`, `view_gravel`, `coverage_poly` (chemins ;
tous `None` par défaut), `overlay_dir`, `max_concurrent`, `max_queued`, `budget_ms`, `distance_m_min`, `distance_m_max`,
`team_cache_entries`, `overlay_ttl_days`, `stale_grace_h`, `storage.{max_namespaces, max_overlays_per_namespace,
max_namespace_mb, max_total_mb, allowed_namespaces}`, `deposit.{max_concurrent, queue_wait_s, budget_s, idle_timeout_s, max_body_bytes, max_document_bytes, max_tracks,
memo_entries, index_cache_mb}`. **Aucun secret** dans la configuration : elle reste journalisée au démarrage.

Sans chemin, ou fichier absent, illisible, vue d'un autre graphe (`EngineError::ViewMismatch`), polygone dont la boîte
s'écarte de plus de 5 km de celle des nœuds : un `warn` au démarrage, le service **démarre** et sert l'altitude ;
les routes `generator` rendent 503 `data_unavailable` (`source = graph`) ; `/health/ready` n'en dépend pas ;
`/v1/capabilities` dit `generator.enabled = false` et `reason`. Graphe chargé : `generator` publie `graph` (`id`,
date des données = `replication_timestamp`, version de l'outil), `coverage` (`bbox` et polygone GeoJSON : 765 points
pour la région), `ride_types`, la correspondance du curseur (`strict_at`, `discovery_at`, `lambda_max` 3, `lambda_min` 1,
`familiarity` 0,5, `step` 0,01), les limites du §8, `overlay_ttl_days`, `matcher_hash`, `ascent`.

## 10. Journaux et métriques sans donnée privée

Une ligne par génération (`mode`, `ride_type`, tranche de distance, `outcome`, `reason`, routes rendues, refusées,
`duration_ms`, `ns8`) et par dépôt (traces lues, exclues, illisibles, hors région, km appariés, causes, octets, durée,
`ns8`). Jamais : coordonnée, polyligne, clé, `Ord-Team`, nom de fichier, `source_digest`. Pas de valeur de champ dans
les problèmes (pointeurs seulement), comme aujourd'hui. Métriques, **sans étiquette d'espace ni d'équipe** :
`ord_generator_requests_total{outcome,reason}`, `ord_generator_duration_seconds{mode,ride_type}`,
`ord_generator_routes_total`, `ord_generator_routes_rejected_total{violation}`, `ord_overlay_deposits_total{outcome}`,
`ord_overlay_deposit_seconds`, `ord_overlay_cache_{hits,misses}_total`, `ord_overlays_stored`,
`ord_overlay_purged_total{reason}`, `ord_overlay_corrupt_total`.

## 11. Tests (synthétiques : graphe `MemSource`, vues construites en test, documents fabriqués)

1. **Cloisonnement** : dépôt sous K1 ; sous K2, `absent`, `DELETE` 204 et K1 reste `ready` ; à `cursor` < 1, 409 au corps
   **identique octet pour octet** (hors `instance`) à celui d'une équipe inconnue sous K1. Clé de 31 octets, `Ord-Team`
   `../x` → 400 ; le dossier ne contient que des noms hexadécimaux.
2. **Atomicité** : dépôt 2 coupé (sans ligne de fin, compte faux, erreur de flux, budget dépassé, disque plein simulé) →
   la génération et l'état servent encore le dépôt 1 ; aucun `.tmp` après la purge. Deux dépôts de la même équipe → 409.
3. **Persistance et graphe** : nouvel `AppState` sur le même dossier → `ready` ; graphe G2 → `stale` (`graph`), purge après
   la grâce ; `matcher_hash` changé → `stale` (`matcher`).
4. **Chiffrement** : ni `ORDOVRL1` ni la référence d'équipe dans les fichiers ; octet modifié, fichiers échangés → `absent`.
5. **Équivalence** : sur un export synthétique, la surcouche du flux du service = celle de l'outil, octet pour octet hors
   en-tête ; index par cellule = index entier sur toutes les requêtes `near` d'une grille de points.
6. **Bornes** : document > borne → 413 `document_bytes` ; corps gzip qui gonfle au-delà → 413 `body_bytes` ; mémo et LRU de
   l'index respectent leur taille (tests unitaires).
7. **Curseur** : 0 → strict, 1 → découverte, λ décroissant au centième, 1,01 → 400 ; poids du moteur monotones en λ.
8. **Contrôleur** : parcours corrompu par un crochet de test retiré, compté ; sentinelle : aucune coordonnée journalisée.
9. **Couverture et dégradé** : départ hors polygone → 422 `out_of_coverage` ; disque qui déborde → `coverage = partial` ;
   sans graphe : démarrage, `/v1/route-details` répond, `generator` 503, `capabilities.generator.enabled = false`.
10. **Contrat** : `public_routes()` et `contract/openapi.json` (test `contract` existant), version 1.9.

## 12. Exploitation : installer ou changer de graphe

1. Sur un poste : `ord-graph-build build` (PBF, `.poly`) puis `views` (`road`, `gravel`) ; fichiers 0444 (région : 125 Mo
   de base, 65 et 92 Mo de vues), copiés sous un **nouveau nom** dans un dossier monté en lecture seule, jamais par-dessus
   un fichier projeté (un fichier tronqué sous `mmap` tue le processus : lot 0 §4).
2. Changer `ORD_GENERATOR__{GRAPH,VIEW_ROAD,VIEW_GRAVEL,COVERAGE_POLY}`, redéployer ; identités vérifiées, `graph_id` publié.
3. Le backend voit `capabilities.generator.graph.id` changer (et `stale` sur chaque équipe) et redépose tout ; d'ici là,
   `cursor` < 1 rend 409 `overlay_required`, la découverte marche. Après `stale_grace_h`, purge ; l'ancien fichier à la main.

## 13. Changements demandés aux crates du lot 2

`ord-loops` : λ par requête en 256es (`Request::off_team_cost_q8`) à la place de `Params::lambda` (correctif de la sonde, trois
remplacements dans `Weighter`) ; `ord-loops-check`, `ord-graph` : rien (`team_id` des `.ordo` du service à 0).

## 14. Questions au propriétaire

1. **Chiffrement au repos** (§2) : le volume est aujourd'hui « régénérable, hors sauvegarde ». Le garder quand même
   (recommandé : il protège contre une copie du volume pour le coût d'une dépendance déjà présente) ou s'en passer ?
2. **Défaut du curseur côté produit** : 0,25 (λ 2,5 ; médiane 0,80 ; 301 réussites) ou plus près de l'équipe (0,10 ;
   0,81 ; 297) ? Et un cran visible à 0 pour le strict, vu le saut de 90 à ≈ 300 réussites ?
3. **Familiarité fixée** (0,5 en équipe d'abord, 1 en strict) : la décision 4 retire le réglage ; d'accord pour ne plus
   l'exposer du tout, y compris dans le contrat du service ?
4. **D+ des propositions** : absent en 3a (le backend demande `/v1/route-details` `fast` pour la proposition retenue,
   comme à l'enregistrement), ou calculé dans le service avant la réponse (trois profils `fast`, latence et tuiles en plus) ?
5. **Mémoire** : rester à 2 Go (3 équipes en cache, dépôt de 200 Mio, marge mince), passer à 3 Go, ou une seconde instance
   de la même image réservée au générateur (altitude coupée) ?
6. **Distance maximale** : 200 km (mesuré jusqu'à 150 km) alors que le contrat Pédalons va jusqu'à 300 km ?
7. **Durée de vie** d'une surcouche non redéposée : 14 jours (trois nuits ratées ne coupent rien) ?
8. **Réseau `pedalons-shared` chiffré** (`--opt encrypted`) pour que le secret ne circule pas en clair : à faire, au prix
   d'IPsec sur tout le trafic partagé (tuiles comprises) ?
9. **Équipes hors région** : 422 `out_of_coverage` et plus de générateur pour elles une fois la recherche Java retirée
   (plan §12 quater) : confirmé ?
10. **Polygone** : lu à côté du graphe (`coverage_poly`) en 3a ; l'inscrire dans le fichier de graphe plus tard (lot 6) ?

## 15. Risques ouverts

- `TeamData` dense (≈ 55 Mio par équipe quelle que soit sa taille) : trois en cache, un défaut coûte 0,15 à 0,2 s ; la liste
  creuse prévue au lot 2 (≈ 0,7 Mo) reste à faire si les équipes se multiplient.
- Curseur monotone en distribution seulement (§5) ; une région, une équipe qui fait 61 % des km.
- Non mesurés : temps à 2 places avec une requête d'altitude en parallèle (p90 jusqu'à 1,3 s à 12 fils), budget de 8 s,
  index par cellule, mémoire rendue par glibc après un dépôt (RSS resté à 748 Mio après l'index rendu, dans la sonde).
- Disponibilité non protégée contre un pair hostile (§2) ; secret en clair sur le réseau tant que la question 8 est ouverte.

## 16. Sonde et reproduction (hors dépôt)

Copie jetable d'`ord-loops` et d'`ord-loops-check` (seul changement : λ par requête), entrées du banc par
`tools/ord-loops-bench/src/input.rs`, index par `tools/ord-graph-overlay/src/index.rs` ; binaires `cursor` et `mem`, tailles
de l'export par un script Python ; sorties par lettre sous `~/.cache/osm-route-details/graph/lot3a/`.

## 17. Écarts du codage

Ce qui diffère de la conception, et pourquoi. Le reste (routes, cloisonnement par HMAC et ChaCha20-Poly1305, stockage
`graph-<id>/<ns>/<team>.ordo.enc`, remplacement atomique, curseur, contrôleur dans le chemin de réponse, erreurs, bornes,
journaux et métriques) est codé tel quel.

1. **Le workspace ne se chargeait pas** : `tools/ord-loops-viewer` (une page HTML et un script Python, commit `06fbdca`)
   est pris par le motif `tools/*` sans être un crate. `exclude = ["tools/ord-loops-viewer"]` dans `Cargo.toml`.
2. **L'outil ne passe pas par `Deposit::push_track`.** Sa boucle d'agrégation porte des comptes que le service n'a pas
   (continuité, contrôle de `fo`/`to`, ensembles de géométries, échantillons du rapport) ; la réécrire en dessous de
   `Deposit` aurait défait ses tests. Le **jugement** (`agg::verdict`), les **recouvrements** (`push_chain_covs`), le
   **comptage** (`count_track`), `overlay_arrays`, `params_hash`, l'appariement d'une géométrie (`job::match_job`), la
   lecture du document (`doc`) et l'appariement lui-même sont **le même code** pour l'outil et le service. La garde est le
   test de l'outil `the_service_deposit_gives_the_tool_overlay` : `Deposit` donne exactement les arêtes et les comptes
   de l'outil sur les scénarios synthétiques, avec l'index entier, l'index par cellule, un index par cellule évincé à
   chaque requête et une mémoïsation de 2 entrées. Sur la vraie région, les neuf surcouches du service ont exactement le
   nombre d'arêtes de celles de l'outil (§18).
3. **La mémoïsation est indexée aussi par `trv`** (empreinte de la polyligne, longueur, bits de `trv`) : l'outil prend le
   `trv` de la première apparition d'une géométrie, ce qui, avec une éviction, rendrait le résultat dépendant de la taille
   de la mémoïsation. Identique à l'outil tant qu'une même géométrie est annoncée avec un même `trv`. Éviction par deux
   générations (la courante, l'ancienne) : bornée à `memo_entries`, seulement du calcul en plus.
4. **L'index par cellule vit le temps d'un dépôt** (il n'est pas gardé entre deux dépôts) ; `CellIndex::fits` (aucune arête
   plus large qu'une cellule) est vérifié **une fois au chargement** (si faux : `SegIndex` entier par dépôt, un `warn`).
   Équivalence démontrée sur un quadrillage de 42 × 42 points × 3 rayons contre l'index entier.
5. **Règle du polygone corrigée.** « Boîte du polygone à moins de 5 km de celle des nœuds » **refusait la vraie région** :
   le polygone Geofabrik couvre la mer jusqu'à 8,8 km au-delà des nœuds à l'ouest (boîte du polygone
   `[-2,675 ; 46,219 ; 0,919 ; 48,570]`, des nœuds `[-2,559 ; 46,267 ; 0,956 ; 48,583]`). Règle codée : le polygone peut
   dépasser la boîte des nœuds de 15 km, les nœuds peuvent dépasser celle du polygone de 5 km, et 95 % au moins d'un nœud
   sur 16 sont dans le polygone ; sinon `reason = polygon_mismatch`, un `warn` avec les deux boîtes.
6. **Cache des équipes** : au lieu de « remplacer l'entrée du cache après le `rename` », chaque entrée garde l'empreinte
   du fichier lu (inode, date, taille) et la compare au fichier courant à chaque demande. Un remplacement atomique change
   l'inode : pas de fenêtre où un lecteur peut remettre l'ancienne entrée. `TeamData` n'est construit qu'à la première
   génération après un dépôt, pas au dépôt.
7. **Effacer pendant un dépôt** : `DELETE …/overlay` et `DELETE …/overlays` rendent 409 `deposit_in_progress` (et non 204)
   quand un dépôt de cette équipe, ou de cet espace, tient le verrou : sans cela le `rename` du dépôt ressuscitait le
   fichier effacé.
8. **Silence ou budget du dépôt dépassé : 504 `timeout`** (pas de code 408 dans le contrat). Mémoire du flux : une file
   d'une ligne, donc au plus **trois** lignes de `max_document_bytes` (en cours de découpe, en file, en traitement), pas deux.
9. **Format de ligne** : la ligne de fin est reconnue à son préfixe `{"end"` ; les lignes vides sont tolérées (comptées dans
   la numérotation `/lines/<n>`) ; une ligne de trace a `type` obligatoire, `excluded` facultatif (faux), `network_edges`
   `null` ou absent (illisible) ; clés inconnues refusées sur la ligne, tolérées dans le document.
10. **Curseur à 1 avec en-têtes** : si la clé et l'équipe sont donnés et la surcouche prête, elle sert à mesurer les parts
    d'équipe (`team_m`, `overlay` rempli) ; sinon elle est ignorée sans erreur. Sous 1, les en-têtes sont obligatoires (400
    `/headers/…`), et l'ordre des contrôles est : corps, en-têtes, puis couverture (422), puis surcouche (409).
11. **Purge d'un autre graphe** : « 48 h » se compte depuis la date du **dernier fichier** du dossier `graph-*` ancien (pas
    depuis le démarrage sur le nouveau graphe) ; un dépôt nocturne le rafraîchit donc au plus tard la veille du changement.
12. **Ajouts de configuration** : `generator.purge_interval_s` (3 600). Le graphe sans vues ni polygone éteint le
    générateur (`not_configured`) sans refuser le démarrage. `verify()` complet du graphe n'est pas lancé au démarrage
    (il relit tout le fichier) : l'ouverture contrôle l'en-tête et les sections, `Engine::new` l'identité des vues.
13. **Crochets de test** dans le code de production (`#[doc(hidden)] TestHooks` : écriture qui échoue après le `.tmp`,
    parcours altéré avant le contrôleur), jamais activés hors tests ; ils permettent les tests HTTP « disque plein » et
    « parcours refusé » sans simulateur de disque.
14. `ord-loops` : `Request::off_team_cost_q8` remplace `Params::lambda` comme prévu (§13) ; la valeur par défaut 768 donne
    exactement les poids du lot 2 (λ entier). Les poids sont arrondis à l'unité (`(λ₂₅₆ × w + 128) >> 8`).
15. **Dockerfile** : manifestes et sources des quatre crates ajoutés, manifestes des outils copiés (le workspace doit les
    lire) ; **non construit ici** (pas de construction d'image dans ce lot). Le dossier des surcouches par défaut est sous le
    volume déjà déclaré (`/var/cache/osm-route-details/generator`).
16. Version du paquet calée sur l'api (1.9.0), comme aux versions précédentes ; `PointV1` existait déjà dans le contrat (le
    point du générateur s'appelle `GeoPointV1`).

Limites assumées (non couvertes par le code) : le secret reste en clair sur le réseau (§2, question 8) ; la file de
génération pleine (503 `overloaded`) et le plafond de `max_concurrent` ne sont pas testés (non déterministes sans horloge
injectée) ; le dépôt, lui, l'est (second dépôt pendant qu'un premier tient la place).

## 18. Relevés sur la vraie région (16 novembre 2026)

Binaire `release`, graphe et vues du relevé du lot 2, surcouches **déposées par HTTP** depuis l'export privé du banc (rien
n'est dans le dépôt ; sorties sous `~/.cache/osm-route-details/graph/lot3a/smoke/`), `MALLOC_ARENA_MAX=2`, un seul
processus. Mémoire : `VmRSS` / `VmHWM` de `/proc`, en ko.

| Mesure | Valeur |
|---|---|
| Démarrage jusqu'à `/health/live` (graphe, vues, polygone, index d'accroche) | 3 s ; RSS 452 396 ko |
| Dépôt de la plus grosse équipe (2 726 traces, 703,1 Mo de NDJSON) | **45 s**, HTTP 200 ; pic 652 188 ko (+199 724 ko sur l'état au démarrage, comme l'estimation du §4) |
| Fichier stocké de cette équipe | 662 828 octets (140 766 arêtes) |
| Neuf équipes (de 1 à 2 726 traces) | 73 s au total ; nombre d'arêtes **identique** à celui de l'outil pour les neuf |
| Génération, 50 km (strict, équipe d'abord, découverte) | 0,15 à 0,53 s (strict, avec le chargement de `TeamData` : 0,53 s) |
| Génération, 100 km gravel | 0,51 s ; quatre 150 km en parallèle sur deux places : 0,53 à 1,09 s |
| RSS après trois équipes chargées et les générations | 938 384 ko, puis 993 892 ko après les quatre 150 km en parallèle |
| Parcours refusés par le contrôleur | 0 sur 33 parcours rendus |

Le polygone de 765 points est publié par `/v1/capabilities` avec sa boîte. Mesures d'un poste (16 cœurs), sans requête
d'altitude simultanée : la marge de 2 Go du §8 reste à vérifier sous charge réelle.

## 19. Corrections après revue (16 novembre 2026)

Deux revues indépendantes (justesse, sécurité). Corrigé, avec un test chacun (`tests/generator.rs`, `store.rs`) :

1. **File de génération qui fuyait** : le compteur d'attente n'était rendu que si l'attente se terminait ; un futur lâché
   (client parti, délai du proxy) gardait sa place, et après `max_queued` lâchers tout 503 immédiat. Garde `QueueSlot` (comme
   `admission.rs` de `ord-pipeline`). Test `a_dropped_queued_generation_gives_back_its_place_in_the_queue` (vérifié : il échoue sans la garde).
2. **Budget de dépôt mou** : seul le silence entre trames était borné. L'échéance est maintenant passée au fil, relue entre deux
   lignes et avant l'écriture, et les envois au fil sont sous `timeout_at`. Test : un corps qui goutte une ligne toutes les 100 ms
   (jamais de silence) finit en 504 après `budget_s`, la place est rendue.
3. **Place de dépôt unique prise sans fin par un environnement** : une place par environnement, file équitable avec attente
   bornée (`queue_wait_s`) au lieu de « essayer ou 503 ». Le test d'attente montre qu'un second environnement passe quand le
   premier part.
4. **Quotas partagés** : octets **par espace** ; la somme globale ne décide plus de rien. `allowed_namespaces` optionnel.
5. **Découpe quadratique** des lignes : un décalage `scan` mémorise l'endroit déjà parcouru (6 Mio en trames de 4 Kio, test).
6. **`fsync` du dossier après `rename`** : plus une erreur (le dépôt a eu lieu) ; test par crochet.
7. **Purge au démarrage** : seuls les `.tmp` de plus de 5 minutes.
8. **Double construction de `TeamData`** : une à la fois par équipe (test : deux demandes à froid, une construction).
9. **`panic`** : le hook global ne journalise que l'endroit.

**Non fait / non couvert** : le travail par document (nombre de points, d'arêtes) n'est borné que par `max_document_bytes` et le
budget ; le port `/metrics` doit être gardé hors du réseau partagé par l'exploitation (la jauge et l'histogramme restent
agrégés sur tous les environnements) ; K circule toujours en clair tant que la question 8 (réseau chiffré ou mTLS) n'est pas
tranchée ; un débit minimal n'est pas imposé (le budget dur suffit à borner la place).

## 20. Vérification de bout en bout (16 novembre 2026)

Relevé complet : [ord-loops-lot3a-mesures.md](ord-loops-lot3a-mesures.md) (dépôts par HTTP des neuf équipes, surcouches
identiques à l'outil, banc rejoué par HTTP, cloisonnement, redémarrage, changement de graphe, journaux). Deux changements
en sont sortis : `/v1/capabilities` publie `limits.storage.max_namespace_mb` et `limits.deposit.queue_wait_s`, et une
longueur nulle ne s'écrit plus `-0.0`.
