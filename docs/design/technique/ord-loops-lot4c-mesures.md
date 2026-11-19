# Générateur de parcours — lot 4 (4c) : figures organiques au banc, campagnes et lecture des critères

> Relevé du 18 novembre 2026 sur la branche `feat/ord-loops-lot4c` (depuis `develop` à `6889245`, le 3b fusionné). Campagnes
> jouées avec le binaire `release` de `ea6ef63` (le dernier commit de code est `1b91dfd`, la relecture), Rust 1.100.0, poste
> de développement (16 fils sur 8 cœurs physiques, 30 Go). Graphe et vues France `france-20261115-z11`. Conception :
> [`ord-loops-lot4.md`](ord-loops-lot4.md), §15 ; décisions du propriétaire : §9.7 ; écarts du codage : §15.19. Sonde :
> [`ord-loops-lot4c-sonde.md`](ord-loops-lot4c-sonde.md).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/lot4/4c/` (campagnes),
> `…/lot4/4c-conteneur/` (séance en conteneur), `…/lot4/smoke-4c/` et `…/lot4/smoke-4c-review/` (essais). Lettres d'équipes
> et agrégats seulement ici : ni départ, ni coordonnée, ni identifiant d'équipe, de trace ou d'arête. Rien n'a été poussé ni
> déployé.
>
> Marques : **[mesuré]** vient des rapports du banc ou de la séance en conteneur ; **[calculé]** d'un calcul fait ici sur ces
> mesures. Sans marque dans un tableau : mesuré. Les temps du banc sont pris à 16 fils simultanés : comparables entre variantes
> d'un même tirage, pas à un relevé à un fil ni au service.

> **Mise à jour du 19 novembre 2026** : l'étape « virages » du §16 de la note de conception est codée ; son essai de fumée est
> au **§14** (les §0 à §13 restent le relevé du 18 novembre, pris avant l'étape).

## 0. En bref

- **Bascule : non.** Le défaut du service reste `classic`. Aucun réglage du §15.15 ne tient tous les critères de la bascule.
  Pour `svc+org` aux réglages par défaut, **A1, A3 (ROAD), B1, B2 et D tiennent** ; **C1, A2, A4 et C2 manquent** [§5 à §8].
  Le commit de bascule n'est donc pas fait. La séance en conteneur de C3, prévue après la bascule, ne l'est pas non plus.
  Une séance **indicative** l'a remplacée : les deux figures y sont servies dans le même conteneur de 3 Gio. L'organique y
  passerait le seuil en série (P50 0,28 s pour ≤ 0,29), mais **pas à 4 clients** (0,66 s pour ≤ 0,56). La mémoire tient
  (pic `anon` 1,49 Gio), et une 1.10 garde les profils écrits par la 1.11 [§11].
- **Le temps manque.** Au banc, `svc+org` prend **× 1,29 · × 1,38** le P50 de `svc` (curseurs 0,25 · 1), et C1 demande
  ≤ 1,15. Le couloir seul (`tri:0`) coûte × 1,10 · × 1,12, et les triangles le reste. Aucun réglage de `w`, κ, `pairs` ou `wide`
  ne passe sous 1,15. Sans reprise (`noretry`), le temps passe (× 1,05 · × 1,12 à `w` 0,02) mais r / D et la part de triangles
  manquent alors A1 et A2 [§8, §10].
- **La forme gagne nettement** : W1 / IQR de r / D à **× 0,46 · × 0,49** de `svc` en ROAD, **× 0,66 · × 0,72** en GRAVEL. La
  compacité ROAD revient au réel (P50 0,330 contre 0,320 réel et 0,217 sous `svc`). La part de premiers à deux sommets passe de 6 à 7 %
  à **51 et 49 %** en ROAD (réel 53 %). En GRAVEL, elle passe à 40 et 32 % (réel 43 %) : **0,317 au curseur 1, sous la borne de
  0,32**. Le partage de route (`shared_share`) s'éloigne du réel en GRAVEL (A4) [§6].
- **Le goût tient (B1)**, et la **réserve du fait (d) est levée** : A ROAD, `svc-org − common-org` = −0,058 · −0,045. Le profil
  d'équipe sert mieux que la table commune sous les figures organiques, alors que `svc − common` valait +0,011 · +0,021 dans le
  même tirage. **Règle L non tenue** (`lc` pire de 0,051) : une bascule devrait apprendre en `organic` [§7].
- **Déterminisme (D) tenu** : mêmes empreintes à 1 et 16 fils sur B et C. Neutralité (I2 au banc) tenue. `base` et `svc` ont
  rendu les empreintes de la sonde à chaque campagne [§5].
- **Après le 4c : le 4d** (décision 6 du §9.7 : A1 tient). La bascule du défaut reste ouverte. Elle demande une décision du
  propriétaire sur C1 et sur A4, ou une étape de plus contre le refus `SHARP_TURN` (§13).

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
| 8 | **non fait** | bascule du défaut : les critères ne tiennent pas (§13) |
| 9 | `a28be40`, ce commit | relevé, §15.19, étape 4 du §10 |
| relecture | `1d218ed` | facteur minimal lu sur T1, sans marge ni fenêtre (§15.19, point 11) |
| relecture | `e6a75be` | gabarit du couloir refait quand le rayon change (point 12) |
| relecture | `5852ea3` | au plus `max_pairs` paires distinctes par requête, reprises à part ; `sharp_retry`, option `noretry` (point 13) |
| relecture | `63468df` | message d'arrêt du banc (empreinte de la sonde) |
| relecture | `1b91dfd` | une seule réparation d'épi par triangle (point 14) |

Constantes générales écrites par `shape-common` [mesuré, 2 071 boucles ROAD, 780 GRAVEL, 4 MTB] : ROAD 838 · 1 098 · 135
boucles à ≤ 1, 2, ≥ 3 sommets à 10 % de D, `tri_q8` 145, r / D des triangles P10 à P90 `0,252 · 0,286 · 0,318 · 0,345 · 0,363` ;
GRAVEL 432 · 335 · 13, `tri_q8` 112, `0,207 · 0,236 · 0,267 · 0,294 · 0,316` ; MTB `tri_q8` 0. Elles redonnent les parts du relevé du
4a au dixième (40,5 / 53,0 / 6,5 et 55,4 / 42,9 / 1,7 %).

## 2. Essai de fumée (avant les campagnes) : commande

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph` :

```
$B likeness --teams <C|B> --limit 20 --holdout-share 0.25 \
  --variants base,svc,common+org,svc+org,svc+org+tri:0+w:0 --label smoke-<C|B> --out $G/lot4/smoke-4c
```

Une minute et demie à trois minutes par équipe. Empreintes [mesuré] : C : `base` `55bacae189c658ee`, `svc`
`c584b04e30bf54e9` (= `svc-org-tri0-w000`), `common-org` `151cda3c498241af`, `svc-org` `749323f5d85ccd98` ; B : `base`
`51ee2e3500688b9d`, `svc` `bdcedda897265fb4` (= `svc-org-tri0-w000`), `common-org` `cfc1f2e869f270a7`, `svc-org` `65b8abac85370c79`.

