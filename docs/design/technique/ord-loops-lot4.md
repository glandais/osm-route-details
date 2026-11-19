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
> **Remplacé (§9.4)** par un profil recalé par équipe, appris au dépôt par regénération, et une table commune dans le
> moteur : `ord-loops-lot4-sonde2.md` §10.1 ; spécification au 3b.

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

### 9.3 Décisions avant les campagnes de la seconde sonde (2026-11-17, réponses au §13.1)

1. **Déterminisme** : pas de passe complète (faite à petite échelle à la relecture, §13.13 point 15) ; essai court
   seulement.
2. **Relevé** : `docs/design/technique/ord-loops-lot4-sonde2.md` (et non `ord-loops-lot4-recal.md`, nom des §13.10 et
   §13.13).
3. **Forme de la table** : 72 cases et effets principaux joués en campagne 1 ; la campagne 2 prend les effets principaux
   s'ils font au plus 0,03 de plus que 72 cases au pas 3 en lecture « écart » dans chacun des 4 groupes, sinon 72 cases
   (règle du §13.8 point 4).
4. **Pas** : 3 pas, γ = 0,5, amortissement au changement de signe, borne de pas `[1/3, 3]` ; pas de normalisation.
5. **Campagne 2** : `--holdout-share 0.25` ; n0 = 25 ; mélange par équipe en effets principaux : option (a), accepté tel
   quel, l'effet de jauge du §13.13 point 16 signalé au relevé ; `famroad:1`.
6. **Critère GRAVEL** (`S / S_base ≤ 0,5`) lu sur la lecture « toutes » ; la lecture « écart » sert de contrôle de
   surapprentissage (écart > 0,10 au pas 3 → préférer un pas plus tôt ou les effets principaux), sur la lecture toutes
   équipes seulement. Un GRAVEL qui plafonne entre 0,50 et 0,55 est acceptable : on avance au 4c.
7. **Profil par équipe** retenu si B ROAD passe sous 1,0 avec `rt` aux deux curseurs en lecture « toutes » (lecture
   « écart » en appoint, indicative), pendant que `rc` reste au-dessus de 1,2, et si A ne perd pas plus de 0,03. Sinon :
   table générale par type.
8. **Budget du dépôt** pour un profil par équipe : au plus 30 min sur 4 fils pour la plus grosse équipe, allègements admis
   (au plus 500 boucles par équipe, un seul curseur, recalcul hebdomadaire). Production : 12 cœurs partagés avec tous les
   services, conteneur limité à 3 Gio. Coût calculé avec ces allègements sur les temps mesurés en campagne 2 ; l'effet
   d'un plafond de 500 boucles sur la qualité est mesuré si le banc le permet sans code nouveau, sinon dit non mesuré.
9. **Service** : critères inchangés (P50 par appel ≤ +25 % de `base`, part `OK` en baisse d'un point au plus, aucune
   `PANIC` ni `WINDOW_ESCAPE`) ; un multiplicateur moyen hors de 1 ± 0,10 est signalé.

### 9.4 Décisions (2026-11-17, après la seconde sonde, `ord-loops-lot4-sonde2.md`)

1. **Goût** : profil **par équipe**, appris au dépôt par regénération (recalé, 72 cases, 3 pas, γ = 0,5, amortissement au
   changement de signe, bornes `[1/3, 3]` par pas et `[1/4, 4]` cumulée, `n0` = 25, au plus 500 boucles par équipe), et
   **table commune** constante du moteur, repli des requêtes sans équipe ou sans profil. Appliqué à toutes les arêtes de la
   vue, aux deux curseurs. Remplace le §3.2 (§10.1 du relevé de la seconde sonde).
2. **Rythme** : le profil se recalcule à **chaque dépôt de nuit**, dans la même passe que la surcouche.
3. **Normalisation des tables** (A à 0,891) : tranchée **au banc pendant le 3b**, normalisée contre non normalisée.
4. **Familiarité** : reste à 0,5 partout ; la décision 3 du §9.2 est levée, l'étape « familiarité par type » supprimée.
5. **Suite** : 3b (profil au dépôt, table commune dans le moteur), puis le 4c avec ses distributions rejouées sur le profil.

### 9.5 Décisions (2026-11-17, après la relecture du 3b, avant la campagne 4)

Appliquées telles quelles, sans les rediscuter ; ce qui est codé est au §14.16 (points 24 à 29).

1. **Ordre** : la campagne 4 (§14.13) tourne **avant** la fusion ; on ne fusionne que si les critères tiennent (faits (a) à
   (d) du §14.12.5, règle du §14.12.4 pour la normalisation, service).
2. **B ROAD et le mélange** : si B ROAD reste ≥ 1,0 avec `svc`, **retour au mélange de la sonde** (forme de `rt` : chaîner
   depuis la table neutre en mélangeant à chaque pas avec la table commune de ce pas, poids nu `G_T / (G_T + n0 · G_P / N_P)`,
   sans plafond `N / (N + n0)`) : pas d'autre levier. La campagne 4 joue donc les deux formes **dans la même campagne**
   (`svc` et `svc+mix:sonde`).
3. **Normalisation** : la règle du §14.12.4 tranche sur la campagne 4, **définitivement**.
4. **Plafond de 500 boucles** : **par type** si `svc+capt:500` fait gagner au moins 0,03 au GRAVEL de A (lecture « toutes »),
   **sinon par équipe**. Lu sur la forme retenue (`svc-capt500` contre `svc`, ou `svc-sonde-capt500` contre `svc-sonde`).
