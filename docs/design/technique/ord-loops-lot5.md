# Générateur de parcours — lot 5 : off-road (une part de chemins tenue, roulabilité, tronçons longs), première conception

Écrit le 23 novembre 2026. **Première conception, rien de codé, rien de mesuré pour ce lot.** Plan d'ensemble :
`docs/plans/2026-11-15-generateur-parcours-osm.md` du dépôt tribly (§2, §6, §7.4, décision 8 du §11, lot 5 du §12, §12 septies
pour le lot 4) ; registre `API-207`. Suite des notes `ord-loops-lot4.md` (goût, formes), `ord-loops-lot6.md` (fenêtres) et
`ord-1-14-planete-mesures.md` (planète, budget de 30 s). Moteur de départ : paquet **1.14.0**, API de service **1.13**
(`crates/ord-server/src/lib.rs:17`) ; Pédalons : API `16.1.1`.

Marques : **[mesuré]** vient d'un relevé déjà écrit dans une note ou dans le plan ; **[calculé]** d'un calcul fait ici sur
ces relevés ; **[estimé]** d'un ordre de grandeur ; **[à mesurer]** n'a aucun chiffre et en demande un avant de coder la suite.
Les équipes sont désignées par leur lettre, comme dans les relevés du lot 4 ; aucune donnée de production n'entre dans le dépôt.

Les choix que cette note prend sans réponse du propriétaire sont marqués **(défaut pris, à contredire)** et repris au §14.

> **Décisions du propriétaire, 23 novembre 2026, après le 5a** (elles priment sur le reste de la note) :
> 1. **Crans : 0,25 · 0,47 · 0,60.** « Beaucoup » est ramené de 0,65 à 0,60 (environ 76 % des requêtes l'atteignent au
>    mieux, contre 46 à 58 % à 0,65) ; au-delà, le parcours le plus proche est rendu avec `reached = false`.
> 2. **Grille élargie** : μ ∈ {0,25 ; 0,35 ; 0,5 ; 0,7 ; 1 ; 1,4 ; 2}, au plus 4 pas par requête, choisis selon la cible
>    (prior lu sur la sonde), sans hypothèse de monotonie.
> 3. **Temps : environ ×3 accepté.** C1 est réécrit : médiane avec cible ≤ ×3,0 de la même requête sans cible, P90 ≤ ×3,5 ;
>    toujours au plus 4 pas et aucun pas lancé au-delà de 10 s de calcul ; C4 (aucun 504 de plus sur la planète) inchangé.
> 4. **Écart de définition (R2)** : accepté pour ce lot ; la couverture de la typologie des tuiles est un chantier à part
>    (registre tribly `API-214`).
> 5. **Défauts du §14 confirmés** : sans réglage, une demande gravel vaut « Comme d'habitude » ; exemption d'équipe avec un
>    parcours du type, sans effet en mode strict ; bornes du §6.1 telles quelles. **R4** (cran par équipe) : repoussé à un
>    lot suivant (changement du format de profil).

