# Générateur de parcours — lot 4 : relevé du 3b (table commune, profil d'équipe appris au dépôt, mémoire en conteneur)

> Relevé du 17 novembre 2026 sur la branche `feat/ord-loops-lot4-3b` (commits `55e81fc` à `f0ba46f`, puis ce relevé ;
> paquet 1.10.0, API 1.10), Rust 1.100.0, poste de développement (16 fils, 30 Go), graphe et vues France
> `france-20261115-z11`. Conception : [`ord-loops-lot4.md`](ord-loops-lot4.md), §14 (écarts du codage : §14.16). Relevés
> précédents : [`ord-loops-lot4-sonde2.md`](ord-loops-lot4-sonde2.md), [`ord-loops-lot6-mesures.md`](ord-loops-lot6-mesures.md).
> Sorties complètes, **privées et hors dépôt**, sous `~/.cache/osm-route-details/graph/lot4/` (`lot4-3b-commune-*`,
> `3b-essai-*`, `smoke-3b/`, `3b-conteneur/`). Équipes désignées par les lettres A à I ; ni départ, ni coordonnée, ni
> identifiant d'équipe, de trace ou d'arête ici. Rien n'a été poussé, publié ni déployé : les images Docker sont locales.
>
> **Ce relevé n'est pas une campagne.** La consigne était une fumée : la chaîne commune a été rejouée (la table en dépend),
> le banc a joué le chemin du service sur une équipe et sur deux équipes à 20 boucles, le service a tourné dans un conteneur
> de 3 Gio. **Les campagnes 2 à 4 du §14.13 n'ont pas tourné** : les faits du §14.12.5 (S / S_base du chemin du service,
> B ROAD sous 1,0, `svc ≤ rt05-p3 + 0,05`, A) et la décision sur la normalisation ne sont **pas établis** ici.
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

## 6. Ce qui n'a pas été fait

- **Campagnes 2 à 4** (`svc+cap:0`, `svc+capt:500`, A en entier, les faits du §14.12.5, la normalisation sur toutes les
  équipes) ; question 2 du §14.1 (500 par équipe ou par type) donc sans chiffres ; question 3 (réapprendre `C` sur 100 %).
- Le banc région du lot 6 (`--compare --self-check --base-taste common`) : l'égalité fenêtre / graphe entier avec la table
  commune est tenue par les tests existants rejoués sur la référence re-bénie (`the_window_of_all_tiles_is_the_flat_engine`,
  `any_superset_of_the_needed_tiles_gives_the_same_routes_and_no_escape`) ; le temps de construction des fenêtres avec la
  table commune n'est pas mesuré.
- Cache de pages évincé avant les démarrages ; le conteneur sans altitude (comme au lot 6) ; l'arrêt du service pendant une
  phase de profil (testé en logique, pas provoqué).
- Les générations de la séance ne rejouent pas celles du lot 6 (§5, encadré) : les temps ne se comparent pas ligne à ligne.

## 7. Tests

[mesuré] `cargo test --workspace` : 870 passés, 0 échec, 11 ignorés (les mêmes que les relevés précédents : cache de banc
d'altitude requis). Nouveaux : `ord-graph-match` 31 (collecteur, dépôt avec collecteur, joueur jouet, format, marche déplacée),
`ord-loops-bench` 58 (variantes `svc`, règle de la normalisation, `taste-common`, un pas contre `step72`), `ord-loops` 57
(poids de base, référence 1.9.2 sans re-bénédiction, table commune comme poids de base ou par le crochet), `ord-server` 53 de
bibliothèque (magasin, purge, chiffrement, réglages) et 9 de bout en bout (`generator_profile`).
