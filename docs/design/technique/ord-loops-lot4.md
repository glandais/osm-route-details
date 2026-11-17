# Générateur de parcours — lot 4 : parcours organiques (goût appris, squelette et chair, attracteurs), première conception

Écrit le 17 novembre 2026. **Première conception, rien de codé, rien de mesuré pour ce lot.** Plan d'ensemble :
`docs/plans/2026-11-15-generateur-parcours-osm.md` du dépôt tribly (§5, §7, §10.3, lot 4 du §12) ; registre `API-207`.
Suite des notes `ord-loops-lot2.md` (moteur), `ord-loops-lot3a.md` (service) et `ord-loops-lot6.md` (fenêtres).

Marques : **[mesuré]** vient d'un relevé déjà écrit dans une note ou dans le plan ; **[calculé]** d'un calcul fait ici sur
ces relevés ; **[estimé]** d'un ordre de grandeur ; **[à mesurer]** n'a aucun chiffre et en demande un avant de coder la suite.

---

## 0. Ce que le lot livre

Des parcours qui ressemblent à ceux que les équipes tracent, et de quoi le **mesurer**. Quatre pièces :

| # | Pièce | Où | Sortie mesurable |
|---|---|---|---|
| 4a | **Banc de vraisemblance** et relevé du moteur 1.9.2 tel quel | `tools/ord-loops-bench` | Distributions générées contre réelles ; part d'arêtes retrouvées d'un parcours tenu à l'écart |
| 4b | **Profil d'équipe** calculé au dépôt : goût par classe de voie, formes, destinations | `ord-graph-match`, `ord-server` (stockage), `ord-loops` (coût) | Écart des parts de classes ; gain de reconstruction |
| 4c | **Squelette et chair** : lentille avec couloir, triangle, quadrilatère | `ord-loops` | r / D, compacité, sommets ; temps |
| 4d | **Attracteurs** : destinations de l'équipe, puis OSM, puis lieux d'équipe | `ord-loops`, `ord-graph-build`, plus tard le backend | Part des points lointains posés sur un attracteur ; reconstruction |

Ordre : 4a d'abord, **parce qu'il peut changer la suite** (§1). Puis 4b et 4c, indépendants l'un de l'autre ; 4d s'appuie
sur les deux.

**Ce que le lot ne change pas** : les garanties et le contrôleur indépendant (`ord-loops-check`, aucune règle touchée) ;
le format du graphe et des vues ; le mode strict ; le contrat de Pédalons (`RouteGeneratorRequest` et la réponse ne
portent ni figure ni famille : rien à y ajouter pour ce lot) ; le travail de nuit du backend.

**Hors lot, faute de données dans le graphe** : la **pente** et le **trafic présumé** du poids d'agrément (plan §5.4). Les
attributs d'arête n'ont ni dénivelé ni `maxspeed` / `lanes` (bits 40 à 63 réservés, `ord-graph-lot0.md`). Donc pas non
plus de « têtes de montées » comme attracteurs. Voir la question 2 du §9.

---

## 1. Ce que le moteur fait déjà, et ce qui lui manque

Lu dans `crates/ord-loops/src/generate.rs` (1.9.2) :

- **Une seule famille, le point de demi-tour** : par secteur de 30°, le nœud T qui minimise
  `|aller(T) + ρ · retour(T) − D|` ; aller = chemin de l'arbre T1, retour = A* vers l'arrivée sur le graphe privé de
  l'aller. **C'est déjà une lentille à un point lointain** : la figure de 28 % des boucles réelles selon le plan (§2).
- Le retour n'est écarté de l'aller que par l'**interdiction des mêmes arêtes** ; rien ne lui donne de largeur. La
  pénalité de couloir et la famille polygone étaient prévues au lot 2 et n'ont pas été faites (`ord-loops-lot2.md` §12.3,
  §14.3). `Family::Polygon` existe dans l'énumération, sans code.