> **Révision du 23 novembre 2026, après le C4** (relevé : [`ord-loops-lot5-mesures.md`](ord-loops-lot5-mesures.md), §10.5
> et §10.6 ; elle prime sur le §4.3) : avec une consigne, la réponse n'est jamais pire que sans.
> 1. **Le pas 0 n'est jamais joué sous μ = 1** : il est joué à `max(prior, 1)`. Sous 1, une génération coûte plus que sans
>    cible dans les agglomérations (× 1,7 à × 3,6 à μ ≤ 0,35 sur toutes les requêtes mesurées, × 0,9 à × 2,2 à 0,7, états fixés
>    de l'A* en proportion, §10.5), et le pas 0 est le seul à garder le budget entier du service : c'était le 504 de Tokyo à
>    0,47 (pas 0 à 0,7 : 33,9 s contre 15,1 s à μ = 1). À μ = 1, le pas 0 est la génération sans cible au bit près ; le prior
>    (0,7 pour 0,47, 0,35 pour 0,60) est joué au **pas 1** si la part est trop basse ou s'il n'y a aucune proposition.
>    Au-dessus de 1 (cran 0,25, prior 1,4), le pas 0 reste au prior : × 0,8 à × 1,9 (plus cher sur 2 requêtes sur 5, §10.5),
>    pas de 504 de plus au relevé ; la garantie n'y est donc pas stricte (écart laissé au propriétaire, §10.6).
> 2. **Dernier recours** : si aucun pas n'a de candidat et que μ = 1 n'a pas été joué, la génération sans cible est jouée,
>    dans le budget du service seul (hors `max_steps` et hors délai des pas). `DistanceTooShort`, qui suit μ (plus court chemin
>    d'un aller simple), compte comme un pas sans candidat au lieu d'arrêter la recherche.
> 3. **Arrêt avant un pas k ≥ 1** : `écoulé + 1,5 × durée(plus long pas joué, pas 0 compris) > until` ; la réserve de 0,2 ×
>    pas 0 disparaît, l'estimation n'est plus jamais sous le pas 0. Un pas lancé reste coupé à `until` et jeté.
> 4. **Les 10 s se comptent depuis la première fenêtre** : après un échappement de fenêtre, la seconde recherche ne regagne
>    pas 10 s de pas.

> **Mise à jour du 23 novembre 2026, après le 5a** (relevé : [`ord-loops-lot5-mesures.md`](ord-loops-lot5-mesures.md), §7) :
> les règles du §3.5 sont appliquées. **R1** : crans **0,25 · 0,47 · 0,65** en `offroad` (réel GRAVEL P10 0,264, P50 0,470,
> P90 0,611 ; M2 ne confirme pas les 20 / 38 / 49 %), reportés au §10.1 et au §14. **R2** déclenchée (écart médian 0,083, P90
> 0,154) : cause dans la source (12,6 % de la longueur de l'export sans classe de revêtement), pas dans `SURFACE_OTHER` ; crans
> gardés sur `offroad`. **R3** : 29 % de courbes monotones, la recherche du §4.3 est la **grille** {0,35 ; 0,5 ; 0,7 ; 1 ; 1,4}.
> **R4** déclenchée (B à 0,310). **R5** : bornes du §6.1 inchangées. **R6** : le **5e est abandonné** (§5.2 ; `run1k` et
> `switch_10km` générés déjà plus enchaînés que le réel). **R7** : pas de 5h. Le relevé montre aussi que « Beaucoup » à 0,65
> et C1 ne tiendront pas tels qu'écrits : points du §8 du relevé, laissés au propriétaire, rien n'est relâché ici.

Relue contre le code le 23 novembre (seconde lecture) : la règle `surface_rd` écrite sur la table de Valhalla dès le départ
(§2.2), la marge des heuristiques avec τ (§5.2), l'exemption d'équipe exigeant un parcours du type (§6.2), le réglage du
contrôleur passé par le service (§6.4), `reached` recalculé après le contrôleur (§8.2, §10.1), des critères A, B et C1 resserrés
ou rendus cohérents (§9) et l'envoi de la consigne conditionné aux capacités (§10.2) viennent de cette relecture.

---

## 0. Ce que le lot livre

Une personne qui prépare une sortie gravel dit « peu », « comme d'habitude » ou « beaucoup » de chemins, et le parcours
proposé **tient cette part** à quelques points près, sur des chemins qui roulent, enchaînés en tronçons plutôt que saupoudrés.
Aujourd'hui le moteur ne reçoit aucune consigne de ce genre : le non-revêtu n'entre que par un multiplicateur fixe
(×0,75 en gravel, ×0,6 en VTT, `cost.rs:110-123`) et par le goût appris, et la part obtenue est loin du réel (§1).

| # | Pièce | Où | Sortie mesurable |
|---|---|---|---|
| 5a | **Mesurer d'abord** : la part de non-revêtu du moteur 1.14 contre les vraies boucles de chaque équipe, dans une définition unique (§2) ; sonde de la courbe part(μ) | `tools/ord-loops-bench` (+ crochet `probe` dans `ord-loops`) | Écart de part par équipe et par type ; courbe part(μ) par requête ; longueur des tronçons ; longueur réelle sur des arêtes que les bornes du §6 écarteraient |
| 5b | **Une définition, mesurée partout** : `Attrs::surface_rd()` (règle route-details), mesure dans le vérificateur du moteur, dans `ord-loops-check` et dans la réponse | `ord-graph`, `ord-loops`, `ord-loops-check`, `ord-server` | Accord avec la classe `sc` de l'export sur les boucles réelles |
| 5c | **Bornes de roulabilité** par type, dans le moteur et dans le contrôleur | `ord-loops`, `ord-loops-check` | 0 `NOT_RIDEABLE` ; longueur réelle écartée |
| 5d | **La consigne de part** : champ d'API, multiplicateur μ du non-revêtu cherché pas à pas dans le budget, terme de choix, rendu « atteint ou non » | `ord-loops`, `ord-server` | Part obtenue contre part visée (§9) |
| 5e | ~~**Coût de passage** revêtu ↔ non revêtu, quand une consigne est donnée~~ **abandonné par R6 au 5a** | `ord-loops` | Part de non-revêtu en tronçons de plus d'1 km |
| 5f | **Service** api 1.14, paquet 1.15.0, séance en conteneur | `ord-server`, image | Critères C du §9 |
| 5g | **Pédalons** : contrat, trois crans dans le panneau web | dépôt tribly | Tests de ressource et du panneau ; e2e |

Ordre : 5a d'abord, **parce qu'il fixe les valeurs des trois crans et la forme de la recherche** (§3.5). 5b et 5c sont
indépendants et peuvent courir en parallèle après 5a ; 5d s'appuie sur 5b ; 5e sur 5d ; 5f et 5g ferment le lot.

**Ce que le lot ne change pas** : le format du graphe et des vues (aucune reconstruction, `graph_id` inchangé, aucune
surcouche `stale`) ; le format du profil d'équipe et de la table commune (aucun redépôt) ; le mode strict (ni recherche de μ,
ni bornes, ni coût de passage : §4.1 et §6.3) ; les types ROAD
(aucune consigne, aucune borne nouvelle : empreintes du banc au bit près) ; le parcours rendu quand aucune consigne n'est
donnée **et** que les bornes sont coupées (`rideability = off`) : 1.14 au bit près.

**Hors lot** :

- **La pente non revêtue** (plan §6.5) : le graphe n'a pas d'altitude (`ascent_m` toujours `null`, `routes.rs:168`) ; il
  faudrait écrire une pente par arête dans les bits réservés (40 à 63, `attrs.rs:212`), donc reconstruire le graphe et les
  vues, changer `graph_id` et faire redéposer toutes les équipes, pour un effet que le banc ne peut pas lire (équipes de
  plaine, comme les cols au 4d). Repoussée à un lot « altitude » qui servirait aussi le D+ (§14, défaut 9).
- **Les chaînes off-road comme attracteurs du squelette** (plan §6.4, seconde moitié) : repoussées sous condition (§5.3).
- **La roulabilité « suspecte »** et toute confirmation entre équipes (plan §7.4) : décision 5 du plan (pas d'agrégat entre
  équipes) ; seule la confirmation par l'équipe elle-même entre dans le lot, comme exemption des bornes (§7).
- **« Comme d'habitude » propre à chaque équipe** (la médiane de ses propres boucles gravel) : mesurée au 5a, décidée après
  (§3.5, règle R4).
- **Une consigne en ROAD**, et l'écran générateur du mobile (il n'existe pas ; seul le client généré bouge).

---

## 1. Ce que le moteur fait déjà, et ce qui lui manque

Relu dans le code de 1.14.0 le 23 novembre (état des lieux vérifié aux lignes citées).

- **Attributs** (`crates/ord-graph/src/attrs.rs:192-277`) : `class`, `surface` (30 valeurs plus « autre »), `tracktype`,
  `smoothness` (8 valeurs, `:175`), `mtb_scale` (0 à 6, `:424-433`), `access`, `bicycle_tag`, `service`, `routes` (dont
  `ROUTE_MTB`). **`smoothness` et `mtb_scale` ne sont lus nulle part** hors des tests d'`attrs` ; `tracktype` seulement pour le
  `track` revêtu (grade1) et le « rugueux » (≥ 4).
- **Vue gravel** (`crates/ord-graph/src/view.rs:64-118`) : accès `No` et `Private` et `service=*` écartés ; classes routières,
  `track`, `path`, `bridleway`, `footway` / `pedestrian` désignés gardés. **Aucun filtre de roulabilité.** Le VTT roule sur la
  même vue (`cost.rs:20-26`).
- **Coût** (`cost.rs`) : `w = ⌈len × K / 64 × q8[classe de goût] / 256⌉` (`:258-262`). En gravel, `off_road(a)` (`:78-81` :
  `is_unpaved` ou classe `track` / `path` / `bridleway`) vaut ×0,75, une grande route ×1,25 ; en VTT ×0,6 et ×1,4.
  **Multiplicateur fixe**, aucun coût de passage. Les poids des chaînes de vue sont précalculés au montage de la fenêtre
  (`window.rs:431-466`, `w_osm`, au goût de base) ou, quand l'équipe a son propre goût, par requête dans le vecteur au goût
  de `WindowTeam` (`team.rs:374`, `search.rs:220`) ; ils sont lus par `Ctx::chain` (`search.rs:250-270`), qui y ajoute la
  familiarité et le λ hors équipe (`lambda_q8`, `search.rs:244`), et rend `w` tel quel sans surcouche. **Le λ du plan §6.3 n'est donc pas libre : le code appelle déjà λ le coût hors équipe.
  Cette note nomme μ le multiplicateur du non-revêtu.**
- **Goût** : 72 classes = 6 voies × 3 revêtements × balisé × destination (`cost.rs:133-178`), revêtement 0 connu revêtu, 1
  `surface` absent, 2 connu non revêtu (`taste_surface`, `:162-170`). Le goût corrige la part de chaque classe vers le réel
  de l'équipe ; il **ne vise pas une part demandée**.
- **Mesure** : `unpaved_m` vient du vérificateur interne (`verify.rs:410-417`) et ne compte que le **tag** `surface`
  (`Attrs::is_unpaved`, `attrs.rs:279-295`) ; `presumed_surface_m` compte voie verte et `track` sans surface
  (`cost.rs:46-48`). `ord-loops-check` ne mesure aucun revêtement (`Report`, `check.rs:137-153`) et n'a aucune règle de
  roulabilité (`rules.rs:59-125` : admission et rugueux d'équipe seulement).
- **API** : `GenerateRequestV1` (`crates/ord-server/src/generator/routes.rs:74-95`) n'a aucune consigne de revêtement ; la
  réponse rend `unpaved_m`, `unpaved_share`, `presumed_surface_m` (`:158-160`).
- **Choix des propositions** : `scored_route` (`generate.rs:117-145`) note écart de distance, recouvrement, croisements, cap
  et part d'équipe ; rien sur le revêtement.

**Ce que disent les relevés** (GRAVEL, 780 boucles réelles, `ord-loops-lot4-mesures.md` §GRAVEL classes) **[mesuré]** :

| mesure | réel P10 · P50 · P90 | moteur 1.9.2, curseur 0,25 · 1 (médiane) |
|---|---|---|
| `surf_unpaved` (tag seul) | 0,119 · **0,253** · 0,376 | 0,099 · 0,104 |
| `surf_presumed` | 0,182 · 0,326 · 0,493 | 0,329 · 0,361 |
| `way_path` (`track` + `path` + …) | 0,214 · **0,447** · 0,592 | 0,166 · 0,205 |

Le goût du 3b relève le non-revêtu tagué généré à 0,157–0,175 selon les lectures (`ord-loops-lot4-sonde.md`) **[mesuré]** :
**moins des deux tiers du réel**, alors que le présumé est déjà au niveau du réel. Le moteur prend des chemins, mais pas les
bons, et pas assez de chemins connus non revêtus.

---

## 2. Une seule définition du non-revêtu (5b)

### 2.1 Le problème

Les crans de la décision 8 (≈ 20 %, 38 %, ≥ 50 %) viennent du §2 du plan, calculé par `shapes.py` sur `sc == "unpaved"` :
la classe de route-details (`crates/ord-core/src/classification/tables.rs:233-260`), qui lit le **tag `surface`, puis
`tracktype` (grade1 revêtu, grade2 à 5 non revêtu), puis la classe seule** (`INFERRED_UNPAVED` = `track`, `path`, `footway`,
`pedestrian`, `bridleway`, `sidewalk`, `crossing`, `:88-89`). Le moteur et le banc ne comptent que le tag : le réel y est à
25 %, pas à 38 %. Viser « 38 % » dans la définition du moteur serait trompeur ; le viser dans celle de route-details sans la
mesurer dans le moteur serait impossible.

### 2.2 La règle retenue

**La part visée, la part rendue, le banc et le contrôleur comptent le non-revêtu selon la règle de route-details**, recopiée
sur les attributs du graphe (défaut pris, à contredire) :

```rust
/// ord-graph, attrs.rs : revêtement « au sens route-details » (ord-core, classification::surface).
pub enum SurfaceRd { Paved, Unpaved, Unknown }
impl Attrs {
    pub fn surface_rd(self) -> SurfaceRd {
        // 1. tag : classe Valhalla du code (table RAW_SURFACE d'ord-core recopiée code par code, pas is_unpaved) ;
        //    0 (absent), SURFACE_OTHER, ou code absent de la table de Valhalla (salt, ice) → règle suivante.
        // 2. tracktype : 1 → Paved ; 2..=5 → Unpaved ; 0 ou 7 → règle suivante.
        // 3. classe : track, path, footway, pedestrian, bridleway → Unpaved ;
        //    classes routières, cycleway, steps → Paved ; sinon Unknown.
    }
    pub fn offroad(self) -> bool { self.surface_rd() == SurfaceRd::Unpaved }
}
```

Raisons :

1. **Pédalons affiche déjà cette définition** : la page d'un parcours enregistré montre la répartition de route-details. Un
   parcours proposé à « 38 % de chemins » doit montrer 38 % une fois enregistré.
2. Les crans du §2 sont exprimés dans cette définition ; ils gardent leur sens.
3. `Unknown` compte comme revêtu dans la part (comme route-details : la part est non revêtu / longueur totale).

**Ce que la règle ne touche pas** : `Attrs::is_unpaved` (tag seul) reste tel quel, parce qu'il entre dans `taste_surface`,
donc dans la classe de goût, donc dans `Common::hash` : le changer rendrait incompatibles la table commune et tous les profils.
Le `off_road` du coût (`cost.rs:78-81`) reste aussi tel quel (1.14 au bit près sans consigne). La réponse garde
`unpaved_share` (tag seul) et ajoute `offroad_share` (§8).

**Écart connu avec route-details, corrigé dès l'écriture** : route-details passe la valeur brute de `surface` par la table
de Valhalla (`RAW_SURFACE`, `tables.rs:104-144` : sous-chaîne, première entrée qui correspond ; revêtu = `paved_smooth`,
`paved`, `paved_rough`), alors qu'`is_unpaved` suit sa propre liste (`attrs.rs:279-295`). Relu le 23 novembre sur les 30 codes,
ils divergent sur **quatre** valeurs : `unhewn_cobblestone` (non revêtu pour `is_unpaved`, `paved_rough` donc revêtu pour
Valhalla, par la sous-chaîne `cobblestone`), `wood` (revêtu pour `is_unpaved`, `compacted` donc non revêtu pour Valhalla),
`salt` et `ice` (non revêtus pour `is_unpaved`, absents de la table de Valhalla : règle suivante, `tracktype` puis classe).
`paving_stones` n'en fait pas partie (revêtu des deux côtés). **`surface_rd` s'écrit donc directement sur la table de Valhalla,
code par code** (une table de 32 entrées : 0, les 30 codes, `SURFACE_OTHER`), et non sur `is_unpaved`. Reste un écart que le
graphe ne peut pas combler : `SURFACE_OTHER` (31) a perdu la valeur brute, que Valhalla aurait pu reconnaître par sous-chaîne
(`gravel;grass`, `asphalt;compacted`…) ; `surface_rd` la passe à la règle suivante. Ce résidu, et l'écart de source (tuiles
contre PBF), sont ce que mesure M2 (§3.2).

**Test d'accord** : un test lit les 32 codes `surface` × les 7 `tracktype` (0 à 5 et 7) × les classes et compare `surface_rd`
à `ord_core::classification::surface` sur les mêmes attributs (valeur brute reconstruite depuis `SURFACES` ; pour
`SURFACE_OTHER`, une valeur que Valhalla ne reconnaît pas). `ord-core` ne dépend que d'`ord-domain`, `rstar`, `rustc-hash`,
`serde` et `thiserror` (relu dans `crates/ord-core/Cargo.toml`) : il entre en `dev-dependency` d'`ord-graph` sans cycle, et le
test vit dans `ord-graph`.

---

## 3. 5a — Mesurer d'abord

Aucun changement du moteur servi. Deux pièces au banc : des mesures de plus dans `likeness` (3.1 à 3.3), puis une sonde
derrière la fonctionnalité `probe` (3.4).

### 3.1 Mesures ajoutées à `Measures` (`tools/ord-loops-bench/src/likeness/measure.rs`, `out.rs`)

Sur la suite d'arêtes (appariée pour le réel, rendue pour le généré), pondérées par la longueur :

| Mesure | Définition |
|---|---|
| `offroad` | part de la longueur où `surface_rd() == Unpaved` (§2.2) |
| `offroad_sc` | **réel seulement** : part de la longueur des `network_edges` dont `sc == "unpaved"` dans l'export (la définition de `shapes.py`) ; `export.rs` lit aujourd'hui `trv` et `wk`, pas `sc` : à ajouter |
| `run_p50_m` | médiane, pondérée par la longueur, des tronçons non revêtus : suites maximales d'arêtes `offroad` consécutives, un trou revêtu de moins de 50 m n'interrompt pas un tronçon |
| `run1k` | part de la longueur `offroad` prise dans des tronçons d'au moins 1 km (la « chaîne » du plan §6.4) |
| `switch_10km` | passages revêtu ↔ non revêtu par 10 km |
| `banned` | part de la longueur sur des arêtes que les bornes du §6 écarteraient, **équipe non exemptée** et **exemptée** (deux colonnes) |
| `smooth_known`, `mtb_known` | part de la longueur dont `smoothness` / `mtb:scale` est renseigné (couverture du tag) |

`offroad` est **rapporté à part** et n'entre pas dans `S_GRAVEL` (`compare.rs:38-49`, la liste que S moyenne ;
`compare.rs:318` est `NAMED`, les mesures nommées du tableau) : l'ajouter changerait S pour toutes les variantes et rendrait les
seuils B du lot 4 incomparables. Il s'ajoute à `NAMED` pour GRAVEL. ROAD le rapporte sans critère (réel 0 % médian, 5 % au P90
**[mesuré, §2 du plan]**).