## 3. Essai de fumée : agrégats

20 boucles par équipe, deux curseurs, jeux sur la surcouche complète ; l'échantillon est petit : **indicatif**. Les §3.1 à
§3.3 sont **d'avant la relecture** ; le §3.4 rejoue l'essai après elle.

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

### 3.3 Service (contre `svc` du même tirage)

| équipe · curseur | `svc` P50 / P90 (ms) | `svc+org` P50 / P90 | × `svc` (P50 · P90) | `OK` | PANIC · WINDOW_ESCAPE · VIOLATION |
|---|---|---|---|---:|---|
| C · 0,25 | 2 145 / 4 135 | 1 693 / 5 037 | 0,79 · 1,22 | 90 % | 0 · 0 · 0 |
| C · 1 | 913 / 3 043 | 1 442 / 4 633 | 1,58 · 1,52 | 90 % | 0 · 0 · 0 |
| B · 0,25 | 105 / 207 | 106 / 250 | 1,01 · 1,21 | 90 % | 0 · 0 · 0 |
| B · 1 | 141 / 191 | 155 / 278 | 1,10 · 1,45 | 90 % | 0 · 0 · 0 |

Deux exécutions identiques (`svc` et `svc-org-tri0-w000`, même empreinte) donnaient des P50 à ×0,96 · ×1,03 sur C et ×0,86 ·
×1,03 sur B [mesuré] : un bruit de 5 à 15 % sur 20 boucles.

### 3.4 Après la relecture

Même commande que le §2, variantes `base,svc,common+org,svc+org,svc+org+noretry,svc+org+tri:0+w:0`, `--label review2-<C|B>`,
sorties sous `$G/lot4/smoke-4c-review/`, binaire `release` de `1b91dfd` [mesuré]. Empreintes : `base` et `svc` inchangées (C
`55bacae189c658ee`, `c584b04e30bf54e9` ; B `51ee2e3500688b9d`, `bdcedda897265fb4`), `svc-org-tri0-w000` = `svc` sur les deux ; C :
`svc-org` `c0324bdca9385c0f`, `common-org` `43caf4010b25b3ec`, `svc-org-noretry` `3188dadbdb3debde` ; B : `svc-org`
`d43b5e1115120b68`, `common-org` `cfc1f2e869f270a7` (inchangée), `svc-org-noretry` `d5501b85edb489d5`. Rejouées deux fois sur C
(avant et après le commit `1b91dfd`) : mêmes empreintes.

| équipe · type | variante | recherches par requête (reprises) | états des jambes 2 par requête | requêtes avec un triangle gardé |
|---|---|---|---:|---:|
| C · ROAD | `svc+org` | 4,37 (0,90) | 177 317 | 0,53 |
| C · ROAD | `svc+org+noretry` | 3,30 (0) | 117 292 | 0,33 |
| C · GRAVEL | `svc+org` | 5,50 (2,25) | — | 0,25 |
| B · ROAD | `svc+org` | 3,07 (0,29) | — | 0,36 |
| B · GRAVEL | `svc+org` | 2,62 (0,27) | — | 0,65 |
| B · GRAVEL | `svc+org+noretry` | 2,38 (0) | — | 0,58 |

| équipe · curseur | `svc+org` × `svc` (P50 · P90) | `svc+org+noretry` × `svc` | `svc+org+tri:0+w:0` × `svc` (bruit) |
|---|---|---|---|
| C · 0,25 | 0,89 · 1,01 | 0,92 · 1,02 | 0,93 · 0,97 |
| C · 1 | 1,11 · 1,41 | 1,09 · 1,37 | 0,92 · 0,95 |
| B · 0,25 | 1,10 · 1,23 | 1,11 · 1,03 | 0,95 · 1,00 |
| B · 1 | 1,06 · 1,52 | 1,10 · 1,11 | 0,99 · 1,05 |

L'essai ne jugeait rien : 7 à 14 boucles par groupe, deux équipes, A absente. Les campagnes l'ont remplacé.

