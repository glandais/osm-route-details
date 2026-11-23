# Journal des changements

Ce fichier commence à la version 1.10.0 (lot 4, 3b). Pour les versions antérieures, voir les notes de conception sous
`docs/design/technique/` (`ord-loops-lot2.md` à `ord-loops-lot6*.md`) et l'historique git.

## 1.15.0 (API 1.14)

Lot 5 du générateur (`docs/design/technique/ord-loops-lot5.md`) : une part de non-revêtu visée, des bornes de roulabilité.
Ajouts seulement dans le contrat ; sans `offroad_target` et avec `generator.rideability = off`, les parcours sont ceux de
1.14 au bit près. **`rideability` vaut `on` par défaut** (moteur : `Params::rideability`, service : `ORD_GENERATOR__RIDEABILITY`) :
les arêtes que le type ne peut pas rouler (§6.1) sont écartées en gravel et VTT, sauf celles que l'équipe a roulées, et le
contrôleur refuse un parcours qui en emprunte une (`NOT_RIDEABLE`). Les parcours des références figées ne changent pas (le
graphe synthétique n'a aucune arête bornée) ; sur des données réelles, des parcours gravel et VTT changent.

### Ajouté

- **`offroad_target`** (requête de `POST /v1/generator/routes`, gravel et VTT, dans [0 ; 0,8], 400 en route ou hors bornes) :
  part de la longueur à rouler hors revêtement (règle de `/v1/route-details` : tag `surface`, puis `tracktype`, puis classe).
  Le moteur joue au plus `generator.offroad_max_steps` (4) générations à des multiplicateurs μ du non-revêtu pris sur une
  grille {0,25 ; 0,35 ; 0,5 ; 0,7 ; 1 ; 1,4 ; 2}, mélange leurs candidats et rend les trois meilleurs, notés avec l'écart à la
  cible ; plus aucun pas après `min(échéance - 1 s, generator.offroad_steps_until_ms)` (10 s, comptées depuis la première
  fenêtre). En mode strict : un seul pas. **Jamais pire que sans cible** : la première génération est toujours jouée à
  μ = 1, c'est-à-dire la génération sans cible au bit près, dans le budget entier du service ; les suivantes sont coupées et
  jetées au délai des pas : une consigne ne coûte ni 504 ni 422 que la requête sans cible n'aurait pas eu.
- **Par parcours : `offroad_m` et `offroad_share`**, mesurés par le contrôleur indépendant, pour tous les types (route
  comprise). `unpaved_m`, `unpaved_share` et `presumed_surface_m` restent le tag seul.
- **Réponse : `offroad`** (`null` sans cible, sinon `{ target, tolerance, reached, steps }`). `reached` se lit sur la première
  proposition **que le contrôleur a gardée**, avec sa part à lui.
- **Capacités : `generator.offroad`** (`ride_types`, `target_min`, `target_max`, `tolerance`, `max_steps`, `rideability`).
- **Réglages** (`ORD_GENERATOR__...`) : `OFFROAD_TOLERANCE` (0,05), `OFFROAD_MAX_STEPS` (4, de 1 à 7),
  `OFFROAD_STEPS_UNTIL_MS` (10 000) et `RIDEABILITY` (`on` | `off`), passé au moteur **et** au contrôleur du service.
- **Métriques** : histogramme `ord_generator_offroad_steps` (`ride_type`), compteur `ord_generator_offroad_total`
  (`ride_type`, `target_band`, `reached`).

## 1.14.0 (API 1.13)

### Ajouté

- **Fichier d'index de service** (`<graphe>.ordi`, note `ord-loops-planete.md`) : les index globaux que le générateur
  calculait à chaque démarrage (chaînes étrangères et composantes courtes des deux vues, arêtes plus larges qu'une
  cellule, échantillon des nœuds pour le contrôle du polygone), calculés hors ligne par la sous-commande
  **`ord-graph-build index`** et projetés au démarrage. Lié à la base et aux deux vues. Réglage `generator.index`
  (variable `ORD_GENERATOR__INDEX`, par défaut le graphe avec l'extension `.ordi`). Sur la planète (582 Mo, 20 min de
  calcul) : prêt en 7,6 s au lieu de 37 min, dans 3 Go (il était tué), 920 Mo d'anonyme au plus pendant une série de
  générations, plus aucun 503 (`ord-loops-planete.md` §11).

### Changé

- **Budget d'une génération : 8 s -> 30 s** (`generator.budget_ms`, `ORD_GENERATOR__BUDGET_MS`, publié dans `/v1/capabilities`
  sous `generator.limits.budget_ms`). Mesures sur la planète, en série, pages froides, 3 Go, sans altitude
  (`ord-1-14-planete-mesures.md`) : dans 18 zones denses (Tokyo, Osaka, Séoul...), 60 à 150 km, médiane 2,0 s, p90 5,5 s,
  maximum 11,7 s (Tokyo, route 150 km) ; à 200 km, médiane 5,1 s, maximum 25,7 s. Réussites à 8 s : 79 sur 83 (60 à 150 km) et
  27 sur 42 (200 km) ; à 30 s : 83 et 42. À quatre clients simultanés sur 60 à 200 km, 16 générations sur 84 (19 %)
  finissaient en 504 à 8 s, aucune à 30 s (maximum 30,3 s). Le moteur n'abandonne jamais sur le budget (0 `budget` sur
  616 générations à budget levé) : `astar_budget`, `max_concurrent` (2) et `max_queued` (8) ne changent pas, les parcours
  et les références figées non plus ; la France reste identique (maximum 6,4 s à 200 km). Mémoire inchangée (anonyme 972 Mo
  au plus, aucun `oom`). Les clients dont le délai était calé sur 8 s doivent l'allonger : budget + altitude + marge
  (Pédalons : 35 s).