5. **Table commune** : mesurée sur 75 % (celle du dépôt), puis **réapprise sur 100 % une seule fois** avant la mise en
   service, en le disant : nouveau `Common::hash`, nouvelle table commitée (et, si la forme `sonde` est retenue, les mètres
   des trois pas, qu'exporte la même sous-commande : §14.16, point 26).
6. **Profil partiel** (1 ou 2 pas dans le budget) : `present` et état `ready`, **sauf** si le dernier profil **complet** date
   de plus de 7 jours : `stale`, raison `profile`.
7. **`generator.profile.max_loops`** reçoit une borne haute à la validation : 5 000.
8. **Mesure en conteneur de 3 Gio** : la séance (§14.13) est rejouée **une fois**, après tout changement d'apprentissage,
   juste avant la fusion.

**Appliquées sur la campagne 4** (17 novembre au soir ; chiffres au §6 du relevé
[`ord-loops-lot4-3b-mesures.md`](ord-loops-lot4-3b-mesures.md), décisions au §7 du relevé) :
- **Décision 2, forme `capped`** (défaut inchangé). B ROAD « toutes » de `svc` vaut 0,750 · 0,924 [mesuré], sous 1,0 aux deux
  curseurs. A ne perd pas plus de 0,03 contre `rt05-p3` (au plus +0,017 [calculé]). Le retour à la sonde n'est donc pas
  déclenché.
- **Décision 3, normalisation brute, définitivement** (`COMMON_USE_NORMALIZED = false`). La paire `common-norm` / `common`
  gagne au mieux 0,014, sous le seuil de 0,02 [mesuré].
- **Décision 4, plafond par équipe** (défaut inchangé). `svc-capt500` gagne 0,038 sur le GRAVEL de A au curseur 0,25, mais
  0,003 au curseur 1 [mesuré]. **Lecture prudente, signalée** : le gain est exigé sur le GRAVEL entier, donc aux deux
  curseurs.
- **Décision 1, fusion : non.** (a), (b), (c), le service et `S / S_base` ≤ 0,5 par type tiennent. **(d) ne tient pas** :
  sur A, ROAD au curseur 1, `svc` est à +0,044 de `common`, pour un seuil de 0,03 [mesuré].
- **Décisions 5 et 8 : non faites**, car les critères ne tiennent pas. Elles viennent en dernier : la table à 100 % fausserait
  une campagne de contrôle, et la séance en conteneur ne se joue qu'une fois. Le propriétaire tranche d'abord (d) : §9 du
  relevé.
- **Décisions 6 et 7 : codées** (§14.16, points 24 et 25), relues sans correction.

### 9.6 Décisions du propriétaire (2026-11-18, sur le fait (d))

1. **Fait (d) accepté** à +0,021 · +0,044 (A ROAD, aux deux curseurs, `svc` contre `common`), sous une **réserve écrite, à
   revoir au 4c** (distributions rejouées sur le profil du 3b). Aucun paramètre ne change.
2. **Plafond de 500 boucles** : la lecture « aux deux curseurs » est confirmée, donc **par équipe** (défaut inchangé).
3. **Forme `capped`, table brute sans normalisation** : confirmées (ce sont déjà les défauts).
4. **On enchaîne jusqu'à la fusion** : table commune réapprise sur 100 % (décision 5 du §9.5, faite : §14.16, point 32),
   références re-bénies (même point), puis séance en conteneur rejouée une fois (décision 8 du §9.5, faite : §14.16,
   point 33). La fusion elle-même se fait après, hors de ce travail.

### 9.7 Décisions (2026-11-18, avant le 4c)

Réponses du propriétaire aux questions du §15.18 (la spécification du 4c les prend comme recommandées, sauf les points 2 et 5,
tranchés par l'orchestrateur du travail, comme la spécification le recommandait) :

1. **Temps** : jugé **en conteneur** (C3 : P50 ≤ 0,56 s à 4 clients, ≤ 0,29 s en série) et **relatif au banc** (C1 : P50 ≤
   1,15 × `svc`).
2. **Paires** : **4** par requête, lentille en repli ; `pairs:8` mesuré, retenu seulement s'il gagne 0,05 de W1 / IQR de r / D
   sans manquer C1 (tranché par l'orchestrateur, recommandation de la spécification).
3. **Réserve du fait (d)** : **levée** si `svc-org − common-org` ≤ +0,03 aux deux curseurs ; **maintenue** jusqu'à +0,054 ; au-delà,
   pas de bascule, et la table commune organique est proposée au propriétaire (le codeur ne la fait pas).
4. **Bascule** : **oui**, le défaut passe à `organic` dans un **commit à part** de la branche si A1 à A4, B1, B2, C1, C2 et D
   tiennent, suivi de la séance en conteneur ; commit annulé (`git revert`) si C3 manque. La fusion reste au propriétaire.
5. **Strict et aller simple** : lentille seule (`organic` sans effet), à rouvrir avec un banc strict (tranché par l'orchestrateur).
6. **Après le 4c** : 4d si A1 tient ; sinon une étape « lentille au ρ* » avant le 4d.

### 9.8 Décisions (2026-11-18, après les campagnes du 4c)

Décisions du propriétaire sur le §13 de [`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md), transmises par
l'orchestrateur du travail ; la spécification qui les applique est au §16 :

1. **Étape « virages » avant la bascule** : la recherche des jambes (et le choix des sommets ou des paires si utile)
   modélise la règle des virages du contrôle (grappes de 100 m, < 150°), pour ne plus produire des triangles refusés après
   coup. **Seuils inchangés** : C1 × 1,15 au banc ; C3 0,56 s à 4 clients et 0,29 s en série.
2. **A4 jugé en absolu** pour `shared_share` : médiane ≤ 0,05 et P90 ≤ P90 de `svc` + 0,05, par type et curseur (remplace la
   W1 / IQR de cette mesure).
3. **A2 inchangé**, rejugé après l'étape.
4. **Contrôle interne aligné sur le contrôleur indépendant** pour les virages, dans la même étape : c'est le moteur qui
   s'aligne, `crates/ord-loops-check` ne change pas. Invariant : `figures = Classic` reste 1.10 au bit près (références figées
   sans re-bénédiction) ; si l'alignement change forcément le `Classic`, il ne s'applique qu'en organique, et la note dit
   pourquoi.
5. **Bascule** : comme au §9.7 (commit à part si A1 à A4, B1, B2, C1, C2 et D tiennent, puis séance en conteneur, `git revert` si
   C3 manque). Si elle se fait, la règle L du relevé impose l'apprentissage organique (`learn_figures = organic`) : les profils
   sont `incompatible` jusqu'au redépôt. La fusion reste au propriétaire.
6. **Point 15 du §15.19** (`REUSE_OUTSIDE_TAIL`) : traité dans la même étape si sa cause s'établit. Elle s'établit (§16.0) ;
   le remède est au §16.4.

**Réponses au §16.14, 19 novembre 2026** (propriétaire, transmises par l'orchestrateur du travail) :

1. **Marge** `turn_margin_deg` = **5°** (le modèle refuse à ≥ 145°).
2. **Virages modélisés aussi dans les arbres T1 et T2, en organique seulement** : les heuristiques ne sont plus garanties
   admissibles en organique, accepté ; le `Classic` reste intact.
3. **A4 en absolu pour `shared_share` seul** ; les quatre autres mesures (`far_pos`, `detour_far`, `sinuosity_10`,
   `turns_per_10km`) gardent la W1 / IQR ≤ `svc` + 0,10.
4. **Si C1 manque encore** : la bascule se fait quand même **jusqu'à P50 ≤ × 1,25 de `svc` au banc**, à condition que tous les
   autres critères tiennent, **C3 compris** (≤ 0,56 s à 4 clients, ≤ 0,29 s en série, en conteneur). Au-delà de × 1,25 : pas de
   bascule, et le relevé décompose le temps (couloir, jambes, arbres).
5. **Points de prudence du §16.12 confirmés** : alignement sur les caps et l'épingle, épingle non modélisée, exemption d'arrivée
   lue sur T2, instrumentation de diagnostic non commitée (elle écrit des positions privées).

---

## 10. Étapes, dans l'ordre

1. 4a : mesures de forme en Rust, sous-commande du banc, relevé du moteur 1.9.2 → `ord-loops-lot4-mesures.md`.
   **Arrêt et relecture** : les §3 à §5 sont confirmés ou réordonnés.
2. Sonde du coût d'une recherche point à point (§4.5). **Fait** le 18 novembre sur la branche `feat/ord-loops-lot4c`
   (relevé : [`ord-loops-lot4c-sonde.md`](ord-loops-lot4c-sonde.md)). Une recherche P1 → P2 coûte 5,4 ms et 0,08 × T1
   au P50. Le frein du triangle est la longueur (jambe d'arrivée reprise dans 70 % des cas), pas le budget. Deux
   questions au §9 du relevé.
3. 4b, révisé après les sondes (§9.2 à §9.4) :
   - 3a : sondes du goût au banc → `ord-loops-lot4-sonde.md`, `ord-loops-lot4-sonde2.md` (fait) ;
   - 3b : profil de goût recalé par équipe, appris au dépôt par regénération (au plus 500 boucles), fichier `.ordp.enc`
     (§3.5), table commune dans le moteur ; normalisation tranchée au banc ; mémoire mesurée dans 3 Gio ; références
     figées re-bénies, version du moteur montée.
     **Fait** sur la branche `feat/ord-loops-lot4-3b` (paquet 1.10.0, API 1.10) : code, tests, mesure en conteneur de 3 Gio
     (relevé : [`ord-loops-lot4-3b-mesures.md`](ord-loops-lot4-3b-mesures.md)).
     **Campagne 4 faite** (17 novembre au soir, §6 et §7 du relevé). Les choix retenus sont la forme `capped`, le plafond
     par équipe et la table brute, définitivement : aucun défaut ne change. B ROAD vaut 0,750 · 0,924 et le service tient.
     **Le fait (d) ne tient pas** (A ROAD, curseur 1 : +0,044 contre `common`) : accepté par le propriétaire le 18 novembre
     avec une réserve à revoir au 4c (§9.6).
     **Fait** le 18 novembre : la table commune réapprise sur 100 % et les références re-bénies (§14.16, point 32), puis
     la séance en conteneur de 3 Gio rejouée une fois sur cette table (§14.16, point 33 ; §12 du relevé). **3b terminé :
     prêt à fusionner.**
     **Reste** : la fusion et la mise en service. La réserve sur le fait (d) passe au 4c (point 4).

     Écarts : §14.16.
4. 4c : triangle et couloir (quadrilatère hors lot, §9.1) ; distributions rejouées sur le profil du 3b. **Codé** le 18 novembre
   (étapes 1 à 7 et 9 du §15.14, écarts au §15.19). **Campagnes faites** le même jour (campagnes 1 à 6 du §15.15, relevé :
   [`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md)). La forme gagne nettement (A1 : r / D × 0,46 à × 0,72 de `svc`),
   le goût tient (B1) et la réserve du §9.6 sur le fait (d) est **levée** (B2 : `svc-org − common-org` = −0,058 · −0,045), le
   déterminisme aussi (D). **La bascule du défaut n'est pas faite** : C1 (× 1,29 · × 1,38 au banc), A2 (GRAVEL 1 à 0,317), A4
   (`shared_share` GRAVEL) et C2 (2 jeux `VIOLATION`) manquent, et aucun réglage de `w`, κ, `pairs`, `wide` ou `boost` ne les
   tient tous. Le défaut reste `classic`. Une séance en conteneur indicative (§11 du relevé) sert l'organique en 0,28 s en
   série, mais en 0,66 s à 4 clients, au-delà des 0,56 s de C3. **Prêt à fusionner**, figures
   organiques en option. **Reste**, au propriétaire : C1 et A4 (§13 du relevé), la bascule ; puis le 4d (décision 6 du §9.7, A1
   tient). Spécification : §15.
   **Étape « virages »** (décisions du §9.8, réponses du 19 novembre) : causes établies et **spécifiée** le 18 novembre (§16),
   **codée** le 19 (commits 1 à 4 du §16.9 et un commit de vitesse du modèle, écarts au §16.13). **Essai de fumée fait**
   (§14 de [`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md)) : contrôles I2 et I2′ tenus, `SHARP_TURN` des triangles
   par triangle complet −47 % (ROAD −58 %, GRAVEL −30 %), `REUSE_OUTSIDE_TAIL` à 0, mais **C1 manque** (P50 × 1,54 · × 1,79)
   et la règle d'arrêt du §16.10 (moitié au moins) n'est pas tenue. **Campagnes faites** le 19 novembre, l'arrêt levé par les
   réponses du propriétaire (§15 du relevé) : déterminisme (D tient), campagne complète et une campagne de décomposition du
   temps. Sur le tirage complet, `SHARP_TURN` par triangle complet −86 % (ROAD −95 %, GRAVEL −71 %), refus du contrôleur sur
   les parcours organiques rendus 34 → 0 ; **A1 à A4, B1, B2, C2 et D tiennent** (A2 et C2 manquaient), **C1 manque** : P50
   × 1,24 · × 1,40 de `svc` (second tirage × 1,26 · × 1,37), au-delà de × 1,25 au curseur 1 (réponse 4). **Pas de bascule** :
   le défaut reste `classic`. Le temps restant vient des jambes des triangles (+0,17 à +0,20) et du couloir (+0,13 à +0,15) ; le
   modèle ne coûte rien de net dans les arbres. Séance en conteneur indicative, défauts organiques (§15.7 du relevé) : P50
   0,29 s en série (≤ 0,29), **0,60 s à 4 clients** (≤ 0,56, 0,66 avant l'étape), mémoire et dépôts tenus. **Prêt à fusionner**,
   figures organiques en option ; **reste**, au propriétaire : les seuils C1 et C3, ou une étape de plus sur le temps.
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
   `docs/design/technique/ord-loops-lot4-sonde2.md` (décision 2 du §9.3) (style et marques de `ord-loops-lot4-sonde.md` : conditions et
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
complète n'a été lancée : le commit 5 (campagnes du §13.8, relevé `ord-loops-lot4-sonde2.md`, décision 2 du §9.3) reste à faire ; seuls un essai
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

## 14. 3b : spécification

Écrit le 17 novembre 2026, après la seconde sonde ([`ord-loops-lot4-sonde2.md`](ord-loops-lot4-sonde2.md), §8 à §10.1) et
les décisions du §9.4 (appliquées telles quelles), à la lecture de `crates/ord-loops` (`cost.rs`, `team.rs`, `search.rs`,
`window.rs`, `engine.rs`, `cache.rs`, références figées), de `crates/ord-graph-match` (`deposit.rs`), de
`crates/ord-server/src/generator` (`deposit.rs`, `store.rs`, `crypto.rs`, `windows.rs`, `generate.rs`, `routes.rs`,
`cursor.rs`, `config.rs`), du banc (`likeness/{export,walk,play,holdout,taste,chain,variants,mod}.rs`), des rapports privés
de la campagne `recal-equipe`, et du travail de nuit du backend de Pédalons (`RouteGeneratorOverlayJob`,
`RouteGeneratorV1`, lus **sans** rien y changer). Cette section **suffit au codeur** : ce qu'elle ne dit pas est laissé à
son jugement, à condition de tenir le §14.15. Les sections précédentes ne sont pas réécrites ; les erreurs factuelles
qu'elles portent sont signalées au §14.0 (points 15 à 17).

**Ce que livre le 3b** : (1) la **table commune** `C` (la table recalée commune à trois pas, `rc05-p3`, 72 cases × 3 types,
en entiers) devient le poids de base du moteur, pour toutes les arêtes de la vue, aux deux curseurs, avec ou sans équipe ;
(2) un **profil par équipe** appris à la fin de chaque dépôt en rejouant le moteur sur ses boucles, écrit à côté de la
surcouche dans `<team>.ordp.enc` ; (3) sa **lecture** en service, avec repli sur `C` et l'état `stale` raison `profile` ;
(4) le **banc** qui joue ce chemin du service avec le code du dépôt, tranche la normalisation et se compare à la seconde
sonde ; (5) la **mesure mémoire** du service dans un conteneur de 3 Gio.

### 14.0 Ce que la lecture a établi

1. **Le crochet du goût n'existe que dans `WindowTeam`** (§12.1) : une requête sans équipe n'en a pas (`Weighter::new` :
   `team.and_then(|t| t.win.taste_of(ride))`), ses poids sont les `Window::w_osm` de `cost::edge_weight`
   (`window.rs`, étape 5). La table commune, qui doit valoir **aussi** sans équipe, ne peut donc pas passer par le crochet :
   elle entre dans le **poids de base** du moteur (§14.3.2).
2. **Trois lieux appellent `edge_weight`, et trois seulement** : `window.rs` (sommes `w_osm`), `team.rs`
   (`WindowTeam::build_with_taste`, branche sans goût) et `search.rs` (`Weighter::edge`, morceaux du départ et de
   l'arrivée). `TeamData`, les index, `reverse.rs` et le contrôleur ne lisent aucun poids ; aucune heuristique ne suppose un
   minorant de `K` (l'A\* prend l'arbre retour) : des multiplicateurs sous 1 ne cassent aucune admissibilité. Seuls les
   coûts de virage sont absolus (`TURN_COST_MAX`, §12.0 point 5).
3. **Un goût passé au crochet remplace le poids de base, il ne s'y multiplie pas** : `taste_weight` =
   `⌈len × K × q8 / 16 384⌉`, relatif à `K`. Un apprentissage qui passe toujours **sa** table au crochet (les chaînes du
   banc, le dépôt du §14.5) ne dépend donc pas du poids de base du moteur : la même table rend les mêmes parcours sur le
   moteur du banc (poids de base neutre) et sur celui du service (poids de base `C`). C'est ce qui permet au banc de jouer
   le chemin du service sans second moteur ; c'est vérifié par le test 3 du §14.11.
4. **Le dépôt est synchrone et sa place est partagée par tous les environnements** : un fil bloquant par dépôt, une place
   (`deposit.max_concurrent` = 1), une file de 120 s puis 503 `Retry-After: 30`, un budget de 900 s. Le backend attend la
   réponse jusqu'à 16 min (`overlay.deposit-timeout=PT16M`), refait un dépôt refusé pour charge trois fois puis **saute
   l'équipe** jusqu'à la nuit suivante ; staging et production déposent à 02:10 UTC sur le même service. Une regénération
   de plusieurs minutes **dans** la place de dépôt ferait sauter les équipes de l'autre environnement : elle se fait hors
   de cette place (§14.7.1).
5. **Le backend redépose** quand `state ≠ ready`, quand l'empreinte des traces change, ou quand l'échéance (TTL de 14 jours)
   tombe dans les 7 jours : « à chaque dépôt » (décision 2 du §9.4) veut dire à chaque changement, ou une fois par semaine,
   pas chaque nuit pour chaque équipe. Il lit `stale_reason` comme une chaîne libre et redépose sur **tout** `stale`, et ses
   records portent `@JsonIgnoreProperties(ignoreUnknown = true)` : la raison `profile` et des champs nouveaux ne demandent
   **aucun** changement de son code [lu dans le dépôt tribly].
6. **Une ligne de dépôt ne porte ni identifiant ni date** (`TrackLine` : `type`, `excluded`, `network_edges`). Ni la graine
   du banc (`id_hash ^ seed`) ni un tirage par empreinte d'id ou par récence ne sont possibles au dépôt : graine et tirage se
   font sur une empreinte du **contenu** de la boucle appariée (§14.4.2), ce qui les rend indépendants de l'ordre des lignes.
7. **Le « premier parcours » du banc** (`play::play_one`) est le premier parcours **gardé par le contrôleur indépendant**
   (`TeamCheck` de la surcouche jouée), après une reprise en fenêtre élargie sur `WindowEscape`, **sans** budget de temps par
   requête ; une paire (boucle, curseur) compte si ce parcours existe. Le dépôt fait de même (§14.5.3).
8. **La purge d'une instance 1.9.x ne détruit pas un `.ordp.enc`** (`store.rs` de `develop`, 1.9.2) : dans le dossier du
   graphe courant, elle n'efface que les noms qui **contiennent** `.tmp-` (après 1 h, 300 s au démarrage) et ceux qui
   **finissent** par `.ordo.enc` sans `.tmp-` (après le TTL) ; un dossier d'espace n'est retiré que vide (`remove_dir`) ; un
   dossier `graph-*` d'un autre graphe part en entier après la grâce de 48 h (profils compris, c'est voulu). `read_current`
   n'efface que le chemin `.ordo.enc` d'une surcouche illisible ; `delete` aussi (le profil devient orphelin, ramassé par la
   purge de 1.10, §14.9) ; `delete_namespace` retire tout. Quotas et jauge ne comptent que les `.ordo.enc`. **Aucune parade
   n'est nécessaire**, à deux conditions tenues au §14.9 : le temporaire du profil contient `.tmp-` et ne finit pas par
   `.ordo.enc`, le nom définitif ne contient pas `.tmp-`.
9. **Chiffrement** : `seal` / `open` lient un fichier à `MAGIC ‖ graphe ‖ espace ‖ équipe` (données associées, `MAGIC` =
   `ORDOENC1`). Un profil scellé avec une autre magie ne s'ouvre pas comme surcouche, ni l'inverse : séparation de domaine
   sans nouvelle clé ni nouvelle dérivation.
10. **La table `rc05-p3` de la campagne 2** (`recal-equipe`, 25 % à l'écart, `taste_xxh3` `66ee08d3eb9c910c`) a une table
    MTB **non neutre** apprise sur 4 boucles, avec des classes aux deux bornes (64 et 1 024) ; la ROAD va de 64 à 494, la
    GRAVEL de 64 à 1 024 [lu dans le rapport privé].
11. **Les rapports du banc ne gardent pas les mètres générés par classe** (`PlayRec::first_class_m` est `#[serde(skip)]`,
    `l1_after` n'est qu'un écart L1) : le terme `G_P / N_P` du poids de mélange (§13.2.4) n'est pas relisible. Il faut
    rejouer la chaîne commune ; ce rejeu sert aussi de contrôle (empreintes connues, §14.3.1).
12. **Le mélange du banc est contemporain** : la cible de repli de `rt` est l'erreur moyenne des **mêmes** jeux de toutes les
    équipes (`g_P`), et chaque équipe part de `K` (multiplicateurs 1). Au dépôt une équipe est seule : sa cible doit être
    figée (réserve du §9 du relevé). La forme retenue (§14.5.2) n'a pas été jouée ; le banc la mesure (§14.12).
13. **Pas de `CHANGELOG`** dans le dépôt ; la version du paquet suit celle de l'API (`chore: version 1.8.0, calée sur l'API
    1.8`) : espace de travail en 1.9.2, `API_VERSION` = `"1.9"`.
14. **Plafond : 500 par équipe ou par type ?** La décision 1 du §9.4 dit « au plus 500 boucles **par équipe** », et c'est ce
    que le §8.2 du relevé a mesuré (`--limit 500` sur A, deux types confondus) ; la commande du 3b dit « au plus 500 par
    type ». **La décision du propriétaire est appliquée** (par équipe) ; le plafond par type est joué au banc pour éclairer
    la question 2 du §14.1.
15. *Correction du §3.5* : « Profil absent = moteur d'avant » est remplacé par la décision 1 du §9.4 (profil absent =
    **table commune**). La vérification que le §3.5 demandait au codage est faite (point 8).
16. *Correction du §6* : le 3b consomme l'api **1.10** (ajouts du §14.10) ; les ajouts du 4c et du 4d (`figures`, familles,
    attracteurs) iront en **1.11**.
17. *Correction du §3.5, mémoire du dépôt* : le profil ne s'accumule plus « trace par trace » (estimateur abandonné) ; il
    garde au plus 500 boucles retenues (~1 Mo) pendant l'appariement, puis rejoue le moteur trois fois après la ligne de fin
    (§14.7).

**Coût attendu au dépôt** [calculé sur le §8.1 du relevé, 0,49 s-fil par jeu pour A, 0,20 pour B, 1,85 pour les petites
équipes, banc à 16 fils sur 8 cœurs] : trois générations de 500 boucles × 2 curseurs pour A, soit 3 000 jeux, **~6,1 min
sur 4 fils** ; B (397 boucles) ~2,0 min ; les sept petites (52 boucles) ~2,4 min en tout ; **~10,5 min par environnement et
par nuit de redépôt complet**, deux fois si staging et production redéposent tout.

### 14.1 Choix pris par défaut, à confirmer

Chaque ligne est un choix que les décisions ne tranchent pas ; le défaut est le plus prudent, il est codé tel quel et
signalé au relevé. Une réponse différente du propriétaire change un défaut, pas l'architecture.

| # | Question | Défaut retenu | Ce qui change sinon |
|---|---|---|---|
| 1 | **Forme du mélange avec la table figée** | L'équipe **part de `C`** et chaque pas corrige par sa propre erreur, pondérée : `ĝ = g_T^w` (cible commune = point fixe, `g_P = 1`). Une équipe sans donnée garde `C` au bit près (§14.5.2). | Chaîne de l'équipe depuis `K` puis mélange final avec `C` : apprend contre des tables jamais servies, écartée. Le banc compare au `rt05-p3` contemporain. |
| 2 | **Plafond par équipe ou par type** | 500 **par équipe** (décision 1 du §9.4), tirage par empreinte de contenu, types confondus. Variante `capt:500` jouée au banc. | Par type : ~×1,8 de temps pour A (~11 min sur 4 fils) [calculé], GRAVEL de A mieux appris. Réglage `generator.profile.per_type`. |
| 3 | **Données de `C`** | `rc05-p3` **rejouée** aux réglages de la campagne 2 (25 % à l'écart : apprise sur 75 % des boucles), pour que la lecture « écart » du banc reste honnête ; **MTB neutre** (4 boucles, sous le seuil de 50). | Apprise sur 100 % (`--no-holdout`) pour la production : la lecture « écart » du banc voit alors des boucles de `C`. |
| 4 | **Poids de l'équipe borné** | `w[c] = min(G_T / (G_T + n0 · R), N_T / (N_T + n0))` : jamais plus que la part que ses boucles lui donnent. | Formule nue du §13.2.4 : une classe que `C` ne génère jamais (`R = 0`) prendrait `w = 1` sur une seule boucle. |
| 5 | **Normalisation** | Non, sauf si le banc la trouve meilleure sans rien perdre (règle du §14.12.4). | Normalisée d'office : jamais mesurée, change `C` et tous les profils. |
| 6 | **Où se fait la regénération** | Après l'appariement, **hors** de la place de dépôt, dans une place de profil unique (`profile.max_concurrent` = 1), 4 fils, budget 720 s. | Dans la place de dépôt : l'autre environnement perd des équipes (point 4 du §14.0). Deux places : plus de CPU la nuit. |
| 7 | **Budget dépassé** | La table des pas **finis** est écrite (1 ou 2 pas, signalé) ; aucun pas fini : l'ancien profil est gardé. La surcouche est toujours écrite. | Rien n'est écrit : nouvel essai la nuit suivante, si l'état le demande. |
| 8 | **Profil incompatible** (format, `C` ou réglages d'apprentissage changés) | Table commune servie, `stale` raison `profile` ; jamais effacé à la lecture. | Profil ancien servi quand même jusqu'au prochain dépôt. |
| 9 | **Champs d'API ajoutés** | `stale_reason: profile` ; `profile` dans l'état de la surcouche et dans le rapport de dépôt ; `taste` (`team` \| `common`) dans la réponse de génération. | Seulement `stale_reason: profile` (le strict nécessaire). |
| 10 | **Changelog** | `CHANGELOG.md` créé à la racine, à partir de 1.10.0. | Une ligne dans cette note seulement. |
| 11 | **Boucles de plus de 200 km** | Écartées de l'apprentissage (`max_km` = `generator.distance_m_max`, 200 km) : le service ne les sert pas, et leur fenêtre peut dépasser le budget mémoire. | Jouées comme au banc (fenêtres plus grandes que toute requête servie). |
| 12 | **Banc, poids de base** | `--base-taste neutral` par défaut dans toutes les sous-commandes (les relevés passés se rejouent tels quels), `likeness` toujours neutre. | `common` par défaut : les campagnes du lot 2 et du lot 6 ne se comparent plus. |

### 14.2 Où vit chaque morceau

| Morceau | Où | |
|---|---|---|
| Table commune, constantes générées | `crates/ord-loops/src/taste_common.rs` ; `cost.rs` (`Taste::common`, `common_raw`, `common_normalized`) | nouveau |
| Poids de base au goût | `ord-loops` : `engine.rs` (`Params::base_taste`, `Engine::base_weight`), `window.rs`, `team.rs`, `search.rs` | changé |
| Marche, trace, géométrie plane, longueurs par classe | `ord-graph-match` : `walk.rs`, `plane.rs` (déplacés du banc) | déplacé |
| Retenue et tirage des boucles d'apprentissage | `ord-graph-match` : `profile/collect.rs` ; collecteur facultatif dans `deposit.rs` | nouveau |
| Apprentissage (pas, mélange, bornes, `q8`), joueur abstrait, joueur à plat de référence | `ord-graph-match` : `profile/learn.rs`, `profile/mod.rs` (`Player`, `FlatPlayer`) | nouveau |
| Format en clair du profil | `ord-graph-match` : `profile/format.rs` | nouveau |
| Joueur du service, phase de profil du dépôt | `ord-server` : `generator/profile.rs`, `generator/deposit.rs` | nouveau, changé |
| Fichier chiffré, lecture, état, purge, quotas | `ord-server` : `generator/store.rs`, `generator/crypto.rs` | changé |
| Agrégats d'équipe au goût, réponse | `ord-server` : `generator/windows.rs`, `generator/generate.rs`, `generator/routes.rs` | changé |
| Réglages | `ord-server` : `config.rs` (`generator.profile`) | ajout |
| Contrat, version | `ord-server/src/lib.rs` (`API_VERSION`), `Cargo.toml` (espace de travail), `contract/openapi.json`, `CHANGELOG.md` | changé |
| Banc | `tools/ord-loops-bench` : sous-commande `taste-common`, `likeness/{variants,svc,compare_svc,mod,out}.rs` | nouveau, changé |

`ord-graph-match` gagne deux dépendances **du workspace** (`ord-loops`, `ord-loops-check`, chemins), sans cycle
(`ord-loops-check` → `ord-loops` → `ord-graph`) ; aucune dépendance hors workspace n'est ajoutée (cargo-deny inchangé).
Aucun crate nouveau : le `Dockerfile`, qui recopie la liste des manifestes, ne change pas.

### 14.3 La table commune

#### 14.3.1 Production, au banc

1. **Rapport** : `VariantMeta` gagne `class_meters: ClassMetersOut { pooled: Vec<TypeMeters> }` (trois entrées, ROAD, GRAVEL,
   MTB), `TypeMeters { ride: String, real_m: Vec<f64>, gen_m: Vec<f64>, loops: u32, pairs: u32 }` : le `ClassMeters::of` de la
   variante (déjà calculé pour `l1_after`), **toutes équipes réunies, jamais par équipe**. Un agrégat de 72 nombres par type.
2. **Rejeu de la chaîne commune** (réglages de la campagne 2) :
   `$B likeness --label lot4-3b-commune --holdout-share 0.25 --variants base,rc:0.5+n:3` (~18 min, RSS ~17 Gio). Contrôle
   (affiché au résumé et **arrêt** du rejeu s'il échoue, comme le contrôle « `base` = 4a » : le banc aurait changé) :

   | variante | empreinte | `taste_xxh3` |
   |---|---|---|
   | `base` | `4686c904975302e7` | `2d06800538d394c2` |
   | `rc05-p1` | `a123cb82e6b36fc1` | `40c8278314911d91` |
   | `rc05-p2` | `ad57882c3ed75c10` | `2f57dc10b00d143d` |
   | `rc05-p3` | `2c563b8bbe8059ae` | `66ee08d3eb9c910c` |

   Constantes `RECAL_EQUIPE_*` à côté de `BASE_4A_FINGERPRINT` ; le contrôle s'applique si les réglages sont ceux du 4a hormis
   la part mise à l'écart (`at_4a_defaults_but_holdout_share`) et `holdout_share == 0.25`.
3. **Export** : nouvelle sous-commande
   `$B taste-common --report G/lot4/lot4-3b-commune-rc05-p3-report.json --base G/lot4/lot4-3b-commune-base-report.json
   --expect-xxh3 66ee08d3eb9c910c --min-loops 50 --out crates/ord-loops/src/taste_common.rs` :
   - `q8` : les trois tables `source = recalé`, `overlay = full` ; refus si leur `xxh3` (même calcul que `taste_xxh3`)
     n'est pas `--expect-xxh3` ;
   - un type dont `class_meters.pooled.loops < --min-loops` (MTB : 4) devient **neutre** (256 partout) et prend ses mètres
     générés dans le rapport de `base` (jouée à table neutre) ;
   - `R[t][c] = round(gen_m[c] / loops × 10)`, décimètres générés par boucle (les deux curseurs sommés, convention
     `G_P / N_P` du §13.2.4), `u32` ;
   - table normalisée : `M̄_t = Σ_c R[t][c] · q8[t][c] / Σ_c R[t][c]` (en 256es), `q8n[t][c] = clamp(round(256 ·
     q8[t][c] / M̄_t), 64, 1 024)` ; inchangée si `Σ R = 0` ; c'est la fonction `profile::learn::normalize_q8` (§14.5.4),
     la même que l'apprentissage ;
   - fichier Rust écrit, **généré, à ne pas modifier à la main**, avec en tête la provenance (label, variante, empreinte,
     `taste_xxh3`, part mise à l'écart 0,25, apprise sur les boucles non mises à l'écart, commit du banc, date ; aucune donnée
     privée) :

     ```rust
     pub const COMMON_Q8: [[u16; TASTE_CLASSES]; 3];
     pub const COMMON_Q8_NORMALIZED: [[u16; TASTE_CLASSES]; 3];
     /// Décimètres générés par boucle sous `COMMON_Q8` (les deux curseurs), par type et par classe : le `R` du mélange.
     pub const COMMON_GEN_DM_PER_LOOP: [[u32; TASTE_CLASSES]; 3];
     pub const COMMON_LOOPS: [u32; 3];
     pub const COMMON_SOURCE: &str;
     ```
4. C'est un agrégat de 72 nombres par type : il est commité.

#### 14.3.2 Dans le moteur

- `cost.rs` : `mod taste_common;` ; `Taste::common_raw()`, `Taste::common_normalized()`, et **`Taste::common()`** qui rend
  l'une ou l'autre selon la constante `COMMON_USE_NORMALIZED: bool` (posée par la règle du §14.12.4, `false` jusque-là) ;
  `pub fn common_gen_dm_per_loop() -> &'static [[u32; TASTE_CLASSES]; 3]`. Entiers seulement : même résultat natif et WASM.
- `engine.rs` : `Params` gagne `pub base_taste: Taste`, défaut `Taste::common()`. `Engine::base_weight(&self, ride, a, len)
  -> u32` = `cost::taste_weight(ride, a, len, &self.params.base_taste.q8[ride.index()])` (`#[inline]`). Avec
  `Taste::neutral()`, c'est exactement `edge_weight` (§12.0 point 4).
- Les trois appels du point 2 du §14.0 passent par `base_weight` : `window.rs` (`self.base_weight`), `team.rs` (branche
  `None` de `build_with_taste` : `e.base_weight`), `search.rs` (`Weighter::edge`, branche `None` : `eng.base_weight`).
  `cost::edge_weight` reste (référence des tests), le moteur ne l'appelle plus.
- `WindowTeam::estimate_bytes(win, with_taste: bool)` : 4 octets de plus par chaîne et par type servi avec un goût ;
  `bytes()` les comptait déjà.
- Rien d'autre : format des fenêtres, cache, relaxation, contrôleur inchangés. En strict, le poids de base s'applique
  mécaniquement (il ne change que les poids relatifs des arêtes d'équipe, pas le réseau strict).
- Coût : `taste_class` par lien et par type à la construction d'une fenêtre ; temps de construction mesuré (§14.13 point 5).

#### 14.3.3 Références figées et tests dorés, re-bénis en le disant

- `tests/fixtures/window_reference.txt` est **renommée** `window_reference_192.txt` et rejouée par un test avec
  `Params { base_taste: Taste::neutral(), ..scenario_params() }`, **sans** re-bénédiction : la preuve que le changement ne
  fait rien d'autre que la table.
- Un nouveau `window_reference.txt` est béni avec le défaut (`C`), `ORD_LOOPS_BLESS=1` ; le message du commit le dit, avec le
  nombre de requêtes dont la sortie change.
- `team_reference.txt` (`TeamData`) et `ord-loops-check/tests/fixtures/team_check_reference.txt` **ne doivent pas changer**
  (aucun poids) : s'ils changent, c'est un défaut.
- Les autres tests qui figent des poids ou des parcours (`ord-loops`, `ord-server`) : `base_taste: Taste::neutral()` quand le
  test porte sur l'arithmétique ou la non-régression de 1.9.2, re-bénédiction quand il fige une sortie du service ; chacun
  est nommé dans le message du commit.
- Banc : `likeness` ouvre toujours le moteur à poids de base neutre (l'empreinte de `base` aux réglages du 4a reste
  `55c3b065dc085f42`) ; les autres sous-commandes prennent `--base-taste neutral|common`, défaut `neutral`.

### 14.4 Les boucles d'apprentissage au dépôt (`ord-graph-match`)

#### 14.4.1 Code déplacé du banc (sans effet)

- `likeness/walk.rs` entier (`Leg`, `Join`, `Walk`, `Trace`, `from_pieces`, `from_route`, `to_route`, `trace`, `start_latlon`,
  `haversine_len`) → `ord_graph_match::walk` ; les fonctions planes qu'il utilise et celles de `bearing_of` (`Pt`, `dist`,
  `length`, `cumulative`, `barycentre`, `heading_deg`) → `ord_graph_match::plane` ; `measure::class_lengths` →
  `walk::class_lengths`. Le `shape.rs` du banc les réexporte (`pub use`) : rien d'autre ne change dans le banc.
- `export::judge` est coupé en deux : la partie pure, sur `(ty, excluded, readable, total_m, matched_m, pieces)`, devient
  `profile::collect::judge_pieces(gf, &RetainParams, …) -> Result<(Walk, Trace), Refusal>` (avec `Refusal` et
  `RetainParams`) ; le `judge` du banc l'appelle.
- `play::bearing_of` et le corps de `play::request_of` → `profile::collect::loop_request(start: (f64, f64), bearing,
  distance_m, ty: u8, mode: Mode, familiarity: f32, off_team_cost_q8: u32, seed: u64) -> Request` ; le banc l'appelle avec
  `id_hash ^ seed` et `cursor::apply`, comme aujourd'hui.
- Contrôle : `$B likeness --teams C --limit 5 --label 3b-refacto` rend la même empreinte qu'au commit précédent ; les tests
  de `walk.rs` partent avec le code et passent sans changement.

#### 14.4.2 Le collecteur

```rust
pub struct RetainParams { pub min_matched: f64, pub max_hole_m: f64, pub min_km: f64, pub max_km: Option<f64>, pub loop_gap: f64 }
// Défauts du banc : 0,95 ; 1 000 ; 5 ; None ; 0,05. Le service passe max_km = distance_m_max / 1 000 (200).
pub enum Cap { Team(usize), PerType(usize), None }
pub struct LearnLoop {
    pub key: u64, pub ty: u8,
    pub start: (f64, f64), pub bearing: Option<Bearing8>, pub distance_m: f64,
    /// Mètres du réel par classe de goût (`walk::class_lengths`).
    pub real_m: [f64; TASTE_CLASSES],
}
pub struct LoopCollector { /* params, cap, tas, décomptes */ }
impl LoopCollector {
    pub fn new(params: RetainParams, cap: Cap) -> Self;
    pub fn offer(&mut self, gf: &MappedGraph, ty: u8, excluded: bool, readable: bool,
                 total_m: f64, matched_m: f64, pieces: &[Piece]);
    pub fn finish(self) -> LearnSet;
}
pub struct LearnSet { pub loops: Vec<LearnLoop> /* triées par (key, ty) */, pub counts: CollectCounts }
```

- **Retenue** : `judge_pieces`, dans l'ordre des refus du banc (exclue, illisible, type > 2, part appariée < 0,95, vide,
  trou > 1 000 m, D < 5 km, pas une boucle, D > `max_km`). Une boucle retenue donne `key`, départ (`start_latlon`), cap
  (`bearing_of`), `distance_m = max(round(d_graph / 1 000), 1) × 1 000`, `real_m` ; la marche et la trace sont lâchées.
- **Empreinte** `key = xxh3_64` des octets : `b"ordp/loop/v1"`, `ty`, puis par jambe `edge` (`u32` LE), `reverse` (`u8`),
  `lo.to_bits()`, `hi.to_bits()` (`u64` LE), puis par jonction une étiquette (0 nœud, 1 contact, 2 trou) et le nœud (`u32`)
  ou `trou.to_bits()`. Deux boucles de même empreinte ont la même marche, donc la même requête et le même `real_m`.
- **Tirage** : un tas max sur `(key, ty)` ; au-delà du plafond, la plus grande part. `Cap::Team(n)` : un tas ; `Cap::PerType(n)`
  : un par type ; `Cap::None` : tout. Le résultat est **le même quel que soit l'ordre des lignes** (test 8 du §14.11) : ni
  « les 500 premières », ni un hasard non reproductible.
- **Dans `Deposit`** : `Deposit::with_collector(gf, idx, poly, memo, collector)` ; par trace, `add_edge` accumule `total_m`,
  `matched_m` et `pieces` **exactement** comme `export::track_match` (`trv` NaN → `glen` ; `total_m += trv` pour toute arête ;
  sur `Counted`, `matched_m += trv` et `pieces.extend(chain.pieces)`) ; `end_track` appelle `offer` puis vide ; `push_track`
  et `push_matched` passent `excluded` et la lisibilité. `finish()` est inchangée ; `finish_learn(self) -> (Vec<u32>,
  Vec<u16>, DepositStats, Option<LearnSet>)` rend aussi les boucles. Sans collecteur, les tableaux de surcouche sont les
  mêmes octet pour octet (l'outil `ord-graph-overlay` ne change pas).
- **Mémoire** : au plus 500 × ~700 o, plus la marche de la trace en cours.

### 14.5 L'apprentissage (`ord_graph_match::profile::learn`)

#### 14.5.1 Paramètres et table commune

```rust
pub struct LearnParams {
    pub gamma: f64,        // 0,5
    pub steps: u8,         // 3
    pub m: f64,            // 0,5 (lissage de ratio_table)
    pub step_bound: f64,   // 3 : g dans [1/3, 3] par pas
    pub mult_bound: f64,   // 4 : multiplicateur cumulé dans [1/4, 4]
    pub n0: f64,           // 25
    pub sign_eps: f64,     // 0,02 : |ln ĝ| en deçà, pas de changement de signe
    pub normalize: bool,   // COMMON_USE_NORMALIZED
    pub cap: Cap,          // Team(500)
    pub cursors: [u32; 2], // centièmes : 25 et 100
    pub seed: u64,         // 20 261 117
    pub retain: RetainParams,
}
impl LearnParams { pub fn hash(&self) -> u64; } // xxh3 d'un texte canonique "ordp/learn/v1;gamma=0.5;…" de tous les champs
pub struct Common { pub q8: [[u16; TASTE_CLASSES]; 3], pub gen_dm_per_loop: [[u32; TASTE_CLASSES]; 3] }
impl Common { pub fn engine(normalized: bool) -> Self; pub fn hash(&self) -> u64; } // xxh3 des q8 puis des R, petit-boutiste
```

#### 14.5.2 L'état et le pas

État par type `t` : `mult[c] = C.q8[t][c] / 256` (exact en `f64`), `gamma[c] = γ`, `sign[c] = 0`. Table jouée au pas 1 :
`C` elle-même, donc le service sans profil.

**Pas `k` = 1 à `steps`** (le pas `k` joue la table `T_{k−1}`, `T_0 = C`, et rend `T_k` ; trois pas = trois générations, `C`,
`T_1`, `T_2`, comme `base`, p1, p2 au §8.1 du relevé) :

1. **Requêtes** : pour chaque boucle de `LearnSet` dans l'ordre (empreinte croissante), pour chaque curseur dans l'ordre,
   `loop_request(…, cursor::apply(h), seed = key ^ p.seed)` ; les types sans boucle n'ont pas de requête.
2. **Jeu** : `player.play_all(&reqs, &taste)` (§14.5.3).
3. **Mètres**, dans le même ordre, par type : pour chaque paire dont le jeu rend des mètres, `real[c] += real_m[c]`,
   `gen[c] += gen_m[c]`, `pairs += 1` ; une boucle avec au moins une paire compte dans `loops` (`N`).
4. Pour chaque type de `N > 0` (sinon sa table reste celle du pas précédent, `C` au départ) :
   - `g = ratio(real, gen)` : neutre (1 partout) si `Σ real ≤ 0` ou `Σ gen ≤ 0` ; sinon `p_n = real / Σ real`, `p_d = gen /
     Σ gen`, `g = (p_n + m · p_d) / ((1 + m) · p_d)` borné à `[1 / step_bound, step_bound]`, `g = 1` si `p_d = 0` (la
     `ratio_table` du banc à `min_km = 0`, même expression, même ordre d'opérations) ;
   - `w[c] = 0` si `gen[c] ≤ 0` ; sinon `min(gen[c] / (gen[c] + n0 · R[c]), N / (N + n0))`, `R[c] =
     C.gen_dm_per_loop[t][c] / 10` (mètres par boucle) ;
   - `ĝ[c]` = 1 si `w ≤ 0` ; `g[c]` si `w ≥ 1` ; sinon `exp(w · ln g[c])` ;
   - amortissement : `u = ln ĝ[c]` ; `s = 0` si `|u| < sign_eps`, sinon le signe de `u` ; si `s ≠ 0`, `sign[c] ≠ 0` et
     `s ≠ sign[c]` : `gamma[c] /= 2` ; si `s ≠ 0` : `sign[c] = s` (le `ParamState::damp` du banc) ;
   - `mult[c] = (mult[c] * ĝ[c].powf(-gamma[c])).clamp(1 / mult_bound, mult_bound)` ;
   - si `normalize` : `M̄ = Σ_c gen[c] · mult[c] / Σ_c gen[c]` (rien si la somme est nulle), `mult[c] = (mult[c] /
     M̄).clamp(1 / mult_bound, mult_bound)` ;
   - `q8[c] = (256 · mult[c]).round().clamp(1, 65 535) as u16` (dans `[64, 1 024]` de fait).
5. **Diagnostics** par type : `loops`, `pairs`, `N / (N + n0)`, `l1_gap = Σ |p_n − p_d|`, `mean_mult = Σ p_d · mult`,
   paramètres dont γ a été divisé, γ minimal, classes à la borne.

**Propriétés voulues** : une équipe sans donnée d'un type (`N = 0` ou `gen = 0`) garde `C` **au bit près** ; un ensemble vide
rend `C` sans aucun jeu ; avec `C` neutre et `n0 = 0`, un pas est le `step72` du banc sur `ratio_table(min_km = 0)` (test 10c).

```rust
pub struct Learned { pub taste: Taste, pub steps_done: u8, pub loops: [u32; 3], pub pairs: [u32; 3],
                     pub plays: u64, pub diag: Vec<StepDiag> }
pub fn learn(set: &LearnSet, cursors: &[(Mode, f32, u32)], player: &dyn Player, common: &Common,
             p: &LearnParams) -> Learned;
```

Les curseurs arrivent **déjà appliqués** (`ord-graph-match` ne dépend pas d'`ord-server` : l'appelant passe
`cursor::apply(h)` pour `h` de `p.cursors`). **Partiel** : si `play_all` rend `Err(Stopped)` au pas `k`, le résultat est
`T_{k−1}`, `steps_done = k − 1` ; `loops` et `pairs` sont ceux du dernier pas fini.

#### 14.5.3 Le joueur

```rust
pub struct Stopped;
pub trait Player: Sync {
    /// Joue chaque requête avec `taste` passé au crochet (`WindowTeam::build_with_taste(…, Some(taste))`) sur la surcouche
    /// du dépôt ; une `WindowEscape` est rejouée une fois en fenêtre élargie ; les parcours rendus passent au contrôleur
    /// indépendant dans l'ordre du moteur ; rend les mètres par classe du premier gardé, `None` sinon. Pas de budget de
    /// temps par requête. Le résultat `i` est celui de `reqs[i]`, quel que soit le nombre de fils.
    fn play_all(&self, reqs: &[Request], taste: &Taste) -> Result<Vec<Option<[f64; TASTE_CLASSES]>>, Stopped>;
}
/// Le premier parcours gardé : `walk::class_lengths` de `Walk::from_route`. Partagé par tous les joueurs.
pub fn first_kept_class_meters<B: AsRef<[u8]>>(gf: &MappedGraph, checker: &Checker<'_, B>, req: &Request,
                                               g: &Generation) -> Option<[f64; TASTE_CLASSES]>;
/// Joueur de référence : un fil, fenêtre entière par requête. Pour les tests du service et du banc.
pub struct FlatPlayer<'a, B> { /* engine, TeamData, Checker */ }
```

Deux autres joueurs : `ServicePlayer` dans `ord-server` (§14.7.3) et `BenchPlayer` dans le banc (§14.12.2). Tous rendent les
mêmes résultats que `FlatPlayer` (tests 23) : c'est ce qui fait du banc un juge du chemin du service.

#### 14.5.4 Normalisation, déterminisme

- `pub fn normalize_q8(q8: &[u16; TASTE_CLASSES], weights: &[f64; TASTE_CLASSES], bound: f64) -> [u16; TASTE_CLASSES]` : la
  normalisation de `C` par l'outil d'export (poids `R`). Le pas normalise les `mult` en `f64` (étape 4), pas les `q8`.
- Sommes séquentielles dans un ordre fixé, `f64` dans l'apprentissage seulement, `q8` entiers en sortie, parallélisme
  seulement dans le joueur, à résultats indexés : **même binaire, même machine → mêmes octets, quel que soit le nombre de
  fils**. D'une plateforme à l'autre, `ln`, `powf` et `exp` de la libm peuvent différer d'une unité du dernier rang : un `q8`
  peut alors différer de 1, rarement. Accepté : le profil est calculé là où il est servi.

### 14.6 Le fichier de profil

**Nom** `<team>.ordp.enc`, à côté de `<team>.ordo.enc` (`graph-<id>/<ns>/`) ; temporaire `<team>.ordp.tmp-<16 hex>`.

**En clair, format 1** (`ord_graph_match::profile::format`) :

```
u32 LE                longueur du JSON
JSON                  ProfileMeta { format: 1, learned_at: i64, overlay_deposited_at: i64, common: "<16 hex>",
                                    learn: "<16 hex>", engine: "1.10.0", steps: u8, loops: [u32; 3], pairs: [u32; 3],
                                    normalized: bool }
3 × 72 × u16 LE       q8 (ROAD, GRAVEL, MTB)
```

- **Lecture** : `Incompatible(Format)` si le JSON est illisible, `format ≠ 1`, la longueur des tables ≠ 432 octets ou un
  `q8` nul ; `Incompatible(Settings)` si `common ≠ Common::engine(normalize).hash()`, si `learn ≠ LearnParams` courants (dont
  le plafond du réglage) ou si `steps = 0` alors que `loops ≠ [0, 0, 0]` (jamais écrit, refusé quand même).
- **Chiffrement** : `Credentials::seal_with(magic, graph_id, plain)` / `open_with`, avec `MAGIC_PROFILE = b"ORDPENC1"` ;
  `seal` / `open` deviennent des appels avec `MAGIC` (`ORDOENC1`), octet pour octet comme aujourd'hui. Octet de version de
  l'enveloppe : 1. Même clé, même nonce aléatoire.
- **Taille** : ~0,7 Ko. Un format 2 (formes du 4c, destinations du 4d) montera `format` ; un format inconnu est
  `Incompatible(Format)` : table commune et `stale` raison `profile` (§14.8), jamais effacé à la lecture.

### 14.7 Le dépôt dans le service

#### 14.7.1 Phases

1. **Appariement**, comme aujourd'hui, dans la place de dépôt, avec `Deposit::with_collector(…, LoopCollector::new(retain,
   cap))` si les profils sont actifs (`retain.max_km = distance_m_max / 1 000`).
2. Après la ligne de fin : `finish_learn`, encodage `ordo`, `Overlay::decode_for(&ordo, identity)` ; l'index d'appariement
   (`CellIndex` ou `SegIndex`) et la mémoïsation sont lâchés.
3. **La place de dépôt est rendue** : le `OwnedSemaphorePermit` passe dans `Worker` et y est lâché ici. Le verrou de l'équipe
   reste tenu jusqu'à la réponse (aucun second dépôt de l'équipe ; l'environnement ne peut toujours pas tenir deux places).
4. **Phase de profil** (profils actifs et au moins une boucle retenue) :
   - échéance = min(début de la phase + `profile.budget_s`, échéance du dépôt − 30 s) ;
   - une place `profile_slots` (`profile.max_concurrent`) prise depuis le fil bloquant, attente bornée par l'échéance et
     coupée par `stopping` (`Handle::block_on` sous `timeout`, ou `try_acquire` toutes les 50 ms) ; pas de place à
     l'échéance : issue `skipped` ;
   - `TeamData` et `TeamCheck` de la **nouvelle** surcouche, comme `ready_build` (réservation `reserve_build` des deux plans,
     constructions en parallèle, réservation rendue), puis `hold_team(octets)` sous un numéro neuf, rendu en fin de phase ;
     `TeamZoneEscape` : issue `skipped` ;
   - `learn(…, &ServicePlayer, &Common::engine(normalize), &params)` ;
   - les agrégats d'équipe au goût de chaque pas sont lâchés (`drop_team`).
5. **Écritures** (§14.7.4), puis réponse.

Ensemble vide (aucune boucle retenue : équipe hors région, que des allers simples…) : aucun jeu, profil = `C`, `steps = 0`,
`loops = [0, 0, 0]`, écrit (l'état devient `ready`, le backend ne redépose pas pour rien).

#### 14.7.2 Réglages `generator.profile`

| Clé | Défaut | Rôle |
|---|---|---|
| `enabled` | `true` | `false` : ni collecteur, ni profil écrit, ni lu ; l'état n'est jamais `stale` raison `profile` |
| `threads` | 4 | fils du joueur, de 1 à 16 |
| `max_concurrent` | 1 | places de profil, tous environnements |
| `budget_s` | 720 | budget de la phase, sous `deposit.budget_s` |
| `max_loops` | 500 | plafond (dans `learn`) |
| `per_type` | `false` | plafond par type au lieu de par équipe (dans `learn`) |

Validation comme `deposit` (`config.rs`) ; variables `ORD_GENERATOR__PROFILE__*`. La normalisation n'est pas un réglage :
elle va avec `C` (`COMMON_USE_NORMALIZED`).

#### 14.7.3 Le joueur du service (`generator/profile.rs`)

- `ServicePlayer { ready, team: &TeamData, check: &TeamCheck, threads, stop }` ; à chaque `play_all`, un numéro d'agrégat neuf
  (`NEXT_SERIAL`) pour la table jouée, `threads` fils (`std::thread::scope`), un indice partagé (`AtomicUsize`), un
  `Workspace` par fil (`take_workspace` / `give_workspace`), résultats rangés par indice.
- Une requête : `engine.plan_with(req, window_radius_step_m, extra)` → `windows.acquire(engine, &plan, Some(TeamRef { serial,
  data, taste: Some(taste) }), échéance, stop)` → `generate_in` ; `WindowEscape` : une reprise avec `extra =
  longest_chain_m(vi) + 10 000` (la règle de `generate_windowed`) ; contrôleur construit comme dans `generate.rs`
  (`Checker::with_tables`) → `first_kept_class_meters`.
- `AcquireError::Overloaded` : le joueur **attend** et reprend jusqu'à l'échéance (le profil n'est pas une requête
  d'utilisateur) ; échec du moteur : `None` ; panique : attrapée (`catch_unwind`, comme le banc), `None`, compteur
  `ord_profile_play_panics_total` (attendu à 0).
- Fin de `play_all` : `windows.drop_team(serial)` (nouvelle méthode de `WindowManager`, sur `WindowCache::drop_team`).
- `stop` = `stopping` ou échéance dépassée : `Err(Stopped)`.

#### 14.7.4 Écritures, ordre, échecs

`Store::commit_with_profile(c, meta, ordo, profile: Option<&[u8]>)` (`commit` l'appelle avec `None`) :

1. scelle la surcouche et, s'il y en a un, le profil (`MAGIC_PROFILE`) ;
2. quota : surcouche comme aujourd'hui, **plus** les octets du profil (les octets d'un espace comptent désormais les
   `.ordp.enc` ; le nombre de surcouches ne les compte pas) ;
3. profil : temporaire, `fsync`, `rename` vers `<team>.ordp.enc` ;
4. surcouche : temporaire, `fsync`, `rename` (le crochet `fail_commit` agit ici, après le profil) ;
5. `fsync` du dossier, `forget`, jauges.

| Issue du profil | Fichier écrit | Ancien profil |
|---|---|---|
| `learned` (3 pas) | oui | remplacé |
| `partial` (1 ou 2 pas : budget, arrêt du service) | oui, `steps` < 3 | remplacé |
| `empty` (aucune boucle) | oui (`C`, `steps = 0`) | remplacé |
| `skipped` (aucun pas fini : pas de place, budget au pas 1, arrêt, échappement de construction) | non | gardé s'il existe |
| profils désactivés | non | gardé, non lu |

La surcouche est écrite dans **tous** les cas où elle l'est aujourd'hui : un échec du profil ne fait jamais échouer un dépôt.

#### 14.7.5 Rapport, journaux, métriques

- `DepositReportV1` gagne `profile: Option<DepositProfileV1>` (absent si désactivé) : `{ outcome: learned | partial | empty |
  skipped, steps: u8, loops: u32, plays: u64, seconds: f64 }`.
- Journal `info` (`ns8` seulement) : issue, pas, boucles par type, jeux, durées (attente de place, phase), et par pas et
  par type `l1_gap` et `mean_mult` (agrégats). Jamais de coordonnée, de clé, de nom.
- Métriques : `ord_profile_builds_total{outcome}`, `ord_profile_build_seconds`, `ord_profile_slot_wait_seconds`,
  `ord_profile_plays_total`, `ord_profile_play_panics_total`, jauge `ord_profiles_stored`.

#### 14.7.6 Mémoire

Tout ce que la phase tient de gros passe par le budget unique (`memory_budget_mb`) : fenêtres, agrégats d'équipe au goût
(`estimate_bytes(win, true)`), quatre espaces de travail de plus, `TeamData` et `TeamCheck` de la nouvelle surcouche
(`reserve_build`, puis `hold_team`). Hors budget : l'ensemble d'apprentissage (≤ 1 Mo), les résultats (≤ 1 000 × 576 o) et
les mètres. [estimé] Pic anonyme : celui du lot 6 à 4 clients (1,1 Go) plus ~0,1 Go ; mesuré au §14.13. Une phase de profil
de jour (dépôt manuel par l'administration) peut prendre des places du budget aux générations et faire monter les 503 :
accepté, signalé.

### 14.8 Lecture en service, repli, état

- `Entry` gagne `profile: Option<Taste>` et `profile_id: Option<FileId>` ; `cached()` compare l'empreinte de la surcouche
  **et** celle du profil (absent compris) : un profil remplacé recharge l'entrée.
- `ready_build`, après la surcouche : `read_profile(c) -> ProfileRead { Present(Taste, ProfileMeta), Absent,
  Incompatible(Kind) }` (un déchiffrement qui échoue est `Incompatible(Format)`) ; `Present` → `Some`, sinon `None`. **Jamais**
  d'effacement à la lecture (un format plus récent peut venir d'une autre instance pendant un retour arrière).
- `WindowManager::acquire` reçoit `TeamRef { serial, data, taste: Option<&Taste> }` et construit
  `WindowTeam::build_with_taste(engine, &win, data, taste)` ; la clé (fenêtre, numéro) reste juste (un profil par entrée).
  Sans profil : `build` (poids de base `C`). Sans équipe : `w_osm` de la fenêtre (`C`).
- `status(c)` : l'état de la surcouche d'abord, comme aujourd'hui ; s'il est `Ready` et que les profils sont actifs : profil
  `Present` → `Ready`, sinon **`Stale(StaleReason::Profile)`**. Les raisons `graph` et `matcher` priment.
- `ready()` **ne rend jamais** `Stale(Profile)` : un profil absent ne bloque aucune génération (pas de 409). Dans
  `generate.rs`, le bras `StaleReason::Profile` est écrit pour l'exhaustivité, commenté « jamais rendu par `ready()` », et un
  test le tient.
- Contrat : `StaleReasonV1::Profile` (`"profile"`) ; `OverlayStatusV1.profile: Option<ProfileStatusV1 { state: present |
  absent | incompatible, learned_at, steps, loops }>` (absent si la surcouche l'est ou si les profils sont désactivés) ;
  `GenerateResponseV1.taste: common | team` (`team` si l'entrée de l'équipe porte un profil). Doc de `OverlayStateV1::Stale`
  : « … or without a usable taste profile ».

### 14.9 Purge, suppression, quotas, déploiement roulant

Store de 1.10 :
- `PROFILE_EXT = "ordp.enc"` ; `is_profile_name` : finit par `.ordp.enc` et ne contient pas `.tmp-` ; `is_overlay_name`
  inchangé (ne reconnaît pas un profil).
- **Purge du graphe courant** : temporaires et surcouches comme aujourd'hui (la règle des temporaires couvre ceux du
  profil) ; un profil part s'il est plus vieux que le TTL, **ou** s'il n'a pas de `<team>.ordo.enc` voisin et date de plus de
  `TMP_MAX_AGE` (1 h : orphelin d'une suppression par une 1.9.x, ou d'un premier dépôt dont la surcouche n'a pas pu s'écrire).
  `PurgeReport` gagne `profile_ttl` et `profile_orphan` (raisons de `ord_overlay_purged_total`). Autres graphes : inchangé.
- `delete(c)` retire les deux fichiers sous tous les graphes ; `delete_namespace` inchangé.
- Quotas : octets d'un espace = surcouches + profils ; nombre de surcouches inchangé.

| Pendant un déploiement roulant ou un retour arrière | Ce qui se passe |
|---|---|
| purge d'une 1.9.x (démarrage ou périodique) | profil gardé (point 8 du §14.0) ; temporaire d'un profil effacé seulement après 300 s ou 1 h (son écriture dure des millisecondes) |
| génération par une 1.9.x | surcouche lue, profil ignoré : `K` brut, le temps du déploiement |
| dépôt reçu par une 1.9.x | surcouche nouvelle, ancien profil gardé ; 1.10 le sert (taste de l'équipe, un peu ancien) et dit `ready` |
| suppression par une 1.9.x | profil orphelin, retiré par la première purge de 1.10 après 1 h |
| profil d'un format plus récent lu par 1.10 | `Incompatible(Format)` : `C`, `stale` raison `profile`, fichier gardé ; le redépôt de la nuit le réécrit en format 1 |
| premier démarrage de 1.10 | toutes les surcouches `stale` raison `profile` : le backend redépose tout la nuit suivante (~10,5 min de profils par environnement [calculé]) ; les générations servent `C` d'ici là |

**Conclusion** : aucune parade nécessaire ; les deux conditions du point 8 du §14.0 sont tenues par un test (15).

### 14.10 Versions, contrat, changelog

- Paquet 1.9.2 → **1.10.0** (espace de travail), `API_VERSION` `"1.9"` → **`"1.10"`**, calés comme jusqu'ici. Le moteur
  change de comportement (poids de base `C`) : le message du commit et le changelog le disent.
- Contrat : **ajouts seulement** (§14.7.5, §14.8) ; `contract/openapi.json` régénéré par le test `contract` existant ;
  `/v1/capabilities` inchangé.
- `CHANGELOG.md` à la racine, à partir de 1.10.0 (rubriques « Ajouté », « Changé », « Compatibilité » : déploiement roulant
  sûr, backend inchangé, première nuit de redépôt complet ; une ligne renvoie aux notes pour les versions antérieures).

### 14.11 Tests (synthétiques, aucune donnée de production)

**`ord-loops`**
1. Table commune : tous les `q8` dans `[64, 1 024]`, MTB neutre, empreintes figées de `COMMON_Q8` et `COMMON_Q8_NORMALIZED`,
   `Taste::common()` égal à la table choisie par `COMMON_USE_NORMALIZED`.
2. Poids de base neutre → `window_reference_192.txt` **sans** re-bénédiction ; défaut → `window_reference.txt` re-bénie.
3. **Neutralité du chemin sans profil vis-à-vis de la table commune**, sur le scénario de référence (deux zooms, trois modes,
   avec équipe) : (a) moteur `C` + `build(None)` ≡ moteur neutre + `build_with_taste(Some(&Taste::common()))` : mêmes
   `ChainTeam`, `w_osm` = `w_taste` chaîne par chaîne, mêmes sorties (`Debug`) ; (b) moteur `C` + `build(None)` ≡ moteur `C`
   + `build_with_taste(Some(&Taste::common()))` ; (c) sans équipe : `w_osm` d'une fenêtre du moteur `C` = Σ `taste_weight(C)`
   de ses liens.
4. `base_weight` à table neutre = `edge_weight` (toutes classes, longueurs du test 2 du §12.8).
5. `estimate_bytes(win, true)` ≥ `bytes()` d'un `WindowTeam` construit avec un goût.
6. `team_reference.txt` inchangée.

**`ord-graph-match`**
7. Les tests de `walk.rs` déplacés passent tels quels.
8. Collecteur : chaque refus du banc sur des morceaux synthétiques ; `Cap::Team(n)` garde les `n` plus petites empreintes
   quel que soit l'ordre d'arrivée (trois ordres mélangés → même `LearnSet`) ; `Cap::PerType` ; empreinte égale pour deux
   marches égales, différente pour la marche inverse ; le tas ne dépasse jamais le plafond.
9. `Deposit` avec collecteur : tableaux de surcouche identiques à ceux sans collecteur ; `push_track` et `push_matched`
   rendent le même `LearnSet`.
10. Apprentissage avec un joueur jouet (`gen ∝ disponible · mult^−ε`, comme le test 4 du §13.9) : (a) `w = 0` → `q8` = `C`
    au bit près ; (b) ensemble vide → `C`, `steps_done = 0`, joueur jamais appelé ; (c) dans le banc (qui a `chain.rs`) :
    `C` neutre, `n0 = 0`, un pas → les `q8` de `step72(ratio_table(min_km = 0))` ; (d) trois pas : `l1_gap` final sous la
    moitié de l'initial, γ de la classe élastique divisé au pas 2 et pas les autres ; (e) `mult` toujours dans `[1/4, 4]` ;
    (f) normalisé : `Σ gen · mult / Σ gen` = 1 à 1e-12 avant borne ; (g) joueur qui s'arrête au pas 2 → `steps_done = 1`,
    table `T_1` ; (h) `R = 0` pour une classe → `w = N / (N + n0)`.
11. Format : aller-retour en clair ; `format = 2` → `Incompatible(Format)` ; tables tronquées ; `q8` nul ; empreinte de `C` ou
    de réglages différente → `Incompatible(Settings)`.
12. `LearnParams::hash` change avec chaque champ ; `Common::hash` figée.

**`ord-server`**
13. Chiffrement : un profil scellé `ORDPENC1` ne s'ouvre pas comme surcouche, ni l'inverse ; les tests existants de
    `seal` / `open` passent inchangés.
14. Magasin : `commit_with_profile` puis `read_profile` → `Present`, mêmes `q8` ; `fail_commit` → profil nouveau, surcouche
    ancienne, erreur de dépôt ; format inconnu → état `stale` raison `profile`, fichier gardé, génération à `C` ; profil
    absent → `stale` raison `profile` ; désactivé → `ready` ; surcouche d'un autre graphe avec profil → `stale` raison
    `graph`.
15. Purge : profil au TTL ; orphelin de plus d'1 h retiré, de moins d'1 h gardé ; temporaire de profil retiré à son âge ;
    **règles de 1.9.2 recopiées telles quelles dans le test** (`is_tmp_name`, `is_overlay_name` de `develop`) : aucune ne
    reconnaît `<team>.ordp.enc`, la seconde ne reconnaît pas `<team>.ordp.tmp-…` ; `delete` retire les deux ; quota compte le
    profil.
16. Dépôt de bout en bout sur le graphe synthétique (`testkit`, `min_km` abaissé par un crochet de test caché) : profil
    `learned`, 3 pas, fichier présent, état `ready`, génération `taste: team` ; aucune boucle → `empty`, `C`, `ready`.
17. **Déterminisme** : le même dépôt avec `threads` 1 et 4 → mêmes `q8`, `loops`, `pairs` (le reste du JSON hors
    `learned_at`).
18. Budget minuscule → `partial` ou `skipped`, surcouche écrite, ancien profil gardé sur `skipped`.
19. Place : une phase de profil retenue par un crochet de test n'empêche pas le dépôt d'un autre espace (pas de 503).
20. Génération : équipe sans profil ≡ équipe dont le profil vaut `C` (mêmes parcours ; seul `taste` diffère).
21. Contrat régénéré ; validation des réglages.

**`ord-loops-bench`**
22. `parse_variants` : `svc`, `common`, `norm`, `cap`, `capt` et leurs refus (§14.12.1).
23. `BenchPlayer` ≡ `FlatPlayer` sur la fixture du banc ; `ServicePlayer` ≡ `FlatPlayer` sur le graphe synthétique du
    service (test côté `ord-server`).
24. Le test 10c.
25. `taste-common` sur un rapport synthétique : refus d'empreinte, MTB neutralisé sous `--min-loops`, arrondi de `R`, table
    normalisée, fichier qui compile (il est inclus par `ord-loops`).

### 14.12 Le banc : chemin du service, normalisation, plafond

#### 14.12.1 Variantes

| Composant | Rôle | Nom |
|---|---|---|
| `common` | `C` (brute, ou normalisée avec `norm`) passée à toutes les équipes, deux surcouches | `common` |
| `svc` | par équipe, le profil appris **par le code du dépôt** (§14.12.2), joué sur les deux surcouches | `svc` |
| `norm` | avec `common` ou `svc` : `C` normalisée et pas normalisés | suffixe `-norm` |
| `cap:<n>` | avec `svc` : plafond par équipe `n` (0 : aucun) ; défaut 500 | suffixe `-cap<n>` |
| `capt:<n>` | avec `svc` : plafond par type `n` | suffixe `-capt<n>` |

Refus (avant toute phase lourde) : `norm` sans `common` ni `svc` ; `cap` ou `capt` sans `svc` ; les deux ensemble ; `svc` ou
`common` avec un autre goût, `fam` ou `famroad` ; noms en double. `base` reste requise plus tôt dans la liste (pour
`S_base`).

#### 14.12.2 Déroulé de `svc`

Par équipe, dans l'ordre des lettres :
1. **Dépôt d'apprentissage** : `holdout::build_overlay` avec un collecteur, sur les traces **non mises à l'écart** (la
   surcouche « écart ») ; une équipe sans boucle mise à l'écart prend toutes ses traces (surcouche complète). Mêmes
   `RetainParams` que le service (`max_km` 200).
2. `TeamData` et contrôleur de cette surcouche (ceux que `build_team` fait déjà pour les jeux « écart »).
3. `learn(set, cursors, &BenchPlayer { … }, &Common::engine(norm), &params)` : `BenchPlayer` joue par `SharedWindows` avec
   une clé d'équipe neuve par pas (au-delà des `2t`, `2t + 1`), `drop_team` après chaque pas, `--threads` fils.
4. Le profil sert aux jeux de l'équipe sur la surcouche complète (« toutes ») et sur la surcouche « écart » (« écart ») : la
   structure de `rt` (une table par équipe pour les deux surcouches, apprise sans les boucles mises à l'écart), donc
   comparable à `rt05-p3`. Différences connues : plafond, cible figée, surcouche d'apprentissage « écart » (plus honnête que
   la complète de `rt`), D ≤ 200 km.

`VariantMeta` gagne `svc: Option<SvcMeta>` : `common` et `learn` (empreintes), `cap`, `normalize`, et par équipe (lettre)
boucles par type, `steps_done`, jeux, `learn_wall_s`, `learn_cpu_s` (Σ des ms des jeux), `est_4_threads_s` = `learn_cpu_s /
4` [calculé], `l1_gap` et `mean_mult` par pas et par type. `taste_xxh3` couvre les tables `svc`.

#### 14.12.3 Comparaison (`compare_svc.rs`, section « 3b » de `<label>-compare.{md,json}`)

1. `S / S_base`, toutes équipes, groupes ROAD 0,25 · ROAD 1 · GRAVEL 0,25 · GRAVEL 1, lectures « toutes » et « écart » :
   `rt05-p3`, `common`, `common-norm`, `svc`, `svc-norm`, `svc-cap0`, `svc-capt500`, et une ligne de rappel **seconde sonde**
   (constantes) : `rt05-p3` toutes 0,244 · 0,195 · 0,371 · 0,259, écart 0,203 · 0,183 · 0,379 · 0,286 ; `rc05-p3` toutes
   0,255 · 0,194 · 0,377 · 0,303.
2. A et B, mêmes variantes, « toutes » et « écart » ; B ROAD en tête, rappel `rt05-p3` 0,80 · 0,93.
3. Temps d'apprentissage par équipe ; estimation sur 4 fils contre le budget de 720 s.
4. Service (§7 actuel) par variante ; `mean_mult` par type pour `C`, `C` normalisée et les profils de A et B.
5. Lecture, en faits : la règle du §14.12.4 appliquée, et les points du §14.12.5.

#### 14.12.4 Normalisation : la règle

**Normalisée retenue** si, toutes équipes, lectures « toutes » et « écart », dans les 4 groupes, pour les deux paires
(`common-norm` contre `common`, `svc-norm` contre `svc`) : `S / S_base` normalisée ≤ brute + 0,02 partout, **et** ≤ brute −
0,02 dans au moins un groupe, **et** B ROAD (`svc-norm`, « toutes ») < 1,0 aux deux curseurs, **et** service pas pire (P50 ≤
1,10 × brute à chaque curseur, part `OK` ≥ brute − 0,3 point, ni `PANIC` ni `WINDOW_ESCAPE`). **Sinon brute**, la
configuration mesurée. Le codeur applique la règle, pose `COMMON_USE_NORMALIZED` en conséquence au commit 6 du §14.14 et
l'écrit au relevé ; le propriétaire peut la renverser.

#### 14.12.5 Lecture du chemin du service

Faits à donner, avec les seuils proposés : (a) « toutes » et « écart », les 4 groupes ≤ 0,5 ; (b) B ROAD « toutes » < 1,0
aux deux curseurs ; (c) `svc` ≤ `rt05-p3` (même campagne) + 0,05 par groupe en « toutes » (marge du plafond et de la cible
figée) ; (d) A : `svc − common` ≤ +0,03 en ROAD (« toutes ») — le profil ne dégrade pas A contre la table commune ; (e)
signaux de service du §12.6 contre `base`. Si (a) ou (b) échoue, le codage continue sur la branche (rien n'est déployé) et le
relevé le dit en tête : la mise en service revient au propriétaire.

### 14.13 Campagnes et mesure en conteneur

**Banc** (`B=./target/release/ord-loops-bench`, une campagne à la fois, sorties sous
`~/.cache/osm-route-details/graph/lot4/` seulement) :

0. Après le commit 3 : `$B likeness --teams C --limit 5 --label 3b-refacto` (même empreinte qu'avant le déplacement).
1. Après le commit 1 : la chaîne commune et l'export du §14.3.1.
2. Essai : `$B likeness --teams B,C --limit 20 --holdout-share 0.25 --variants base,common,svc,svc+norm --label 3b-essai`.
3. Déterminisme : `$B likeness --teams B,C --holdout-share 0.25 --variants base,svc --threads 1 --label 3b-det1`, puis
   `--threads 16 --label 3b-det16` : mêmes empreintes et mêmes `taste_xxh3`.
4. Campagne : `$B likeness --label lot4-3b --holdout-share 0.25
   --variants base,rt:0.5+n:3,common,common+norm,svc,svc+norm,svc+cap:0,svc+capt:500` ; contrôles affichés (sans arrêt) :
   `base` = `4686c904975302e7`, `rt05-p3` = `451651c82f8a4b92` / `3fc8e87f9b9fbfcf` (seconde sonde). [estimé] ~70 min, RSS ≤
   21 Gio. **Commande retenue après les décisions du §9.5, qui ajoute la forme `sonde` dans la même campagne** : voir le
   §14.16, point 29.
5. Après le commit 6 : le banc région du lot 6 (`--compare --self-check --budget-mb 1000`) avec `--base-taste common` :
   0 différence fenêtre contre graphe entier ; temps de construction des fenêtres contre `--base-taste neutral`.

**Conteneur, 3 Gio** (après le commit 7 ; méthode des §9 et §10 de `ord-loops-lot6-mesures.md`) :
- `docker build -t osm-route-details:lot4-3b .` (jamais poussée) ; et, depuis un `git worktree` de `develop`,
  `osm-route-details:1.9.2-local`.
- `docker run --memory=3g --memory-swap=3g`, ports sur `127.0.0.1`, fichiers France en lecture seule, volume nommé neuf,
  `ORD_CACHE__OFFLINE=true`, IGN désactivé, réglages par défaut (profils : 4 fils) ; cache de pages évincé avant chaque
  démarrage ; échantillons toutes les 0,5 s de `memory.current`, `memory.peak`, `anon` / `file` (`memory.stat`),
  `memory.events`, `docker stats` toutes les 10 s ; sorties privées sous `G/lot4/3b-conteneur/`.
- Séance : (1) **18 dépôts** (corps NDJSON privés de `G/lot3a/deposit/`, deux clés d'espace tirées au hasard par le script et
  jamais écrites, les deux clés **en parallèle** comme staging et production à 02:10, `curl -T` avec `Expect: 100-continue`) :
  statut, durée et `profile` de chaque rapport ; (2) les 18 états : `ready`, `profile.state = present` ; (3) générations : 54
  réseaux en série, 114 à 4 clients (la séance du lot 6), `taste: team` ; (4) un dépôt de A pendant 4 clients de génération ;
  (5) redémarrage : états `ready`, purge de démarrage, nombre de `.ordp.enc` du volume inchangé (compté par un conteneur
  annexe, nombres seulement) ; (6) **compatibilité** : 1.9.2 sur le même volume, purge de démarrage puis périodique
  (`ORD_GENERATOR__PURGE_INTERVAL_S=10`), nombre de `.ordp.enc` inchangé, générations servies ; retour en 1.10 : `ready` ;
  (7) dépôt de A avec `docker update --cpus=4`.
- Critères proposés : `oom` 0 et `memory.events max` 0 ; pic `anon` ≤ 2,4 Gio (80 % de 3 Gio) ; tous les dépôts en 200 ;
  profil de A `learned` (3 pas) dans le budget par défaut ; pas plus d'un 503 par équipe pour l'autre clé (le backend en
  refait trois) ; 1.9.2 garde tous les profils.
- Relevé `docs/design/technique/ord-loops-lot4-3b-mesures.md` : lettres et agrégats seulement (correspondance `team-N` →
  lettre lue dans le fichier privé des lettres).

### 14.14 Commits

Chacun passe `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` et les tests des crates touchés ; avant
le dernier, `cargo test --workspace`. Fin de message : une ligne vide, puis la ligne `Co-Authored-By` du modèle qui code.

1. `feat(bench): likeness, mètres par classe au rapport et export de la table commune (lot 4, 3b)` — `class_meters`,
   contrôles `RECAL_EQUIPE_*`, sous-commande `taste-common`, test 25. Puis campagne 1 du §14.13 et export.
2. `feat(loops): table de goût commune figée, sans effet sur les poids (lot 4, 3b)` — `taste_common.rs` généré, `Taste::common*`,
   `common_gen_dm_per_loop`, test 1.
3. `refactor(graph-match): marche, trace et retenue des boucles partagées avec le banc` — §14.4.1, test 7, contrôle 0.
4. `feat(graph-match): profil de goût par regénération, boucles du dépôt, apprentissage et format` — §14.4.2, §14.5, §14.6
   (en clair), `FlatPlayer`, tests 8 à 12.
5. `feat(bench): likeness, chemin du service (svc, common, norm, cap, capt) et comparaison du 3b` — §14.12, tests 10c, 22,
   23 (banc). Puis campagnes 2 à 4 ; règle de la normalisation.
6. `feat(loops): poids de base à la table commune (changement de comportement assumé, références re-bénies)` — §14.3.2,
   §14.3.3, `COMMON_USE_NORMALIZED` posé par la règle, `--base-taste` du banc, tests 2 à 6 ; le message nomme chaque fixture
   et chaque test re-béni. Puis le point 5 des campagnes.
7. `feat(server): profil d'équipe appris au dépôt, fichier .ordp.enc, état stale « profile » (api 1.10, 1.10.0)` — §14.6 à
   §14.10, `CHANGELOG.md`, contrat, tests 13 à 21 et 23 (service).
8. Mesure en conteneur, puis `docs(ord-loops): relevé du 3b, chemin du service et mémoire en conteneur (lot 4)` — relevé
   `ord-loops-lot4-3b-mesures.md` (style des relevés précédents, marques de l'en-tête), §14.16 « Écarts du codage » ajouté
   ici, et le point 3 du §10 marqué fait.

### 14.15 Ce que le 3b ne fait pas

Rien au backend de Pédalons ; ni `params_hash` de l'appariement, ni format du graphe et des vues, ni contrôleur
(`ord-loops-check`) ; pas de formes ni de destinations (4c, 4d) ; familiarité à 0,5 partout (décision 4 du §9.4) ; pas de
table par curseur ; pas de saut de l'apprentissage quand l'empreinte des traces n'a pas changé (décision 2 du §9.4 : recalcul
à chaque dépôt) ; aucune dépendance hors workspace ; **rien n'est poussé, aucune image publiée, aucun déploiement**, aucun
accès à la production.

### 14.16 Écarts du codage

Écrits au commit 8, après le codage ; chaque écart dit ce qui a été fait à la place et pourquoi. Les mesures sont dans
[`ord-loops-lot4-3b-mesures.md`](ord-loops-lot4-3b-mesures.md).

**Ordre et découpage des commits**

1. **Les commits 2 et 3 sont permutés** dans l'historique : le refactor des morceaux partagés (`refactor(graph-match)`) est
   avant la table commune (`feat(loops)`). Le rejeu de la chaîne commune (18 min, 17 Gio) tournait pendant le codage du
   refactor et la table n'existait pas encore ; le contenu de chaque commit est celui de la spécification. Les deux ne se
   touchent pas (le 3 ne change que `ord-graph-match` et le banc, le 2 que `ord-loops`).
2. Le commit 6 porte aussi `--base-taste` du banc du lot 2 et le moteur neutre de `likeness`, comme prévu au §14.3.3 ; le
   commit 5 compile sans eux (retirés puis remis à la main, le moteur n'ayant pas encore `base_taste`).
3. Le commit 7 contient le contrat régénéré, `CHANGELOG.md` et les tests du service ; les tests de stockage du service qui
   comptaient « un fichier par dépôt » comptent désormais séparément surcouches et profils (`World::overlays`,
   `World::profiles`), et le dépôt « disque plein » laisse le **profil neuf** à côté de l'ancienne surcouche : c'est l'ordre
   voulu du §14.7.4 (profil d'abord), la date d'apprentissage du profil change, rien d'autre.

**Table commune**

4. `COMMON_LOOPS` vaut `[1545, 553, 3]` : le MTB a **3** boucles comptées dans le rejeu, là où le §14.0 point 10 en lisait 4 dans
   le rapport privé de la campagne 2 (écart non investigué : le MTB est neutre dans les deux cas, sous le seuil de 50). Les
   quatre contrôles `RECAL_EQUIPE_*` (`base`, `rc05-p1`, `rc05-p2`, `rc05-p3`) sont **passés aux bits près** : empreintes de
   résultat et `taste_xxh3` identiques à la campagne 2.
5. L'en-tête de `taste_common.rs` porte « commit ad94ccd (arbre modifié) » : le binaire de la campagne était celui de
   `55e81fc` (commit 1), mais le banc lit `git` au moment d'écrire chaque rapport et le dernier (`rc05-p3`) a été écrit après
   le commit du refactor. Provenance exacte : binaire du commit 1, rejeu aux réglages de la campagne 2.
6. `COMMON_USE_NORMALIZED` reste à `false` : voir le point 17. Pas de rejeu à 100 % des boucles (question 3 du §14.1) : à
   décider par le propriétaire une fois le banc validé.

**Apprentissage et dépôt**

7. Le test 10c (un pas sur une table commune neutre avec `n0 = 0` égale le `step72` du banc sur `ratio_table` à
   `min_km = 0`) est dans le banc (`svc.rs`), avec un joueur jouet, pas sur les parcours du moteur : l'égalité est celle des
   tables rendues pour les mêmes mètres, c'est ce que dit la spécification.
8. Test 23 (banc) : **`BenchPlayer ≡ FlatPlayer` n'est pas écrit.** Le banc n'a pas de graphe synthétique avec ses deux
   vues (seulement le graphe de base de `fixture.rs`), et le joueur du banc n'est pas une copie du service : il appelle le
   même `first_kept_class_meters` et le même moteur. Il est exercé par la fumée (déterminisme à 1 et 16 fils, §3 du
   relevé). `ServicePlayer ≡ FlatPlayer` est, lui, testé de bout en bout côté service
   (`the_service_learns_what_the_reference_player_learns_on_the_same_loops`).
9. Le test 16 n'utilise pas le crochet caché `min_km` : des anneaux de 7,2 km (six rues de 300 m par côté) passent le seuil
   de 5 km sur la grille de 21 × 21 des tests du générateur. Le crochet (`TestHooks::profile_min_km_milli`) existe quand
   même, lu à la construction du générateur, pour un graphe plus petit ; un second crochet (`profile_hold`) retient la
   phase de profil (tests 18 et 19).
10. `ServicePlayer` traite `AcquireError::Overloaded` comme un arrêt : `WindowManager::acquire` attend déjà la place
    jusqu'à l'échéance qu'on lui passe (celle de la phase), et ne rend `Overloaded` qu'à l'échéance. C'est le comportement
    « attendre jusqu'à l'échéance » du §14.7.3 par la même attente.
11. Le plafond (`max_loops`, `per_type`) entre dans `LearnParams::hash` (§14.6) : **changer `generator.profile.max_loops`
    rend tous les profils écrits `incompatibles` (réglages)**, donc `stale` raison `profile` jusqu'au prochain dépôt. C'est ce
    que dit le §14.6 (« dont le plafond du réglage ») ; à savoir avant de toucher au réglage.
12. La purge d'un dossier d'un autre graphe compte ses profils sous `profile_ttl` (le §14.9 ne dit pas où) ; le dossier part
    en entier après la grâce de 48 h, profils compris.
13. `Deposit::finish` est gardé (appelle `finish_learn`) : l'outil `ord-graph-overlay` ne change pas.
14. `/v1/capabilities` est inchangé (§14.10), donc ne dit pas si les profils sont actifs : l'état d'une équipe le dit
    (`profile` absent de la réponse = profils désactivés).

**Banc et campagnes**

15. **Aucune campagne complète n'a tourné** (consigne : une fumée). Le rejeu de la chaîne commune (campagne 1) est fait, car
    la table en dépend ; les campagnes 2 à 4 du §14.13 ne le sont pas. L'essai du §14.13 (point 2) a tourné avec
    `--limit 20` sur B et C : il exerce le chemin de bout en bout, mais ses `S / S_base` ne se lisent pas (20 boucles par
    équipe, A absente). Les faits du §14.12.5 ne sont donc **pas établis** : (a), (b), (c), (d) restent à mesurer par la
    campagne 4. En tête du relevé.
16. `svc` apprend sur **toutes** les boucles de l'équipe non mises à l'écart (le dépôt collecte tout), même avec `--limit` :
    `--limit` ne borne que les boucles jouées. Choix cohérent avec le service (qui apprend sur ses traces, pas sur un
    échantillon), noté parce qu'un essai à `--limit 20` apprend sur plusieurs centaines de boucles.
17. **Normalisation (décision 3 du §9.4)** : la règle du §14.12.4 est codée (`compare_svc::norm_rule`) et appliquée à l'essai,
    qui la rejette (perte de 0,145 et service moins bon) ; **elle n'est pas tranchée par une vraie campagne**. Par prudence,
    `COMMON_USE_NORMALIZED = false` (brute, la configuration mesurée à la seconde sonde). Le propriétaire peut renverser :
    une constante, `Common::hash` change, les profils écrits deviennent incompatibles.
18. Variantes `svc+cap:0` et `svc+capt:500` : codées et testées (`parse_variants`, `LearnSet::capped` = collecteur plafonné),
    **non jouées**.
19. Le banc région du lot 6 (`--compare --self-check --budget-mb 1000 --base-taste common`, §14.13 point 5) n'a pas été
    rejoué : voir le relevé.

**Conteneur**

20. La séance en conteneur (§14.13) est **réduite** : voir le relevé pour ce qui a été fait.

**Relecture (17 novembre, après le commit 8)**

21. **Panique de la phase de profil** (corrigé, `fix(server)`) : seul `generate_in` tournait sous `catch_unwind` dans le
    joueur du service. Une panique ailleurs dans la phase faisait tomber tout le dépôt : le contrôleur et la marche du
    parcours gardé (`first_kept_class_meters`), la construction des données de la nouvelle surcouche, ou le `join` des fils.
    Le backend recevait alors un 500 et la surcouche n'était pas écrite, ce qui contredit le §14.7.4. Les octets de
    `hold_team` n'étaient pas rendus non plus : le budget de mémoire restait amputé jusqu'au redémarrage. Désormais, une
    panique donne `skipped`, la surcouche est écrite et une garde rend les octets. Le test
    `a_panic_in_the_profile_phase_still_writes_the_overlay_and_gives_its_memory_back` échoue si l'on retire la garde.
22. **`overlay_deposited_at`** (corrigé, `fix(server)`) : l'en-tête du profil recevait l'heure de fin de la phase, et non
    la date de dépôt écrite dans la surcouche (§14.6). Ce champ n'est ni lu ni exposé.
23. **À trancher** (tranché par les décisions 6 et 7 du §9.5, codé aux points 24 et 25) : `generator.profile.max_loops` n'a pas de borne haute. La mémoire et le temps de
    l'apprentissage ne sont bornés que par ce réglage et par le budget de 720 s. Un profil `partial` (1 ou 2 pas) est
    servi comme `present`, l'état est `ready`, et le backend ne redépose qu'au changement des traces ou à l'échéance. Le
    §14.1 (question 7) l'accepte ; une variante `stale` raison `profile` sur un profil partiel ferait réessayer la nuit
    suivante.

**Après les décisions du §9.5 (17 novembre, branche `feat/ord-loops-lot4-3b`, rien de poussé ni de déployé)**

24. **Décision 7, `max_loops` borné** (`feat(server)`) : la validation de `generator.profile` refuse `max_loops` hors de
    `1..=5 000` (`PROFILE_MAX_LOOPS_BOUND`, le défaut reste 500) ; le test de validation couvre 0, 5 001 et la borne. Le
    plafond entre déjà dans `LearnParams::hash` : aucune incidence sur les profils écrits. **Clôt le point 23, première
    moitié.**
25. **Décision 6, profil partiel** (`feat(server)`) : l'en-tête du profil (format 1, champ facultatif, jamais lu par 1.9.2)
    porte `complete_at`, la date du dernier profil **complet** (tous les pas du réglage, ou ensemble vide) ; un profil
    partiel la **recopie** de celui qu'il remplace (`Store::read_profile` au dépôt). Choix prudents, que rien ne tranchait :
    - sans profil précédent lisible (absent, ou appris avec d'autres réglages), un premier profil partiel **ouvre la série** :
      `complete_at` = sa date. Sept jours de profils seulement partiels, de nuit en nuit, finissent donc en `stale` raison
      `profile` (le backend redépose au changement des traces ou à l'échéance du TTL de 14 jours, jamais plus tôt : c'est
      lui qui décide, l'état ne fait que le demander) ;
    - un en-tête sans `complete_at` vaut la date d'apprentissage ; **« plus de 7 jours »** est strict (exactement 7 jours :
      `ready`) ;
    - `Store::status` lit l'horloge (`status_at(c, now)` pour les tests) ; `ready()` ne rend toujours jamais `stale(profile)` :
      un profil partiel trop vieux est servi comme avant ;
    - **le contrat ne change pas** : l'objet `profile` reste `state: present` avec ses `steps`, seul l'état de la surcouche passe
      à `stale` raison `profile` (la description de la raison dans le code dit « ou partiel depuis plus de 7 jours »).
    Tests : `a_partial_profile_is_ready_unless_the_last_complete_one_is_older_than_seven_days` (complet vieux de 30 jours :
    `ready` ; partiel avec un complet d'hier : `ready` ; partiel avec un complet de 8 jours : `stale` profil ; exactement 7
    jours : `ready` ; sans date : `ready` ; ensemble vide : `ready`), `a_complete_profile_dates_itself_and_a_partial_one_carries_the_last_complete_date`
    (service) et `completeness_and_the_date_of_the_last_complete_profile` (format). **Clôt le point 23.**
26. **Décision 2, le mélange de la sonde, en option** (`feat(bench)` puis `feat(server)`) : `generator.profile.mix =
    capped | sonde`, **défaut `capped`** (le 3b tel que codé : mêmes octets, mêmes empreintes `learn` et `common` qu'avant,
    vérifié sur la fumée). `LearnParams.mix` entre dans `LearnParams::hash` **dès `sonde`** (`;mix=sonde` en fin de texte
    canonique) et `Common::hash_for(mix)` ajoute à l'empreinte de la table commune celle des mètres des pas : un profil de
    l'autre forme, ou appris sur d'autres mètres de pas, est `incompatible` (réglages), jamais effacé. La forme `sonde` :
    - part de la table **neutre** (pas 1 joué à `K`, comme `rt`), pas de `C` ; un type sans boucle garde `C` au bit près ;
    - à chaque pas `k`, `ln ĝ[c] = w · ln g_T[c] + (1 − w) · ln g_P[c]`, `w = G_T / (G_T + n0 · G_P / N_P)` (poids nu, `w = 0`
      si le dénominateur n'est pas un nombre strictement positif, donc `N_P = 0` compris ; `w = 0` rend exactement `g_P`, `w
      ≥ 1` exactement `g_T`), `g_T` sans seuil de kilomètres, `g_P` neutre sous 200 km de réel ; puis signe, γ, `mult` et
      `q8` comme le pas plafonné. Ce sont les expressions et l'ordre d'opérations de `team_weight`, `blend` et `ratio_table`
      du banc ;
    - **la cible de chaque pas est figée** : `Common.steps[k]`, les mètres (réel, généré, boucles), toutes équipes réunies, que
      joue la table de départ du pas `k` de la **chaîne commune** (`base`, `rc05-p1`, `rc05-p2`), exportés en décimètres par
      `taste-common --step2 <rc05-p1-report> --step3 <rc05-p2-report>` (constantes `COMMON_STEP_*`, accesseurs
      `ord_loops::common_step_*`). Les quatre constantes d'avant sont inchangées bit pour bit (diff de `taste_common.rs`).
      **Écart avec `rt05-p3`** : `rt` mélange avec le `g_P` des jeux **contemporains** des tables d'équipe (chaque équipe joue
      sa table) ; au dépôt une équipe est seule, la cible est celle de la chaîne commune (tout le monde joue la même table).
      L'égalité au bit près est celle de l'arithmétique **à entrées égales** (test), pas celle des deux chaînes ;
    - au-delà de trois pas, le pas 3 se relit ; `normalize` s'applique comme au plafonné (après chaque pas, sur les mètres
      générés de l'équipe), les cibles des pas restant celles de la chaîne brute ; un profil partiel de la forme `sonde` est
      plus près de la table neutre que de `C` (T₁ ou T₂ de `rt`), à savoir avant de la mettre en service.
    **Test demandé** : `the_sonde_mix_gives_the_benchs_rt_tables_bit_for_bit` (`likeness/svc.rs`) : une équipe de 14 boucles
    ROAD et 6 GRAVEL, trois pas, un joueur jouet, une cible commune synthétique à trois pas (classes sans généré, classe à `w =
    1`, MTB sans boucle) ; les `q8` du service redonnent ceux de `ChainState::step(RecalTeam, …)` du banc, case par case, aux
    deux types ; et la forme plafonnée en donne d'autres. Autres tests : départ neutre et type sans donnée
    (`learn.rs`), poids nu sans le plafond, empreintes (`learn.rs`, `format.rs`), `ProfileMix` (`config.rs`), `mix:sonde`
    (`variants.rs`), export des pas (`taste_common.rs`).
27. **Variantes du banc** : `svc+mix:sonde`, combinable avec `norm`, `cap:<n>` et `capt:<n>` (noms `svc-sonde`,
    `svc-sonde-norm`, `svc-sonde-cap0`, `svc-sonde-capt500`) ; `mix` sans `svc`, `mix:capped` (le défaut n'a pas de suffixe) et
    deux `mix` sont refusés. `SvcMeta` gagne `mix`. La section « 3b » de la comparaison range les nouvelles colonnes, lit
    les faits (a) à (d) **pour chaque forme** (`svc` et `svc-sonde`, (c) et (d) contre `rt05-p3` et `common`) et applique la
    règle de la normalisation **aussi** à la paire `svc-sonde-norm` / `svc-sonde` (rubrique « sonde », pour la décision 3 si la
    forme `sonde` est retenue ; la décision écrite pour le plafonné ne change pas).
28. **Fumée** (`likeness --teams C --limit 8 --holdout-share 0.25 --variants base,svc,svc+mix:sonde,svc+mix:sonde+norm`, 1 fil et
    16 fils, sorties sous `G/lot4/smoke-3b-sonde/`) : **mêmes empreintes de résultat et mêmes `taste_xxh3` à 1 et à 16 fils**
    pour les quatre variantes ; `svc` (plafonné) garde exactement l'empreinte `learn` (`3f83f013b3a80305`), l'empreinte `common`
    et l'empreinte de résultat (`2f9a295d010c540c`) de la fumée de la relecture ; `svc-sonde` et `svc-sonde-norm` ont les leurs.
    Équipe C seule, 22 boucles d'apprentissage, 8 jouées : **les `S / S_base` ne se lisent pas** (A et B absentes).
29. **Commande exacte de la campagne 4**, une seule campagne, tout le reste arrêté (16 cœurs, 30 Go ; pic ~21 Gio ; [estimé]
    ~100 min, soit les ~70 min du §14.13 plus trois variantes `svc` de ~10 min) :

    ```bash
    G=~/.cache/osm-route-details/graph
    B=./target/release/ord-loops-bench      # cargo build --release -p ord-loops-bench, depuis le commit de la branche
    $B likeness --label lot4-3b --holdout-share 0.25 \
      --variants base,rt:0.5+n:3,common,common+norm,svc,svc+norm,svc+cap:0,svc+capt:500,svc+mix:sonde,svc+mix:sonde+norm,svc+mix:sonde+capt:500 \
      > $G/lot4/lot4-3b.log 2>&1
    ```

    Variantes produites : `base`, `rt05-p1` à `rt05-p3`, `common`, `common-norm`, `svc`, `svc-norm`, `svc-cap0`, `svc-capt500`,
    `svc-sonde`, `svc-sonde-norm`, `svc-sonde-capt500`. Contrôles inchangés (`base` = `4686c904975302e7`, `rt05-p3` =
    `451651c82f8a4b92` / `3fc8e87f9b9fbfcf`). Pas de `svc-sonde-cap0` : la décision 4 ne le demande pas et c'est la variante la
    plus chère (A sans plafond) ; `svc+cap:0` reste pour la forme plafonnée. **Comment lire, dans l'ordre du §9.5** :
    (1) B ROAD « toutes » de `svc` aux deux curseurs : < 1,0 aux deux, on garde `svc` (forme plafonnée, défaut) ; sinon
    `svc-sonde` (décision 2), à condition que ses faits (a) à (d) tiennent ; (2) la normalisation par la règle du §14.12.4
    lue sur la forme retenue (`norm_rule` ou `norm_rule_sonde` du `compare.json`) ; (3) le plafond par type si
    `svc-capt500` (ou `svc-sonde-capt500`) gagne au moins 0,03 au GRAVEL de A, « toutes », contre `svc` (ou `svc-sonde`) ;
    (4) si la forme `sonde` est retenue, `generator.profile.mix = sonde` devient le défaut du service (une ligne de
    `config.rs`) et l'export à 100 % (décision 5) réécrit aussi les `COMMON_STEP_*`. **À rejouer après tout cela, avant la
    fusion** (décision 8) : la séance en conteneur de 3 Gio.

**Relecture et campagne 4 (17 novembre au soir, branche `feat/ord-loops-lot4-3b`, rien de poussé ni de déployé)**

30. **Relecture des commits `8131ab1` à `e779537`** :
    - **Neutralité du défaut `capped`** : vérifiée. Le texte canonique et `Common::hash` sont inchangés. Le chemin du pas est
      le même, et un type sans boucle garde `C`. Les empreintes de la fumée étaient restées les mêmes (point 28).
    - **Fidélité de la forme `sonde` à `rt`** : vérifiée. `team_weight`, `blend` et `ratio_table` sont repris avec le même
      ordre d'opérations, au seuil de 200 km de `TasteParams`, à `n0` = 25 et avec les bornes de `TasteParams`. Le test au
      bit près compare bien avec `ChainState::step(RecalTeam, …)`. L'écart de la cible figée est celui du point 26.
    - **Règle des 7 jours** : vérifiée. La date est recopiée depuis le profil précédent lisible, la comparaison est stricte,
      et l'ensemble vide compte comme complet.
    - **Un défaut corrigé** (`502c6ac`, `feat(bench)`) : `norm_rule_sonde` ne jugeait que la paire `svc-sonde-norm` /
      `svc-sonde`. Le §14.12.4 demande aussi la paire `common-norm` / `common`, car la normalisation change le poids de base
      de toutes les requêtes. La règle juge désormais les deux paires. La conclusion de la campagne 4 n'en change pas.
31. **Campagne 4** : la commande du point 29 a tourné en 1 h 24, avec un pic de 18,2 Gio. Les contrôles `base` et `rt05-p3`
    sont passés aux bits près. Les décisions 2, 3 et 4 appliquées sont au §9.5, les chiffres au §6 du relevé.
    - **Second tirage des temps** (`lot4-3b-p50`) : les empreintes sont les mêmes, et le P50 bouge de ±0,06 × `base`.
    - **Pas de commit de défauts** : la campagne confirme les défauts codés.
    - **Choix prudent, signalé** : pour la décision 4, le gain est exigé sur le GRAVEL de A aux deux curseurs, pas à un seul.
    - **Pas prêt à fusionner** : le fait (d) ne tient pas. La réapprise sur 100 % et la séance en conteneur sont donc
      remises après la décision du propriétaire (§8 et §9 du relevé).
32. **Table commune réapprise sur 100 %** (18 novembre, décision 5 du §9.5, après l'acceptation de (d), §9.6) :
    `likeness --label lot4-3b-commune100 --no-holdout --variants base,rc:0.5+n:3` (commit du banc `2a4e446`, 16 fils, pic de
    16 122 Mo, sorties sous `G/lot4/lot4-3b-commune100-*`), puis `taste-common --holdout-share 0 --expect-xxh3
    effbdf7c3e979d77 --min-loops 50 --step2 … --step3 …`. Les mètres des trois pas `COMMON_STEP_*` sont réécrits à 100 % au
    même passage (le même outil, la même chaîne). Les contrôles `RECAL_EQUIPE_*` sont ceux du rejeu à 25 % : ils ne
    s'appliquent pas à `--no-holdout` ; `base` redonne bien le `taste_xxh3` neutre `2d06800538d394c2`.
    - **Empreintes** : `taste_xxh3` de `rc05-p3` `66ee08d3eb9c910c` -> `effbdf7c3e979d77` ; empreinte de résultat
      `2c563b8bbe8059ae` -> `cb966d45c326c3bd` ; empreinte des trois tables exportées (MTB neutre) `ea699c10cdd18e0c` ->
      `0090972c6fbf4bdf` (nouveau `Common::hash` : les profils écrits sur l'ancienne table sont `incompatible`, jamais
      effacés). Boucles comptées `[1545, 553, 3]` -> `[2045, 754, 4]` ; MTB neutre dans les deux cas (moins de 50).
    - **Écart 75 % -> 100 %** (`COMMON_Q8`, 72 valeurs par type) : ROAD, 24 cases sur 72 changent, écart maximal 172 (case 67 :
      118 -> 290, soit |ln| 0,90) ; GRAVEL, 39 sur 72, écart maximal 768 (case 59 : 256 -> 1 024, la borne haute, |ln| 1,39) ;
      MTB, aucun. Un agrégat : aucune ligne par équipe. À relire au 4c avec la réserve du §9.6 : la table à 100 % déplace
      quelques classes peu fournies jusqu'à la borne.
    - **Références** : `window_reference.txt` re-bénie (`ORD_LOOPS_BLESS=1`) : 43 requêtes sur 140 changent (36 gardent le
      nombre de routes et de pas, 7 changent aussi de pas) ; `window_reference_192.txt` (poids neutre), `team_reference.txt` et
      `team_check_reference.txt` **ne bougent pas**, comme prévu au §14.3.3. Les empreintes FNV figées de `cost.rs`
      (`RAW_HASH`, `NORMALIZED_HASH`) sont re-bénies avec la table.
    - **Mince écart du banc** : l'en-tête généré par `taste-common` dit « sans mise à l'écart » quand `--holdout-share 0`.
    - **Reste** : la séance en conteneur (décision 8), à rejouer une fois sur cette table.
33. **Relecture du point 32 et séance en conteneur rejouée** (18 novembre, décision 8 du §9.5 ; relevé, §12) :
    - **Relecture** : `taste-common` relancé depuis la branche sur les rapports `lot4-3b-commune100-*`. Il redonne
      `crates/ord-loops/src/taste_common.rs` octet pour octet après `rustfmt`. Les rapports portent `holdout` = 0 et
      `kept` = `retained` pour les neuf équipes : la table vient bien de 100 % des boucles retenues. Le fichier ne contient
      que des agrégats. Seule `window_reference.txt` est re-bénie. Aucune correction.
    - **Mention « arbre modifié » de la provenance** : la cause n'est pas établie, car l'état de l'arbre au moment de la
      construction du banc n'est pas gardé. La mention était déjà dans l'en-tête de la table à 75 % (`ad94ccd`). La chaîne
      `base`, `rc:0.5+n:3` joue sur un poids de base neutre : `base` redonne son `taste_xxh3` neutre, et à 75 % les contrôles
      `RECAL_EQUIPE_*` passaient aux bits près malgré la même mention. **Laissé tel quel, signalé.**
    - **Séance** : image `osm-route-details:lot4-3b` reconstruite depuis `b9374a4`, avec la méthode et les réductions du
      §5 du relevé (`drop_caches` refusé sans droit d'administration). Résultats : 18 dépôts en 200 au premier essai,
      16 `learned` et 2 `empty` ; profil de A seul en 236,0 s (285,9 s de dépôt sous 4 CPU) ; le plus long dépôt dure
      524,8 s, attente de la place comprise. Pic `anon` de 1 584 Mo (1,47 Gio), `memory.events` à 0 partout, aucune ligne
      `WARN` ou `ERROR`, aucune panique, aucun redémarrage. Génération à 4 clients : p50 de 0,49 s. 1.9.2 garde les 18
      `.ordp.enc`. Tous les critères du §14.13 tiennent.
    - **Prêt à fusionner : oui.** La réserve du §9.6 sur (d) passe au 4c (§10, point 4).

---

## 15. 4c : spécification

Écrit le 18 novembre 2026 sur la branche `feat/ord-loops-lot4c`, après la sonde point à point
([`ord-loops-lot4c-sonde.md`](ord-loops-lot4c-sonde.md)) et les décisions des §9.1 et §9.4 à §9.6 (appliquées telles
quelles), à la lecture de `crates/ord-loops` (`lib.rs`, `generate.rs`, `search.rs`, `reverse.rs`, `engine.rs`, `window.rs`,
`team.rs`, `cost.rs`, `probe.rs`, `tests_reference.rs`), de `crates/ord-graph-match/src/profile` (`collect.rs`, `learn.rs`,
`format.rs`), de `crates/ord-server/src/generator` (`generate.rs`, `routes.rs`), du banc (`likeness/{play,p2p,variants,shape,
svc}.rs`) et des relevés du 4a, du 3b et de la sonde. Cette section **suffit au codeur** : ce qu'elle ne dit pas est laissé
à son jugement, à condition de tenir les invariants du §15.8 et le §15.17. Les sections antérieures ne sont pas réécrites ;
leurs erreurs factuelles sont signalées au §15.0 (points 12 à 17).

**Ce que livre le 4c** : (1) une **génération organique** dans `ord-loops` : la lentille d'aujourd'hui (figure k = 1),
élargie par un **couloir**, et le **triangle** (k = 2), tirés par requête sur la répartition réelle des sommets à 10 % de
D ; (2) la **répartition par équipe** calculée au dépôt et rangée dans l'en-tête du profil du 3b ; (3) le **service** en
api 1.11, champ `figures` ajouté, défaut `classic` ; (4) le **banc** qui mesure organique contre `svc` et les critères
chiffrés de la bascule du défaut ; (5) si ces critères tiennent, la bascule et une séance en conteneur.

### 15.0 Ce que la lecture a établi

1. **Le moteur ne fait qu'une figure** (`generate_inner`) : par passe (6 au plus), les deux arbres, puis par secteur de 30°
   (4 secteurs dans le cap, 12 sans cap, ordre tiré par la graine) au plus `tries_per_sector` (6) demi-tours ; **un
   candidat retenu par secteur au plus** (`break` au premier gardé) ; le retour par `astar` (heuristique T2, plafond
   `2 h0 + 10 000`), sinon l'ordre inverse (`astar_back`). Budget d'états **par passe** : `6 × |T1| + 20 000`, l'ordre
   inverse le sien (`/ 3 + 10 000`). Tolérance du candidat : `Params::tolerance` = 0,10. Choix : tri par note puis clé,
   glouton avec recouvrement < 0,5 et part d'équipe minimale.
2. **Le bit `COR` existe déjà** (`search.rs`, valeur 4) : `astar` et `astar_back` multiplient le poids par **3** quand il
   est posé, mais **rien ne le pose**. Le retirer ne change aucune sortie.
3. **Les références figées hachent le `Debug` complet** de `(Result<Generation, Failure>, Diagnostics)`
   (`tests_reference::snapshot`) : **ajouter un champ à `Generation`, `Diagnostics` ou `Route`, ou renommer une variante de
   `Family`, change les 140 empreintes** de `window_reference.txt` et de `window_reference_192.txt`, même si aucun parcours
   ne change. Le `Debug` de `Request` n'y entre pas.
4. **`family` est une chaîne libre du contrat** (`RouteV1.family: &'static str`, doc « `turnaround` or `polygon` ») ; le
   backend de Pédalons **ne lit pas** ce champ [lu dans le dépôt tribly : aucune occurrence]. Ajouter `triangle` est un
   ajout de documentation.
5. **L'en-tête JSON du profil tolère les champs inconnus** (`ProfileMeta` sans `deny_unknown_fields`) : un champ facultatif
   ajouté au format 1 est ignoré par 1.10, comme `complete_at` l'a été par 1.9.2 (§14.16, point 25). Pas de format 2.
6. **Le collecteur du dépôt voit chaque boucle retenue** avec sa marche et sa polyligne plane (`LoopCollector::offer`,
   `trace.pts`) **avant** le plafond de 500 : les formes peuvent y être comptées sur toutes les boucles, pour un coût nul.
7. **La sonde ne vérifie pas le virage en P2** entre le dernier état de la jambe 2 et le premier du chemin T2 : dans la
   génération, ce virage doit être contrôlé (sinon `SHARP_TURN` au contrôle).
8. **La géométrie du triangle** [calculé] : sommets à vol d'oiseau r de part et d'autre, écart θ vu du départ, détour f
   sur les trois jambes : `r / D = 1 / (2 f (1 + sin(θ/2)))`. Avec le détour du moteur (f = 1,25, §4.3 du relevé du 4a) :
   0,298 à θ = 40°, 0,234 à θ = 90°, **0,329 à θ = 25°**. Le r / D médian des triangles réels est 0,318 (ROAD) et 0,267
   (GRAVEL) : l'écart de 40° à 90° de la sonde convient au GRAVEL et place les triangles ROAD **sous** le réel ; la bande
   des mètres T1 et l'écart minimal se règlent ensemble (variante `wide`, §15.11).
9. **La lentille garde son r / D** : à figure égale, l'aller du moteur fait 1,25 fois le vol d'oiseau, celui des équipes
   1,41 (ROAD) à 1,66 (GRAVEL) ; le couloir ne baisse presque pas r / D (−5 % pour une compacité × 2,7, §4.3 du relevé du
   4a). Le 4c ne touche donc pas au choix du demi-tour : le gain de r / D vient des triangles.
10. **Le mode strict n'a aucun jeu au banc** (curseurs 0,25 et 1 seulement) ; l'aller simple non plus.
11. **Facteur minimal de coût** : `Weighter::chain` vaut `λ · reste + M[z] · a + b` (équipe d'abord), `M[z] · a + b`
    (strict) ou `w` (découverte), avec `w = reste + a + b` le poids de base (au goût) de la chaîne. Donc
    `chain(c) ≥ min(1, λ, min_z M[z] / 256) · w(c)` : le plus petit `w / longueur` de la fenêtre (ou des agrégats au goût de
    l'équipe), calculé **une fois**, borne toutes les requêtes ; le facteur d'une requête s'en déduit par deux minimums.

**Erreurs factuelles des sections antérieures, corrigées ici** (le texte d'origine reste) :

12. *§4.1* : k ∈ {1, 2, 3} → **{1, 2}** (quadrilatère hors lot, §9.1) ; « r / D visé » ne vaut que pour le triangle (point 9).
13. *§4.2* : « estimation à ±5 % de D » et « un nœud par secteur » → fenêtre **[0,91 ; 1,01] × D** et **3 sous-bandes** par
    secteur (§3 et §5 du relevé de la sonde) ; la sécante recale un biais **par requête**, pas « par secteur ».
14. *§4.3* : « un bit de plus dans l'estampille `Ban` » → une **trame lue par position** (§15.6) : poser un bit par chaîne
    demanderait un index case → chaînes ; le `COR` inutilisé (point 2) est retiré.
15. *§4.6* : « `Family` gagne `Lens`, `Triangle`, `Quad` ; `Polygon` retiré » → la lentille **garde le nom `Turnaround`**
    (point 3 : un renommage change les références figées) et la valeur d'API `turnaround` ; seul `Triangle` est ajouté ;
    `Polygon` reste (documenté dans le contrat, jamais rendu ; retiré au prochain changement cassant).
16. *§3.3* : les formes de l'équipe ne sont pas « le profil » au sens des tables : elles vont dans l'en-tête JSON du profil
    (point 5), sans entrer dans son empreinte.
17. *§6* : l'api du 4c est **1.11** (déjà dit au §14.0, point 16).

### 15.1 Choix tranchés

| # | Question | Tranché | Raison |
|---|---|---|---|
| 1 | Figures | Lentille (k = 1, le demi-tour d'aujourd'hui, au couloir) et triangle (k = 2) | §9.1 |
| 2 | Part du quadrilatère réel (6,5 % ROAD, 1,7 % GRAVEL) | **Ôtée, parts renormalisées** : `p_tri = n₂ / (n₁ + n₂)` | Aucune figure ne la porte ; la verser au triangle surestimerait `vertices_10 = 2` |
| 3 | Où vit la répartition | Constantes générales générées (`shape_common.rs`) **et** formes de l'équipe dans l'en-tête du profil (champ facultatif) | Points 5 et 6 du §15.0 |
| 4 | Seuil de l'équipe | 50 boucles retenues du type, comptées **avant** le plafond de 500 | §9.1 ; seules A (ROAD 1 860, GRAVEL 545) et B (167, 228) passent |
| 5 | r / D visé | Tiré par requête entre P10 et P90 des **triangles** réels (équipe ou général) ; sert de **clé de rang** des paires, jamais de filtre | Garder le rendement de 97 % des paires (§3 de la sonde) |
| 6 | Tirage | Le **premier** parcours suit la figure tirée ; les deux autres préfèrent une figure absente à note voisine (Δ ≤ 1,0) | Le banc lit le premier ; §4.1 pour les suivants |
| 7 | Paires par requête | **4** au plus, deux triangles gardés au plus, lentille en repli | Question 2 de la sonde, §15.18 question 2 |
| 8 | Bornes T1 / T2 | Admissibles : **−2 500** sur chacune | §4 de la sonde |
| 9 | Facteur minimal | Une fois par fenêtre et par agrégat d'équipe, par type | §4 de la sonde ; point 11 |
| 10 | Plafond d'une recherche P1 → P2 | **1 × \|T1\|** états, dans le budget partagé de la passe | P90 mesuré 0,28 × T1 ; aucune n'a dépassé 6 × T1 |
| 11 | Jambe d'arrivée | Chemin T2 s'il ne reprend rien **et** si le virage en P2 est permis ; sinon `astar`, plafond inchangé (multiplié par le couloir) | Point 7 ; un retour au-delà de 2 × T2 sort de toute façon de ±10 % de D |
| 12 | Épi | Angle ≥ 150° entre les 500 m d'arrivée et de départ d'un **sommet de triangle** ; une réparation par triangle | §4.4 ; pas sur la lentille (son demi-tour est un épi par nature) |
| 13 | Couloir | Trame lue par position, `w` = 0,03 D, κ = 2, queue 0,15 D, sommets exclus sur 2 w | §4.3 ; point 14 |
| 14 | Strict, aller simple, sans boucle | `Classic` (organique sans effet) | Point 10 : rien de mesuré |
| 15 | MTB | `p_tri = 0` : lentille au couloir | 4 boucles réelles |
| 16 | Apprentissage du profil | Joue les figures du réglage `generator.profile.learn_figures` (défaut `classic`) ; `organic` entre dans l'empreinte | Tranché au banc par `lc` (§15.12, règle L) |
| 17 | Défaut du service | `classic` jusqu'aux critères du §15.12 ; bascule dans un commit à part | §6, §10 étape 6 |
| 18 | API | 1.11, ajouts seulement ; paquet 1.11.0 | §14.0, point 16 |

### 15.2 Où vit chaque morceau

| Morceau | Où | |
|---|---|---|
| `Figures`, `Organic`, `FigureMix`, `OrganicTune`, `Family::Triangle`, `FigureDiagnostics` | `ord-loops` : `lib.rs` | ajout |
| Constantes de forme générales, générées | `ord-loops` : `shape_common.rs` (accesseur `FigureMix::general`) | nouveau |
| Recherche point à point (déplacée de `probe.rs`), trame du couloir, `Ban.cor`, facteur minimal | `ord-loops` : `search.rs`, `reverse.rs`, `window.rs`, `team.rs` | changé |
| Génération organique, paires, sécante, épi, choix | `ord-loops` : `generate.rs` (le lot de la lentille inchangé) et `figures.rs` (nouveau : paires, triangle, choix) | changé, nouveau |
| Sonde | `ord-loops` : `probe.rs` appelle `search::astar_p2p` (marge 0 : le relevé se rejoue) | changé |
| Mesures pures de forme (Douglas-Peucker, `vertex_indices`, `vertices`, r / D) | `ord-graph-match` : `plane.rs` (déplacées du banc, réexportées par `shape.rs`) | déplacé |
| Formes de l'équipe au dépôt | `ord-graph-match` : `profile/shape.rs` (nouveau), `collect.rs`, `format.rs`, `learn.rs` | nouveau, changé |
| Service | `ord-server` : `generator/{routes,generate,store,profile}.rs`, `config.rs`, `lib.rs`, contrat, `CHANGELOG.md` | changé |
| Banc | `tools/ord-loops-bench` : sous-commande `shape-common`, `likeness/{variants,play,svc,compare_org,out}.rs` | nouveau, changé |

### 15.3 Tirage de la figure

**Constantes générales** (`shape_common.rs`, généré par `$B shape-common`, §15.11.1) : par type, `n₁`, `n₂`, `n₃₊` (comptes
`vertices_10` des boucles réelles retenues, k ≤ 1 compté avec 1), `SHAPE_TRI_Q8[t] = round(256 · n₂ / (n₁ + n₂))` et
`SHAPE_RD_TRI_MILLI[t]` = P10, P25, P50, P75, P90 (type 7) de r / D des boucles à `vertices_10 = 2`, en millièmes. Attendu
[calculé sur le §3.3 du relevé du 4a] : ROAD `tri_q8` ≈ 145 (0,567), GRAVEL ≈ 112 (0,436) ; MTB **forcé à 0** (4 boucles).
Un agrégat de quelques nombres par type : commité.

**Formes de l'équipe** (§15.9) : `TypeShape { loops, k1, k2, k3, rd_tri_milli: Option<[u16; 5]> }` par type.
`ShapeProfile::mix(ride) -> Option<FigureMix>` : `Some` si `loops ≥ 50` ; `tri_q8 = round(256 · k2 / (k1 + k2))` ;
`rd_tri_milli` de l'équipe si `k2 ≥ 20`, sinon celui des constantes générales. MTB : toujours `None`.

**Choix du mélange** (service et banc, une fonction `ord_graph_match::profile::shape::mix_for(shapes: Option<&ShapeProfile>,
ride) -> FigureMix`) : celui de l'équipe s'il existe, sinon `FigureMix::general(ride)`. Sans équipe, sans profil, profil
sans formes (écrit par 1.10) ou équipe sous le seuil : général.

**Tirage, par requête** (organique, boucle, mode ≠ strict) : générateur à part, **jamais celui de la lentille** (sa suite de
tirages reste celle de `Classic`) : `fr = SplitMix64(r.seed ^ fingerprint(r) ^ FIGURE_SALT)` avec `FIGURE_SALT =
0x4f52_4746_4947_0001`. Dans cet ordre :
1. `tri_first = fr.below(256) < tri_q8_eff`, `tri_q8_eff = min(256, tri_q8 · tri_boost_q8 / 256)` ;
2. `ρ* = interp(u)`, `u = 0,1 + 0,8 · fr.below(10 000) / 10 000`, interpolation linéaire par morceaux entre (0,10 ; P10),
   (0,25 ; P25), (0,50 ; P50), (0,75 ; P75), (0,90 ; P90) de `rd_tri_milli / 1 000` ;
3. `clockwise = fr.below(2) == 0` : sens du triangle (P2 après P1 dans le sens horaire vu du départ, ou l'inverse).

Le triangle est cherché **quelle que soit** la figure tirée pour le premier parcours (§15.7) : `tri_first` ne décide que du
choix. `tri_q8 = 0` : aucune recherche de triangle.

### 15.4 Points lointains, paires, estimation, sécante

**Candidats** (une passe sur `t1.reached`, comme la sonde) : nœud atteint par T1 et T2, **degré de vue ≥ 3**
(`v.deg[node] ≥ 3` : une sortie autre que l'arête d'arrivée, §3 de la sonde), mètres T1 `m₁ / D` dans la bande
`[band_lo, band_hi)` (défaut [0,27 ; 0,39]), secteur de 30° dans le cap (même règle que la lentille : centre à 50° au plus ;
sans cap, les 12). Par secteur et par sous-bande (`sub_bands` = 3, tiers égaux de la bande) : le plus agréable, `coût T1 /
mètres T1` minimal, départage par id de nœud.

**Paires** : deux candidats d'écart `sep` vu du départ dans `[sep_lo, sep_hi]` (défaut [40° ; 90°]), ordonnés selon
`clockwise`. Pour P1, P2 : `r_i` vol d'oiseau départ → P_i, `m_i` mètres T1 du meilleur état, `φ = (m₁ / r₁ + m₂ / r₂) / 2`,
`c` vol d'oiseau P1 → P2, `m₃` mètres T2 du meilleur état de P2, **`est = m₁ + φ · c + m₃`** (la formule de la sonde),
`r_pair = max(r₁, r₂) / D`.

**Fenêtre et sécante** : biais `β` par requête, initial `beta0` = 1,04 (biais médian mesuré +4,2 % de D). Une paire est
**admissible** si `|β · est − D| ≤ est_tol · D` (`est_tol` = 0,05) : à `β = 1,04`, `est ∈ [0,913 ; 1,010] × D`, la fenêtre
tranchée par la sonde. Après chaque triangle **complet** (trois jambes trouvées, avant contrôle) de longueur `L` :
`β ← clamp(β · sqrt(L / (β · est)), 0,90 ; 1,30)` (demi-pas géométrique : la queue de l'erreur, P90 0,27 × D, ne doit pas
emporter β sur un seul cas) ; `secant = false` le fige.

**Rang** des paires admissibles non encore cherchées : `(⌊|r_pair − ρ*| / 0,02⌋, agrément moyen des deux sommets, id P1,
id P2)` croissant. La paire suivante est relue après chaque mise à jour de β.

### 15.5 Les jambes

**Jambe 1** : `t1.path_to(meilleur état de P1)`.

**Jambe 2, `search::astar_p2p`** (la fonction de la sonde déplacée, `pub(crate)`) :
- départ : l'état d'arrivée de la jambe 1 et son prédécesseur (règle de virage et `retro_ok` comme `astar`) ; cible : le
  **nœud** P2, atteint par n'importe quel état ;
- graphe privé : `Ban` de la jambe 1 (`BAN` au-delà de la queue `tail_reuse_share · D`, `TAIL` au coût × 2 en deçà), plus le
  couloir de la jambe 1 (§15.6) ; disque D/2 ; `forb` de la passe ;
- heuristique au nœud x : `max(⌊crow_dm(x, P2) · f_min⌋, T1b(P2) − T1b(x) − 2 500, T2b(x) − T2b(P2) − 2 500)` (soustractions
  saturées à 0), `T1b` et `T2b` le coût du meilleur état du nœud dans chaque arbre (0 si non atteint), `2 500 =
  TURN_COST_MAX + 500` (la sonde rejoue avec une marge 0 : paramètre `slack`) ;
- `f_min = 0,999 · base_min[ride] · min(1, λ_q8 / 256, min M[z] / 256)` en équipe d'abord, `0,999 · base_min[ride]` en
  découverte (§15.0, point 11 ; le strict ne joue pas de triangle) ; `base_min` = `Window::min_w_per_dm(ride)` sans goût d'équipe,
  `WindowTeam::min_w_per_dm(ride)` avec (calculés à la construction : `min_c w(c) / len_dm(c)` sur les chaînes de longueur
  non nulle, `f64`) ; le facteur 0,999 absorbe l'arrondi `+128 >> 8` des multiplications ;
- budget : décompté du **budget de la passe** (`budget` de `generate_inner`), plafonné à `|T1|` états par recherche ;
  plafond atteint : recherche abandonnée (compteur) ; budget de la passe à 0 : la phase des triangles s'arrête pour la
  passe, **sans** lever `exhausted` (la lentille a déjà joué) ;
- `WindowEscape`, `stop` : comme `astar`.

**Jambe 3** : `ret = t2.path_to(meilleur état T2 de P2)`. Gardée telle quelle si (a) aucune de ses chaînes n'est `BAN` dans le
`Ban` de la jambe 2 ni prise par la jambe 2, **et** (b) `turn_cost(dernier état de la jambe 2, son prédécesseur, ret[0])`
est `Some` et `retro_ok` tient. Sinon `astar` (heuristique T2) depuis le dernier état de la jambe 2, sur le graphe privé des
jambes 1 (`BAN` / `TAIL` comme la jambe 2) et 2 (`BAN`), couloir des jambes 1 et 2, budget de la passe.

**Épi** (sommets P1 puis P2 du triangle) : sur les positions des nœuds (`Local` du départ), `h_in` = cap du point situé
`spike_m` (500 m) **avant** le sommet le long de la jambe d'arrivée jusqu'au sommet, `h_out` = cap du sommet au point situé
500 m **après** le long de la jambe de départ (le premier nœud au-delà, ou le bout de la jambe) ; épi si
`turn_angle_deg(h_in, h_out) ≥ spike_deg` (150°). **Réparation, une seule par triangle** : P' = le dernier nœud de degré ≥ 3
de la jambe d'arrivée situé à 500 m au moins avant le sommet ; la jambe d'arrivée est tronquée à P' (préfixe de ses états),
la jambe de départ refaite depuis P' (jambe 2 vers P2 si le sommet est P1 ; jambe 3 si c'est P2), l'épi relu ; s'il reste,
ou si la recherche échoue, la paire est jetée. La forme « couloir partagé » du §4.4 n'est pas codée : le couloir l'écarte
déjà au-delà de 2 w (§15.6) ; le banc compte les épis rendus (`spikes_kept`, par le contrôle de l'angle sur les parcours
gardés) et la décision se reprend s'il en compte.

**Longueur** : `L = mètres(jambe 1) + mètres(jambe 2) + mètres(jambe 3)` ; hors `tolerance · D` (0,10) : sécante, paire
suivante. Sinon `steps_of` sur les états concaténés, `Route { family: Family::Triangle, .. }`, `check_route_in` : refus →
transitions `SHARP_TURN` ajoutées à `new_forb` comme pour la lentille, refus compté dans `FigureDiagnostics` ; gardé →
même note que la lentille, ajouté à `scored` (même déduplication par clé).

### 15.6 Couloir

**Trame** (`search::CorGrid`, dans `ViewWs`, réutilisée d'une requête à l'autre, comptée par `Workspace::bytes`) : carrée,
centrée au départ (coordonnées `Local`), demi-côté `D/2 + w`, cellule `cell = clamp(w / 3, 100 m, 500 m)`, un bit par case
(`Vec<u64>`). À w = 0,03 D : 300 m à 30 km, 500 m dès 50 km ; 424 × 424 cases (22 Ko) à 200 km [calculé].

**Marquage d'une jambe** : pour deux états consécutifs, le segment entre leurs nœuds d'arrivée, échantillonné tous les
`cell / 2` (une chaîne de vue peut faire plusieurs kilomètres sans nœud), **sauf** les parties dont l'abscisse le long du
parcours est (a) à moins de `tail_reuse_share · D` (0,15 D) du départ ou de l'arrivée (la queue), ou (b) à moins de `2 w` d'un
sommet (demi-tour de la lentille, P1, P2) — sans (b), toute jambe partirait de son sommet dans le couloir de la précédente.
Puis **dilatation** par un disque de rayon `⌈w / cell⌉` cases (gabarit précalculé).

**Coût** : à la relaxation d'une arête de vue dans `astar`, `astar_back` et `astar_p2p`, si `ban.cor` est `Some` et que le
nœud d'arrivée tombe dans une case marquée : `w ← min((w · κ_q8 + 128) >> 8, u32::MAX / 2)`, après le `× 2` de la queue.
`Ban` gagne `cor: Option<&CorGrid>` et `kappa_q8: u32` ; `None` partout en `Classic`. Le plafond d'`astar` et
d'`astar_back` devient `h0 · (256 + κ_q8) / 256 + 10 000` **quand `cor` est `Some`** (κ = 1 redonne `2 h0 + 10 000`), et
reste exactement `2 h0 + 10 000` sinon. L'heuristique reste admissible : le couloir n'augmente que des coûts.

**Où** : retour de la lentille (couloir de l'aller), ordre inverse de la lentille (couloir du retour), jambe 2 (couloir de la
jambe 1), jambe 3 cherchée (couloir des jambes 1 et 2). Le chemin T2 gardé tel quel (§15.5) n'est pas pénalisé.

**Valeurs de départ** : `w` = 0,03 D (`corridor_w_milli` = 30 ; 0 = sans couloir), κ = 2 (`kappa_q8` = 512) : la séparation
minimale du §4.3, pas la largeur visée (0,11 D).

**Plan de réglage** (banc, §15.15) : campagne 1, `w` ∈ {0 ; 0,03} à κ = 2 (l'effet du couloir isolé) ; campagne 2, `w` ∈
{0,02 ; 0,05} à κ = 2, puis κ ∈ {1,5 ; 3} au meilleur `w`. **Choix** : la plus petite W1 / IQR de la compacité ROAD
(moyenne des deux curseurs, lecture « toutes »), parmi les réglages qui tiennent C1, C2 et B1 du §15.12 et dont la W1 / IQR de
r / D ne dépasse pas celle de `w` = 0,03 de plus de 0,03 ; à 0,02 près, le plus petit `w` puis le plus petit κ.

### 15.7 Génération organique : une passe, puis le choix

`generate_inner` prend `figures` ; `org = Some(o)` si `r.figures` est `Organic(o)`, que la requête est une boucle et que le
mode n'est pas strict ; sinon `org = None` et **tout le code suit le chemin de `Classic`**.

```text
pour chaque passe (6 au plus) :
    arbres T1, T2 ; faisabilité (passe 0)                                     — inchangé
    boucle des secteurs de la lentille                                        — inchangée, sauf :
        org et w > 0 : couloir de l'aller (ou du retour en ordre inverse) dans le Ban, plafond ajusté
    si org, tri_q8_eff > 0, non exhausted, triangles non finis :
        candidats et paires de cette passe (paires (P1, P2) déjà cherchées sautées)
        tant que cherchées < max_pairs (4), triangles gardés < tri_keep (2), budget de la passe > 0 :
            paire admissible de rang minimal, sinon sortir
            jambe 2 ; épi en P1 ; jambe 3 ; épi en P2 ; L ; sécante ; tolérance ; contrôle ; note ; scored
    fin : exhausted, ou (scored ≥ 3 et triangles finis), ou new_forb vide — « triangles finis » vaut vrai sans org
```

« Triangles finis » : `tri_q8_eff = 0`, ou `tri_keep` triangles gardés, ou `max_pairs` paires cherchées, ou plus aucune paire
admissible dans la passe. `max_pairs` et `tri_keep` comptent sur **toute la requête**, passes comprises.

**Choix des trois parcours** (après le filtre de part d'équipe, inchangé) :
1. premier : le meilleur (note, clé) de la famille `tri_first ? Triangle : Turnaround` ; s'il n'y en a pas, le meilleur toutes
   familles ;
2. deuxième et troisième : parmi les restants dont le recouvrement avec chaque choisi est < 0,5, `c` le meilleur toutes
   familles, `c'` le meilleur d'une famille **pas encore choisie** ; `c'` si sa note ≤ note de `c` + `near_score` (1,0 : 5 % de
   D d'écart de longueur, ou 45° d'écart de cap), sinon `c`.

Sans triangle, ce choix est exactement le glouton de `Classic` (même tri, même filtre). `Family::Triangle` pour les
triangles, `Turnaround` pour les lentilles.

### 15.8 Interface du moteur et non-régression

```rust
pub struct Request { /* … */ pub figures: Figures }          // tous les littéraux du workspace : `figures: Figures::Classic`
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Figures { Classic, Organic(Organic) }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Organic { pub mix: FigureMix, pub tune: OrganicTune }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FigureMix { pub tri_q8: u16, pub rd_tri_milli: [u16; 5] }      // FigureMix::general(ride), FigureMix::LENS_ONLY
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrganicTune {                                     // Default = les valeurs du service
    pub corridor_w_milli: u16, pub kappa_q8: u16,            // 30, 512
    pub max_pairs: u8, pub tri_keep: u8, pub sub_bands: u8,  // 4, 2, 3
    pub band_milli: [u16; 2], pub sep_deg: [u8; 2],          // [270, 390], [40, 90]
    pub beta0_milli: u16, pub est_tol_milli: u16, pub secant: bool,   // 1 040, 50, true
    pub tri_boost_q8: u16, pub near_score_milli: u16,        // 256, 1 000
    pub spike_deg: u8, pub spike_m: u16,                     // 150, 500
}
pub enum Family { Turnaround, Polygon, Triangle }             // ajout en dernier ; `Polygon` jamais rendu
pub struct FigureDiagnostics { /* compteurs, §15.11.3 */ }
pub fn generate_with_figures_in(e, win, team, r, ws, stop)
    -> (Result<Generation, Failure>, Diagnostics, FigureDiagnostics);
```

- `generate_with_diagnostics_in` appelle `generate_with_figures_in` et lâche le troisième terme : **`Generation`,
  `Diagnostics`, `Route` ne gagnent aucun champ** (§15.0, point 3). Les compteurs des triangles ne vont que dans
  `FigureDiagnostics` ; `Diagnostics` (dont `rejected`, qui classe les échecs) reste celui de la lentille.
- `fingerprint(r)` n'entre pas `figures` ; la lentille consomme le même générateur dans le même ordre.
- Les réglages vivent dans la requête et non dans `Params` : le banc fait varier `w`, κ, les paires sans rouvrir le moteur.
- **Invariants, tenus par des tests (§15.13)** :
  - **I1** : `Figures::Classic` rend exactement 1.10 : `window_reference.txt` et `window_reference_192.txt` passent **sans
    re-bénédiction** ; `team_reference.txt` et `team_check_reference.txt` inchangées.
  - **I2** : `Organic` avec `tri_q8 = 0` et `corridor_w_milli = 0` rend exactement `Classic` (mêmes `Debug`), sur les 140
    requêtes de la référence.
  - **I3** : tout parcours rendu en organique passe `check_route_in` et le contrôleur indépendant (`engine_vs_checker`).
  - **I4** : même graine, même sortie ; fenêtre plus grande, même sortie et aucun échappement (lot 6).
- Poids et pénalités en entiers (κ en 256es) ; ρ*, β et l'estimation en `f64` (opérations IEEE déterministes, `sqrt`
  correctement arrondie) : même résultat natif et WASM, comme la géométrie d'aujourd'hui.
- `probe.rs` appelle `search::astar_p2p` avec `slack = 0` et sans couloir ; la sonde se rejoue (empreintes de `base` et
  `svc` inchangées avec elle).

### 15.9 Profil : formes de l'équipe au dépôt

- `ord_graph_match::plane` reçoit, **déplacées du banc sans changement**, `douglas_peucker`, `dist_to_segment`,
  `vertex_indices`, `vertices` et le calcul de `r_over_d` (`far_of(pts) -> (r_over_d, far_pos)`) ; `shape.rs` du banc les
  réexporte. Contrôle : empreinte de `likeness --teams C --limit 5` inchangée.
- `profile/shape.rs` : `ShapeAcc` (par type : comptes k ≤ 1, 2, ≥ 3 sur `vertices(trace.pts, 0,10)`, et les r / D des boucles
  à k = 2), alimenté dans `LoopCollector::offer` **après** `judge_pieces` et **avant** le plafond ; `finish()` rend
  `LearnSet { loops, counts, shapes: ShapeProfile }` (quantiles type 7 en millièmes, `None` sous 20 boucles à k = 2).
- `ProfileMeta` gagne `#[serde(default, skip_serializing_if = "Option::is_none")] pub shapes: Option<ShapeProfile>` :
  format 1 inchangé, **hors** des empreintes `common` et `learn`. Écrit pour `learned`, `partial` et `empty` (`None` si aucune
  boucle) ; `skipped` garde l'ancien profil et ses formes. Taille : +~200 octets.
- `LearnParams` gagne `figures: LearnFigures { Classic, Organic }`, défaut `Classic`, ajouté au texte canonique **seulement**
  s'il vaut `Organic` (`;figures=organic`, comme `mix`) : les profils écrits restent compatibles tant qu'il vaut `Classic`.
  `loop_request` reçoit `figures: Figures` ; en `Organic`, l'apprentissage joue `Organic { mix: mix_for(Some(&set.shapes),
  ride), tune: Default }`.
- Lecture en service : `Entry` gagne `shapes: Option<ShapeProfile>` (profil `Present` seulement). Un profil écrit par 1.10
  (sans formes) sert le mélange général jusqu'au dépôt suivant ; l'état ne devient **pas** `stale` pour cela (A et B
  diffèrent du général de 1 à 11 points sur `p_tri`, B ROAD 0,454 contre 0,567 [calculé sur le §3.3 du relevé du 4a] ; le
  redépôt vient au changement des traces ou dans les 7 jours).

### 15.10 Service (api 1.11, paquet 1.11.0)

- `POST /v1/generator/routes` : `figures: classic | organic`, facultatif ; absent → `generator.figures` (réglage, défaut
  `classic`, variable `ORD_GENERATOR__FIGURES`). Le service construit `Figures::Organic { mix: mix_for(entry.shapes, ride),
  tune: OrganicTune::default() }`.
- Réponse : `figures` (la valeur appliquée) ; `routes[].family` peut valoir `triangle` (doc : « `turnaround`, `triangle` ;
  `polygon` is never returned »).
- `/v1/capabilities` : `generator.figures: { default, values: [classic, organic] }`.
- `generator.profile.learn_figures: classic | organic`, défaut `classic` (§15.12, règle L) ; validé comme les autres.
- Métriques : `ord_generator_routes_total{family}` (valeurs fixes) ; journaux : la famille, jamais une position.
- `API_VERSION` 1.11, espace de travail 1.11.0, `CHANGELOG.md` (« Ajouté » : figures organiques, famille triangle, formes du
  profil ; « Compatibilité » : défaut inchangé, déploiement roulant sûr — 1.10 ignore `shapes`, Pédalons n'envoie pas
  `figures` ; « Changé » au commit de bascule seulement). Contrat régénéré par le test `contract`. Rien pour Pédalons.

### 15.11 Banc

#### 15.11.1 Constantes générales

`$B shape-common --report <rapport base> --out crates/ord-loops/src/shape_common.rs` : lit les `LoopRec` du réel (le réel ne
dépend pas du moteur : le rapport `base` le plus récent du même export convient, par exemple celui de la campagne 2 de la
sonde), **refuse** si les boucles retenues ne sont pas ROAD 2 071, GRAVEL 780, MTB 4 ou si les parts `vertices_10` ne
redonnent pas 40,5 / 53,0 / 6,5 et 55,4 / 42,9 / 1,7 % au dixième ; écrit le fichier généré (provenance, aucune donnée
privée), MTB à `tri_q8 = 0`.

#### 15.11.2 Variantes

Composant `org`, combinable avec `svc` ou `common` (le 4c se juge sur le chemin du service du 3b), et ses options :

| Option | Effet | Nom |
|---|---|---|
| `org` | `Organic`, réglages par défaut ; avec `svc`, apprentissage organique | `-org` |
| `w:<x>` | `corridor_w_milli = 1 000 x` (0 : sans couloir) | `-w003` |
| `k:<x>` | `kappa_q8 = 256 x` | `-k15` |
| `tri:0` | `tri_q8 = 0` (lentille au couloir seule) | `-tri0` |
| `pairs:<n>` | `max_pairs = n` | `-pairs8` |
| `wide` | bande [0,25 ; 0,43], écart [25° ; 90°], 4 sous-bandes (§15.0, point 8) | `-wide` |
| `boost:<x>` | `tri_boost_q8 = 256 x` | `-b130` |
| `lc` | avec `svc+org` : apprentissage `Classic`, service organique | `-lc` |

Refus avant toute phase lourde : `org` sans `svc` ni `common` ; une option sans `org` ; `lc` sans `svc` ; une option en double.
`common+org` donne à toutes les équipes le mélange **général** (le service sans profil) ; `svc+org`, celui de l'équipe (formes
du collecteur de la surcouche d'apprentissage, « écart »), sinon le général.

**Contrôle de neutralité** : `svc+org+tri:0+w:0` doit rendre **l'empreinte de `svc`** (I2 au banc) ; `base` et `svc` celles de
la sonde (`4686c904975302e7`, `384f003c0bd19b7f`, `--holdout-share 0.25`, toutes équipes). Affichés au résumé ; arrêt si
`base` diffère.

#### 15.11.3 Ce que le banc enregistre

`play_one` appelle `generate_with_figures_in`. `PlayRec` gagne `fig: FigAgg` (agrégats de `FigureDiagnostics` : `applied`,
`tri_first`, `rho_target`, `candidates`, `pairs`, `pairs_admissible`, `searched`, `leg2_found`, `leg2_capped`,
`leg3_t2`, `leg3_astar`, `leg3_failed`, `complete`, `within_tol`, `spikes`, `spikes_repaired`, `tri_rejected` par code,
`tri_scored`, `p2p_settled`, `leg3_settled`, `beta_final`) ; `GenRoute.family` existe déjà. Rien de privé : des compteurs.

#### 15.11.4 Comparaison (`compare_org.rs`, section « 4c » de `<label>-compare.{md,json}`)

Groupes ROAD 0,25 · ROAD 1 · GRAVEL 0,25 · GRAVEL 1, lectures « toutes » et « écart », toutes équipes puis A et B, premier
parcours gardé, jeux `OK`, normalisation fixe du §12.6 :
1. **Formes** : W1 / IQR de `r_over_d`, `compactness`, `vertices_10`, `far_pos`, `detour_far`, `sinuosity_10`,
   `turns_per_10km`, `shared_share`, par variante, et l'écart à `svc` ; P50 de r / D et de la compacité ; part des premiers
   à `vertices_10 = 2`, part des premiers `Triangle`, et r / D et compacité médians **par famille**.
2. **Goût** : `S / S_base` par variante ; **fait (d) organique** : A ROAD « toutes », `svc-org − common-org`, aux deux
   curseurs, à côté de `svc − common` ; **cases à la borne** : pour chaque classe dont le `COMMON_Q8` vaut 64 ou 1 024
   (ROAD, GRAVEL), parts réelle et générée de la longueur (mètres groupés, `class_meters.pooled`) sous `svc` et `svc-org`, et
   le nombre de classes à la borne cumulée `[1/4, 4]` des profils de A et B.
3. **Service** : P50 et P90 par appel contre `svc` du même tirage, part `OK`, `PANIC`, `WINDOW_ESCAPE`, `VIOLATION`.
4. **Triangles** : les compteurs du §15.11.3 (rendements : paires admissibles par requête, triangles complets, à ±10 %,
   gardés par le contrôle ; refus par code ; épis ; β final P10 · P50 · P90).
5. **Lecture, en faits** : les critères du §15.12, chacun « tient » ou « ne tient pas » avec ses chiffres.

### 15.12 Critères

Candidat : `svc-org` aux réglages retenus (§15.6) ; référence : `svc` du même tirage. Lecture « toutes », toutes équipes,
quatre groupes, sauf mention.

**Formes**
- **A1, r / D** : W1 / IQR ≤ **0,75 ×** celle de `svc` dans les deux groupes ROAD, **≤ 0,85 ×** dans les deux GRAVEL. Attendu
  [calculé, modèle grossier : 40 % de triangles à r / D tiré du réel, 60 % de lentilles à 0,39] : ~0,5 × en ROAD, ~0,7 × en
  GRAVEL. Le P50 de r / D ROAD sous 0,36 (critère du §12 du relevé du 4a) est **rapporté, pas exigé** : tant que les
  lentilles font plus de la moitié des premiers parcours, le médian reste dans les lentilles.
- **A2, figure** : part des premiers à `vertices_10 = 2` dans **[0,40 ; 0,58]** en ROAD et **[0,32 ; 0,50]** en GRAVEL, à
  chaque curseur (réel : 0,530 et 0,429) ; W1 / IQR de `vertices_10` ≤ 0,5 × `svc`. Attendu [calculé] : ~0,45 et ~0,37
  (`p_tri × ~0,70` de triangles gardés, plus 9 % de lentilles comptées à 2).
- **A3, compacité** : ROAD ≤ 0,80 × `svc` aux deux curseurs ; GRAVEL ≤ `svc` + 0,05.
- **A4, le reste** : `far_pos`, `detour_far`, `sinuosity_10`, `turns_per_10km`, `shared_share` ≤ `svc` + 0,10 chacune.

**Goût**
- **B1** : `S / S_base` ≤ celui de `svc` + 0,03 dans les quatre groupes, lectures « toutes » **et** « écart », et ≤ 0,5 (fait
  (a)).
- **B2, réserve du fait (d)** : A ROAD « toutes », `svc-org − common-org` ≤ **+0,054** à chaque curseur (l'acceptation du
  §9.6, +0,044, plus 0,01 de bruit de tirage) ; **réserve levée** si ≤ +0,03 aux deux.
- **B3, cases à la borne** : signalées au propriétaire si, sous `svc-org`, la part générée d'une classe à la borne s'écarte
  de la réelle de plus de max(0,02 ; 0,5 × réelle) **et** plus que sous `svc` ; non bloquant.

**Service**
- **C1, temps au banc** : P50 par appel ≤ **1,15 ×** `svc` à chaque curseur (deux fois le surcoût calculé de la sonde, +8 %) ;
  P90 ≤ 1,25 × `svc`. Entre 1,15 et 1,20 au premier tirage : second tirage des temps (`--variants base,svc,svc+org…`), moyenne
  des deux.
- **C2, issues** : part `OK` ≥ `svc` − 0,5 point à chaque curseur ; aucune `PANIC` ni `WINDOW_ESCAPE` ; `VIOLATION` ≤ `svc`.
- **C3, conteneur de 3 Gio** (après la bascule, séance du §14.13 rejouée) : P50 à 4 clients ≤ 0,56 s (1,15 × 0,49 s), en
  série ≤ 0,29 s (1,15 × 0,25 s) ; pic `anon` ≤ 2,4 Gio ; `oom` 0 ; tous les dépôts en 200.
- **D, déterminisme** : mêmes empreintes à 1 et 16 fils (B et C).

**Bascule du défaut sur `organic`** : A1, A2, A3 (ROAD), A4, B1, B2, C1, C2 et D tiennent → commit de bascule, puis C3 ; si C3
échoue, le commit est annulé (`git revert`) et le relevé le dit. Sinon le défaut reste `classic` et le relevé nomme les
critères manqués. La fusion revient au propriétaire.

**Règle L, apprentissage** : `svc-org-lc` (appris en `Classic`, servi organique) contre `svc-org` : si `S / S_base` de `lc` ≤
celui de `svc-org` + 0,02 dans les quatre groupes « toutes » **et** si B2 tient pour `lc`, `learn_figures` reste `classic` (aucun
profil invalidé, coût d'apprentissage inchangé) ; sinon `learn_figures = organic` au commit de bascule : tous les profils
deviennent `incompatible`, donc `stale` raison `profile`, et se refont à la nuit suivante (~10,5 min de profils par
environnement, × ~1,08 [calculé sur le §14.0 et le §6 de la sonde]).

**Gain de triangles trop faible** (A2 sous la borne basse) : `boost` = min(1,5 ; part réelle / part obtenue), joué en
campagne 2 ; retenu s'il ramène A2 dans l'intervalle sans faire manquer A1, B1 ou C1.

### 15.13 Tests (synthétiques, aucune donnée de production)

**`ord-loops`**
1. I1 : les deux références figées passent telles quelles ; `team_reference.txt` inchangée.
2. I2 : sur le scénario de référence (deux zooms, 70 requêtes, trois modes, avec équipe), `Organic { mix: LENS_ONLY, tune: {
   corridor_w_milli: 0, .. } }` rend des `snapshot` égaux à `Classic`, ligne par ligne.
3. Référence organique : `tests/fixtures/organic_reference.txt`, bénie au commit 3 avec `tri_q8 = 256` et les réglages par
   défaut, rejouée ensuite ; **re-bénissable** au réglage, en le disant au commit.
4. Triangle sur une grille de 48 × 48 : `tri_q8 = 256`, une boucle dans un cap → au moins une requête rend un
   `Family::Triangle` ; P1 et P2 sont sur le parcours, dans l'ordre et le sens tirés ; longueur à ±10 %.
5. Admissibilité : sur 200 paires tirées de grilles avec virages et sens uniques, `astar_p2p` (bornes, `slack` 2 500) trouve
   le **même coût** que la même recherche à heuristique nulle ; avec `slack` 0, au moins un cas plus cher est toléré (le
   défaut que la sonde a mesuré).
6. Facteur minimal : pour toutes les chaînes d'une fenêtre de test, trois modes, familiarités {0 ; 0,5 ; 1}, λ {1 ; 3 ; 5},
   avec et sans goût : `chain(c) ≥ f_min · len_dm(c)`.
7. Couloir : deux routes parallèles à 200 m et une troisième à 4 km, la troisième moins de 1,5 fois plus longue : sans
   couloir le retour prend la parallèle, avec `w` couvrant 200 m et κ = 2 il prend la troisième ; queue et zone du sommet non
   marquées (lecture directe de la trame).
8. Épi : un sommet au bout d'un peigne → épi détecté ; P' est le dernier nœud de degré ≥ 3 à 500 m ; sans P' possible, paire
   jetée.
9. Tirage : sur 10 000 graines, part de `tri_first` à `tri_q8 / 256` ± 0,02 ; ρ* dans [P10 ; P90] ; `FIGURE_SALT` ne change
   rien à la suite de la lentille (test 2).
10. Choix : sans triangle, l'ordre de `Classic` ; avec, premier de la famille tirée, préférence à note voisine.
11. I4 : déterminisme et fenêtre plus grande (`any_superset_of_the_needed_tiles_gives_the_same_routes_and_no_escape`) étendus
    à `Organic`.

**`ord-loops-check`** : 12. `engine_vs_checker` étendu à `Organic` (`tri_q8 = 256`) : tout parcours rendu passe (I3).

**`ord-graph-match`** : 13. mesures déplacées : tests du banc passés tels quels ; 14. `ShapeAcc` : boucle triangulaire → k = 2,
aller-retour → 1 ; comptes indépendants du plafond et de l'ordre ; quantiles ; `mix_for` sous 50 boucles → général ;
15. format : en-tête avec `shapes`, aller-retour ; en-tête sans `shapes` → `None` ; décodage par une copie de la structure de
1.10 (sans le champ) → succès ; `LearnParams::hash` inchangé à `Classic`, changé à `Organic`.

**`ord-server`** : 16. `figures` absent → réglage ; `organic` → famille `triangle` possible, `figures` rendu ; 17. profil avec
formes → mélange de l'équipe (crochet de test qui lit le `Request`) ; 18. capacités, contrat régénéré, validation des
réglages ; 19. dépôt de bout en bout : `shapes` écrit, relu.

**`ord-loops-bench`** : 20. `parse_variants` : les options du §15.11.2 et leurs refus ; 21. `shape-common` sur un rapport
synthétique : refus des comptes, MTB à 0, fichier qui compile.

### 15.14 Commits

Chacun passe `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` et les tests des crates touchés (les tests du
backend de Pédalons ne sont pas concernés) ; avant le dernier, `cargo test --workspace`. Fin de message : ligne vide puis
`Co-Authored-By` du modèle qui code.

1. `refactor(graph-match): mesures de forme pures partagées avec le banc (lot 4, 4c)` — §15.9 premier point, test 13 ; contrôle
   d'empreinte.
2. `feat(loops): recherche point à point, couloir et facteur minimal, sans effet sur Classic (lot 4, 4c)` — `astar_p2p`
   déplacée, `CorGrid`, `Ban.cor`, `COR` retiré, `min_w_per_dm`, plafond ajusté ; tests 1, 5, 6, 7 ; la sonde rejouée sur une
   équipe (même relevé).
3. `feat(loops): figures organiques, triangle et lentille au couloir (lot 4, 4c)` — §15.3 à §15.8 sans les constantes
   (`FigureMix::general` provisoire = `LENS_ONLY`), `generate_with_figures_in` ; tests 2, 3, 4, 8 à 12.
4. `feat(bench): forme commune exportée et constantes du moteur (lot 4, 4c)` — `shape-common`, `shape_common.rs` généré,
   test 21 ; la référence organique re-bénie (le mélange général change), en le disant.
5. `feat(graph-match): formes de l'équipe au dépôt, dans l'en-tête du profil (lot 4, 4c)` — §15.9 ; tests 14, 15.
6. `feat(bench): likeness, variantes organiques et comparaison du 4c` — §15.11 ; test 20. Puis campagnes du §15.15.
7. `feat(server): figures organiques en option, famille triangle, formes du profil (api 1.11, 1.11.0)` — §15.10 ; tests 16 à
   19.
8. Si les critères tiennent : `feat(server): figures organiques par défaut (lot 4, 4c)` — `generator.figures = organic`,
   `learn_figures` selon la règle L, réglages retenus en `OrganicTune::default()` (et la référence organique re-bénie si le
   réglage a changé), `CHANGELOG` « Changé » ; puis la séance en conteneur.
9. `docs(ord-loops): relevé du 4c, figures organiques au banc (lot 4)` — `ord-loops-lot4c-mesures.md` (style des relevés, marques
   de l'en-tête, lettres d'équipes et agrégats seulement), §15.19 « Écarts du codage », étape 4 du §10 marquée.

### 15.15 Campagnes, temps, mémoire

Une à la fois, la machine pour elle seule (16 cœurs, 30 Go) ; sorties sous `~/.cache/osm-route-details/graph/lot4/4c/`
seulement. `B=./target/release/ord-loops-bench`.

| # | Après | Commande (`$B likeness …`) | [estimé] |
|---|---|---|---|
| 0 | commit 1 | `--teams C --limit 5 --label 4c-refacto` : empreinte d'avant | 1 min |
| 1 | commit 6 | `--teams C --limit 20 --holdout-share 0.25 --variants base,svc,svc+org+tri:0+w:0 --label 4c-neutre` : `svc-org-tri0-w0` = `svc` | 3 min |
| 2 | — | `--teams B,C --limit 20 --holdout-share 0.25 --variants base,svc,common+org,svc+org --label 4c-essai` | 6 min |
| 3 | — | `--teams B,C --holdout-share 0.25 --variants base,svc+org --threads {1,16} --label 4c-det{1,16}` | 25 min |
| 4 | — | `--holdout-share 0.25 --variants base,common,svc,common+org,svc+org,svc+org+w:0,svc+org+tri:0,svc+org+lc --label lot4-4c` | ~65 min, RSS ≤ 21 Gio |
| 5 | — | `--holdout-share 0.25 --variants base,svc,svc+org+w:0.02,svc+org+w:0.05,svc+org+pairs:8,svc+org+wide --label lot4-4c-r1` | ~55 min |
| 6 | — | `--holdout-share 0.25 --variants base,svc,svc+org+w:<w*>+k:1.5,svc+org+w:<w*>+k:3[,…+boost:<b>]` `--label lot4-4c-r2` | ~40 min |
| 7 | commit 8 | `--holdout-share 0.25 --variants base,common,svc,common+org,svc+org --label lot4-4c-final` (défauts retenus), puis second tirage des temps | ~45 + 25 min |
| 8 | commit 8 | séance en conteneur du §14.13 (image `osm-route-details:lot4-4c`, défauts organiques) | ~45 min |

Estimations sur la campagne 4 du 3b (13 variantes en 84 min, pic 18,2 Gio) et la sonde (`base` + `svc` en 12 min, pic 19,5
Gio), +8 % par jeu organique. Total machine ~6 h. `wide` et `pairs:8` informent les questions 2 et 6 du §15.18 ; ils ne
deviennent défauts que s'ils gagnent 0,05 de W1 / IQR de r / D sans manquer C1.

### 15.16 Risques

- **Rendement du triangle** après contrôle sous les ~70 % supposés : A2 manque ; levier `boost`, puis `wide`.
- **Couloir et plafond** : un retour plus coûteux dépasse plus souvent `2 h0` ; le plafond suit κ (§15.6), C2 le surveille.
- **Temps** : le GRAVEL est déjà à 527 ms au P50 au banc avant tout triangle (question 1).
- **Une équipe fait la statistique** : A pèse 84 % des boucles ; la répartition générale est la sienne ; B ROAD (53 %
  de lentilles) est la seule équipe qui en diffère et dépasse le seuil.
- **Table commune apprise en `Classic`** : si les triangles déplacent les classes, B1 ou B2 manquent ; le remède (rejouer la
  chaîne commune en organique) change `Common::hash` et invalide tous les profils : décision du propriétaire, pas du
  codeur.
- **La lentille garde son r / D** (0,39 contre 0,353 et 0,300) : le gain de forme vient des seuls triangles (question 6).
- **Le piège du `Debug`** (§15.0, point 3) : un champ ajouté par mégarde à `Route` ou `Diagnostics` casse I1 sans rien changer
  au comportement ; le test 1 le voit, ne pas re-bénir.
- **Déploiement roulant** : 1.10 ignore `shapes` et ne connaît pas `figures` (refus d'une clé inconnue) ; Pédalons ne
  l'envoie pas ; la bascule du défaut ne touche que les instances 1.11.

### 15.17 Ce que le 4c ne fait pas

Quadrilatère ; r / D visé pour la lentille ; figures en strict et en aller simple ; attracteurs et destinations (4d) ;
réapprentissage de la table commune ; changement du contrôleur (`ord-loops-check`), du format du graphe, des vues, de
`params_hash` ; figure choisie par la personne (§9.1, point 5 : `figures` sert l'exploitation et le banc, Pédalons ne
l'envoie pas) ; rien au backend de Pédalons ; **rien n'est poussé, aucune image publiée, aucun déploiement**, aucun accès à la
production.

### 15.18 Questions au propriétaire

Chaque question a une option recommandée, **appliquée par défaut** dans la spécification ; une réponse différente change un
réglage ou un seuil, pas l'architecture.

1. **Où lire la cible « médiane sous 500 ms jusqu'à 100 km » ?** Au banc à 16 fils, la génération 1.10 est déjà à 527 ms au
   P50 en GRAVEL de 30 à 60 km et 966 ms de 60 à 100 km, avant tout triangle [mesuré, sonde §2] ; le triangle ajoute ~8 %
   [calculé]. En conteneur, le 3b sert 0,25 s en série et 0,49 s à 4 clients [mesuré]. **Recommandé** : la lire en
   conteneur (C3 : ≤ 0,56 s à 4 clients, ≤ 0,29 s en série) ; au banc, juger le 4c par rapport à `svc` (C1 : P50 ≤ 1,15 ×).
2. **Combien de paires par requête ?** Avec 4, 63 % des requêtes ont un triangle à ±5 % de D et 78 % à ±10 %, pour ~23 ms et
   0,29 × T1 d'états au P50 [mesuré / calculé, sonde §5 et §6]. **Recommandé** : 4, lentille en repli ; `pairs:8` mesuré en
   campagne 5, retenu seulement s'il gagne 0,05 de W1 / IQR de r / D sans manquer C1.
3. **Seuils de la réserve du fait (d)** : acceptée à +0,021 · +0,044 (A ROAD, `svc` contre `common`) [mesuré, §9.6].
   **Recommandé** : réserve **levée** si `svc-org − common-org` ≤ +0,03 aux deux curseurs ; **maintenue** jusqu'à +0,054 (le
   bruit d'un tirage, 0,01) ; au-delà, la bascule ne se fait pas et la table commune organique vous est proposée (elle
   invalide tous les profils, une nuit de redépôt, ~10,5 min de profils par environnement [calculé, §14.0]).
4. **La bascule du défaut sur `organic` par le codeur, sur critères ?** **Recommandé** : oui, dans un commit à part de la
   branche, si A1 à A4, B1, B2, C1, C2 et D tiennent, suivi de la séance en conteneur (annulé si C3 manque) ; la fusion reste
   la vôtre. Coût éventuel : si la règle L choisit l'apprentissage organique, une nuit où les équipes sont servies par la
   table commune.
5. **Le mode strict reste-t-il en lentille seule ?** Aucun jeu strict au banc (curseurs 0,25 et 1) [mesuré, 4a] ; les
   triangles y seraient servis sans mesure. **Recommandé** : oui pour le 4c (`organic` sans effet en strict et en aller
   simple), à rouvrir avec un banc strict.
6. **Après le 4c, la lentille ou le 4d ?** Le 4c laisse le r / D des lentilles à ~0,39 contre 0,353 (ROAD) et 0,300
   (GRAVEL) réels [mesuré, 4a §4.3], et les lentilles restent ~60 à 70 % des premiers parcours [calculé, §15.12 A2].
   **Recommandé** : passer au 4d si A1 tient ; sinon, une étape « lentille au ρ* » (le demi-tour choisi aussi sur r / D
   visé, à mesurer au même banc) avant le 4d.

### 15.19 Écarts du codage

Écrits le 18 novembre 2026 au codage des étapes 1 à 7 et 9 du §15.14 (branche `feat/ord-loops-lot4c`). **L'étape 8 (la
bascule du défaut) n'est pas faite** : elle attend les campagnes du §15.15 (non lancées par le codeur, §15.17). Relevé de
l'essai de fumée et commandes des campagnes : [`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md).

1. **Un commit de plus : `fix(loops)`, entre les étapes 5 et 6.** Au premier essai de fumée (équipe C, 20 boucles, ROAD),
   62 % des triangles complets sont refusés pour `SHARP_TURN` et seuls 40 % des requêtes gardent un triangle (attendu : ~70 %).
   Cause : le chemin d'arrivée de l'arbre retour est le même pour toutes les paires et porte souvent un virage que le
   contrôle refuse (la règle des grappes de 100 m, que la recherche ne modélise pas) ; les transitions interdites que le
   refus ajoute ne servaient qu'à la lentille, les quatre paires étant déjà « cherchées ». **Une paire refusée pour un virage
   serré n'est plus comptée comme cherchée pour la requête** : la passe suivante la reprend avec les transitions interdites en
   plus (elle compte pour la passe : au plus `max_pairs` essais par passe). Rendement mesuré : 77 % des requêtes ROAD avec un
   triangle gardé. À revoir aux campagnes ; la référence organique figée ne change pas.
2. **Facteur minimal (§15.0 point 11, §15.5)** : le facteur 0,999 n'absorbe pas l'arrondi `+ 128 >> 8` sur une chaîne courte
   (test 6 : coût 242 pour 0,1729 × 1 400 = 242,08). `min_w_per_dm` est calculé sur `max(w − 16, 0) / len_dm` (16 unités de
   poids, constante `ROUND_MARGIN`), et `Weighter::min_factor` rend 0 quand le facteur d'équipe (`min(1, λ, min M / 256)`) tombe
   sous 0,1 (la marge ne couvre plus l'arrondi) : l'heuristique est alors celle des deux bornes triangulaires seules. En strict
   le facteur ne contient pas λ (le strict ne joue pas de triangle).
3. **La référence organique n'est pas re-bénie au commit 4** : `organic_reference.txt` se joue avec un mélange explicite des
   tests (`tri_q8 = 256`, r / D de `[230, 270, 318, 360, 420]`), non avec `FigureMix::general` ; les constantes générées ne
   la touchent donc pas. `shape-common` a été lancé sur `lot4-3b-commune100-base-report.json` (ROAD 2 071, GRAVEL 780, MTB 4,
   parts redonnées au dixième) : ROAD `tri_q8` 145, r / D `[252, 286, 318, 345, 363]` ; GRAVEL 112, `[207, 236, 267, 294, 316]`.
4. **Apprentissage organique des variantes du banc** : le service joue `OrganicTune::default()` à l'apprentissage
   (`LearnFigures::Organic` n'a pas de réglage). Pour que `svc+org+tri:0+w:0` rende l'empreinte de `svc` (§15.11.2) et que
   `svc+org+w:0.02` apprenne ce qu'on lui sert, `BenchPlayer` réécrit les figures des requêtes d'apprentissage avec les
   options de la variante (`Org::rewrite`) ; sous `lc`, l'apprentissage reste classique. Contrôle fait : `svc-org-tri0-w000`
   et `svc` ont l'empreinte `c584b04e30bf54e9` (équipe C, 20 boucles).
5. **Noms des variantes** : `w:<x>` donne `-w<centièmes sur 3 chiffres>` (`w:0` : `-w000`, pas `-w0` comme le §15.15), `k:1.5` `-k15`,
   `boost:1.3` `-b130`, `tri:0` `-tri0`, `pairs:8` `-pairs8`, `wide`, `lc`.
6. **Tests** : le test 17 (« crochet de test qui lit le `Request` ») est remplacé par l'accesseur `test_profile_shapes`
   (formes écrites au dépôt et relues) et les tests unitaires de `mix_for` ; le test 5 ne fait qu'écrire le nombre de cas
   plus chers sans la marge (36 sur 191 paires), sans l'exiger ; les tests 8 lisent `is_spike` et `repair_prefix` sur un
   peigne (réseau synthétique à carrefours tous les 500 m), non la phase complète.
7. **Critères non évalués par le banc** : D (empreintes à 1 et 16 fils, campagne 3) et C3 (conteneur) sont marqués « non
   évalué » dans la lecture ; B2 demande `common+org` dans le même tirage (campagnes 4 et 7, pas 5 ni 6 : sans lui, le verdict
   de bascule reste « non évalué »). Le choix de `w` et de κ de la campagne 6 (§15.6) se lit dans les tableaux, il n'est pas
   automatisé.
8. **`spikes_kept`** compte, sur la polyligne des triangles **gardés** (tous ceux du jeu, pas seulement le premier), les sommets
   gardés à 10 % de D dont l'angle d'arrivée et de départ atteint 150° à 500 m ; un aller-retour compte 1 par nature, on ne
   compte que les triangles.
9. **`ProfileMeta.shapes: Option<ShapeProfile>`** n'est pas mis en boîte : `ProfileRead` porte `#[allow(clippy::large_enum_variant)]`
   (une valeur par lecture d'équipe).
10. **Sans objet pour 1.10** : `figures` n'existe pas dans le contrat de 1.10, qui refuse une clé inconnue ; Pédalons ne
    l'envoie pas (§15.16).

**Relecture du 18 novembre 2026** (commits `1d218ed` à `1b91dfd`, après celui du relevé) : quatre corrections, qui changent
les points 1 et 2 ci-dessus ; les références figées, Classic et organique, passent sans re-bénédiction.

11. **Le facteur minimal est lu sur T1, pas sur la fenêtre** (remplace le point 2 ; écart au §15.5 et au choix 9 du §15.1).
    Avec la marge de 16, le plus petit `(w − 16) / len_dm` de la fenêtre vaut **0** sur le graphe France dans les 30 fenêtres
    relevées (Paris, Lyon, Nantes, Clermont-Ferrand et un départ rural ; 30, 60 et 100 km ; ROAD et GRAVEL ; fenêtres des tuiles
    nécessaires et de classe) [mesuré] : une chaîne de moins de 1,6 m suffit, et le terme du vol d'oiseau de l'heuristique
    était mort en service. Sans la marge, le minimum dépend de la fenêtre (GRAVEL, Nantes, 30 km : 0,175875 sur les tuiles
    nécessaires, 0,175782 sur la fenêtre de classe [mesuré]), alors que le service sert une requête dans toute fenêtre du cache
    qui la contient : I4 serait rompu. Le facteur vaut désormais 0,999 fois le plus petit `chain(c) / len_dm(c)` des états
    fixés par T1 de la passe, calculé à la première recherche de la passe : T1 fixe tout état atteignable dans le disque, donc
    toute chaîne qu'une jambe 2 relâche (au prédécesseur près de la règle de rétro-virage : un écart borné, déterministe) ;
    `chain` est le poids exact, sans arrondi à couvrir ; T1 ne dépend pas de la fenêtre. Coût : une passe sur les états de T1 (P50 145 000 en ROAD,
    190 000 en GRAVEL, §2 de la sonde) par passe qui cherche une paire. `Weighter::min_factor`, `Window::min_w_per_dm`, `WindowTeam::min_w_per_dm`
    et `ViewData::min_w_per_dm` sont retirés. Test 6 : la borne tient pour toute chaîne atteinte par T1, et le facteur est le
    même sur la fenêtre des tuiles nécessaires et sur le graphe entier.
12. **Le gabarit du couloir suit le rayon de sa requête.** `CorGrid::begin` ne refaisait le gabarit que si la trame changeait ;
    à case bornée (500 m) et côté égal, deux requêtes peuvent demander deux rayons (D = 66,6 puis 66,7 km à w = 0,03 D : rayon
    4 puis 5 cases) : la seconde héritait du gabarit de la première, et la sortie dépendait de la requête jouée avant dans le
    même espace de travail (D, I4). Test `a_reused_grid_dilates_with_the_radius_of_its_request`.
13. **Au plus `max_pairs` paires distinctes par requête** (amende le point 1). La reprise retirait la paire des paires
    cherchées : quatre paires neuves par passe, jusqu'à 24 recherches par requête, contre la décision 2 du §9.7. Une paire
    prend désormais sa place à la première recherche et la garde ; refusée pour un virage serré avec de nouvelles transitions
    interdites, elle est reprise **une fois** à la passe suivante, sans place de plus (les reprises non jouées faute de budget
    passent à la passe d'après). `OrganicTune::sharp_retry` (défaut `true`), option `noretry` du banc (`-noretry`, la lettre du
    §15.7), compteur `FigureDiagnostics::retried` (colonne « cherchées (reprises) » de la section « 4c ») ; le test de
    déterminisme vérifie `searched − retried ≤ max_pairs`.
14. **Une seule réparation d'épi par triangle** (§15.5) : un second épi, en P2, après une réparation en P1, jette la paire.
15. **Non changé, à surveiller** : des triangles complets sont refusés pour `REUSE_OUTSIDE_TAIL` (11 sur 100 à ±10 % de D, C
    ROAD, `svc+org`, essai de relecture). La jambe 2 tourne sur le graphe privé de la jambe 1 avec `TAIL` au coût × 2 (§15.5)
    alors qu'elle ne passe jamais dans la queue du parcours (ses abscisses sont au-delà de 0,27 D des deux bouts) : tout
    réemprunt de la queue de la jambe 1 par la jambe 2 est refusé par le contrôle. La mettre en `BAN` serait sans risque, mais
    la cause des onze refus n'est pas établie (la jambe 3 peut aussi déborder la queue) ; laissé à la lecture des campagnes.

Essai de relecture (équipes C et B, 20 boucles, mêmes réglages que l'essai du codeur, variantes `base`, `svc`, `common+org`,
`svc+org`, `svc+org+noretry`, `svc+org+tri:0+w:0`) : chiffres au §3.4 de [`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md).

**Campagnes du 18 novembre 2026** (campagnes 1 à 6 du §15.15, binaire de `ea6ef63` ; relevé :
[`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md), §4 à §13). Choix faits par prudence là où la spécification ne
tranche pas :

16. **Campagne 6 à `w` = 0,02, choisi sans C1.** La règle du §15.6 retient le réglage de plus petite compacité ROAD parmi ceux qui
    tiennent C1, C2 et B1. Aucun réglage de la campagne 5 ne tient C1, si bien que la règle n'a aucun candidat. La campagne 6 a
    été jouée au `w` que la règle désigne quand on lui retire C1 (0,02 : C2 et B1 tiennent, r / D dans la marge de 0,03). Elle
    ajoute `boost:1.5` (A2 sous la borne en GRAVEL : 0,429 / 0,283 = 1,52, plafonné à 1,5) et une variante hors plan,
    `svc+org+w:0.02+noretry`, qui mesure le levier du temps. Le choix « au meilleur `w` » est donc indicatif.
17. **Aucun réglage retenu.** Faute de candidat qui tienne C1, A1 et A2 ensemble, `OrganicTune::default()` garde les valeurs du
    §15.8. La référence organique n'est pas re-bénie, et `pairs:8` comme `wide` ne sont pas retenus (décision 2 du §9.7 ; gain de
    W1 / IQR de r / D de `pairs:8` : 0,048 en moyenne, et C1 manque).
18. **Pas de campagne 7, pas de C3.** Elles suivent la bascule, qui n'est pas faite. Une séance en conteneur **indicative** les
    remplace pour le temps (§11 du relevé). L'image est construite sur `ea6ef63`, aux défauts `classic`. Les figures y sont
    choisies par requête (`figures: classic`, puis `organic`) sur les mêmes départs et le même volume, et une 1.10 (image du 3b)
    est jouée sur ce volume. Le cache de pages n'a pas été évincé (`sudo` sans mot de passe indisponible), comme à la séance du 3b
    qui utilisait le même script.
19. **Lecture de C2.** Un jeu `VIOLATION` est un jeu dont tous les parcours rendus sont refusés par le contrôleur indépendant. Sous
    `svc-org`, ce contrôleur refuse 34 parcours sur 19 479 pour `SHARP_TURN` ; il en refuse 0 sous `svc`, 5 sous `common` (en
    `Classic`), 10 sous `tri:0` et 2 sous `w:0`. Le contrôle interne et le contrôleur indépendant divergent donc déjà en `Classic`,
    et le couloir multiplie les cas. Le service ne rend que ce que le contrôleur accepte : ce n'est pas une faute de I3. Le critère
    est appliqué tel quel (2 jeux contre 0 : C2 manque). Rien n'est changé (§15.17 : le contrôleur n'est pas touché).
20. **B2 négatif.** `svc-org − common-org` vaut −0,058 · −0,045, parce que la table commune apprise en `Classic` sert mal les
    figures organiques (`common-org` perd 0,07 sur `common` en A ROAD) ; la réserve est levée par la lettre de la décision 3 du
    §9.7 (≤ +0,03).
21. **Point 15, lu** : `REUSE_OUTSIDE_TAIL` ne pèse que 6,8 % des triangles ROAD à ±10 % de D sous `svc-org` (753 sur 11 143), contre
    `SHARP_TURN` 55 % (6 147). La jambe 2 reste en `TAIL` au coût × 2 : la passer en `BAN` ne lèverait pas le frein principal.
22. **Mineurs, non corrigés** : au banc, quand l'équipe A n'est pas jouée, le détail de B2 écrit « `common-org` absent » même si la
    variante est jouée (le critère lit A seulement) ; `w:0.025` et `w:0.03` portent toujours le même nom de variante.

## 16. 4c, étape « virages » : spécification

Écrit le 18 novembre 2026 sur la branche `feat/ord-loops-lot4c`, après les campagnes du 4c
([`ord-loops-lot4c-mesures.md`](ord-loops-lot4c-mesures.md)) et les décisions du §9.8, à la lecture de `ord-loops`
(`verify.rs`, `search.rs`, `reverse.rs`, `generate.rs`, `figures.rs`), de `ord-loops-check` (`check.rs`, `geo.rs`,
`bounded.rs`) et de `ord-graph/src/view.rs` (caps des chaînes). Cette section **suffit au codeur** ; ce qu'elle ne dit pas est
laissé à son jugement, à condition de tenir les invariants du §16.6. Les §15.0 à §15.19 restent vrais, sauf ce que le §16
remplace explicitement.

**Ce que livre l'étape** : (1) en organique, toutes les recherches respectent la règle des virages du contrôle (virage simple et
grappes de 100 m), par un contrôle incrémental à la relaxation ; (2) en organique, aucune jambe ne repasse sur l'une de ses
chaînes ; (3) en organique, le contrôle interne calcule les caps comme le contrôleur indépendant ; (4) le banc (options,
A4 en absolu) ; (5) trois campagnes, puis la bascule si les critères du §16.11 tiennent.

### 16.0 Ce que la mesure a établi

Instrumentation **non commitée** (relevés écrits par variables d'environnement dans le banc, le contrôle interne et le
contrôleur ; correctif gardé hors dépôt avec les sorties sous `~/.cache/osm-route-details/graph/lot4/4c-virages/`). Deux
tirages : la campagne complète `base,common,svc,svc+org` (toutes équipes, `--holdout-share 0.25`), dont les empreintes sont
celles de la campagne 4 (`base` `4686c904975302e7`, `common` `07b62287e64fa258`, `svc` `384f003c0bd19b7f`, `svc-org`
`c48118447315c118`) : l'instrumentation est neutre ; et l'essai de fumée `--teams B,C --limit 20`, rejoué trois fois
(`svc-org` `27952d4befd451f4` aux trois).

1. **Contrôle interne contre contrôleur (34 contre 5) : une seule cause, la projection** [mesuré]. Le contrôleur refuse
   `SHARP_TURN` **43** parcours que le contrôle interne a acceptés : 4 sous `base`, 5 sous `common`, 0 sous `svc`, **34** sous
   `svc-org`. Dans les 43 cas, les degrés, les longueurs de grappe, les bouts exemptés et l'épingle sont identiques des deux
   côtés ; seuls les caps diffèrent. Le contrôle interne prend les caps dans un repère `Local` centré **au départ** du parcours
   (`verify.rs`, `loc = Local::new(s_e7)`) ; le contrôleur dans un repère centré **au nœud** (`Proj::new(origin)`,
   `check.rs::heading`). L'échelle des x diffère de `cos φ₀ / cos φ` : l'angle du contrôle interne est plus petit de 0,04° à
   1,34° (le nœud est à 10,5 à 81,6 km du départ), et l'angle du contrôleur vaut 150,024° à 150,056°. Recalculé dans un repère
   centré au nœud, l'angle du moteur redonne celui du contrôleur **à 0,0009° près dans les 43 cas**.
   Les 34 cas de `svc-org` (33 GRAVEL, 1 ROAD) se groupent à **trois distances du départ** (24 refus à 10,5 km,
   9 à 14,2 km, 1 à 81,6 km) : quelques carrefours, que les figures organiques traversent plus souvent [calculé]. Le « couloir multiplie les cas » du §15.19
   (point 19) n'est donc pas une propriété du couloir.
   **L'aligner change forcément le `Classic`** : `base` et `common` rendent 9 candidats que le contrôle aligné refuserait, et la
   recherche continuerait autrement, si bien que l'empreinte de `base` changerait. L'alignement ne s'applique donc qu'en
   organique (décision 4 du §9.8). En `Classic`, le service continue d'écarter ces parcours au contrôleur (`checker_rejected`),
   comme 1.10.
2. **Où tombent les virages refusés des triangles** [mesuré, essai B et C, `svc+org`, 1 487 triangles refusés pour
   `SHARP_TURN`, deux surcouches, reprises comprises] : 12 897 paires de nœuds refusées, dont 2 082 par la règle du virage simple
   et 10 815 par la règle des grappes ; 2 723 endroits distincts, dont **70 % au milieu d'une jambe** (à plus d'un kilomètre d'un
   sommet et des bouts). La jambe 1 (le chemin de T1) porte le virage dans **38 %** des triangles refusés (571), la jambe 2 seule
   dans 29 % (426), la jambe 3 seule dans 23 % (348). Le motif : une suite de chaînes courtes (3 à 30 m : giratoires, carrefours
   à plusieurs nœuds, ouvertures de terre-plein) où chaque virage est permis (< 150°) mais où leur somme est un demi-tour (150° à
   178°). La recherche ne voit qu'un nœud à la fois, avec les caps de bout de chaîne : la règle des grappes n'y est pas, et pour
   une chaîne de moins de 50 m le cap prolongé (`in_head_ext`, mélange avec le prédécesseur) est loin de la fenêtre de 30 à 50 m
   du contrôle (écart médian ~80°) ; 1 477 des 2 082 refus simples ont leurs deux chaînes sous 50 m. Une grappe refusée enjambe 1
   à 9 chaînes intérieures (1 : 3 016 paires, 2 : 2 710, 3 : 2 019, 4 à 9 : 3 053).
3. **Un modèle calculable sur la seule vue suffit** [mesuré, même essai]. Caps repris sur un chemin approché : une chaîne de
   50 m ou plus est droite le long de son cap de bout (`v.head`) ; une chaîne plus courte est le segment entre ses deux nœuds
   (`v.pos`) ; fenêtres de 30 à 50 m sur ce chemin, comme le contrôle. Sur les paires refusées où il se calcule (2 058 et 10 810), le modèle trouve ≥ 150° dans
   82 % (simple) et 84 % (grappe) des cas, **≥ 145° dans 91 % et 89 %**, ≥ 140° dans 93 % ; sur les triangles **acceptés**
   (2 780 triangles, 1 728 941 paires de nœuds hors des 250 m des bouts), il dépasse à tort 145° sur 439 paires (2,5 × 10⁻⁴), et
   150° sur 38 (2,2 × 10⁻⁵).
4. **`REUSE_OUTSIDE_TAIL` (point 15 du §15.19) : la cause s'établit** [mesuré, campagne complète, `svc-org`, 1 796 triangles
   refusés avec ce code, deux surcouches]. **1 695 (94 %)** contiennent un **aller-retour dans une même jambe** : une chaîne prise
   dans un sens puis dans l'autre par la même jambe (jambe 1, le chemin de T1 : 840 ; jambe 2 : 609 ; jambe 3 : 266). Dans l'essai,
   tous ces réemprunts sont en sens inverse, et la seconde passe vient souvent plus d'un kilomètre après la première. La jambe 2
   sur la queue de la jambe 1 (l'hypothèse du point 15) ne pèse que 52 refus (2,9 %), la jambe 3 sur la queue de la jambe 1 avec
   plus de 0,15 D derrière elle 49 (2,7 %). L'étiquetage par état de chaîne orientée n'interdit pas de reprendre une chaîne en sens
   inverse : quand un demi-tour direct est interdit, une recherche peut faire demi-tour par une boucle au bout d'une impasse ou
   d'une route, et revenir sur ses pas.
5. **Une racine commune** : les recherches font demi-tour au milieu d'une jambe, d'une façon que le contrôle refuse, soit par
   une grappe de chaînes courtes (`SHARP_TURN`), soit par un aller-retour (`REUSE_OUTSIDE_TAIL`). Les deux remèdes vont
   ensemble : interdire la seule grappe pousserait les recherches vers l'aller-retour.

### 16.1 Choix tranchés

| # | Question | Tranché | Raison |
|---|---|---|---|
| 1 | Comment la recherche évite les virages refusés | **Contrôle incrémental à la relaxation**, sur le chemin courant (prédécesseurs, puis jambes précédentes), caps du modèle du §16.0 point 3 | Coût nul hors des zones de chaînes courtes ; aucune mémoire ; même dépendance au chemin que `in_head_ext` et `retro_ok` aujourd'hui |
| 2 | État étendu (chaîne, chaîne d'arrivée) | Écarté | ~3 fois plus d'états dans T1 et T2, qui dominent le temps (C1) ; une grappe enjambe jusqu'à 9 chaînes intérieures, une chaîne d'arrivée ne suffit pas |
| 3 | Transitions interdites précalculées aux jonctions | Écarté | Une grappe est une suite de 3 à 11 chaînes, pas une paire ; les triplets ne couvrent que 28 % des paires refusées (une seule chaîne intérieure) ; précalcul par fenêtre et mémoire du cache |
| 4 | Recherches concernées | **Toutes**, en organique : T1, T2, retour de la lentille, ordre inverse, jambes 2 et 3 | La jambe 1 (chemin de T1) porte 38 % des refus |
| 5 | Marge | **5°** (`turn_margin_deg`) : refus du modèle à ≥ 145° | 91 % et 89 % des refus captés, 2,5 × 10⁻⁴ faux refus par paire |
| 6 | Épingle d'une même way | Non modélisée (le modèle refuse là où le contrôle exempte) | Demande les polylignes de base ; rare entre deux nœuds de degré ≥ 3 |
| 7 | Aller-retour dans une jambe | **Interdit** en organique, dans toutes les recherches (test d'ancêtre borné) | 94 % des `REUSE_OUTSIDE_TAIL` |
| 8 | Jambe 2 sur la queue de la jambe 1 | **`BAN`** au lieu de `TAIL` | Toujours refusé par le contrôle (§15.19, point 15) ; 2,9 % des refus |
| 9 | Contrôle interne | Caps et corde d'épingle **centrés au nœud** en organique ; `Classic` inchangé | §16.0, point 1 |
| 10 | Reprises (`sharp_retry`) et interdits appris | **Gardés**, en filet | Les 9 à 11 % de refus que la marge ne capte pas |
| 11 | Réglages | Dans `OrganicTune` (`turns`, `turn_margin_deg`, `no_reuse`), défauts actifs | Le banc les fait varier sans rouvrir le moteur, comme au §15.8 |

### 16.2 Le modèle des virages (`ord-loops`, `turns.rs`, nouveau)

**Données** : rien de nouveau dans la vue ni dans la fenêtre. Positions des nœuds `v.pos` (repère `Disc::xy_of` de la requête),
caps de bout `in_head` et `out_head` (`v.head`, fenêtre de 30 à 50 m de la chaîne, `ord-graph/src/view.rs`), longueurs
`v.len_dm`, degrés `v.deg`.

```rust
pub(crate) struct TurnModel { pub max_deg: f64 /* 150 − marge */, pub cluster_dm: u32 /* 1 000 */, pub free_end_dm: u32 /* 2 000 */ }
```

**Chemin approché et fenêtres.** `pt_back(path, k, d)` : point situé à `d` mètres **avant** le nœud d'arrivée de l'état `path[k]`,
en remontant : une chaîne de 50 m ou plus est droite, du nœud d'arrivée vers l'arrière le long de `in_head` (le point est
`E − d′ · u(in_head)`) ; une chaîne plus courte est le segment entre ses nœuds ; au-delà du chemin connu, le dernier segment est
prolongé. `pt_fwd(path, k, d)`, le symétrique en avançant depuis le nœud de départ de `path[k]` (`out_head`). Cap avant le nœud :
`heading(pt_back(50) → pt_back(30))` ; cap après : `heading(pt_fwd(30) → pt_fwd(50))` ; angle `turn_angle_deg`. Tout en `f64`
(opérations IEEE, résultat identique en natif et en WASM, comme la géométrie d'aujourd'hui).

**Ce que l'ajout d'un état fixe** (recherche **en avant**, chemin finissant par `f`, état candidat `fo`, `b₀ = tgt(f)`) :
- `B` = `b₀` s'il est de degré ≥ 3, plus les nœuds de degré ≥ 3 du chemin situés à **moins de 50 m avant** `b₀` (leur fenêtre
  d'après déborde sur `fo`) ;
- pour chaque `b ∈ B`, les nœuds `a` de degré ≥ 3 du chemin, de `a = b` (virage simple) jusqu'à `run(a, b) ≤ cluster_dm`
  (`run` : somme des `len_dm` des chaînes entières entre `a` et `b`, exactement la boucle du contrôle) ;
- la paire `(a, b)` est **refusée** si `angle(cap avant a, cap après b) ≥ max_deg`, sauf si elle est exemptée aux bouts :
  `b` à `free_end_dm` au plus du départ du parcours (mètres connus : jambes précédentes plus `meters` de la recherche), ou `a` à
  `free_end_dm` au plus de l'arrivée (connu exactement en arrière ; en avant, lu sur `t2.meters` du meilleur état de `a` :
  une approximation, le filet du §16.5 couvre l'écart ; dans T1, construit avant T2, aucune exemption à l'arrivée) ;
- une paire refusée refuse la relaxation `f → fo` (`continue`, comme un virage interdit) ; le compteur `turn_cut` monte.

**Voie rapide** : `len(f) ≥ 50 m` donne `B ⊆ {b₀}` ; `len(f) > 100 m` donne `a = b₀` seul ; si de plus `len(fo) ≥ 50 m`, le modèle
se réduit à `angle(in_head(f), out_head(fo)) ≥ 145°`, sans remonter le chemin. Hors de la voie rapide, `turn_evals` monte (mesure
du coût).

**En arrière** (`tree_backward`, `astar_back` ; état candidat `fg` placé **avant** `f`, `a₀ = src(f)`) : le symétrique. `A` =
`a₀` et les nœuds de degré ≥ 3 à moins de 50 m **après** `a₀` sur la suite connue (successeurs de l'arbre, puis la jambe fixée
derrière) ; pour chaque `a`, les `b` jusqu'à 100 m après ; cap avant `a` prolongé au-delà du début de `fg`. Exemptions : `a` près de
l'arrivée (exact, mètres vers l'arrivée), `b` près du départ (lu sur `t1.meters`).

**Où** (organique et `tune.turns`) : `tree_forward`, `tree_backward`, `astar` (retour de la lentille, jambe 3), `astar_p2p` (jambe
2), `astar_back` (ordre inverse). Le test vient **après** `turn_cost` et `retro_ok`, qui restent tels quels (coûts de virage,
demi-tour, interdits appris) : le modèle ne fait qu'interdire de plus. La sonde (`probe.rs`) n'est pas touchée.

**Contexte des jonctions** : le chemin que remonte le modèle continue au-delà de la racine de la recherche dans les jambes déjà
posées. `astar_p2p` reçoit la jambe 1 ; la jambe 3 (`astar`), les jambes 1 et 2 ; le retour de la lentille, l'aller ; `astar_back`,
le retour fixé derrière lui. Seuls les ~150 derniers mètres (50 + 100) sont lus. `Ctx` gagne `turns: Option<&TurnModel>`, `None`
en `Classic` et quand `tune.turns` est faux ; les fonctions de recherche gagnent un paramètre `ctx_path: &[u32]` (vide en `Classic`).

**Chemins d'arbre gardés tels quels** : la jambe 3 prise sur T2 (§15.5) est rejouée par le modèle sur ses états des 150 premiers
mètres, avec les jambes 1 et 2 pour contexte (`junction_ok`) ; un refus l'envoie à l'A* (comme un réemprunt). La jambe 1 et l'aller
de la lentille sont des chemins de T1, sûrs par construction ; leur jonction avec la jambe suivante est vue par la recherche de
celle-ci. Le retour de l'ordre inverse (chemin de T2) est sûr de même, et sa jonction est vue par `astar_back`.

**Heuristiques** : T1 et T2 portent le modèle. Leurs coûts peuvent dépasser le vrai coût contraint d'un état (l'étiquette dépend du
chemin), si bien que les bornes de `astar_p2p` et `astar` ne sont plus garanties admissibles en organique : la recherche reste
**sûre** (tout chemin rendu respecte le modèle), peut-être un peu moins bonne. Le test d'admissibilité 5 du §15.13 se joue modèle
coupé.

### 16.3 Le contrôle interne aligné

- `verify::check_route_in` ne change pas (`Classic`, banc, sonde). Une variante interne, `check_route_aligned` (ou un paramètre
  `Rules { node_frame: bool }` d'une fonction commune), prend les caps du virage simple et des grappes dans un `Local` **centré au
  nœud** (le nœud entre les morceaux, comme l'origine de `Checker::heading`), et la corde et la portion droite de l'épingle dans un
  `Local` centré au nœud `a` (`Checker::hairpin`). Le reste est inchangé.
- Appelée par la lentille (`generate.rs`) et par les triangles (`figures.rs`) quand la requête est organique et `tune.turns` vrai.
- Le couloir doublé (`CORRIDOR_DOUBLED`) garde son repère : aucune divergence n'y est relevée (§16.0, point 1).
- Attendu : zéro parcours organique rendu refusé `SHARP_TURN` par le contrôleur (34 sous `svc-org`).

### 16.4 Aucun aller-retour dans une jambe (`tune.no_reuse`)

- **Test d'ancêtre borné**, à la relaxation `f → fo` de toutes les recherches organiques : si `t.get(fo ^ 1) < t.get(f)`, remonter
  les prédécesseurs depuis `f` tant que `t.meters(cur) ≥ t.meters(fo ^ 1)` (les mètres décroissent le long des ancêtres : la
  remontée s'arrête au premier ancêtre plus près de la racine) ; trouver `fo ^ 1` refuse la relaxation (`reuse_cut`). En arrière,
  le symétrique sur les successeurs et les mètres vers l'arrivée. Hors du filtre, rien n'est remonté ; dans le cas courant (l'état
  inverse atteint plus tard, ou par un autre chemin), la remontée s'arrête en un ou deux pas.
- La même chaîne dans le même sens est impossible (un état est fixé une fois). Les jambes précédentes sont déjà `BAN`.
- **Jambe 2** : `stamp_leg1` pose `BAN` sur **toutes** les chaînes de la jambe 1 (plus de `TAIL`) ; la jambe 3 et la lentille gardent
  `TAIL` (leur réemprunt de la queue est légitime près de l'arrivée).
- Non traité : la jambe 3 sur la queue de la jambe 1 avec plus de 0,15 D derrière elle (2,7 %).

### 16.5 Reprises, interdits appris

Inchangés : un refus du contrôle interne ajoute ses transitions `SHARP_TURN` aux interdits de la passe suivante, et `sharp_retry`
reprend une fois la paire refusée (§15.19, point 13). Attendu : moins de passes et de reprises, puisque 90 % des refus sont captés
avant ; `noretry` est rejoué à la campagne 3 pour en lire le prix. Aucun changement de défaut sans mesure.

### 16.6 Interface et invariants

```rust
pub struct OrganicTune { /* …, */ pub turns: bool /* true */, pub turn_margin_deg: u8 /* 5 */, pub no_reuse: bool /* true */ }
pub struct FigureDiagnostics { /* …, */ pub turn_cut: u64, pub turn_evals: u64, pub reuse_cut: u64 }   // toute la requête
```

`Generation`, `Diagnostics`, `Route` et `Family` ne gagnent rien (§15.0, point 3).

- **I1** : `Figures::Classic` rend 1.10 au bit près : `window_reference.txt`, `window_reference_192.txt`, `team_reference.txt`,
  `team_check_reference.txt` passent **sans re-bénédiction** ; au banc, `base` `4686c904975302e7`, `common` `07b62287e64fa258`,
  `svc` `384f003c0bd19b7f`.
- **I2 (remplace celui du §15.8)** : `Organic { mix: LENS_ONLY, tune: { corridor_w_milli: 0, turns: false, no_reuse: false, .. } }`
  rend exactement `Classic` sur les 140 requêtes de la référence ; au banc, `svc+org+tri:0+w:0+turns:0+reuse:0` rend `svc`.
- **I2′** : `turns: false` et `no_reuse: false` rendent l'organique de `ea6ef63` : au banc, `svc+org+turns:0+reuse:0` rend
  `c48118447315c118` (toutes équipes) et `27952d4befd451f4` (essai B et C).
- **I3, I4** : inchangés (tout parcours organique passe le contrôleur ; déterminisme ; fenêtre plus grande, même sortie).

### 16.7 Tests (synthétiques, aucune donnée de production)

**`ord-loops`**
1. I1 tel quel ; I2 et I2′ (le test 2 du §15.13 réécrit avec les deux champs).
2. Modèle exact sur chaînes droites : réseau à arêtes droites (dont des chaînes de 5 à 40 m), chemins tirés au hasard : pour toute
   paire `(a, b)` du contrôle, l'angle du modèle égale celui du contrôle aligné à 10⁻⁶° près.
3. Demi-tour composé : deux chaussées à sens unique à 20 m l'une de l'autre, ouvertures de terre-plein tous les 400 m, et un
   giratoire de six nœuds à 10 m : une recherche point à point vers une cible derrière l'état de départ ne rend, modèle actif,
   aucun chemin refusé `SHARP_TURN` par le contrôle aligné ; modèle coupé, elle rend le demi-tour par l'ouverture (garde : le
   test ne prouve rien sinon).
4. De bout en bout, sur ces réseaux : 50 requêtes organiques, aucun `SharpTurn` dans `tri_rejected` ni dans le `rejected` de la
   lentille, modèle actif ; au moins un, modèle coupé.
5. Exemptions : une grappe à moins de 200 m du départ (et de l'arrivée, en arrière) n'est pas refusée.
6. Aller-retour : réseau où le chemin le moins cher de P1 à P2 revient sur ses pas (boucle au bout d'une route) : `no_reuse`
   rend un chemin sans chaîne répétée, ou aucun ; sans, l'aller-retour (garde).
7. Jambe 2 : aucune chaîne de la jambe 1 dans la jambe 2, queue comprise.
8. I4 : déterminisme et fenêtre plus grande avec les nouveaux défauts.
9. Référence organique (`organic_reference.txt`) **re-bénie** aux commits 2 et 3, en le disant.

**`ord-loops-check`**
10. Alignement : réseau à 47,2° de latitude, carrefour à ~45 km au nord du départ dessiné pour un angle de 150,02° au nœud
    (< 150° dans le repère du départ) : le contrôle interne `Classic` l'accepte, le contrôle aligné et le contrôleur le refusent
    (garde : les deux repères doivent diverger, sinon le test ne prouve rien).
11. `engine_vs_checker` organique (I3) avec les nouveaux défauts.

**`ord-loops-bench`**
12. `parse_variants` : `turns:0` (`-turns0`), `reuse:0` (`-reuse0`), `margin:<deg>` (`-m<deg>`), refus en double et sans `org`.
13. A4 en absolu sur des distributions synthétiques (médiane, P90, marge de `svc`).

### 16.8 Banc

- Options `turns:0`, `reuse:0`, `margin:<deg>` (§16.7, test 12) ; `FigAgg` gagne `turn_cut`, `turn_evals`, `reuse_cut`, affichés par
  requête dans la section « 4c ».
- Section « 4c » : refus du **contrôleur indépendant** sur les parcours rendus, par code et par variante (déjà comptés dans
  `dropped`), à côté des refus du contrôle interne ; contrôles d'empreinte I2 et I2′ affichés au résumé, arrêt si l'un manque.
- A4 (`compare_org.rs`) : pour `shared_share`, médiane ≤ 0,05 **et** P90 ≤ P90 de `svc` + 0,05, par type et curseur (lecture
  « toutes », premier parcours gardé) ; pour `far_pos`, `detour_far`, `sinuosity_10`, `turns_per_10km`, la W1 / IQR ≤ `svc` + 0,10
  comme au §15.12.

### 16.9 Commits

Chacun passe `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` et les tests des crates touchés ;
avant le dernier, `cargo test --workspace`. Fin de message : ligne vide, puis `Co-Authored-By` du modèle qui code.

1. `feat(loops): contrôle interne aligné sur le contrôleur pour les virages, en organique (lot 4, 4c)` — §16.3, `OrganicTune::turns`,
   tests 1 (I2) et 10 ; la référence organique re-bénie si elle change, en le disant.
2. `feat(loops): la recherche organique suit la règle des virages du contrôle (lot 4, 4c)` — §16.2, `turn_margin_deg`,
   compteurs ; tests 2 à 5 ; référence organique re-bénie.
3. `feat(loops): aucun aller-retour dans une jambe organique (lot 4, 4c)` — §16.4, `no_reuse` ; tests 6, 7, 8, 11 ; référence
   re-bénie.
4. `feat(bench): options turns, reuse et margin, A4 en absolu pour le partage de route (lot 4, 4c)` — §16.8 ; tests 12, 13.
   Puis les campagnes du §16.10.
5. Si les critères tiennent : `feat(server): figures organiques par défaut (lot 4, 4c)` — `generator.figures = organic`,
   `generator.profile.learn_figures = organic` (règle L du relevé), `CHANGELOG` « Changé », puis la séance en conteneur ;
   `git revert` de ce commit si C3 manque.
6. `docs(ord-loops): relevé de l'étape virages (lot 4, 4c)` — nouvelles sections de `ord-loops-lot4c-mesures.md` (style des relevés,
   lettres d'équipes et agrégats seulement), §16.13 « Écarts du codage », étape 4 du §10 marquée.

### 16.10 Campagnes (trois au plus, une complète)

Une à la fois, la machine pour elle seule ; `B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`, sorties
sous `$G/lot4/4c-virages/` seulement, binaire reconstruit après le commit 4.

1. **Essai** (~5 min) :
   `$B likeness --teams B,C --limit 20 --holdout-share 0.25 --variants base,svc,svc+org,svc+org+turns:0+reuse:0,svc+org+tri:0+w:0+turns:0+reuse:0,svc+org+reuse:0 --label virages-essai --out $G/lot4/4c-virages`.
   Contrôles : `base` `93149ec262198e9e`, `svc-org-turns0-reuse0` `27952d4befd451f4` (I2′), `svc-org-tri0-w000-turns0-reuse0` =
   `svc` (I2). Lecture : refus `SHARP_TURN` et `REUSE_OUTSIDE_TAIL` par triangle complet, requêtes avec un triangle gardé,
   reprises, `turn_cut`, `turn_evals`, P50 × `svc` (indicatif, bruit de 5 à 15 %). **Arrêt** (relevé, pas de campagne 2) si un
   contrôle manque, ou si les refus `SHARP_TURN` par triangle complet ne baissent pas de moitié au moins contre
   `svc-org-turns0-reuse0`.
2. **Déterminisme** (~13 min) : `$B likeness --teams B,C --holdout-share 0.25 --variants base,svc+org --threads 1 --label virages-det1 --out $G/lot4/4c-virages`,
   puis la même avec `--threads 16 --label virages-det16` : mêmes empreintes (D).
3. **Complète** (~70 min, RSS ≤ 21 Gio) :
   `$B likeness --holdout-share 0.25 --variants base,common,svc,common+org,svc+org,svc+org+noretry,svc+org+lc,svc+org+turns:0+reuse:0 --label lot4-4c-virages --out $G/lot4/4c-virages`.
   Contrôles : `base`, `common`, `svc` aux empreintes de I1, `svc-org-turns0-reuse0` à `c48118447315c118` (I2′). Si C1 tombe entre
   1,15 et 1,20, second tirage des temps dans la même campagne :
   `$B likeness --holdout-share 0.25 --variants base,svc,svc+org --label lot4-4c-virages-t2 --out $G/lot4/4c-virages`, moyenne des deux.

Puis, si le §16.11 tient : commit 5, et la séance en conteneur du §11 du relevé (image `osm-route-details:lot4-4c-virages`, jamais
poussée, défauts organiques, mêmes départs et même volume, méthode du §14.13).

### 16.11 Critères

Candidat : `svc-org` aux défauts du §16.6, contre `svc` du même tirage (campagne 3), lecture « toutes », toutes équipes, quatre
groupes, sauf mention. Les seuils sont ceux du §15.12, sauf A4 :

- **A1** : W1 / IQR de r / D ≤ 0,75 × `svc` (ROAD), ≤ 0,85 × (GRAVEL).
- **A2** (inchangé, décision 3 du §9.8) : part des premiers à `vertices_10 = 2` dans [0,40 ; 0,58] (ROAD) et [0,32 ; 0,50] (GRAVEL),
  à chaque curseur ; W1 / IQR de `vertices_10` ≤ 0,5 × `svc`.
- **A3** : compacité ROAD ≤ 0,80 × `svc` (GRAVEL rapporté).
- **A4** (décision 2) : `shared_share` en absolu, médiane ≤ 0,05 et P90 ≤ P90 de `svc` + 0,05, par type et curseur ; `far_pos`,
  `detour_far`, `sinuosity_10`, `turns_per_10km` : W1 / IQR ≤ `svc` + 0,10.
- **B1** : `S / S_base` ≤ `svc` + 0,03 et ≤ 0,5, « toutes » et « écart ». **B2** : A ROAD, `svc-org − common-org` ≤ +0,054 aux deux
  curseurs (réserve levée ≤ +0,03).
- **C1** : P50 ≤ 1,15 × `svc` à chaque curseur, P90 ≤ 1,25 ×. **C2** : `OK` ≥ `svc` − 0,5 point ; aucune `PANIC` ni `WINDOW_ESCAPE` ;
  `VIOLATION` ≤ `svc`. Rapporté avec C2 : les refus `SHARP_TURN` du contrôleur sur les parcours organiques rendus (attendu 0 ; un
  cas relevé est expliqué au relevé).
- **D** : mêmes empreintes à 1 et 16 fils (campagne 2).
- **C3** (après la bascule, en conteneur de 3 Gio) : P50 ≤ 0,56 s à 4 clients, ≤ 0,29 s en série ; pic `anon` ≤ 2,4 Gio ; `oom` 0 ;
  dépôts en 200.
- **Règle L** : rejugée (`svc-org-lc`) ; si la bascule se fait, `learn_figures = organic` sauf si `lc` tient la règle du §15.12.
- Rapportés, non bloquants : part `SHARP_TURN` des refus de triangles (87 à 91 % au 4c), `REUSE_OUTSIDE_TAIL`, requêtes avec un
  triangle gardé, reprises par requête, `turn_cut`, `turn_evals`, `reuse_cut`, `svc-org-noretry` contre `svc-org`.

**Bascule** : A1 à A4, B1, B2, C1, C2 et D tiennent → commit 5, puis C3 ; si C3 manque, `git revert` et le relevé le dit. Sinon le
défaut reste `classic` et le relevé nomme les critères manqués. La fusion revient au propriétaire.

### 16.12 Risques, et points tranchés par prudence

- **Temps** : le modèle coûte dans les zones de chaînes courtes, dans T1 et T2 aussi ; le gain attendu vient de passes et de reprises
  en moins. Non chiffré avant mesure : `turn_evals` et le P50 de l'essai le disent ; la règle d'arrêt de la campagne 1 évite une
  campagne complète inutile.
- **Rendement** : un triangle que le contrôle aurait accepté peut être perdu par un faux refus (2,5 × 10⁻⁴ par paire), par la
  dépendance au chemin des étiquettes, ou par l'interdiction de l'aller-retour (une jambe sans demi-tour possible n'est plus
  trouvée). A2 en GRAVEL était à 0,003 de sa borne.
- **Heuristiques non admissibles** en organique (§16.2) : chemins sûrs, peut-être un peu plus longs.
- Points non couverts par les décisions, tranchés ici par prudence : (1) la décision 2 vise `shared_share` seul, les quatre autres
  mesures d'A4 gardent leur W1 / IQR (un seuil absolu de 0,05 n'a pas de sens pour `turns_per_10km`) ; (2) l'alignement porte sur
  les seuls caps et l'épingle, là où la divergence est mesurée ; (3) l'épingle n'est pas modélisée (la recherche est plus stricte
  que le contrôle) ; (4) l'exemption près de l'arrivée, en avant, est lue sur T2 et peut se tromper (filet du §16.5) ;
  (5) l'instrumentation du §16.0 n'est pas commitée (elle écrit des positions de parcours, privées) ;
  (6) les questions du §16.14 ont leur option recommandée appliquée.

### 16.13 Écarts du codage

Écrits le 19 novembre 2026 par le codeur (commits 1 à 4 du §16.9). Le point technique que les réponses du propriétaire ne
couvraient pas (les détails ci-dessous) est tranché prudemment, du côté qui refuse davantage ou qui garde le `Classic`.

1. **Fenêtre de cap de 25 à 50 m, pas de 30 à 50 m.** Le contrôle prend `near = min(30, far / 2)` : avec 50 m de chemin connu,
   `near` vaut 25 m, et c'est aussi la fenêtre des caps `v.head` de la vue. Le §16.2 (comme le §15) écrit « 30 à 50 m » ;
   le modèle prend 25 m et 50 m, ce qui est ce qui le fait égaler le contrôle aligné à 10⁻⁶° près sur des chaînes droites
   (test `the_model_angle_equals_the_aligned_check_on_straight_short_chains`).
2. **Aucune exemption pour le virage simple.** Le §16.2 dit « la paire `(a, b)` est refusée … sauf si elle est exemptée aux
   bouts », mais le contrôle (`verify.rs`, `check.rs` avec `hairpin_single_node` faux) ne lève ni le virage simple ni l'épingle
   d'un nœud seul aux bouts : seules les grappes `a ≠ b` sont exemptées (200 m du départ pour `b`, de l'arrivée pour `a`). Le
   modèle fait de même : `a = b` n'est jamais exempté. Conséquence : un demi-tour par une ouverture de terre-plein de 20 m
   (deux virages de 90° dont la fenêtre de cap de 25 à 50 m enjambe le raccord) est un virage simple de 180° aux deux
   nœuds, refusé même à 100 m du départ (test 3) ; l'exemption n'a d'effet que sur une grappe dont les virages simples sont
   chacun sous le seuil (raccord de plus de 50 m, test 5 à écart de 70 m).
3. **Longueur d'un segment de chaîne courte : la corde.** Pour une chaîne de moins de 50 m, le chemin approché est le segment
   entre ses deux nœuds ; ses points à `d` mètres sont repérés par la longueur de la **corde** et non par `len_dm / 10`. Sur une
   chaîne droite c'est la longueur du contrôle (le test d'exactitude à 10⁻⁶° en dépend, `len_dm` est arrondi au décimètre) ;
   sur une chaîne courbe le modèle perd en précision, ce que la marge couvre. Les mètres de la grappe (`run`) et des bouts
   libres restent ceux de `len_dm`, comme le contrôle. Les chaînes de 50 m ou plus sont droites sur `len_dm / 10`.
4. **Repère des caps du modèle : au nœud.** Le §16.2 ne le dit pas. Le modèle prend les caps dans un repère centré au nœud
   (celui du contrôle aligné et du contrôleur), non dans le repère `Disc` du départ : sinon l'écart de projection de 0,04° à
   1,34° du §16.0 s'y retrouverait. Il ne se sert donc pas de `Disc`.
5. **À rebours, le modèle est le modèle en avant sur le parcours retourné.** Les paires `(a, b)` sont les mêmes (l'angle de
   virage et la règle des exemptions sont symétriques par retournement) : `refuses_bwd` lit les états qui suivent `f`, les
   retourne et appelle le même `TurnModel::eval`. Pas de seconde implémentation.
6. **Contexte des recherches : `Lead` et `Ctx.org`, pas `ctx_path` et `Ctx.turns`.** `Ctx` gagne un seul champ,
   `org: Option<&OrgSearch>` (le modèle, `no_reuse`, les compteurs de la requête), et les recherches un paramètre
   `Lead { path, dm, aux }` : les états posés avant la racine (après, à rebours), les mètres du parcours à la racine, et
   l'arbre de l'autre bout, qui sert à lire la distance à l'extrémité inconnue (T2 pour une recherche en avant, T1 à rebours ;
   `tree_backward` gagne `t1: Option<&Tree>`). Dans T1, `aux` est absent : aucune exemption à l'arrivée, comme le §16.2.
7. **Voie rapide plus large que le texte.** Quand `len(f) ≥ 50 m` et que `b0` n'est pas un carrefour, la relaxation n'est pas
   évaluée du tout (`B ⊆ {b0}` vide) ; `turn_evals` ne compte que les évaluations qui remontent le chemin.
8. **Jonction de la jambe 3 sur un chemin d'arbre** : `junction_ok` rejoue le modèle sur chaque état du chemin d'arbre dont le
   début est à moins de 160 m du nœud de jonction (1 600 dm, au lieu des « ~150 mètres »), avec les jambes 1 et 2 pour
   contexte ; un refus envoie la jambe à l'A*, comme un réemprunt.
9. **Longueurs des états d'extrémité.** Le premier état du parcours (partiel, morceau d'accroche) et le dernier gardent leur
   `len_dm` entier dans le chemin approché ; seuls les mètres de l'exemption sont exacts (`Tree::meters` compte le morceau
   partiel). L'erreur ne touche que les 50 premiers et derniers mètres, où se joue peu de virages.
10. **Références.** Commit 1 : aucune référence ne change (le contrôle aligné ne diffère pas sur la grille de référence).
    Commit 2 : la référence organique ne change pas non plus (aucun virage que le modèle refuse n'est pris sur cette grille). Commit 3 :
    `no_reuse` change **64 des 140** lignes de `organic_reference.txt` (mêmes issues, parcours différents), re-bénie ; celle
    d'avant l'étape est gardée à côté, `organic_reference_before_turns.txt`, et rejouée avec `turns: false` et
    `no_reuse: false` (test `the_organic_reference_is_replayed_with_turns_and_no_reuse_off`, I2′ en test). Le test I2 (`LENS_ONLY`,
    couloir 0) coupe aussi les deux options.
11. **Tests 8 et 11** : ce sont les tests existants (`the_organic_generation_is_deterministic`,
    `any_window_gives_the_organic_reference`, `every_organic_route_passes_the_independent_checker_and_triangles_are_among_them`),
    qui jouent `OrganicTune::default()` donc les nouveaux défauts ; aucun test dupliqué. Le test 10 est dans
    `crates/ord-loops-check/tests/aligned_turns.rs` (un nœud à 40 km au nord du départ, 150,02° au nœud, 149,8° au départ).
12. **Vitesse du modèle** (commit `perf(loops)`, hors §16.9). À l'essai de fumée, la première version du modèle faisait
    `svc-org` à × 2,35 · × 2,73 de `svc` (P50, B et C) contre × 1,2 · × 1,1 sans le modèle. Le profil la mettait dans `atan2`, `hypot`,
    `sincos` et `fmod` : le test d'angle se fait désormais sur les vecteurs (produit scalaire négatif de carré au moins
    `cos² × |u|² × |v|²`), les vecteurs unitaires des 65 536 caps viennent d'une table (f32, 512 Ko), une fenêtre se lit en une
    remontée, et le virage simple après une chaîne de plus de 100 m se calcule sans remonter le chemin. Mêmes décisions
    (empreintes inchangées) ; × 1,54 · × 1,79 ensuite. Le test d'exactitude à 10⁻⁶° (chaînes courtes) tient toujours ; pour les
    chaînes longues la tolérance est celle du cap stocké (0,02°).
13. **Banc.** `Org` gagne `turns0`, `reuse0` et `margin_deg` ; `record.rs` (désaccord moteur-contrôleur) lit le contrôle aligné
    pour une boucle organique ; `mod.rs` arrête la campagne si I2 (`svc+org+tri:0+w:0+turns:0+reuse:0` = `svc`) ou I2′
    (`svc+org+turns:0+reuse:0` = `c48118447315c118` toutes équipes, `27952d4befd451f4` à `--teams B,C --limit 20`) manque.
    La section « 4c » du résumé affiche `turn_cut`, `turn_evals`, `reuse_cut` par requête et les refus du contrôleur par code.

**Relecture indépendante (19 novembre 2026, commits de `1e732c1` à `3c68940`).** Tenus à la lecture et aux tests : l'invariant
`Classic` (`Ctx.org` vaut `None` hors boucle organique non stricte, contrôle aligné réservé à l'organique avec `turns`, sonde
intouchée) ; les paires `(a, b)`, la grappe (`run`, somme des `len_dm` des chaînes entières entre `a` et `b`, coupée à `> 1 000`
comme `run > cluster_m`), les degrés, le seuil `≥ 150 − marge` (test vectoriel et voie rapide `ceil` exacts au bord) et
l'exemption des seules grappes, celles du contrôleur ; le retournement à rebours ; l'interdiction de l'aller-retour, qui ne
remonte que les ancêtres ; le déterminisme et la fenêtre (le modèle ne lit que la vue, les arbres et le chemin) ;
`ord-loops-check` n'a gagné que le test 10, sous `tests/`. Trois écarts corrigés, du côté qui refuse davantage :

14. **T2 n'a plus d'exemption au départ** (commit `fix(loops)`, `tree_backward` reprend sa signature d'avant l'étape). Le §16.2
    la lisait sur `t1.meters` ; or un chemin de T2 est toujours la **fin** du parcours. Dans une boucle, il repasse près du
    départ à l'approche de l'arrivée, à des kilomètres du départ le long du parcours : l'exemption y laissait passer les grappes
    dont `b` est à moins de 200 m de l'arrivée et `a` au-delà, que le contrôle refuse. C'est le symétrique de T1, qui n'a pas
    d'exemption à l'arrivée. `astar_back` la garde, lue sur T1 : l'aller qu'il cherche est bien le début du parcours.
15. **La jambe 2 n'a plus d'exemption à l'arrivée** (même commit). Aucun de ses nœuds n'est à 200 m de l'arrivée le long du
    parcours, même quand elle passe près du départ ; l'exemption lue sur T2 (point de prudence 4 du §16.12, confirmé par le
    propriétaire) reste celle des recherches qui finissent à l'arrivée, le retour de la lentille et la jambe 3.
16. **Jonction d'un chemin d'arbre rejouée sur 200 m** (commit `fix(loops)`, `JUNCTION_DM`, au lieu de 160 m, point 8). Une
    paire dont la fenêtre d'avant `a` déborde sur les jambes posées a son `a` à moins de 50 m de la jonction et son `b` à moins
    de 150 m ; sa fenêtre d'après n'est lue entière qu'à l'état qui commence moins de 50 m après `b`.
17. **Constaté, non corrigé** : l'A* rouvre un état fixé quand l'heuristique n'est plus cohérente (T2 porte le modèle, réponse 2 du
    §9.8). Un état rouvert garde ses successeurs déjà étiquetés, que le modèle avait jugés sur son ancien chemin : « tout chemin
    rendu respecte le modèle » (§16.2) n'est vrai qu'en l'absence de réouverture. `turn_cost` et `retro_ok` ont la même
    dépendance depuis 1.10. Le contrôle interne aligné reste le filet, et le contrôleur ne refuse rien à l'essai.

Mêmes références (organique comprise, sans re-bénédiction), mêmes empreintes à l'essai de fumée rejoué (relevé, §14.6) :
sur ce tirage, les trois corrections ne changent que des étapes internes des recherches.

**Campagnes (19 novembre 2026, binaire de `4fca0b9`, relevé §15).** Les réponses du propriétaire (fin du §9.8) lèvent l'arrêt
de l'essai ; points tranchés pendant les campagnes, chaque fois du côté prudent :

18. **Une campagne de plus que les trois du §16.10.** La réponse 4 demande, si C1 dépasse × 1,25, que le relevé décompose le
    temps (couloir, jambes, arbres) ; aucune variante de la campagne 3 ne sépare le couloir du modèle. Une campagne courte
    (`base,svc,svc+org,svc+org+tri:0,svc+org+tri:0+turns:0+reuse:0`, 32 min) le fait, et sert de second tirage des temps
    de `svc-org`. Elle ne juge rien d'autre.
19. **Deux marges rapportées dans la campagne 3** (`margin:10`, `margin:15`), pour la question du §14.5 : la marge reste 5°
    (réponse 1), elles ne sont pas jugées. Aucune ne change le temps (P50 × 1,24 à × 1,26 · × 1,41).
20. **La tolérance × 1,25 de la réponse 4 se lit à chaque curseur**, comme C1. Elle tient au curseur 0,25 (× 1,24, second tirage
    × 1,26, moyenne × 1,25) et manque au curseur 1 (× 1,40 et × 1,37, moyenne × 1,38) : pas de bascule. L'écart au seuil dépasse
    nettement le bruit de 1 à 2 % mesuré entre deux tirages de `svc` sur toutes les équipes (§12 du relevé).
21. **Pas de commit 5, pas de séance C3.** La séance en conteneur jouée est **indicative** (relevé, §15.7), comme celle du 4c :
    image de `4fca0b9` construite localement, jamais poussée, défauts organiques posés par variables d'environnement
    (`ORD_GENERATOR__FIGURES=organic`, `ORD_GENERATOR__PROFILE__LEARN_FIGURES=organic`) au lieu d'un commit, pour mesurer ce que
    servirait la bascule sans la faire.
22. **Le `CHANGELOG` de 1.11.0** (non publiée) dit que les recherches organiques suivent la règle des virages et ne font pas
    d'aller-retour : c'est un comportement de l'option, pas un changement de défaut.

### 16.14 Questions au propriétaire

Chaque question a une option recommandée, appliquée par défaut ; une autre réponse change un réglage, pas l'architecture.

1. **Marge du modèle** : 5° (recommandé : 90 % des refus captés, 2,5 × 10⁻⁴ faux refus par paire) ; 0° (82 à 84 %, 2,2 × 10⁻⁵) ;
   10° (93 %, 7 × 10⁻⁴).
2. **Modèle dans les arbres T1 et T2** (recommandé : oui, la jambe 1 porte 38 % des refus), au prix d'heuristiques non garanties
   admissibles en organique ; sinon, chemins d'arbre seulement validés après coup, et des paires perdues.
3. **A4** : la décision 2 lue pour `shared_share` seul (recommandé) ; ou pour les cinq mesures.
4. **Si C1 manque encore après l'étape** : pas de bascule, le relevé décompose le temps (couloir seul : +10 à +12 % au 4c) ;
   le prochain levier serait le couloir (recommandé), avant tout relâchement de seuil.
