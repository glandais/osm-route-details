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
*[Correction du 17 novembre, à la lecture du code : `ord-graph-overlay` est un binaire sans cible bibliothèque, son
lecteur CSV ne peut pas être importé tel quel ; il passe dans `ord-graph-match`, §11.2.]*

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

> **Abandonné le 2026-11-17 (§9.2)** : l'estimateur « roulé contre disponible » ci-dessous aggrave les écarts de classes
> en GRAVEL (`ord-loops-lot4-sonde.md`). Les classes de goût restent ; l'estimateur sera recalé. Texte gardé pour mémoire.

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

### 9.1 Décisions (2026-11-17, après le relevé du 4a)

1. **Découverte** : oui, le goût appris s'applique dès qu'une surcouche existe ; sans équipe, OSM seul.
2. **Pente et trafic** : hors lot ; à revoir après la sonde du goût.
3. **Juger le goût** : la mise à l'écart de 10 % reste pour les formes ; le goût se juge sur la répartition des classes de
   voie (grandes routes, cyclables, chemins) des parcours générés contre les vrais, pas sur la reconstruction.
4. **Lieux d'équipe comme attracteurs** : après le lot 4 ; le 4d s'en tient aux destinations OSM.
5. **Figure choisie** : non, tirage automatique sur la répartition réelle (sommets à 10 % de D).
6. **Seuil** : 50 boucles.
7. **Ordre** : celui du relevé (`ord-loops-lot4-mesures.md`) : sonde du goût dans le banc (toutes arêtes, équipe
   comprise) → 4c réduit au triangle et au couloir (quadrilatère hors lot) → 4b complet seulement si la sonde divise par
   deux les écarts de classes → 4d.

### 9.2 Décisions (2026-11-17, après la sonde du goût, `ord-loops-lot4-sonde.md`)

1. **Le goût « roulé contre disponible » du §3.2 est abandonné** : il double les biais de `K` (GRAVEL 1,20 à 1,48 fois
   l'écart de `base`). Les classes de goût (72 cases) restent ; s'il y a un goût, il est **recalé** (réel contre généré).
2. **Seconde sonde, au banc seulement, avant tout le reste** : recalé itéré en 2 à 3 pas et recalé par équipe. Elle
   tranche entre une table générale par type dans les poids OSM et un profil par équipe appris au dépôt.
3. **Familiarité** : reste à 0,5 au service pour l'instant ; réglée avec le goût retenu (meilleure combinaison mesurée :
   familiarité 1 + recalé γ 0,5, ROAD au curseur 0,25).

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

---

## 11. 4a : spécification du banc

Écrit le 17 novembre 2026 après lecture de `tools/ord-loops-bench`, `crates/ord-graph-match`, `tools/ord-graph-overlay`,
`ord-loops` 1.9.2 (`lib.rs`, `generate.rs`, `cache.rs`, `window.rs`), `ord-loops-check` (`check.rs`, `rules.rs`) et du
chemin de dépôt et de génération du service (`ord-server/src/generator/deposit.rs`, `generate.rs`, `cursor.rs`). Cette
section **suffit au codeur** : elle tranche tout ce que les §2 et §10 (étape 1) laissent ouvert. Ce qu'elle ne dit pas
est laissé à son jugement, à condition de ne toucher ni au moteur, ni au contrôleur, ni au service (§11.2).

### 11.0 Ce que la lecture a établi

1. **Le lecteur CSV n'est pas importable** : `ord-graph-overlay` n'a que `main.rs`. `CsvReader` passe dans
   `ord-graph-match` (§11.2), l'outil le réexporte. Correction signalée au §2.
2. **Le dépôt ne rend pas la suite d'une trace**, mais tout ce qu'il faut pour la refaire est public : `match_job`
   (pure), `agg::verdict`, `Chain::pieces` (dans l'ordre de `g`), `agg::push_chain_covs`, `agg::count_track`. Sa
   mémoïsation (clé `(xxh3(g), g.len(), trv.to_bits())`) est privée et ne change aucun résultat : apparier une fois
   chaque clé distincte en parallèle puis compter trace par trace donne **les mêmes** comptes que `push_track`. Il manque
   seulement une entrée « appariement déjà fait » au `Deposit` (§11.2).
3. **`shapes.py` et le compte de sommets** : `corners = len(keep) − 2` retire le départ **et** son double ; il n'y a
   donc pas de « point de plus gardé ». Deux faits expliquent le tableau du plan §2 :
   - sur une boucle ouverte d'un écart non nul, la première coupe prend le point **le plus loin de la droite
     départ–arrivée** (distance à une droite presque dégénérée, de direction quelconque), pas le point le plus loin du
     départ ; si l'écart est nul, c'est la distance au départ ;
   - une lentille compte **1 + un sommet par côté dont la flèche dépasse 5 % de D**. Avec une compacité de 0,30, la
     flèche est de l'ordre de 0,05 D (§4.3 : 0,11 D d'écart entre les deux arcs) : la lentille se répartit entre 1, 2 et
     3 sommets. La ligne « Figure » du plan est une étiquette, pas une mesure.
   Le banc rapporte donc **quatre** comptes (§11.5.3) et propose celui du §4.1.
4. **Le service ne rend que les parcours que le contrôleur indépendant accepte** (`Config::default()`, dans l'ordre du
   moteur) : « premier parcours rendu » = premier parcours de `Generation::routes` qui passe `Checker::check`.
5. **Une `Route` se fabrique à la main** (champs publics) : la suite appariée d'une trace réelle **continue par nœuds**
   peut passer le même contrôleur, ce qui donne les croisements et le recouvrement du réel par **la même définition**
   que pour le généré, et dit au passage quelles règles les vraies boucles enfreindraient.
6. **Les lettres des 9 équipes existent déjà** dans `lot2/team-letters.json` (A à I) ; rien à étendre.
7. Export : 3 377 lignes, ROAD 2 451, GRAVEL 913, MTB 13, aucune exclue, toutes avec `track_id` **[mesuré, 17 novembre]**.

### 11.1 Où vit le code, ligne de commande, sorties

**Sous-commande `likeness` de `ord-loops-bench`**, sans casser l'appel actuel :