### 3.2 Ce que le relevé 5a rapporte

- **M1, par type et par équipe** (lettres ; équipes de moins de 30 boucles du type regroupées en « autres ») : réel P10 · P25 ·
  P50 · P75 · P90 de `offroad`, et la même chose pour le premier parcours généré, aux curseurs 0,25 et 1, variante du service
  `svc+org`. Écart médian, W1 / IQR.
- **M2, accord des définitions** : sur les boucles réelles, `offroad` contre `offroad_sc`, médiane de l'écart par boucle et
  P90 de l'écart absolu ; et les déciles de `offroad_sc` face à ceux de `shapes.py` (38 % médian, §2 du plan).
- **M3, tronçons** : `run_p50_m`, `run1k`, `switch_10km`, réel contre généré.
- **M4, bornes** : `banned` sur le réel (deux colonnes) et sur le généré ; couverture `smooth_known`, `mtb_known` de la vue
  gravel dans les fenêtres jouées.
- **M5, ROAD** : `offroad` réel et généré (contrôle que ROAD n'a rien à corriger).

### 3.3 Commande

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`, graphe et vues par défaut du banc (France,
`$G/lot6/france-20261115-z11.*`), export et lettres par défaut (`~/code/perso/tribly-generator-bench/data/`) :

```
cargo build --release -p ord-loops-bench
# essai court (déterminisme du relevé, quelques minutes)
$B likeness --teams B,C --limit 20 --holdout-share 0.25 --variants base,svc+org --label 5a-smoke --out $G/lot5/5a
# relevé complet, toutes équipes, deux curseurs, ROAD et GRAVEL (les MTB sont trop peu : rapportés sans critère)
$B likeness --holdout-share 0.25 --variants base,svc+org --label 5a --out $G/lot5/5a
```

Garde-fou : à mesures ajoutées près, la variante `svc+org` doit redonner l'**empreinte de résultat** de la campagne finale
du 4c (`ord-loops-lot4c-mesures.md` §16) au bit près ; les nouvelles mesures ne changent aucun parcours. Sorties hors dépôt
sous `$G/lot5/5a/` ; le relevé écrit dans `docs/design/technique/ord-loops-lot5-mesures.md` ne porte que des agrégats et des
lettres.

### 3.4 Sonde part(μ) (fonctionnalité `probe`)

Le crochet : dans `Ctx::chain` et `Ctx::edge` (`search.rs:250-290`), sous `#[cfg(feature = "probe")]`, le poids d'une chaîne
devient `w × (256 + (μ_q8 − 256) × f_c / 256) / 256`, où `f_c` (u8, en 256es) est la part de la longueur de la chaîne `c` sur
des arêtes `offroad`, calculée au montage de la fenêtre à côté de `w_osm` (§4.4). Variante du banc `mu:<μ>`
(`likeness/variants.rs`), composable : `svc+org+mu:0.5`. Un drapeau `--ride-types gravel` (nouveau) évite de rejouer ROAD.
`f_c` est une part de **longueur**, pas de poids : le non-revêtu d'une chaîne y pèse déjà moins par mètre (× 0,75 en gravel, plus
le goût), si bien que la part de longueur dépasse la part de poids et que μ agit un peu plus fort que s'il ne pesait que sur
la part non revêtue du poids. Approximation voulue (l'exact demanderait
un tableau de poids non revêtus par goût, donc par équipe et par requête) ; la sonde mesure l'effet réel, et le prior l'absorbe.

```
$B likeness --holdout-share 0.25 --ride-types gravel --label 5a-mu --out $G/lot5/5a-mu \
  --variants base,svc+org,svc+org+mu:0.25,svc+org+mu:0.35,svc+org+mu:0.5,svc+org+mu:0.7,svc+org+mu:1.4,svc+org+mu:2
```

`mu:1` redonne `svc+org` au bit près (contrôle du crochet). Rapporté, par requête puis agrégé :

- la courbe part(μ) du premier parcours, et de la meilleure des trois propositions au sens de |part − cible| ;
- sa **monotonie** (part des requêtes où la part décroît quand μ croît, à 0,02 près) ;
- pour chaque cible t ∈ {0,10 ; 0,15 ; … ; 0,60}, le μ médian qui l'atteint et la part des requêtes qui l'atteignent à ±0,05
  quelque part sur la grille (borne haute de ce que la recherche peut faire) ;
- le temps par pas (P50, P90) contre `svc+org`.

### 3.5 Ce que le 5a décide (règles écrites avant le relevé)

- **R1, valeurs des crans** : « Peu » = P10 réel de `offroad` en GRAVEL arrondi à 0,05 ; « Comme d'habitude » = P50 réel
  arrondi à 0,01 ; « Beaucoup » = P90 réel + 0,05, arrondi à 0,05. Si M2 confirme les 20 / 38 / 49 % du §2 à ±0,03, ce sont
  **0,20, 0,38 et 0,55** (défaut pris, à contredire). Les valeurs vivent dans le backend de Pédalons (§10), pas dans le moteur.
- **R2, accord des définitions** : `surface_rd` suit déjà la table de Valhalla code par code (§2.2) ; l'écart qui reste vient
  de `SURFACE_OTHER` et de la source (tuiles contre PBF). Si l'écart médian `offroad − offroad_sc` dépasse 0,03 ou son P90 absolu
  0,08, la cause est cherchée valeur par valeur au 5a (quelles valeurs brutes derrière `SURFACE_OTHER`) avant le 5d, et le
  relevé dit l'écart qu'une personne verra entre la part proposée et la part affichée une fois le parcours enregistré ; les crans
  restent calés sur `offroad` (R1), la définition que le moteur sait viser.
- **R3, forme de la recherche** : si au moins 90 % des courbes part(μ) sont monotones, la recherche du §4.3 est une sécante
  bornée ; sinon une grille courte (μ dans {0,35 ; 0,5 ; 0,7 ; 1 ; 1,4}) dont on garde le meilleur, sans hypothèse d'ordre.
- **R4, « Comme d'habitude » par équipe** : si la médiane réelle d'au moins une équipe de plus de 30 boucles gravel s'écarte de
  plus de 0,08 de la médiane générale, la question est posée au propriétaire pour un lot suivant (valeur apprise au dépôt, dans
  le profil, donc un changement de format) ; sinon elle est close.
- **R5, bornes** : si `banned` sans exemption dépasse 1 % de la longueur réelle à la médiane, ou 5 % au P90, la borne fautive
  (celle qui pèse le plus) est relâchée d'un cran avant le 5c ; avec exemption d'équipe, le réel doit tomber sous 0,2 % médian.
- **R6, coût de passage** : si `run1k` généré (à `mu` donnant la médiane réelle) est à moins de 0,10 sous le réel et
  `switch_10km` à moins de 1,5 × le réel, le 5e est **abandonné** (rien à corriger) ; sinon il est fait.
- **R7, attracteurs de chaînes** : voir §5.3.

---

## 4. 5d — La consigne de part

### 4.1 Champ d'API (service, api 1.13 → **1.14**, paquet 1.14.0 → **1.15.0**, ajout seulement)

```rust
/// GenerateRequestV1 (routes.rs) :
/// Share of the length on unpaved ways the routes should aim at, route-details definition (surface tag, then tracktype,
/// then way class), within capabilities.generator.offroad.target_min..target_max, rounded to 0.01.
/// Gravel and mtb only. Absent or null: no target (1.14 behaviour).
#[serde(default)]
pub offroad_target: Option<f64>,
```

