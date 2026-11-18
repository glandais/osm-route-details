# Générateur de parcours — lot 4 (4c) : figures organiques au banc, essai de fumée et protocole des campagnes

> Relevé du 18 novembre 2026 sur la branche `feat/ord-loops-lot4c` (depuis `develop` à `6889245`, le 3b fusionné), binaire
> `release` du dernier commit de code (`7c7c857`), Rust 1.100.0, poste de développement (16 fils, 30 Go). Graphe et vues France
> `france-20261115-z11`. Conception : [`ord-loops-lot4.md`](ord-loops-lot4.md), §15 ; décisions du propriétaire : §9.7 ; écarts du
> codage : §15.19. Sonde : [`ord-loops-lot4c-sonde.md`](ord-loops-lot4c-sonde.md).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/lot4/smoke-4c/` (essai) et
> `~/.cache/osm-route-details/graph/lot4/4c/` (contrôles de commit). Lettres d'équipes et agrégats seulement ici ; rien n'a été
> poussé ni déployé.
>
> Marques : **[mesuré]** vient des rapports du banc ; **[calculé]** d'un calcul sur ces mesures. Sans marque dans un tableau :
> mesuré.

## 0. En bref

- **Ce relevé n'est pas celui des campagnes.** Le codage des étapes 1 à 7 et 9 du §15.14 est fait (`ord-loops` : recherche
  point à point, couloir, triangle ; `ord-graph-match` : formes de l'équipe au dépôt ; banc ; service api 1.11). **Aucune campagne
  du §15.15 n'a été lancée** (§15.17) : les critères du §15.12 ne sont donc pas jugés, et **la bascule du défaut (étape 8) n'est
  pas faite**. Le défaut du service reste `classic`.
- **Invariants.** I1 : `Figures::Classic` rend 1.10 au bit près : les références figées passent sans re-bénédiction, et `base` et
  `svc` du banc rejouent leurs empreintes de la sonde sur l'équipe C (20 boucles) : `55bacae189c658ee` et `c584b04e30bf54e9`.
  I2 : `svc+org+tri:0+w:0` rend l'empreinte de `svc` sur les deux équipes de l'essai. I3 : le contrôleur indépendant accepte tout
  parcours organique rendu (test `engine_vs_checker`, des triangles en font partie). I4 : déterminisme et fenêtres plus grandes
  testés sur la référence organique ; le déterminisme à 1 et 16 fils reste à lire à la campagne 3.
- **Essai de fumée** (équipes C et B, 20 boucles, `base`, `svc`, `common+org`, `svc+org`, `svc+org+tri:0+w:0`) : des triangles
  sont rendus et passent le contrôle ; **67 à 77 % des requêtes ROAD de C** et 36 à 43 % de celles de B gardent un triangle ; la
  part de premiers parcours à deux sommets monte de 7 à 50 % (ROAD de C, curseur 1) ; **mais le temps au curseur 1 monte de
  1,5 × sur C (P50)** : un risque pour C1, à lire à la campagne (échantillon de 20 boucles, bruit des 16 fils) [§3].

## 1. Ce qui est codé

| Étape (§15.14) | Commit | Contenu |
|---|---|---|
| 1 | `2061520` | mesures de forme pures partagées avec le banc ; empreinte de `likeness --teams C --limit 5` inchangée (`5ca347d60d501815`) |
| 2 | `96b7f73` | `astar_p2p`, `CorGrid`, `Ban.cor`, `Weighter::min_factor` ; la sonde rejouée sur l'équipe C rend le même relevé aux temps près |
| 3 | `3235185` | `Figures`, `Family::Triangle`, `generate_with_figures_in`, tirage, paires, jambes, épi, choix ; références organiques |
| 4 | `cb3687a` | `shape-common` et `shape_common.rs` généré |
| 5 | `e73324e` | formes de l'équipe au dépôt (`ShapeAcc`, `ProfileMeta.shapes`, `LearnParams.figures`) |
| (fix) | `2bdcccb` | une paire refusée pour un virage serré est reprise à la passe suivante (§15.19, point 1) |
| 6 | `cddfc95` | variantes `org`, section « 4c » de la comparaison |
| 7 | `7c7c857` | service : `figures`, `triangle`, `generator.figures`, `learn_figures`, api 1.11, paquet 1.11.0 |
| 8 | **non fait** | bascule du défaut : attend les campagnes |
| 9 | ce commit | ce relevé, §15.19, étape 4 du §10 |

Constantes générales écrites par `shape-common` [mesuré, 2 071 boucles ROAD, 780 GRAVEL, 4 MTB] : ROAD 838 · 1 098 · 135
boucles à ≤ 1, 2, ≥ 3 sommets à 10 % de D, `tri_q8` 145, r / D des triangles P10 à P90 `0,252 · 0,286 · 0,318 · 0,345 · 0,363` ;
GRAVEL 432 · 335 · 13, `tri_q8` 112, `0,207 · 0,236 · 0,267 · 0,294 · 0,316` ; MTB `tri_q8` 0. Elles redonnent les parts du relevé du
4a au dixième (40,5 / 53,0 / 6,5 et 55,4 / 42,9 / 1,7 %).

## 2. Essai de fumée : commande

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph` :