## 4. Campagnes : protocole et périmètre

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`, chaque commande avec `--out $G/lot4/4c`, binaire
reconstruit sur `ea6ef63` avant la première. Une campagne à la fois, la machine pour elle seule. Durées et pics [mesuré] (pic de
mémoire résidente du processus, `time -v`) :

| # | Commande (`$B likeness …`) | Durée | Pic RSS |
|---|---|---:|---:|
| 1 | `--teams C --limit 20 --holdout-share 0.25 --variants base,svc,svc+org+tri:0+w:0 --label 4c-neutre` | 1 min 29 | 15,4 Gio |
| 2 | `--teams B,C --limit 20 --holdout-share 0.25 --variants base,svc,common+org,svc+org --label 4c-essai` | 2 min 48 | 15,2 Gio |
| 3 | `--teams B,C --holdout-share 0.25 --variants base,svc+org --threads 1 --label 4c-det1`, puis `--threads 16 --label 4c-det16` | 11 min 18 · 1 min 54 | 10,4 · 15,1 Gio |
| 4 | `--holdout-share 0.25 --variants base,common,svc,common+org,svc+org,svc+org+w:0,svc+org+tri:0,svc+org+lc --label lot4-4c` | 49 min 42 | 19,2 Gio |
| 5 | `--holdout-share 0.25 --variants base,svc,svc+org+w:0.02,svc+org+w:0.05,svc+org+pairs:8,svc+org+wide,svc+org+noretry --label lot4-4c-r1` | 49 min 35 | 18,7 Gio |
| 6 | `--holdout-share 0.25 --variants base,svc,svc+org+w:0.02+k:1.5,svc+org+w:0.02+k:3,svc+org+w:0.02+boost:1.5,svc+org+w:0.02+noretry --label lot4-4c-r2` | 41 min 02 | 18,6 Gio |

- **Périmètre** des campagnes 4 à 6 : toutes les équipes, 2 855 boucles retenues (dont A : ROAD 1 860, GRAVEL 545 ; B : ROAD 167,
  GRAVEL 228), un quart mis à l'écart pour la reconstruction, deux curseurs (0,25 et 1). Par variante : 4 130 requêtes ROAD et
  1 560 GRAVEL jouées en organique (surcouche complète et surcouche d'écart). Les critères se lisent sur le premier parcours
  gardé, lecture « toutes », toutes équipes, quatre groupes (ROAD et GRAVEL à chaque curseur). La lecture se trouve dans la
  section 10 « 4c » de `<label>-compare.md`.
- **Campagne 6** : jouée à `w` = 0,02 (le choix du §15.6, appliqué sans C1, que rien ne tient ; §10 et §15.19, point 16). `boost:1.5`
  vient de la règle du §15.12 (A2 sous la borne en GRAVEL : part réelle 0,429 / part obtenue 0,283 à `w` 0,02, soit 1,52, plafonné à
  1,5). `svc+org+w:0.02+noretry` est ajouté pour mesurer le levier du temps.
- **Campagne 7** (finale, après la bascule) et **séance C3** : non jouées, la bascule n'étant pas faite. La séance indicative du
  §11 les remplace pour le temps en conteneur.
- La reprise d'un seul P50 au second tirage (C1 entre 1,15 et 1,20) ne s'applique à aucun candidat : `svc+org` est à 1,29 et 1,38.

## 5. Contrôles et déterminisme

| Contrôle | Attendu | Obtenu [mesuré] | |
|---|---|---|---|
| `base`, toutes équipes (campagnes 4, 5, 6) | `4686c904975302e7` | `4686c904975302e7` aux trois | tient |
| `svc`, toutes équipes (campagnes 4, 5, 6) | `384f003c0bd19b7f` | `384f003c0bd19b7f` aux trois | tient |
| Neutralité (I2 au banc), C, 20 boucles | `svc-org-tri0-w000` = `svc` | `c584b04e30bf54e9` aux deux ; `base` `55bacae189c658ee` | tient |
| D : B et C, `base` à 1 et 16 fils | mêmes empreintes | `052224c85ef4145d` aux deux | tient |
| D : B et C, `svc-org` à 1 et 16 fils | mêmes empreintes | `de990d0fb10115b3` aux deux | **tient** |

Les variantes organiques sont déterministes : leurs empreintes ne dépendent pas du tirage des temps. Les formes et le goût de
`svc-org-w002`, par exemple, se comparent donc d'une campagne à l'autre ; seuls les temps se lisent contre le `svc` du même tirage.
Empreintes de la campagne 4 : `common` `07b62287e64fa258`, `common-org` `83dd3bf887233e28`, `svc-org` `c48118447315c118`,
`svc-org-w000` `1265fd2c543728bb`, `svc-org-tri0` `a6a90f2d9caf43d1`, `svc-org-lc` `c1b979279b9bde93`.

## 6. Formes : réel, classique, organique

Premier parcours gardé, toutes équipes, lecture « toutes ». Chaque cellule : P50 · W1 / IQR (IQR du réel du type). `svc` est le
classique du service (1.10), `svc-org` l'organique aux réglages par défaut (campagne 4) [mesuré].

| type · curseur | mesure | réel P50 | `svc` | `svc-org` | × `svc` (W1) [calculé] |
|---|---|---:|---|---|---:|
| ROAD 0,25 | r / D | 0,329 | 0,404 · 1,240 | 0,372 · 0,567 | 0,46 |
| ROAD 1 | r / D | 0,329 | 0,405 · 1,263 | 0,380 · 0,615 | 0,49 |
| GRAVEL 0,25 | r / D | 0,285 | 0,355 · 1,281 | 0,341 · 0,843 | 0,66 |
| GRAVEL 1 | r / D | 0,285 | 0,357 · 1,290 | 0,344 · 0,925 | 0,72 |
| ROAD 0,25 | compacité | 0,320 | 0,217 · 0,614 | 0,330 · 0,124 | 0,20 |
| ROAD 1 | compacité | 0,320 | 0,210 · 0,637 | 0,322 · 0,120 | 0,19 |
| GRAVEL 0,25 | compacité | 0,226 | 0,196 · 0,203 | 0,245 · 0,341 | 1,68 |
| GRAVEL 1 | compacité | 0,226 | 0,177 · 0,349 | 0,251 · 0,273 | 0,78 |
| ROAD 0,25 · 1 | `vertices_10` (W1) | — | 0,594 · 0,592 | 0,102 · 0,138 | 0,17 · 0,23 |
| GRAVEL 0,25 · 1 | `vertices_10` (W1) | — | 0,373 · 0,420 | 0,047 · 0,135 | 0,13 · 0,32 |
| ROAD 0,25 · 1 | `shared_share` (W1) | 0,002 | 0,715 · 0,776 | 0,568 · 0,719 | — |
| GRAVEL 0,25 · 1 | `shared_share` (W1) | 0,007 | 0,472 · 0,460 | 0,677 · 0,672 | **+0,205 · +0,212** |

Parts des figures (premier parcours) et médianes par famille :

| type · curseur | v2 réel | v2 `svc` | v2 `svc-org` | part `Triangle` | r / D triangles · lentilles | compacité triangles · lentilles |
|---|---:|---:|---:|---:|---|---|
| ROAD 0,25 | 0,530 | 0,064 | **0,507** | 0,378 | 0,308 · 0,395 | 0,450 · 0,284 |
| ROAD 1 | 0,530 | 0,069 | **0,491** | 0,383 | 0,306 · 0,401 | 0,438 · 0,280 |
| GRAVEL 0,25 | 0,429 | 0,090 | **0,402** | 0,268 | 0,274 · 0,352 | 0,318 · 0,230 |
| GRAVEL 1 | 0,429 | 0,043 | **0,317** | 0,221 | 0,271 · 0,354 | 0,313 · 0,233 |

- **Les triangles portent le gain** : leur r / D médian (0,306 à 0,308 en ROAD, 0,271 à 0,274 en GRAVEL) est celui des triangles
  réels (0,318 et 0,267, §1). Les lentilles restent à 0,395 à 0,401 (ROAD) et 0,352 à 0,354 (GRAVEL), comme prévu au §15.0,
  point 9. Le P50 de r / D ROAD (0,372 et 0,380) reste au-dessus de 0,36 : il était rapporté, pas exigé (A1).
- **Le couloir seul** (`svc-org-tri0`) baisse peu r / D (W1 × 0,91 · × 0,92 en ROAD) mais ouvre la lentille : compacité ROAD 0,274 au
  lieu de 0,217. **Les triangles sans couloir** (`svc-org-w000`) donnent r / D × 0,50 · × 0,55, mais la compacité de leurs lentilles
  reste à 0,210 : les deux leviers s'additionnent [mesuré, campagne 4].
- **Par équipe** (A et B, ROAD, `svc` puis `svc-org`, W1 / IQR de r / D) : A 1,263 → 0,546 et 1,272 → 0,589 ; B 1,151 → 0,891 et
  1,222 → 0,935. B gagne moins : son mélange d'équipe tire moins de triangles (part `Triangle` 0,17 à 0,18 en ROAD, contre 0,40 pour
  A), et son ROAD réel n'a que 44 % de figures à deux sommets.
- **GRAVEL** : la compacité dépasse le réel (0,245 et 0,251 contre 0,226), et la part à deux sommets manque la borne au curseur 1
  (0,317 contre 0,32). Le `shared_share` réel est presque nul (P50 0,007), et son IQR l'est aussi. Les figures organiques
  partagent encore moins de route, et la W1 / IQR monte de 0,2 : c'est ce qui fait manquer A4. Toutes les variantes organiques
  le manquent, sauf `w002-k15` et `w002-noretry` (§10).

## 7. Goût et réserve du fait (d)

`S / S_base`, lecture « toutes » [mesuré, campagne 4] :

| variante | ROAD 0,25 | ROAD 1 | GRAVEL 0,25 | GRAVEL 1 | A ROAD 0,25 · 1 | B ROAD 0,25 · 1 |
|---|---:|---:|---:|---:|---|---|
| `common` | 0,253 | 0,205 | 0,365 | 0,298 | 0,192 · 0,153 | 1,875 · 2,125 |
| `svc` | 0,223 | 0,190 | 0,290 | 0,214 | 0,203 · 0,174 | 0,886 · 0,969 |
| `common-org` | 0,323 | 0,280 | 0,299 | 0,260 | 0,261 · 0,225 | 1,800 · 2,032 |
| `svc-org` | 0,230 | 0,194 | 0,268 | 0,225 | 0,203 · 0,180 | 0,778 · 0,825 |
| `svc-org-lc` | 0,281 | 0,226 | 0,255 | 0,214 | 0,254 · 0,201 | 0,740 · 0,883 |

- **B1 tient** pour `svc-org` : au plus +0,011 sur `svc` en « toutes » et +0,029 en « écart » (GRAVEL 1 : 0,260 contre 0,231 ;
  GRAVEL 0,25 : 0,330 contre 0,302, +0,028). La marge est de 0,03, si bien que le GRAVEL « écart » est au bord.
- **B2, fait (d) organique** : A ROAD, `svc-org − common-org` = **−0,058 · −0,045** [calculé] → **réserve levée** (décision 3 du
  §9.7 : ≤ +0,03 aux deux curseurs). La table commune apprise en `Classic` sert mal les figures organiques (`common-org` perd 0,07
  sur `common` en ROAD), alors que le profil d'équipe les sert aussi bien que la lentille. Dans le même tirage, `svc − common` vaut
  +0,011 · +0,021, l'écart accepté au §9.6. La table commune organique n'a donc pas lieu d'être proposée.
- **B3, cases à la borne** : aucune classe signalée. Parts de longueur réel · `svc` · `svc-org` : ROAD classe 64 (`COMMON_Q8` 64) 0,008 ·
  0,001 · 0,001 ; GRAVEL 34 (64) 0,002 · 0,001 · 0,001 ; GRAVEL 59 (1 024) 0,000 aux trois. Classes à la borne cumulée [1/4, 4] des
  profils appris : A 6 sous `svc` et sous `svc-org`, B 5 sous `svc` et 4 sous `svc-org`.
- **Règle L non tenue** : `svc-org-lc` (appris en `Classic`, servi organique) est pire que `svc-org` de **+0,051** en ROAD 0,25
  (0,281 contre 0,230) et de +0,032 en ROAD 1. B2 tient pour `lc` (−0,007 · −0,024). En cas de bascule, `learn_figures` passerait
  donc à `organic` : tous les profils deviendraient `incompatible`, et une nuit de redépôt suivrait (§15.12).

## 8. Service : temps et issues

Contre `svc` du même tirage, temps par appel à 16 fils [mesuré, campagne 4] :

| variante | curseur | P50 / P90 (ms) | × `svc` P50 · P90 | `OK` | VIOLATION |
|---|---:|---|---|---:|---:|
| `svc` | 0,25 | 323 / 800 | 1 · 1 | 98,2 % | 0 |
| `svc` | 1 | 273 / 817 | 1 · 1 | 97,9 % | 0 |
| `svc-org` | 0,25 | 416 / 1 048 | **1,29 · 1,31** | 98,1 % | 2 |
| `svc-org` | 1 | 378 / 1 019 | **1,38 · 1,25** | 97,9 % | 0 |
| `svc-org-tri0` (couloir seul) | 0,25 · 1 | 355 / 833 · 305 / 818 | 1,10 · 1,04 ; 1,12 · 1,00 | 98,2 · 97,9 % | 1 · 1 |
| `svc-org-w000` (triangles seuls) | 0,25 · 1 | 395 / 1 033 · 351 / 961 | 1,22 · 1,29 ; 1,28 · 1,18 | 98,0 · 97,9 % | 0 · 0 |
| `common-org` | 0,25 · 1 | 367 / 996 · 344 / 872 | 1,14 · 1,25 ; 1,26 · 1,07 | 98,3 · 98,0 % | 0 · 0 |

- **C1 ne tient pas** pour `svc-org` : P50 × 1,29 · × 1,38 contre ≤ 1,15 ; P90 × 1,31 au curseur 0,25 (≤ 1,25). La sonde estimait +8 %
  [calculé, sonde §6] ; la mesure donne +29 à +38 %, qui se décomposent en +10 à +12 % pour le couloir (`tri0`) et le reste pour
  les triangles. Une requête ROAD cherche 3,69 paires dont 0,73 reprises ; en GRAVEL, 4,28 paires dont 1,03 reprises. Les états des
  jambes 2 et 3 montent à 63 838 et 4 502 par requête ROAD, 101 753 et 10 432 en GRAVEL.
- **C2 ne tient pas, de peu** : aucune `PANIC` ni `WINDOW_ESCAPE`, `OK` à 0,1 point de `svc`, mais **2 jeux `VIOLATION`** au curseur
  0,25 sur la surcouche complète, contre 0 (3 avec la surcouche d'écart, sur 7 134 jeux). Un jeu `VIOLATION` est un jeu dont tous les parcours rendus sont refusés par le contrôleur indépendant
  (`ord-loops-check`), ce qui n'est pas une faute de I3. Les trois jeux concernés sont des GRAVEL de A, un seul parcours rendu,
  refusé `SHARP_TURN`. Sur l'ensemble des parcours rendus, le contrôleur indépendant en refuse 34 sur 19 479 sous `svc-org`,
  0 sous `svc`, 5 sous `common` (en `Classic`), 10 sous `tri0` et 2 sous `w000`, toujours pour `SHARP_TURN` [mesuré]. Le contrôle
  interne du moteur et le contrôleur indépendant divergent donc déjà en `Classic`, et le couloir multiplie les cas. Le service
  ne rend que ce que le contrôleur indépendant accepte (code `checker_rejected`).
- La mémoire du banc reste sous la borne de 21 Gio : pic de 19,2 Gio sur la campagne 4.

## 9. Triangles : compteurs par requête

`svc-org`, campagne 4 [mesuré] :

| type | requêtes | `tri_first` | paires admissibles | cherchées (reprises) | complets | à ±10 % | gardés | requêtes avec triangle | β P10 · P50 · P90 |
|---|---:|---:|---:|---|---:|---:|---:|---:|---|
| ROAD | 4 130 | 0,586 | 9,58 | 3,69 (0,73) | 13 775 | 11 143 | 4 403 | 0,634 | 1,005 · 1,057 · 1,254 |
| GRAVEL | 1 560 | 0,450 | 10,01 | 4,28 (1,03) | 6 080 | 5 195 | 1 298 | 0,532 | 0,983 · 1,038 · 1,223 |

- **Refus du contrôle interne**, triangles à ±10 % de D : ROAD `SHARP_TURN` 6 147, `REUSE_OUTSIDE_TAIL` 753,
  `TOO_MANY_CROSSINGS` 147 ; GRAVEL `SHARP_TURN` 3 805, `TOO_MANY_CROSSINGS` 237, `REUSE_OUTSIDE_TAIL` 105,
  `EDGE_USED_MORE_THAN_TWICE` 31. **Le virage serré fait 87 % des refus en ROAD et 91 % en GRAVEL** [calculé]. Le refus
  `REUSE_OUTSIDE_TAIL` (§15.19, point 15) ne pèse que 6,8 % des triangles ROAD à ±10 % : il n'est pas le frein.
- **Jambes** : aucune jambe 2 au plafond ; jambes 3 ROAD par le chemin T2 6 745, par A* 7 509, échecs 742.
- **Épis** : 795 relevés en ROAD (748 réparés, 47 jetés), 31 gardés sur les parcours rendus ; 1 034 en GRAVEL (938, 96), 42 gardés.
- **β** reste centré sur le biais initial, avec une queue haute (P90 1,22 à 1,25).
- **Sans reprise** (`noretry`, campagne 5) : 2,97 paires par requête ROAD et 3,09 en GRAVEL, mais la part de requêtes avec un
  triangle tombe à 0,453 (ROAD) et 0,268 (GRAVEL). Les reprises après un virage serré apportent donc près d'un tiers des requêtes
  ROAD avec un triangle, et la moitié des GRAVEL [calculé].

## 10. Réglage : couloir, κ, paires, bande

Critères par variante, pour le candidat contre `svc` de son tirage. A3 n'entre dans la bascule que pour le ROAD, qui tient pour
toutes les variantes ; la colonne A3 ci-dessous porte les quatre groupes. B2 ne se lit qu'à la campagne 4, qui seule joue
`common+org` [mesuré] :

| variante (campagne) | A1 | A2 | A3 (4 groupes) | A4 | B1 | C1 : P50 · P90 (0,25 ; 1) | C2 | W1 r / D ROAD (0,25 · 1) | compacité ROAD (W1) |
|---|---|---|---|---|---|---|---|---|---|
| `svc-org` (4) | oui | non | non | non | oui | 1,29 · 1,31 ; 1,38 · 1,25 | non | 0,567 · 0,615 | 0,124 · 0,120 |
| `svc-org-w000` (4) | oui | non | oui | non | oui | 1,22 · 1,29 ; 1,28 · 1,18 | oui | 0,626 · 0,694 | 0,201 · 0,234 |
| `svc-org-tri0` (4) | non | non | non | non | non | 1,10 · 1,04 ; 1,12 · 1,00 | non | 1,124 · 1,164 | 0,349 · 0,373 |
| `svc-org-lc` (4) | oui | oui | non | non | non | 1,20 · 1,27 ; 1,32 · 1,20 | non | 0,523 · 0,587 | 0,116 · 0,117 |
| `svc-org-w002` (5) | oui | non | oui | non | oui | 1,26 · 1,26 ; 1,37 · 1,19 | oui | 0,594 · 0,643 | 0,081 · 0,085 |
| `svc-org-w005` (5) | oui | non | non | non | non | 1,27 · 1,31 ; 1,41 · 1,17 | non | 0,486 · 0,537 | 0,277 · 0,292 |
| `svc-org-pairs8` (5) | oui | oui | non | non | oui | 1,28 · 1,35 ; 1,39 · 1,31 | oui | 0,485 · 0,584 | 0,147 · 0,134 |
| `svc-org-wide` (5) | oui | non | non | non | non | 1,25 · 1,33 ; 1,34 · 1,15 | oui | 0,630 · 0,677 | 0,091 · 0,090 |
| `svc-org-noretry` (5) | oui | non | non | non | oui | 1,12 · 1,12 ; 1,22 · 1,07 | oui | 0,718 · 0,777 | 0,094 · 0,132 |
| `svc-org-w002-k15` (6) | oui | non | oui | **oui** | oui | 1,26 · 1,24 ; 1,39 · 1,19 | oui | 0,599 · 0,660 | 0,095 · 0,100 |
| `svc-org-w002-k30` (6) | oui | non | non | non | oui | 1,25 · 1,26 ; 1,27 · 1,16 | oui | 0,603 · 0,645 | 0,079 · 0,084 |
| `svc-org-w002-b150` (6) | oui | non | non | non | oui | 1,29 · 1,29 ; 1,31 · 1,10 | oui | 0,285 · 0,307 | 0,191 · 0,192 |
| `svc-org-w002-noretry` (6) | non | non | oui | **oui** | oui | **1,05 · 1,08 ; 1,12 · 1,03** | oui | 0,766 · 0,815 | 0,135 · 0,160 |

- **Aucun réglage ne tient C1 avec A1 et A2.** La règle de choix du §15.6 (la plus petite compacité ROAD parmi les réglages qui
  tiennent C1, C2 et B1) n'a donc **aucun candidat**. Les défauts restent ceux de la spécification (`w` 0,03, κ 2, 4 paires, bande
  [0,27 ; 0,39]) : aucun réglage n'est retenu, et la référence organique n'est pas re-bénie.
- **Sans C1**, la règle désigne `w` = 0,02 [calculé]. C2 et B1 tiennent pour `w` 0 et 0,02 ; ils manquent à 0,03 (C2) et à 0,05 (C2,
  B1). Compacité ROAD moyenne 0,083 à 0,02 contre 0,218 à 0 ; r / D à +0,027 · +0,028 de `w` 0,03, dans la marge de 0,03. κ reste
  à 2 : à 1,5, r / D dépasse cette marge (+0,032 · +0,045) ; à 3, aussi au curseur 0,25 (+0,036).
- **`pairs:8` n'est pas retenu** (décision 2) : il gagne 0,082 · 0,031 de W1 / IQR de r / D en ROAD et 0,038 · 0,042 en GRAVEL
  (0,048 en moyenne [calculé]), et manque C1 (× 1,28 · × 1,39).
- **`wide`** n'améliore pas r / D (0,630 · 0,677 contre 0,567 · 0,615) malgré deux fois plus de paires admissibles : non retenu.
- **`boost:1.5`** ramène la part GRAVEL à deux sommets dans la borne aux deux curseurs (0,480 et 0,349). Mais il pousse le ROAD
  au-dessus de 0,58 (0,646 et 0,656), car le boost multiplie `tri_q8` des deux types : A2 manque par le ROAD, et le boost n'est pas
  retenu.
- **Le temps vient des reprises et du couloir** : `noretry` gagne 0,17 sur le P50 au curseur 0,25 et 0,16 au curseur 1. Le couloir
  à `w` 0,02 au lieu de 0,03 ne gagne presque rien (0,01 à 0,03) ; sans couloir (`w000`), le gain est de 0,07 à 0,10.
- **`w002-k15`** est le réglage le plus proche de la bascule hors temps : A1, A3, A4, B1 et C2 tiennent. Il ne manque A2 que par la
  part GRAVEL au curseur 1 (0,276).

## 11. Séance en conteneur de 3 Gio (indicative)

**Faite à titre indicatif**, sans bascule : C3 n'est pas un critère jugé ici (§4, §15.19 point 18). Image `osm-route-details:lot4-4c`
construite sur `ea6ef63` (jamais poussée), défauts du service (`figures` `classic`, `learn_figures` `classic`), méthode du §14.13 :
`--memory=3g --memory-swap=3g`, fichiers France en lecture seule, volume neuf, `ORD_CACHE__OFFLINE=true`, échantillons de
`memory.current`, `memory.peak`, `anon` toutes les 0,5 s. Le cache de pages n'a pas été évincé avant les démarrages (`sudo`
indisponible sans mot de passe). Les figures sont choisies **par requête** (`figures: classic` puis `organic`), sur les mêmes
départs et le même volume : 18 dépôts (deux clés en parallèle), puis 54 générations en série et 162 à 4 clients pour chaque
figure, puis 162 à 4 clients en organique pendant un dépôt de A, enfin une 1.10 (`osm-route-details:lot4-3b`) sur le même volume.

| mesure [mesuré] | `classic` | `organic` | × `classic` [calculé] | seuil C3 (`organic`) |
|---|---|---|---:|---|
| 54 générations en série : P50 · P90 · max (s) | 0,24 · 0,50 · 0,75 | **0,28** · 0,76 · 1,17 | 1,17 · 1,52 | ≤ 0,29 s : tiendrait |
| 162 générations à 4 clients : P50 · P90 · max (s) | 0,49 · 0,75 · 0,92 | **0,66** · 1,33 · 1,91 | 1,35 · 1,77 | ≤ 0,56 s : **ne tiendrait pas** |
| 162 à 4 clients pendant un dépôt de A : P50 · P90 (s) | — | 0,84 · 1,52 | — | — |
| codes des réponses | 200 : 48 et 144 ; 422 : 6 et 18 | mêmes nombres | — | — |
| premiers parcours `triangle` | 0 | 10 sur 48 (série), 30 sur 144 (4 clients) | — | — |

- **Dépôts** : 18 sur 18 en 200, tous au premier essai ; le plus long en 519 s. Les profils sont `learned` en 3 pas pour 16 équipes
  et `empty` pour 2, puis les 18 états sont `ready` avec `profile.state = present`. Le dépôt de A pendant 4 clients de génération
  passe en 200 (288 s).
- **Mémoire** : pic `anon` **1,49 Gio** pendant les dépôts et 1,39 Gio pendant les générations (`memory.peak` 1,52 Gio). Le seuil était
  de 2,4 Gio. `oom` et `memory.events max` restent à 0, sans aucun redémarrage. Au 3b, sur la même séance, le pic `anon` était de
  1,47 Gio pendant les dépôts.
- **Compatibilité** : une 1.10 démarrée sur le même volume après les dépôts de la 1.11 garde les 18 profils (18 `.ordp.enc` avant et
  après ses purges, de démarrage et périodique). Elle rend les 18 états `ready` / `present` et sert les 54 générations en série
  (P50 0,24 s). La 1.11 redémarrée ensuite les retrouve `ready`.
- **Lecture** : en série, le surcoût organique (× 1,17) est proche de celui du banc et passerait sous le seuil de C3, à 0,01 s près.
  À 4 clients, il monte à × 1,35, et le P50 de 0,66 s **dépasse les 0,56 s** de C3. Comme au banc, le surcoût grandit avec la
  concurrence : les recherches des triangles prennent du processeur à chaque client, et 4 clients se partagent 4 cœurs
  (profils : 4 fils). Si la bascule avait été faite sur ces chiffres, C3 aurait demandé l'annulation du commit.

## 12. Réserves

- **Temps au banc** : 16 fils sur 8 cœurs physiques. Sur toutes les équipes, deux tirages de `svc` (campagnes 4 et 5) donnent 323 et
  320 ms au curseur 0,25, 273 et 268 ms au curseur 1 : 1 à 2 % d'écart, contre 10 % sur les essais à 20 boucles. Les écarts de
  C1 (+14 et +23 points au-dessus du seuil pour `svc-org`) le dépassent nettement.
- **A4 tient à une mesure presque nulle** : en GRAVEL, le `shared_share` réel a un P50 de 0,007, et son IQR est minuscule. Une W1 / IQR
  de +0,2 correspond à des écarts absolus de quelques millièmes de D. Le critère est appliqué tel quel ici.
- **C2 tient à deux jeux** (trois avec l'écart, sur 7 134) : ce sont les refus du contrôleur indépendant (§8). La divergence
  entre les deux contrôles existe déjà en `Classic`.
- **B1 en GRAVEL « écart »** est à +0,028 et +0,029 pour une marge de 0,03.
- **Lecture du premier parcours seulement** : les deux autres préfèrent une figure absente à note voisine (§15.7). Ils ne sont
  pas jugés ici.
- **La part `Triangle` de B** (0,17 à 0,18 en ROAD, contre 0,40 pour A) : son mélange d'équipe tire moins de triangles que le
  général, et il en garde moins. Le gain de r / D de B (× 0,77) est loin de celui de A (× 0,43 à × 0,46).

## 13. Ce que le relevé décide

- **Bascule du défaut : non.** Pour `svc+org` aux réglages par défaut, A1, A3 (ROAD), B1, B2 (réserve levée) et D tiennent ; **C1**
  (× 1,29 · × 1,38), **A2** (GRAVEL 1 à 0,317 pour une borne de 0,32), **A4** (`shared_share` GRAVEL à +0,21) et **C2** (2 jeux
  `VIOLATION` contre 0) manquent. Aucune variante jouée ne tient tous les critères. Le commit de bascule n'est pas fait, la séance
  C3 non plus ; le défaut reste `classic`, et `learn_figures` aussi.
- **Réglages** : aucun n'est retenu. `OrganicTune::default()` reste celui de la spécification, et la référence organique n'est pas
  re-bénie. `pairs:8` et `wide` ne sont pas retenus (décision 2).
- **Réserve du fait (d) : levée** (B2, −0,058 · −0,045). La table commune organique n'est pas proposée.
- **Règle L** : non tenue. Une bascule future devra apprendre en `organic`, au prix d'une nuit de redépôt.
- **Après le 4c : le 4d**, puisque A1 tient (décision 6 du §9.7 ; r / D × 0,46 à × 0,72 de `svc`). L'étape « lentille au ρ* »
  n'est pas nécessaire pour A1.
- **Ce qui reste au propriétaire** pour une bascule future :
  1. **C1 et C3** : la séance indicative montre que relâcher C1 ne suffit pas, car C3 manquerait aussi à 4 clients (0,66 s
     pour ≤ 0,56, §11). Deux voies : accepter ce temps, ce qui revient à changer le seuil de la décision 1 du §9.7, ou demander
     une étape de plus avant la bascule. Le levier mesuré est le refus `SHARP_TURN` du contrôle interne (87 à 91 % des refus de
     triangles, §9). Une recherche qui modéliserait la règle des grappes de 100 m garderait les triangles sans reprise : elle
     aurait le temps de `noretry` (C1 tient) avec le rendement des reprises (A1 et A2).
  2. **A4** : garder ou non `shared_share` dans A4 tant que sa valeur réelle est presque nulle (§12). `w002-k15` le tient.
  3. **C2** : un jeu `VIOLATION` vient d'une divergence entre le contrôle interne et le contrôleur indépendant, déjà présente en
     `Classic` (5 parcours refusés sous `common`). L'aligner touche au contrôleur ou au moteur, ce que le §15.17 exclut du 4c.
- **Prêt à fusionner** : la branche l'est en l'état, figures organiques en option. Le défaut `classic` rend 1.10 au bit près (I1 :
  références figées sans re-bénédiction, `base` et `svc` aux empreintes de la sonde). L'API 1.11 n'a que des ajouts. Le
  déterminisme est tenu. Dans la séance en conteneur, les dépôts passent, la mémoire tient et une 1.10 garde les profils de
  la 1.11 (§11). La fusion revient au propriétaire.

## 14. Étape « virages » : essai de fumée (19 novembre 2026)

Relevé du 19 novembre 2026, branche `feat/ord-loops-lot4c`, binaire `release` du dernier commit de code (`perf(loops)` du modèle
des virages, après `80f33c3`). Spécification : [`ord-loops-lot4.md`](ord-loops-lot4.md), §16 ; réponses du propriétaire : fin du
§9.8 ; écarts du codage : §16.13. Sorties **privées et hors dépôt** sous `~/.cache/osm-route-details/graph/lot4/smoke-4c-virages/`
(`virages-essai*`, `virages-marges*`, `avant-optimisation/`). Lettres d'équipes et agrégats seulement. **Seul l'essai de fumée du
§16.10 est joué** : les campagnes 2 et 3 ne le sont pas (voir « Arrêt » ci-dessous). Même machine, mêmes conventions de marques
qu'en tête de note ; 20 boucles par équipe, 44 requêtes ROAD et 34 GRAVEL, B et C réunies : **indicatif**.

### 14.1 Commande et contrôles

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph` :