- **Bornes** : `[0, 0,8]`, arrondi au centième. Au-delà, ou non fini : 400 `invalid_request`. 0 est permis (« le moins possible »).
- **Type** : `road` avec une cible → 400 `invalid_request` (le champ n'a pas de sens sur la vue route ; défaut pris, à
  contredire : plus clair qu'un champ ignoré). `gravel` et `mtb` l'acceptent.
- **Défaut** : absent = aucune cible = comportement de 1.14 (à la borne de roulabilité près, §6). Le service n'invente pas de
  cible : c'est le backend qui envoie la valeur du cran.
- **Compatibilité** : champ facultatif, `#[serde(default)]` ; un client 1.13 ne l'envoie pas et reçoit la même chose qu'avant.
  Le service refuse les clés inconnues (`strict_json.rs`) : **le service 1.15 se déploie avant le backend qui envoie le champ**,
  et le backend n'envoie le champ que si les capacités annoncent `generator.offroad` (§10.2), ce qui protège aussi un retour
  arrière du moteur.
- **Mode strict** (curseur 0) : la cible est acceptée et sans effet : aucune recherche de μ (un seul pas, μ = 1, τ = 0), pour que
  le mode strict reste celui de 1.14 ; la réponse dit `reached` selon la part obtenue, `steps: 1`.

### 4.2 Le multiplicateur μ

Le poids de toute chaîne et de toute arête partielle devient, **après** goût, familiarité et λ hors équipe :

```
w′ = ⌈w × (256 + (μ_q8 − 256) × f / 256) / 256⌉,   f = part offroad de la longueur, en 256es (u8)
```

- μ < 1 rend le non-revêtu moins cher (plus de chemins), μ > 1 plus cher. μ_q8 entier (256 = neutre) : même poids en natif et
  en WASM. μ ∈ `[1/4, 4]`, cherché sur x = log₂ μ par pas de 1/8 (grille entière, déterministe).
- μ s'applique **aussi aux arêtes d'équipe** : sans cela, en « équipe d'abord » sur un réseau d'équipe dense, la consigne
  n'aurait presque aucune prise. Le réseau d'équipe reste préféré par sa familiarité ; μ ne fait que déplacer l'équilibre revêtu
  / non revêtu à l'intérieur et autour.
- Il ne touche ni le coût des virages ni le couloir. Il s'applique dans **toutes** les branches de `Ctx::chain` et de
  `Ctx::edge` (y compris le `return w` sans surcouche, `search.rs:259`), et pas en amont dans `w_osm`, que la fenêtre en cache
  partage entre requêtes.
- **Heuristiques** : rien à multiplier par μ. L'A* classique prend pour heuristique l'arbre retour T2, calculé avec les mêmes
  poids `Ctx` (donc avec μ) à chaque pas ; l'heuristique point à point de l'organique (`P2pTarget::h`, `search.rs:1050`) prend
  `f_min` lu sur les poids réels des chaînes fixées par T1 (`t1_min_factor`, `search.rs:1005`, §15.19 de la note du lot 4), qui
  suit μ de lui-même. Le test 6 du lot 4 (`tests_organic.rs:128`, `chain(c) ≥ f_min · len_dm(c)`) est étendu à μ ∈ {1/4, 1, 4}.

### 4.3 La recherche de μ, pas à pas dans le budget

Chaque pas est une génération complète à μ donné (arbres, jambes, contrôle interne), **sans** le contrôleur indépendant, qui
ne passe qu'une fois, sur les parcours finalement rendus.

1. **Pas 0** (révisé après le C4, encadré du haut : joué à `max(prior, 1)`, le prior au pas 1) : μ₀ = `prior(type, cible)`, table linéaire par morceaux apprise au 5a (μ médian qui atteint chaque cible,
   §3.4), figée dans le moteur comme la table commune (`offroad_prior.rs`, constantes générées par le banc, provenance sans
   donnée privée). Le prior est général, pas par équipe (défaut pris, à contredire).
2. **Mesure** : part `offroad` de la première proposition après le choix (§4.5). Dans `[cible − tol, cible + tol]` → arrêt.
3. **Pas suivants** : sans encadrement, x recule ou avance d'un pas de 1 (μ ÷ 2 ou × 2) dans le bon sens ; dès qu'un
   encadrement existe, sécante sur les deux points qui l'encadrent, arrondie à la grille de 1/8, puis bissection si la sécante
   retombe sur un point déjà joué (règle R3 : grille fixe si les courbes ne sont pas monotones). **Le 5a a tranché pour la
   grille** (29 % de courbes monotones) : μ pris dans {0,35 ; 0,5 ; 0,7 ; 1 ; 1,4}, pas 0 au point de grille du prior, le
   meilleur des candidats mis en commun gardé, sans sécante ni bissection.
4. **Arrêt** au premier pas dans la tolérance, ou si l'une de ces conditions tombe avant de lancer le pas k + 1 :
   - k + 1 > `offroad.max_steps` (**4** par défaut) ;
   - `écoulé + 1,5 × durée(pas k) + réserve_contrôle > min(échéance − 1 s, début + offroad.steps_until_ms)` (révisé après le
     C4 : `1,5 × durée(plus long pas joué)`, sans réserve, le début étant celui de la première fenêtre), avec
     `offroad.steps_until_ms` = **10 000** et `réserve_contrôle` = 0,2 × durée(pas 0) ;
   - x sort de `[−2, 2]` (μ hors de `[1/4, 4]`) ;
   - le stop du moteur (client parti, échéance).

   Raison des 10 s : dans les zones les plus denses de la planète une seule génération dure jusqu'à 25,7 s
   (`ord-1-14-planete-mesures.md` §3) **[mesuré]** ; la consigne ne doit jamais transformer une génération rendue en 504. Là,
   le moteur ne joue que le pas 0 et le dit (`steps: 1`). En France une génération dure 0,68 à 1,01 s en médiane (chaud,
   froid) **[mesuré]** : quatre pas tiennent largement.
5. **Ce que rend la recherche** : les candidats contrôlés de tous les pas joués sont **mis en commun**, puis le choix ordinaire
   (note du §4.5, terme de part compris, règle de diversité) les classe une seule fois : un candidat du pas 0 peut gagner au pas
   2. Égalité de note : le candidat du pas le plus ancien. La mesure de l'étape 2 se fait sur la première proposition de ce choix
   commun, recalculé après chaque pas.
6. **Un pas coupé ne coûte rien** : chaque pas k ≥ 1 reçoit son propre `stop`, levé à `min(échéance − 1 s, début +
   offroad.steps_until_ms)` ou par le stop du service. Un pas coupé est jeté (ses candidats partiels aussi) et la recherche rend
   ce qu'elle avait : la consigne ne transforme jamais en 504 une génération dont le pas 0 a abouti. Seul le pas 0 garde le stop
   du service tel quel (1.14).

**Ce qui est réutilisé d'un pas à l'autre** : la fenêtre montée (tuiles, index, composantes) et son tableau `f` (§4.4) ; les
données d'équipe de la fenêtre (`TypeTeam`, `ChainTeam`) et le goût ; le fichier d'attracteurs ; l'accroche du départ et de
l'arrivée ; le tirage de la figure par la graine (`tri_first`, proportions) ; la liste des candidats déjà contrôlés. **Ce qui
est refait** : les arbres T1 / T2, les recherches de jambes, les points lointains (ils dépendent des distances), les budgets
`astar_budget` (remis à neuf à chaque pas, pour que le pas k + 1 ne soit pas affamé par le pas k).

**Tolérance** : `offroad.tolerance` = **0,05** (cinq points) par défaut, publiée dans les capacités. C'est l'écart que les
crans peuvent se permettre (0,20 · 0,38 · 0,55 restent disjoints à ±0,05) et l'écart interquartile réel est de 0,15
**[calculé, §1]** : viser plus fin n'aurait pas de sens pour une personne.

**Cible inatteignable** (un secteur sans chemins, ou que des chemins) : le moteur rend les parcours les plus proches et le
dit : `offroad.reached = false`, la part obtenue dans chaque proposition. Il ne refuse pas : un parcours à 22 % quand on en
demande 38 reste une proposition utile, et c'est à l'interface de le dire (§10.3).

### 4.4 Le tableau `f` au montage de fenêtre

`window.rs:431-466` somme déjà les poids des arêtes de base de chaque chaîne pour `w_osm`. Dans la même passe, pour la vue
gravel seulement : `off_dm` = décimètres `offroad` de la chaîne, `f_c = ⌊256 × off_dm / len_dm⌋` saturé à 255 (u8). Un octet
par chaîne de la vue gravel **[calculé : `w_osm` y prend 8 octets par chaîne (deux types, gravel et VTT,
`rides_of_view`, `engine.rs:541`) ; l'octet de plus est de l'ordre de 2 % de ce que pèsent par chaîne `w_osm`, `chain_off`,
`gid`, `head`, sans compter les liens]**, compté dans `bytes()` (`engine.rs:192-212`). Les deux bits de bout (`head_off`,
`tail_off` : la première et la dernière arête de base de la chaîne sont-elles `offroad`) servent au 5e et se rangent dans le
même octet si l'on prend `f` sur 6 bits (précision de 1/64, suffisante) ; les deux bits de roulabilité du §6.3 (gravel, VTT)
prennent alors un second octet : à trancher par l'agent, test de précision à l'appui.

### 4.5 Le choix des propositions

Quand une cible est donnée, `scored_route` (`generate.rs:117-145`) ajoute un terme `offroad.score_weight × |part − cible|`,
`score_weight` = **20** (cinq points d'écart valent 1, comme 45° d'écart de cap ; défaut pris, réglé au banc dans {10, 20,
40}). La règle de diversité (recouvrement < 0,5) et la part d'équipe minimale ne changent pas. La part mesurée pour la note est
celle du vérificateur interne (`verify.rs`, qui compte `offroad_m` à côté d'`unpaved_m`, §8.2).

### 4.6 Avec le goût appris

Le goût porte déjà, par équipe, un goût du non-revêtu (les trois revêtements de `taste_surface` dans ses 72 classes). Les deux
ne se contredisent pas, ils se composent :

- le **goût** répartit la longueur entre les classes à revêtement donné (quelles petites routes, quels chemins balisés) et
  rapproche les parts de classes de celles de l'équipe ;
- **μ** déplace l'équilibre global revêtu / non revêtu jusqu'à la cible demandée.

Le μ trouvé absorbe ce que le goût fait déjà : pour une équipe qui aime les chemins, « Comme d'habitude » tombera sur un μ
proche de 1. **Le goût reste appris sans consigne** (μ = 1) au dépôt : le profil ne change ni de format ni de hachage, aucune
surcouche ne devient `stale` (défaut pris, à contredire). La table `prior` est apprise au banc **sur la table commune** ; une
équipe au goût très marqué coûtera un pas de plus, ce que le relevé mesure (pas par équipe).

### 4.7 Interface du moteur

`Request` n'a pas de `Default` et se construit en littéral à une douzaine d'endroits (tests, banc, service, `ord-graph-match`,
exemple), dont la retouche (`tools/ord-loops-bench/src/retouche/mod.rs:253` et `resize.rs:163` ; `control.rs:45` passe par
`..proto.clone()`). **On ne lui ajoute pas de champ** : la consigne passe par une entrée nouvelle,

```rust
pub struct Steer { pub offroad: Option<OffroadTarget> }      // Steer::NONE : 1.14
pub struct OffroadTarget { pub target_milli: u16, pub tolerance_milli: u16, pub max_steps: u8, pub steps_until_ms: u32 }
pub fn generate_steered_in<B: AsRef<[u8]>>(e: &Engine<B>, win: &Window, team: Option<TeamCtx<'_>>, r: &Request,
    steer: &Steer, ws: &mut Workspace, stop: &mut dyn FnMut() -> bool)
    -> (Result<SteeredGeneration, Failure>, Diagnostics, FigureDiagnostics);   // diagnostics du pas rendu
pub struct SteeredGeneration { pub generation: Generation, pub offroad: Option<OffroadOutcome> }
pub struct OffroadOutcome { pub reached: bool, pub steps: u8, pub mu_q8: u32 }   // reached : au sens du vérificateur interne
```

(mêmes arguments et même forme de retour que `generate_with_figures_in`, `generate.rs:245`, plus `steer` ; `Generation` et
`Diagnostics` ne gagnent aucun champ, parce que les références figées en hachent le `Debug` complet), et les entrées existantes (`generate_in`,
`generate_with_figures_in`…) délèguent avec `Steer::NONE`. `OffroadOutcome::reached` est un diagnostic du moteur : la réponse
du service le recalcule sur ce que le contrôleur a gardé (§8.2). La retouche ne change pas
d'une ligne. Les bornes de roulabilité (§6) et le coût de passage (§5) sont des réglages de `Params` (qui a un `Default`).

---

## 5. Les chaînes off-road (plan §6.4)

### 5.1 Ce que le plan demandait

Des suites continues d'arêtes non revêtues roulables de plus d'un kilomètre, servant d'attracteurs au squelette, et un coût
à chaque passage revêtu ↔ non revêtu, pour que le parcours enchaîne les chemins au lieu d'alterner tous les 200 m.

### 5.2 Le coût de passage : dans le lot (5e), sous la règle R6

**Abandonné le 23 novembre 2026 par R6** (relevé du 5a, §4 et §7) : au μ qui donne la médiane réelle, `run1k` généré vaut
0,75 contre 0,65 réel et `switch_10km` 11,2 contre 14,5. Le texte ci-dessous reste pour mémoire.

- **Où** : au passage d'une chaîne à la suivante, là où le moteur paie déjà le virage (`search.rs`, règle de virage, `in_head`
  et `TURN_COST_MAX`). Si `tail_off(chaîne entrante) ≠ head_off(chaîne sortante)` : + τ. À l'intérieur d'une chaîne les passages
  sont fixés par le graphe et ne se choisissent pas.
- **Valeur** : τ = **150 m équivalents** (1 500 décimètres pondérés) en gravel, 100 en VTT (défauts pris, réglés au banc dans
  {0, 75, 150, 300}).
- **Seulement avec une cible** : sans consigne, τ = 0 et 1.14 au bit près.
- **Heuristiques** : un coût ajouté positif garde la borne « vol d'oiseau × `f_min` », mais **pas** les deux bornes
  triangulaires de `P2pTarget::h`, qui retranchent `P2P_SLACK = TURN_COST_MAX + 500` (`search.rs:1044`) parce que le coût d'une
  transition sépare deux états du même nœud. Avec τ > 0 la marge devient `TURN_COST_MAX + 500 + τ` (champ `slack`, déjà
  porté par `P2pTarget`) ; τ s'ajoute **hors** du plafond `.min(TURN_COST_MAX + 500)` du coût de virage (`search.rs:394`). Sans
  cible, marge et plafond restent ceux de 1.14. Le modèle de virages en organique (lot 4, §16) n'est pas touché autrement.
- **Précalcul** : les bits `head_off` / `tail_off` au montage de la fenêtre (§4.4). **Aucun précalcul hors ligne** : une
  chaîne off-road « au sens du plan » n'a pas à exister comme objet pour que le coût de passage fasse son effet, et un fichier
  de plus à côté du graphe se paie en mise en service (`OPS-61`, `OPS-65`).

### 5.3 Les chaînes comme attracteurs : repoussées

Raisons :

1. Le 4d a montré que le mécanisme d'attracteurs **dépasse** le réel dès que la lentille joue, sans réglage qui tienne tous
   les critères (`ord-loops-lot4d-mesures.md`, §9.11 de la note du lot 4) **[mesuré]** ; le réutiliser pour des chaînes
   reprendrait ce risque sans le traiter.
2. μ et τ agissent sur toute la recherche, pas seulement sur le point lointain ; la sonde du 5a dit s'ils suffisent.
3. Les attracteurs de chemins sont des données publiques (OSM) mais le choix de leur poids se règle sur la reconstruction,
   et le banc gravel est petit (780 boucles, une équipe dominante).

**R7, condition de reprise** : après le 5e, si « Beaucoup » manque O1 (§9) **et** que `run1k` généré reste à plus de 0,15
sous le réel, un sous-lot 5h « chaînes attractives » est proposé au propriétaire, avec un précalcul au montage de fenêtre
(composantes de chaînes `offroad` de plus d'1 km, ancrées à leur milieu, poids = longueur).

---

## 6. 5c — Bornes dures de roulabilité

### 6.1 Les règles, par type

Une arête est **non roulable** pour un type si l'une des conditions de sa ligne tient. Les valeurs sont celles d'OSM ;
attention aux codes du graphe : `Attrs::mtb_scale()` vaut 0 si absent, sinon échelle + 1 (« `mtb:scale` ≥ 2 » est donc le code
≥ 3, « ≥ 4 » le code ≥ 5) ; `Attrs::smoothness()` vaut la position dans `SMOOTHNESS` + 1 (`horrible` 6, `very_horrible` 7,
`impassable` 8) et 15 pour une valeur inconnue, qui reste permise ; `tracktype` `grade5` est le code 5 (7 = valeur inconnue,
permise). Les tests du §11 prennent chaque bord (code limite et code voisin).

| Type | Interdit toujours | Interdit sauf arête d'équipe (§6.2) |
|---|---|---|
| ROAD | — (la vue route a ses règles, inchangées) | — |
| GRAVEL | `smoothness=impassable` | `smoothness` ∈ {`horrible`, `very_horrible`} ; `mtb:scale` ≥ 2 ; `tracktype=grade5` ; `surface=mud` |
| MTB | `smoothness=impassable` | `mtb:scale` ≥ 4 |

`very_bad`, `bad`, `grade4`, `sand`, `grass` restent permis en gravel (ce sont des chemins qu'un vélo gravel roule, mal) : le
coût les rend chers, la borne ne les interdit pas (défauts pris, à contredire ; R5 du 5a les remet en cause s'il le faut).
Les accès (`no`, `private`, `service=*`) sont déjà écartés par la vue et le contrôleur.

**Pente non revêtue** : hors lot (§0).

### 6.2 L'exemption d'équipe

Une arête **roulée par l'équipe avec un parcours du type** échappe à la colonne de droite : l'équipe l'a roulée, c'est la
confirmation par les traces du plan §7.4, au seul niveau 1 (l'équipe elle-même). Attention : la règle « d'équipe » existante
(`team.rs:685-697`, `rules.rs:149-174`) admet un parcours de **n'importe quel** type, sauf pour les arêtes « présumées »
(`path`, `footway`, surface rugueuse) ; une arête `mtb:scale=3` en `surface=gravel` prise par un seul parcours ROAD (erreur
d'appariement) y serait d'équipe. L'exemption exige donc, comme les présumées, un parcours GRAVEL ou MIXED (VTT : MTB, GRAVEL ou
MIXED) sur l'arête. C'est un prédicat **à part**, `exempt(ride, attrs, counts)`, écrit dans le moteur et réécrit dans le
contrôleur : `team_edge` et les drapeaux `TEAM` de `TypeTeam` ne changent pas (sinon les parcours de 1.14 changeraient sans
consigne). Sans surcouche (découverte sans équipe), aucune exemption.

### 6.3 Dans le moteur

- Réglage `Params::rideability: Rideability { On, Off }`, défaut `On`. `Off` redonne 1.14 au bit près (pour les tests figés
  et pour rejouer un relevé ancien, la retouche par exemple).
- **Pas dans la vue** : la vue reste celle de 1.14 (aucune reconstruction, aucun changement de `graph_id`, aucun redépôt). Au
  montage de la fenêtre, une chaîne qui contient une arête non roulable reçoit un bit `unrideable` par type (gravel, VTT ; nom à
  garder distinct du `BAN` de `search.rs`, l'interdiction par passe de la lentille) ; `Ctx::chain` et `Ctx::edge` rendent `INF`
  pour elle, sauf exemption d'équipe. L'exemption se calcule arête par arête (la chaîne est interdite si une arête non roulable
  et non exemptée y figure), une fois par fenêtre et par équipe, à côté des agrégats de `WindowTeam`, sur la seule liste des
  chaînes marquées : rien par requête.
- **Mode strict** : bornes non appliquées. Le strict ne prend que des arêtes d'équipe et reste celui de 1.14 (§0) ; le contrôleur
  ne lève pas `NOT_RIDEABLE` en strict (défaut pris, à contredire).
- Les index de composantes (calculés sur la vue sans bornes) peuvent surestimer une composante : au pire un `no_candidate` de
  plus, jamais un parcours faux. Rapporté au 5c.
- **Le profil d'équipe** est appris par le moteur : avec les bornes, un profil appris en 1.15 différerait de très peu d'un
  profil appris en 1.14. On ne change pas son hachage (défaut pris, à contredire) : les profils 1.14 restent servis, la passe
  de nuit les réapprend au fil des redépôts ordinaires.

### 6.4 Dans `ord-loops-check`

Le contrôleur reste écrit sans lire le moteur : la règle est **réécrite depuis le §6.1 de cette note**, dans
`crates/ord-loops-check/src/rules.rs` (`pub fn rideable(ride: RideType, a: Attrs, team: bool) -> bool`), avec sa propre lecture
de `smoothness`, `mtb_scale`, `tracktype`, `surface` (codes d'`ord-graph`, pas de fonction d'`ord-loops`). Violation nouvelle
`NotRideable` (`NOT_RIDEABLE`) dans `check.rs`, rangée après `NotAdmitted`. Elle est active quand le contrôle reçoit
`rideability = on` : un champ de `Config` (`check.rs:70`), défaut `on`. **Le service construit aujourd'hui son contrôleur avec
`Config::default()`** (`ord-server/src/generator/generate.rs:395-401`) : il doit lui passer son propre réglage `rideability`, sinon un
service à `off` verrait le contrôleur jeter tout parcours passé par une arête bornée. Le banc fait de même pour rejouer les
relevés anciens.

Le `Report` gagne `offroad_m` (§8.2) : la part rendue par le service est celle **du contrôleur**, le moteur ne fait que viser.
`offroad_m` se compte par `Attrs::surface_rd` d'`ord-graph`, fonction partagée avec le moteur : ce n'est pas une règle que le
contrôleur arbitre mais une mesure, et son garde-fou indépendant est le test d'accord avec `ord-core` (§2.2).

---

## 7. Le §7.4 du plan (roulabilité confirmée par les traces) : en partie

- **Dans le lot** : la confirmation par l'équipe elle-même, sous la forme de l'exemption du §6.2. Elle existe déjà en germe
  (règle du « rugueux » d'équipe, `team.rs:328-352`) ; le lot l'étend aux nouvelles bornes. Rien de nouveau à calculer la
  nuit : la surcouche porte déjà les arêtes prises par type.
- **Hors lot** : un chemin sans tag « confirmé roulable » parce que d'**autres** équipes l'ont pris, et le chemin « suspect »
  que tout parcours gravel contourne. Les deux demandent un agrégat entre équipes (décision 5 du plan) et le seuil *k* du §7
  (niveau 2). Le « suspect » à l'intérieur d'une seule équipe est possible mais rare sur le banc (une équipe fait 85 % des
  kilomètres) : sans mesure qui le justifie, il n'entre pas.

---

## 8. Service (api 1.14, paquet 1.15.0, ajouts seulement)

### 8.1 Requête

`offroad_target` (§4.1). Rien d'autre.

### 8.2 Réponse

- Par parcours : **`offroad_m`** et **`offroad_share`** (= `offroad_m / length_m`, au millième), mesurés par le contrôleur
  indépendant selon §2.2, pour tous les types (ROAD compris). `unpaved_m`, `unpaved_share` et `presumed_surface_m` restent,
  inchangés (tag seul), documentés comme tels.
- Au niveau de la réponse : **`offroad`**, `null` sans cible, sinon `{ "target": 0.38, "tolerance": 0.05, "reached": true,
  "steps": 2 }`. `reached` se lit sur la **première proposition rendue**, avec l'`offroad_share` du contrôleur : le contrôleur
  peut jeter la première proposition du moteur (`dropped`), et le `reached` du moteur (§4.7) n'est alors plus le bon. Les
  autres propositions portent leur part.
- `mu` n'est pas rendu (diagnostic interne ; il est dans les journaux à la décimale, défaut pris).

### 8.3 Capacités

`generator.offroad` : `{ "ride_types": ["gravel", "mtb"], "target_min": 0.0, "target_max": 0.8, "tolerance": 0.05,
"max_steps": 4, "rideability": "on" }`. Réglages de `GeneratorConfig` (`config.rs`) : `offroad_tolerance`, `offroad_max_steps`,
`offroad_steps_until_ms`, `rideability` (`on` | `off`), validés au démarrage.

### 8.4 Journaux et métriques

Histogramme `ord_generator_offroad_steps` (étiquette `ride_type`), compteur `ord_generator_offroad_total` (étiquettes
`ride_type`, `target_band` = cible arrondie au dixième, `reached`). Jamais une coordonnée, jamais un identifiant d'équipe en clair
(le `ns8` habituel seulement).

---

## 9. Critères d'acceptation

Jugés au banc sur les requêtes du 5a (une par boucle réelle, `svc+org`, mise à l'écart 25 %, deux curseurs), GRAVEL, aux
trois valeurs de cran fixées par R1, sauf mention contraire. Référence de temps et de forme : **1.14 (`svc+org`) dans la même
séance**. Défauts pris, à contredire ; ils sont écrits avant toute mesure, comme au lot 4, et ne se relâchent qu'avec le
propriétaire.

**O, la part**

- **O1** : la première proposition est à ±0,05 de la cible dans au moins **80 %** des requêtes au cran « Comme d'habitude »,
  et **70 %** aux crans « Peu » et « Beaucoup », à chaque curseur.
- **O2** : médiane de (part − cible) dans `[−0,02 ; +0,02]` à chaque cran et chaque curseur.
- **O3** : aucune équipe de plus de 30 boucles gravel sous **60 %** d'O1 au cran « Comme d'habitude ».
- **O4** : `reached` est vrai si et seulement si la première proposition **rendue** (après le contrôleur) est dans la
  tolérance : **100 %** des réponses, y compris celles où le contrôleur a jeté un parcours (test de service avec le crochet
  `corrupt_routes`).
- **O5** : les requêtes où `reached = false` sont rapportées avec la part de non-revêtu disponible dans le disque D / 2 ;
  aucun seuil, mais la note dit combien viennent d'un secteur sans chemins.

**E, les tronçons** (si le 5e est fait, R6)

- **E1** : au cran « Comme d'habitude », `run1k` médian généré à 0,10 au plus sous le réel, et pas sous 1.14.
- **E2** : `switch_10km` médian généré ≤ 1,5 × le réel.

**K, le contrôleur**

- **K1** : **0** refus du contrôleur indépendant (`VIOLATION`) sur tous les parcours rendus, nouvelle règle `NOT_RIDEABLE`
  comprise, ROAD et GRAVEL, avec et sans cible.
- **K2** : `banned` avec exemption sur le réel sous 0,2 % médian (R5) ; `no_candidate` de plus dû aux bornes ≤ 1 % des
  requêtes GRAVEL.

**A et B, formes et goût non dégradés** (seuils du lot 4, §16.11 et §9.9 de sa note, rapportés à 1.14)

Ce sont des critères de **non-régression** contre 1.14, aux trois crans (pas seulement « Comme d'habitude » : « Beaucoup » est
celui qui tord le plus les formes), aux deux curseurs, lectures « toutes » et « écart » :

- **A1** : W1 / IQR de r / D et de la compacité ≤ celle de 1.14 + 0,05.
- **A2** : part des premiers parcours à `vertices_10 = 2` dans [0,32 ; 0,50] (la bande GRAVEL du lot 4, §16.11) ; part des
  triangles parmi les premiers parcours ≥ celle de 1.14 − 0,05.
- **A3** : `shared_share` en absolu, médiane ≤ 0,05 et P90 ≤ P90 de 1.14 + 0,05 (décision 2 du §9.8 du lot 4) ; `far_pos`,
  `detour_far`, `sinuosity_10`, `turns_per_10km` : W1 / IQR ≤ 1.14 + 0,10.
- **B1** : `S / S_base` calculé sur les classes GRAVEL **hors revêtement** (`way_major`, `way_tertiary`, `way_minor`,
  `way_street`, `way_cycle`, `marked`) ≤ 1.14 + 0,03, la marge du B1 du lot 4.
- **B2** : au cran « Comme d'habitude », l'écart absolu entre médiane générée et médiane réelle de `way_path` et de
  `surf_unpaved` est au plus **la moitié** de celui de 1.14. (Pas de W1 / IQR sur `offroad` : la consigne concentre la part
  générée autour d'une seule cible, alors que le réel s'étale sur un IQR de 0,15 ; le W1 entre les deux a un plancher de l'ordre
  de l'écart absolu moyen du réel à sa médiane, et ne dit pas si la consigne est tenue. O1 et O2 le disent.)

**C, le temps et la mémoire**

- **C1** (banc) : avec cible, P50 ≤ **× 1,50** et P90 ≤ **× 2,20** de 1.14 à chaque curseur ; au moins **50 %** des requêtes
  réglées au pas 0 (nombre de pas médian = 1 : un pas médian de 2 doublerait le P50 et ne tiendrait pas × 1,50), P90 du nombre
  de pas ≤ 3. Sans cible, P50 ≤ × 1,03 et P90 ≤ × 1,05 (le coût des bornes seul).
- **C2** (déterminisme) : même graine, même réponse, pas compris ; fenêtre plus grande, même réponse (lot 6, §5.4).
- **C3** (conteneur de 3 Gio, défauts du service) : en série, P50 ≤ **0,50 s** avec cible (0,30 s en 1.12, §16 du relevé du
  4c) ; à 4 clients, P50 ≤ 1,00 s ; pic `anon` ≤ 1,6 Gio ; 0 `oom` ; dépôts en 200.
- **C4** (planète) : la série « zones denses » de `ord-1-14-planete-mesures.md` §3 (60–150 km et 200 km), toutes ses requêtes
  jouées en gravel avec cible 0,38 à 30 s de budget, contre les mêmes requêtes en gravel sans cible : **aucun 504 de plus**,
  aucun 200 devenu 422, aucune requête avec cible plus longue que max(la même sans cible, 10 s) + 1 s (le `stop` des pas
  k ≥ 1, §4.3) ; rapportés : la part des recherches arrêtées par
  `steps_until_ms`, et la part `reached` (sans seuil : le banc français ne dit rien des villes denses).

**I, invariants**

- **I1** : ROAD, toutes variantes : empreintes de 1.14 au bit près.
- **I2** : GRAVEL sans cible et `rideability = off` : empreintes de 1.14 au bit près.
- **I3** : `mu:1` de la sonde = `svc+org` au bit près.

**Bascule** : comme au lot 4 (§9.7, décision 4) : la consigne n'a pas de défaut à basculer (c'est le client qui l'envoie) ;
ce qui bascule est `rideability = on`, dans un commit à part, si K1, K2 et I1 tiennent, suivi de la séance en conteneur ; `git
revert` si C3 manque. La fusion et la mise en service restent au propriétaire.

---

## 10. Côté Pédalons (après le moteur, dépôt tribly)

### 10.1 Contrat

- `RouteGeneratorRequest.unpaved` : énumération **`RouteGeneratorUnpaved`** `LOW | USUAL | HIGH`, facultative. Absente =
  `USUAL` pour GRAVEL et MTB ; **ignorée pour ROAD** (le backend n'envoie rien au moteur, défaut pris : le site ne la montre pas
  en route, un client qui l'enverrait n'a pas à échouer).
- Les **valeurs numériques restent dans le backend** (`RouteGeneratorService`, à côté de `DEFAULT_DISCOVERY`) : `LOW` **0,25**,
  `USUAL` **0,47**, `HIGH` **0,65**, fixées par R1 au 5a (d'abord 0,20 · 0,38 · 0,55). Une énumération au contrat plutôt qu'un nombre : les valeurs se règlent sans
  toucher aux clients.
- `GeneratedRouteDto.unpavedShare` : lit désormais `offroad_share` (définition route-details, la même que la page d'un parcours
  enregistré), et `unpaved_share` si le moteur ne rend pas `offroad_share` (moteur 1.14 après un retour arrière). Description du
  contrat mise à jour.
- `GeneratedRouteDto.unpavedTarget` (nombre, nullable : `null` en ROAD ou sans consigne envoyée) et `unpavedTargetReached`
  (booléen, nullable) : de quoi dire « moins de chemins que demandé ». **Calculé par le backend pour chaque parcours**,
  `|offroadShare − target| ≤ offroad.tolerance`, et non recopié du `reached` du moteur : le contrôle `GeneratedRouteSanity` du
  backend peut jeter la première proposition, et le `reached` du moteur ne vaut que pour sa première.
- `pedalons.api.version` 16.1.1 → **16.2.0** (ajouts) ; skill `contract-first-api` ; clients web et mobile régénérés (le mobile
  n'a pas d'écran générateur : rien d'autre).

### 10.2 Backend

- DTO `backend/src/main/java/fr/pedalons/dto/routes/request/RouteGeneratorRequest.java` (`:41-46`) : champ `unpaved`.
- `RouteGeneratorService.java` (`:162-171`) : traduction cran → `offroadTarget`, seulement si le type est GRAVEL ou MTB **et que
  les capacités du moteur annoncent `generator.offroad`** (déjà lues et mises en cache par `RouteGeneratorGateway`,
  `:159-165`) avec ce type dans `ride_types` ; sinon rien n'est envoyé et `unpavedTarget` reste `null`.
- `infrastructure/routedetails/RouteGeneratorV1.java` (`:25-32`) : `GenerateRequest.offroadTarget` (`Double`, `NON_NULL`) ;
  réponse : `offroadM`, `offroadShare`, objet `offroad`.
- `toDto` (`:183`) et `GeneratedRouteSanity.java` (`:103`) : `unpavedShare` depuis `offroadShare`, dans [0, 1].
- Tests (à lancer par le propriétaire, la règle du dépôt interdit de les lancer soi-même) : traduction des trois crans et de
  l'absence ; ROAD n'envoie rien ; réponse lue ; ressource 200 avec et sans `unpaved`.
- Ordre de déploiement : moteur 1.15 en service d'abord. Le moteur 1.14 refuserait la clé `offroad_target` (`strict_json.rs`) ;
  la lecture des capacités rend l'ordre sûr dans les deux sens : un backend 16.2.0 devant un moteur 1.14 (retour arrière du
  moteur) n'envoie pas la clé et continue de générer, sans consigne.

### 10.3 Site

- `frontend/src/lib/routeGenerator.ts` : `GeneratorParams.unpaved?` et `routeGeneratorRequest` (`:15-23`, `:109-120`) ; rien
  envoyé en ROAD.
- `frontend/src/hooks/filters/routeGeneratorFilters.ts` : `unpaved: z.enum(RouteGeneratorUnpaved).optional().catch(undefined)`,
  alias d'URL `ch` ; absent = `USUAL`, qui ne s'écrit pas dans l'URL.
- `RouteGeneratorPanel.tsx` (`:365-388`) : sous le sélecteur de type, un `SegmentedControl` « Chemins » à trois crans,
  **« Peu » · « Comme d'habitude » · « Beaucoup »**, caché en ROAD. `RouteGeneratorModal.tsx` (`:25`) : défaut absent.
- `GeneratedRouteCard.tsx` (`:42-43`, `:92-94`) : la part s'affiche comme aujourd'hui ; si `unpavedTargetReached === false`,
  une ligne discrète, épicène : « Moins de chemins que demandé dans ce secteur » ou « Plus de chemins que demandé dans ce
  secteur » selon le signe de l'écart.
- Tests : `RouteGeneratorPanel.test.tsx` (contrôle caché en ROAD, valeur envoyée en GRAVEL, URL lue et écrite) ;
  `GeneratedRouteCard` (ligne « moins de chemins ») ; e2e du générateur : un essai en gravel au cran « Beaucoup ».
- Registre : une entrée `API` et une entrée `WEB` créées par `node scripts/ledger.mjs new`, puis `index` ; `API-207` mis à
  jour ; §12 du plan : une section « Lot 5 » avec ce qui est mesuré.

---

## 11. Tests (synthétiques, aucune donnée de production)

- **`surface_rd`** : table des 30 codes × `tracktype` × classes contre `ord_core::classification::surface` (§2.2).
- **μ** : sur une grille à deux couloirs parallèles (l'un revêtu, l'autre en `track` `gravel`) de même longueur, la part
  rendue croît quand μ baisse ; μ = 256 rend 1.14 au bit près ; poids entiers identiques en natif et en WASM (test des
  bornes de l'arithmétique, comme `taste_weight_scales_and_never_vanishes`).
- **Recherche** : cible atteignable → `reached`, pas ≤ 4 ; cible inatteignable (grille sans chemins, cible 0,5) → `reached =
  false`, la proposition la plus proche, aucun refus ; `steps_until_ms` = 0 → un seul pas ; `stop` levé entre deux pas →
  `Stopped`.
- **Mise en commun** : un candidat du pas 0 plus proche de la cible que tous ceux du pas 1 est rendu.
- **Coût de passage** : sur une grille où un chemin continu de 2 km et trois morceaux de 600 m donnent la même part, τ > 0
  prend le chemin continu ; τ = 0 sans cible rend 1.14.
- **Bornes** : une arête `smoothness=horrible` au milieu du seul passage → contournée en gravel, prise en VTT ; prise en gravel
  si un parcours GRAVEL ou MIXED de l'équipe l'a roulée, **pas** si seul un parcours ROAD l'a prise ; `impassable` jamais, même
  d'équipe ; chaque bord de code (`mtb_scale` 2 et 3, 4 et 5 ; `smoothness` 5, 6 et 15 ; `tracktype` 4, 5 et 7) ; mode strict
  inchangé ; `rideability = off` → 1.14.
- **Contrôleur** : `NOT_RIDEABLE` levé sur un parcours fabriqué qui passe par une arête interdite ; non levé si elle est
  d'équipe ; propriété moteur / contrôleur (`engine_vs_checker.rs`) étendue : tout parcours rendu avec bornes et cible passe.
- **Service** : 400 sur `offroad_target` hors bornes, non fini, ou en `road` ; `offroad: null` sans cible ; capacités ;
  réponse 1.13 inchangée pour un client qui n'envoie pas le champ (champs ajoutés seulement) ; `reached` recalculé quand le
  contrôleur jette la première proposition (crochet `corrupt_routes`) ; service à `rideability = off` : le contrôleur reçoit
  `off` et ne jette rien de plus qu'en 1.14.
- **Heuristiques** : test 6 du lot 4 (`chain(c) ≥ f_min · len_dm(c)`) à μ ∈ {1/4, 1, 4} ; avec τ > 0, la borne triangulaire de
  `P2pTarget::h` ne dépasse jamais le coût exact sur une grille fabriquée.
- **Retouche** : `cargo test -p ord-loops-bench` passe sans modification des fichiers `retouche/` (§4.7).

---

## 12. Risques

- **OSM renseigne mal `smoothness` et `mtb:scale`** : les bornes peuvent n'avoir presque rien à écarter (M4 le dit). C'est
  acceptable : elles coûtent peu et protègent là où le tag existe.
- **La part n'est pas monotone en μ** : un petit changement de μ peut basculer le point lointain et la figure. R3 prévoit la
  grille ; la mise en commun des candidats amortit.
- **Le temps** : chaque pas est une génération entière. La table `prior` doit mettre la plupart des requêtes dans la tolérance
  au premier pas, sinon C1 manque ; c'est la première chose à regarder au relevé.
- **Une équipe dominante** (85 % des kilomètres) : le prior général est surtout le sien. O3 regarde les autres à part.
- **Définition** : `surface_rd` suit Valhalla et diverge donc d'`is_unpaved` sur quatre valeurs (§2.2) en plus de la règle
  `tracktype` puis classe ; c'est voulu, et documenté dans la réponse (`unpaved_*` tag seul, liste du graphe ; `offroad_*`
  route-details). Le résidu `SURFACE_OTHER` reste un écart entre la part proposée et la part affichée après enregistrement (R2).
- **Chemins mal tagués** (plan §13) : un `track` sans `surface` compte comme non revêtu et peut être un chemin disparu. Le
  curseur, l'exemption d'équipe et l'avertissement de l'éditeur restent les réponses ; la consigne « Beaucoup » y pousse un peu
  plus : à relire sur carte (sortie du lot, §13 étape 9).

---

## 13. Étapes, dans l'ordre, pour des agents

Chaque étape est un commit (ou deux) qui compile, passe `cargo test --workspace` hors tests lents, `cargo clippy` sur Rust
stable et `./format.sh` côté tribly ; arrêt et relecture marqués **[arrêt]**.

1. **Banc 5a, mesures** — `tools/ord-loops-bench/src/likeness/{export,measure,out,compare}.rs` : lecture de `sc` dans
   `network_edges`, mesures du §3.1 (`offroad` par `Attrs::surface_rd` : la première moitié de l'étape 3, `SurfaceRd` et son
   test d'accord, se fait **avant** l'étape 1, pour que le 5a mesure dans la définition finale), drapeau `--ride-types`. Tests : mesures sur des suites fabriquées (tronçons, trou de
   50 m, part). Garde-fou : empreinte `svc+org` inchangée.
2. **Sonde μ** — `crates/ord-loops/src/{window,search,engine}.rs` sous `feature = "probe"` (tableau `f`, crochet `mu_q8`),
   `likeness/variants.rs` (`mu:<μ>`). Tests : `mu:1` bit pour bit ; croissance de la part sur la grille à deux couloirs.
   Campagnes du §3.3 et §3.4 ; relevé `ord-loops-lot5-mesures.md` (agrégats, lettres). **[arrêt]** : R1 à R7 appliquées,
   les écarts de cette note écrits en tête du relevé.
3. **5b, définition** — `crates/ord-graph/src/attrs.rs` (`SurfaceRd`, `surface_rd`, `offroad`) ; test d'accord avec ord-core
   (§2.2) ; `crates/ord-loops/src/verify.rs` et `lib.rs` (`Route::offroad_m`) ; `crates/ord-loops-check/src/check.rs`
   (`Report::offroad_m`). Aucun parcours ne change.
4. **5c, bornes** — `crates/ord-loops/src/{engine,window,search}.rs` (`Params::rideability`, bit `ban`, exemption d'équipe) ;
   `crates/ord-loops-check/src/{rules,check}.rs` (`rideable`, `NOT_RIDEABLE`, **écrit depuis le §6.1 seul**, par un agent qui
   ne lit pas le code du moteur de l'étape) ; tests du §11 ; `engine_vs_checker.rs`. Références figées : `rideability = off`
   les garde ; en `on`, re-bénies en le disant, avec le nombre de parcours changés.
5. **5d, consigne dans le moteur** — `crates/ord-loops/src/offroad.rs` (nouveau : `Steer`, `OffroadTarget`, recherche du
   §4.3, mise en commun), `offroad_prior.rs` (constantes générées par le banc), `generate.rs` (terme de note §4.5,
   `generate_steered_in`), `search.rs` (μ hors `probe`, heuristique pour μ < 1). Tests du §11 (μ, recherche, mise en commun).
6. **5e, coût de passage** (si R6 le garde) — `window.rs` (bits `head_off`, `tail_off`), `search.rs` (τ au passage de chaîne,
   seulement avec cible) ; test de la grille continu / morceaux.
7. **Banc de la consigne** — `likeness` : variantes `target:<t>` (passe par `generate_steered_in`) et `tau:<m>`, sorties O1 à
   O5, E1, E2, nombre de pas ; campagnes aux trois crans, deux curseurs, et réglages de `score_weight` et τ. **[arrêt]** :
   critères du §9 ; relevé complété.
   *Code livré (23 novembre, sans campagne)* : `--variants svc+org+target:0.47` (cibles dans ]0, 1[, ne se compose pas avec
   `mu`) et `svc+org+ride:on` ; `likeness/{steer,compare_steer}.rs`, section 11 du `-compare.md` : O1 à O5, E1, E2, pas, C1
   réécrit (rapport des quantiles de `gen_ms`, le temps des appels au moteur seuls, contre la même variante sans cible). ROAD
   est joué sans consigne ; `ride:on` pose les bornes sur le moteur donc sur toute l'exécution (`base` comprise) et coupe les
   contrôles d'empreinte de référence ; pour comparer avec et sans bornes, deux exécutions. `score_weight` et τ ne se règlent
   pas encore au banc (constante du moteur ; le 5e est abandonné).
8. **5f, service** — `crates/ord-server/src/generator/{routes,generate}.rs` (`offroad_target`, validation, `offroad` et
   `offroad_*` dans la réponse, depuis le `Report` du contrôleur, `reached` sur la première proposition rendue ; réglage
   `rideability` passé au `Config` du contrôleur), `capabilities.rs`, `config.rs`, `lib.rs` (`API_VERSION =
   "1.14"`), `Cargo.toml` (1.15.0), OpenAPI du service ; tests de route ; séance en conteneur de 3 Gio (C3) et série planète
   (C4). Bascule `rideability = on` dans un commit à part (§9).
   *Code livré (23 novembre, sans séance en conteneur ni série planète)* : `offroad_target` (400 en route et hors [0 ; 0,8]),
   `offroad_m` / `offroad_share` par parcours, `offroad` en réponse (`reached` recalculé sur la première proposition que le
   contrôleur garde, crochet de test `corrupt_first_route`), `generator.offroad` dans les capacités, réglages
   `offroad_tolerance` / `offroad_max_steps` / `offroad_steps_until_ms` / `rideability` (passé au moteur et au contrôleur),
   métriques du §8.4, API 1.14, paquet 1.15.0, contrat régénéré. `rideability` est `off` dans ce commit ; la bascule (`on` par
   défaut, moteur et service) est le commit suivant : références figées re-bénies, 0 parcours changé (le graphe synthétique
   n'a aucune arête bornée).
9. **Relecture sur carte** — avec `tools/ord-loops-viewer`, vingt parcours GRAVEL au cran « Beaucoup » et vingt au cran
   « Comme d'habitude », tirés au hasard sur deux équipes et deux zones hors banc : chemins qui existent, enchaînement, pas de
   chemin interdit. Notes dans le relevé, sans coordonnées. **[arrêt]** : fusion par le propriétaire, mise en service
   (`OPS`, image 1.15.0).
   *Fait (23 novembre)* : relevé §11, dans le banc (boucles mises à l'écart de A et B) et par attributs d'arêtes et images
   statiques plutôt que par la visionneuse ; aucun chemin interdit ni aller-retour fabriqué, points de bornes et de définition
   pour le propriétaire.
10. **5g, Pédalons** (dépôt tribly, après la mise en service du moteur) — contrat et backend (§10.1, §10.2), skill
    `contract-first-api`, site (§10.3), traductions, tests, e2e ; registre et plan (§10.3, dernier point).

`surface_rd` et son test d'accord passent avant l'étape 1 ; le reste de l'étape 3 et l'étape 4 peuvent courir en parallèle
après l'étape 2 ; l'étape 5 attend la 3. L'étape 4 change des parcours (bornes) : les campagnes de l'étape 2 sont faites avant
qu'elle soit fusionnée, et l'étape 7 se joue bornes comprises.

---

## 14. Défauts pris, à contredire

1. **Définition** : la part visée et rendue suit la règle de route-details (tag, puis `tracktype`, puis classe), recopiée en
   `Attrs::surface_rd` ; `is_unpaved` (tag seul) et `unpaved_share` restent pour la compatibilité et le goût.
2. **Nom** : μ pour le multiplicateur du non-revêtu (λ est déjà le coût hors équipe dans le code) ; champ `offroad_target`.
3. **Crans** : 0,25 · 0,47 · 0,65 (R1 au 5a ; d'abord 0,20 · 0,38 · 0,55), recalés par R1 sur le réel mesuré dans la définition retenue ; valeurs dans le backend de
   Pédalons, pas dans le moteur ; mêmes valeurs en VTT (13 parcours : rien à apprendre).
4. **Tolérance** ±0,05 ; au plus 4 pas ; pas après 10 s de calcul ; μ ∈ [1/4, 4] sur une grille de 1/8 en log₂.
5. **Prior** général par type, appris au banc sur la table commune ; pas de prior par équipe.
6. **Cible inatteignable** : on rend le plus proche avec `reached = false`, jamais un refus.
7. **ROAD** : la cible y est refusée (400) par le moteur et ignorée par Pédalons.
8. **μ s'applique aussi au réseau d'équipe**, après le goût ; le goût reste appris sans consigne, profils inchangés.
9. **Pente non revêtue hors lot** (pas d'altitude dans le graphe), avec un futur lot « altitude ».
10. **Coût de passage** dans le lot, seulement avec cible, au montage de fenêtre, sans fichier hors ligne ; **chaînes
    attractives repoussées** (règle R7).
11. **Bornes** du §6.1, dans le moteur à la fenêtre (pas dans la vue), exemption d'équipe sauf `impassable` — exemption qui
    exige un parcours du type (GRAVEL ou MIXED ; VTT : aussi MTB), pas seulement la règle « d'équipe » actuelle —, sans effet en
    mode strict, `rideability = on` par défaut après bascule ; hachage du profil inchangé.
12. **§7.4** : seule la confirmation par l'équipe elle-même entre ; ni « confirmé par d'autres » ni « suspect ».
13. **« Comme d'habitude » par équipe** : non, sauf si R4 le justifie, et alors dans un lot suivant.
14. **Critères** du §9, écrits avant mesure ; ils ne se relâchent qu'avec le propriétaire.
15. **Mode strict** : ni recherche de μ, ni bornes, ni coût de passage : il reste celui de 1.14.
16. **`reached`** : recalculé par le service sur la première proposition rendue, et par le backend de Pédalons pour chaque
    parcours (`unpavedTargetReached`) ; le backend n'envoie la consigne que si les capacités l'annoncent.
17. **Versions** : la note de départ parlait d'une API 1.14 → 1.15 ; le code dit API **1.13** (`ord-server/src/lib.rs:17`) pour
    le paquet 1.14.0. Le lot fait API 1.13 → **1.14**, paquet 1.14.0 → **1.15.0**.