- T est choisi sur la seule distance : **aucune notion de destination**.
- Le coût hors équipe est `λ × longueur × K(classe, surface, accès)`, K fixe (`cost.rs`) : **aucun goût d'équipe** hors
  des routes déjà roulées.
- Note de choix : écart de distance, recouvrement, croisements, écart de cap, part hors équipe. Rien sur la forme.

Trois manques donc : les figures à deux et trois points lointains (65 % des boucles réelles), la largeur de la lentille,
et le choix du point lointain. **Aucun n'est chiffré** : on ne sait pas aujourd'hui si les parcours du moteur ont un
r / D de 0,32 et une compacité de 0,30 ou s'ils en sont loin. D'où le 4a en premier.

**Incohérence du plan à lever au 4a.** Son §2 compte les sommets « hors départ » : 2 = lentille (28 %), 3 = triangle
(40 %), 4 = quadrilatère (25 %). Son §5.2 dit lentille = **un** point lointain, triangle = deux. Les deux ne se
recollent que si la simplification garde un point de plus (le départ en double, ou un coude de sortie). Le banc recalcule
ces comptes avec une définition écrite (§2.1) ; les proportions du tirage (§4.1) en dépendent.

---

## 2. 4a — Banc de vraisemblance

Nouvelle sous-commande de `tools/ord-loops-bench` (qui sait déjà lire des requêtes, lancer le moteur dans les trois modes
et passer le contrôleur) ; elle lit l'export du banc par le lecteur CSV de `tools/ord-graph-overlay`. Graphe : la France
(`fr-2026-11-15`), pour avoir les 9 équipes et non 7.

### 2.1 Mesures de forme, les mêmes sur le réel et sur le généré

Reprise de `shapes.py` (hors dépôt, dossier du banc) en Rust, pour une polyligne fermée de longueur D :

- r / D, r = distance à vol d'oiseau du départ au point le plus éloigné ; position de ce point le long du parcours ;
- compacité 4πA / D² ;
- **sommets** : Douglas-Peucker à 5 % de D sur la polyligne **ouverte au départ**, le départ et son double non comptés ;
  définition figée dans le code et dans cette note, puis proportions relues (§1) ;
- parts de la longueur par classe de voie et par groupe de revêtement (les classes du goût, §3.2) ;
- virages de plus de 45° aux nœuds de degré ≥ 3, par 10 km ;
- part de la longueur recouverte et croisements (déjà dans `Route`).

Le réel est mesuré sur la **suite d'arêtes appariée** de chaque trace (ce que le dépôt calcule), pas sur la trace brute :
même graphe, mêmes longueurs, comparaison juste.

### 2.2 Distance entre distributions

Pour chaque boucle réelle retenue, une requête : même départ, même D (arrondie au km), même type, cap = cap du barycentre
de la trace arrondi à 45°, graine fixe. Premier parcours rendu, et meilleur des trois. Par mesure : quantiles P10 à P90 du
réel et du généré, et distance de Wasserstein d'ordre 1 normalisée par l'écart interquartile du réel (un seul nombre par
mesure, comparable d'une passe à l'autre). Par mode (`cursor` 0,25 et 1) et par type.

### 2.3 Reconstruction

1. **Mise à l'écart** : 10 % des boucles de chaque équipe, tirées par l'empreinte de l'identifiant de trace (stable).
2. Surcouche et profil (§3) reconstruits **sans elles**.
3. Pour chacune : requête du §2.2 ; **part de ses mètres retrouvés** (arêtes de base communes, une fois comptées, sur la
   longueur de la trace), pour le premier parcours et pour le meilleur des trois.
4. Deux lectures séparées, parce qu'en « équipe d'abord » une trace tenue à l'écart reste en grande partie sur des routes
   que d'autres traces de l'équipe ont roulées : part retrouvée **sur les arêtes d'équipe** et **sur les arêtes qu'aucune
   autre trace ne prend**. La seconde seule dit si le goût et les attracteurs servent.

Une équipe fait 85 % des kilomètres **[mesuré, lot 1]** : chaque chiffre est donné par équipe, jamais seulement au total.

### 2.4 Ce que le relevé initial décide

