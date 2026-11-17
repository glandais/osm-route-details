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