```rust
// main.rs : l'actuelle `struct Args` devient `#[derive(clap::Args)] struct BenchArgs` (mêmes champs), le corps de
// `main` devient `fn run_bench(a: BenchArgs) -> Result<()>`.
#[derive(Debug, Parser)]
#[command(args_conflicts_with_subcommands = true, about = "…")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    bench: BenchArgs,
}
#[derive(Debug, Subcommand)]
enum Command {
    /// Banc de vraisemblance du lot 4 (4a) : formes réelles contre générées, reconstruction.
    Likeness(likeness::LikenessArgs),
}
```

Modules, sous `tools/ord-loops-bench/src/likeness/` : `mod.rs` (arguments, orchestration, §11.9), `export.rs` (lecture,
appariement, retenue, §11.3), `walk.rs` (marche commune, §11.4), `shape.rs` (mesures pures sur polylignes, quantiles,
Wasserstein : **aucune dépendance au graphe**, §11.5 et §11.7), `measure.rs` (mesures qui lisent le graphe : classes,
degrés, longueurs), `holdout.rs` (mise à l'écart et surcouches, §11.8), `play.rs` (requêtes, cache partagé, fils,
§11.6), `out.rs` (JSON et résumé, §11.10). Dépendances ajoutées au `Cargo.toml` du banc : `ord-graph-match`, `rayon`,
`flate2` (toutes trois déjà du workspace : rien de neuf pour cargo-deny).

Arguments (`LikenessArgs`), défauts sous `~/.cache/osm-route-details/graph/` (noté `G`) :

| Argument | Défaut | Rôle |
|---|---|---|
| `--graph`, `--road`, `--gravel` | `G/lot6/france-20261115-z11.{ordg,road.ordv,gravel.ordv}` | graphe et vues |
| `--poly` | `G/lot6/france.poly` | région de `match_job` |
| `--export` | `~/code/perso/tribly-generator-bench/data/prod-network.csv.gz` | export privé |
| `--letters` | `G/lot2/team-letters.json` | `{ id d'équipe : lettre }` |
| `--overlays` | `G/lot6/overlays` | seulement pour le contrôle d'égalité des surcouches (§11.8) ; absent : contrôle sauté |
| `--out`, `--label` | `G/lot4/`, `likeness` | sorties `<label>-report.json`, `<label>-summary.md` |
| `--cursors` | `0.25,1` | valeurs jouées, par `ord_server::generator::cursor::{hundredths, apply}` |
| `--teams` | toutes | lettres, séparées par des virgules |
| `--limit`, `--max-km` | aucun | au plus N boucles retenues par équipe (les premières dans l'ordre de l'export) ; D ≤ k km |
| `--min-km` | `5` | D minimale d'une boucle retenue |
| `--loop-gap` | `0.05` | arrivée à moins de cette part de D du départ |
| `--min-matched` | `0.95` | part appariée minimale d'une trace (§11.3) |
| `--max-hole-m` | `1000` | plus long trou admis dans la suite appariée |
| `--holdout-share`, `--holdout-min-loops` | `0.10`, `10` | mise à l'écart (§11.8) |
| `--no-holdout` | non | saute la reconstruction |
| `--seed` | `20261117` | graine de base (§11.6) |
| `--threads` | `available_parallelism()` | fils de génération |
| `--budget-mb`, `--radius-step-m` | `6000`, `10000` | cache de fenêtres partagé, pas de classe des fenêtres |
| `--calibrate-only` | non | ne fait que l'étalonnage `shapes.py` (§11.5.3), sans graphe, et s'arrête |

**Confidentialité** (règle du banc existant, à tenir partout) : départs, coordonnées, ids de trace, d'équipe et
d'arête de base ne sortent **ni** dans le JSON, **ni** dans le résumé, **ni** sur la sortie d'erreur. Les équipes sont
lettrées ; une boucle est désignée par `(lettre, i)`, `i` = rang parmi les boucles retenues de l'équipe dans l'ordre de
l'export. Seules des empreintes (xxh3) et des agrégats sortent. Les sorties sont écrites sous `G/lot4/` et nulle part
ailleurs.

### 11.2 Changements hors du banc (liste fermée)

1. **`ord-graph-match/src/csv.rs`** (nouveau, `pub mod csv`) : `CsvReader` et son test déplacés **tels quels** de
   `tools/ord-graph-overlay/src/export.rs`, qui garde `pub use ord_graph_match::csv::CsvReader;`. Aucun changement de
   comportement ; `ord-graph-match` n'a pas besoin de `flate2` (le lecteur prend un `BufRead`).
2. **`Deposit::push_matched`** dans `ord-graph-match/src/deposit.rs`. Refactorisation sans changement de comportement :
   le corps de la boucle de `push_track` après la mémoïsation passe dans une fonction privée
   `fn add_edge(&mut self, wk: u8, trv: f32, job: &Job)` (repli `trv` NaN → `job.glen`, statistiques, `verdict`,
   `push_chain_covs`), la fin dans `fn end_track(&mut self, ty: u8)` (`count_track`). Puis :
   ```rust
   /// Comme [`Deposit::push_track`], pour une trace dont chaque géométrie a déjà été appariée par [`match_job`]
   /// (pure : même résultat que la mémoïsation). `edges` : `(wk, trv annoncé, appariement)` dans l'ordre de la trace ;
   /// `None` : document illisible.
   pub fn push_matched<'j, I>(&mut self, ty: u8, excluded: bool, edges: Option<I>)
   where
       I: IntoIterator<Item = (u8, f32, &'j Job)>;
   ```
   Elle ne touche pas aux compteurs `memo_hits` / `memo_misses`. Le service n'en change rien (il garde `push_track`).
3. **`ord-loops/src/lib.rs`** : `pub use cost::{is_greenway, multiplier_table, RideType};`. Justification : la classe
   « voie cyclable ou verte » du §3.2 est définie par `cost::is_greenway` ; le banc doit lire **la même** définition,
   pas une copie. Aucune autre ligne du moteur ne change. Rien dans `ord-loops-check` ni dans `ord-server`.

Pas de changement de version (`1.9.2`) : ni le service ni le contrat ne bougent.

### 11.3 Lecture de l'export, appariement, retenue

**Lecture** (`export.rs`) : gzip en flux (`flate2::bufread::MultiGzDecoder`), `CsvReader`, colonnes repérées par leur
nom d'en-tête : `team_id`, `track_id`, `surface_type`, `excluded_from_generator`, `network_edges` (les autres sont
ignorées). Par lots de 48 lignes analysées en parallèle (`rayon`, comme `pipeline::ingest` de l'outil), ordre conservé.
Structures :

```rust
pub struct TeamEdge { pub key: u32, pub trv: f32, pub wk: u8 }      // `key` : indice dans `Ingest::keys`
pub struct ExportTrack {
    pub team: usize,            // indice dans `Ingest::teams` (ids d'équipe, ordre d'apparition)
    pub id_hash: u64,           // xxh3_64(track_id) ; l'id lui-même n'est pas gardé
    pub ty: u8,                 // doc::type_code(surface_type)
    pub excluded: bool,
    pub readable: bool,         // document lu
    pub edges: Vec<TeamEdge>,   // dans l'ordre du document
}
pub struct Ingest {
    pub teams: Vec<u64>,
    pub tracks: Vec<ExportTrack>,
    /// Clés distinctes `(g, trv)`, dédoublonnées par `(xxh3_64(g), g.len(), trv.to_bits())` : exactement la clé de la
    /// mémoïsation du dépôt.
    pub keys: Vec<(Box<str>, f32)>,
    pub rows: u64,
    pub export_xxh3: u64,       // empreinte du fichier lu, comme l'outil
}
```

**Appariement** : `SegIndex::build(engine.base())` (le chemin de l'outil : l'index entier, 6 s et 8,9 Go de pic pour
l'outil sur la France **[mesuré, `lot6/overlay-report.json`]**), puis
`jobs: Vec<Job> = keys.par_iter().map(|(g, trv)| match_job(gf, &seg, &poly, g, *trv)).collect()`.

**Par trace** (en parallèle sur les traces), pour chaque `TeamEdge` dans l'ordre : `job = &jobs[key]`,
`m = if trv.is_nan() { job.glen } else { trv }` ; `total_m += m` ; selon `agg::verdict(wk, job)` : `Counted(chain)` →
`matched_m += m` et les `chain.pieces` sont ajoutés à la suite, dans leur ordre ; `Outside` → `outside_m += m` ;
`Fail` → trou (rien d'ajouté). `matched_share = matched_m / total_m` : c'est la notion du dépôt, trace par trace.

**Retenue d'une boucle** (dans cet ordre, chaque refus compté par équipe et par cause dans le résumé) :
non exclue ; document lu ; type `ROAD`, `GRAVEL` ou `MTB` (`MIXED` et inconnus refusés) ; `matched_share ≥ 0,95` ;
suite non vide ; plus long trou ≤ 1 000 m ; `D ≥ 5 km` ; boucle : distance planaire entre le premier et le dernier point
de la polyligne < `0,05 × L` (§11.4 pour D et L). Les traces non retenues **restent dans les dépôts** (§11.8).

### 11.4 La marche : une représentation commune au réel et au généré (`walk.rs`)

```rust
/// Une arête de base parcourue sur la portion [lo, hi] ⊂ [0, 1] de sa longueur (paramètre a → b), dans le sens
/// b → a si `reverse`.
pub struct Leg { pub edge: u32, pub reverse: bool, pub lo: f64, pub hi: f64 }
pub enum Join { Node(u32), Touch, Hole(f64) }   // jonction entre deux jambes ; `Hole` : trou, longueur planaire (m)
pub struct Walk { pub legs: Vec<Leg>, pub joins: Vec<Join> }   // joins.len() == legs.len() - 1
```

**Depuis une trace réelle** (`Walk::from_pieces`) : un `Piece { edge, from, to, elen }` donne
`lo = min(from, to) / elen`, `hi = max(from, to) / elen` (bornés à [0, 1]), `reverse = !piece.forward()`. Puis, dans
l'ordre :
1. jeter les morceaux de moins de 1 m (`matcher::MIN_PIECE_M` : bruit de recalage aux carrefours) ;
2. fusionner deux morceaux consécutifs de la même arête et du même sens si la sortie de l'un et l'entrée de l'autre sont
   à 2 m ou moins le long de l'arête ;
3. une jambe dont l'entrée (ou la sortie) est à 2 m ou moins d'une extrémité est **à ce nœud** (fraction ramenée à 0
   ou 1) ;
4. jonction : `Node(n)` si la sortie de la jambe i et l'entrée de la jambe i + 1 sont au même nœud `n` ; sinon `Touch`
   si leurs points sont à 5 m ou moins ; sinon `Hole(d)`.

Une marche est **continue** si toutes ses jonctions sont `Node` (seules la première et la dernière jambe peuvent alors
être partielles).

**Depuis une `Route`** (`Walk::from_route`) : une jambe par `Step` ; `lo, hi = 0, 1` sauf l'entrée de la première
(`route.start.offset_m / g`) et la sortie de la dernière (`route.end.offset_m / g`), où `g` = longueur haversine de la
géométrie de l'arête (`node_pos(a)`, `geometry`, `node_pos(b)`, sommée par `ord_loops_check::haversine_m`) ; une seule
étape : les deux. Toutes les jonctions sont `Node(nœud de sortie de l'étape i)`.

**Vers une `Route`** (`Walk::to_route`, marche continue seulement) : `steps` = les jambes ; `start` =
`Position { edge: legs[0].edge, offset_m: entrée × g }`, `end` de même avec la sortie de la dernière ; les champs de
mesure (`length_m`, `team_m`, …) à 0, `family: Turnaround`. Le contrôleur ne lit que la suite, les positions et le
graphe.

**Trace géométrique** (`Walk::trace(gf) -> Trace`) : repère local `ord_graph_match::geo::Frame` centré sur le point
d'entrée de la première jambe (le départ vaut donc (0, 0)) ; pour chaque jambe, la polyligne complète de l'arête en
mètres locaux, coupée aux fractions `lo`, `hi` **le long de sa longueur locale**, retournée si `reverse` ; concaténée,
points consécutifs égaux retirés (un `Touch` ou un `Hole` se referme par le segment droit implicite). La `Trace` porte :
`pts`, `cum` (abscisses cumulées), `join_arc[j]` (abscisse de la jonction j), `leg_m[i] = (hi − lo) × len_dm / 10`
(longueur **du graphe**, la même pour le réel et le généré), `d_graph = Σ leg_m + Σ trous`, `holes_m`, `max_hole_m`,
`l_poly` (longueur de la polyligne).

D de la requête = `d_graph` ; D des mesures de forme = `l_poly` (cohérentes entre elles, la différence est contrôlée,
§11.10).

### 11.5 Mesures (`shape.rs` pures, `measure.rs` sur le graphe)

```rust
#[derive(Clone, Debug, Serialize)]
pub struct Measures {
    pub d_km: f64,
    pub r_over_d: f64,
    pub far_pos: f64,
    pub compactness: f64,
    pub vertices: u32,          // §11.5.3, la définition figée
    pub vertices_10: u32,       // même règle à 10 % de D
    pub vertices_legacy: u32,   // règle de shapes.py sur la même polyligne
    pub turns_per_10km: f64,
    pub shared_share: f64,
    pub crossings: Option<u32>, // contrôleur ; `None` pour une marche réelle non continue
    pub way: [f64; 6],          // parts de longueur par classe de voie (§3.2)
    pub surface: [f64; 3],      // revêtu connu, présumé, non revêtu connu
    pub marked: f64,            // part sur un itinéraire balisé
    pub destination: f64,       // part en accès `destination`
    pub gap_share: f64,         // trous / D (0 pour le généré)
}
```

Liste ordonnée des mesures comparées (`MEASURES: [(&str, fn(&Measures) -> Option<f64>)]`) : `r_over_d`, `far_pos`,
`compactness`, `vertices`, `vertices_10`, `vertices_legacy`, `turns_per_10km`, `shared_share`, `crossings`,
`way_major`, `way_tertiary`, `way_minor`, `way_street`, `way_cycle`, `way_path`, `surf_paved`, `surf_presumed`,
`surf_unpaved`, `marked`, `destination`.

#### 11.5.1 Forme (sur `pts`, D = `l_poly`)

- **r / D** : `r` = maximum, sur les sommets de `pts`, de la distance planaire au départ ; **`far_pos`** = abscisse de ce
  sommet / D (le **premier** maximum strict : mise à jour sur `>` seulement).
- **Compacité** : `4π A / D²`, `A` = valeur absolue de l'aire de Gauss (lacet) du polygone `pts` **fermé** (segment
  dernier → premier compris ; le départ étant l'origine, c'est aussi l'aire de `shapes.py`).
- **Barycentre** (pour le cap, §11.6) : centre des segments pondéré par leur longueur.

#### 11.5.2 Virages, recouvrement, croisements

- **Virages** (`shape::turns(pts, cum, junctions: &[(f64, u8)], min_deg: u8, min_angle: f64, window_m: f64) -> u32`,
  pure) : pour chaque jonction `Node` d'abscisse `s` et de degré ≥ 3, cap d'entrée = cap du vecteur
  `P(s − 25 m) → P(s)`, cap de sortie = `P(s) → P(s + 25 m)` (`P` interpolé sur la polyligne, abscisse bornée à
  [0, L]) ; angle = `|cap sortie − cap entrée|` ramené à [0, 180] ; compté si > 45°. Les jonctions `Touch` / `Hole`, le
  départ et l'arrivée ne comptent pas. `turns_per_10km = virages / (l_poly / 10 000)`.
- **Degré d'un nœud** (`measure::degree(gf, n, e_in, e_out)`) : nombre d'ids d'arête distincts dans
  `gf.cell(ci).out(n − gf.cell_info(ci).first_node)` (`ci = gf.node_cell(n)`), **plus** `e_in` et `e_out`. Le CSR ne
  porte que les sens ouverts au vélo : ajouter les deux arêtes de la marche compte l'arrivée par un sens unique.
- **Recouvrement** : `shared_share = Σ len(e) / d_graph` sur les arêtes de base prises par **deux jambes ou plus**
  (sens confondus), chaque arête comptée une fois, `len(e) = len_dm / 10`. Même fonction pour le réel et le généré ; pour
  le généré, l'écart à `Route::shared_m` est contrôlé (§11.10).
- **Croisements** : `Report::crossings` du contrôleur indépendant (`Checker::with_tables`, `Config::default()`) : pour
  le généré, le rapport qui sert déjà à garder le parcours ; pour le réel, sur `Walk::to_route` d'une marche **continue**,
  requête `{ start: départ de la marche, end: None, bearing: None, distance_m: d_graph, ride_type, mode: Discovery,
  familiarity: 0.5, off_team_cost_q8: 256, seed: 0 }` (découverte : aucune règle d'équipe). Marche non continue :
  `None`, et la mesure exclut la boucle (compte rapporté). Les violations du contrôleur sur le réel sont gardées
  (§11.10, « le contrôleur sur les boucles réelles ») ; `WRONG_END` y est attendu (départ et arrivée réels ne sont pas
  la même position) et rapporté à part.

#### 11.5.3 Sommets : la définition figée, et les trois autres comptes

**Définition figée (`vertices`)**, à recopier dans la doc de la fonction : sur la polyligne **ouverte au départ**
`p[0..n]` (arrivée non refermée), `tol = 0,05 × D` :
1. `f` = indice du sommet le plus éloigné du départ (premier maximum strict) ; si `|p[f]| < tol`, 0 sommet ;
2. garder `{0, f, n − 1}`, puis Douglas-Peucker récursif (pile, comme `shapes.py`) sur `[0, f]` et `[f, n − 1]`, avec la
   distance d'un point **au segment** (projection bornée ; segment dégénéré : distance au point), coupe si
   `distance > tol` ;
3. `vertices = |gardés| − 2` (le départ et son double ne comptent pas).
Aller-retour = 1, triangle = 2, quadrilatère partant d'un coin = 3 ; cercle = 3 ; lentille = 1 + un par côté dont la
flèche dépasse 5 % de D.

**`vertices_10`** : la même règle à `tol = 0,10 × D` (la lentille d'une compacité de 0,30 y vaut 1 : c'est le compte
candidat pour les proportions du tirage du §4.1). **`vertices_legacy`** : la fonction `dp` de `shapes.py` portée
exactement (corde `p[0] → p[n − 1]`, distance à la **droite**, ou au point si la corde est nulle ; `len(keep) − 2`),
sur la même polyligne appariée. **Étalonnage `raw_legacy`** (`shape::calibrate`, sans graphe) : `shapes.py` porté **en
entier** sur les `g` bruts de l'export, ligne par ligne : concaténation `pts += g if not pts else g[1:]`, `tot` = somme
des `trv` (absent → 0), rejet si `len(pts) < 50` ou `tot < 5000`, projection `((lon − lon0) × 111320 × cos lat0,
(lat − lat0) × 110540)`, un point sur trois (`[::3]`), mêmes D, écart, `rmax`, `fr` (abscisse depuis le point 1), aire
sans terme de fermeture, boucle si `gap < 0,05 D`, quantiles `v[min(n − 1, int(n × p / 100))]`, histogramme
`min(corners, 6)`, médianes par tranche de D. `--calibrate-only` écrit ces lignes **au format de `shapes.py`** (même
texte, mêmes arrondis `.3f`) : `diff` avec la sortie de `python3 -I harness/shapes.py data/prod-network.csv.gz` doit
être vide, à un dernier chiffre près au plus sur quelques quantiles (`math.dist` et `hypot` peuvent différer d'un ulp).
La ligne « unpaved share » de `shapes.py` lit `sc`, absent de `ParsedEdge` : elle n'est pas portée.

Le résumé met côte à côte les histogrammes de `raw_legacy`, `vertices_legacy`, `vertices` et `vertices_10` sur les
boucles réelles retenues : l'effet des données (brut contre apparié) et celui de la définition (droite contre départ,
5 % contre 10 %) se lisent séparément.

#### 11.5.4 Classes de voie et de revêtement (§3.2), par jambe, pondérées par `leg_m`

`measure::way_class(a: Attrs) -> usize`, dans cet ordre : `Cycleway` ou `ord_loops::is_greenway(a)` → 4 (voie cyclable
ou verte) ; `Primary | Secondary | Trunk | Motorway` → 0 (grande route ; `Motorway`, fermée au vélo, n'y arrive que
par une trace réelle) ; `Tertiary` → 1 ; `Unclassified | Road` → 2 (petite route) ; `Residential | LivingStreet |
Service` → 3 (rue) ; tout le reste et classe absente → 5 (chemin). Les `_link` suivent leur classe.
`surface_group(a)` : `a.surface() == 0` → 1 (présumé) ; sinon `a.is_unpaved()` → 2 ; sinon 0. `marked` :
`a.routes() != 0`. `destination` : `a.access() == Access::Destination`.

### 11.6 Requêtes, jeu, choix des parcours (`play.rs`)

**Requête d'une boucle réelle** : `start` = point d'entrée de la première jambe (lat, lon) ; `end: None` ;
`distance_m = round(d_graph / 1000) × 1000` ; `ride_type` : ROAD → `Road`, GRAVEL → `Gravel`, MTB → `Mtb` ; `bearing` =
cap du départ vers le barycentre (§11.5.1) arrondi au plus proche multiple de 45° (`Bearing8`), `None` si le barycentre
est à moins de `0,02 × l_poly` du départ ; `seed = id_hash ^ args.seed` (fixe, propre à la trace, l'id n'en sort
pas) ; `mode`, `familiarity`, `off_team_cost_q8` = `cursor::apply(cursor::hundredths(c))` pour chaque `c` de
`--cursors` (0,25 → équipe d'abord, λ = 2,5, q8 = 640, familiarité 0,5 ; 1 → découverte, q8 = 256).

**Jeu** (un appel par requête, pas de tours chauds : le temps n'est qu'indicatif, les fils se partagent la machine) :
fenêtre et agrégats par le cache partagé (§11.9) ; `generate_with_diagnostics_in(engine, &win, Some(TeamCtx { data,
win: &wt }), &req, ws, &mut || false)` sous `catch_unwind` (comme `record::one_call`). `Err(WindowEscape)` : **une**
reprise dans la fenêtre élargie du service, `engine.plan_with(&req, step, engine.longest_chain_m(vi) + 10_000.0)`,
construite hors cache ; une seconde fuite est rapportée `WINDOW_ESCAPE` (attendu : 0).

**Parcours gardés** : ceux de `Generation::routes`, dans l'ordre, que `checker.check(&req, &route).ok()` accepte (le
service). Les refusés sont comptés par code. Pour chaque parcours gardé : `Measures` (sur `Walk::from_route`),
`length_over_target = report.length_m / distance_m`, `team_share = report.team_m / report.length_m`, `family`,
`recovered` (§11.8, contre la surcouche de ce jeu), `route_xxh3` = xxh3 des octets `(base_edge u32 LE, reverse u8)` de
chaque étape suivis des deux décalages arrondis au décimètre.

- **Premier** = le premier gardé.
- **Meilleur des trois, pour les distributions** (`best_shape`) : le gardé qui minimise
  `Σ_k |m_k − réel_k| / IQR_k` sur `k ∈ {r_over_d, far_pos, compactness}`, `IQR_k` du réel **du même type** sur toutes
  les équipes (1 si nul) ; égalité : le plus petit rang. Il mesure ce que le moteur **sait** proposer parmi trois.
- **Meilleur des trois, pour la reconstruction** (`best_recovered`) : le gardé de plus grande part retrouvée totale ;
  égalité : le plus petit rang.

Issue d'un jeu : `OK` (au moins un gardé), `VIOLATION` (des parcours, aucun gardé), code d'échec (`record::failure_code`),
`PANIC`, `WINDOW_ESCAPE`. Les distributions générées ne prennent que les `OK` ; la part de réussite est rapportée à côté
de chaque table.

### 11.7 Distributions : quantiles, Wasserstein

- **Quantiles** P10, P25, P50, P75, P90 par interpolation linéaire (`h = (n − 1) p`, type 7). Seul l'étalonnage
  `raw_legacy` garde la règle de `shapes.py`.
- **W1** entre deux échantillons à une dimension de tailles quelconques :
  `W1 = ∫ |F_réel(x) − F_gén(x)| dx`, calculée exactement sur les deux échantillons triés (parcours fusionné des points
  de rupture, `|Fa − Fb| × Δx`). **Normalisée** : `W1 / (P75 − P25)` du réel ; `None` si l'écart interquartile est nul
  (écrit « IQR nul », W1 brute donnée à côté).
- **Δ médian apparié** : médiane, sur les requêtes `OK`, de `généré − réel` de la même boucle.
- Groupes : par type (ROAD, GRAVEL, MTB ; un groupe de moins de 30 boucles est marqué « indicatif »), par curseur, pour
  le premier et pour `best_shape`, toutes équipes ; puis **par équipe** (équipes d'au moins 30 boucles du type ; les
  autres réunies en « autres ») pour `r_over_d`, `compactness`, `vertices`, `vertices_10` et `turns_per_10km`. Les
  distributions prennent tous les jeux sur la surcouche **complète** (boucles mises à l'écart comprises).

### 11.8 Reconstruction (`holdout.rs`)

**Mise à l'écart, stable** : par équipe ayant au moins `--holdout-min-loops` (10) boucles retenues, trier ses boucles
retenues par `id_hash` croissant et mettre à l'écart les `ceil(0,10 × n)` premières. Ne dépend que de l'ensemble des
boucles retenues de l'équipe : ni de l'ordre de lecture, ni des autres équipes, ni du nombre de fils. Équipes sous le
seuil : rapportées « trop peu de boucles », pas de reconstruction.

**Surcouches, par le code du dépôt** : pour chaque équipe, `Deposit::new(gf, &seg, &poly, 1)` et, pour **toutes** ses
traces dans l'ordre de l'export (exclues, illisibles, non retenues comprises : le dépôt les reçoit aussi),
`push_matched(ty, excluded, readable.then(|| edges.iter().map(|e| (e.wk, e.trv, &jobs[e.key]))))` ; la variante
« écart » saute les traces mises à l'écart. `finish()` → `ord_graph::overlay::encode(&OverlayMeta { graph:
gf.graph_identity(), team_id: 0, export_xxh3, export_rows: stats.tracks, params_hash: agg::params_hash(), tool_version,
build_epoch: 0, types: TYPES })` → `Overlay::decode`. Toutes les surcouches (complètes et « écart ») sont faites
**avant** de lâcher l'index et les appariements (§11.9). **Contrôle** : la surcouche complète doit égaler, arête par
arête et compte par compte, le fichier de `--overlays` de l'équipe (`<id>-france-20261115-z11.ordo`) ; le résumé dit
« identique » ou le nombre d'arêtes différentes, sans faire échouer le banc. Les jeux du §11.6 se font sur la surcouche
**reconstruite** (c'est celle que le service tient après un dépôt).

**Jeux** : chaque boucle mise à l'écart est rejouée, même requête (même graine), sur la surcouche « écart » de son
équipe (`TeamData` et `TeamCheck` construits pour elle).

**Part retrouvée** (`Recovered`), pour un parcours généré `G` et la boucle réelle `R`, contre la surcouche `O` du jeu :
`E(X)` = ensemble des arêtes de base de X (sens ignorés, chaque arête une fois) ; `len(e) = len_dm / 10`.
- `total = Σ_{E(R) ∩ E(G)} len / Σ_{E(R)} len` ;
- `team` : la même sur `E_team(R) = { e ∈ E(R) : ord_loops_check::team_edge(ride, attrs(e), O.get(e), &TypeIndex::of(O)) }`
  (la notion « d'équipe pour ce type » du contrôleur) ;
- `fresh` : la même sur `E_fresh(R) = { e ∈ E(R) : O.get(e)` absent ou tout à zéro `}` — les arêtes qu'**aucune autre
  trace** de l'équipe ne prend ;
- `fresh_real_share = Σ_{E_fresh(R)} len / Σ_{E(R)} len` : la part de nouveau dans la trace elle-même (contexte).
`None` pour une part dont le dénominateur est nul. Calculée pour **tous** les parcours gardés, y compris sur la
surcouche complète (lecture « dans l'échantillon », pour voir l'écart avec la reconstruction).

Rapport, **par équipe** et par curseur, pour le premier et pour `best_recovered` : nombre mis à l'écart, réussite,
médiane par trace et part **agrégée** (Σ mètres retrouvés / Σ mètres réels) de `total`, `team`, `fresh`, et la
médiane de `fresh_real_share` ; puis les mêmes chiffres « dans l'échantillon » (surcouche complète) pour les mêmes
traces. Jamais seulement au total.

### 11.9 Phases, parallélisme, mémoire, temps, déterminisme

**Phases** (`likeness::run`) :
1. Ouvrir le moteur (`Engine::new`, fenêtré, `Params::default()`) et `GraphIndex::build(engine.base())` ; tout le banc
   lit `engine.base()` comme graphe (`MappedGraph` = `GraphFile<Mmap>`).
2. Lire l'export ; étalonnage `raw_legacy` en parallèle sur les traces.
3. `SegIndex` ; appariement des clés distinctes (`rayon`).
4. Marches, parts appariées, retenue (`rayon` sur les traces) ; mise à l'écart.
5. Surcouches complètes et « écart » de chaque équipe (`rayon` sur les équipes, un `Deposit` chacune) ; contrôle
   d'égalité.
6. Lâcher `SegIndex`, `jobs` et `keys` (le pic de mémoire est derrière).
7. Mesures du réel (`rayon` sur les boucles, sauf les croisements) ; IQR par type.
8. Par équipe, dans l'ordre des lettres : `TeamData::plan_build` / `build_planned` et `TeamCheck::plan` /
   `build_planned` sur la surcouche complète ; un `Checker::with_tables` partagé par les fils (il est `Sync`) ; **liste
   de tâches** : contrôle de chaque boucle réelle continue, puis un jeu par (boucle, curseur), triée par (vue, case de
   0,05° du départ) pour la localité du cache ; exécution par les fils. Puis, si l'équipe a des boucles mises à l'écart,
   même chose sur la surcouche « écart » (jeux seulement). Les agrégats de l'équipe sont ensuite retirés du cache
   (`drop_team`) et ses données lâchées.
9. Tri des résultats par (lettre, i, curseur, surcouche), écriture du JSON et du résumé.

**Fils** : `std::thread::scope`, `--threads` fils, un `Workspace` **par fil** créé une fois (réserve de `Workspace`
réutilisée d'une équipe à l'autre), un `AtomicUsize` pour la tâche suivante, résultats rendus par fil puis triés.
**Cache partagé** `SharedWindows` (nouveau, dans `play.rs` ; le `Windows` du banc reste tel quel) : un
`Mutex<WindowCache>` sous `--budget-mb`. Acquisition : `plan = engine.plan(&req, step)` ; sous le verrou,
`lookup(vi, &plan.need)` ; manqué → hors verrou, `build_window(vi, &plan.class, …)` ; sous le verrou, `make_room(est)`
puis `insert` (si la place manque parce que tout est tenu, insérer quand même et compter le dépassement : le budget du
banc est souple). Agrégats d'équipe de même (`team` / hors verrou `WindowTeam::build` / `insert_team`), clé d'équipe
`2 × t` pour la surcouche complète, `2 × t + 1` pour « écart ». Deux fils peuvent bâtir la même fenêtre : sans effet
sur les résultats.

**Mémoire [estimé]** : pic en phase 3 (`SegIndex` et appariements, ~9 Go comme l'outil) plus le moteur fenêtré ouvert
(~1 à 2 Go) ; en phase 8, cache ≤ 6 Go, espaces de travail ≤ 16 × ~0,2 Go, une équipe (~0,1 Go). ~12 Go au plus, sous
les 30 Go du poste. Le banc échantillonne `RssAnon` (`windows::AnonSampler`) et rapporte le pic de chaque phase.

**Temps [estimé]** : ~2 900 boucles retenues × 2 curseurs + ~300 mises à l'écart × 2 ≈ 6 400 générations ; à 0,3 à
0,5 s de calcul chacune (163 ms au médian et 442 ms au P90 jusqu'à 100 km au lot 2, plus long au-delà), 16 fils :
3 à 5 min ; lecture, appariement, dépôts et constructions d'équipe : ~2 min. **Une dizaine de minutes** la campagne.

**Déterminisme** : résultat indépendant du nombre de fils et de l'ordre d'exécution (graine fixe par trace ; une
fenêtre qui contient les tuiles nécessaires rend le même parcours, lot 6 §5.4 ; mise à l'écart par empreinte ;
`build_epoch: 0`). Le résumé imprime une **empreinte de résultat** : xxh3 des lignes triées
`lettre i curseur surcouche issue route_xxh3…` (sans aucun temps). Critère : même empreinte avec `--threads 1` et
`--threads 16`.

### 11.10 Sorties

**`<label>-report.json`** : `meta` (lignes du banc existant : date, commit, machine, fils, charge, profil, graphe,
empreinte de l'export, paramètres, durée et pic par phase) ; `retention` par lettre (lignes, exclues, illisibles,
type refusé, part appariée, trou, < 5 km, non-boucle, retenues, mises à l'écart) ; `calibration` (`raw_legacy`) ;
`overlays` (lettre, arêtes, km comptés, égalité au fichier) ; `loops` : un enregistrement par boucle retenue :

```rust
struct LoopRec {
    team: String, i: u32, ty: String, d_km: f64, matched_share: f64, contiguous: bool, holdout: bool,
    real: Measures,
    real_check: Option<Vec<String>>,       // codes du contrôleur sur la marche continue
    plays: Vec<PlayRec>,
}
struct PlayRec {
    cursor: f64, overlay: String,           // "full" | "holdout"
    outcome: String, n_routes: usize, n_kept: usize, dropped: BTreeMap<String, u32>, ms: f64,
    routes: Vec<GenRoute>,                  // les gardés, dans l'ordre du moteur
    best_shape: Option<usize>, best_recovered: Option<usize>,
}
struct GenRoute {
    m: Measures, length_over_target: f64, team_share: f64, family: String,
    recovered: Recovered, shared_vs_route_m: f64, route_xxh3: String,
}
```

Aucun départ, aucune coordonnée, aucun id d'arête, de trace ou d'équipe.

**`<label>-summary.md`**, recopiable tel quel dans le rapport commité (lettres et agrégats seulement) :
1. Conditions (lignes `meta`).
2. Retenue par équipe.
3. Étalonnage : `raw_legacy` contre le plan §2 (r / D, position du point lointain, compacité, histogramme) ; puis les
   quatre histogrammes de sommets du §11.5.3 sur les boucles retenues, par type.
4. Surcouches reconstruites contre fichiers du lot 6.
5. Par type × curseur : une table par groupe, une ligne par mesure : réel P10 · P25 · P50 · P75 · P90, premier idem,
   W1 / IQR du premier, Δ médian apparié, `best_shape` P50 et W1 / IQR ; en tête : n réel, n généré, part `OK`.
6. Par équipe (§11.7).
7. Le contrôleur sur les boucles réelles continues : part continue, puis par code de violation la part des boucles qui
   l'enfreignent (`WRONG_END` à part).
8. Issues des jeux par curseur et surcouche (OK, VIOLATION, échecs par code, échappements, paniques).
9. Reconstruction par équipe (§11.8).
10. Cohérence : médiane et P90 de `|l_poly − d_graph| / d_graph` (réel et généré) et de `|shared − Route::shared_m|`.
11. **Lecture du §2.4**, écrite comme des faits, sans décision : par type et curseur, le P50 généré (premier) de r / D
    et de compacité est-il dans [P25, P75] du réel ; P50 généré de compacité contre P25 du réel ; part `fresh` agrégée de
    `best_recovered` par équipe en découverte.
12. Empreinte de résultat.

### 11.11 Tests (synthétiques, aucune donnée de production)

`shape.rs`, sur des polylignes densifiées (un point tous les 10 m), valeurs attendues **calculées dans le test** à
partir des formules :
- **cercle** de rayon R : r / D ≈ 1/π, compacité ≈ 1, `far_pos` ≈ 0,5, `vertices` = 3 (le diamètre s'écarte de
  R = 0,159 D, les quarts de 0,047 D), `vertices_10` = 3 ;
- **aller-retour** sur une droite : r / D = 0,5, compacité = 0, `vertices` = 1 ;
- **triangle** (0, 0) → (8 000, 0) → (4 000, 6 000) → départ : r / D = 8 000 / D, compacité = 4π × 24·10⁶ / D²,
  `vertices` = 2, `far_pos` = 8 000 / D ;
- **carré** partant d'un coin : `vertices` = 3 ;
- **lentille** de corde 10 km, deux arcs de flèche s : s = 300 m → `vertices` = 1, `vertices_10` = 1 ; s = 1 500 m →
  `vertices` = 3, `vertices_10` = 1 ;
- **règle de `shapes.py`** : aller-retour vers le nord terminé à 1 m au sud du départ → `vertices_legacy` = 0 (la corde
  est la droite du trajet) quand `vertices` = 1 ;
- **virages** : un angle droit à une jonction de degré 3 compte, le même à degré 2 non, un coude de 30° non, le départ
  non ;
- **W1** : échantillons égaux → 0 ; décalés de c → |c| ; {0, 1} contre {0, 0, 1, 1} → 0 ; {0} contre {1, 2} → 1,5 ;
  normalisation par l'IQR ; IQR nul → `None` ;
- **quantiles** : [1, 2, 3, 4] → P50 = 2,5, P10 = 1,3.

`holdout.rs` : même ensemble dans deux ordres → mêmes boucles ; `ceil(10 %)` ; une boucle ajoutée à une autre équipe ne
change rien ; équipe sous le seuil → aucune.

`walk.rs` et `measure.rs`, sur un petit graphe `MemSource` (grille de `residential`, une `cycleway`, comme
`ord-graph-match/tests/index.rs`, fichier dans un `tempfile`) : fusion de deux morceaux contigus d'une arête ; morceau de
0,5 m jeté ; jonctions `Node`, puis `Hole` quand un morceau manque ; `to_route` d'une marche continue passée au
`Checker` → ni `EMPTY`, ni `DISCONTINUOUS`, ni `BAD_OFFSET` ; `from_route(to_route(w))` rend `w` ; degré d'un nœud
intérieur de la grille = 4 ; part `way_cycle` = longueur de la `cycleway` / D.

`ord-graph-overlay/src/e2e_tests.rs`, test `the_service_deposit_gives_the_tool_overlay` : une variante de plus,
« appariements précalculés » (`match_job` puis `push_matched`), mêmes arêtes et mêmes comptes que l'outil.

`main.rs` : `Cli::try_parse_from(["ord-loops-bench", "--cursors", "0.25,1"])` rend `command: None` ;
`["ord-loops-bench", "likeness", "--limit", "3"]` rend `Likeness`.

### 11.12 Étapes et commits

Chaque commit passe `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` et les tests des crates
touchés (`cargo test -p ord-graph-match -p ord-graph-overlay -p ord-loops-bench`, plus `-p ord-loops` pour l'étape 2).

1. `refactor(match): le lecteur CSV de l'export passe dans ord-graph-match` — §11.2 point 1.
2. `feat(match): Deposit::push_matched, une trace aux géométries déjà appariées` et
   `feat(loops): is_greenway exporté pour le banc, sans changement de comportement` — §11.2 points 2 et 3, variante du
   test de l'outil.
3. `feat(bench): sous-commande likeness et mesures de forme` — `Cli`, `shape.rs` et ses tests, `--calibrate-only`
   (vérifié contre `shapes.py`, §11.5.3).
4. `feat(bench): likeness, suites appariées des traces de l'export` — `export.rs`, `walk.rs`, `measure.rs`, tests.
5. `feat(bench): likeness, requêtes en parallèle et distributions` — `play.rs`, `out.rs`.
6. `feat(bench): likeness, reconstruction par mise à l'écart` — `holdout.rs`, tests.
7. Campagne (§11.13), puis `docs(ord-loops): relevé du moteur 1.9.2 au banc de vraisemblance (lot 4, 4a)` : le rapport,
   et ce §11 corrigé de ce que le codage a appris (écarts signalés comme tels).

### 11.13 Campagne et rapport

Binaire en `--release`. Dans l'ordre :
1. `ord-loops-bench likeness --calibrate-only` contre `shapes.py` (§11.5.3).
2. Essai court : `--teams C --limit 5`.
3. Déterminisme : `--teams B --threads 1 --label det1` puis `--threads 16 --label det16` : même empreinte.
4. Campagne : toutes les équipes, curseurs 0,25 et 1, reconstruction.

**Rapport** `docs/design/technique/ord-loops-lot4-mesures.md` (style de `ord-loops-lot6-mesures.md`, marques de l'en-tête de cette note) :
conditions et commandes ; retenue ; étalonnage et les quatre comptes de sommets, avec **la définition proposée pour le
§4.1** ; distributions ROAD et GRAVEL aux deux curseurs ; par équipe ; le contrôleur sur le réel ; reconstruction par
équipe ; lecture du §2.4 et ce qu'elle propose pour l'ordre des §3 à §5 ; écarts du codage. **Équipes lettrées et
agrégats seulement**, jamais une coordonnée ni un id. Puis **arrêt et relecture** (§10, étape 1) : rien du 4b ni du
4c ne se code avant.

### 11.14 Ce que le 4a ne fait pas

Pas de GeoJSON ni de visionneuse (un besoin de regarder un cas se traitera par le banc existant) ; pas de jeu HTTP ;
pas de mode strict par défaut (`--cursors 0` le permet) ; pas de profil d'équipe (4b) ni de règle de forme dans le
moteur ; aucun réglage du moteur, du contrôleur ou du service.

### 11.15 Écarts du codage (17 novembre, après relecture et campagne)

Ce que le codage et la relecture ont changé à la spécification ci-dessus ; détail et effets mesurés au §11 de
[`ord-loops-lot4-mesures.md`](ord-loops-lot4-mesures.md), qui porte aussi le relevé et ce qu'il propose pour les §3 à §5.

- **§11.4, raccord au nœud commun** : deux jambes voisines arrêtées à 25 m ou moins, le long de leur arête, d'un nœud
  qu'elles partagent sont prolongées jusqu'à lui (`Walk::bridge_at_nodes`), avant le classement des jonctions.
- **§11.8, E(R)** : une arête de la trace ne compte que si le dépôt la compte (sens ouvert au vélo, union couverte ≥
  `coverage_threshold`) ; mise à l'écart en `ceil(0,10 × n)` sans erreur d'arrondi flottante.
- **§11.7, croisements** : le généré d'une mesure ne prend que les boucles dont le réel est mesuré.
- **§11.10, jumelles** : `twin_share` dans `LoopRec` (part des mètres d'une boucle mise à l'écart prise par une autre
  boucle retenue de l'équipe), deux colonnes de plus en section 9 du résumé.
- **§11.13, étalonnage** : la sortie de `shapes.py` se compare tronquée au nombre de lignes de
  `<label>-calibration.txt` (la ligne « unpaved share » n'est pas portée).
- **§11.9, mémoire** : pic anonyme de 14,2 Go à la campagne France (estimé ~12 Go) ; durée 231 s.

---

## 12. Sonde du goût : spécification

Écrit le 17 novembre 2026, après le relevé du 4a (`ord-loops-lot4-mesures.md` §4, §7.1, §12) et la décision 7 du §9.1,
à la lecture de `ord-loops` (`cost.rs`, `search.rs` : `Weighter`, `team.rs` : `WindowTeam`, `window.rs`, `generate.rs`),
de `ord-server/src/generator/cursor.rs` et du banc (`tools/ord-loops-bench/src/likeness/`). Cette section **suffit au
codeur** : ce qu'elle ne dit pas est laissé à son jugement, à condition de tenir le §12.1 (moteur neutre par défaut) et
de ne toucher ni au service (`ord-server`), ni au contrôleur (`ord-loops-check`).

**Question posée** : un goût appris par classe de voie, appliqué **à toutes les arêtes de la vue** (arêtes d'équipe
comprises), aux deux curseurs, réduit-il de moitié au moins les écarts de classes entre parcours générés et réels
(critère de la décision 7 pour lancer le 4b complet) ? Et la familiarité au-dessus de 0,5, qui ne demande aucun code
moteur, en fait-elle autant ? Rien n'est stocké au dépôt, rien ne change au service.

### 12.0 Ce que la lecture a établi

1. **Un seul point d'entrée des poids** : `Weighter` (`search.rs`). `chain(c)` lit `w_osm[c]` (poids sans équipe de la
   chaîne, calculé à la construction de la fenêtre, `window.rs` étape 5) et, s'il y a une équipe, le `ChainTeam`
   `{a, b, z, strict}` de `WindowTeam` (sommes des poids sans équipe des arêtes d'équipe et comblées) :
   `λ·(w − a − b) + M[z]·a + b`. `edge(e)` refait la même chose pour une arête de base (morceaux partiels du départ et
   de l'arrivée, `make_anchor`). Les arbres, l'A\* (heuristique = arbre retour, calculé avec le même `Weighter`) et
   l'accroche (`w.chain(c) != INF`) ne lisent rien d'autre. La longueur (`Tree::meters`, choix du demi-tour, tolérance)
   est tenue à part des poids.
2. **`Weighter::new` ignore l'équipe en découverte** (`(Mode::Discovery, _) => None`) : ni λ, ni `M[z]`. La familiarité
   n'a donc **aucun effet** au curseur 1 ; la variante `fam` y joue « équipe d'abord à λ = 1 » (§12.3).
3. **`Window` est partagée par toutes les équipes** (cache du service et du banc) ; **`WindowTeam` est déjà propre à une
   équipe et à une fenêtre**. Un goût d'équipe y a sa place naturelle : la fenêtre ne change pas, la relaxation non
   plus (elle lit toujours un `u32` par chaîne), seuls les agrégats d'équipe portent des poids au goût. C'est aussi la
   forme que prendrait le 4b au service (§3.2 : « 12 → 16 octets par chaîne et par type »).
4. **Poids entiers** : `edge_weight = ⌈len_dm × K / 64⌉`. Un multiplicateur en 256es appliqué **dans** le même arrondi,
   `⌈len_dm × K × q / (64 × 256)⌉`, rend **exactement** `edge_weight` pour `q = 256` (⌈256x / 16 384⌉ = ⌈x / 64⌉) :
   la neutralité est arithmétique, pas approchée.
5. **Les coûts de virage sont absolus** (`TURN_COST_MAX` = 2 000, en décimètres pondérés) : un goût qui baisse les poids
   d'une classe rend les virages relativement plus chers sur elle. Effet du second ordre, accepté, signalé au relevé.
6. Le changement de mode au curseur 1 de `fam` (découverte → équipe d'abord) change aussi la graine des secteurs
   (`generate::fingerprint` lit le mode) et active le terme d'équipe de la note de choix et le filtre `min_share` :
   `fam` au curseur 1 se compare **en distribution**, pas parcours à parcours.
7. Le §3.2 réservait le goût aux arêtes non roulées : **remplacé pour la sonde** par la décision 7 du §9.1 (toutes les
   arêtes de la vue). Les sections précédentes ne sont pas réécrites.

### 12.1 Le crochet moteur (`crates/ord-loops`)

**Optionnel et neutre par défaut** : sans goût, aucune ligne exécutée ne change de résultat ; avec un goût neutre
(256 partout), les poids sont identiques au bit près. Entiers seulement (règle de `multiplier_table` : même résultat
natif et WASM). Aucune nouvelle dépendance, aucun changement de version.

**`cost.rs`** (ajouts) :

```rust
/// Nombre de classes de goût : 6 voies × 3 revêtements × balisé × destination (§3.2).
pub const TASTE_CLASSES: usize = 72;

/// Voie (§3.2), dans cet ordre : `Cycleway` ou `is_greenway` → 4 (voie cyclable ou verte) ;
/// `Primary | Secondary | Trunk | Motorway` → 0 (grande route) ; `Tertiary` → 1 ; `Unclassified | Road` → 2 (petite
/// route) ; `Residential | LivingStreet | Service` → 3 (rue) ; tout le reste et classe absente → 5 (chemin).
pub fn taste_way(a: Attrs) -> usize;      // le corps actuel de `likeness::measure::way_class`, déplacé tel quel
/// `surface() == 0` → 1 (présumé) ; sinon `is_unpaved()` → 2 (non revêtu connu) ; sinon 0 (revêtu connu).
pub fn taste_surface(a: Attrs) -> usize;  // le corps actuel de `likeness::measure::surface_group`
/// `((voie × 3 + revêtement) × 2 + balisé) × 2 + destination`, balisé = `routes() != 0`,
/// destination = `access() == Access::Destination`. Pure, dans [0, 72).
pub fn taste_class(a: Attrs) -> usize;

/// Goût par classe : multiplicateur du poids d'une arête de base, en 256es, par type (`RideType::index()`) et par
/// classe de goût. 256 = neutre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taste {
    pub q8: [[u16; TASTE_CLASSES]; 3],
}
impl Taste {
    pub fn neutral() -> Self;            // 256 partout
    pub fn is_neutral(&self) -> bool;    // 256 partout, les trois types
}

/// Poids d'une arête de base au goût : `⌈len_dm × K × q8[classe] / (64 × 256)⌉`, borné à `u32::MAX / 8` comme
/// `edge_weight`. Avec 256 partout, rend exactement `edge_weight` (§12.0, point 4).
pub fn taste_weight(ride: RideType, a: Attrs, len_dm: u32, q8: &[u16; TASTE_CLASSES]) -> u32;
```

(Le produit tient en `u64` : `len_dm` < 2³², `K` ≤ 1 000, `q8` < 2¹⁶.) `Taste` ne dérive pas `Default` (tableaux de
plus de 32 éléments) : `neutral()` en tient lieu.

**`team.rs`, `WindowTeam`** :

```rust
pub struct WindowTeam {
    per: [Vec<ChainTeam>; 3],
    /// Sonde du goût (§12) : poids sans équipe de chaque chaîne **au goût**, par type ; vide = pas de goût.
    w_taste: [Vec<u32>; 3],
    taste: Option<Box<Taste>>,
}

impl WindowTeam {
    /// Inchangée pour l'appelant : `build_with_taste(e, win, team, None)`.
    pub fn build<B: AsRef<[u8]>>(e: &Engine<B>, win: &Window, team: &TeamData) -> Self;
    /// Avec `Some(taste)`, pour **chaque** type servi par la vue (même si sa table est neutre : les tests y passent
    /// par l'arithmétique), le poids de chaque lien est `taste_weight(ride, attrs, len, &taste.q8[ride])` :
    /// `w_taste[ride][c]` = Σ sur tous les liens (borné à `u32::MAX / 8` comme `w_osm`), et `a`, `b`, `z` de
    /// `ChainTeam` sont faits des **mêmes** poids au goût. Avec `None`, exactement le calcul d'aujourd'hui.
    pub fn build_with_taste<B: AsRef<[u8]>>(e: &Engine<B>, win: &Window, team: &TeamData, taste: Option<&Taste>) -> Self;
    /// `(poids au goût des chaînes, table du type)` si un goût a été donné.
    pub(crate) fn taste_of(&self, ride: RideType) -> Option<(&[u32], &[u16; TASTE_CLASSES])>;
}
```

`bytes()` compte aussi `w_taste` ; `estimate_bytes` ne change pas (le service ne passe jamais de goût). Avec un goût,
la construction lit `attrs_len` de **tous** les liens (aujourd'hui : ceux d'équipe ou comblés seulement) : c'est son
seul surcoût de temps, mesuré par le banc (§12.5).

**`search.rs`, `Weighter`** : un champ `taste: Option<&'a [u16; TASTE_CLASSES]>`, et dans `new` :

```rust
// Indépendant du mode : le goût s'applique aussi en découverte dès qu'il y a une surcouche (décision 1 du §9.1).
let taste = team.and_then(|t| t.win.taste_of(ride));
w_osm: taste.map_or(&v.w_osm[ride.index()][..], |(w, _)| w),
taste: taste.map(|(_, q)| q),
```

`chain` ne change pas (`w − a − b` reste cohérent : les trois sommes sont faites des mêmes poids par arête). Dans `edge`,
`let w = match self.taste { None => cost::edge_weight(..), Some(q) => cost::taste_weight(.., q) };`, le reste
inchangé. En strict, le goût s'applique aussi mécaniquement (sans objet au banc, qui ne joue pas le strict).

**`lib.rs`** : `pub use cost::{is_greenway, multiplier_table, taste_class, taste_surface, taste_way, taste_weight,
RideType, Taste, TASTE_CLASSES};`.

**Comment le goût traverse les fenêtres précalculées** : ni la `Window` (ses `w_osm` restent ceux d'OSM, partagés par
toutes les équipes et toutes les variantes, le cache n'est pas touché), ni la relaxation (toujours un `u32` lu par
chaîne). Le goût n'existe que dans `WindowTeam`, déjà construite par (fenêtre, équipe) : au banc, par (fenêtre, équipe,
surcouche, variante). Coût : 4 octets par chaîne et par type servi en plus des 12 de `ChainTeam` (vue `road` : un type ;
vue `gravel` : deux, `Gravel` et `Mtb`), et une passe sur tous les liens de la fenêtre à la construction.

**Neutralité, prouvée par** (§12.8) : les tests et fixtures existants **inchangés** (aucune re-bénédiction, jamais
`ORD_LOOPS_BLESS`) ; la référence figée `window_reference.txt` rejouée avec `build_with_taste(Some(&Taste::neutral()))`
(140 requêtes, deux zooms, les trois modes) ; et, à l'échelle, l'empreinte de la variante `base` du banc égale à celle
du 4a, `55c3b065dc085f42` (§12.7).

### 12.2 Les tables de goût, au banc (`likeness/taste.rs`, nouveau)

Tout le calcul est dans le banc, en `f64` à partir de **sommes entières** (l'ordre des sommes n'a donc aucun effet) ;
seule la table finale, en entiers, entre dans le moteur.

**Paramètres** (`TasteParams`, arguments du §12.3) : `m` = 0,5 (lissage) ; `bound` = 3 (g dans [1/3, 3]) ;
`min_km` = 200 (seuil en km-passages, ou en km de traces pour `rc`) ; `cell_m` = 500 (trame) ; `corridor_cells` = 2
(dilatation).

**Membre d'une vue** (`ViewEdges`) : un bit par arête de base et par vue (`header().edges` = 23,5 M arêtes : 2,9 Mo par
vue), posé en parcourant toutes les chaînes de chaque fichier de vue (`view_chain(c)` pour `c` dans
`0..header().edges` du fichier de vue, `link & !DIR_BIT`). Les fichiers de vue sont rouverts par `MappedGraph::open_mmap`
(le moteur garde les siens privés). Construit une fois, lâché après les sommes.

**Trame** : case `(lat_e7.div_euclid(LAT), lon_e7.div_euclid(LON))`, `LAT = round(cell_m × 10⁷ / 111 320)` (44 916
pour 500 m), `LON = round(LAT / cos 46,5°)` (65 251 : une case de 540 m de large à 42° N, 457 m à 51° N) ; clé
`u64` = `(x as u32) << 32 | (y as u32)`, ensembles `FxHashSet`.

**Sommes d'une surcouche** (`class_sums(gf, &ViewEdges, &Overlay, &TasteParams) -> ClassSums`), pour chaque type
`t` ∈ {Road, Gravel, Mtb} de nom `ROAD`, `GRAVEL`, `MTB` dans `o.meta.types` (absent : sommes nulles ; `MIXED` ignoré,
l'export n'en a pas, §11.0 point 7), vue `vi(t)` = `RideType::view_index()` :

- `passages_t(e) = c[2i] + c[2i + 1]` (les deux sens), `i` = indice du type dans la surcouche ;
- **roulé** `ridden[t][classe(e)] += len_dm(e) × passages_t(e)` pour chaque arête de la surcouche avec
  `passages_t > 0` **et** dans la vue `vi(t)` (`u64`, dm × passages) ;
- **couloir** de `t` : les cases de la polyligne (`node_pos(a)`, `geometry(e)`, `node_pos(b)`) de chaque arête avec
  `passages_t > 0` (dans la vue ou non), densifiée à un point tous les `cell_m / 2` au plus sur chaque segment
  (interpolation linéaire en E7, longueur de segment équirectangulaire), puis dilatées de `corridor_cells` dans les deux
  axes (carré 5 × 5) : le « 1 km » du §3.2 vaut 1 à 1,5 km selon la position dans la case ;
- **disponible** `avail[t][classe(e)] += len_dm(e)` pour chaque arête de base `e` de la vue `vi(t)` dont la case de
  `node_pos(a)` **ou** de `node_pos(b)` est dans le couloir de `t` (une arête roulée est donc toujours disponible).

Un seul passage sur les 23,5 M arêtes de base par surcouche traite les trois types ; `rayon` par paquets de 2¹⁶ arêtes,
réduction par addition des tableaux (entiers : résultat indépendant du découpage). `ClassSums { ridden: [Vec<u64>; 3],
avail: [Vec<u64>; 3] }` (72 cases chacun ; `Vec` parce que `serde` ne sérialise pas les tableaux de plus de 32), avec
`add(&mut self, &Self)` pour la mise en commun.

**Table d'un type** (`ratio_table(num: &[f64], den: &[f64], num_km: f64, gamma: f64, p: &TasteParams) -> (Vec<u16>,
Vec<f64>)`, pure, rend `q8` et `g`) :
- si `num_km < min_km` ou `Σ den = 0` ou `Σ num = 0` : neutre (256 partout, g = 1) ;
- `p_n(c) = num(c) / Σ num`, `p_d(c) = den(c) / Σ den` ;
- `p_d(c) = 0` → g = 1 ; sinon `g(c) = (p_n(c) + m · p_d(c)) / ((1 + m) · p_d(c))`, borné à `[1 / bound, bound]` (une
  classe disponible jamais prise tend vers m / (1 + m) = 1/3) ;
- `q8(c) = round(256 × g(c)^(−γ))` borné à [1, 65 535].

**Trois sources de table** (et une `Taste` par source = les trois types ; une `Taste` entièrement neutre n'est **pas**
passée au moteur : `None`) :

| Source | `num` | `den` | `num_km` | Tables |
|---|---|---|---|---|
| **équipe** (`ge`) | `ridden` de la surcouche de l'équipe | `avail` de la même | Σ ridden / 10 000 | une par équipe et par surcouche jouée (complète, « écart ») |
| **générale** (`gg`) | Σ des `ridden` de toutes les équipes jouées | Σ des `avail` | idem | une pour les jeux sur surcouches complètes (sommes des complètes) ; une pour les jeux « écart » (chaque équipe y apporte sa surcouche « écart » si elle en a une, sa complète sinon) |
| **recalée** (`rc`) | longueurs **réelles** par classe des boucles retenues **non mises à l'écart** du type | longueurs par classe du **premier parcours de `base`**, mêmes boucles, surcouche complète, tous les curseurs joués, jeux `OK` | Σ num / 1 000 (km de traces) | une seule, pour les deux surcouches |

`rc` n'est pas un goût d'équipe : c'est le réglage de `K` par type qui viserait directement l'écart mesuré (le dépôt ne
pourrait pas le calculer, il demande des générations). Il borne ce qu'un multiplicateur par classe **peut** faire : si
`rc` ne divise pas les écarts par deux, aucun goût par classe ne le fera, et la cause est ailleurs (forme du chemin,
classes trop grossières) ; si `rc` y arrive et pas `ge` / `gg`, c'est l'estimateur « roulé contre disponible » qui
manque, pas le levier.

**Longueurs par classe d'une marche** (`measure::class_lengths(gf, &Walk, &Trace) -> [f64; TASTE_CLASSES]`) :
`Σ leg_m` par `taste_class(attrs(leg.edge))` (trous exclus) ; pour le réel en phase 7 (une fois par boucle), pour le
**premier** parcours gardé de chaque jeu (champ `#[serde(skip)] first_class_m: Option<Vec<f64>>` de `PlayRec`).

`measure::way_class` et `measure::surface_group` deviennent des appels à `ord_loops::taste_way` / `taste_surface` : une
seule définition, celle du moteur.

### 12.3 Variantes, ligne de commande, déroulé

**Un argument de plus à `likeness`** (pas de sous-commande nouvelle) :

| Argument | Défaut | Rôle |
|---|---|---|
| `--variants` | aucune | liste séparée par des virgules ; absente : le banc du 4a **à l'identique** (une passe `base`, sorties `<label>-report.json` et `<label>-summary.md`) |
| `--taste-m`, `--taste-bound`, `--taste-min-km` | `0.5`, `3`, `200` | §12.2 |
| `--taste-cell-m`, `--taste-corridor-cells` | `500`, `2` | §12.2 |

**Syntaxe d'une variante** : `base`, ou des composants joints par `+` : `fam:<f>` (familiarité, f ∈ [0, 1]),
`ge:<γ>` (goût d'équipe), `gg:<γ>` (goût général), `rc:<γ>` (goût recalé), γ ∈ ]0, 4]. Au plus un composant de goût et
un `fam`. Nom (fichiers, tableaux) : `fam` + centièmes sur trois chiffres, `ge` / `gg` / `rc` + dixièmes sur deux
chiffres, joints par `-` : `fam:0.75` → `fam075`, `ge:1+fam:1` → `ge10-fam100`. Refus (avant toute phase lourde) :
composant inconnu, nombre illisible ou hors bornes, deux goûts, nom en double, `rc` sans `base` **plus tôt** dans la
liste.

```rust
pub enum TasteSource { Team, General, Recal }
pub struct Variant { pub name: String, pub fam: Option<f32>, pub taste: Option<(TasteSource, f64 /* γ */)> }
pub fn parse_variants(s: &[String]) -> Result<Vec<Variant>>;
```

**Requête d'une variante** : celle du 4a (`play::request_of`), puis `fam` : en équipe d'abord, `familiarity = f` ; en
découverte (curseur 1), `mode = TeamFirst`, `off_team_cost_q8 = 256` (λ = 1) et `familiarity = f` (§12.0, points 2 et
6) ; le strict n'est pas touché. Le goût ne change pas la requête : il passe par `WindowTeam`.

**Campagne de la sonde** (variantes jouées dans l'ordre, une à la fois, dans le même processus) :

| Variante | Ce qu'elle mesure |
|---|---|
| `base` | le moteur d'aujourd'hui ; empreinte = celle du 4a |
| `fam:0.75`, `fam:1` | la familiarité seule, sans code moteur (`M[z]` pondère les arêtes d'équipe par leurs passages) |
| `ge:0.5`, `ge:1` | le goût **par équipe** (4b complet) |
| `gg:0.5`, `gg:1` | le goût **général** par type (un réglage de `K` appris) ; A pèse 85 % des km : il diffère de `ge` surtout pour B |
| `rc:1` | le réglage de `K` qui vise l'écart mesuré : la borne du levier « multiplicateur par classe » |

Puis, **seulement si la lecture le justifie**, une seconde campagne de combinaisons (`base` en tête, pour la comparaison
et le contrôle d'empreinte), par exemple `base,ge:1+fam:1,ge:2` : la syntaxe le permet sans code.

**Déroulé** (`likeness::run` ; phases 1 à 7 du §11.9 inchangées, sauf ajouts) :
- **7** : en plus, `real_class_m[i] = class_lengths(...)` de chaque boucle retenue (en mémoire, non sérialisé).
- **7 bis, « goût : sommes par classe »** (si une variante a `ge` ou `gg`) : `ViewEdges`, puis `class_sums` de chaque
  surcouche complète et de chaque surcouche « écart » ; `ViewEdges` lâché.
- **8, par variante, dans l'ordre de la liste** : si `rc`, ajuster la table sur les jeux de `base` (§12.2) ; puis équipe
  par équipe dans l'ordre des lettres, exactement le §11.9 (8) — `build_team`, jeux sur la surcouche complète, puis
  jeux des boucles mises à l'écart sur la surcouche « écart » — avec la `Taste` de la variante pour cette équipe et
  cette surcouche (`None` si neutre ou sans goût) et les requêtes adaptées par `fam`. `TeamEnv` gagne
  `taste: Option<&Taste>` ; `SharedWindows::acquire` et `acquire_wide` la reçoivent et appellent
  `WindowTeam::build_with_taste`. Clés d'équipe du cache `2t` / `2t + 1` comme au 4a, `drop_team` après chaque
  (équipe, surcouche) : aucun agrégat d'une variante n'est lu par la suivante. Les `Window` restent en cache d'une
  variante à l'autre. À la fin de la variante : ses sorties (§12.5) sont écrites, ses `LoopRec` gardés pour la
  comparaison (et pour `rc`).
- **9** : la comparaison (§12.5, §12.6).

Les `TeamData` et `TeamCheck` sont reconstruits à chaque variante comme au 4a (quelques secondes par équipe) plutôt que
gardés : la mémoire reste celle du 4a.

### 12.4 Mesures ajoutées à `Measures`

Pour la forme du chemin (relevé §4.3 et §12 : « l'écart de forme est d'abord un écart de chemin ») :
- **`detour_far: Option<f64>`** = `far_pos / r_over_d` (abscisse du point le plus éloigné sur sa distance à vol
  d'oiseau ; `None` si `r_over_d = 0`). Réel 1,41 à 1,72, généré 1,25 à 1,28 au 4a [calculé sur le JSON].
- **`sinuosity_10: Option<f64>`** = `l_poly / P`, `P` = périmètre du polygone **fermé** des points gardés par la règle
  de `vertices_10` (départ, sommets, arrivée, puis retour au départ) ; `None` si `P = 0`. Demande
  `shape::vertex_indices(pts, tol_share) -> Vec<usize>` (les indices gardés, triés) dont `vertices` devient
  `len − 2` : même règle, aucun compte ne change.

Ajoutées à `MEASURES` après `turns_per_10km` (les tableaux du résumé les prennent d'elles-mêmes). L'empreinte de résultat
ne lit aucune mesure : elle ne change pas.

### 12.5 Sorties

Sous `G/lot4/` seulement ; lettres et agrégats seulement, jamais un départ, une coordonnée, un id.

**Par variante `v`** : `<label>-<v>-report.json` et `<label>-<v>-summary.md`, le rapport et le résumé du 4a
(`out::Report`, `out::summary`), plus un champ `variant` :

```rust
struct VariantMeta {
    name: String, fam: Option<f32>, taste: Option<String> /* "ge" | "gg" | "rc" */, gamma: Option<f64>,
    params: TasteParams,
    /// Tables jouées : (lettre ou "général" ou "recalé", "full" | "holdout", type) -> q8 (72) et g (72).
    tables: Vec<TasteTable>,
    taste_xxh3: String,          // xxh3 de toutes les q8, dans l'ordre (lettre, surcouche, type, classe), petit-boutiste
    team_windows: TeamWindowCounters, // constructions de WindowTeam : nombre, ms totales, plus gros (octets)
    secs: f64, peak_anon_mb: f64,
}
```

`TeamWindowCounters` est tenu par `SharedWindows` (`AtomicU64` : constructions, nanosecondes, maximum de `bytes()`).
Le résumé de la variante commence par ces lignes, puis le résumé du 4a tel quel (empreinte de résultat de la variante en
section 12).

**Comparaison** : `<label>-compare.json` (les nombres des tableaux ci-dessous, et les `ClassSums` agrégées en parts :
par source et par type, `p_n`, `p_d`, `g`, `q8` des 72 classes) et `<label>-compare.md` :

1. Conditions : commit, variantes et paramètres, `taste_xxh3`, empreinte de résultat de chaque variante, et le
   contrôle **« `base` = 4a : `55c3b065dc085f42` »** (oui / non) quand le banc tourne aux défauts.
2. Les tables apprises, par type (ROAD, GRAVEL) et par source (A, B, générale, recalée ; surcouches complètes) : une
   ligne par **voie** (6), par **revêtement** (3) et pour **balisé** (agrégation des 72 classes : Σ `p_n`, Σ `p_d`, et
   le multiplicateur moyen `Σ p_d · q8 / 256 / Σ p_d`).
3. **Le critère** (§12.6), une table par lecture : lignes = (type, curseur), colonnes = `S` de `base` puis, par
   variante, `S` et `S / S_base` (en gras si ≤ 0,5).
4. Les écarts nommés du relevé : ROAD `way_major`, `way_cycle` ; GRAVEL `way_path`, `surf_unpaved`, `way_tertiary` :
   P50 réel, P50 de chaque variante, W1 / IQR de chaque variante, lecture « toutes ».
5. Par équipe, A et B séparément : le tableau 3 restreint à l'équipe (lectures « toutes » et « écart » ; B « écart »
   marqué indicatif, < 30 boucles par type).
6. Forme : `r_over_d`, `compactness`, `far_pos`, `detour_far`, `sinuosity_10`, `turns_per_10km`, `vertices_10` : P50
   réel, P50 et W1 / IQR par variante, par type et curseur, lecture « toutes ».
7. Service : par variante et curseur, part `OK` et issues par code, ms P50 / P90 (toutes D, et D ≥ 100 km), P50 de
   `team_share`, P10 / P90 de `length_over_target`, constructions de `WindowTeam` ; les **signaux** du §12.6.
8. Lecture, écrite comme des faits, sans décision : pour chaque variante, le nombre de groupes (2 types × 2 curseurs ×
   lectures « toutes » et « écart ») où `S / S_base ≤ 0,5`, et ses signaux de service.

### 12.6 Lectures et critères

**Trois lectures** (le goût est appris sur les traces mêmes qu'on mesure ; seule la deuxième en est exempte) :
- **« toutes »** : jeux sur la surcouche complète, toutes les boucles retenues (tables apprises sur la surcouche
  complète ; `rc` sur les 90 % non mis à l'écart, donc dans l'échantillon à 90 %) ;
- **« écart »**, la lecture honnête : les boucles mises à l'écart (286 : A 241, B 40, C 3, D 2), jouées sur la surcouche
  « écart » de leur équipe, avec les tables apprises **sans elles** (`ge` : surcouche « écart » ; `gg` : sommes
  « écart » ; `rc` : ajusté hors d'elles) ;
- **« écart, dans l'échantillon »** : les mêmes boucles, jeux sur la surcouche complète ; l'écart avec la précédente
  mesure la fuite de l'apprentissage.

**Groupes** : type ROAD et GRAVEL (le VTT, 4 boucles, reste dans les résumés par variante), curseur 0,25 et 1, toutes
équipes puis A et B. Premier parcours gardé, jeux `OK` (comme au 4a). **Normalisation fixe** : W1 divisée par l'IQR
du réel **de toutes les boucles retenues du type** (le dénominateur du 4a), la même pour toutes les lectures, équipes
et variantes (écrire `w1_over(real, gen, iqr)` ; `wasserstein1_normalised` prend l'IQR de son propre échantillon, ce
qui rendrait les petits groupes incomparables).

**Écart de classes d'un groupe** : `S = moyenne des W1 / IQR` sur
- ROAD : `way_major`, `way_tertiary`, `way_minor`, `way_street`, `way_cycle`, `surf_paved`, `surf_presumed`, `marked`
  (8 ; `way_path` et `surf_unpaved` exclus : IQR réel 0,008 et 0,002, §4.1 du relevé) ;
- GRAVEL : les mêmes plus `way_path` et `surf_unpaved` (10).

Au 4a, lecture « toutes » : **S = 0,614 (ROAD, 0,25), 0,722 (ROAD, 1), 0,735 (GRAVEL, 0,25), 0,680 (GRAVEL, 1)**
[calculé sur le §4.1 du relevé] ; la `base` de la sonde doit les redonner à un millième près.

**Critère du 4b** (décision 7) : `S(v) / S(base) ≤ 0,5` dans le même groupe et la même lecture ; il se lit d'abord sur
« écart » et « toutes », types × curseurs. **Signaux de service** (une variante ne doit pas dégrader le service) : part
`OK` en baisse de plus d'un point contre `base` au même curseur ; une `PANIC` ou un `WINDOW_ESCAPE` ; ms P50 au-delà de
1,25 × `base` ou P90 au-delà de 1,5 × `base` (indicatif : machine pleine). **Forme** : rapportée, sans seuil ; on
regarde surtout `detour_far` et `r_over_d` en GRAVEL (relevé §12 : « en GRAVEL, le goût est le levier de r / D »).

### 12.7 Temps, mémoire, ordre des campagnes

**[estimé]** sur le relevé du 4a (231 s, dont 210 s de jeux à 16 fils ; pic anonyme 14,2 Go) :
- phases communes : ~1 min comme au 4a, plus `ViewEdges` (~10 s, 6 Mo) et 13 passages de `class_sums` (9 surcouches
  complètes, 4 « écart » ; ~1 à 2 s chacun sur 16 fils) ;
- par variante : 3,5 à 4 min de jeux (`base`, `fam`), 4 à 6 min avec un goût (constructions de `WindowTeam` sur tous
  les liens, et des arbres qui peuvent s'étendre autrement) ; `build_team` refait par variante : ~10 à 20 s ;
- **campagne de 8 variantes : 35 à 50 min** ;
- mémoire : celle du 4a (cache de fenêtres sous `--budget-mb` 6 000, partagé entre variantes), plus `w_taste` (4 octets
  par chaîne et par type, une équipe à la fois) et les `LoopRec` gardés (~0,1 à 0,15 Go par variante) : **pic ~15 à
  16 Go**, sous les 30 Go. Une seule campagne à la fois, jamais deux processus.

**Ordre**, binaire `--release` (`cargo build --release -p ord-loops-bench`, `B=./target/release/ord-loops-bench`) :
1. Essai court : `$B likeness --teams C --limit 5 --variants base,fam:1,ge:1,gg:1,rc:1 --label sonde-essai` (tables
   non neutres pour C en ROAD, sorties et comparaison lisibles).
2. Déterminisme : `$B likeness --teams B --variants base,ge:1,gg:1,rc:1 --threads 1 --label sonde-det1`, puis
   `--threads 16 --label sonde-det16` : mêmes empreintes de résultat **et** mêmes `taste_xxh3`, variante par variante.
3. Campagne : `$B likeness --label sonde --variants base,fam:0.75,fam:1,ge:0.5,ge:1,gg:0.5,gg:1,rc:1`. **Arrêt** si
   l'empreinte de `base` n'est pas `55c3b065dc085f42` (le crochet ne serait pas neutre, ou le banc aurait changé) :
   chercher, ne pas lire la suite.
4. Seulement si la lecture le justifie : une seconde campagne de combinaisons (§12.3), `--label sonde2`.
5. Relevé (§12.9).

### 12.8 Tests (synthétiques, aucune donnée de production)

**`ord-loops`** :
1. `taste_class` : 72 valeurs distinctes atteintes par une grille d'attributs synthétiques (`Attrs` composés comme dans
   les tests existants), toujours < 72 ; cas : `primary` revêtu non balisé → (0, 0, 0, 0) ; `cycleway` → voie 4 ;
   `path` + `bicycle=designated` → voie 4 (`is_greenway`) ; `path` sans désignation → 5 ; `track` sans `surface` →
   (5, 1) ; `access=destination` → bit destination ; `_link` suit sa classe.
2. `taste_weight` avec 256 partout **égal** à `edge_weight` pour les trois types, toutes les classes de voie, avec et
   sans `surface`, accès `yes` / `destination`, et des longueurs 1, 7, 999, 10⁶ dm et proche du plafond ; avec 512,
   égal à `⌈2 × len × K / 64⌉` ; avec 85, jamais nul pour une longueur non nulle.
3. `WindowTeam::build_with_taste(Some(&Taste::neutral()))` contre `build` sur la grille `grid(5, 300.0)` et la surcouche
   du test `weights_are_monotone_in_lambda_…` : mêmes `ChainTeam` (a, b, z, strict) chaîne par chaîne, `w_taste` égal
   à `win.v.w_osm` pour chaque type servi.
4. **Neutralité de bout en bout** : `assert_matches_reference` avec un exécutant qui construit la fenêtre entière
   (`full_window`) et `WindowTeam::build_with_taste(e, &win, t, Some(&Taste::neutral()))` : la fixture
   `window_reference.txt` passe **sans** re-bénédiction (les trois modes y sont, la découverte avec équipe comprise).
5. Effet : une `Taste` où la classe de la rue roulée vaut 512 → `Weighter::chain` de cette chaîne double (à l'arrondi
   près, valeur exacte calculée dans le test) **en découverte** (où l'équipe est ignorée sans goût) comme en équipe
   d'abord à f = 0,5 ; la chaîne hors équipe d'une autre classe ne bouge pas ; sans goût, `Discovery` rend toujours
   `w_osm`.
6. Les tests existants, dont `multiplier_table_is_exact_and_golden`, la référence plate et les tests de fenêtres,
   **inchangés**.

**`ord-loops-bench`** (`taste.rs`, `measure.rs`, `shape.rs`, `mod.rs`) :
7. `ratio_table` : parts égales → 256 partout ; une classe disponible jamais roulée → g = 1/3, `q8` = 768 à γ = 1,
   443 à γ = 0,5 ; une classe roulée deux fois sa part (valeur attendue calculée dans le test) ; borne 3 → `q8` = 85 à
   γ = 1 ; `p_d = 0` → 256 ; `num_km` sous le seuil → 256 partout.
8. `class_sums` sur un petit `MemSource` (comme `likeness/fixture.rs`) : une grille de `residential`, une `cycleway`,
   une `primary` à 3 km ; surcouche roulant deux rues (passages 1 et 3) → `ridden` = Σ len × passages par classe
   exactement ; la `primary` lointaine absente de `avail`, une rue à 300 m présente ; une arête hors vue jamais comptée ;
   même résultat avec `rayon` sur 1 fil et sur 4.
9. Mise en commun : `add` de deux `ClassSums` = sommes case à case ; la table générale de deux équipes égales = la
   table de l'une.
10. `parse_variants` : noms (`fam075`, `ge05`, `ge10-fam100`) ; refus listés au §12.3.
11. Requête de `fam` : curseur 0,25 → `TeamFirst`, q8 640, f ; curseur 1 → `TeamFirst`, q8 256, f ; curseur 0 inchangé.
12. `detour_far` et `sinuosity_10` : aller-retour → 1 et 1 ; cercle → π / 2 et 2π / 4√2 ≈ 1,111 (carré inscrit : `vertices_10` = 3) ; triangle du §11.11 → 1 et 1 ;
    `vertex_indices(...).len() − 2 == vertices(...)` sur les formes du §11.11.
13. Comparaison : `S` et `S / S_base` sur des échantillons synthétiques (écart divisé par deux → 0,5 exactement à un
    décalage moitié) ; `w1_over` avec un IQR fixe.

### 12.9 Commits

Chacun passe `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` et `cargo test -p ord-loops
-p ord-loops-bench` (`ord-loops` en entier à chaque commit qui le touche).

1. `feat(loops): goût par classe, crochet optionnel et neutre par défaut (sonde du lot 4)` — `cost.rs`, `team.rs`,
   `search.rs`, `lib.rs`, tests 1 à 6.
2. `feat(bench): likeness, détour jusqu'au point lointain et sinuosité` — §12.4, `way_class` / `surface_group` vers
   le moteur, `class_lengths`, test 12.
3. `feat(bench): likeness, tables de goût par équipe, générale et recalée` — `taste.rs`, tests 7 à 9.
4. `feat(bench): likeness --variants, la sonde du goût` — variantes, déroulé du §12.3, sorties par variante, tests 10
   et 11.
5. `feat(bench): likeness, comparaison des variantes` — `<label>-compare.{md,json}`, test 13.
6. Campagnes (§12.7), puis `docs(ord-loops): relevé de la sonde du goût (lot 4)` : nouveau relevé
   `docs/design/technique/ord-loops-lot4-sonde.md` (style de `ord-loops-lot4-mesures.md`, marques de l'en-tête :
   conditions et commandes, tables apprises, critère par lecture, par équipe, forme, service, ce que la sonde décide
   pour le 4b selon la décision 7), et un §12.11 « Écarts du codage » ajouté ici. Équipes lettrées et agrégats
   seulement.

### 12.10 Ce que la sonde ne fait pas

Rien au dépôt ni au service (`ord-server`, contrat, version : inchangés) ; rien au contrôleur ; pas de stockage de
profil ; pas de formes ni de destinations (4c, 4d) ; pas de nouveau réglage de `K` dans le moteur (si `rc` gagne, le
réglage se décidera après le relevé) ; pas de pente ni de trafic (décision 2 : à revoir après la sonde, au relevé).

### 12.11 Écarts du codage

Écrit après le codage des commits 1 à 5 du §12.9 (les campagnes du §12.7 et le relevé restent à faire). Ce que le code
fait autrement que le §12, ou en plus :

1. **`WindowTeam::taste_of` rend `None` si la vue ne sert pas le type** (`w_taste` vide), même quand un goût a été donné :
   un `Weighter` du type `Mtb` sur la vue `road` n'existe pas, mais le crochet ne lit jamais un `w_taste` absent.
2. **Une `Taste` entièrement neutre n'est jamais passée au moteur** (`learn` rend `None`), comme le §12.2 le demande ; les
   tests passent, eux, `Some(&Taste::neutral())` pour exercer l'arithmétique (§12.8, points 3 et 4).
3. **`ViewEdges::build(views, n_base_edges)`** reçoit les deux vues déjà ouvertes (la phase 7 bis les rouvre par
   `open_graph`, mmap seul, sans copie) ; les bits sont posés en parallèle (`AtomicU64::fetch_or`).
4. **`rc` : km de traces** = Σ des mètres du réel sur les paires (boucle, curseur) dont le jeu de `base` est `OK`, donc
   chaque boucle y compte une fois par curseur ; le seuil `min_km` de 200 s'y lit sur cette somme. La table porte la
   source `recalé` et l'étiquette de surcouche `full`, et sert aussi aux jeux « écart » (une seule table, §12.2).
5. **`PlayRec::first_class_m`** est rempli pour le premier parcours gardé de **tout** jeu (pas seulement `base` sur la
   surcouche complète) : le coût est négligeable (72 flottants) et `rc` filtre par surcouche et issue.
6. **Les compteurs de fenêtres du rapport d'une variante sont des écarts** (`windows` : fenêtres construites, servies du
   cache, reprises) et `team_windows` (constructions de `WindowTeam`, ms, plus gros `bytes()`) est remis à zéro entre
   variantes (`SharedWindows::take_team_counters`) ; les phases d'une variante listent toutes les phases déjà faites.
7. **L'arrêt « `base` ≠ 4a »** n'a lieu que si `--variants` est donné et que le banc tourne aux réglages du 4a
   (`LikenessArgs::at_4a_defaults` : ni `--limit`, ni `--max-km`, ni `--teams`, curseurs `0,25,1`, réglages de retenue et
   graine par défaut) ; sans `--variants`, le banc imprime le contrôle et ne s'arrête pas. Il écrit d'abord les sorties de
   `base`, puis s'arrête avant la variante suivante. Le contrôle apparaît aussi dans `<label>-compare.md`.
8. **`detour_far` et `sinuosity_10` sont calculées dans `shape::shape`** (le périmètre `P` d'un polygone dégénéré, sans
   aucun sommet gardé, est 2 × la distance départ-arrivée) ; `shape::vertices` appelle `vertex_indices` (`len − 2`,
   saturé).
9. **La comparaison n'affiche pas la colonne de `base` dans le tableau du critère** (`S` et `S / S_base` y valent
   trivialement `S` et 1) ; le JSON la garde. Les tableaux « par équipe » ne portent que A et B, lectures « toutes » et
   « écart » (la lecture « écart, dans l'échantillon » n'existe que pour toutes les équipes) ; les groupes de moins de
   30 boucles y sont marqués indicatifs.
10. **Les tables apprises du tableau 2** sont celles des surcouches complètes ; les multiplicateurs moyens se lisent par
    variante, les parts (`Σ p_n`, `Σ p_d`, `g`) ne dépendent pas de γ.
11. **Le déterminisme de l'essai (§12.7, point 2)** a été vérifié à petite échelle (équipe C, 3 boucles, quatre variantes,
    1 contre 16 fils : mêmes empreintes de résultat et mêmes `taste_xxh3`). La neutralité à grande échelle (`base` =
    `55c3b065dc085f42`) est **vérifiée par la campagne elle-même** (arrêt automatique sinon) ; l'essai de fumée ne la
    couvre pas, il tourne sur une équipe et quelques boucles.
12. **À la relecture** (même jour, binaire `release` de la branche) : la neutralité a été vérifiée sur toute l'équipe B
    (397 boucles, jeux « écart » compris) : `base` redonne `5f04c040f3d10bf4`, l'empreinte de B au 4a (relevé, commandes de l'en-tête), et le
    binaire de `develop` donne la même ; sur C (5 boucles), `develop` et la branche donnent aussi la même empreinte. Le
    point 2 du §12.7 a été joué tel quel sur B (`base,ge:1,gg:1,rc:1`, 1 fil contre 16) : mêmes empreintes de résultat
    et mêmes `taste_xxh3`, variante par variante. `taste_xxh3` ne lit que les `q8` : `ge` et `gg` ont la même sur une
    seule équipe qui a des boucles mises à l'écart, et diffèrent sans elles (`ge` n'a alors que ses trois tables
    `full`, `gg` en a six). Les `q8` d'une table ne sont pas normalisées (`Σ p_d · q8 / 256` n'est pas 1 : sur B en
    GRAVEL, la plupart des classes passent au-dessus de 1) : le goût change aussi l'échelle des poids contre le coût
    absolu des virages (§12.0, point 5), à lire au relevé.

---

## 13. Seconde sonde du goût : spécification

Écrit le 17 novembre 2026, après le relevé de la première sonde ([`ord-loops-lot4-sonde.md`](ord-loops-lot4-sonde.md)) et
la décision 2 du §9.2, à la lecture du code de la première sonde (`ord-loops` : `cost.rs`, `team.rs`, `search.rs` ; banc :
`likeness/{mod,taste,variants,compare,play}.rs`) et des journaux privés de ses campagnes. Cette section **suffit au
codeur** : ce qu'elle ne dit pas est laissé à son jugement, à condition de tenir le §13.12. Les sections précédentes ne
sont pas réécrites.

**Questions posées** (décision 2 du §9.2), au banc seulement :
1. Un **recalé itéré** (2 à 3 pas, chaque pas appris contre les parcours du pas précédent) fait-il passer le GRAVEL sous
   `S / S_base ≤ 0,5`, et à quel pas ?
2. Un **recalé par équipe** (longueurs de l'équipe seule, repli vers la table commune sous un seuil) garde-t-il B juste
   en ROAD et fait-il au moins aussi bien que la table commune pour A ? Sinon : table générale par type.
3. Que coûterait un profil par équipe **appris au dépôt** (il faut regénérer ses boucles à chaque pas) ?
4. Le service tient-il : ms P50 dans +25 % de `base`, part `OK` à moins d'un point ?

Chaque question se lit en **« toutes »** et en **« écart »** (la lecture honnête : table apprise sans les boucles mises
à l'écart, mesurée sur elles), §13.4.

**Rien au moteur** : le crochet du §12.1 suffit (une `Taste` commune ou par équipe passe déjà par
`WindowTeam::build_with_taste`). `crates/ord-loops`, `crates/ord-server`, le contrôleur et le contrat ne changent pas ;
l'empreinte de `base` reste `55c3b065dc085f42` (arrêt automatique déjà en place, §12.11 point 7).

### 13.0 Ce que la lecture a établi

1. **Le recalé du §12 est un pas unique.** `RcInputs::of` (`likeness/mod.rs`) somme, par type, les mètres par classe du
   réel et du premier parcours de `base` (boucles non mises à l'écart, surcouche complète, jeux `OK`, chaque boucle une
   fois par curseur `OK`) ; `taste::learn` → `ratio_table` → `q8 = round(256 · g^−γ)`. Le lissage `m = 0,5` fait de `g` un
   rapport **déjà amorti** : `g = (r + m) / (1 + m)` avec `r = p_n / p_d`. Le pas effectif sur le logarithme du poids est
   `−γ · ln((r + 0,5) / 1,5)`, soit 0,5 à 0,75 fois `−γ · ln r` selon `r`.
2. `PlayRec::first_class_m` est rempli pour le premier parcours de **tout** jeu (§12.11 point 5) : apprendre sur
   n'importe quelle variante jouée ne demande rien à `play.rs` ni au moteur. Les `LoopRec` d'une variante restent en
   mémoire jusqu'à la comparaison (`compare::Done`), `first_class_m` compris (seulement `#[serde(skip)]`).
3. Une `Taste` par équipe passe déjà au moteur (`ge`), une `Taste` commune aussi (`gg`, `rc`) : le recalé par équipe n'est
   qu'une autre source de tables, la clé de cache d'équipe (`2t`, `2t + 1`) et `drop_team` ne changent pas.
4. **Gains du premier pas** [calculé sur les P50 du §4.3 du relevé] : `gain = 1 − ln(réel / rc05) / ln(réel / base)` ;
   1 = le pas atteint la cible, > 1 la dépasse ; l'erreur (en logarithme) est multipliée par `1 − gain` à chaque pas si la
   réponse reste la même.

   | classe | réel | base | rc05 | gain à γ = 0,5 |
   |---|---:|---:|---:|---:|
   | ROAD grande route | 0,244 | 0,104 | 0,411 | **1,61** |
   | ROAD petite route | 0,140 | 0,265 | 0,118 | 1,27 |
   | ROAD balisé | 0,163 | 0,249 | 0,174 | 0,85 |
   | ROAD voie cyclable | 0,033 | 0,157 | 0,057 | 0,65 |
   | GRAVEL tertiaire | 0,085 | 0,225 | 0,103 | 0,80 |
   | GRAVEL non revêtu | 0,253 | 0,099 | 0,166 | 0,55 |
   | GRAVEL chemin | 0,447 | 0,166 | 0,276 | 0,51 |
   | GRAVEL petite route | 0,097 | 0,188 | 0,134 | 0,51 |
   | GRAVEL revêtu | 0,400 | 0,570 | 0,502 | 0,36 |

5. **Conséquences pour le pas** [calculé, en supposant la réponse constante d'un pas à l'autre et les classes
   indépendantes, ce qui n'est vrai qu'en première approximation] :
   - à γ = 0,5, `|1 − gain| ≤ 0,64` partout : l'itération **converge** ; trois pas laissent 4 % de l'erreur initiale aux
     cyclables ROAD, 13 % aux chemins GRAVEL, 26 % au revêtu GRAVEL (le plus lent), et 23 % aux grandes routes ROAD, qui
     oscillent (gain 1,61 : signe alterné) ;
   - γ = 1 double les gains : grandes routes ROAD 3,2, `|1 − 3,2| > 1`, **divergence** — c'est ce que `rc10` montre
     (0,585 contre 0,244) ;
   - γ = 0,25 laisserait 40 % de l'erreur des chemins GRAVEL après trois pas (gain 0,26) : le GRAVEL ne passerait pas ;
   - **un amortissement par classe** quand le signe de la correction s'inverse (le pas est divisé par deux) ramène les
     grandes routes ROAD à un gain de 0,8 dès le deuxième pas : 2 % de l'erreur initiale après trois pas au lieu de 23 %,
     sans ralentir les classes qui convergent par en dessous. Retenu (§13.2).
6. **`S` ne voit que des marges** : voie (6), revêtement (3), balisé ; jamais le bit destination, jamais un croisement
   (« chemin balisé » contre « chemin » et « balisé »). Sur 72 cases par type, les cases rares (grande route non
   revêtue, destination…) ont des rapports bruités, que la borne `[1/3, 3]` arrête à chaque pas mais que trois pas
   cumuleraient jusqu'à 3^1,5 ≈ 5,2. Une case pas du tout générée au pas précédent (`p_d = 0`) reste neutre à jamais : la
   table à 72 cases ne peut pas y amener un parcours. D'où une seconde forme de table, en **effets principaux** (§13.2.3),
   jouée à côté de la table à 72 cases.
7. **Mise à l'écart** : `holdout::select` prend, équipe par équipe, les premières boucles par rang d'empreinte d'id de
   trace : l'écart à 25 % **contient** celui à 10 %. À 10 % : A 241, B 40 (14 ROAD, 26 GRAVEL : lecture honnête
   indicative), C 3, D 2 ; à 25 % [calculé] : A 602, B 100 (~35 ROAD, ~65 GRAVEL), C 7, D 3, soit 712, et 7 134 jeux par
   variante au lieu de 6 282 (+14 %). La lecture « toutes » (surcouche complète, toutes les boucles) ne dépend pas de la
   part mise à l'écart : `S_base` « toutes » reste 0,614 · 0,721 · 0,735 · 0,680 quelle qu'elle soit.
8. **Temps par équipe** (journaux privés de la première sonde, 16 fils sur 8 cœurs, machine pleine) : `base`, A 2 406
   boucles en 162 s, B 397 boucles en 11 s, constructions d'équipe et jeux « écart » compris ; soit ~0,49 s-fil par appel
   pour A et ~0,20 pour B [calculé]. `PlayRec::ms` couvre l'acquisition des fenêtres et l'appel du moteur.

### 13.1 Choix proposés au propriétaire

Les défauts de cette spécification sont les propositions ci-dessous ; une réponse différente ne change qu'un défaut ou
la liste des variantes du §13.8, pas le code.

| # | Question | Ce qui est en jeu | Proposé |
|---|---|---|---|
| 1 | **Table à 72 cases ou en effets principaux ?** Joue-t-on les deux en campagne 1 ? | `S` ne mesure que voie, revêtement, balisé (§13.0 point 6). Les effets principaux (11 paramètres par type) visent exactement ce que `S` mesure, sans artefact de case rare, et touchent aussi les cases jamais générées ; ils ne peuvent pas apprendre un croisement. Coût : 3 variantes de plus, ~13 min. | Les deux ; la campagne 2 prend la forme gagnante par la règle du §13.8. |
| 2 | **Part mise à l'écart de la campagne 2 : 10 % ou 25 % ?** | À 10 %, la lecture honnête de B tient sur 14 boucles ROAD et 26 GRAVEL ; à 25 %, ~35 et ~65. Les tables sont alors apprises sur 75 % des boucles (A ~1 800, B ~300). Les contrôles d'empreinte du 4a ne s'appliquent plus (ils sont faits en campagne 1) ; « toutes » ne change pas ; +14 % de jeux. | 25 %. |
| 3 | **Seuil de repli d'une équipe vers la table commune, `n0`** | Poids de l'équipe ≈ N / (N + n0), N = ses boucles d'apprentissage du type. À n0 = 25 : B ROAD ~0,81, B GRAVEL ~0,89, A 0,94 à 0,98, C ≤ 0,44, D ≤ 0,24 [calculé, 25 % à l'écart, répartition par type estimée sur la lecture « écart » de la première sonde]. À 50 (le seuil des formes, décision 6 du §9.1) : B ROAD 0,68, B garderait un tiers de la correction de A qui l'éloigne (`rc05` double son `S` ROAD). L'écart-type d'un log-rapport de classe est de l'ordre de 1 / √N : 0,2 à N = 25. | n0 = 25 boucles. |
| 4 | **Familiarité dans les chaînes : ROAD au curseur 0,25 seulement ?** | `fam:1` de la première sonde touchait aussi le GRAVEL (il le gâte : 0,61 à 0,65 contre 0,54 à 0,57) et le curseur 1 (il y supprime la découverte). Une table apprise sur les jeux d'une chaîne apprend contre le générateur de la chaîne : avec une familiarité partout, elle apprendrait contre un service qui n'existera pas. | `famroad:1` (1 en ROAD en équipe d'abord, 0,5 ailleurs : la configuration visée par la décision 3 du §9.2). |
| 5 | **Budget du dépôt de nuit pour un profil par équipe** | Trois pas = trois générations de toutes les boucles de l'équipe aux deux curseurs [estimé, §13.0 point 8] : A ~2 h-fil (~30 min sur 4 fils, ~7 min sur 16), B ~8 min-fil (~2 min sur 4 fils). Allègements possibles, non mesurés : un seul curseur (÷ 2), au plus 500 boucles par équipe (÷ 5 pour A), recalcul hebdomadaire. | « Tenable » = au plus 30 min sur 4 fils pour la plus grosse équipe ; le banc donne les temps mesurés (§13.7). |
| 6 | **Normaliser les tables** (échelle des poids contre le coût absolu des virages, §12.0 point 5) ? | Normaliser casserait le contrôle « pas 1 = `rc05` » (§13.8) et mêlerait deux effets dans la même sonde. Le multiplicateur moyen est rapporté à chaque pas. | Non ; rapporté ; décidé au 3b. |
| 7 | **Nombre de pas** | 3 suffit au calcul du §13.0 point 5 ; un 4ᵉ pas coûte ~4,5 min par chaîne. La syntaxe admet jusqu'à 5. | 3. |

### 13.2 Le pas itéré (`likeness/chain.rs`, nouveau)

Tout en `f64`, dans un ordre de sommes fixé (séquentiel, boucles dans l'ordre de `loops`), sans `rayon` : quelques
milliers de boucles × 72 cases, négligeable. Seules les `q8` entières entrent dans le moteur.

#### 13.2.1 Entrées d'un pas : `ClassMeters`

```rust
/// Mètres par classe d'apprentissage, par type (`RideType::index()`, `usize::from(l.ty).min(2)` comme `RcInputs`).
pub struct Meters {
    pub real: [Vec<f64>; 3],   // 72 chacun
    pub gen: [Vec<f64>; 3],
    /// Boucles comptées (au moins une paire comptée) et paires (boucle, curseur) comptées.
    pub loops: [u32; 3],
    pub pairs: [u32; 3],
}
pub struct ClassMeters {
    /// Toutes équipes : calculé **exactement** comme `RcInputs::of` du §12 (mêmes conditions, même ordre d'addition :
    /// boucles dans l'ordre de `loops`, curseurs dans l'ordre des jeux, `num[ty][c] += real_class_m[i][c]` puis
    /// `den[ty][c] += fc[c]`), pour que le pas 1 redonne `rc05` au bit près.
    pub pooled: Meters,
    /// Par équipe, indice `t` de `loops[i].team`, même passe.
    pub team: Vec<Meters>,
}
impl ClassMeters {
    pub fn of(loops: &[export::Loop], held: &HashSet<usize>, real_class_m: &[[f64; 72]], plays: &[Vec<PlayRec>],
              n_teams: usize) -> Self;
}
```

Une paire compte si la boucle n'est **pas** mise à l'écart, le jeu est sur la surcouche `full`, d'issue `OK`, avec
`first_class_m`. `RcInputs` disparaît : `rc` passe par les chaînes (§13.5). `ClassMeters::of` est calculé sur les jeux
d'une variante **avant** qu'ils ne partent dans les `LoopRec`.

#### 13.2.2 Table à 72 cases (`dim:72`, défaut)

État d'un propriétaire de table (la table commune, ou une équipe), par type :

```rust
pub struct ParamState {
    /// Multiplicateur cumulé du poids, par classe (72) ; 1 au départ.
    pub mult: Vec<f64>,
    /// γ courant par paramètre (72 ici, 11 en effets principaux) ; γ de la chaîne au départ.
    pub gamma: Vec<f64>,
    /// Dernier signe non nul de la correction, par paramètre : −1, 0, +1.
    pub sign: Vec<i8>,
}
```

Un pas, pour un type, depuis `num = real`, `den = gen` :
1. `g = ratio_table(num, den, Σ num / 1 000, γ, p).1` — la fonction du §12.2 telle quelle (lissage `m`, borne
   `[1/bound, bound]` par pas, neutre sous `min_km`, `g = 1` si `p_d = 0`) ; ses `q8` sont ignorées.
2. Pour chaque classe `c`, `u = ln g[c]` ; `s = 0` si `|u| < 0,02`, sinon le signe de `u`. Si `s ≠ 0`, `sign[c] ≠ 0` et
   `s ≠ sign[c]` : `gamma[c] /= 2`. Si `s ≠ 0` : `sign[c] = s`.
3. `mult[c] = (mult[c] * g[c].powf(-gamma[c])).clamp(1.0 / B, B)`, `B = --taste-mult-bound` (défaut 4). Écrire
   l'expression **ainsi** : au pas 1, `mult = 1.0` et `gamma = γ` rendent `1.0 * g.powf(-γ)`, les bits de `ratio_table` ;
   la borne cumulée ne touche pas un premier pas (`g^−0,5 ∈ [0,58 ; 1,73]`).
4. `q8[c] = (256.0 * mult[c]).round().clamp(1.0, 65_535.0) as u16`.

Point fixe : réel = généré ⇒ `g = 1`, `u = 0`, rien ne bouge.

#### 13.2.3 Effets principaux (`dim:me`)

Onze paramètres par type : voie `a[w]` (6, `w = c / 12`), revêtement `b[s]` (3, `s = (c / 4) % 3`), balisé `β[k]`
(2, `k = (c / 2) % 2`) ; le bit destination est ignoré (les deux cases qui ne diffèrent que par lui ont toujours le même
multiplicateur). Mêmes axes que `compare::class_rows`.

```rust
pub struct MainEffects { pub a: [f64; 6], pub b: [f64; 3], pub beta: [f64; 2] }
/// Moindres carrés pondérés de `ln g` sur `a[w] + b[s] + β[k]`, par relaxation (Gauss-Seidel).
pub fn fit_main_effects(ln_g: &[f64], w: &[f64], bound: f64) -> MainEffects;
```

- Cible `ℓ[c] = ln g[c]` (le `g` de l'étape 1 du §13.2.2), poids `ω[c] = p_n[c] + p_d[c]` ; `ω[c] = 0` si `p_d[c] = 0`.
- Départ à 0 ; au plus 200 balayages, arrêt quand la plus grande variation est < 1e-12 ; un balayage met à jour dans
  l'ordre `a[0..6]`, `b[0..3]`, `β[0..2]` : `a[w] = Σ_{c ∈ w} ω (ℓ − b[s(c)] − β[k(c)]) / Σ_{c ∈ w} ω` (0 si la somme des
  poids est nulle), de même pour `b` et `β`.
- Jauge : `b̄ = Σ_s Ω_s b[s] / Σ_s Ω_s` (`Ω_s` = poids de la marge), `b[s] −= b̄`, `a[w] += b̄` pour tout `w` ; de même
  pour `β` ; si `Σ Ω = 0`, rien.
- Chaque coefficient est borné à `[−ln bound, ln bound]`.

Un pas : `u` = le coefficient de chaque paramètre ; signe et γ comme au §13.2.2 (étape 2), **par paramètre** (11) ;
puis pour chaque classe `mult[c] = (mult[c] * (−(γ_a[w]·a[w] + γ_b[s]·b[s] + γ_β[k]·β[k])).exp()).clamp(1/B, B)` et
`q8` comme au §13.2.2. Le pas 1 de `me` ne redonne **pas** `rc05` (autre table).

#### 13.2.4 Recalé par équipe (`rt`)

Un propriétaire par équipe jouée (`order`), plus la table commune comme cible de repli. Pour une équipe `t` et un type,
avec `T` = `team[t]`, `P` = `pooled` des **mêmes** jeux (ceux du pas précédent de la chaîne `rt`, où chaque équipe a
joué sa propre table) :
- `g_T = ratio_table(T.real, T.gen, Σ T.real / 1 000, γ, p')`, `p'` = `p` avec `min_km = 0` (le repli tient lieu de
  seuil) ; `g_P = ratio_table(P.real, P.gen, Σ P.real / 1 000, γ, p)` ;
- poids de l'équipe, par classe : `w[c] = G_T[c] / (G_T[c] + n0 · G_P[c] / N_P)`, `G` = mètres générés, `N_P` =
  `P.loops[type]`, `n0 = --taste-team-n0` (défaut 25) ; `w[c] = 0` si le dénominateur est nul. Si l'équipe génère la
  classe au même rythme par boucle que l'ensemble, `w = N_T / (N_T + n0)` : la forme « n / (n + n0) » demandée, rendue
  adaptative par classe (une classe que l'équipe génère peu suit la table commune) ;
- `ln ĝ[c] = w[c] · ln g_T[c] + (1 − w[c]) · ln g_P[c]` ; puis signe, γ, `mult` et `q8` comme au §13.2.2 avec `ĝ`, sur
  l'état de l'équipe.

En effets principaux (`rt` + `dim:me`) : `fit_main_effects` sur l'équipe (poids `ω` de l'équipe) et sur l'ensemble ;
mélange **par paramètre**, `w` = même formule avec les mètres générés de la marge (`G_T,niveau`, `G_P,niveau`) ; puis le pas
du §13.2.3 sur l'état de l'équipe.

Au pas 1 (tout le monde à 1), `g_P` est le `g` de `rc05` : une équipe sans donnée d'un type (`w = 0`) reçoit **exactement**
les `q8` de `rc05` pour ce type. Aux pas suivants, `g_P` est l'erreur moyenne restante des tables d'équipe : une équipe
sans donnée suit la chaîne commune. La table commune de `rt` n'est **pas jouée** : elle ne sert que de cible.

#### 13.2.5 Diagnostics d'un pas

Par propriétaire (`général`, `commun` pour la cible de `rt`, ou la lettre) et par type, dans `ChainOwnerStep` :
`loops`, `pairs`, `weight` (`rt` : `N_T / (N_T + n0)`), `l1_gap = Σ_c |p_n[c] − p_d[c]|` des entrées du pas (l'écart que
le pas corrige), `mean_mult = Σ_c p_d[c] · mult[c]` (dérive d'échelle, question 6), `halved` (paramètres dont γ a été
divisé à ce pas), `gamma_min`, `at_bound` (classes à `1/B` ou `B`).

### 13.3 L'état d'une chaîne

```rust
pub enum Dims { D72, Me }
pub enum Owner { General, Team(usize) }
pub struct ChainState { pub dims: Dims, pub gamma0: f64, pub owners: BTreeMap<Owner, [ParamState; 3]> }
pub struct StepOut {
    pub general: Option<Taste>,                  // rc
    pub team: BTreeMap<usize, Option<Taste>>,    // rt
    pub tables: Vec<TasteTable>,
    pub diag: Vec<ChainOwnerStep>,
}
impl ChainState {
    pub fn new(dims: Dims, gamma0: f64) -> Self;
    /// Un pas : `rc` met à jour `General` depuis `m.pooled` ; `rt` met à jour chaque équipe de `order` (et rend aussi
    /// la cible `commun` dans `diag`, jamais dans `tables`).
    pub fn step(&mut self, src: TasteSource, m: &ClassMeters, order: &[usize], letter_of: &[String],
                p: &TasteParams) -> StepOut;
}
```

`TasteParams` gagne `team_n0: f64` (25) et `mult_bound: f64` (4), arguments `--taste-team-n0` et `--taste-mult-bound`.
Une `Taste` entièrement neutre n'est jamais passée au moteur (`None`, §12.11 point 2).

**Tables sorties** (`VariantMeta::tables`, donc `taste_xxh3`) : `rc` en 72 cases → source `recalé` ; `rc` en effets
principaux → `recalé-me` ; `rt` → la lettre de chaque équipe jouée ; surcouche `full` ; `pn` = parts réelles, `pd` =
parts générées des entrées du pas, `g` = le `g` (ou `ĝ`, ou `exp(a + b + β)`) du pas, `q8` = **cumulées**. Une table par
type (trois). Ainsi `rc:0.5` seul redonne la même liste de tables que la première sonde, et la même `taste_xxh3`.

### 13.4 Lecture honnête

- **Apprentissage** : seulement les boucles **non** mises à l'écart, jouées sur la surcouche complète (§13.2.1), à chaque
  pas. Aucun jeu d'une boucle mise à l'écart n'entre jamais dans une table.
- **Mesure** : les trois lectures du §12.6, inchangées (« toutes », « écart », « écart, dans l'échantillon »), à chaque
  pas. Les boucles mises à l'écart sont jouées sur la surcouche « écart » de leur équipe avec la table de la variante (la
  même pour les deux surcouches, comme `rc`).
- **Fuite restante, connue** : les jeux d'apprentissage se font sur la surcouche complète, qui contient les traces mises
  à l'écart (en équipe d'abord, elles influencent les parcours des autres boucles). Mesurée faible à un pas (§4.2 du
  relevé : ≤ 0,05) ; l'écart « écart, dans l'échantillon » − « écart » est rapporté **à chaque pas** pour voir s'il
  grandit avec l'itération. L'éviter demanderait de jouer aussi les boucles non mises à l'écart sur la surcouche « écart »
  (temps doublé) : écarté.
- **Groupes** : toutes équipes, puis A et B, types ROAD et GRAVEL, curseurs 0,25 et 1 ; groupes de moins de 30 boucles
  marqués indicatifs (B « écart » à 10 % : 14 et 26).

### 13.5 Variantes : syntaxe et dépendances (`likeness/variants.rs`)

**Composants nouveaux** (joints par `+` aux existants) :

| Composant | Rôle | Nom |
|---|---|---|
| `rt:<γ>` | recalé **par équipe** (§13.2.4), γ ∈ ]0, 4] | `rt` + dixièmes : `rt05` |
| `n:<k>` | chaîne de `k` pas, entier de 1 à 5 ; seulement avec `rc` ou `rt` | suffixe `-p1` … `-pk`, un par variante |
| `dim:<72\|me>` | forme de la table ; seulement avec `rc` ou `rt` ; défaut `72` | `me` ajoute `-me` après le goût ; `72` n'ajoute rien |
| `famroad:<f>` | familiarité `f` pour les requêtes ROAD en équipe d'abord seulement ; découverte, strict et GRAVEL intacts | `famroad` + centièmes : `famroad100` |

**Expansion** : `rc:0.5+n:3` donne, **à sa place dans la liste**, trois variantes consécutives `rc05-p1`, `rc05-p2`,
`rc05-p3`. Le nom est fait des composants dans l'ordre donné joints par `-` (règle du §12.3), `n` n'y figurant pas, puis
`-p<k>` : `famroad:1+rt:0.5+dim:me+n:3` → `famroad100-rt05-me-p1` … `-p3`. **`rc:<γ>` sans `n`** reste une variante
unique nommée `rc05` (chaîne d'un pas, sans suffixe) : la première sonde se rejoue telle quelle.

```rust
pub enum TasteSource { Team, General, Recal, RecalTeam }      // code "rt" pour RecalTeam
pub struct Chain { pub stem: String, pub step: u8, pub steps: u8, pub dims: Dims }
pub struct Variant {
    pub name: String,
    pub fam: Option<f32>,
    /// `famroad` : la familiarité ne touche que ROAD en équipe d'abord.
    pub fam_road_only: bool,
    pub taste: Option<(TasteSource, f64)>,
    /// `Some` pour `rc` et `rt` (`rc` seul : `steps = 1`, nom sans suffixe).
    pub chain: Option<Chain>,
}
```

`is_base()` : ni familiarité ni goût. `adapt_request(req, fam, road_only)` : avec `road_only`, `familiarity = f` si
`req.ride_type == RideType::Road && req.mode == Mode::TeamFirst`, sinon la requête n'est pas touchée.

**Dépendances** : le pas 1 d'une chaîne apprend sur la **dernière `base` jouée avant elle** (requise plus tôt dans la
liste, comme `rc` aujourd'hui), quelle que soit la familiarité de la chaîne (`famroad100-rc05-p1` apprend sur `base`,
comme `fam100-rc05`) ; le pas `k ≥ 2` apprend sur les jeux du pas `k − 1` **de la même chaîne**, joué juste avant lui.
L'expansion garantit l'ordre ; on ne peut pas entrelacer deux chaînes.

**Refus** (avant toute phase lourde, en plus de ceux du §12.3) : `n` ou `dim` sans `rc` / `rt` ; `n` non entier ou hors
de [1, 5] ; `dim` autre que `72` ou `me` ; `fam` et `famroad` ensemble ; deux `n` ou deux `dim` ; `rt` sans `base` plus
tôt ; nom en double **après** expansion (`rc:0.5+n:1` et `rc:0.5+dim:72+n:1` donnent tous deux `rc05-p1`).

### 13.6 Déroulé (`likeness/mod.rs`)

Phases 1 à 7 inchangées ; 7 bis seulement si une variante a `ge` ou `gg` (aucune dans les campagnes du §13.8). En 8 :

- `rc_inputs` est remplacé par `base_meters: Option<ClassMeters>` (calculé après le premier `base`, gardé toute la
  campagne), `prev_meters: Option<ClassMeters>` (calculé après chaque pas qui a un successeur, lâché quand celui-ci a
  appris) et `chain: Option<ChainState>` (créé au pas 1, lâché après le dernier pas).
- Avant de jouer une variante de chaîne : `inputs` = `base_meters` au pas 1, `prev_meters` sinon ;
  `out = chain.step(...)`. `play_variant` reçoit `out` : `rc` → la `Taste` commune pour toutes les équipes et les deux
  surcouches ; `rt` → `out.team[t]` pour l'équipe `t`, les deux surcouches ; `out.tables` → `VariantRun::tables`.
- Après chaque variante (base comprise) : `ClassMeters::of(...)` sur ses jeux, pour `prev_meters` et pour un diagnostic
  `l1_after` (par propriétaire et type : l'écart L1 que **ses** parcours laissent ; pour la table commune, sur `pooled`).
- **Temps par équipe** : `play_variant` mesure, par équipe, `build_s` (`build_team` des deux surcouches), `wall_s`
  (l'équipe entière, comme le message actuel), et, sur les jeux `full` seulement, `plays_full` et `cpu_s_full`
  (Σ `PlayRec::ms` / 1 000).
- **Contrôle « pas 1 = `rc05` »** : si `a.at_4a_defaults()`, si `m`, `bound` et `min_km` sont aux défauts et si la
  variante est `rc05` ou `rc05-p1` (`rc`, 72 cases, γ = 0,5, sans familiarité), sa `taste_xxh3` doit être
  `4002e232ea272c83` et son empreinte de résultat `ebb20ed148f75185` (relevé, §2) ; sinon, comme pour `base` : sorties
  écrites, message, **arrêt** (« le pas 1 n'est pas `rc05` : chercher, ne pas lire la suite »). Constantes à côté de
  `BASE_4A_FINGERPRINT`.
- **Contrôle « toutes »** (hors défauts du 4a, campagne 2) : la comparaison imprime les `S_base` « toutes » et les
  compare à 0,614 · 0,721 · 0,735 · 0,680 à un millième près (oui / non) ; pas d'arrêt.

`VariantMeta` gagne : `fam_road_only: bool`, `chain: Option<ChainMeta>` (`stem`, `step`, `steps`, `learned_from`,
`dims`, `team_n0`, `mult_bound`, `diag: Vec<ChainOwnerStep>`), `l1_after: Vec<OwnerGap>`, `team_times:
Vec<TeamTime>` (`team` lettre, `loops`, `plays_full`, `cpu_s_full`, `cpu_s_full_by_cursor: Vec<(f64, f64)>`,
`build_s`, `wall_s`). Lettres et agrégats seulement.

### 13.7 Sorties (`likeness/compare.rs`)

Les sorties par variante et la comparaison du §12.5 restent (elles prennent les nouvelles variantes d'elles-mêmes).
Ajouts à `<label>-compare.{md,json}` :

1. **Conditions** : en plus, `team_n0`, `mult_bound`, la part mise à l'écart, et les deux contrôles du §13.6.
2. **§3 bis, chaînes pas à pas** — une table par chaîne (`stem`) et par lecture (« toutes », « écart », « écart, dans
   l'échantillon ») ; toutes équipes :

   | pas | appris sur | ROAD 0,25 | ROAD 1 | GRAVEL 0,25 | GRAVEL 1 | groupes ≤ 0,5 |
   |---|---|---:|---:|---:|---:|---:|
   | p0 | — | 1,00 | 1,00 | 1,00 | 1,00 | 0 |
   | p1 | base | `S/S_base` | … | | | |

   La ligne `p0` est `base`. Sous la table : par pas, la fuite (« écart, dans l'échantillon » − « écart ») par groupe.
3. **§5 bis, par équipe et par pas** : même table pour A et B, lectures « toutes » et « écart », effectif de boucles en
   tête de colonne, indicatif sous 30.
4. **§3 ter, diagnostics** : par chaîne et par pas, une ligne par propriétaire (`général` ; pour `rt` : `commun`, A, B,
   puis « autres » = somme des autres lettres) et type : boucles, poids, `l1_gap` (avant), `l1_after` (après),
   `mean_mult`, `halved`, `gamma_min`, `at_bound`. Puis les multiplicateurs **cumulés** agrégés (6 voies, 3 revêtements,
   balisé ; le tableau 2 actuel, une colonne par pas).
5. **§7, service** : colonne `× base` du P50 en plus, au même curseur ; signaux du §12.6 inchangés (P50 > 1,25 × base,
   P90 > 1,5 × base, part `OK` en baisse de plus d'un point, `PANIC`, `WINDOW_ESCAPE`).
6. **§9, coût d'un profil par équipe** [estimé] — pour chaque chaîne `rt`, par équipe (A, B, « autres ») : les
   générations nécessaires à `n` pas sont `base`, p1, …, p(n−1) ; pour chacune `plays_full`, `cpu_s_full`, `wall_s`,
   `build_s` ; puis le total des `cpu_s_full` et le temps estimé sur 1, 4, 8 et 16 fils (`total / fils`) ; enfin le même
   calcul sur les seuls jeux du curseur 0,25 (l'allègement « un seul curseur » de la question 5). Avertissement imprimé : mesuré à
   16 fils sur 8 cœurs, machine pleine ; un cœur dédié est plus rapide, non mesuré.
7. **§8, lecture** (faits, sans décision) : par chaîne, le premier pas où chaque groupe GRAVEL et ROAD passe sous 0,5
   (« toutes » et « écart »), ou « jamais » ; pour A et B, `rt` contre `rc` du même pas et de la même forme ; les signaux
   de service par pas ; la plus grande fuite par chaîne ; `mean_mult` le plus éloigné de 1.

JSON : `chains: Vec<ChainSummary { stem, source, dims, fam, steps: Vec<String> }>`, `team_cost: Vec<TeamCostRow>` ;
les `S` par pas se lisent dans `criterion` (lignes déjà par variante).

### 13.8 Campagnes, ordre, temps et mémoire

Binaire `--release`, `B=./target/release/ord-loops-bench`, sorties sous `~/.cache/osm-route-details/graph/lot4/`
seulement. **Une campagne à la fois**, jamais deux processus. Les labels `sonde` et `sonde2` sont pris (première sonde).

1. **Essai court** : `$B likeness --teams C --limit 5 --variants base,rc:0.5+n:2,rt:0.5+n:2,famroad:1+rt:0.5+dim:me+n:2
   --label recal-essai` : sorties et comparaison lisibles, aucune panique.
2. **Déterminisme** : `$B likeness --teams B --variants base,rc:0.5+n:2,rt:0.5+dim:me+n:2 --threads 1 --label recal-det1`
   puis `--threads 16 --label recal-det16` : mêmes empreintes de résultat et mêmes `taste_xxh3`, variante par variante
   (~15 min à 1 fil).
3. **Campagne 1, `recal-general`** (réglages du 4a, mise à l'écart 10 %) :
   `$B likeness --label recal-general --variants base,rc:0.5+n:3,rc:0.5+dim:me+n:3`
   - 7 variantes : `base`, `rc05-p1..p3`, `rc05-me-p1..p3` ;
   - **arrêts automatiques** : `base` ≠ `55c3b065dc085f42` ; `rc05-p1` ≠ (`ebb20ed148f75185`, `4002e232ea272c83`) ;
   - [estimé] ~1 min de phases communes, `base` ~205 s, pas de chaîne 210 à 260 s (`rc05` 213 s, `rc10` 241 s) : **26 à
     32 min** ; mémoire de la première sonde : pic anonyme ~16 à 17 Go, RSS < 20 Go.
4. **Choix de la forme pour la campagne 2** (règle, sauf avis contraire du propriétaire) : `X = me` si, au pas 3 en
   lecture « écart », `rc05-me-p3` fait au plus 0,03 de plus que `rc05-p3` dans **chacun** des 4 groupes toutes équipes
   (moins de paramètres, aucun artefact de case rare) ; sinon `X = 72`. On note `+dim:X` (omis pour 72).
5. **Campagne 2, `recal-equipe`** (mise à l'écart 25 %, question 2) :
   `$B likeness --label recal-equipe --holdout-share 0.25 --variants base,rc:0.5+dim:X+n:3,rt:0.5+dim:X+n:3,famroad:1+rc:0.5+dim:X+n:3,famroad:1+rt:0.5+dim:X+n:3`
   - 13 variantes, dans cet ordre : `base`, la chaîne commune, la chaîne par équipe, puis les deux avec `famroad` ;
   - la chaîne commune y est rejouée parce que l'écart à 25 % change les tables et les boucles mesurées : `rt` ne se
     compare qu'à `rc` de la même campagne ;
   - contrôle « toutes » (§13.6), pas d'arrêt ;
   - [estimé] +14 % de jeux ; 235 à 310 s par variante (`fam100-rc05` : 242 s à 10 %) : **50 à 70 min** ; `LoopRec`
     gardés ~0,15 Go par variante : pic anonyme ~17,5 à 18 Go, RSS ≤ 21 Go, sous les 30 Go.
6. **Relevé** (§13.10, commit 5).

### 13.9 Tests (synthétiques, aucune donnée de production)

`ord-loops-bench` seulement (`ord-loops` n'est pas touché ; ses tests ne changent pas) :

1. `parse_variants` : `rc:0.5+n:3` → `rc05-p1`, `rc05-p2`, `rc05-p3` consécutifs, chaîne `(stem rc05, k, 3, D72)` ;
   `rc:0.5` → `rc05`, chaîne d'un pas ; `famroad:1+rt:0.5+dim:me+n:2` → `famroad100-rt05-me-p1`, `-p2` ; `dim:72` sans
   effet sur le nom ; refus du §13.5 un par un (`n:3` seul, `ge:0.5+n:2`, `n:0`, `n:6`, `n:1.5`, `dim:36`,
   `fam:1+famroad:1`, `rt:0.5` sans `base` plus tôt, `base,rc:0.5+n:1,rc:0.5+dim:72+n:1`).
2. `adapt_request` avec `famroad` : ROAD au curseur 0,25 → `TeamFirst`, q8 640, f ; ROAD au curseur 1 → `Discovery`
   inchangé ; GRAVEL au curseur 0,25 → familiarité inchangée ; strict inchangé.
3. Pas à 72 cases : (a) depuis `mult = 1`, les `q8` du pas sont **égales** à celles de `ratio_table` pour trois jeux de
   vecteurs et γ ∈ {0,5 ; 1} (à γ = 1,5, avec `mult_bound` = 10 pour que la borne cumulée ne morde pas) ; (b) réel = généré → `mult`, `gamma`, `sign` inchangés ; (c) une classe dont la
   correction change de signe voit son γ divisé par deux, et elle seule ; une correction sous 0,02 ne compte pas comme
   changement de signe ; (d) avec des entrées extrêmes répétées 5 fois, `mult` reste dans `[1/4, 4]` et `at_bound` les
   compte.
4. **Convergence sur un générateur jouet** : `gen[c] ∝ avail[c] · mult[c]^(−ε[c])`, ε = 3,2 pour une classe et 1 pour
   les autres, réel fixé ; trois pas à γ = 0,5 : `l1_gap` après le pas 3 inférieur à la moitié de l'initial, γ de la classe
   élastique divisé par deux au pas 2 et pas celui des autres (valeurs exactes calculées dans le test).
5. Recalé par équipe : (a) une équipe sans donnée d'un type reçoit **exactement** les `q8` de la table commune du même
   pas ; (b) une équipe au même rythme par boucle que l'ensemble a `w = N_T / (N_T + n0)` (valeur exacte) ; (c) avec
   `n0 = 0`, ses `q8` sont celles de sa propre table.
6. Effets principaux : (a) une cible exactement additive (`ℓ = a + b + β` donnés, tous les poids > 0) est retrouvée à
   1e-9 près (valeurs ajustées) ; (b) deux cases qui ne diffèrent que par le bit destination ont le même `mult` ; (c) un
   niveau sans poids garde 0 ; (d) après jauge, `Σ Ω_s b[s] = 0` et `Σ Ω_k β[k] = 0` ; (e) coefficients bornés à
   `ln bound`.
7. `ClassMeters::of` : `pooled` égal **au bit près** à l'ancien `RcInputs::of` (gardé dans le test comme référence) sur
   des jeux synthétiques ; la somme des `team` égale `pooled` à 1e-9 relatif ; une boucle mise à l'écart, un jeu
   `holdout` ou un jeu non `OK` n'y comptent jamais.
8. Comparaison : sur une liste synthétique de `Done` (une `base`, une chaîne de trois pas), la table pas à pas
   (ordre, `p0` = 1,00, premier pas sous 0,5), et le coût d'un profil (générations `base`, p1, p2 ; total et divisions
   par fils exacts).

### 13.10 Commits

Chacun passe `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` et `cargo test -p ord-loops-bench`.
Fin de message : une ligne vide puis la ligne `Co-Authored-By` du modèle qui code.

1. `feat(bench): likeness, chaînes de recalé dans --variants (n, dim, rt, famroad)` — `variants.rs`, arguments
   `--taste-team-n0` et `--taste-mult-bound`, tests 1 et 2.
2. `feat(bench): likeness, pas du recalé itéré, par équipe et en effets principaux` — `chain.rs` (pur : `ParamState`,
   pas à 72 cases, `fit_main_effects`, pas en effets principaux, mélange par équipe, diagnostics), tests 3 à 6.
3. `feat(bench): likeness, chaînes jouées pas à pas` — `ClassMeters`, `ChainState` dans `run` et `play_variant`, `rc`
   par les chaînes, contrôle « pas 1 = `rc05` », temps par équipe, champs de `VariantMeta`, test 7.
4. `feat(bench): likeness, comparaison pas à pas et coût d'un profil par équipe` — `compare.rs` (§13.7), test 8.
5. Campagnes du §13.8, puis `docs(ord-loops): relevé du recalé itéré et par équipe (lot 4)` : nouveau relevé
   `docs/design/technique/ord-loops-lot4-recal.md` (style et marques de `ord-loops-lot4-sonde.md` : conditions et
   commandes, contrôles, tables par pas, critère par lecture et par pas, A et B, diagnostics, forme, service, coût d'un
   profil, ce que la sonde décide au regard des questions du §13), et un §13.13 « Écarts du codage » ajouté ici. Équipes
   lettrées et agrégats seulement : ni départ, ni coordonnée, ni id de trace ou d'équipe.

### 13.11 Ce que le relevé tranche, et comment

Écrit comme des faits ; la décision revient au propriétaire.

1. **GRAVEL** : le premier pas où les deux groupes GRAVEL passent sous 0,5 en « écart » **et** en « toutes », par
   chaîne ; « jamais » sinon. Si aucun pas n'y arrive, dire ce qui reste (chemins, non revêtu : P50 et W1 / IQR) et si
   `l1_after` stagne (plancher du levier « multiplicateur par classe ») ou décroît encore (un pas de plus servirait).
2. **Par équipe** : `rt` contre `rc`, même pas, même forme, campagne 2. Les faits à donner : B ROAD (« toutes » et
   « écart », deux curseurs) — `rt` sous 1,0 (B ne perd plus) et l'écart à `rc` ; A — `rt − rc` en « écart ». Lecture
   proposée : profil par équipe défendable si B ROAD passe sous 1,0 avec `rt` aux deux curseurs et deux lectures, pendant
   que `rc` reste au-dessus de 1,2, sans que A perde plus de 0,03 ; sinon, **table générale par type**.
3. **Surapprentissage** : la fuite par pas (« écart, dans l'échantillon » − « écart ») ; au-delà de 0,10 au pas 3, la
   chaîne surapprend : préférer un pas plus tôt ou les effets principaux.
4. **Coût** : le §9 de la comparaison, rapporté au budget de la question 5.
5. **Service** : par pas, P50 contre 1,25 × `base`, part `OK` à moins d'un point, `PANIC` et `WINDOW_ESCAPE` ; et la
   dérive `mean_mult` (au-delà de 1 ± 0,10 : à normaliser au 3b).

### 13.12 Ce que la seconde sonde ne fait pas

Rien au moteur (`crates/ord-loops` inchangé, empreinte de `base` `55c3b065dc085f42`), au service (`ord-server`), au
contrôleur, au contrat, à la version ; rien au dépôt ; pas de nouvelle dépendance. Pas de goût `ge` / `gg` (abandonnés,
décision 1 du §9.2) ; pas de table par curseur (une table par type sert les deux curseurs, comme le moteur l'appliquera) ;
pas de normalisation (question 6) ; pas de jeu des boucles d'apprentissage sur la surcouche « écart » (§13.4) ; pas de
pente ni de trafic.

### 13.13 Écarts du codage

Écrits le 17 novembre 2026, au codage des commits 1 à 4 du §13.10 (branche `feat/ord-loops-lot4-sonde2`). Aucune campagne
complète n'a été lancée : le commit 5 (campagnes du §13.8, relevé `ord-loops-lot4-recal.md`) reste à faire ; seuls un essai
de fumée (une à deux équipes, 5 à 40 boucles, sorties privées sous `~/.cache/osm-route-details/graph/lot4/smoke-sonde2/`) et
un contrôle de déterminisme à petite échelle (équipe B, 20 boucles, 1 et 16 fils : mêmes empreintes de résultat et mêmes
`taste_xxh3`, variante par variante, `rc` et `rt` en effets principaux compris) ont tourné. Ni le contrôle « pas 1 = `rc05` »
(§13.6), ni `base` = `55c3b065dc085f42`, ni le contrôle « `S_base` toutes » n'ont donc été exercés sur la France : ils le sont
au premier lancement de la campagne 1 (arrêt automatique si le pas 1 n'est pas `rc05`).

1. **`Dims` vit dans `variants.rs`** (et `chain.rs` le réexporte) : `Chain` en a besoin au commit 1, avant que `chain.rs`
   existe.
2. **`ClassMeters::of`** garde la signature du §13.2.1 (`&[export::Loop]`) et délègue à `of_keys` sur les seuls
   `(équipe, type)` des boucles : c'est tout ce qu'elle en lit, et le test 7 n'a pas à fabriquer des `Walk` et des `Trace`.
3. **La comparaison des chaînes est dans `compare_chain.rs`**, appelée par `compare.rs` (qui reste dans ses 900 lignes) ;
   les sections sont émises à leur place : 3 bis et 3 ter après le 3, 5 bis après le 5, 8 bis puis 9 après le 8.
4. **Effets principaux, jauge** : le centrage des marges `b` et `β` ne décale que les niveaux qui ont du poids (un niveau sans
   poids garde 0, test 6c) ; la borne `±ln bound` s'applique après la jauge, si bien que `Σ Ω_s b[s] = 0` n'est exact que tant
   que rien n'est borné (cas du test 6d).
5. **Mélange par équipe, bits** : `w = 0` rend `g_P` (ou le coefficient commun) sans passer par `exp ∘ ln`, et `w = 1` rend
   `g_T`, pour que « une équipe sans donnée reçoit exactement les `q8` de la table commune » (test 5a) soit vrai au bit près ;
   sinon `exp(w ln g_T + (1 − w) ln g_P)`. La table commune de `rt` est tenue dans l'état `Owner::General` (son signe et son γ
   suivent la cible `g_P`), jamais jouée.
6. **Cas dégénéré à une équipe** : si une campagne ne joue qu'une équipe, `rt` donne les mêmes tables que `rc` (l'ensemble est
   l'équipe, `g_T = g_P`) ; observé à la fumée sur B et sur C. Sans objet aux campagnes du §13.8 (A, B, C, D, E…).
7. **Fuite** : la convention du §13.4 est gardée (« écart, dans l'échantillon » − « écart », en `S / S_base`) ; elle est
   **négative** quand l'échantillon se lit mieux que l'écart (le signe d'un surapprentissage). Le seuil de 0,10 du §13.11
   point 3 est donc lu **en valeur absolue** ; la plus grande fuite rapportée au §8 bis est la plus grande en valeur
   absolue, avec son signe.
8. **Diagnostics** : un propriétaire sans paire comptée pour un type (le MTB, une équipe sans GRAVEL) n'a pas de ligne ; « autres »
   somme les boucles, les paires, les paramètres divisés et les classes à la borne, et moyenne simplement `l1 avant`, `l1 après`,
   `poids` et `mult. moyen`. `l1_after` est aussi calculé pour `base` et les variantes sans chaîne (propriétaire `général`, et
   les équipes), sans conséquence au rapport. Les multiplicateurs cumulés du §3 ter sont pondérés **par la part générée du pas 1**
   dans toutes les colonnes (comparables d'un pas à l'autre) ; `rt` les donne pour A et B.
9. **Nom** : `dim:me` est inséré juste après le composant de goût quel que soit son ordre dans la spécification
   (`dim:me+rc:0.5` donne `rc05-me`) ; le reste suit l'ordre donné.
10. **Contrôles** : `Conditions` gagne `holdout_share`, `step1_check` et `all_check_applies` ; le contrôle « `S_base` toutes »
    ne s'applique que hors réglages du 4a, aux curseurs par défaut, sans `--limit`, `--max-km` ni `--teams`, et sans
    `--no-holdout` ; ailleurs, « sans objet ». Le contrôle « pas 1 = `rc05` » demande en plus `m`, `bound` et `min_km` aux
    défauts (`LikenessArgs::taste_at_4a_defaults`).
11. **Coût** : `cpu_s_full` est la somme des `PlayRec::ms` des jeux `full` ; « autres » est la **somme** des autres lettres
    (pas un profil unique) ; le temps sur `k` fils est `total / k` (un calcul, pas une mesure), et l'allègement « un seul
    curseur » prend le premier curseur de `--cursors` (0,25 par défaut).
12. **Commit 1** : `rt` y était refusé à l'exécution (message « pas encore branché »), levé au commit 3.

**Relecture** (17 novembre, relecteur indépendant, avant les campagnes) :

13. **Surapprentissage, lu à surcouche égale.** La « fuite » du §13.4 (« écart, dans l'échantillon » − « écart ») compare
    deux surcouches pour les mêmes boucles : elle mesure ce que leurs propres traces apportent à la surcouche complète, et
    rien du surapprentissage des tables, qui n'apprennent jamais sur ces boucles. Le §13.11 point 3 se lit donc sur une
    lecture nouvelle, **« apprentissage »** (boucles non mises à l'écart, surcouche complète) : surapprentissage =
    `S/S_base` « écart, dans l'échantillon » − « apprentissage », même surcouche, même table, nul à `p0` par
    construction, positif quand les boucles qui n'ont pas servi se lisent moins bien. Rapporté au §3 bis (toutes
    équipes, A, B) et au §8 bis (plus forte valeur) ; la fuite de la surcouche reste rapportée à côté. Les lignes par
    équipe du critère portent les quatre lectures (le tableau 5 en montre toujours deux). **Bruit** : à la fumée (B et C,
    5 boucles mises à l'écart par équipe) ces différences vont de −0,4 à +0,5 : seule la lecture toutes équipes de la
    campagne (241 puis 712 boucles mises à l'écart) est lisible ; pour B (~35 ROAD à 25 %), indicatif.
14. **Contrôle « `S_base` toutes »** : il s'appliquait dès que les réglages sortaient du 4a, curseurs, limite et équipes
    exceptés (un autre `--seed` ou `--min-km` l'aurait fait échouer à tort). Il ne s'applique plus que si seules
    `--holdout-share` et `--holdout-min-loops` diffèrent (`at_4a_defaults_but_holdout_share`).
15. **Essai court et déterminisme sur une seule équipe** : `rt` y est dégénéré (point 6), le mélange par équipe n'y est
    donc pas exercé. Les commandes ci-dessous jouent deux équipes (B et C) ; vu à la relecture, B et C, 20 boucles chacune,
    25 % à l'écart, `base,rc:0.5+n:2,rt:0.5+n:2,rt:0.5+dim:me+n:2` : `rt` diffère de `rc` dès le pas 1, 2 min 09 à 16 fils,
    RSS 15,1 Go, aucune panique.
16. **Non corrigé, à savoir** : en effets principaux, le mélange par équipe se fait paramètre par paramètre (§13.2.4) sur
    deux ajustements dont la jauge est fixée séparément (marges centrées sur les poids de l'équipe, et sur ceux de
    l'ensemble). Quand les poids d'équipe d'une voie et d'un revêtement diffèrent, la somme mélangée dépend de cette
    convention (de `(w_voie − w_revêtement) × décalage de jauge`) ; négligeable quand l'équipe génère au rythme de
    l'ensemble (A), à garder en tête pour B et les petites équipes.

**Commandes des campagnes** (§13.8, essai court et déterminisme sur deux équipes, point 15 ;
`B=./target/release/ord-loops-bench`, depuis le dépôt, une campagne à la fois) :

```
cargo build --release -p ord-loops-bench
# 1. essai court
$B likeness --teams B,C --limit 5 --variants base,rc:0.5+n:2,rt:0.5+n:2,famroad:1+rt:0.5+dim:me+n:2 --label recal-essai
# 2. déterminisme (deux équipes : le mélange de `rt` y est exercé ; ~20 à 30 min à 1 fil, estimé)
$B likeness --teams B,C --variants base,rc:0.5+n:2,rt:0.5+dim:me+n:2 --threads 1 --label recal-det1
$B likeness --teams B,C --variants base,rc:0.5+n:2,rt:0.5+dim:me+n:2 --threads 16 --label recal-det16
# 3. campagne 1
$B likeness --label recal-general --variants base,rc:0.5+n:3,rc:0.5+dim:me+n:3
# 4. forme X = me ou 72 (règle du §13.8.4), puis campagne 2 (X = me : +dim:me ; X = 72 : rien)
$B likeness --label recal-equipe --holdout-share 0.25 --variants base,rc:0.5+dim:X+n:3,rt:0.5+dim:X+n:3,famroad:1+rc:0.5+dim:X+n:3,famroad:1+rt:0.5+dim:X+n:3
```