```
$B likeness --teams <C|B> --limit 20 --holdout-share 0.25 \
  --variants base,svc,common+org,svc+org,svc+org+tri:0+w:0 --label smoke-<C|B> --out $G/lot4/smoke-4c
```

Une minute et demie à trois minutes par équipe. Empreintes [mesuré] : C : `base` `55bacae189c658ee`, `svc`
`c584b04e30bf54e9` (= `svc-org-tri0-w000`), `common-org` `151cda3c498241af`, `svc-org` `749323f5d85ccd98` ; B : `base`
`51ee2e3500688b9d`, `svc` `bdcedda897265fb4` (= `svc-org-tri0-w000`), `common-org` `cfc1f2e869f270a7`, `svc-org` `65b8abac85370c79`.

## 3. Essai de fumée : agrégats

20 boucles par équipe, deux curseurs, jeux sur la surcouche complète ; l'échantillon est petit : **indicatif**.

### 3.1 Les triangles, par requête (`svc+org` ; entre parenthèses `common+org`)

| équipe · type | requêtes | `tri_first` | paires admissibles | cherchées | jambes 2 trouvées | jambes 3 : T2 · A* · échec | complets | gardés | requêtes avec un triangle gardé |
|---|---:|---:|---:|---:|---:|---|---:|---:|---:|
| C · ROAD | 30 | 0,57 | 12,7 (12,6) | 5,1 (5,4) | 4,7 (5,1) | 68 · 61 · 12 (60 · 89 · 7) | 129 (144) | 31 (39) | 0,67 (0,77) |
| C · GRAVEL | 8 | 0,38 | 10,6 (9,8) | 6,5 (5,8) | 6,1 (5,5) | 9 · 41 · 4 (6 · 40 · 1) | 44 (43) | 4 (4) | 0,25 (0,25) |
| B · ROAD | 14 | 0,57 | 5,9 (4,2) | 3,1 (3,4) | 2,2 (2,8) | 7 · 18 · 6 (13 · 22 · 4) | 25 (34) | 7 (9) | 0,36 (0,43) |
| B · GRAVEL | 26 | 0,62 | 5,5 (5,4) | 2,7 (3,2) | 2,8 (3,0) | 16 · 46 · 13 (20 · 38 · 16) | 51 (58) | 31 (27) | 0,69 (0,62) |

- **Refus du contrôle interne** (triangles complets à ±10 % de D, `svc+org`) : C ROAD `SHARP_TURN` 71 et `REUSE_OUTSIDE_TAIL` 19
  sur 109 ; C GRAVEL `SHARP_TURN` 37 sur 41 ; B ROAD `SHARP_TURN` 8, `REUSE_OUTSIDE_TAIL` 2 sur 17 ; B GRAVEL `SHARP_TURN` 14 sur 45.
  **Le virage serré est le principal frein**, même après le commit `2bdcccb` (qui a fait passer C ROAD de 40 à 77 % des requêtes
  avec un triangle) : la règle des grappes de 100 m n'est pas modélisée par la recherche (§15.19, point 1).
- **Aucune jambe 2 abandonnée au plafond** (`leg2_capped` 0) ; paires cherchées 2,7 à 6,5 par requête (au plus 4 par passe,
  reprises comprises) ; épis : 21 relevés à B GRAVEL, tous réparés ; 1 épi gardé en tout (C ROAD, `svc+org`).