### Corrigé

- **Fenêtre d'une requête** : la zone d'accroche prend les tuiles des deux bouts des chaînes qui touchent les cases
  d'accroche, au lieu d'être élargie de la plus longue chaîne de toute la vue (566 km en route, 1 406 km en gravel sur la
  planète : 503 `overloaded` partout). Mêmes parcours.
- **Classe de fenêtre au-delà de 107,4° de longitude** : le centre de tuile débordait un `i32` (classe de millions de
  tuiles, 503 de Tokyo à Anchorage) ; calcul en `i64`.
- **Arêtes plus larges qu'une cellule** : seuil de décodage propre à la latitude de chaque cellule (le plus petit du
  graphe s'effondrait près des pôles), liste lue une fois au démarrage et partagée par les dépôts (elle était relue à
  chaque dépôt), plafond de 1 000 à 100 000 avant le repli sur l'index entier.

### Compatibilité

- **Fichier facultatif** : sans lui, le service calcule les index au démarrage, comme avant. Présent mais illisible ou
  d'un autre graphe, le générateur est éteint (`reason` : `graph_unreadable`) : le recalculer tuerait le conteneur sur la
  planète. Contrat HTTP inchangé ; aucun redépôt.
- **Budget de 30 s** : seule la valeur par défaut publiée sous `generator.limits.budget_ms` change, pas le contrat. Un
  client qui coupe avant (Pédalons : 10 s jusqu'ici) abandonne des générations que le service aurait rendues ; le délai
  du client et celui de tout proxy intermédiaire passent au budget plus l'altitude et une marge (Pédalons : 35 s).
  `ORD_GENERATOR__BUDGET_MS=8000` rend le comportement de 1.13. Les parcours rendus ne changent pas.

## 1.13.1 (API 1.13)

### Corrigé

- **Cap d'un aller simple** : il choisit maintenant les secteurs du point de demi-tour, comme pour une boucle (50° au plus du
  cap), élargis à 95° puis à tous seulement si aucune passe n'a rendu de parcours. Il n'était qu'un terme de la note : pour une
  arrivée proche du départ, la route de l'équipe l'emportait et les parcours partaient à 90–170° du cap demandé. Boucles et
  aller simple sans cap inchangés au bit près.

## 1.13.0 (API 1.13)

### Ajouté

- **Fichier d'attracteurs OSM** (lot 4, 4d) : un fichier `<graphe>.road.orda` et `<graphe>.gravel.orda` par vue, qui range
  les cafés, boulangeries, points de vue, bourgs, cols et sommets de l'extrait (nœuds et ways, relations ignorées), accrochés
  aux nœuds de la vue à 300 m, et la zone de 300 m autour de chaque accroche (un masque de catégories par nœud). Il est lié au
  graphe de base **et** à la vue (comptes, empreinte de l'extrait, dates de construction) ; un fichier d'un autre graphe est
  refusé. Données OSM publiques, hors dépôt. Environ 10 Mo (route) et 12 Mo (gravel) pour la France.
- **Sous-commande `ord-graph-build attractors`** : écrit les deux fichiers depuis l'extrait du graphe (le même, octet pour
  octet : taille et empreinte sont vérifiées), `--check` relit et recalcule. Environ 45 s et 1,2 Go pour la France.
- **Moteur** : en organique seulement, les attracteurs sont des sommets candidats du triangle, un point de rang des paires de
  sommets et le demi-tour préféré de la lentille (dans une tolérance de 5 % de D). Poids généraux (commerce, point de vue, col
  et sommet en route ; point de vue, col et sommet en gravel ; rien au VTT) ; jamais en classique, en strict ni en aller simple.
