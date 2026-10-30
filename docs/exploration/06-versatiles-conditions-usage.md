# 06 — Conditions d'usage de l'API publique VersaTiles

> Recherche du 2026-10-30. Elle complète la §3.1 de
> [03-sources-tuiles-vectorielles.md](03-sources-tuiles-vectorielles.md). Sources : docs,
> dépôts et discussions `versatiles-org`, plus des requêtes `curl` sur le serveur réel.
> **[F]** = fait vérifié (source citée ou constat `curl`). **[I]** = inférence ou estimation
> de notre part.

## 1. Verdict

**L'API publique `tiles.versatiles.org` convient au MVP**, pour un faible volume avec cache
disque, à quatre conditions :

1. User-Agent descriptif : `osm-route-details/<version> (+https://github.com/glandais)`,
   sur le modèle d'osm-surface ;
2. cache disque TTL 7 jours (≥ 6 h), ETag stocké et revalidation `If-None-Match`, tuile
   périmée servie si le serveur est en erreur ; `Accept-Encoding: br, gzip` ;
3. concurrence ≤ 8, backoff exponentiel sur `429`/`5xx` en respectant `Retry-After`,
   jamais de pré-chargement massif ;
4. attribution OSM + ESA WorldCover dans les réponses, et journalisation de la date des
   données lue dans `tiles.json`.

On passe au fichier local dès qu'une condition du §7 est remplie.

## 2. Statut du service et règles d'usage