- **β final** (C ROAD, `svc+org`) : P10 · P50 · P90 `0,996 · 1,039 · 1,130`, autour du biais initial 1,04.

### 3.2 Formes (premier parcours gardé, toutes équipes de l'équipe jouée, lecture « toutes »)

P50 de r / D et de la compacité ; part des premiers à `vertices_10 = 2` (réel entre parenthèses) :

| équipe · type · curseur | r / D réel | `svc` | `svc+org` | compacité réel | `svc` | `svc+org` | v2 `svc` | v2 `svc+org` | v2 réel |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| C · ROAD 0,25 | 0,308 | 0,354 | 0,331 | 0,251 | 0,284 | 0,347 | 0,50 | 0,57 | 0,40 |
| C · ROAD 1 | 0,308 | 0,401 | 0,356 | 0,251 | 0,205 | 0,244 | 0,07 | 0,29 | 0,40 |
| B · ROAD 0,25 | 0,313 | 0,374 | 0,369 | 0,217 | 0,161 | 0,289 | 0,29 | 0,43 | 0,43 |
| B · ROAD 1 | 0,313 | 0,385 | 0,377 | 0,217 | 0,278 | 0,346 | 0,00 | 0,43 | 0,43 |
| B · GRAVEL 0,25 | 0,268 | 0,356 | 0,318 | 0,230 | 0,191 | 0,282 | 0,27 | 0,45 | 0,23 |
| B · GRAVEL 1 | 0,268 | 0,369 | 0,333 | 0,230 | 0,168 | 0,246 | 0,00 | 0,55 | 0,23 |

Le r / D médian baisse de 0,01 à 0,05 sous `svc+org` (la lentille garde son r / D, §15.0 point 9) ; la part de figures à deux sommets
monte nettement au curseur 1 (7 → 29 %, 0 → 43 à 55 %). Un jeu de 7 à 14 boucles ne juge pas les seuils A1 à A4.

### 3.3 Service (contre `svc` du même tirage)

| équipe · curseur | `svc` P50 / P90 (ms) | `svc+org` P50 / P90 | × `svc` (P50 · P90) | `OK` | PANIC · WINDOW_ESCAPE · VIOLATION |
|---|---|---|---|---:|---|
| C · 0,25 | 2 145 / 4 135 | 1 693 / 5 037 | 0,79 · 1,22 | 90 % | 0 · 0 · 0 |
| C · 1 | 913 / 3 043 | 1 442 / 4 633 | **1,58 · 1,52** | 90 % | 0 · 0 · 0 |
| B · 0,25 | 105 / 207 | 106 / 250 | 1,01 · 1,21 | 90 % | 0 · 0 · 0 |
| B · 1 | 141 / 191 | 155 / 278 | 1,10 · 1,45 | 90 % | 0 · 0 · 0 |

**Lecture honnête** : 16 fils sur 8 cœurs physiques, 20 boucles. Deux exécutions identiques (`svc` et `svc-org-tri0-w000`, même
empreinte) donnent des P50 à ×0,96 · ×1,03 sur C et ×0,86 · ×1,03 sur B [mesuré] : le bruit est de l'ordre de 5 à 15 %. Le critère C1 (P50 ≤ 1,15 × `svc`, P90 ≤ 1,25 ×) **n'est pas tenu sur
cet essai au curseur 1** ; l'estimation de la sonde (+8 % pour quatre paires) ne tient plus avec les reprises de passe et le
surcoût du couloir [calculé : ~5 paires cherchées en moyenne au lieu de 4, 0,3 à 0,6 × T1 d'états de recherche par requête, contre
0,29 × T1 pour quatre paires à la sonde]. À lire à la campagne 4 avant toute bascule ; leviers prévus au §15.16 : `pairs`, `wide`, et
ici le nombre de reprises par passe (§15.19, point 1).