- r / D et compacité du moteur 1.9.2 déjà dans l'intervalle P25–P75 du réel → la pénalité de couloir (§4.3) descend en
  priorité, les figures à deux et trois points passent devant.
- Compacité très en dessous de 0,30 (retour collé à l'aller) → le couloir d'abord : c'est le changement le plus court.
- Reconstruction hors routes d'équipe déjà haute en découverte → le goût appris apporte peu, à dire avant de le coder.

Temps : le banc est hors service, un fil par requête, comme la campagne du lot 2.

---

## 3. 4b — Profil d'équipe, calculé au dépôt

### 3.1 Pourquoi au dépôt, dans le service

Le dépôt reçoit **une ligne par trace**, avec ses arêtes dans l'ordre et leur géométrie, et les apparie au graphe
(`Deposit::push_track`, `ord-loops-lot3a.md` §4). Tout ce qu'il faut pour le goût, les formes et les destinations passe
donc déjà par le service, qui est seul à avoir le graphe. **Le backend n'a rien à calculer ni à envoyer de plus**, et le
travail de nuit ne change pas : un redépôt refait le profil.

Aujourd'hui le dépôt ne garde que des comptes par arête ; la suite d'une trace est jetée après agrégation. Le profil
s'accumule dans la même passe, trace par trace, mémoire bornée (§3.5).

### 3.2 Goût par classe (plan §7.2)

**Classe de goût** d'une arête de base, tirée de ses seuls attributs, 6 × 3 × 2 × 2 = 72 cases au plus :

| Axe | Valeurs |
|---|---|
| Voie | grande route (`primary`, `secondary`, `trunk`) · `tertiary` · petite route (`unclassified`, `road`) · rue (`residential`, `living_street`, `service`) · voie cyclable ou verte (`cycleway`, voie verte au sens de `cost::is_greenway`) · chemin (`track`, `path`, `bridleway`, reste) |
| Revêtement | revêtu connu · présumé (pas de `surface`) · non revêtu connu |
| Balisage | sur un itinéraire `lcn` / `rcn` / `ncn` / `mtb` (`Attrs::routes`) · non |
| Accès | `destination` · autre |

Par type de parcours t et par classe c :

- **roulé(c)** = Σ longueur × passages du type t (les deux sens), sur les arêtes de la surcouche ;
- **disponible(c)** = Σ longueur des arêtes **de la vue du type** à moins de 1 km d'une arête roulée par l'équipe
  (trame de 500 m marquée par les arêtes roulées, dilatée de deux cases ; les cases chaudes de la construction creuse du
  lot 6 bornent le balayage) ;
- **g(c)** = (part roulée + m × part disponible) / (part disponible + m × part disponible), m = 0,5 : une classe sans
  passage tend vers 1/3 et non vers 0, une classe rare ne s'emballe pas ; borné à [1/3, 3] ; g = 1 si le type a moins de
  200 km-passages (le VTT, 13 parcours **[mesuré, plan §13]**, reste donc aux réglages généraux).

Le couloir de 1 km et m sont des réglages de départ **[à mesurer]** au banc de reconstruction.

**Application au coût.** Multiplicateur `g(c)^(−γ)`, γ = 0,5 au départ, en 256es dans une table d'entiers (même règle
que `multiplier_table` : pas de flottant dans les poids). Il s'applique **aux arêtes que l'équipe n'a pas roulées**,
c'est son objet (plan §7.2 : « garder le goût de l'équipe sur des routes qu'elle n'a jamais prises ») ; les arêtes
d'équipe gardent leur poids d'aujourd'hui. Dans `WindowTeam`, par chaîne et par type, le « reste hors équipe »
`w − a − b` calculé à la relaxation devient un champ précalculé `Σ w(b) × mult(classe(b))` sur les arêtes de base non
roulées : 12 → 16 octets par chaîne et par type **[calculé]**, à compter dans le budget des fenêtres du lot 6. Le morceau
partiel du départ et de l'arrivée (`Weighter::edge`) suit la même table.

