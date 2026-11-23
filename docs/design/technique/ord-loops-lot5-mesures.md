# Générateur de parcours — lot 5 (5a) : la part de non-revêtu du moteur 1.14 contre le réel, et la sonde part(μ)

> Relevé du 23 novembre 2026 sur la branche `feat/ord-lot5`. Conception : [`ord-loops-lot5.md`](ord-loops-lot5.md) (§2.2, §3,
> §5.2-5.3, §6, §9, §13 étape 2). Campagnes jouées avec le binaire `release` du banc (fonctionnalité `probe`), Rust 1.100.0,
> poste de développement (AMD Ryzen 7 4800H, 16 fils, 30 Go). Graphe et vues France `france-20261115-z11` (outil ord-graph
> 1.9.0, réplication 1794777748) ; export de production du 14 novembre (3 377 lignes, empreinte xxh3 `ca53159cc90498d6`).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/lot5/5a/` (essai et relevé complet) et
> `…/lot5/5a-mu/` (sonde). Lettres d'équipes et agrégats seulement : ni départ, ni coordonnée, ni identifiant d'équipe, de
> trace ou d'arête, ni nom de lieu. Rien n'a été poussé ni déployé. Le §9 ajoute la campagne de la consigne (5d, étape 7),
> sorties sous `…/lot5/5d/`, `…/lot5/5d-off/` et `…/lot5/5d-smoke/`.
>
> Marques : **[mesuré]** vient des rapports du banc (résumés `*-summary.md`, comparaisons `*-compare.md`) ; **[calculé]** d'un
> calcul fait pour ce relevé sur les rapports JSON par requête (variantes jointes sur l'identifiant de requête équipe × indice ×
> type), sur l'export ou sur l'extrait OSM, par des scripts jetables gardés hors dépôt. Quantiles par interpolation linéaire et
> W1 / IQR (IQR du réel de tout le type) comme le banc ; recalculés, ils redonnent les chiffres des résumés au millième. Temps du
> banc pris à 16 fils simultanés : comparables entre variantes d'une même campagne seulement.

## 0. En bref

- **Le moteur 1.14 sous-produit les chemins, mais moins qu'annoncé au §1 de la note** : dans la définition `offroad`
  (route-details recopiée sur le graphe), le réel GRAVEL est à **0,264 · 0,470 · 0,611** (P10 · P50 · P90) et le premier parcours
  `svc+org` à 0,409 (curseur 0,25) et 0,444 (curseur 1) en médiane, soit **−0,06 et −0,03** ; `base` (sans goût) est à −0,26.
  L'écart est porté par l'équipe A (−0,07 · −0,04) ; B est déjà au réel [§2].
- **Les deux définitions ne s'accordent pas (R2 déclenchée)** : `offroad − offroad_sc` vaut **+0,083 en médiane, P90 0,154**
  (seuils 0,03 et 0,08), positif sur 100 % des boucles. Cause trouvée : **ni `SURFACE_OTHER` ni la règle**, mais la source.
  12,6 % de la longueur des traces GRAVEL de l'export n'a **aucune classe de revêtement** dans route-details (typologie des
  tuiles absente sur ces arêtes) ; le graphe (PBF) leur donne une classe, non revêtue pour 80 % d'entre elles. Sur 539 boucles
  jointes, écart ≈ 0,80 × part sans classe − 0,012, corrélation 0,95 [§3].
- **Crans (R1)** : **0,25 · 0,47 · 0,65** en `offroad`. Une fois le parcours enregistré, une personne verra en médiane
  **8 points de moins** en « non revêtu » (P90 15 points), la différence s'affichant surtout en « inconnu » : environ 0,20 ·
  0,38 · 0,49 à la page du parcours, soit les crans du §2 du plan [§3.4].
- **La part n'est pas monotone en μ requête par requête (R3 → grille)** : 29 % des courbes du premier parcours décroissent à
  0,02 près (seuil 90 %), 51 % à 0,05 près. La médiane, elle, décroît régulièrement : 0,534 à μ = 0,25 → 0,234 à μ = 2
  (curseur 0,25) [§6.1, §6.2].
- **« Beaucoup » à 0,65 est hors de portée du moteur** : la part maximale sur la grille plafonne à 0,56 · 0,58 en médiane ;
  0,65 n'est atteint à ±0,05 que par 30 · 41 % des requêtes au premier parcours et 46 · 58 % au mieux des trois, toutes valeurs
  de μ confondues — sous les 70 % d'O1 même avec une recherche parfaite. « Peu » (0,25) : 56 · 46 % et 91 · 81 % ;
  « Comme d'habitude » (0,47) : 79 · 78 % et 95 · 95 % [§6.3].
- **Le temps d'O1 et de C1 ne tiendra pas tel qu'écrit** : une simulation de la recherche sur la sonde (prior, grille, ≤ 4 pas)
  ne règle au pas 0 que 12 à 39 % des requêtes selon le cran, et le nombre de pas médian est de 2 à 4 ; C1 demande ≥ 50 % et
  une médiane de 1 [§6.4].
- **R4 déclenchée** : la médiane réelle de B (0,310) est à −0,160 de la médiane générale ; A (0,519) à +0,049. La question d'un
  « Comme d'habitude » par équipe est posée au propriétaire [§7].
- **R5 : bornes inchangées** (`banned` réel médiane 0, P90 1,1 % ; avec exemption médiane 0, max 0,3 %). **R6 : le 5e est
  abandonné** (`run1k` généré 0,75 contre 0,65 réel, `switch_10km` 11,2 contre 14,5, au μ qui donne la médiane réelle).
  **R7 : pas de 5h en vue** (`run1k` généré au-dessus du réel à toute valeur de μ) [§4, §5, §7].

## 1. Écarts à la note

1. **La sonde ne rejoue pas le `svc+org` du relevé complet.** Avec `--ride-types gravel`, la mise à l'écart est tirée parmi les
   seules boucles GRAVEL : B apprend son profil sur 340 boucles au lieu de 297, A sur 500 dans les deux cas (plafond), et les
   profils diffèrent. `svc+org` de la sonde (`e590568823f7377a`) n'a donc pas l'empreinte du 4c, et seuls 18 · 14 jeux sur 780 y
   rendent exactement les parcours du relevé complet [calculé] ; `base`, qui n'apprend rien, rend les mêmes 780 · 780. Les
   agrégats restent proches (part médiane 0,416 · 0,456 contre 0,409 · 0,444). **Toute la sonde est lue contre son propre
   `svc+org`** (μ = 1 de la courbe) ; le relevé complet sert à M1 à M5. Le contrôle de référence est sans objet sous
   `--ride-types` (commit `c9335e7`), comme prévu.
2. **`mu:1` n'est pas dans la sonde complète** (ni dans la commande du §3.4) : I3 est vérifié sur l'essai court, où
   `svc+org+mu:1` rend `50adca52ef713d18`, l'empreinte de `svc+org` au bit près [§1.2].
3. **L'essai court a joué `svc+org+mu:1` et `svc+org+mu:0.5` en plus** de `base,svc+org` (§3.3), avec le binaire de `2aeac85`.
4. **M4, couverture « de la vue gravel dans les fenêtres jouées »** : le banc mesure `smooth_known` et `mtb_known` **le long des
   parcours** (réels et générés), pas sur la vue. La couverture de la vue n'est pas mesurée.
5. **M4, « la borne qui pèse le plus »** : `banned` est une part agrégée sur toutes les bornes, sans ventilation par borne. Sans
   objet ici (R5 ne se déclenche pas), mais impossible à dire à partir des rapports.
6. **M2 compare des mesures prises sur deux suites d'arêtes** : `offroad` sur les arêtes du graphe appariées (part appariée
   ≥ 0,95), `offroad_sc` sur la trace entière de l'export, pondérée par `trv`. L'écart d'appariement est borné par 5 % de la
   longueur et ne peut expliquer qu'une faible part d'un écart toujours positif de 4 à 15 points.
7. **La grille de μ de la sonde s'arrête à 2** (la note admet μ jusqu'à 4) : les μ médians des cibles basses (≤ 0,30) sont
   **censurés** (seules les requêtes qui croisent la cible avant μ = 2 entrent dans la médiane) et sous-estiment le μ utile.

## 1.1 Conditions

| campagne | commande (`$B likeness …`, `--out $G/lot5/<dossier>`) | binaire | durée | pic anonyme |
|---|---|---|---:|---:|
| essai | `--teams B,C --limit 20 --holdout-share 0.25 --variants base,svc+org,svc+org+mu:1,svc+org+mu:0.5 --label 5a-smoke` | `2aeac85` | 5 min 03 | 12,2 Go |
| complète | `--holdout-share 0.25 --variants base,svc+org --label 5a` | `fd6eada` | 12 min 40 | 16,1 Go |
| sonde | `--holdout-share 0.25 --ride-types gravel --label 5a-mu --variants base,svc+org,svc+org+mu:0.25,…:0.35,…:0.5,…:0.7,…:1.4,…:2` | `c9335e7` | 34 min 22 | 13,4 Go |

`fd6eada` et `c9335e7` ne changent que la documentation et le contrôle de référence du banc depuis `2aeac85`. Requêtes du relevé
complet : 2 855 boucles retenues (2 071 ROAD, 780 GRAVEL, 4 MTB ; A 2 406, B 397, C 27, D 11, E 9, H et I 2, F 1), deux curseurs
(0,25 et 1), surcouche complète, plus 712 jeux mis à l'écart. Sonde : les 780 boucles GRAVEL (A 545, B 228, C 5, H et I 1).

## 1.2 Garde-fous

| contrôle [mesuré] | attendu | obtenu | |
|---|---|---|---|
| Essai : `base` · `svc+org` | `93149ec262198e9e` · `50adca52ef713d18` (4d, §16.1 du relevé du 4c) | les mêmes | tient |
| I3 (essai) : `svc+org+mu:1` = `svc+org` | `50adca52ef713d18` | `50adca52ef713d18` | tient |
| Complète : `base` | `4686c904975302e7` (campagne 2, sonde 4c) | le même | tient |
| Complète : `svc+org` = campagne finale du 4c (§16.1) | `6324bdfdabf62f38` | le même | tient : les mesures ajoutées ne changent aucun parcours |
| Sonde : `base` | parcours de `base` du relevé complet | 780 · 780 jeux identiques [calculé] | tient |
| Sonde : refus du contrôleur indépendant, toutes variantes | 0 | 0 sur 5 013 à 5 587 parcours rendus par variante | tient |
| Sonde : PANIC, WINDOW_ESCAPE | 0 | 0 | tient |

## 2. M1 et M5 : la part `offroad`, réel contre premier parcours

Relevé complet, surcouche complète, premier parcours des jeux `OK` [calculé ; les lignes « toutes » sont celles du résumé].
Δ = médiane générée − médiane réelle ; Δ apparié = médiane par requête de (généré − réel).

| type · équipes | n | réel P10 · P25 · P50 · P75 · P90 | `svc+org` 0,25 : P10 · … · P90 | Δ · Δ apparié · W1/IQR | `svc+org` 1 : P10 · … · P90 | Δ · Δ apparié · W1/IQR |
|---|---:|---|---|---|---|---|
| GRAVEL · toutes | 780 | 0,264 · 0,347 · 0,470 · 0,550 · 0,611 | 0,240 · 0,331 · 0,409 · 0,482 · 0,524 | −0,061 · −0,063 · 0,27 | 0,235 · 0,354 · 0,444 · 0,506 · 0,559 | −0,027 · −0,037 · 0,15 |
| GRAVEL · A | 545 | 0,380 · 0,455 · 0,519 · 0,571 · 0,632 | 0,316 · 0,372 · 0,447 · 0,500 · 0,531 | −0,072 · −0,076 · 0,38 | 0,356 · 0,413 · 0,482 · 0,524 · 0,570 | −0,037 · −0,042 · 0,21 |
| GRAVEL · B | 228 | 0,188 · 0,246 · 0,310 · 0,361 · 0,420 | 0,124 · 0,215 · 0,332 · 0,394 · 0,428 | +0,021 · +0,001 · 0,14 | 0,151 · 0,223 · 0,277 · 0,399 · 0,438 | −0,033 · −0,001 · 0,12 |
| GRAVEL · autres | 7 | 0,331 · 0,348 · 0,379 · 0,441 · 0,485 | (5 jeux) médiane 0,352 | −0,028 · −0,063 · 0,28 | (5 jeux) médiane 0,470 | +0,090 · +0,063 · 0,32 |
| GRAVEL · toutes, `base` | 780 | idem | 0,081 · 0,139 · 0,212 · 0,272 · 0,350 | −0,258 · −0,236 · 1,16 | 0,106 · 0,175 · 0,244 · 0,299 · 0,355 | −0,227 · −0,221 · 1,03 |
| ROAD · toutes (M5) | 2 071 | 0,000 · 0,000 · 0,001 · 0,009 · 0,045 | 0,000 · 0,000 · 0,000 · 0,001 · 0,002 | −0,001 · 0,000 · (IQR 0,009) | 0,000 · 0,000 · 0,000 · 0,001 · 0,002 | −0,001 · 0,000 · — |
| ROAD · A | 1 860 | 0,000 · 0,000 · 0,001 · 0,007 · 0,028 | P90 0,002 | −0,001 | P90 0,002 | 0,000 |
| ROAD · B | 167 | 0,000 · 0,001 · 0,013 · 0,244 · 0,365 | P90 0,003 | −0,013 | P90 0,003 | −0,013 |
| ROAD · autres | 44 | 0,000 · 0,000 · 0,002 · 0,018 · 0,082 | P90 0,001 | −0,002 | P90 0,000 | −0,002 |
| MTB · autres (indicatif) | 4 | 0,361 · 0,380 · 0,475 · 0,594 · 0,657 | médiane 0,413 | −0,061 | médiane 0,276 | −0,199 |

- **Le goût fait déjà l'essentiel** : `base` est à 26 points sous le réel, `svc+org` à 3 à 6. Ce qui reste est l'équipe A, au
  goût le plus « chemins » (0,519 réel) ; B, dont la médiane réelle est à 0,310, est rendue à ±0,03.
- **M5, ROAD n'a rien à corriger** : le réel y est à 0,1 % médian, 4,5 % au P90 ; le généré à 0 % médian, 0,2 % au P90 (la vue
  route n'a presque pas de chemins). Le P75 · P90 de B en ROAD (0,244 · 0,365) est un fait d'étiquetage : une part des sorties
  de B déclarées ROAD empruntent des chemins. Aucune consigne n'est prévue en ROAD : rien à changer.
- `offroad` est rapporté à part, hors `S_GRAVEL` : les `S / S_base` du relevé sont ceux du 4c (contrôle `6324bdfdabf62f38`).

## 3. M2 et R2 : accord des définitions

### 3.1 Les chiffres

Sur les 780 boucles GRAVEL réelles [calculé] :

| mesure | P10 | P25 | P50 | P75 | P90 |
|---|---:|---:|---:|---:|---:|
| `offroad − offroad_sc`, par boucle | 0,041 | 0,059 | **0,083** | 0,117 | 0,154 |
| \|`offroad − offroad_sc`\| | — | — | 0,083 | — | **0,154** |

Écart moyen 0,090 ; écart > 0,03 sur 95 % des boucles, négatif sur aucune. En ROAD l'écart est nul (P90 0,010).

Déciles, D1 à D9 [calculé] :

| | D1 | D2 | D3 | D4 | D5 | D6 | D7 | D8 | D9 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `offroad_sc` (export, `sc == "unpaved"`) | 0,197 | 0,255 | 0,301 | 0,339 | **0,380** | 0,401 | 0,426 | 0,460 | 0,491 |
| `offroad` (graphe, `surface_rd`) | 0,264 | 0,330 | 0,373 | 0,426 | **0,470** | 0,506 | 0,535 | 0,565 | 0,611 |

`offroad_sc` redonne les 20 / 38 / 49 % de `shapes.py` (§2 du plan) au centième : **la lecture de `sc` par le banc est la
bonne**, c'est la définition recopiée sur le graphe qui lit autre chose. **R2 déclenchée** (0,083 > 0,03 et 0,154 > 0,08).

### 3.2 La cause : la typologie absente de l'export, pas `SURFACE_OTHER`

**`SURFACE_OTHER` est négligeable** [calculé, extrait OSM France du 15 novembre, `osmium tags-count` sur les voies `highway` des
classes routières, cyclables et de chemins] : 4 833 voies sur 3 852 210 portant `surface` ont une valeur hors des 30 codes du
graphe (**0,13 %**), 2 663 sur 1 324 478 parmi `track`, `path`, `footway`, `pedestrian`, `bridleway` (**0,20 %**). En tête :
`chipseal` (1 188), `stone` (1 122), `stepping_stones` (314), `metal_grid`, `tiles` (265 chacune), `dirt;sand` (230),
`linoleum`, `artificial_turf`, `dirt/sand`, `rocks`. Valhalla en reconnaîtrait une partie par sous-chaîne (`chipseal` et
`stone` revêtus, `stepping_stones` et `artificial_turf` non revêtus, `dirt;sand` non revêtu), que `surface_rd` passe à
`tracktype` puis à la classe. Compte en voies, pas en longueur ; même à dix fois la longueur moyenne, l'effet reste sous le
point de pourcentage, loin des 8 points mesurés.

**La source l'explique** [calculé sur l'export, 913 traces GRAVEL, 44 934 km, pondération `trv`] : la répartition de la longueur
selon `sc` et sa source `ss` est

| `sc` · `ss` | part de la longueur |
|---|---:|
| revêtu · `tagged` | 0,367 |
| non revêtu · `tagged` | 0,191 |
| non revêtu · `inferred_highway` | 0,162 |
| revêtu · `inferred_highway` | 0,143 |
| **absent · absent** | **0,126** |
| non revêtu · `inferred_tracktype` | 0,009 |
| revêtu · `inferred_tracktype` | 0,003 |

12,6 % de la longueur n'a **aucune classe** : 8,8 points sur des arêtes sans genre de voie (`wk` absent), le reste sur des
`path` (13 % de leur longueur), `track`, `cycleway` et `footway` sans classe. Ce sont des arêtes du graphe de route-details
pour lesquelles la typologie des tuiles (VersaTiles shortbread, z14, `"indicative": true` dans le document) n'a rien rendu ;
le PBF du moteur leur donne une classe. L'export est en api 1.8 sur toutes les lignes : ce n'est pas un mélange de versions.

**Jointure par boucle** [calculé] : la part `sc == "unpaved"` recalculée sur l'export identifie de façon unique **539** des 780
boucles du rapport (égalité à 10⁻⁹ ; les autres ont une valeur ambiguë ou une pondération qui diffère au dernier chiffre). Sur
elles :

- écart `offroad − offroad_sc` P10 · P50 · P90 : 0,038 · 0,081 · 0,152 (la même distribution que sur les 780) ;
- part de longueur sans classe P10 · P50 · P90 : 0,066 · 0,116 · 0,202 ; **corrélation avec l'écart 0,95** ;
- régression : écart ≈ **0,80 × part sans classe − 0,012** : 80 % de la longueur sans classe est non revêtue au sens du graphe ;
- en comptant toute la longueur sans classe comme non revêtue, l'écart devient négatif (P50 −0,035) : le résidu se partage
  entre sans-classe revêtu et quelques chemins dont les tuiles disent revêtu (`path` · `asphalt` 2,1 % de la longueur) ;
- par équipe : A écart médian 0,089 pour 0,124 sans classe ; B 0,063 pour 0,095.

La règle `surface_rd` n'est donc pas en cause : à typologie égale elle redonne `sc`. Le correctif n'est pas dans le moteur
(5b) mais dans route-details (couverture de la typologie des tuiles, ou repli sur la classe de l'arête), hors de ce lot.

### 3.3 Ce que R2 décide

Les crans restent calés sur `offroad` (R1), la définition que le moteur sait viser. La cause est connue avant le 5d ; rien à
changer au 5b (le test d'accord avec ord-core reste le bon garde-fou de la règle).

### 3.4 L'écart qu'une personne verra

Un parcours généré n'a pas d'`offroad_sc` au banc (il n'est pas passé par route-details) : l'écart n'est mesurable que sur les
boucles réelles, enregistrées puis classées par route-details, qui sont la meilleure image d'un parcours proposé puis
enregistré. Pour une part proposée X, la page du parcours enregistré montrera en « non revêtu » environ **X − 0,08** (médiane),
X − 0,15 au P90 ; la différence apparaît surtout en « inconnu », que la répartition du site affiche (`TrackSurface.UNKNOWN`).
Aux crans de R1 : **0,25 → ≈ 0,20 ; 0,47 → ≈ 0,38 ; 0,65 → ≈ 0,50** (lecture par déciles : D1 0,264 → 0,197, D5 0,470 → 0,380, D9 0,611 → 0,491). La carte
du générateur (`GeneratedRouteCard`) montrerait `offroad_share` ; le parcours enregistré, moins : c'est le point 5 à trancher au
§8.

## 4. M3 : tronçons

Relevé complet, GRAVEL, premier parcours [mesuré] :

| mesure | réel P10 · P25 · P50 · P75 · P90 | `svc+org` 0,25 | `svc+org` 1 | `base` 0,25 · 1 (P50) |
|---|---|---|---|---|
| `run_p50_m` | 843 · 1 086 · 1 291 · 1 653 · 1 862 | 1 047 · 1 176 · 1 499 · 1 782 · 2 279 | 1 052 · 1 127 · 1 534 · 1 819 · 2 634 | 1 374 · 1 338 |
| `run1k` | 0,405 · 0,548 · **0,646** · 0,734 · 0,791 | 0,516 · 0,627 · **0,703** · 0,783 · 0,843 | 0,539 · 0,617 · **0,698** · 0,773 · 0,838 | 0,641 · 0,628 |
| `switch_10km` | 7,7 · 11,3 · **14,5** · 17,2 · 19,1 | 6,3 · 8,8 · **10,4** · 12,6 · 14,2 | 6,7 · 9,0 · **11,3** · 14,1 · 15,5 | 6,0 · 6,9 |

**Le moteur enchaîne déjà plus que le réel** : tronçons plus longs (+160 à +180 m en médiane), plus de non-revêtu en tronçons
d'1 km (+0,05 à +0,06), moins de passages (−3 à −4 par 10 km). Le réel alterne davantage, ce qu'aucun coût de passage ne
rapprocherait.

Le long de la sonde, au premier parcours, médianes [calculé] :

| μ | 0,25 | 0,35 | 0,5 | 0,7 | 1 | 1,4 | 2 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `run1k`, curseur 0,25 · 1 | 0,751 · 0,731 | 0,752 · 0,742 | 0,755 · 0,741 | 0,753 · 0,734 | 0,705 · 0,708 | 0,656 · 0,661 | 0,607 · 0,599 |
| `switch_10km`, curseur 0,25 · 1 | 12,6 · 13,3 | 12,4 · 12,9 | 11,8 · 12,7 | 11,2 · 12,2 | 10,7 · 11,3 | 9,2 · 10,2 | 7,6 · 8,5 |
| `run_p50_m`, curseur 0,25 · 1 | 1 660 · 1 628 | 1 661 · 1 548 | 1 653 · 1 550 | 1 635 · 1 576 | 1 494 · 1 537 | 1 381 · 1 408 | 1 253 · 1 280 |

## 5. M4 : bornes et couverture

GRAVEL, relevé complet [mesuré, agrégat pondéré calculé] :

| mesure | réel P10 · P25 · P50 · P75 · P90 | `svc+org` 0,25 · 1, P90 | autre |
|---|---|---|---|
| `banned` (sans exemption) | 0 · 0 · **0** · 0,003 · **0,011** | 0,008 · 0,006 | réel : 0,32 % de la longueur totale, 11 % des boucles au-dessus de 1 %, max 9,6 % |
| `banned_team` (exemption d'équipe) | 0 · 0 · **0** · 0 · 0 | 0 · 0,001 | réel : 0,00 % de la longueur, max 0,3 % |
| `smooth_known` (le long des parcours) | 0,022 · 0,049 · 0,151 · 0,222 · 0,281 | 0,316 · 0,338 | |
| `mtb_known` (le long des parcours) | 0 · 0 · 0 · 0,002 · 0,019 | 0,003 · 0,006 | |

Dans la sonde, `banned` P90 du premier parcours monte à 0,012–0,015 pour μ ≤ 0,7 (0,008 · 0,009 à μ = 1) : plus de chemins,
plus d'arêtes que les bornes écarteront. `mtb:scale` n'est presque jamais renseigné le long des boucles (0 % médian, 1,9 % au
P90) : la borne `mtb:scale ≥ 2` aura très peu à mordre (risque du §12, confirmé).

## 6. La sonde part(μ) (§3.4)

Sonde complète, GRAVEL, 780 requêtes, surcouche complète ; lue contre son propre `svc+org` (§1, point 1).

### 6.1 La courbe

Premier parcours, P10 · P25 · P50 · P75 · P90 ; « meilleur des trois » = max et min des trois propositions, médianes
[calculé] :

| μ | curseur 0,25 : premier | max · min des trois, P50 | curseur 1 : premier | max · min, P50 |
|---:|---|---|---|---|
| 0,25 | 0,378 · 0,464 · **0,534** · 0,590 · 0,631 | 0,582 · 0,487 | 0,379 · 0,462 · **0,554** · 0,612 · 0,648 | 0,605 · 0,510 |
| 0,35 | 0,364 · 0,453 · **0,528** · 0,579 · 0,620 | 0,568 · 0,467 | 0,355 · 0,460 · **0,538** · 0,598 · 0,639 | 0,591 · 0,497 |
| 0,5 | 0,344 · 0,446 · **0,513** · 0,558 · 0,606 | 0,553 · 0,453 | 0,344 · 0,444 · **0,523** · 0,578 · 0,625 | 0,571 · 0,484 |
| 0,7 | 0,311 · 0,396 · **0,473** · 0,530 · 0,569 | 0,531 · 0,388 | 0,322 · 0,425 · **0,500** · 0,551 · 0,595 | 0,546 · 0,441 |
| 1 | 0,247 · 0,336 · **0,416** · 0,480 · 0,525 | 0,478 · 0,323 | 0,234 · 0,355 · **0,456** · 0,511 · 0,551 | 0,504 · 0,379 |
| 1,4 | 0,147 · 0,236 · **0,328** · 0,404 · 0,463 | 0,413 · 0,260 | 0,132 · 0,263 · **0,375** · 0,451 · 0,486 | 0,457 · 0,309 |
| 2 | 0,073 · 0,135 · **0,234** · 0,311 · 0,365 | 0,309 · 0,157 | 0,058 · 0,174 · **0,267** · 0,342 · 0,392 | 0,349 · 0,213 |

- **Asymétrie** : rendre le non-revêtu plus cher porte loin (−0,18 · −0,19 de μ = 1 à 2), le rendre moins cher plafonne
  (+0,12 · +0,10 de 1 à 0,25, dont +0,06 · +0,04 entre 0,7 et 0,25). La part maximale sur la grille, par requête, vaut P10 ·
  P50 · P90 **0,426 · 0,560 · 0,641** (curseur 0,25) et 0,411 · 0,580 · 0,657 (curseur 1) : le réseau autour du départ borne
  ce que μ peut faire.
- Amplitude par requête, part(0,25) − part(2) : P10 · P50 · P90 0,150 · 0,299 · 0,425 (curseur 0,25), 0,147 · 0,288 · 0,408.

### 6.2 Monotonie

Courbes complètes (les sept jeux `OK`) : 767 · 758 requêtes. Part des courbes où la part du premier parcours ne remonte jamais
de plus de 0,02 quand μ croît (toutes paires de points) [calculé] :

| lecture | curseur 0,25 | curseur 1 |
|---|---:|---:|
| premier parcours, à 0,02 près, toutes paires | **0,29** | **0,29** |
| idem, points consécutifs seulement | 0,30 | 0,29 |
| idem, à 0,05 près | 0,51 | 0,52 |
| idem, sous-grille {0,35 … 1,4} de R3, à 0,02 près | 0,45 | 0,40 |
| meilleur des trois (cible : la part réelle de la boucle), à 0,02 près | 0,26 | 0,28 |
| plus forte remontée par requête, P25 · P50 · P75 · P90 | 0,014 · 0,048 · 0,110 · 0,170 | 0,014 · 0,047 · 0,106 · 0,165 |

Un petit changement de μ bascule souvent la figure ou le point lointain (risque du §12) : la remontée médiane, 0,05, est de
l'ordre de la tolérance.

### 6.3 Cibles : le μ médian et la part qui l'atteint

Pour chaque cible t : nombre de requêtes dont la courbe du premier parcours croise t sur la grille, et μ médian du croisement
(interpolé en log₂ μ entre les deux points qui encadrent t) ; part des requêtes qui atteignent t à ±0,05 **quelque part sur la
grille**, au premier parcours puis au meilleur des trois (borne haute de ce que la recherche peut faire, mise en commun
parfaite comprise) ; le point de grille le plus proche du μ médian, et la part atteinte **à ce seul point** (premier ;
meilleur des trois). Curseur 0,25 · curseur 1 [calculé] :

| t | croisements | μ médian | atteint, premier | atteint, meilleur des 3 | point de grille | à ce point, premier | à ce point, meilleur des 3 |
|---:|---|---|---|---|---|---|---|
| 0,10 | 160 · 138 | 1,69 · 1,55 (censuré) | 0,30 · 0,22 | 0,49 · 0,33 | 2 · 1,4 | 0,26 · 0,11 | 0,47 · 0,21 |
| 0,15 | 240 · 182 | 1,56 · 1,35 (censuré) | 0,33 · 0,26 | 0,61 · 0,44 | 1,4 · 1,4 | 0,12 · 0,11 | 0,26 · 0,26 |
| 0,20 | 331 · 247 | 1,45 · 1,32 (censuré) | 0,41 · 0,37 | 0,78 · 0,67 | 1,4 · 1,4 | 0,18 · 0,12 | 0,36 · 0,28 |
| **0,25** | 441 · 350 | 1,38 · 1,32 (censuré) | **0,56 · 0,46** | **0,91 · 0,81** | 1,4 · 1,4 | 0,23 · 0,12 | 0,51 · 0,31 |
| 0,30 | 579 · 453 | 1,32 · 1,36 | 0,67 · 0,63 | 0,97 · 0,92 | 1,4 · 1,4 | 0,28 · 0,20 | 0,63 · 0,45 |
| 0,35 | 676 · 612 | 1,18 · 1,32 | 0,76 · 0,76 | 0,97 · 0,98 | 1 · 1,4 | 0,26 · 0,28 | 0,58 · 0,57 |
| 0,40 | 706 · 676 | 0,99 · 1,21 | 0,79 · 0,76 | 0,98 · 0,97 | 1 · 1,4 | 0,34 · 0,32 | 0,67 · 0,66 |
| 0,45 | 675 · 667 | 0,80 · 0,98 | 0,80 · 0,78 | 0,96 · 0,97 | 0,7 · 1 | 0,36 · 0,34 | 0,66 · 0,72 |
| **0,47** | 651 · 652 | 0,65 · 0,86 | **0,79 · 0,78** | **0,95 · 0,95** | 0,7 · 1 | 0,38 · 0,39 | 0,69 · 0,73 |
| 0,50 | 611 · 621 | 0,53 · 0,67 | 0,79 · 0,78 | 0,94 · 0,94 | 0,5 · 0,7 | 0,45 · 0,43 | 0,75 · 0,77 |
| 0,55 | 435 · 478 | 0,42 · 0,50 | 0,75 · 0,77 | 0,91 · 0,89 | 0,5 · 0,5 | 0,45 · 0,46 | 0,76 · 0,75 |
| 0,60 | 232 · 314 | 0,36 · 0,35 | 0,55 · 0,60 | 0,76 · 0,76 | 0,35 · 0,35 | 0,37 · 0,39 | 0,64 · 0,64 |
| **0,65** | 54 · 107 | 0,29 · 0,32 | **0,30 · 0,41** | **0,46 · 0,58** | 0,25 · 0,35 | 0,22 · 0,24 | 0,40 · 0,44 |
| 0,70 | 1 · 8 | — | 0,07 · 0,14 | 0,15 · 0,27 | 0,25 · 0,25 | 0,05 · 0,09 | 0,11 · 0,20 |

(0,47, 0,65 et 0,70 ajoutés à la grille de cibles du §3.4 pour lire les crans de R1.) Par équipe, à ±0,05, quelque part sur
la grille, premier · meilleur des trois, curseur 0,25 / 1 : « Comme d'habitude » 0,47 : A 0,84 · 1,00 / 0,86 · 1,00, **B 0,64
· 0,82 / 0,54 · 0,81** ; « Beaucoup » 0,65 : A 0,42 · 0,64 / 0,56 · 0,81, **B 0,01 · 0,02 / 0,02 · 0,02** ; « Peu » 0,25 :
A 0,55 · 0,91 / 0,39 · 0,76, B 0,58 · 0,88 / 0,60 · 0,91.

### 6.4 Ce que la recherche peut espérer (simulation)

Simulation sur les jeux de la sonde [calculé, optimiste] : pas 0 au point de grille le plus proche du μ médian du §6.3 ; à chaque
pas, si le premier parcours est hors tolérance, point voisin de la grille dans le sens de l'écart (part trop basse → μ plus
petit) ; au plus 4 pas ; candidats mis en commun et choix du plus proche de la cible (le choix réel pèse aussi la forme, et ne
prendra pas toujours celui-là). Grille de R3 {0,35 ; 0,5 ; 0,7 ; 1 ; 1,4} puis grille de la sonde {0,25 … 2}. Curseur 0,25 ·
curseur 1 :

| cran | grille | dans la tolérance après recherche | réglé au pas 0 | pas P50 · P90 |
|---|---|---|---|---|
| Peu 0,25 | R3 | 0,68 · 0,44 | 0,23 · 0,12 | 4 · 4 |
| Peu 0,25 | sonde | 0,90 · 0,80 | 0,23 · 0,12 | 3 à 4 · 4 |
| Comme d'habitude 0,47 | R3 ou sonde | 0,93 · 0,93 | 0,38 · 0,39 | 2 · 4 |
| Beaucoup 0,65 | R3 | 0,40 · 0,49 | 0,17 · 0,24 | 4 · 4 |
| Beaucoup 0,65 | sonde | 0,46 · 0,58 | 0,22 · 0,24 | 4 · 4 |

« Réglé au pas 0 » compte le premier parcours ; si le terme de choix du §4.5 fait remonter en tête le meilleur des trois, la
part réglée au pas 0 monte au plus à la dernière colonne du §6.3 (0,51 · 0,31 ; 0,69 · 0,73 ; 0,40 · 0,44). **Dans les deux
lectures, C1 (≥ 50 % au pas 0, pas médian 1, P50 ≤ × 1,50) ne tient pas**, sauf peut-être au cran du milieu ; O1 « Beaucoup »
manque même avec quatre pas.

### 6.5 Temps par variante

Jeux sur la surcouche complète, ms par jeu à 16 fils, contre `svc+org` de la même séance [calculé ; mesuré au §7 de la
comparaison] :

| μ | curseur 0,25 : P50 · P90 (ms) | × `svc+org` P50 · P90 | curseur 1 : P50 · P90 | × P50 · P90 | `OK` 0,25 · 1 |
|---:|---|---|---|---|---|
| 0,25 | 611 · 1 344 | 1,09 · 1,19 | 590 · 1 340 | 1,06 · 1,17 | 771 · 766 |
| 0,35 | 592 · 1 310 | 1,06 · 1,16 | 584 · 1 270 | 1,05 · 1,11 | 771 · 761 |
| 0,5 | 575 · 1 266 | 1,02 · 1,12 | 569 · 1 139 | 1,02 · 0,99 | 769 · 766 |
| 0,7 | 576 · 1 176 | 1,03 · 1,04 | 573 · 1 192 | 1,03 · 1,04 | 772 · 769 |
| 1 (`svc+org`) | 561 · 1 133 | 1 · 1 | 555 · 1 146 | 1 · 1 | 772 · 770 |
| 1,4 | 599 · 1 137 | 1,07 · 1,00 | 583 · 1 239 | 1,05 · 1,08 | 772 · 772 |
| 2 | 587 · 1 153 | 1,05 · 1,02 | 565 · 1 159 | 1,02 · 1,01 | 772 · 772 |

Un pas coûte une génération entière, à ±10 % : deux pas médians doubleraient le P50. μ bas coûte un peu plus et perd quelques
`OK` (jusqu'à 11 `NO_CANDIDATE` de plus au curseur 1 à μ = 0,35, un `NO_ROUTE:BUDGET`).

## 7. Application des règles du §3.5

| règle | condition | mesuré | verdict |
|---|---|---|---|
| **R1**, crans | Peu = P10 réel `offroad` arrondi à 0,05 ; habitude = P50 à 0,01 ; Beaucoup = P90 + 0,05 à 0,05 ; 0,20 · 0,38 · 0,55 si M2 confirme 20 / 38 / 49 % à ±0,03 | P10 0,264 · P50 0,470 · P90 0,611 ; M2 ne confirme pas (`offroad` à 0,264 · 0,470 · 0,611 contre 0,20 · 0,38 · 0,49) | **0,25 · 0,47 · 0,65** |
| **R2**, définitions | écart médian ≤ 0,03 et P90 absolu ≤ 0,08 | 0,083 et 0,154 | **déclenchée** ; cause : typologie absente de l'export (12,6 % de la longueur sans classe, corrélation 0,95), `SURFACE_OTHER` négligeable (0,13 % des voies) ; écart vu : −0,08 médian, −0,15 au P90 ; crans gardés sur `offroad` |
| **R3**, recherche | ≥ 90 % de courbes monotones → sécante | 29 % · 29 % (51 · 52 % à 0,05 près) | **grille** {0,35 ; 0,5 ; 0,7 ; 1 ; 1,4}, meilleur gardé, sans hypothèse d'ordre |
| **R4**, habitude par équipe | une équipe de > 30 boucles gravel à plus de 0,08 de la médiane | A 0,519 (+0,049) ; **B 0,310 (−0,160)** | **déclenchée** : question au propriétaire pour un lot suivant |
| **R5**, bornes | `banned` réel ≤ 1 % médian et ≤ 5 % P90 ; avec exemption < 0,2 % médian | 0 % et 1,1 % ; avec exemption 0 % (max 0,3 %) | **non déclenchée** : bornes du §6.1 inchangées |
| **R6**, coût de passage | `run1k` généré à moins de 0,10 sous le réel et `switch_10km` < 1,5 × le réel, au μ qui donne la médiane réelle | μ = 0,7 (curseur 0,25, part 0,473) : `run1k` 0,753 contre 0,646 (au-dessus), `switch_10km` 11,2 contre 14,5 (× 0,77) ; curseur 1 entre μ = 0,7 et 1 (0,500 · 0,456) : 0,734 · 0,708 et 12,2 · 11,3 | **5e abandonné** (E1 et E2 sans objet) |
| **R7**, chaînes attractives | après le 5e : « Beaucoup » manque O1 **et** `run1k` généré à plus de 0,15 sous le réel | `run1k` généré ≥ 0,60 à toute valeur de μ (réel 0,646) ; O1 « Beaucoup » à lire à l'étape 7 | **5h non proposé** : la seconde condition ne peut pas tenir ; si « Beaucoup » manque O1, ce sera par plafond de part (§6.1), pas par saupoudrage |

## 8. Ce que cela change à la suite, et ce qui demande le propriétaire

- **5b** : inchangé. La règle est bonne ; l'écart est dans route-details (hors lot). Le test d'accord reste le garde-fou.
- **5c** : bornes du §6.1 telles quelles ; la borne `mtb:scale` aura très peu à mordre.
- **5d** : recherche sur la grille de R3, prior lu au §6.3 (point de grille par cible), mise en commun des candidats. La
  simulation (§6.4) dit déjà que **C1 et O1 « Beaucoup » ne tiendront pas** tels qu'écrits ; le terme de choix (`score_weight`)
  devient le premier levier, avant le nombre de pas.
- **5e** : abandonné (R6). **5h** : non proposé (R7).
- **Décisions du propriétaire** :
  1. **« Beaucoup » à 0,65** (R1) est hors de portée : borne haute 0,46 · 0,58 contre 70 % pour O1, et 0,02 pour B. Garder la
     règle et accepter qu'O1 manque à ce cran, ou plafonner le cran (0,60 : borne haute 0,76 · 0,76 ; 0,55 : 0,91 · 0,89).
  2. **Grille de R3** : sa borne {0,35 … 1,4} coupe « Peu » (borne haute 0,68 · 0,44 contre 0,90 · 0,80 sur {0,25 … 2}) et
     « Beaucoup » ; l'élargir à 0,25 et 2 (ou au-delà, μ ≤ 4) est un écart à la règle écrite.
  3. **C1** (≥ 50 % au pas 0, P50 ≤ × 1,50) : le bruit par requête (remontée médiane 0,05) est de l'ordre de la tolérance ;
     garder C1 et accepter un échec probable, relever le seuil de temps, ou élargir la tolérance.
  4. **R4** : un « Comme d'habitude » propre à chaque équipe (B à 0,31, appris au dépôt, changement de format du profil).
  5. **R2** : la page du parcours enregistré montrera ≈ 8 points de moins que la carte du générateur. Corriger la couverture de
     la typologie dans route-details (hors lot), ou afficher sur la carte du générateur la part attendue après enregistrement.

## 9. 5d — la consigne au banc (étape 7, §13 de la note)

Relevé du 23 novembre 2026, sur les **crans et le C1 décidés par le propriétaire** (encadré en tête de la note : 0,25 · 0,47 ·
0,60 ; médiane avec cible ≤ × 3,0 de la même requête sans cible, P90 ≤ × 3,5). Même poste, même graphe, même export que
le 5a. **Lettres et agrégats seulement.** Marques comme plus haut : **[mesuré]** vient du `5d-compare.md` (section 11 « 5d »,
sections 3 à 7 et 10) ; **[calculé]** d'un script jetable hors dépôt sur les rapports JSON par requête (W1 / IQR et `S`
recalculés redonnent ceux du banc au millième).

### 9.1 Conditions

| campagne | commande (`$B likeness …`, `--out $G/lot5/<dossier>`) | binaire | durée | pic anonyme |
|---|---|---|---:|---:|
| essai | équipes B et C, 17 boucles GRAVEL, mêmes variantes, `--label 5d-smoke` | `f5ee718` | ≈ 6 min | — |
| complète | `--holdout-share 0.25 --ride-types gravel --variants base,svc+org,svc+org+ride:on,svc+org+ride:on+target:0.25,…:0.47,…:0.60 --label 5d` (reconstituée d'après les noms de variantes du rapport) | `f5ee718` | 27 min 47 | 13,5 Go |
| sans bornes (ajoutée pour ce relevé) | `--holdout-share 0.25 --ride-types gravel --variants base,svc+org --label 5d-off` | `f5ee718` | 6 min 02 | 12,9 Go |

Les 780 boucles GRAVEL de la sonde (A 545, B 228, C 5, H et I 1), deux curseurs (0,25 et 1), lectures « toutes » (surcouche
complète) et « écart » (194 boucles mises à l'écart). ROAD n'est pas joué (`--ride-types gravel`). Temps à 16 fils
simultanés, comparables entre variantes d'une même campagne seulement.

**Écart à la note : la campagne complète ne mesure pas l'effet des bornes.** `ride:on` pose les bornes sur le moteur de toute
l'exécution (§13, étape 7) : dans `5d`, `base` et `svc+org` sont déjà bornés, et `svc+org` rend l'empreinte de
`svc+org+ride:on` au bit près (`211044881d80a2e9`). D'où la troisième campagne, sans `ride:on`, au même commit. Elle sert aussi
de **référence 1.14** des critères A et B (le §9 dit « 1.14 (`svc+org`) dans la même séance ») ; la référence des critères O,
E et C reste `svc+org+ride:on`, celle du banc.

Garde-fous [mesuré] :

| contrôle | attendu | obtenu | |
|---|---|---|---|
| `5d-off` : `base` · `svc+org` = sonde du 5a (`c9335e7`) | `c3d03271f978f182` · `e590568823f7377a` | les mêmes | tient : les étapes 3 à 8 ne changent aucun parcours sans bornes ni cible (I2 en GRAVEL) |
| `5d` : `svc+org` = `svc+org+ride:on` | même empreinte | `211044881d80a2e9` des deux côtés | tient (et explique l'écart ci-dessus) |
| refus du contrôleur indépendant (K1), GRAVEL, toutes variantes | 0 | 0 sur 5 297 à 5 656 parcours rendus par variante | tient |
| PANIC, WINDOW_ESCAPE | 0 | 0 | tient |

### 9.2 L'effet des bornes seules (`svc+org` sans bornes contre `svc+org+ride:on`)

[calculé, `5d-off` contre `5d`]

| | `base` 0,25 · 1 | `svc+org` 0,25 · 1 |
|---|---|---|
| empreinte | `c3d03271…` → `46f6a1c8…` | `e5905688…` → `21104488…` |
| profil appris (`taste_xxh3`) | — | `34f4c22e…` → `9702f000…` : **le goût change**, l'apprentissage rejoue le moteur borné |
| premier parcours changé, « toutes » | 8,6 % · 23,3 % | **69,4 % · 83,6 %** |
| une proposition au moins changée, « toutes » | 17,7 % · 46,0 % | 94,1 % · 97,2 % |
| idem, « écart » (premier · une au moins) | 7,7 % · 26,8 % ; 17,0 % · 46,9 % | 68,0 % · 84,5 % ; 95,9 % · 99,0 % |
| `NO_CANDIDATE` (sur 780) | 22 → 21 · 22 → 22 | 6 → 6 · 8 → 9 |
| parcours rendus passant sur une arête bornée hors exemption (`banned_team` > 0) | 494 → **0** (sur 5 315 · 5 297) | 636 → **0** (sur 5 434 · 5 427) ; max 7,2 % de la longueur → 0 |
| part `offroad` médiane du premier | 0,212 → 0,211 · 0,244 → 0,242 | 0,416 → 0,415 · 0,456 → 0,457 |
| ms P50 · P90 | 252 · 567 → 251 · 569 ; 212 · 439 → 217 · 430 | 570 · 1 132 → 575 · 1 143 ; 564 · 1 151 → 536 · 1 125 |

Goût et formes, `svc+org` sans → avec bornes (`S / S_base` contre la `base` sans bornes) [calculé] :

| lecture · curseur | `S / S_base` | B1 (classes hors revêtement) | r/D | compacité | `far_pos` | `detour_far` | `sinuosity_10` | `turns_per_10km` | v2 · tri |
|---|---|---|---|---|---|---|---|---|---|
| toutes · 0,25 | 0,278 → 0,292 | 0,289 → 0,304 | 0,738 → 0,728 | 0,372 → 0,337 | 0,173 → 0,191 | 0,581 → 0,604 | 0,775 → 0,784 | 0,831 → 0,825 | 0,460 · 0,355 → 0,477 · 0,359 |
| écart · 0,25 | 0,285 → 0,265 | 0,288 → 0,270 | 0,726 → 0,703 | 0,341 → 0,327 | 0,288 → 0,231 | 0,718 → 0,642 | 0,884 → 0,863 | 0,916 → 0,906 | 0,505 · 0,381 → 0,495 · 0,392 |
| toutes · 1 | 0,227 → 0,203 | 0,234 → 0,204 | 0,795 → 0,780 | 0,258 → 0,222 | 0,206 → 0,182 | 0,659 → 0,628 | 0,767 → 0,763 | 0,615 → 0,575 | 0,408 · 0,313 → 0,407 · 0,324 |
| écart · 1 | 0,263 → 0,230 | 0,289 → 0,249 | 0,803 → 0,805 | 0,209 → 0,212 | 0,295 → 0,275 | 0,748 → 0,734 | 0,836 → 0,843 | 0,642 → 0,654 | 0,407 · 0,304 → 0,397 · 0,314 |

- **Les bornes changent beaucoup de parcours, peu de choses mesurées.** Sur `base`, l'effet direct touche 9 à 27 % des premiers
  parcours ; sous le goût, 68 à 85 %, parce que le profil appris change avec elles. Aucune mesure de forme ou de goût ne se
  dégrade au-delà de +0,03 (W1 / IQR) ; trois lectures sur quatre s'améliorent en `S`. Ni temps ni issue ne bougent
  (`NO_CANDIDATE` + 1 sur 780 au curseur 1 : 0,13 %, sous le 1 % de K2).
- **K2** : `banned` avec exemption sur le réel, médiane 0, P90 0, max 0,34 % (le 5a disait la même chose) ; `no_candidate` de
  plus dû aux bornes 0 et 1 sur 780 : **tient**. Les bornes font ce qu'elles disent : plus aucun parcours rendu sur une arête
  bornée hors exemption.

### 9.3 Les critères du §9, par cran et par curseur

Valeurs « toutes / écart » ; ✓ tient, ✗ ne tient pas. O1, O2, E, pas et C1 : [mesuré] ; A, B, C1 par requête, O5 détaillé :
[calculé]. A et B sont jugés contre 1.14 (`svc+org` sans bornes, `5d-off`) ; contre `svc+org+ride:on`, les verdicts sont les
mêmes sauf deux cases signalées.

| critère (seuil) | Peu 0,25 · c 0,25 | Peu 0,25 · c 1 | Habitude 0,47 · c 0,25 | Habitude 0,47 · c 1 | Beaucoup 0,60 · c 0,25 | Beaucoup 0,60 · c 1 |
|---|---|---|---|---|---|---|
| **O1** (≥ 70 % ; 80 % à 0,47) | 74 / 78 % ✓ | 63 / 68 % ✗ | 82 / 86 % ✓ | 83 / **78 %** ✓ / ✗ | 62 / 62 % ✗ | 67 / 69 % ✗ |
| **O2** (médiane ∈ ±0,02) | +0,015 / +0,010 ✓ | +0,017 / +0,013 ✓ | −0,003 / −0,003 ✓ | +0,006 / +0,011 ✓ | −0,038 / −0,039 ✗ | −0,024 / −0,022 ✗ |
| pas P50 · P90 · max ; réglés au pas 0 | 2 · 4 · 4 ; 42 / 44 % | 2 · 4 · 4 ; 27 / 25 % | 1 · 4 · 4 ; 56 / 59 % | 1 · 4 · 4 / 2 · 4 · 4 ; 51 / 46 % | 1 · 4 · 4 ; 52 / 52 % | 1 · 4 · 4 ; 55 / 55 % |
| **C1**, rapport des quantiles P50 · P90 (≤ 3,0 · 3,5) | × 1,58 · 2,38 / 1,44 · 2,73 ✓ | × 2,25 · 2,83 / 2,07 · 2,77 ✓ | × 1,26 · 2,44 / 1,18 · 2,09 ✓ | × 1,53 · 2,14 / 1,52 · 2,33 ✓ | × 1,38 · 2,34 / 1,30 · 2,37 ✓ | × 1,40 · 2,10 / 1,38 · 2,13 ✓ |
| **C1**, par requête P50 · P90 (≤ 3,0 · 3,5) | × 1,86 · **4,40** / 1,83 · **4,16** ✗ | × 2,15 · **4,43** / 2,14 · **4,34** ✗ | × 1,32 · **4,23** / 1,19 · **3,97** ✗ | × 1,50 · **3,87** / 1,76 · **4,36** ✗ | × 1,84 · **4,56** / 1,83 · **4,49** ✗ | × 1,71 · **4,47** / 1,88 · **4,72** ✗ |
| **A1** r/D, compacité (≤ 1.14 + 0,05) | 0,886 · 0,492 contre 0,738 · 0,372 ✗ / ✗ | 1,003 · 0,379 contre 0,795 · 0,258 ✗ / ✗ | 0,519 · 0,313 ✓ / ✓ | 0,595 · 0,278 ✓ / ✓ (¹) | 0,373 · 0,273 ✓ / ✓ | 0,379 · 0,213 ✓ / ✓ |
| **A2** v2 ∈ [0,32 ; 0,50], tri ≥ 1.14 − 0,05 | 0,481 · 0,376 ✓ / **0,505** ✗ (²) | 0,442 · 0,352 ✓ / ✓ | 0,475 · 0,378 ✓ / ✓ | 0,427 · 0,346 ✓ / ✓ | 0,444 · 0,351 ✓ / ✓ | 0,397 · 0,323 ✓ / ✓ |
| **A3** `shared_share` P50 · P90 | 0,003 · 0,012 ✓ | 0,003 · 0,014 ✓ | 0,003 · 0,023 ✓ | 0,003 · 0,020 ✓ | 0,003 · 0,026 ✓ | 0,003 · 0,025 ✓ |
| **A3** `far_pos`, `detour_far`, `sinuosity_10`, `turns_per_10km` (≤ 1.14 + 0,10) | sinuosité 0,945 contre 0,775, virages 1,013 contre 0,831, détour 0,690 contre 0,581 ✗ / ✗ | sinuosité 1,018 contre 0,767, virages 0,903 contre 0,615 ✗ / ✗ | ✓ / ✓ (`far_pos` + 0,035, les trois autres en baisse) | ✓ / ✓ | ✓ / ✓ | ✓ / ✓ |
| **B1** `S / S_base` hors revêtement (≤ 1.14 + 0,03) | 0,598 contre 0,289 ✗ / 0,558 contre 0,288 ✗ | 0,598 contre 0,234 ✗ / 0,627 contre 0,289 ✗ | 0,256 ✓ / 0,270 ✓ | **0,2645 contre 0,2636** ✗ (de 0,001) / 0,286 ✓ | 0,336 contre 0,319 ✗ / 0,343 contre 0,318 ✗ | 0,398 contre 0,264 ✗ / 0,407 contre 0,319 ✗ |
| **B2** (0,47 seul) écart à la médiane réelle ≤ ½ de 1.14 : `way_path` | — | — | 0,011 contre 0,050 ✓ / 0,017 contre 0,062 ✓ | 0,002 contre 0,017 ✓ / 0,006 contre 0,025 ✓ | — | — |
| **B2** `surf_unpaved` | — | — | 0,022 contre 0,040 ✗ / 0,018 contre 0,026 ✗ | 0,028 contre 0,043 ✗ / 0,026 contre 0,038 ✗ | — | — |
| **E1** (0,47 seul ; `run1k` ≥ réel − 0,10 et ≥ sans cible) | (0,639 sous 0,706 sans cible) | (0,615 sous 0,699) | 0,744 / 0,727 contre 0,644 · 0,706 ✓ | 0,715 / 0,720 ✓ | (0,774) | (0,748) |
| **E2** `switch_10km` ≤ 1,5 × réel | 8,2 / 8,1 contre 14,6 ✓ | 8,6 / 8,7 ✓ | 11,2 / 11,3 ✓ | 12,4 / 12,5 ✓ | 12,4 / 12,3 ✓ | 13,0 / 12,8 ✓ |
| **K1** `VIOLATION` (0) | 0 ✓ | 0 ✓ | 0 ✓ | 0 ✓ | 0 ✓ | 0 ✓ |

(¹) Contre `svc+org+ride:on` : compacité 0,278 > 0,222 + 0,05, ✗ (lecture « toutes ») ; contre 1.14 (0,258) ✓. Pareil pour
B1 « écart » curseur 1 (0,282 contre 0,245 + 0,03, ✗ contre la référence bornée ; ✓ contre 1.14).
(²) La référence 1.14 elle-même est à 0,505 dans cette case : hors bande sans consigne.

- **O3** (O1 par équipe de plus de 30 boucles au cran 0,47, ≥ 60 %) [mesuré, « toutes »] : A 86 · 91 %, **B 70 · 63 %** :
  tient, de peu pour B au curseur 1. En lecture « écart » (57 boucles de B) : A 88 · 87 %, **B 79 · 58 %** [calculé] : sous
  60 % au curseur 1.
- **O4** : au banc, `reached` du moteur égal à la mesure du banc sur le premier parcours gardé dans **100 %** des jeux, à chaque
  cran. Au service, le test `reached_follows_the_first_route_the_checker_kept` (crochet `corrupt_first_route`) est celui de
  l'étape 8 ; il n'a pas été rejoué ici.
- **E1, E2** : **sans objet au sens strict** (5e abandonné, R6) ; rapportés. E1 n'est défini qu'au cran 0,47, où il tient ; les
  « ✗ » du `-compare.md` à 0,25 viennent de ce que le banc l'applique à tous les crans : avec moins de chemins, `run1k` baisse
  naturellement sous la variante sans cible.
- **C1, le reste** : au plus 4 pas partout ✓. Le banc n'enregistre pas l'instant de lancement de chaque pas : « aucun pas lancé
  au-delà de 10 s » ne se vérifie pas directement. Indice : 0 à 3 jeux par groupe dépassent 10 s de génération (max 17,4 s), tous
  en 1 pas sauf un (2 pas, 10,0 s) [calculé] ; aucun ne montre de pas lancé tard.

Par équipe, O1 · O2 (« toutes », curseur 0,25 · 1) [calculé] :

| équipe | Peu 0,25 | Habitude 0,47 | Beaucoup 0,60 |
|---|---|---|---|
| A (545) | 75 % · +0,017 ; 57 % · +0,031 | 86 % · +0,003 ; 91 % · +0,011 | 77 % · −0,021 ; 86 % · −0,004 |
| B (222 jeux `OK`) | 73 % · +0,010 ; 79 % · −0,004 | 70 % · −0,019 ; 63 % · −0,033 | **24 % · −0,087 ; 18 % · −0,111** |

C, H et I (5, 1 et 1 boucles) : sans critère.

### 9.4 O5 : d'où viennent les cibles manquées

[calculé] Pour chaque jeu manqué : sens de l'écart, puis, quand la part rendue est trop basse, la part `offroad` de la **boucle
réelle partie du même point** (une personne de l'équipe a-t-elle trouvé la part visée à cet endroit ?).

| cran · curseur | manqués | trop haut · trop bas | trop bas : la boucle réelle atteint la cible − 0,05 | trop bas, par équipe (A · B) |
|---|---:|---|---|---|
| 0,25 · 0,25 | 197 | 151 · 46 | 42 sur 46 | — |
| 0,25 · 1 | 283 | **238** · 45 | 41 sur 45 | — |
| 0,47 · 0,25 | 141 | 30 · 111 | 41 sur 111 | 51 · 60 |
| 0,47 · 1 | 133 | 35 · 98 | 25 sur 98 | 23 · 75 |
| 0,60 · 0,25 | 296 | 5 · **291** | 34 sur 291 | 120 · 169 |
| 0,60 · 1 | 257 | 6 · **251** | 14 sur 251 | 69 · 182 |

- **« Peu » manque par excès de chemins, surtout pour A au curseur 1** (39 % des jeux de A trop hauts) : le goût appris de A
  (chemins) tire contre la consigne et la grille ne descend pas assez (μ ≤ 2). Ce n'est pas un secteur sans routes.
- **« Beaucoup » manque par défaut, et c'est B** : 76 à 82 % des jeux de B sont trop bas. **La part réelle de B est à 0,31 en
  médiane, 0,42 au P90 ; 0,4 % de ses boucles atteignent 0,55** : autour de ses départs, personne ne roule à 0,60, et la boucle
  réelle du même départ n'atteint la cible que dans 1 cas sur 169 · 182. C'est un **secteur sans assez de chemins pour 0,60**
  (et le R4 du 5a, un « Comme d'habitude » propre à B, déjà posé). A, dont 36 % des boucles réelles dépassent 0,55, tient O1 à
  77 · 86 %.
- **La colonne « secteur sans chemins » du `-compare.md` surestime.** Elle range un manqué dans ce cas quand la part de
  non-revêtu des arêtes du disque D / 2 est sous la cible − 0,05 ; or ce disque vaut 0,25 à 0,37 presque partout (P90 0,373 ;
  42 à 63 valeurs distinctes seulement, beaucoup de boucles partant des mêmes points), y compris là où A tient 0,47 et 0,60. À
  0,47, 37 des 51 manqués trop bas de A ont une boucle réelle au-dessus de 0,42 au même départ : ce sont des **échecs de la
  recherche**, pas du secteur. La mesure par la boucle réelle du même départ est plus parlante ; le disque est trop large pour
  trancher.

### 9.5 Lecture

- **« Comme d'habitude » (0,47) tient l'essentiel** : O1 82 à 86 % (78 % en « écart » au curseur 1), O2 à 0,01 près, formes
  **meilleures** que 1.14 sauf `far_pos` (W1 / IQR de r/D − 0,20 à − 0,22), goût hors revêtement égal (B1 0,256 à
  0,286 ; une case à 0,001 au-dessus du seuil), `way_path` ramené au réel (B2 ✓). **B2 `surf_unpaved` ne tient pas** : l'écart
  au réel baisse d’un tiers à 45 % (0,040 → 0,022 ; 0,043 → 0,028), pas de moitié ; la consigne vise `offroad` (classe de route-details), pas
  le tag `surface` seul, et le reste de l'écart est dans les chemins sans tag de revêtement.
- **« Peu » (0,25) tient la part au curseur 0,25, pas au curseur 1** (63 · 68 %), par excès de chemins chez A ; il **dégrade
  les formes et le goût** : r/D et compacité + 0,12 à + 0,21 en W1 / IQR, sinuosité et virages + 0,17 à + 0,30, B1 doublé
  (0,56 à 0,63 contre 0,23 à 0,29). Une partie est structurelle — le réel contre lequel A et B mesurent est la boucle de l'équipe,
  à 0,47 en médiane : un parcours à 0,25 roule d'autres classes et tourne moins (`turns_per_10km` 19,5 contre 27,6) — mais r/D
  et compacité ne s'expliquent pas par les classes.
- **« Beaucoup » (0,60) ne tient pas O1 ni O2** (62 à 69 %, −0,02 à −0,04), à cause de B (O1 18 à 24 %) ; sans B, il tiendrait
  (A 77 · 86 %). Formes ✓ ; B1 ✗ (0,34 à 0,41 contre ≤ 0,26 à 0,32) : par construction, puisque la cible s'écarte du réel des
  équipes.
- **B1 aux crans « Peu » et « Beaucoup » ne peut pas tenir tel qu'écrit** : il compare au réel des boucles de l'équipe, qui sont
  à « Comme d'habitude ». Il ne mesure là que l'écart demandé.
- **C1 réécrit : il tient ou non selon la lecture du texte.** En rapport de quantiles (ce que le banc calcule), tient partout
  (P50 × 1,18 à 2,25, P90 × 2,09 à 2,83). En rapport **par requête** (« de la même requête sans cible »), la médiane tient
  (× 1,19 à 2,15) mais le **P90 est à × 3,87 à 4,72 partout** : 14 à 32 % des requêtes paient plus de × 3,5, celles qui vont à 4
  pas (15 à 38 % des jeux).

### 9.6 Ce qui demande une décision

1. **C1** : quelle lecture vaut, quantiles (tient) ou par requête (P90 manque partout, × 3,9 à × 4,7) ? Si c'est par requête :
   accepter, ou ramener le P90 par un pas de moins (3 au plus) au prix d'O1.
2. **« Beaucoup » pour B** : 0,4 % des boucles réelles de B passent 0,55 ; O1 0,60 ne tiendra pas tant que le cran est commun.
   R4 (cran par équipe, repoussé à un lot suivant) est la seule réponse qui ne relâche pas O1 ; sinon, juger O1 « Beaucoup » hors
   des équipes dont le réel n'y va pas.
3. **« Peu » au curseur 1** (O1 63 · 68 %) et ses formes (A1, A3, B1) : élargir la grille vers μ > 2 pour A, ou accepter.
4. **B1 aux crans « Peu » et « Beaucoup »** : critère à réécrire (contre une référence à la même part, ou à 0,47 seulement).
5. **B2 `surf_unpaved`** : manque de peu à 0,47 ; la cause est la définition (R2, `API-214`), pas la consigne.
6. **Non mesuré ici** : K1 et I1 en ROAD (campagne GRAVEL seule), C3 (conteneur), C4 (planète), relecture sur carte (étape 9).

## 10. Service : C3 (France, conteneur de 3 Gio) et C4 (planète)

> Relevés du 23 novembre 2026, sur `d0bea13` (paquet 1.15.0, API 1.14). Image `ord-lot5:dev` construite du `Dockerfile` du
> dépôt (41 Mo, jamais poussée) ; référence : l'image publiée `ghcr.io/glandais/osm-route-details:1.14.0` (API 1.13), sur
> les mêmes requêtes **sans** cible (elle n'a pas la consigne). Rien n'est tiré d'une donnée de production.

### 10.1 Cadre

- `docker run --memory 3g --memory-swap 3g`, environnement du service partagé (`MALLOC_ARENA_MAX=2`,
  `MALLOC_MMAP_THRESHOLD_=1048576`, caches d'altitude et de rues 256 Mo, hors ligne, pas d'altitude IGN), fichiers du graphe
  montés **un à un en lecture seule** (pas de `.orda` : attracteurs absents, comme en production), volume neuf, **défauts du
  service** (budget 30 s, `max_concurrent` 2, `max_queued` 8, `rideability = on`). Mémoire lue dans le groupe du conteneur
  (`anon`, `memory.peak`, `memory.events`) toutes les 0,5 s ; état par `docker inspect`. Pages de fichier évincées avant le
  démarrage seulement, pas entre les requêtes.
- **Requêtes** : gravel, découverte (`cursor` 1, aucune équipe, **aucun dépôt** : les profils d'équipe et le test « dépôts en
  200 » du C3 ne sont pas joués, la donnée est privée), graine 1, quatre variantes : sans cible, 0,25, 0,47, 0,60. Ce ne sont
  **pas** les départs d'équipe de la séance du 4c : ils vont de 40 à 80 km au lieu de 30 à 50 km, donc le seuil absolu du C3
  ne se compare pas tel quel (sans cible, le P50 est déjà 0,55 s ici).
  - France : 18 villes × gravel 40, 60, 80 km = 54 requêtes en série par variante ; 162 à 4 clients (graines 1 à 3).
  - Planète : zones denses (Paris, Tokyo, Shinjuku, New York, Séoul, Osaka) × gravel 40, 80, 150, 200 km = 24 ; rurales
    (Périgord, Castille, Iowa, Tokachi, Bavière, Galles) × 40, 80, 150 km = 18 ; 4 clients sur les 42 (même ordre, mélange
    dense puis rural, donc très japonais en tête, comme le pire cas du 1.14 §4).
- Outil : un pilote Python hors dépôt (dérivé de celui du relevé 1.14), pas conservé ici.

### 10.2 C3, France

| série | variante | n | P50 (s) | P90 (s) | max (s) | `reached` | pas moyen |
|---|---|---:|---:|---:|---:|---|---:|
| série | sans cible | 54 | 0,55 | 0,87 | 1,80 | — | — |
| série | 0,25 | 54 | 0,59 | 1,78 | 2,93 | 45 / 54 | 1,85 |
| série | 0,47 | 54 | 0,61 | 2,10 | 6,62 | 51 / 54 | 1,87 |
| série | 0,60 | 54 | 0,74 | 2,48 | 5,41 | 39 / 54 | 2,06 |
| 4 clients | sans cible | 162 | 0,87 | 1,67 | 3,19 | — | — |
| 4 clients | 0,25 | 162 | 1,63 | 3,11 | 7,20 | 128 / 162 | 2,01 |
| 4 clients | 0,47 | 162 | 1,47 | 3,88 | 8,26 | 150 / 162 | 1,80 |
| 4 clients | 0,60 | 162 | 1,82 | 4,75 | 12,00 | 115 / 162 | 2,19 |

- Image 1.14.0, mêmes requêtes sans cible : série 0,54 · 1,05 · 2,01 ; 4 clients 0,91 · 2,10 · 3,22. L'image du lot 5 sans cible
  est à l'identique (0,55 · 0,87 ; 0,87 · 1,67).
- Rapport par requête avec / sans cible, en série : médiane × 1,21 · × 1,35 · × 1,26 (0,25 · 0,47 · 0,60), P90 × 2,8 · × 2,6 ·
  × 3,1 : sous le × 3,0 / × 3,5 du C1 réécrit.
- 162 + 54 réponses par variante, **toutes en 200** : aucun 503, aucun 504, aucun 422. **Mémoire** : `anon` 841 Mo au plus
  (818 en série), `memory.peak` 3 221 Mo (pages de fichier récupérables, comme au 1.14 §7), `oom` 0, `oom_kill` 0, 0 redémarrage,
  `healthy`, prêt en 8,4 s.
- **Verdict C3 : seuils absolus non tenus à la lettre, aucun défaut de fond.** Le P50 en série avec cible (0,59 · 0,61 · 0,74 s)
  dépasse 0,50 s, et le P50 à 4 clients (1,47 à 1,82 s) dépasse 1,00 s ; mais ces seuils datent de requêtes plus courtes avec
  équipe (0,30 s), et la base sans cible est ici déjà à 0,55 s et 0,87 s : le surcoût propre à la consigne est de × 1,07 à × 1,35
  en série (P50 des séries), × 1,7 à × 2,1 à 4 clients (les pas d'une même requête se partagent deux places de calcul). Sur le
  sous-ensemble de 40 km seulement (le plus proche des départs du 4c) : série 0,43 · 0,46 · 0,84 s, 4 clients 1,44 · 1,37 ·
  1,73 s. Pic `anon` ≤ 1,6 Gio (0,84), 0 `oom` : tenus. « Dépôts en 200 » : non joué.

### 10.3 C4, planète

| série | variante | n | P50 (s) | P90 (s) | max (s) | issues | `reached` |
|---|---|---:|---:|---:|---:|---|---|
| denses, série | sans cible | 24 | 3,65 | 13,58 | 26,61 | 24 × 200 | — |
| denses, série | 0,25 | 24 | 6,77 | 13,16 | 17,97 | 24 × 200 | 12 / 24 |
| denses, série | 0,47 | 24 | 6,99 | 15,79 | 30,05 | 23 × 200, **1 × 504** | 8 / 23 |
| denses, série | 0,60 | 24 | 6,90 | 21,39 | 30,12 | 24 × 200 | 5 / 24 |
| rurales, série | sans cible | 18 | 0,45 | 1,25 | 2,84 | 18 × 200 | — |
| rurales, série | 0,25 · 0,47 · 0,60 | 18 | 0,51 · 0,79 · 0,65 | 2,50 · 2,72 · 2,37 | 4,47 · 8,03 · 6,62 | 18 · 17 · 17 × 200, 0 · 1 · 1 × 422 | 12 / 18 · 8 / 17 · 8 / 17 |
| 4 clients | sans cible | 42 | 5,42 | 28,34 | 30,14 | 39 × 200, 1 × 503, 2 × 504 | — |
| 4 clients | 0,25 | 42 | 7,89 | 27,38 | 30,04 | 39 × 200, 1 × 503, 2 × 504 | 22 / 39 |
| 4 clients | 0,47 | 42 | 8,19 | 28,86 | 30,04 | 37 × 200, 2 × 503, 2 × 504, 1 × 422 | 16 / 37 |
| 4 clients | 0,60 | 42 | 8,25 | 29,81 | 30,04 | 36 × 200, 1 × 503, 4 × 504, 1 × 422 | 13 / 36 |

- **Référence 1.14.0** (mêmes requêtes sans cible) : denses 3,75 · 13,84 · 25,39 s, rurales 0,43 · 1,14 · 3,21 s, **24 + 18 × 200** ;
  4 clients 5,29 · 28,85 · 30,37 s, 39 × 200, 1 × 503, 2 × 504. Galles 40 km (`no_candidate`) rend 200 sans cible et 422 aux cibles
  0,47 et 0,60 (en série et à 4 clients) : **un 200 devenu 422** par cible, à examiner (échec de la recherche à cible, pas du budget).
- **Mémoire** : `anon` 981 Mo au plus (914 en 1.14.0), `memory.peak` 3 221 Mo, `oom` 0, `oom_kill` 0, 0 redémarrage, prêt en 4,8 s.
- **Verdict C4 : non tenu.**
  1. **Un 504 de plus en série** : Tokyo gravel 200 km à 0,47 (30,05 s ; sans cible 16,7 s, 1.14.0 : 200). À 4 clients, les 504 sont 2
     sans cible comme en 1.14.0 et à 0,25 et 0,47, **4 à 0,60** (Tokyo 150 km, Shinjuku 80, 150 et 200 km) ; à 4 clients les
     requêtes lentes se bousculent et le détail bouge d'une variante à l'autre.
  2. **La règle « pas plus longue que max(sans cible, 10 s) + 1 s » manque** : en série, 1 requête à 0,25 (Tokyo 200 km, 16,7 → 18,0 s),
     3 à 0,47 et 6 à 0,60 (jusqu'à Tokyo 200 km à 30,1 s, rendue en 200 avec `reached = false`, 1 pas, après l'échéance du
     contrôleur). Les requêtes concernées sont toutes dans les agglomérations japonaises ou New York, où la base dépasse déjà 6 s.
  3. Médiane par requête avec / sans cible (denses, série) : × 1,13 · × 1,65 · × 1,88, P90 × 2,6 à × 2,8 ; rurales × 1,26 · × 1,81 ·
     × 1,83, P90 × 2,7 à × 3,0 (sous le × 3,0 / × 3,5 du C1 réécrit, comme en France).
  4. Part des pas : 1 pas pour 41 des 96 jeux denses, 4 pas pour 10 ; rurales 4 pas pour 23 sur 53 ; `reached` 5 / 24 à 0,60 en
     zone dense.

### 10.4 Ce qui demande le propriétaire

- **C3** : le seuil absolu (0,50 s · 1,00 s) n'est pas comparable à la séance du 4c ; le rapport avec / sans cible (× 1,07 à × 1,35
  en série) tient. Choisir : lire C3 en rapport, ou rejouer la séance du 4c avec dépôts (équipes, hors de ce relevé).
- **C4** : le 504 de plus est dans l'agglomération de Tokyo à 200 km, au pire de 1.14.0 (16,7 s sans cible). Les pas k ≥ 1
  ne sont pas arrêtés avant 10 s (`steps_until_ms`) par une borne qui tienne compte du budget restant : un pas lancé à 9 s
  avec un coût de 15 s dépasse 30 s. Pistes (non faites) : ne lancer un pas que si la durée du premier pas, ajoutée au temps
  écoulé, tient dans le budget ; ou ramener `steps_until_ms`.

## 11. Relecture sur carte (étape 9, §13 de la note)

> Relecture du 23 novembre 2026 sur `764f644` (moteur de la bascule, `rideability = on`). Lettres et agrégats seulement :
> les images et les fichiers par parcours sont restés hors dépôt et ont été effacés avec le reste des fichiers temporaires.

### 11.1 Cadre et écarts à la note

- **Pas la visionneuse telle quelle.** `tools/ord-loops-viewer` lit les GeoJSON du mode rejeu du lot 2 (`--geojson`) :
  ni consigne, ni attribut d'arête ; et sa page tire MapLibre et un fond de carte du réseau, ce qui aurait envoyé des départs
  d'équipe au-dehors. À la place : un crochet **temporaire, jamais commité**, dans `likeness/play.rs`, a écrit pour le premier
  parcours gardé de chaque jeu à cible chaque arête prise (classe, `surface`, `tracktype`, `smoothness`, `mtb:scale`, accès,
  `surface_rd`, longueur, arête d'équipe, exemption, `rideable` du contrôleur, géométrie) et les arêtes du graphe à quelques
  centaines de mètres du tracé ; un script jetable en a fait une **image statique par parcours** (fond : arêtes du graphe,
  revêtues en gris, non revêtues en beige ; tracé : revêtu en bleu, non revêtu en brun, rugueux permis — `bad`, `very_bad`,
  `grade4`, `sand`, `grass` — en orange, non roulable en magenta), regardée à l'œil, et les mesures ci-dessous.
- **Dans le banc, pas hors banc.** Deux équipes, A et B, dans deux zones distinctes ; boucles **mises à l'écart** de la
  surcouche (lecture « écart », `--holdout-share 0.25`) : le départ est réel mais la boucle n'a pas nourri la surcouche jouée.
  Aucune zone hors banc avec dépôt n'était disponible sans donnée de production nouvelle. Commande :
  `likeness --teams <A|B> --ride-types gravel --holdout-share 0.25 --variants base,svc+org+ride:on+target:0.47,svc+org+ride:on+target:0.60`,
  deux curseurs (0,25 et 1).
- **Population** : 776 premiers parcours gardés (A 274, B 114, par cran), tous lus par attributs ; **tirage au hasard** (graine
  fixe) de **10 par équipe et par cran**, soit 20 à « Beaucoup » (0,60) et 20 à « Comme d'habitude » (0,47) ; **15 des 40
  images** regardées à l'œil (8 à 0,60, 7 à 0,47, les deux équipes), plus le pire cas d'aller-retour ci-dessous. L'existence
  d'un chemin n'est vérifiée que **contre le graphe** (extrait OSM du 15 novembre), pas contre une imagerie ni sur le terrain.

### 11.2 Ce qui tient

| contrôle (776 parcours, deux crans) | obtenu |
|---|---|
| arête non roulable pour le contrôleur (`rideable`, §6.1, exemption comprise) | **0** |
| arêtes bornées prises au titre de l'exemption d'équipe (`grade5`, `horrible`, `mtb:scale` ≥ 2, roulées en GRAVEL par l'équipe) | 0,13 à 0,17 % de la longueur (A), 1,05 à 1,14 % (B) |
| raccord géométrique entre deux arêtes consécutives | 0 trou |
| demi-tour sur une même arête | 0 |
| arête reprise à moins de 2 km d'elle-même (cul-de-sac fait en aller-retour pour gonfler la part) | **0** |
| accès | `yes` ou `destination` seulement (`destination` sur 66 à 76 parcours de A, 2 à 4 de B, quelques mètres) |

- **Aller-retour** : les arêtes prises dans les deux sens sont des **couloirs de sortie et de retour** : 214 km cumulés à
  moins de 3 km du départ, 40 km au-delà (dont 34 non revêtus), et entre l'aller et le retour d'une même arête il y a au
  moins **24 km** de parcours. Le pire (3,1 km repris, boucle de 177 km à 0,60) et le cas de 200 km regardé sont des sorties
  de ville par la même coulée verte, pas des épingles. Médiane de longueur reprise : 0,08 à 0,18 km par parcours.
- **Enchaînement** : à l'œil, les tracés vont de chemin en chemin par de courts liens revêtus, sans zigzag de rue en rue ;
  dans le tirage, tronçons non revêtus de 345 à 914 m en médiane par parcours (le plus long de 1,4 à 6,7 km), 0 à 8
  tronçons de moins de 100 m par parcours. Les sorties d'agglomération se font sur le revêtu (bleu), le non revêtu commence en périphérie.
- **Composition du non revêtu** (part des mètres non revêtus) : `path` 45 à 58 %, `track` 40 à 48 %, `cycleway` à surface non
  revêtue 4 à 5 % (A), < 1 % (B), `footway` et `bridleway` ≤ 1,5 % ; `smoothness` absente sur 84 % (A) et 99 % (B) ;
  `tracktype` absent sur 80 à 87 %. **79 à 93 %** des mètres non revêtus sont sur des arêtes que l'équipe a roulées en
  GRAVEL ou MIXED (exemptées).

### 11.3 Défauts et points à voir

1. **La moitié de la part est présumée** : 45 à 48 % (A) et 55 % (B) des mètres non revêtus n'ont ni `surface` ni
   `tracktype` ; c'est la classe (`path`, `track`) qui les fait non revêtus (§2.2, définition de route-details). Ils sont
   presque tous d'équipe, donc probablement roulables ; mais « Beaucoup » repose sur cette présomption autant que sur des
   tags. Pas un défaut du moteur, une limite de la donnée à dire dans le texte du site.
2. **Rugueux permis hors équipe** : par parcours, médiane 0, P90 0,5 à 1,1 km de `bad` / `very_bad` / `grade4` / `sand` /
   `grass` sur des arêtes jamais roulées par l'équipe en gravel ; **40 parcours sur 776** en ont plus d'1 km (A 15 à 0,47 et
   25 à 0,60 sur 274 ; B 11 et 15 sur 114) ; le pire, 17,4 km sur une boucle de 144 km (`track` gravier `bad`, `path` terre
   `very_bad`). Deux des boucles de A regardées à 0,60, dans la même zone, prennent des `path` en terre `very_bad` hors équipe (1,1 et
   2,5 km).
   Conforme au §6.1 (permis, rendu cher par le coût), mais c'est ce qu'une personne trouvera le plus discutable : à soumettre
   au propriétaire avec R5 (borner `very_bad` hors équipe, ou non).
3. **« Beaucoup » manque en B** : à 0,60, médiane de la part 0,507 en B (0,589 en A) ; dans le tirage, B va de 0,30 à 0,57.
   À l'image, la zone de B offre peu de non revêtu au-delà des arcs déjà pris : déjà vu au §9.4 (O5), pas un tracé faux.
4. **Peu de variété en B** : les boucles de B reprennent les mêmes arcs non revêtus d'une requête à l'autre (même plus long
   tronçon de 5,7 km dans 11 des 20 tirées), et 104 à 108 parcours distincts sur 114 (curseurs 0,25 et 1 identiques pour
   quelques boucles). Effet attendu du goût d'équipe et de l'exemption ; noté, pas corrigé.
5. **Voies vertes en « non revêtu »** : sur deux boucles de A, 40 à 55 % du non revêtu est une `cycleway` en surface compactée
   ou gravier. Roulable et juste au sens de la définition ; une personne qui choisit « Beaucoup » peut s'attendre à plus
   sauvage.

**Verdict de l'étape 9** : aucun chemin interdit, aucune rupture, aucun aller-retour fabriqué ; les chemins pris existent
dans le graphe et leurs attributs sont ceux d'un gravel plausible. Les points 1, 2 et 5 sont des choix de définition ou de
bornes à confirmer par le propriétaire, pas des défauts à corriger avant la fusion.