- **Contrat HTTP (ajouts seulement)** : `/v1/capabilities` porte `generator.attractors` (`available`, `enabled`, `reason` :
  `missing`, `unreadable` ou `mismatch`). Réglages `generator.attractors` (`off` ou `on`, variable
  `ORD_GENERATOR__ATTRACTORS`), `generator.attractors_road` et `generator.attractors_gravel` (par défaut déduits des vues :
  `x.road.ordv` donne `x.road.orda`). Jauge `ord_generator_attractors_available`.

### Compatibilité

- **Fichier facultatif** : sans lui, ou avec un fichier absent, illisible ou d'un autre graphe, le service démarre et sert
  comme 1.12, au bit près. Les attracteurs ne sont disponibles que si les deux vues ont leur fichier.
- **Aucun redépôt** : `graph_id`, `params_hash` et les profils sont inchangés ; l'apprentissage ne lit pas les attracteurs.
- **Déploiement roulant** : une 1.12 ignore le fichier.
- **Défaut `off`** dans cette version : poser les fichiers ne change aucun parcours tant que `generator.attractors` n'est pas
  `on`.

## 1.12.0 (API 1.12)

### Changé

- **Figures organiques par défaut** (lot 4, 4c, bascule) : `generator.figures` vaut désormais `organic`. Une requête qui
  ne nomme pas `figures` reçoit les figures organiques de 1.11 (lentille élargie par le couloir, triangles), la réponse
  porte `figures: organic` et `/v1/capabilities` rend `generator.figures.default = organic`. Toujours sans triangle : le
  mode strict, l'aller simple et le MTB. Les parcours rendus changent en conséquence, et les générations prennent plus de
  temps (au banc, P50 de × 1,25 à × 1,40 du classique ; relevé dans `docs/design/technique/ord-loops-lot4c-mesures.md`).
- **`generator.profile.learn_figures` vaut `organic`** : l'apprentissage joue les figures que le service sert (un profil
  appris en classique et servi en organique perd du goût : +0,067 de `S / S_base` au pire groupe du banc).
- Le moteur d'avant reste disponible au bit près : `figures: classic` dans la requête, ou
  `ORD_GENERATOR__FIGURES=classic` et `ORD_GENERATOR__PROFILE__LEARN_FIGURES=classic` pour tout le service (les profils
  écrits par 1.10 et 1.11 restent alors valides).

### Compatibilité

- **Contrat HTTP inchangé**, hors `info.version` : seule la valeur par défaut publiée change. Le backend de Pédalons
  n'envoie pas `figures`, ne lit pas `family` et redépose sur tout `stale` : rien à changer de son côté.
- **Première nuit** : `learn_figures = organic` entre dans l'empreinte des réglages d'apprentissage. Au premier démarrage
  de 1.12, tous les profils appris par 1.10 ou 1.11 (en classique) sont `incompatible`, et les surcouches `stale` pour la
  raison `profile` jusqu'au redépôt que le backend fait la nuit suivante. D'ici là, les générations servent la table
  commune et le mélange général des figures. Un apprentissage organique est plus long qu'un classique (la plus grosse
  équipe du banc : de l'ordre de 260 à 450 s, sous le budget de 720 s).
- **Déploiement roulant** : une 1.11 et une 1.12 lisent les profils l'une de l'autre comme `incompatible`, sans les
  effacer, et servent alors la table commune ; une 1.10 ignore de même un profil organique. Aucun fichier n'est perdu
  dans un sens ni dans l'autre, mais un retour arrière demande un redépôt.

## 1.11.0 (API 1.11)

### Ajouté