En **découverte** (`cursor` = 1) avec une équipe fournie : voir la question 1 du §9. En **strict** : sans effet.

### 3.3 Formes (plan §7.3)

Pour chaque boucle (arrivée à moins de 5 % de D du départ) de plus de 5 km, les mesures du §2.1. Le profil garde par
type : le nombre de boucles, les quantiles de r / D et de compacité, les parts par nombre de sommets. Le tirage du
squelette (§4.1) prend la distribution de l'équipe à partir de **50 boucles du type** ; en dessous, la distribution
générale du plan (§2), écrite en constantes dans le moteur.

### 3.4 Destinations (plan §7.5)

Par boucle : le point le plus éloigné du départ et les sommets du §2.1, ramenés au nœud de base le plus proche de la
trace appariée. Regroupés sur une trame de 500 m ; une destination = une case, son nœud le plus fréquenté, un compte de
traces distinctes par type. Gardées : compte ≥ 2, les 2 000 plus fréquentées au plus. Ce sont des **données privées de
l'équipe**, au même titre que la surcouche : même fichier chiffré ou fichier voisin, même cloisonnement, même purge
(§3.5).

Le facteur de détour par zone (plan §7.6) **n'est pas stocké** : il se lit dans l'arbre aller de chaque requête
(mètres par la route / vol d'oiseau jusqu'au nœud), §4.2.

### 3.5 Stockage, identité, mémoire

- **Fichier voisin** `<team>.ordp.enc` à côté de `<team>.ordo.enc`, même chiffrement, même remplacement atomique,
  écrit **avant** le `rename` de la surcouche. Pas dans l'`.ordo.enc` : une instance 1.9.x encore en vie pendant un
  déploiement roulant traite un fichier qu'elle ne sait pas lire comme corrompu et **l'efface** (`ord-loops-lot3a.md`
  §3). À vérifier au codage : que la purge de 1.9.x ne touche pas un nom de fichier inconnu.
- **Profil absent = moteur d'avant.** Une surcouche sans profil se sert comme en 1.9.2. `GET /v1/generator/overlay`
  rend alors `stale`, raison nouvelle `profile` : le backend redépose la nuit suivante, sans changement de son code
  (il redépose dès que `state ≠ ready`). Variante : changer `params_hash`, qui rend tout `stale` raison `matcher` ;
  moins lisible, même effet.
- **Mémoire du dépôt** : le goût ajoute 72 × 3 compteurs et la trame de 500 m des arêtes roulées (0,5 Mo pour un
  réseau de 15 000 km **[estimé]**) ; le « disponible » se calcule en fin de dépôt par un balayage des cases chaudes.
  Formes et destinations demandent la suite appariée de la trace en cours, relâchée à la fin de la trace.
- **Mémoire en service** : le profil tient en quelques dizaines de Ko (72 × 3 multiplicateurs, quantiles, 2 000
  destinations de 12 octets), à côté de `TeamData`.

---

## 4. 4c — Squelette et chair

### 4.1 Tirage de la figure

Par requête, la graine tire pour chaque candidat un nombre de points lointains k ∈ {1, 2, 3} selon les parts de l'équipe
ou générales (§3.3), puis un r / D visé entre P10 et P90. Les trois parcours rendus cherchent des **figures
différentes** quand c'est possible : à note voisine, le choix glouton préfère une figure pas encore retenue.

Un aller simple (arrivée ≠ départ) garde le moteur d'aujourd'hui dans ce lot.

### 4.2 Points lointains