## 4. Commandes exactes des campagnes (§15.15)

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`, sorties sous `$G/lot4/4c/`. Une campagne à la fois, la
machine pour elle seule. Les noms de variantes : `svc-org`, `common-org`, `svc-org-w000`, `svc-org-tri0`, `svc-org-lc`,
`svc-org-w002`, `svc-org-w005`, `svc-org-pairs8`, `svc-org-wide`, `svc-org-w<w>-k15`, `svc-org-w<w>-k30` (§15.19, point 5).

| # | Après | Commande (`$B likeness …`, `--out $G/lot4/4c`) | Durée [estimé] |
|---|---|---|---|
| 0 | commit 1 | `--teams C --limit 5 --label 4c-refacto` : fait, `5ca347d60d501815` avant et après | 1 min |
| 1 | commit 6 | `--teams C --limit 20 --holdout-share 0.25 --variants base,svc,svc+org+tri:0+w:0 --label 4c-neutre` : `svc-org-tri0-w000` = `svc` (fait sur C et B à l'essai) | 3 min |
| 2 | — | `--teams B,C --limit 20 --holdout-share 0.25 --variants base,svc,common+org,svc+org --label 4c-essai` | 6 min |
| 3 | — | `--teams B,C --holdout-share 0.25 --variants base,svc+org --threads 1 --label 4c-det1` puis `--threads 16 --label 4c-det16` : mêmes empreintes (critère D) | 25 min |
| 4 | — | `--holdout-share 0.25 --variants base,common,svc,common+org,svc+org,svc+org+w:0,svc+org+tri:0,svc+org+lc --label lot4-4c` | ~65 min, RSS ≤ 21 Gio |
| 5 | — | `--holdout-share 0.25 --variants base,svc,svc+org+w:0.02,svc+org+w:0.05,svc+org+pairs:8,svc+org+wide --label lot4-4c-r1` | ~55 min |
| 6 | — | `--holdout-share 0.25 --variants base,svc,svc+org+w:<w*>+k:1.5,svc+org+w:<w*>+k:3[,svc+org+w:<w*>+boost:<b>] --label lot4-4c-r2` (`<w*>` : choix du §15.6 lu après la campagne 5) | ~40 min |
| 7 | commit 8 | `--holdout-share 0.25 --variants base,common,svc,common+org,svc+org --label lot4-4c-final` (défauts retenus), puis un second tirage des temps | ~45 + 25 min |
| 8 | commit 8 | séance en conteneur du §14.13 (`docker build -t osm-route-details:lot4-4c .`, défauts organiques) | ~45 min |

Lecture : `<label>-compare.md`, section 10 « 4c » (critères A1 à A4, B1, B2, C1, C2 « tient » ou « ne tient pas » avec leurs
chiffres, règle L si `svc-org-lc` est joué). D se lit à la campagne 3 (empreintes), C3 à la séance en conteneur. Le banc s'arrête
si l'empreinte de `base` n'est pas `4686c904975302e7` aux réglages de la sonde (toutes équipes, `--holdout-share 0.25`) ; celle de
`svc`, `384f003c0bd19b7f`, est affichée.

**Décisions de bascule (§9.7, points 3 et 4)** : si A1 à A4 (A3 en ROAD), B1, B2, C1, C2 et D tiennent, le défaut passe à `organic`
dans un commit à part (`generator.figures = organic`, `learn_figures` selon la règle L, `OrganicTune::default()` aux réglages
retenus, référence organique re-bénie si le réglage a changé, `CHANGELOG` « Changé »), suivi de la séance en conteneur ; le commit
est annulé (`git revert`) si C3 manque. B2 : réserve du fait (d) levée à ≤ +0,03 aux deux curseurs, maintenue jusqu'à +0,054 ;
au-delà, pas de bascule et la table commune organique est proposée au propriétaire (elle n'est pas faite). La fusion revient au
propriétaire.

## 5. Ce que l'essai ne dit pas

- Rien sur les seuils du §15.12 : 7 à 14 boucles par groupe, deux équipes ; les groupes GRAVEL de C n'ont que 3 boucles.
- Rien sur la mémoire ni sur le conteneur de 3 Gio ; les temps sont ceux de 16 fils simultanés (§3.3).
- Les équipes A et B dépassent le seuil de 50 boucles ; A n'a pas été jouée : le mélange de l'équipe (ses formes au dépôt) n'a donc
  été lu que pour B et C (C sous le seuil : mélange général).
- `generator.profile.learn_figures` reste `classic` ; la règle L (§15.12) se lit à la campagne 4 (`svc+org+lc`).