- **Figures organiques du générateur** (lot 4, 4c) : en plus de la lentille d'aujourd'hui (aller jusqu'à un point
  lointain, retour par un autre chemin), le moteur sait élargir l'écart entre l'aller et le retour par un **couloir** et
  jouer des **triangles** (deux sommets, trois jambes), tirés par requête sur la répartition réelle des sommets des
  boucles (celle de l'équipe quand elle a au moins 50 boucles du type, sinon la répartition générale). Sans effet en
  mode strict et pour un aller simple. En organique, les recherches suivent la règle des virages du contrôle (virage
  simple et grappes de 100 m, avec une marge de 5°), le contrôle interne calcule les caps comme le contrôleur
  indépendant, et aucune jambe ne repasse sur ses propres routes.
- **Contrat HTTP (ajouts seulement)** : `POST /v1/generator/routes` accepte `figures` (`classic` ou `organic`,
  facultatif ; absent, le réglage `generator.figures`, variable `ORD_GENERATOR__FIGURES`, défaut `classic`) ; la réponse
  porte `figures` (la valeur appliquée) ; `routes[].family` peut valoir `triangle` ; `/v1/capabilities` porte
  `generator.figures` (`default`, `values`).
- **Formes de l'équipe dans le profil** : l'en-tête du profil écrit au dépôt porte, par type, le nombre de sommets à 10 %
  de D de **toutes** les boucles retenues (avant le plafond de 500) et le r / D de leurs triangles. Champ facultatif du
  format 1, hors des empreintes du profil : aucun profil n'est invalidé.
- Réglage `generator.profile.learn_figures` (`classic` par défaut, `organic`) : les figures que joue l'apprentissage. À
  `organic`, il entre dans l'empreinte des réglages et rend les profils écrits `incompatibles` (`stale`, raison `profile`).
- Métrique `ord_generator_routes_total` étiquetée par `family` (`turnaround`, `triangle`).

### Compatibilité

- **Défaut inchangé** : sans réglage, `figures` vaut `classic` et le moteur rend les parcours de 1.10 au bit près (les
  références figées de 1.10 passent sans re-bénédiction). Le backend de Pédalons n'envoie pas `figures` et ne lit pas
  `family`.
- **Déploiement roulant sûr** : une 1.10 ignore `shapes` dans l'en-tête d'un profil écrit par 1.11 (le format ne change
  pas) ; une 1.11 sert le mélange général pour un profil écrit par 1.10 (sans formes) jusqu'au dépôt suivant, sans
  l'écarter. Une 1.10 refuse la clé `figures` d'une requête : seule une 1.11 la reçoit, et Pédalons ne l'envoie pas.

## 1.10.0 (API 1.10)

### Ajouté

- **Profil de goût par équipe, appris au dépôt** : à la fin d'un dépôt, le service rejoue le moteur sur les boucles
  appariées de l'équipe (au plus 500, tirées par empreinte du contenu, donc indépendamment de l'ordre des lignes), trois
  pas de recalé itéré, et écrit `<team>.ordp.enc` à côté de la surcouche (même chiffrement, autre magie `ORDPENC1`).
  La phase se fait hors de la place de dépôt, dans une place de profil unique pour tous les environnements.
  Réglages `generator.profile.*` (`enabled`, `threads` 4, `max_concurrent` 1, `budget_s` 720, `max_loops` 500, borné à 5 000, `per_type`, `mix` `capped` par défaut ou `sonde`, le mélange de la seconde sonde
  de l'apprentissage ; les deux ne s'écrivent pas avec la même empreinte, changer de forme rend les profils écrits
  incompatibles).
- **Contrat HTTP (ajouts seulement)** : `GET /v1/generator/overlay` rend `state: stale` avec `stale_reason: profile`
  quand le profil manque, a été appris avec d'autres réglages, ou n'est que partiel (1 ou 2 pas) sans profil complet depuis
  plus de 7 jours, et un objet `profile` (`state`, `learned_at`, `steps`,
  `loops`) ; le rapport de dépôt porte `profile` (`outcome`, `steps`, `loops`, `plays`, `seconds`) ; la réponse de
  génération porte `taste` (`team` ou `common`).
- Métriques `ord_profile_builds_total{outcome}`, `ord_profile_build_seconds`, `ord_profile_slot_wait_seconds`,
  `ord_profile_plays_total`, `ord_profile_play_panics_total`, `ord_profiles_stored`.

### Changé

- **Comportement du moteur** : le poids de base de toute arête, avec ou sans équipe et aux deux curseurs, est désormais
  la table de goût commune du moteur (la table recalée commune à trois pas, 72 classes par type, MTB neutre), au lieu
  des seuls `K` par classe, apprise sur 100 % des boucles retenues de toutes les équipes réunies (agrégat, aucune donnée
  d'équipe). Les parcours rendus changent en conséquence ; le moteur d'avant se retrouve avec
  `Params::base_taste = Taste::neutral()`.
- La place de dépôt est rendue à la fin de l'appariement, avant la phase de profil.

### Compatibilité

- **Déploiement roulant sûr** : la purge d'une instance 1.9.x ne touche pas un `.ordp.enc` (elle n'efface que les noms
  qui contiennent `.tmp-` et ceux qui finissent par `.ordo.enc`) ; une 1.9.x lit la surcouche et ignore le profil.
- **Backend de Pédalons inchangé** : il redépose sur tout `stale`, lit `stale_reason` comme une chaîne libre et ignore
  les champs inconnus.
- **Première nuit** : au premier démarrage de 1.10, toutes les surcouches sont `stale` pour la raison `profile` ; le
  backend redépose tout la nuit suivante (de l'ordre de 10 minutes de profils par environnement). Les générations
  servent la table commune d'ici là.
- Suppression d'une surcouche par une 1.9.x : le profil devient orphelin, retiré par la première purge de 1.10 après 1 h.