Candidats, lus dans les deux arbres déjà calculés (T1 depuis le départ, T2 vers l'arrivée) :

1. les **attracteurs** de la zone (§5) atteints par T1 et T2 ;
2. en **repli** (plan §5.2, point 3), par secteur de 30°, le nœud de la bande de distance dont le chemin aller est le
   plus agréable : `coût T1 / mètres T1` minimal. Les deux valeurs sont dans l'arbre, aucun calcul de plus.

**k = 1 (lentille)** : comme aujourd'hui, `|aller(T) + ρ · retour(T) − D|` minimal, mais parmi les candidats à moins de
5 % de D de la cible le meilleur attracteur passe devant le nœud quelconque.

**k = 2 (triangle)**, sommets P1 et P2 dans le cap demandé, écartés de 40° à 90° vus du départ :
longueur estimée = `mètres T1(P1) + φ × vol d'oiseau(P1, P2) + mètres T2(P2)`, avec φ = moyenne des rapports
route / vol d'oiseau de P1 et P2 dans T1 (le facteur de détour local, lu et non deviné). On garde les paires dont
l'estimation est à ±5 % de D, classées par poids d'attracteur puis par agrément.

**k = 3 (quadrilatère)** : P1, P2, P3 de même, P2 le plus loin, estimation avec deux jambes intermédiaires.

### 4.3 Les jambes

| Jambe | Recherche | Coût |
|---|---|---|
| départ → P1 | chemin de T1 | aucun |
| dernier point → arrivée | chemin de T2 ; s'il reprend la première jambe au-delà de la queue permise, A* d'heuristique T2 sur le graphe privé des jambes précédentes (l'`astar` existant) | aucun, ou un A* court |
| P_i → P_{i+1} | **A* point à point, à écrire** : graphe privé des autres jambes, borné au disque de la requête, heuristique = max(vol d'oiseau × plus petit facteur de coût, `T1(P_{i+1}) − T1(x)`, `T2(x) − T2(P_{i+1})`) | une recherche par jambe intermédiaire |

Les deux dernières bornes viennent de l'inégalité triangulaire sur les arbres, qui servent de repères gratuits ; elles
sont admissibles parce que les interdits ne font qu'augmenter les coûts (même argument que l'heuristique T2 du lot 2).
La recherche reste dans le disque de rayon D/2 : **la fenêtre de tuiles du lot 6 ne change pas**.

**Couloir** (plan §5.3). Une trame de 500 m sur le disque (200 × 200 cases à 100 km, un tableau de bits) est marquée par
les nœuds des jambes déjà posées, dilatée jusqu'à la largeur *w*, sauf leurs parties à moins de 0,15 × D du départ ou de
l'arrivée le long du parcours (la queue : la pénalité s'y efface). Une arête de vue dont le nœud d'arrivée tombe dans une
case marquée coûte × κ. C'est un bit de plus dans l'estampille `Ban`, lu à la relaxation.

Valeur de départ de *w* **[calculé]**, sur un modèle grossier : deux arcs de parabole de corde r = 0,32 D qui enferment
l'aire d'une compacité de 0,30, soit 0,024 D², s'écartent au plus de 0,11 D (11 km pour 100 km). *w* = 0,03 D et κ = 2
pour commencer : une séparation minimale, pas la largeur visée. Réglage sur la compacité médiane du banc.

La pénalité de couloir vaut pour toutes les figures ; elle compte surtout pour la lentille, dont rien d'autre ne fixe la
largeur.

### 4.4 Distance, point mal placé, contrôle

- **Distance** : l'estimation du §4.2 est corrigée par la sécante, comme le ρ d'aujourd'hui : le rapport mesuré
  longueur réelle / estimation d'un candidat hors tolérance recale φ pour le candidat suivant du même secteur.
- **Point mal placé** (plan §5.3). Signature d'un épi : les 500 m d'arrivée sur P et les 500 m de départ font un angle
  de 150° ou plus, ou partagent le couloir. Réparation : reculer P le long de la jambe d'arrivée jusqu'au dernier nœud de
  degré ≥ 3 d'où la jambe suivante peut partir sans cela, une fois ; sinon le candidat est jeté. La forme générale
  « retirer le sommet et comparer » du plan est laissée de côté tant que le banc ne compte pas d'épis rendus.
- **Contrôle** : chaque candidat passe `check_route_in` puis, dans le service, le contrôleur indépendant, **sans règle
  nouvelle ni assouplie** : croisements ≤ 2, queue ≤ 0,15 × D, couloir doublé ≤ 20 %, virage < 150°, ±10 %. Un
  quadrilatère qui se croise trois fois est refusé comme n'importe quel parcours.

### 4.5 Budget et temps

Aujourd'hui : 2 arbres et un budget global d'A* de 6 × les états fixés de T1 ; 163 ms au médian et 442 ms au P90 en
équipe d'abord jusqu'à 100 km, **sans marge au P90 [mesuré, lot 2]**. Les jambes intermédiaires entrent dans **le même
budget d'états**, pas dans un budget de plus : un triangle coûte une recherche point à point, un quadrilatère deux,
chacune sur un tiers ou un quart de D. Coût d'une telle recherche **[à mesurer]** par une sonde avant de coder les
familles (ordre de grandeur attendu : quelques millisecondes à 100 km, l'heuristique étant bien plus faible que celle
des retours actuels à 0,05 ms).

Cible inchangée : médiane sous 500 ms jusqu'à 100 km. Si elle ne tient pas, le nombre de figures essayées baisse avant
tout autre réglage.

### 4.6 Interface du moteur et non-régression

- `Request` gagne `figures` : `Classic` (le moteur de 1.9.2) ou `Organic`.
- `Family` gagne `Lens`, `Triangle`, `Quad` ; `Polygon`, jamais rendu, est retiré de l'énumération si rien ne le lit.
- `TeamData` porte un profil facultatif.
- **Invariant gardé par la référence figée** (`tests_reference.rs`) : `figures = Classic` et pas de profil rendent
  exactement les parcours de 1.9.2. Le banc de parité du lot 2 et la campagne du lot 6 se rejouent tels quels.
- Poids entiers partout (table du goût, κ en 256es) : même parcours en natif et en WASM, comme aujourd'hui.

---

## 5. 4d — Attracteurs

Trois sources, dans l'ordre où elles coûtent :

1. **Destinations de l'équipe** (§3.4) : livrées par le profil, rien d'autre à construire. Poids = compte de traces.
2. **OSM** (plan §3, point 6) : cols (`mountain_pass`), sommets desservis, points de vue, plans d'eau, bourgs
   (`place=town|village`), cafés et boulangeries, points d'eau. Nouvelle sous-commande d'`ord-graph-build` qui relit **le
   même PBF** (empreinte comparée à celle de l'en-tête du graphe), accroche chaque objet au nœud de vue le plus proche à
   moins de 300 m, et écrit un **fichier voisin** `.orda`, par tuile, lu par `mmap`. Le graphe et les vues ne sont pas
   réécrits : `graph_id` ne bouge pas, aucune surcouche ne devient `stale`. Fichier absent = pas d'attracteurs OSM, le
   service démarre quand même. Mise en service : un fichier de plus à poser à côté du graphe (`OPS-61`).
3. **Lieux de l'équipe et arrivées des sorties** (plan §5.2) : le backend les ajoute au dépôt, une ligne
   `{"place":{"lat":…,"lon":…}}`. Le service 1.9 refuse une clé inconnue : le service doit être en place avant le
   backend. **Proposé hors de ce lot** (question 4).

Un attracteur OSM est public ; il pèse moins qu'une destination d'équipe (poids de départ : 1 contre le compte de
traces, plafonné à 5). Les poids par nature d'objet sont réglés sur la reconstruction, pas à la main.

---

## 6. Service (api 1.10, ajouts seulement)

- `POST /v1/generator/routes` : champ facultatif `figures` (`classic` | `organic`), défaut `classic` tant que le banc
  n'a pas tranché, puis `organic`. `routes[].family` rend les nouvelles valeurs.
- `GET /v1/generator/overlay` : `stale_reason` peut valoir `profile`.
- `/v1/capabilities` : `generator.figures`, `generator.attractors` (fichier présent ou non).
- Journaux et métriques : la famille en étiquette (valeurs fixes), jamais une destination ni une coordonnée.
- Rien pour le backend de Pédalons dans ce lot hors la version de l'image ; le contrôle géométrique du lot 3b
  (distance, continuité, départ et arrivée) vaut pour toute figure.

---

## 7. Tests

Synthétiques, comme les lots 2 et 3a (graphe `MemSource`, surcouches fabriquées) :

- goût : une équipe qui ne roule que des petites routes dans un graphe mêlé a g > 1 pour elles, g < 1 pour les grandes ;
  table d'entiers figée par une empreinte ; un type sous 200 km-passages rend g = 1 ;
- profil absent ou `figures = classic` : référence figée inchangée ;
- triangle et quadrilatère sur une grille : chaque sommet du squelette est sur le parcours, dans l'ordre ;
- couloir : sur deux routes parallèles à 200 m et une troisième à 4 km, le retour prend la troisième ;
- propriété moteur / contrôleur (`engine_vs_checker.rs`) étendue aux figures : tout parcours rendu passe ;
- déterminisme : même graine, même parcours ; fenêtre plus grande, même parcours (lot 6, §5.4) ;
- stockage : profil chiffré, cloisonné par espace, effacé avec la surcouche et par la purge ; dépôt qui échoue, ancien
  profil gardé.

---

## 8. Risques

- **Le banc dit que le moteur est déjà proche du réel** sur les formes : le lot se réduit alors au goût et aux
  attracteurs. C'est un bon résultat, à accepter.
- **Le goût apprend les biais** : une équipe qui roule en ville « aime » les rues. Borne [1/3, 3], γ = 0,5, et la
  reconstruction par équipe pour le voir.
- **Reconstruction trompeuse** en équipe d'abord (§2.3, point 4) : ne lire que la part hors routes d'équipe.
- **Temps** : P90 sans marge aujourd'hui ; les jambes intermédiaires sont le poste neuf.
- **Destinations = où roule l'équipe** : donnée privée de plus dans le service partagé. Même régime que la surcouche ;
  à relire côté sécurité avec `SEC-52`.
- **Une équipe, 85 % des kilomètres** : les réglages généraux sont en fait les siens.

---

## 9. Questions au propriétaire

1. **Découverte avec une équipe** (`cursor` = 1) : le goût appris s'applique-t-il ? Proposé : oui dès qu'une surcouche
   existe (« des routes nouvelles, au goût de l'équipe ») ; sans équipe, OSM seul comme aujourd'hui.
2. **Pente et trafic** : hors lot, ou reconstruire le graphe avec `maxspeed` / `lanes` (bits réservés) en même temps que
   le fichier d'attracteurs ? La reconstruction change `graph_id` et fait redéposer toutes les équipes (automatique, une
   nuit). Proposé : hors lot, à décider après le relevé du 4a.
3. **Mise à l'écart de 10 %** pour la reconstruction, par équipe : suffisant, ou un protocole différent ?
4. **Lieux d'équipe et arrivées des sorties** comme attracteurs : dans ce lot (ligne de plus au dépôt, donc backend) ou
   après ? Proposé : après.
5. **Figure choisie par la personne** (le plan §8 évoquait un champ « figure ») : proposé non, le tirage est
   automatique et les trois propositions varient les figures.
6. **Seuil de 50 boucles** pour que la distribution de formes de l'équipe remplace la générale.

---

## 10. Étapes, dans l'ordre

1. 4a : mesures de forme en Rust, sous-commande du banc, relevé du moteur 1.9.2 → `ord-loops-lot4-mesures.md`.
   **Arrêt et relecture** : les §3 à §5 sont confirmés ou réordonnés.
2. Sonde du coût d'une recherche point à point (§4.5).
3. 4b : profil au dépôt (goût, formes, destinations), stockage, table de coût ; reconstruction rejouée.
4. 4c : couloir sur la lentille, puis triangle, puis quadrilatère ; distributions rejouées après chacun.
5. 4d : destinations d'équipe comme attracteurs, puis fichier OSM.
6. Service api 1.10, relevés en conteneur, bascule du défaut sur `organic`, version et mise en service.
7. Plan de tribly : section « Lot 4 » du §12 avec ce qui est mesuré ; `API-207` mis à jour.
