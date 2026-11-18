# Générateur de parcours — lot 4 : relevé du 3b (table commune, profil d'équipe appris au dépôt, mémoire en conteneur)

> Relevé du 17 novembre 2026 sur la branche `feat/ord-loops-lot4-3b` (commits `55e81fc` à `f0ba46f`, puis ce relevé ;
> paquet 1.10.0, API 1.10), Rust 1.100.0, poste de développement (16 fils, 30 Go), graphe et vues France
> `france-20261115-z11`. Conception : [`ord-loops-lot4.md`](ord-loops-lot4.md), §14 (écarts du codage : §14.16). Relevés
> précédents : [`ord-loops-lot4-sonde2.md`](ord-loops-lot4-sonde2.md), [`ord-loops-lot6-mesures.md`](ord-loops-lot6-mesures.md).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/lot4/` (`lot4-3b-commune-*`,
> `3b-essai-*`, `smoke-3b/`, `3b-conteneur/`). Équipes désignées par les lettres A à I ; ni départ, ni coordonnée, ni
> identifiant d'équipe, de trace ou d'arête ici. Rien n'a été poussé, publié ni déployé : les images Docker sont locales.
>
> **Mise à jour du 18 novembre : prêt à fusionner** (§12). Le propriétaire a accepté le fait (d) sous réserve, à revoir au
> 4c (§9.6 de la note). La table commune est réapprise sur 100 % des boucles retenues (`15c2cbd`), les références sont
> re-bénies (`b9374a4`) et la séance en conteneur de 3 Gio est rejouée une fois sur cette table : 18 dépôts en 200, aucun
> `oom`, pic `anon` de 1,47 Gio, aucune panique. Les §6 à §9 qui suivent sont l'état d'avant ces décisions, tel quel.
>
> **Mise à jour du 17 novembre au soir : la campagne 4 a tourné** (§6 à §9 ci-dessous, commits `8131ab1` à `502c6ac`).
> Le relevé lit ses chiffres et applique les décisions 2, 3 et 4 du §9.5 de la note. **La forme retenue est la forme
> plafonnée (`capped`), le plafond est par équipe et la table est brute** : ce sont les défauts déjà codés, aucun ne
> change. B ROAD vaut 0,750 · 0,924 et le service tient. **Le fait (d) ne tient pas** : sur A, ROAD au curseur 1, `svc`
> perd 0,044 contre `common`, pour un seuil de 0,03. Selon la décision 1, la branche **n'est donc pas prête à fusionner**.
> La réapprise sur 100 % (décision 5) et la séance en conteneur (décision 8) **n'ont pas été faites** : elles sont
> conditionnées aux critères (§9). Les §1 à §5 qui suivent sont le relevé d'avant la campagne, tel quel.
>
> **Ce relevé n'était pas une campagne** (§1 à §5) : la consigne était une fumée. La chaîne commune a été rejouée (la table
> en dépend), le banc a joué le chemin du service sur une équipe puis sur deux équipes à 20 boucles, et le service a tourné
> dans un conteneur de 3 Gio.
>
> Marques : **[mesuré]** vient des rapports du banc, de la séance en conteneur ou des tests ; **[calculé]** d'un calcul fait
> ici sur ces mesures. Sans marque dans un tableau : mesuré.

## 1. Commandes exactes

`B=./target/release/ord-loops-bench`, `G=~/.cache/osm-route-details/graph`.

| Étape | Commande |
|---|---|
| Rejeu de la chaîne commune (commit 1) | `$B likeness --label lot4-3b-commune --holdout-share 0.25 --variants base,rc:0.5+n:3` |
| Export de la table | `$B taste-common --report $G/lot4/lot4-3b-commune-rc05-p3-report.json --base $G/lot4/lot4-3b-commune-base-report.json --expect-xxh3 66ee08d3eb9c910c --min-loops 50 --out crates/ord-loops/src/taste_common.rs` |
| Contrôle du refactor (commit 3) | `$B likeness --teams C --limit 5 --label 3b-refacto` (avant : `3b-refacto-avant`, binaire du commit 1) |
| Essai (commit 5) | `$B likeness --teams B,C --limit 20 --holdout-share 0.25 --variants base,common,svc,svc+norm --label 3b-essai` |
| Fumée du banc, déterminisme | `$B likeness --teams C --limit 8 --holdout-share 0.25 --variants base,common,svc --threads {1,16} --label smoke-det{1,16} --out $G/lot4/smoke-3b` |
| Images | `docker build -t osm-route-details:lot4-3b .` ; `git worktree add --detach <dossier> develop` puis `docker build -t osm-route-details:1.9.2-local .` (jamais poussées) |
| Conteneur | `docker run -d --memory=3g --memory-swap=3g -p 127.0.0.1:18080:8080 -v <ordg, deux ordv, france.poly>:/data/…:ro -v ord3b-vol:/var/cache/osm-route-details -e ORD_CACHE__OFFLINE=true -e ORD_GENERATOR__{GRAPH,VIEW_ROAD,VIEW_GRAVEL,COVERAGE_POLY,OVERLAY_DIR}=…` (volume nommé neuf, IGN désactivé, réglages par défaut dont `profile.threads` = 4) |
| Séance | script privé (hors dépôt) : deux clés tirées au hasard et jamais écrites, `curl -T` avec `Expect: 100-continue`, un fil par clé, les neuf corps NDJSON de `$G/lot3a/deposit/` |
| Tests | `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` |
| Fumée de la relecture (après les deux `fix(server)`) | `cargo build --release -p ord-loops-bench`, puis `$B likeness --teams C --limit 8 --holdout-share 0.25 --variants base,common,svc --threads {1,16} --label relecture-det{1,16} --out $G/lot4/smoke-3b-relecture` |
| **Campagne 4** (binaire de `e779537`) | `/usr/bin/time -v $B likeness --label lot4-3b --holdout-share 0.25 --variants base,rt:0.5+n:3,common,common+norm,svc,svc+norm,svc+cap:0,svc+capt:500,svc+mix:sonde,svc+mix:sonde+norm,svc+mix:sonde+capt:500 > $G/lot4/lot4-3b.log 2>&1` |
| Second tirage des temps (même binaire) | `$B likeness --label lot4-3b-p50 --holdout-share 0.25 --variants base,svc,svc+capt:500,svc+mix:sonde` |

## 2. Vérifications

| Quoi | Résultat |
|---|---|
| `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` | verts |
| `cargo test --workspace` | **870 tests, 0 échec**, 11 ignorés (les tests du banc d'altitude, qui demandent un cache de banc) |
| Rejeu de `base` et des trois pas de la chaîne commune | **bit pour bit** ceux de la campagne 2 : `base` `4686c904975302e7` · `2d06800538d394c2`, `rc05-p1` `a123cb82e6b36fc1` · `40c8278314911d91`, `rc05-p2` `ad57882c3ed75c10` · `2f57dc10b00d143d`, `rc05-p3` `2c563b8bbe8059ae` · `66ee08d3eb9c910c` (arrêt automatique sinon) |
| Refactor (marche, retenue, requête déplacées dans `ord-graph-match`) | `likeness --teams C --limit 5` : empreinte `5ca347d60d501815` avant et après |
| `team_reference.txt`, `team_check_reference.txt` | **inchangées** par le poids de base (aucun poids) |
| `window_reference_192.txt` (ancienne référence) rejouée avec `Taste::neutral()` | passe **sans** re-bénédiction : la table commune ne change rien d'autre que les poids |
| `window_reference.txt` re-bénie (poids de base `C`) | 56 requêtes sur 140 changent de sortie |

## 3. La table commune (campagne 1)

[mesuré] Rejeu à 25 % de boucles à l'écart : 1 057 s au total, quatre variantes de 250 à 268 s, pic anonyme du banc
15 625 Mo. Les mètres par classe ne sont écrits que toutes équipes réunies (72 nombres par type).

| Type | Boucles comptées | Table exportée |
|---|---:|---|
| ROAD | 1 545 | 72 cases dans [64, 494] |
| GRAVEL | 553 | 72 cases dans [64, 1 024] (une ou des cases aux deux bornes) |
| MTB | 3 | **neutre** (sous le seuil de 50 boucles) |

`taste_xxh3` du rejeu `66ee08d3eb9c910c` ; empreinte des trois tables telles qu'exportées (MTB neutre) `ea699c10cdd18e0c`,
retrouvée telle quelle par le banc : `likeness --variants common` rend cette même empreinte de tables dans son rapport
(`smoke-3b/`). La table normalisée est exportée aussi (MTB inchangée) ; `COMMON_USE_NORMALIZED = false`.

## 4. Le chemin du service au banc (fumée)

### 4.1 Déterminisme

`likeness --teams C --limit 8 --variants base,common,svc`, 1 fil puis 16 fils : **mêmes empreintes de résultat** (`base`
`0de54eb273e40d42`, `common` `a67948a2da2acf7a`, `svc` `2f9a295d010c540c`) **et mêmes `taste_xxh3`** (`common`
`ea699c10cdd18e0c`, `svc` `b331aec32a9b8956`). Pour C : 22 boucles d'apprentissage (19 ROAD, 2 GRAVEL, 1 MTB), 3 pas, 132
jeux ; 81 s sur 1 fil, 14 s sur 16 [mesuré].

### 4.2 Essai à 20 boucles par équipe (B et C)

`--limit 20` ne borne que les boucles **jouées** : `svc` apprend sur toutes les boucles non mises à l'écart de l'équipe (B :
392 boucles, 3 pas, 2 352 jeux ; C : 22, 132 jeux). Les `S / S_base` de l'essai reposent sur 20 boucles par équipe, A absente :
**ils ne se lisent pas** ; le tableau ne dit que ce que le chemin produit.

| `S / S_base`, toutes équipes (B + C) | `common` | `svc` | `svc-norm` |
|---|---:|---:|---:|
| toutes, ROAD 0,25 / ROAD 1 | 1,164 / 1,003 | 0,914 / 0,767 | 0,960 / 0,754 |
| toutes, GRAVEL 0,25 / GRAVEL 1 | 0,483 / 0,805 | 0,384 / 0,484 | 0,372 / 0,448 |
| écart, ROAD 0,25 / ROAD 1 | 1,124 / 0,900 | 1,052 / 0,861 | 1,197 / 0,900 |
| écart, GRAVEL 0,25 / GRAVEL 1 | 0,607 / 0,679 | 0,494 / 0,533 | 0,526 / 0,556 |

B, ROAD, toutes : `svc` 1,001 · 0,883, `common` 1,197 · 1,031, `svc-norm` 0,856 · 0,978 (rappel `rt05-p3` à la seconde
sonde, sur toutes les boucles : 0,80 · 0,93). Coût d'apprentissage de B : 365 s de calcul cumulées, **91 s estimées sur 4 fils**
[calculé] ; C : 146 s, 37 s. La règle de la normalisation (§14.12.4) appliquée à l'essai : `svc-norm` perd jusqu'à 0,145 contre
`svc` (seuil 0,02) et le service n'est pas meilleur : **brute**. Un essai à 20 boucles ne tranche pas (§14.16, point 17).

## 5. Le service dans un conteneur de 3 Gio

Image `osm-route-details:lot4-3b` (40,7 Mo), `docker run --memory=3g --memory-swap=3g`, ports sur `127.0.0.1`, fichiers France
en lecture seule, volume nommé neuf, `ORD_CACHE__OFFLINE=true`, réglages par défaut. Deux clés d'espace, un fil de dépôt
chacune, **en parallèle** (staging et production à 02:10) : **18 dépôts**, les neuf corps réels sous chaque clé. Correspondance
`team-N` → lettre par le nombre de traces du corps (A 2 726, B 446, D 71, C 108, E 10) ; F et G (une trace), H et I (sept) ne
se distinguent que par le nombre de boucles retenues (G : aucune).

> **Réduit par rapport au §14.13** : le cache de pages n'a pas été évincé avant les démarrages (`drop_caches` demande un
> droit qu'on n'a pas ici ; les `file` sont donc plus bas que pages froides) ; 54 générations en série et 162 à 4 clients (le
> §14.13 disait 114 : la séance du lot 6 en faisait 114 depuis un jeu de requêtes qu'on n'avait plus), départs pris sur le
> réseau de chaque équipe, trois jeux (route 30 km au curseur 0,25, route 50 km au curseur 1, gravel 40 km au curseur 0,25).

### 5.1 Dépôts (18)

| Équipe | Boucles apprises (ROAD+GRAVEL+MTB) | Jeux | Dépôt, clé 1 (s) | dont profil (s) | Dépôt, clé 0 (s) | dont profil (s) |
|---|---:|---:|---:|---:|---:|---:|
| A | 493 | 3 000 | 298,0 | 237,2 | 522,8 | 365,6 |
| B | 381 | 2 382 | 294,0 | 287,9 | 143,8 | 137,6 |
| D | 6 | 42 | 103,9 | 11,3 | 100,4 | 10,9 |
| C | 20 | 132 | 138,0 | 29,5 | 116,9 | 27,6 |
| E | 3 | 18 | 52,9 | 22,7 | 9,5 | 2,5 |
| H, I | 1 | 12 | 8,3 · 3,6 | 2,2 · 1,3 | 3,3 · 3,6 | 1,4 · 1,3 |
| G | 0 | 0 (profil `empty`, `C`) | 1,7 | 0 | 2,0 | 0 |
| F | 1 | 6 | 4,5 | 3,6 | 5,9 | 4,8 |

Les « dont profil » incluent l'attente de l'unique place de profil quand l'autre environnement la tient : A sous la clé 0 attend
~128 s (365,6 − 237,2), B sous la clé 1 ~150 s. **Les 18 dépôts rendent 200, au premier essai (aucun 503, aucun nouvel essai),
16 profils `learned` à 3 pas, 2 `empty`**, tous dans le budget de 720 s (le plus long : 365,6 s). Après les dépôts : **18 états
`ready`, `profile.state = present`**, 18 `.ordp.enc` et 18 `.ordo.enc` dans le volume, aucun temporaire. Un dépôt de A
dure 4 min (A seule : 237 s de profil sur 4 fils, ~0,08 s par jeu), le §14.0 en prévoyait ~6,1 min [calculé].

### 5.2 Générations

| Séance | Requêtes | Résultats | p50 / p90 / max (s) |
|---|---:|---|---|
| 54 en série | 54 | 48 × 200 (tous `taste: team`), 6 × 422 | 0,24 / 0,48 / 0,81 |
| 4 clients | 162 | 144 × 200 (tous `team`), 18 × 422 | 0,43 / 0,65 / 0,98 |
| 4 clients **pendant un dépôt de A** | 162 | 144 × 200, 18 × 422 | 0,52 / 0,79 / 1,34 |

Le dépôt de A pendant les 4 clients rend 200 en 286 s. Aucun 503, aucun 504. Les 422 sont des refus fonctionnels (couverture,
départ trop loin), les mêmes lignes à chaque séance. Après redémarrage (volume gardé) : les mêmes 18 états `ready` / `present`,
18 `.ordp.enc` (compte inchangé), série 0,25 / 0,67 / 2,86, 4 clients 0,41 / 0,60 / 0,91, pendant dépôt de A 0,44 / 0,65 / 1,11 s
(dépôt : 293 s). Dépôt de A sous `docker update --cpus=4` : **283 s, profil 222,7 s, `learned`**.

### 5.3 Mémoire

| | Mesure |
|---|---|
| `anon` au plus, séance de dépôts (A à 4 fils comprise) | **1 519 Mo** (1,41 Gio) ; `memory.current` 1 592 Mo, `memory.peak` 1 596 Mo |
| `anon` au plus, générations et dépôt de A en parallèle | 1 573 Mo ; après redémarrage 1 151 Mo ; dépôt de A sous 4 CPU 1 026 Mo |
| `memory.events` | `max` 0, `oom` 0, `oom_kill` 0 |
| Critère proposé (`anon` ≤ 2,4 Gio, 80 % de 3 Gio) | tenu : **1,41 Gio au plus** |
| Rappel du lot 6 (2 Gio, sans profil) | `anon` 1 309 Mo à 18 dépôts : **+210 Mo** avec la phase de profil [calculé] |

### 5.4 Compatibilité, déploiement roulant

`osm-route-details:1.9.2-local` (construit depuis un `git worktree` de `develop`) sur le **même volume**, avec
`ORD_GENERATOR__PURGE_INTERVAL_S=10`, 45 s d'observation : **18 `.ordp.enc` avant, 18 après** la purge de démarrage et les
purges périodiques (aucun effacé, aucun temporaire) ; 18 états `ready` (sans champ `profile`) ; 48 × 200 en série, sans
`taste` (champ inconnu de 1.9.2). Retour sur 1.10 : **18 états `ready` / `present`**. La purge de 1.9.2 ne reconnaît pas un
`.ordp.enc` (relu dans son code : seuls les noms contenant `.tmp-` et ceux qui finissent par `.ordo.enc`), ce que le test
`the_192_purge_rules_never_touch_a_profile_file` recopie.

## 5 bis. Relecture

[mesuré] Après les deux corrections de la relecture (§14.16, points 21 et 22 de la note), qui ne touchent que la phase de
profil du service : `cargo fmt` et `cargo clippy --workspace --all-targets -- -D warnings` sont verts, et
`cargo test --workspace` passe **871 tests sans échec**, 11 ignorés. Le test ajouté est
`a_panic_in_the_profile_phase_still_writes_the_overlay_and_gives_its_memory_back`. La fumée du banc à 1 puis 16 fils
redonne les empreintes du §4.1 : `base` `0de54eb273e40d42`, `common` `a67948a2da2acf7a`, `svc` `2f9a295d010c540c`, et les
`taste_xxh3` `ea699c10cdd18e0c` (`common`) et `b331aec32a9b8956` (`svc`). Elle a pris 149 s à 1 fil et 48 s à 16 fils, avec un
pic de 10,4 Gio puis de 13,6 Gio (`/usr/bin/time`). La séance en conteneur n'a pas été rejouée. Les corrections ne
changent ni les tables ni les jeux : seuls le traitement d'une panique et un champ d'en-tête non lu sont touchés.

## 6. Campagne 4 (17 novembre, après les décisions du §9.5)

[mesuré] 1 h 24 min au mur, pic de RSS 18,2 Gio (`/usr/bin/time -v`), 13 variantes, une seule campagne sur la machine.
Les contrôles sont **passés aux bits près** : `base` `4686c904975302e7` · `2d06800538d394c2`, `rt05-p3` `451651c82f8a4b92` ·
`3fc8e87f9b9fbfcf`, soit ceux de la seconde sonde. Les empreintes de résultat sont `common` `d1360d75ec7304bd`, `svc`
`6bc8a67e3f5369e4`, `svc-capt500` `653ddeece5686adf` et `svc-sonde` `b1a1dc7f2e9bb42c`. Le second tirage des temps (§6.4)
redonne ces quatre empreintes.

### 6.1 `S / S_base`, toutes équipes [mesuré]

| lecture · groupe | rt05-p3 | common | common-norm | svc | svc-norm | svc-cap0 | svc-capt500 | svc-sonde | svc-sonde-norm | svc-sonde-capt500 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| toutes · ROAD 0,25 | 0,244 | 0,255 | 0,240 | **0,236** | 0,212 | 0,211 | 0,218 | 0,239 | 0,249 | 0,242 |
| toutes · ROAD 1 | 0,195 | 0,194 | 0,197 | **0,209** | 0,197 | 0,186 | 0,184 | 0,194 | 0,196 | 0,198 |
| toutes · GRAVEL 0,25 | 0,371 | 0,377 | 0,377 | **0,289** | 0,286 | 0,257 | 0,257 | 0,395 | 0,406 | 0,366 |
| toutes · GRAVEL 1 | 0,259 | 0,303 | 0,293 | **0,224** | 0,210 | 0,239 | 0,239 | 0,293 | 0,304 | 0,265 |
| écart · ROAD 0,25 | 0,203 | 0,228 | 0,221 | **0,207** | 0,190 | 0,195 | 0,190 | 0,208 | 0,222 | 0,208 |
| écart · ROAD 1 | 0,183 | 0,176 | 0,184 | **0,200** | 0,184 | 0,177 | 0,170 | 0,178 | 0,186 | 0,180 |
| écart · GRAVEL 0,25 | 0,379 | 0,387 | 0,384 | **0,330** | 0,295 | 0,287 | 0,287 | 0,398 | 0,436 | 0,379 |
| écart · GRAVEL 1 | 0,286 | 0,303 | 0,304 | **0,218** | 0,218 | 0,213 | 0,213 | 0,310 | 0,326 | 0,258 |

Par type, `svc` est au plus à **0,236 en ROAD** et **0,330 en GRAVEL** (lecture « écart » comprise) : bien sous 0,5. Le
`rt05-p3` de cette campagne redonne au millième celui de la seconde sonde (0,244 · 0,195 · 0,371 · 0,259).

### 6.2 A et B [mesuré]

| lecture · groupe | A rt05-p3 | A common | A svc | A svc-capt500 | A svc-sonde | B rt05-p3 | B common | B svc | B svc-sonde |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| toutes · ROAD 0,25 | 0,219 | 0,198 | 0,219 | 0,200 | 0,213 | 0,800 | 1,776 | **0,750** | 0,818 |
| toutes · ROAD 1 | 0,174 | 0,146 | **0,191** | 0,166 | 0,169 | 0,931 | 2,143 | **0,924** | 0,953 |
| toutes · GRAVEL 0,25 | 0,406 | 0,529 | 0,334 | 0,296 | 0,459 | 0,433 | 0,674 | 0,393 | 0,451 |
| toutes · GRAVEL 1 | 0,244 | 0,388 | 0,224 | 0,221 | 0,292 | 0,424 | 0,822 | 0,403 | 0,460 |
| écart · ROAD 0,25 | 0,184 | 0,193 | 0,197 | 0,180 | 0,182 | 0,847 | 1,535 | 0,775 | 0,718 |
| écart · ROAD 1 | 0,166 | 0,144 | 0,191 | 0,160 | 0,163 | 0,896 | 1,816 | 0,837 | 0,924 |

B est sous son plafond de 500 boucles (297 boucles apprises) : `svc-cap0` et `svc-capt500` y redonnent `svc` au bit près.

### 6.3 Faits du §14.12.5 et règle du §14.12.4 (lus par le banc, [mesuré])

| Fait | `svc` (plafonné) | `svc-sonde` |
|---|---|---|
| (a) 4 groupes ≤ 0,5, « toutes » et « écart » | **oui** (au plus 0,330) | oui (au plus 0,398) |
| (b) B ROAD « toutes » < 1,0 aux deux curseurs | **oui** : 0,750 · 0,924 | oui : 0,818 · 0,953 |
| (c) ≤ `rt05-p3` + 0,05 par groupe, « toutes » | **oui** : au plus +0,014 (ROAD 1) | oui : au plus +0,034 (GRAVEL 1) |
| (d) A : forme − `common` ≤ +0,03 en ROAD, « toutes » | **NON** : +0,021 · **+0,044** | oui : +0,014 · +0,022 |
| A : forme − `rt05-p3` en « toutes », pire groupe [calculé] | +0,017 (ROAD 1) | +0,053 (GRAVEL 0,25) |

Pour la règle de la normalisation, **brute** sur les deux formes :
- `common-norm` contre `common` : pire perte +0,008, meilleur gain 0,014. Le seuil de gain de 0,02 n'est pas atteint.
- `svc-norm` contre `svc` : pire perte 0,000, meilleur gain 0,035. La paire est satisfaite, mais la règle demande les deux
  paires.
- `svc-sonde-norm` contre `svc-sonde` : pire perte +0,038, meilleur gain −0,001. Non satisfaite.

B ROAD normalisée reste sous 1,0, et le service normalisé n'est pas pire.

### 6.4 Service [mesuré]

P50 par appel contre `base` du même tirage, et part `OK` (`base` : 98,1 % · 98,2 %). Il y a deux tirages : la campagne,
puis `lot4-3b-p50` (même binaire, machine seule). Ni `PANIC` ni `WINDOW_ESCAPE` dans aucune variante. Une `VIOLATION` à
0,25 dans `svc-sonde-norm` et `svc-sonde-capt500` (signalée, hors des variantes retenues).

| variante | × base P50, curseur 0,25 (campagne · second tirage) | curseur 1 | `OK` 0,25 · 1 |
|---|---|---|---|
| `svc` | **1,12 · 1,17** | **1,05 · 1,08** | 98,3 % · 97,8 % (−0,4 point au plus) |
| `svc-capt500` | 1,32 · 1,26 | 1,26 · 1,22 | 98,3 % · 97,9 % |
| `svc-sonde` | 1,27 · 1,22 | 1,19 · 1,15 | 98,1 % · 98,0 % |
| `svc-cap0` | 1,33 | 1,29 | 98,1 % · 97,9 % |
| `rt05-p3` (rappel) | 1,23 | 1,14 | 98,0 % · 97,9 % |

Le critère de service (P50 ≤ +25 % de `base`, `OK` −1 point au plus) **tient pour `svc`** aux deux tirages. `svc-capt500`
le manque à 0,25 aux deux tirages (+32 %, +26 %). `svc-sonde` est à la limite à 0,25 (+27 %, puis +22 %). D'un tirage à
l'autre, le P50 bouge de ±0,06 : c'est du bruit de mesure sur une machine pleine, à 16 fils.

### 6.5 Coût d'apprentissage (estimé sur 4 fils = CPU / 4, [calculé])

| variante | A : boucles (R, G) · jeux | A : estimé sur 4 fils | budget 720 s |
|---|---|---:|---|
| `svc` | 391, 109 · 3 000 | 324 s | tient |
| `svc-capt500` | 500, 400 · 5 400 | **809 s** | dépasse (estimation pessimiste : le conteneur faisait 237 s là où le banc estime 324 s, soit ~430 s [calculé] pour 5 400 jeux) |
| `svc-sonde-capt500` | 500, 400 · 5 400 | 625 s | tient |
| `svc-cap0` | 1 384, 400 · 10 704 | 1 327 s | dépasse |

## 7. Décisions du §9.5 appliquées

1. **Forme du mélange (décision 2) : `capped`**, défaut inchangé. B ROAD « toutes » vaut 0,750 · 0,924 : il est sous 1,0
   aux deux curseurs. A ne perd pas plus de 0,03 contre `rt05-p3` en « toutes » (au plus +0,017, en ROAD au curseur 1), ni
   en « écart » (au plus +0,025). La décision 2 ne déclenche donc pas le retour à la sonde. `svc-sonde`, mesurée pour
   comparaison, tiendrait les faits (a) à (d), mais pas le service à 0,25 au premier tirage.
2. **Normalisation (décision 3) : brute, définitivement.** `COMMON_USE_NORMALIZED = false` reste. La paire de la table
   commune ne gagne nulle part 0,02. La règle de la forme sonde, corrigée pour juger les deux paires (`502c6ac`), dit aussi
   brute.
3. **Plafond (décision 4) : par équipe**, défaut inchangé (`per_type = false`). Sur le GRAVEL de A en « toutes »,
   `svc-capt500` gagne 0,038 au curseur 0,25 et 0,003 au curseur 1 (0,021 en moyenne) [calculé]. **Choix prudent,
   signalé** : la décision dit « au moins 0,03 au GRAVEL de A » sans dire par quel curseur. Ce relevé exige le gain sur le
   GRAVEL entier, donc aux deux curseurs, et ne retient pas un seul curseur. Sur ce choix, le plafond par type perd aussi
   le service à 0,25, et A dépasse l'estimation du budget sur 4 fils (§6.5). Lu curseur par curseur, la décision donnerait
   « par type ».
4. **Défauts du service** : aucun ne change (`mix = capped`, `max_loops = 500`, `per_type = false`, table brute). Il n'y a
   pas de commit de défauts.

**Critères de fusion (décision 1)** : (a), (b), (c), service, `S / S_base` ≤ 0,5 par type tiennent ; **(d) ne tient pas**
(A, ROAD, curseur 1 : `svc` 0,191 contre `common` 0,146, +0,044 pour +0,03). **Prêt à fusionner : non.** Cause probable
[calculé, non mesuré à part] : avec le plafond de 500 boucles par équipe, A n'apprend que 391 boucles ROAD. La table commune,
elle, est apprise sur 1 545 boucles, dont une large majorité de A. Le plafond par type (500 ROAD) ramène (d) à +0,002 ·
+0,020.

## 8. Réapprise sur 100 % et conteneur : non faites

> Faites le 18 novembre, après la décision du propriétaire sur (d) : §12.

Les étapes 3 de la consigne (décision 5, table commune réapprise sur 100 %) et 8 (séance en conteneur de 3 Gio, une fois)
sont conditionnées aux critères, qui ne tiennent pas. Elles ne sont pas faites, pour deux raisons prudentes :
- **Tables à 100 % :** une table commune apprise sur 100 % des boucles verrait les boucles mises à l'écart. Elle fausserait
  toute campagne de contrôle qu'il faudrait rejouer après le choix du propriétaire, par exemple le plafond par type. Elle
  vient donc en dernier.
- **Conteneur :** la décision 8 ne permet qu'une séance, à rejouer après tout changement d'apprentissage. Changer le plafond
  ou la forme change l'apprentissage.

La séance du §5 (forme plafonnée, par équipe, table à 75 %) reste la dernière mesure en conteneur. Le script privé de la
séance est prêt à être rejoué, sorties sous `G/lot4/3b-conteneur-2/`.

## 9. Ce qui reste avant la mise en service

> Le point 1 est tranché le 18 novembre (fait (d) accepté sous réserve, §9.6 de la note) et le point 2 est fait (§12).
> Le point 3 reste ouvert, sans bloquer la fusion.

1. **Au propriétaire, le fait (d)**, avec trois issues mesurées ici :
   - accepter (d) à +0,044, A ROAD au curseur 1, avec la forme et le plafond actuels ;
   - passer au plafond par type (`per_type = true`) : (d) tient (+0,002 · +0,020), mais le P50 est à +26 % ou +32 % à 0,25,
     et A est estimé à 809 s sur 4 fils, au-delà des 720 s du budget. Le budget est à vérifier en conteneur, où le banc
     surestime (§6.5) ;
   - passer à la forme `sonde` (`mix = sonde`) : (a) à (d) tiennent, mais le P50 est à +22 % ou +27 % à 0,25. Ce n'est pas
     ce que dit la décision 2, puisque B ROAD est sous 1,0.
2. Puis, une seule fois et dans cet ordre :
   - réapprendre `C` sur 100 % (`likeness --label lot4-3b-commune100 --variants base,rc:0.5+n:3` sans `--holdout-share`,
     puis `taste-common` avec `--step2/--step3`) ;
   - commiter la nouvelle `taste_common.rs` (nouveau `Common::hash` : les profils écrits deviennent `incompatibles`) ;
   - re-bénir `window_reference.txt` en le disant ;
   - rejouer la séance en conteneur de 3 Gio.
3. Inchangé depuis le §5 : le banc région du lot 6 avec `--base-taste common`, les pages évincées avant les démarrages, et
   l'arrêt du service pendant une phase de profil.

## 10. Ce qui n'a pas été fait (relevé d'avant la campagne)

- ~~Campagnes 2 à 4~~ : la campagne 4 est faite (§6). Les campagnes 2 (essai) et 3 (déterminisme sur B et C) ne sont pas
  rejouées à part. La campagne 4 vérifie le déterminisme autrement : deux exécutions donnent les mêmes empreintes de
  résultat (§6). Question 2 du §14.1 : tranchée au §7. Question 3 (réapprendre `C` sur 100 %) : décision 5, à faire au §9.
- Le banc région du lot 6 (`--compare --self-check --base-taste common`) : l'égalité fenêtre / graphe entier avec la table
  commune est tenue par les tests existants rejoués sur la référence re-bénie (`the_window_of_all_tiles_is_the_flat_engine`,
  `any_superset_of_the_needed_tiles_gives_the_same_routes_and_no_escape`) ; le temps de construction des fenêtres avec la
  table commune n'est pas mesuré.
- Cache de pages évincé avant les démarrages ; le conteneur sans altitude (comme au lot 6) ; l'arrêt du service pendant une
  phase de profil (testé en logique, pas provoqué).
- Les générations de la séance ne rejouent pas celles du lot 6 (§5, encadré) : les temps ne se comparent pas ligne à ligne.

## 11. Tests

[mesuré] `cargo test --workspace` : 870 passés, 0 échec, 11 ignorés (les mêmes que les relevés précédents : cache de banc
d'altitude requis). Nouveaux : `ord-graph-match` 31 (collecteur, dépôt avec collecteur, joueur jouet, format, marche déplacée),
`ord-loops-bench` 58 (variantes `svc`, règle de la normalisation, `taste-common`, un pas contre `step72`), `ord-loops` 57
(poids de base, référence 1.9.2 sans re-bénédiction, table commune comme poids de base ou par le crochet), `ord-server` 53 de
bibliothèque (magasin, purge, chiffrement, réglages) et 9 de bout en bout (`generator_profile`).

[mesuré] Après les commits des décisions du §9.5 et la correction `502c6ac` : `cargo fmt` et `cargo clippy --workspace
--all-targets -- -D warnings` sont verts. `cargo test --workspace` passe **880 tests sans échec**, 11 ignorés.

[mesuré] Le 18 novembre, après la table à 100 % et la référence re-bénie : `cargo fmt --all -- --check` et `cargo clippy
--workspace --all-targets -- -D warnings` sont verts, Rust 1.100.0 (`rustup check` : à jour). `cargo test --workspace` passe
**880 tests sans échec**, 11 ignorés.

## 12. Table commune à 100 % et séance en conteneur (18 novembre)

Décisions du propriétaire du 18 novembre : §9.6 de la note. Le fait (d) est accepté (+0,021 · +0,044, A ROAD, `svc` contre
`common`) sous une réserve à revoir au 4c. Plafond par équipe, forme `capped`, table brute : inchangés.

### 12.1 Table commune réapprise sur 100 %

[mesuré] `likeness --label lot4-3b-commune100 --no-holdout --variants base,rc:0.5+n:3` (banc du commit `2a4e446`, 809 s, pic
`anon` de 16 122 Mo), puis `taste-common --holdout-share 0 --expect-xxh3 effbdf7c3e979d77 --min-loops 50 --step2 … --step3
…`. Les rapports donnent `holdout` = 0 et `kept` = `retained` pour les neuf équipes. `base` redonne le `taste_xxh3` neutre
`2d06800538d394c2`.

| | 75 % (campagne 1) | 100 % |
|---|---|---|
| `taste_xxh3` de `rc05-p3` | `66ee08d3eb9c910c` | `effbdf7c3e979d77` |
| empreinte de résultat de `rc05-p3` | `2c563b8bbe8059ae` | `cb966d45c326c3bd` |
| tables exportées (MTB neutre) | `ea699c10cdd18e0c` | `0090972c6fbf4bdf` |
| boucles comptées (ROAD, GRAVEL, MTB) | 1 545, 553, 3 | 2 045, 754, 4 |

[mesuré] À la relecture, `taste-common` a été relancé depuis la branche sur les mêmes rapports. Le fichier produit, passé par
`rustfmt`, est **identique octet pour octet** à `crates/ord-loops/src/taste_common.rs` (en-tête compris). L'écart de la table
à 75 % est au §14.16, point 32 de la note. ROAD change 24 cases sur 72, GRAVEL 39, MTB aucune. Une case du GRAVEL atteint la
borne haute (1 024). Seule `window_reference.txt` est re-bénie, 43 requêtes sur 140.

### 12.2 Séance en conteneur de 3 Gio, rejouée une fois

Image `osm-route-details:lot4-3b` reconstruite depuis `b9374a4` (40,8 Mo). L'image de la table à 75 % est gardée en local
sous `lot4-3b-75`. Même méthode et mêmes réductions qu'au §5 : cache de pages non évincé, 54 générations en série et 162 à
4 clients. Script et sorties sont privés, sous `G/lot4/3b-conteneur-2/`, et le volume a été effacé après la séance. Les
lettres sont celles du §5 (mêmes corps, mêmes boucles apprises).

**Dépôts (18, deux clés en parallèle)** [mesuré]

| Équipe | Boucles | Jeux | Dépôt, clé 1 (s) | dont profil (s) | Dépôt, clé 0 (s) | dont profil (s) |
|---|---:|---:|---:|---:|---:|---:|
| A | 493 | 3 000 | 524,8 | 377,3 | 297,6 | 236,0 |
| B | 381 | 2 382 | 144,2 | 137,6 | 296,4 | 290,3 |
| D | 6 | 42 | 99,8 | 10,9 | 103,7 | 11,1 |
| C | 20 | 132 | 117,4 | 27,0 | 137,9 | 29,2 |
| E | 3 | 18 | 9,7 | 2,8 | 53,4 | 22,9 |
| H, I | 1 | 12 | 3,4 · 3,8 | 1,3 · 1,3 | 8,5 · 3,7 | 2,2 · 1,2 |
| G | 0 | 0 (`empty`) | 1,1 | 0 | 2,0 | 0 |
| F | 1 | 6 | 8,4 | 6,4 | 5,8 | 3,8 |

Les 18 dépôts rendent **200 au premier essai** : aucun 503, aucun nouvel essai. On compte 16 profils `learned` à 3 pas et 2
`empty`. Le plus long profil dure 377,3 s, dont environ 141 s d'attente de la place de profil tenue par l'autre clé
[calculé]. Tous tiennent dans le budget de 720 s. Après les dépôts, les 18 états sont `ready` / `present`, avec 18 `.ordp.enc`,
18 `.ordo.enc` et aucun temporaire.

**Générations** (p50 / p90 / max, s) [mesuré]

| Séance | Résultats | Avant redémarrage | Après redémarrage |
|---|---|---|---|
| 54 en série | 48 × 200 (`taste: team`), 6 × 422 | 0,25 / 0,49 / 0,74 | 0,25 / 0,65 / 2,90 |
| 162 à 4 clients | 144 × 200, 18 × 422 | 0,49 / 0,76 / 0,91 | 0,42 / 0,65 / 0,85 |
| 162 à 4 clients pendant un dépôt de A | 144 × 200, 18 × 422 | 0,53 / 0,75 / 1,08 | 0,48 / 0,68 / 0,90 |

Le dépôt de A pendant les générations rend 200 en 289 s, puis en 296 s après redémarrage. Les 422 sont les mêmes refus
fonctionnels qu'au §5. Au redémarrage, on retrouve les 18 états `ready` / `present` et le compte de `.ordp.enc` inchangé.

**Compatibilité** [mesuré] : `1.9.2-local` sur le même volume, avec purge toutes les 10 s pendant 45 s. Le volume compte 18
`.ordp.enc` avant et 18 après. Les 18 états sont `ready`, la série donne 48 × 200 (0,24 / 0,71 / 2,82 s). Au retour sur
1.10, les 18 états sont `ready` / `present`. Le dépôt de A sous `docker update --cpus=4` rend **200 en 285,9 s**, profil
`learned` en 224,4 s.

**Mémoire et état du service** [mesuré]

| | Mesure | §5 (table à 75 %) |
|---|---|---|
| `anon` au plus, dépôts | **1 584 Mo (1,47 Gio)**, `memory.peak` 1 603 Mo | 1 519 Mo |
| `anon` au plus, générations et dépôt de A en parallèle | 1 551 Mo ; après redémarrage 1 104 Mo ; A sous 4 CPU 1 046 Mo | 1 573 · 1 151 · 1 026 Mo |
| `memory.events` (`max`, `oom`, `oom_kill`) | 0, 0, 0 dans toutes les phases | 0, 0, 0 |
| Conteneur | `running`, code 0, `OOMKilled` faux, 0 redémarrage, dans chaque phase | |
| Journaux | aucune ligne `WARN` ni `ERROR`, aucune panique | |

**Critères du §14.13** : tous tiennent. `oom` 0 et `max` 0 ; pic `anon` de 1,47 Gio, pour 2,4 Gio au plus ; tous les dépôts
en 200 ; profil de A `learned` à 3 pas dans le budget ; aucun 503 ; 1.9.2 garde tous les profils. Contre la séance du §5,
les temps de profil sont les mêmes à 1 % près (A seule : 236,0 s contre 237,2 s), et le pic `anon` monte de 65 Mo
[calculé], dans le bruit d'une séance à l'autre.

**Prêt à fusionner : oui.** Réserves : le fait (d), accepté sous réserve et à revoir au 4c avec les quelques classes du
GRAVEL que la table à 100 % pousse à la borne ; et les mesures du §9, point 3, toujours ouvertes.