```
$B likeness --teams B,C --limit 20 --holdout-share 0.25 \
  --variants base,svc,svc+org,svc+org+turns:0+reuse:0,svc+org+tri:0+w:0+turns:0+reuse:0,svc+org+reuse:0 \
  --label virages-essai --out $G/lot4/smoke-4c-virages
```

Contrôles d'empreinte [mesuré], tous tenus (le banc s'arrête sinon) : `base` `93149ec262198e9e` (= le contrôle du §16.10) ; **I2′**
`svc-org-turns0-reuse0` `27952d4befd451f4` = l'organique de `ea6ef63` (`svc-org` de l'essai de la relecture) ; **I2**
`svc-org-tri0-w000-turns0-reuse0` = `svc` `87e04a7ce10a564f`. `svc-org` (défauts de l'étape) : `50adca52ef713d18`, `svc-org-reuse0` :
`2b88ab9aaa71e707`. Le contrôleur indépendant ne refuse **aucun** parcours rendu, sous aucune variante (0 sur 1 329 parcours rendus, six variantes) : les
34 `SHARP_TURN` du §16.0 viennent de la campagne complète (toutes équipes, 0,25 de mise à l'écart) et ne se reproduisent pas sur
cet échantillon, avec ou sans l'alignement : l'effet de l'alignement se lit à la campagne 3.

### 14.2 Refus des triangles : ce que le modèle retire

Triangles complets (trois jambes, à ±10 % de D ou non), refus du contrôle interne, par requête organique jouée [mesuré] :

| variante · type | complets | à ±10 % | gardés | requêtes avec un triangle gardé | `SHARP_TURN` (part des complets) | autres refus | cherchées · reprises (par requête) |
|---|---:|---:|---:|---:|---|---|---|
| `svc-org-turns0-reuse0` ROAD | 136 | 116 | 32 | 0,477 | 75 (0,55) | `REUSE_OUTSIDE_TAIL` 13 | 3,95 · 0,70 |
| `svc-org` ROAD | 105 | 87 | 57 | 0,727 | **24 (0,23)** | aucun | 3,02 · 0,30 |
| `svc-org-reuse0` ROAD | 113 | 97 | 49 | 0,636 | 33 (0,29) | `EDGE_USED_MORE_THAN_TWICE` 11, `REUSE_OUTSIDE_TAIL` 11 | 3,27 · 0,39 |
| `svc-org-turns0-reuse0` GRAVEL | 93 | 83 | 34 | 0,559 | 47 (0,51) | aucun | 3,29 · 0,74 |
| `svc-org` GRAVEL | 76 | 59 | 31 | 0,529 | **27 (0,36)** | aucun | 2,65 · 0,50 |
| `svc-org-reuse0` GRAVEL | 73 | 57 | 30 | 0,529 | 26 (0,36) | aucun | 2,59 · 0,47 |

- `SHARP_TURN` par triangle complet : ROAD 0,55 → 0,23 (**−58 %**), GRAVEL 0,51 → 0,36 (**−30 %**), les deux types réunis 0,53 → 0,28
  (**−47 %**) [calculé]. `REUSE_OUTSIDE_TAIL` des triangles disparaît (13 → 0 en ROAD), et les
  reprises baissent de moitié environ. La part de requêtes avec un triangle gardé monte en ROAD (0,48 → 0,73) et reste en GRAVEL.
- Sans `no_reuse` (`svc-org-reuse0`), les `REUSE_OUTSIDE_TAIL` et `EDGE_USED_MORE_THAN_TWICE` reviennent en ROAD : l'interdiction
  de l'aller-retour fait ce que le §16.0 (point 4) annonçait.
- Compteurs par requête (moyenne, lentille et triangles, apprentissage compris) : ROAD `turn_cut` 105 000, `turn_evals` 810 000,
  `reuse_cut` 33 000 ; GRAVEL 160 000, 1 194 000, 50 000. `turn_evals` ne compte que les évaluations qui remontent le chemin ;
  la voie rapide (virage simple après une chaîne de plus de 100 m, ou deux chaînes de 50 m ou plus) est hors compteur.

### 14.3 Temps rapportés à `svc` (indicatif : bruit de 5 à 15 %)

| variante | P50 × `svc`, curseur 0,25 · 1 | P90 × `svc`, curseur 0,25 · 1 | `OK` | PANIC · WINDOW_ESCAPE · VIOLATION |
|---|---|---|---:|---|
| `svc-org-tri0-w000-turns0-reuse0` (= `svc`) | 1,07 · 1,12 | 1,00 · 1,06 | 90 % | 0 · 0 · 0 |
| `svc-org-turns0-reuse0` (l'organique d'avant) | 1,30 · 1,34 | 1,09 · 1,22 | 90 % | 0 · 0 · 0 |
| **`svc-org`** (défauts de l'étape) | **1,54 · 1,79** | 0,76 · 1,49 | 92,5 % | 0 · 0 · 0 |
| `svc-org-reuse0` (virages seuls) | 1,56 · 1,88 | 0,91 · 1,36 | 90-92,5 % | 0 · 0 · 0 |

- **C1 manque** : P50 × 1,54 et × 1,79 pour ≤ × 1,15, et pour la tolérance du propriétaire (réponse 4, ≤ × 1,25) : **pas de bascule**.
  Le modèle coûte **+18 à +34 %** sur l'organique d'avant (`svc-org` contre `svc-org-turns0-reuse0`) ; l'organique d'avant est déjà
  à × 1,30 · × 1,34 de `svc` dans ce tirage (× 1,29 · × 1,38 à la campagne 4c).
- **Un premier jeu du modèle donnait × 2,35 · × 2,73** (`avant-optimisation/`). Le profil (`perf`) y mettait `atan2`, `hypot`,
  `sincos` et `fmod` en tête : le commit `perf(loops)` fait le test d'angle sur des vecteurs (produit scalaire), lit les
  vecteurs unitaires dans une table, remonte une seule fois par fenêtre et traite le virage simple après une longue chaîne sans
  remonter le chemin. **Mêmes décisions** (empreintes de `svc-org` et de ses variantes inchangées sur B et C), le profil de C
  s'apprend en 20,5 s au lieu de 34,4 s. Dans le profil restant (C seule, `base` et `svc+org`, 20 boucles), le modèle pèse environ **un cinquième** du
  temps du processus (`TurnModel::eval` 12 %, `refuses_fwd`/`refuses_bwd` 6 %, `cos` de `Local::new` 3 %), à côté des deux
  arbres (`tree_forward` 8 %, `tree_backward` 7 %, qui contiennent les appels au modèle sur leur voie rapide) et de `turn_cost`
  (6 %). La décomposition par figure (couloir, jambes 2 et 3, arbres), que la réponse 4 demande en cas d'échec de C1, se fera
  sur la campagne 3, qui ne tourne pas ici.

### 14.4 Arrêt, et pourquoi le modèle laisse des refus

**Arrêt** (§16.10, point 1) : les refus `SHARP_TURN` par triangle complet ne **baissent pas de moitié** contre
`svc-org-turns0-reuse0` (−47 % réunis, −30 % en GRAVEL). Les campagnes 2 (déterminisme) et 3 (complète) ne sont pas lancées ; la
bascule du défaut (commit 5) non plus. Les commandes sont au §16.10 de la note du lot 4 (binaire reconstruit après le commit 4,
sorties sous `$G/lot4/4c-virages/`).

**Lecture des refus qui restent** (diagnostic **non commité**, à l'esprit de l'instrumentation du §16.0 : il évalue le modèle sur
les parcours des triangles refusés ; B et C, `svc+org`, apprentissage compris, 536 triangles refusés pour `SHARP_TURN`) [mesuré] :

- **453 (85 %) ne sont pas refusés par le modèle évalué sur leur route finale** : leur plus grand angle de grappe vaut **135° à
  145° pour 443 d'entre eux** (5 sous 125°, 1 de 125° à 135°, 4 à 145° ou plus, que seule une exemption du modèle laisse passer).
  Ce sont des virages que le contrôle lit à 150° ou plus et que le modèle lit 5° à 15° plus bas. L'effet est de sélection : la recherche
  évite les paires lues à 145° ou plus, et trouve celles que la géométrie approchée (chaînes courtes en corde, chaînes longues
  droites le long de leur cap de bout, giratoires et carrefours à plusieurs nœuds plus courbes que leur corde) sous-estime.
- **83 (15 %) sont refusés par le modèle sur la route finale** : la recherche les a laissés passer parce que le chemin de l'état
  était différent à l'époque (étiquettes de l'arbre qui dépendent du chemin, jonction de jambes, exemption de l'arrivée lue sur T2).
- Une marge plus large **retire** davantage (essai `virages-marges`, mêmes 44 · 34 requêtes) : `SHARP_TURN` par triangle complet,
  ROAD · GRAVEL : marge 5° (défaut) 24 sur 105 · 27 sur 76 ; **10°** 10 sur 86 · 27 sur 71 ; **15°** 11 sur 91 · 13 sur 57
  (`turn_margin_deg`, option `margin:<deg>` du banc), pour des requêtes avec un triangle gardé de 0,73 · 0,53, 0,71 · 0,50, 0,71 · 0,50 et un
  P50 × `svc` de 1,77 · 1,75 (5°), 1,76 · 1,64 (10°), 1,52 · 1,31 (15°) : un modèle qui refuse plus tôt perd moins de temps en reprises, mais
  le tirage est trop petit pour conclure. **La marge de 5° est celle du propriétaire (§9.8, réponse 1) ; elle n'est pas changée.**

### 14.5 Ce que le relevé décide

- **Rien ne change de défaut** : `generator.figures` et `learn_figures` restent `classic`. Le moteur `Classic` rend 1.10 au bit
  près (I1 : références figées sans re-bénédiction, `base` `93149ec262198e9e` à l'essai).
- **Ce qu'un tirage plus grand doit dire** (campagne 3, deux heures de poste) : la baisse de `SHARP_TURN` sur toutes les équipes, le rendement en triangles gardés (A2 GRAVEL), l'effet de l'alignement sur le contrôleur (34 → 0 attendu), le temps
  rapporté à `svc` (C1 ≤ × 1,25 par la réponse 4) et le déterminisme (D). Au vu de l'essai, **C1 ne tiendra probablement pas** : l'organique
  d'avant coûte déjà × 1,3 et le modèle y ajoute un cinquième ; le levier suivant est le couloir (§15.15, +10 à +12 % au 4c) ou
  la voie rapide du modèle (par exemple un refus d'avance quand les deux chaînes sont courtes), avant tout relâchement de seuil.
- **Propositions au propriétaire** : (1) lancer la campagne 3 malgré l'arrêt de l'essai pour lire les critères sur le tirage
  complet, ou la remettre à un réglage de marge (10° ou 15°) ; (2) un modèle plus fidèle sur les chaînes courtes courbes
  (arc entre les deux caps de bout `v.head`) coûterait un peu de temps pour réduire le 85 % ci-dessus.

### 14.6 Après la relecture : l'essai rejoué

La relecture indépendante corrige trois écarts du côté qui refuse davantage (§16.13 de la note, points 14 à 16 : aucune
exemption au départ dans T2, aucune exemption à l'arrivée dans la jambe 2, la jonction d'un chemin d'arbre rejouée sur 200 m).
L'essai est rejoué avec la même commande, sous le label `virages-essai-relecture` (même dossier, 6 min, pic de 15,6 Go) [mesuré] :

- **Empreintes toutes inchangées** : `base` `93149ec262198e9e`, `svc` `87e04a7ce10a564f`, `svc-org` `50adca52ef713d18`,
  `svc-org-reuse0` `2b88ab9aaa71e707`, I2′ `27952d4befd451f4`, I2 = `svc`. Les mêmes triangles sont refusés (`SHARP_TURN`
  24 sur 105 en ROAD, 27 sur 76 en GRAVEL) : sur ce tirage, les corrections ne changent que des étapes internes des
  recherches (`turn_cut` par requête : 105 404 contre 105 401 en ROAD). L'arrêt du §14.4 tient.
- Le contrôleur indépendant ne refuse toujours aucun parcours rendu.
- Temps, indicatif : `svc-org` à × 1,72 · × 1,54 de `svc` (P50, curseurs 0,25 · 1), contre × 1,54 · × 1,79 au premier
  essai : le bruit de l'échantillon est de cet ordre, et C1 reste loin du × 1,25 de la réponse 4.
