# Générateur de parcours — lot 5 (5a) : la part de non-revêtu du moteur 1.14 contre le réel, et la sonde part(μ)

> Relevé du 23 novembre 2026 sur la branche `feat/ord-lot5`. Conception : [`ord-loops-lot5.md`](ord-loops-lot5.md) (§2.2, §3,
> §5.2-5.3, §6, §9, §13 étape 2). Campagnes jouées avec le binaire `release` du banc (fonctionnalité `probe`), Rust 1.100.0,
> poste de développement (AMD Ryzen 7 4800H, 16 fils, 30 Go). Graphe et vues France `france-20261115-z11` (outil ord-graph
> 1.9.0, réplication 1794777748) ; export de production du 14 novembre (3 377 lignes, empreinte xxh3 `ca53159cc90498d6`).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/lot5/5a/` (essai et relevé complet) et
> `…/lot5/5a-mu/` (sonde). Lettres d'équipes et agrégats seulement : ni départ, ni coordonnée, ni identifiant d'équipe, de
> trace ou d'arête, ni nom de lieu. Rien n'a été poussé ni déployé.
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