- **[F]** C'est un **serveur de démonstration**, « free and open to everyone, but it comes
  without any guarantees ». Pas de clé d'API, pas de frais. Sources :
  [guide « use tiles.versatiles.org »](https://docs.versatiles.org/guides/use_tiles_versatiles_org.html)
  et [versatiles.org](https://versatiles.org/).
- **[F]** Il n'existe pas de CGU ni de page « fair use » formelle. Le texte de référence est
  la réponse du mainteneur (Michael Kreil) dans la
  [discussion #88](https://github.com/orgs/versatiles-org/discussions/88) du 2026-10-03 :
  - le cas présenté (application tierce avec cache sur l'appareil) est jugé « fine » ;
  - **limite par IP** (« far above what a single device needs », puis `429`), **pas de quota** ;
  - les requêtes sont journalisées de façon anonymisée « to spot obvious misuse » ;
  - quand on dépend de la disponibilité : mettre un CDN devant, ou héberger soi-même.
- **[F]** La limite est de **200 req/s par IP, burst 1000** (nginx `limit_req_zone … rate=200r/s`,
  `limit_req_status 429`). Sources : [nginx.conf](https://github.com/versatiles-org/tiles.versatiles.org/blob/main/nginx/nginx.conf)
  et [issue #34](https://github.com/versatiles-org/tiles.versatiles.org/issues/34).
  L'en-tête `Retry-After` sur `429` a été ajouté (issue fermée le 2026-10-10). Il figure dans
  `access-control-expose-headers` ; nous n'avons pas provoqué de `429` pour le vérifier.
- **[F]** Usage automatisé ou bulk : **rien ne l'interdit explicitement**. La politique OSMF ou
  OpenFreeMap interdit la collecte en masse ; ici, aucun texte équivalent. Le guide traite
  explicitement le cas « build your own client or cache » (revalidation ETag, backoff 429).
- **[F]** User-Agent : **non exigé** par la doc. Le cas validé dans la discussion #88 envoie
  « a descriptive User-Agent identifying the app ».
- **[I]** Un aspirateur de tuiles, par exemple le pré-chargement de toute la France en z14
  (≈ 400 000 tuiles), entrerait dans l'« obvious misuse ». C'est d'autant plus clair que
  download.versatiles.org existe précisément pour ce besoin. Les tuiles se récupèrent donc
  à la demande uniquement, jamais en pré-chargement massif.

## 3. En-têtes HTTP constatés (`curl -sI`, 2026-10-30)

Tuile testée : `https://tiles.versatiles.org/tiles/osm/14/8299/5636`.

| En-tête | Valeur | Statut |
|---|---|---|
| `cache-control` | `public, max-age=21600, no-transform` (**6 h**) | [F] |
| `etag` | `W/"f12e4a6e…"` (faible) | [F] |
| `If-None-Match` | renvoie **`304`**, corps vide | [F] |
| `last-modified` | absent | [F] |
| `content-encoding` | `br` si `Accept-Encoding: br` (335 Ko au lieu de 625 Ko décompressé) | [F] |
| `access-control-allow-origin` | `*` | [F] |

- **[F]** ETag et `304` sont récents :
  [versatiles-rs#279](https://github.com/versatiles-org/versatiles-rs/issues/279), fermée le
  2026-10-04. Les docs et la discussion #88 disent encore « no validators ».

**Conséquence [I]** : `max-age` est une fraîcheur minimale. Les données ne changent qu'à
chaque release (§4) : un TTL de 7 jours (défaut d'osm-surface) puis une revalidation
`If-None-Match` (un `304` ne coûte quasi rien) est prudent ; 30 jours resterait raisonnable.

## 4. Données servies et fréquence de mise à jour

- **[F]** `/tiles/osm/` sert la variante **fusionnée `osm-landcover.versatiles`** (OSM + ESA
  WorldCover). Sources :
  [sources.json](https://github.com/versatiles-org/tiles.versatiles.org/blob/main/download/sources.json)
  et [Tilesets](https://docs.versatiles.org/basics/tilesets.html).
- **[F]** Le `tiles.json` actuel annonce `planetiler:buildtime` = 2026-06-18 et une
  réplication OSM au **2026-06-07**. Le `osm.20261027.versatiles` est publié sur download,
  mais **le serveur public sert donc des données OSM d'environ 5 mois**.
- **[F]** Les releases du planet `osm` paraissent tous les 2 à 4 mois (12 releases de
  20240101 à 20261027), annoncées par le flux RSS
  [feed-osm.xml](https://download.versatiles.org/feed-osm.xml).
- **[I]** Fraîcheur réelle de l'API : de quelques semaines à plus de 6 mois. C'est acceptable
  pour la typologie pont/tunnel/revêtement. Lire `tiles.json` donne la date des données, à
  journaliser.

## 5. Attribution

- **[F]** OSM : « © OpenStreetMap contributors », ODbL 1.0, obligatoire. Le schéma Shortbread
  est sous CC0 : rien à ajouter.
- **[F]** La variante hébergée est fusionnée, il faut donc **aussi** : « © ESA WorldCover
  project 2021 / Contains modified Copernicus Sentinel data (2021) processed by ESA WorldCover
  consortium » (CC BY 4.0). La chaîne exacte figure dans le champ `attribution` du `tiles.json`.
- **[F]** « © VersaTiles » est apprécié mais facultatif (discussion #88).
- **[I]** Pour notre service, mettre l'attribution dans la réponse API (champ `attribution`)
  et dans le README. Nous n'utilisons pas les couches landcover, mais nous les téléchargeons.
  Citer ESA ne coûte rien et évite toute ambiguïté.

## 6. Hébergement, financement, pérennité

- **[F]** Il s'agit d'un serveur unique : nginx avec cache en RAM disk et `versatiles serve`.
  Les données sont copiées depuis le bucket Cloudflare `cdn.versatiles.cloud`.
  download.versatiles.org est servi à part, depuis Cloudflare R2. Source :
  [README tiles.versatiles.org](https://github.com/versatiles-org/tiles.versatiles.org).
- **[F]** Financement : NLnet / NGI Zero Core (Commission européenne), GitHub Sponsors et
  Open Collective. Le mainteneur écrit que les dons « [don't] cover my expenses yet ». Les
  indisponibilités sont d'environ 1 h par an (≈ 99,99 %), sans garantie ni support 24/7.
- **[I]** Le projet repose sur un mainteneur principal et sur des subventions. L'API peut
  changer de politique sans préavis, d'où l'intérêt d'un repli local prêt (§7).

## 7. Basculer vers le fichier local

La doc officielle est claire : « If you rely on a stable map server, we highly recommend
hosting it yourself ».

**Déclencheurs de bascule [I]** (le premier atteint suffit) :

- un `429` ou un blocage observé, ou une demande du mainteneur ;
- un service ouvert à des tiers ou appelé par un batch (retraitement d'un historique de
  traces), plutôt qu'à la demande ;
- plus d'environ 10 000 tuiles téléchargées par jour, en régime établi, cache chaud ;
- un besoin de données plus fraîches que celles servies (écart avec la dernière release) ;
- une disponibilité devenue critique (pas de repli si l'API tombe et que le cache est froid).

**Méthode [F]** ([guide download](https://docs.versatiles.org/guides/download_tiles.html),
[converter](https://docs.versatiles.org/guides/converter.html)). L'outil `versatiles convert`
lit le planet distant par requêtes HTTP Range, sans télécharger les 62 Go :

```bash
versatiles convert --bbox-border 3 --bbox "-5.3,41.2,9.7,51.2" \
  --min-zoom 14 --max-zoom 14 \
  https://download.versatiles.org/osm.20261027.versatiles france-z14.versatiles
```

Le fichier obtenu se lit ensuite en local avec le crate Rust `versatiles` (même format de
tuiles, sans HTTP), ou se sert avec `versatiles serve` sous le même gabarit `{z}/{x}/{y}`.
Prendre la release datée plutôt que `osm.versatiles` donne des extraits reproductibles.
Variante : `wget -c` du planet entier (62,1 Gio). La page download demande « use this
download service only to download these files ».

**Taille estimée de l'extrait France [I]**. Calcul fait sur le vrai index de blocs du planet
`osm.versatiles` (66,6 Go = 62,1 Gio ; index lu par range, 70 Ko brotli). On suppose une répartition
uniforme des tuiles dans chaque bloc de 256×256, sur la bbox métropole -5,3/41,2/9,7/51,2
(qui déborde sur la Belgique, la Suisse, le nord de l'Espagne et la Manche), on obtient
**≈ 3,7 Go pour z14 seul** (suffisant pour nous) et ≈ 4,9 Go pour z0–14. Compter 4 à 6 Go.
Pour comparaison, z14 pèse 40,2 Go des 66,6 Go du planet.
