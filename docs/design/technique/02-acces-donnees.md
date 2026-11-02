# Technique 02 — Accès aux données, caches, sources et référentiel de couverture

> Statut : conception technique, aucun code. Exigences : [01-conception-generale.md](../01-conception-generale.md)
> (§4 Accès données, Caches, Adaptateurs, Référentiel de couverture ; §5.1, §6, §8.3, §8.6, §10, §11) et
> [02-contrat-interface.md](../02-contrat-interface.md). Références `chemin:ligne` : `vc:` = vcyclist 6.11
> (`M/` = `elevation/src/commonMain/kotlin/io/github/glandais/elevation/`), `os:` = osm-surface `44e5da6`.
> Les valeurs par défaut sont marquées **[mesuré]** (repris d'un projet qui l'a mesuré), **[publié]**
> (règle du fournisseur) ou **[à calibrer]**.

## 1. Périmètre et découpage

Cette section couvre cinq composants du doc 01 §3–4 :

| Composant (doc 01) | Ce qui est spécifié ici | Module Rust |
|---|---|---|
| **Accès données** | port unique `TileSource`, transports HTTP XYZ, WMS `GetMap`, `.versatiles`, PMTiles local ou distant ; budget par hôte, relances | `ord-data::{port, http, local}` ; types communs dans `ord-domain::tiles` |
| **Caches** | LRU mémoire décodé borné en octets avec dédoublonnage, cache négatif, cache disque | `ord-data::{memory, disk}` |
| **Adaptateur élévation** (couche basse) | décodage BIL32 et Terrarium ; grille, bilinéaire au centre, chaîne de repli, origine | codecs : `ord-adapters::{bil, terrarium}` ; lecture : `ord-data::elevation` (trait `TerrainReader` dans `ord-domain::elevation`) |
| **Adaptateur typologie** (couche basse) | décodage MVT de la couche `streets`, quantification ; couloir de tuiles | `ord-adapters::streets` ; `corridor_tiles` dans `ord-core::typology::corridor` |
| **Référentiel de couverture** | contour IGN, zone France, détection GLO-30, emprises des fichiers | `ord-data::coverage` (trait `CoverageRef` dans `ord-domain::coverage`) |

Hors de cette section : la traduction Shortbread vers le modèle neutre (section typologie), la fusion
et les heuristiques, la décision 200/503 (orchestrateur). Ce module **constate et rapporte**. Il ne
décide jamais d'un code HTTP.

Arborescence d'`ord-data` : `{port, http, local, disk, memory, elevation, coverage, attribution, config}` ; `RequestCtx` et `DataLog` sont dans `ord-domain::ctx` (carte des types : T01 §0).

## 2. Le port unique d'accès aux tuiles

### 2.1 Principe

Une seule abstraction de transport renvoie les **octets bruts** d'une tuile z/x/y ; décodage,
validation et poids mémoire relèvent d'un `TileCodec`. `TileReader<C>` compose : mémoire → négatif →
disque → transport → validation → écriture disque → mémoire. Support (`TileSource`) et format
(`TileCodec`) varient indépendamment (doc 01 §4, « choix structurant »).

Quatre issues **distinctes**, jamais confondues (doc 01 §4, os:`tiles.rs:210-229`) : **Data** (tuile
décodée) ; **Empty**, « rien ici » (VersaTiles 204, tuile manquante *dans* l'emprise d'un fichier,
tuile IGN hors contour), mise en cache ; **Absent**, pas de donnée *par construction* (hors emprise
d'un extrait, 404 Mapterhorn au-delà du zoom local), mise en cache, traduite en `*_missing` par les
chaînes ; **Failed** (réseau, 5xx/429 après relances, corps invalide, 404 VersaTiles), jamais mise en
cache comme valide, jamais lue comme « pas de rue ».

### 2.2 Interfaces

```rust
// ord-domain::tiles
#[derive(Clone, Copy, PartialEq, Eq, Hash)] pub struct TileKey { pub z: u8, pub x: u32, pub y: u32 } // Debug masqué (T01 §8)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub enum SourceId { Streets, IgnDtm, IgnDsm, Mapterhorn, MapterhornCoverage }
// ord-data::port
pub struct EntryMeta { pub etag: Option<String>, pub last_modified: Option<String>,   // persistés
                      pub fetched_at: SystemTime, pub server_max_age: Option<Duration> }
pub enum Fetched { Body { bytes: Bytes, meta: EntryMeta }, Empty, Absent, NotModified(EntryMeta) }
#[derive(Debug, Clone, thiserror::Error)]   // messages #[error] omis
pub enum FetchError { Transport(String), Http { status: u16 }, RateLimited { retry_after: Option<Duration> },
                      Timeout, Offline, Io(String) }
#[async_trait::async_trait]
pub trait TileSource: Send + Sync {
    fn descriptor(&self) -> &SourceDescriptor;            // id, zoom, gabarit, budget, attribution
    async fn fetch(&self, id: TileKey, revalidate: Option<&EntryMeta>) -> Result<Fetched, FetchError>;
}
// ord-domain::tiles (codecs implémentés dans ord-adapters)
pub trait TileCodec: Send + Sync + 'static {
    type Tile: Send + Sync + 'static;
    fn decode(&self, id: TileKey, bytes: &[u8]) -> Result<Self::Tile, DecodeError>;
    fn empty(&self, id: TileKey) -> Self::Tile;
    fn weight(&self, tile: &Self::Tile) -> u32;          // octets, pour le LRU
    fn ttl_class(&self, tile: &Self::Tile) -> TtlClass;  // ex. tuile IGN contenant du no-data
}
pub enum TtlClass { Immutable, Expires(Duration) }
pub enum TileOutcome<T> { Data { tile: Arc<T>, stale: bool }, Empty(Arc<T>), Absent, Failed(TileFailure) }
#[async_trait] pub trait TileStore<T>: Send + Sync {           // vue objet consommée par le pipeline
    async fn get_many(&self, keys: &[TileKey], ctx: &RequestCtx) -> Vec<TileOutcome<T>>;
    fn data_version(&self) -> DataVersion;
}
// ord-data::port
pub struct TileReader<C: TileCodec> { /* source, codec, DiskStore?, moka, négatif */ }
impl<C: TileCodec> TileReader<C> { pub async fn get(&self, id: TileKey, ctx: &RequestCtx) -> TileOutcome<C::Tile>; }
impl<C: TileCodec> TileStore<C::Tile> for TileReader<C> { /* get_many : get en parallèle, ordre des clés */ }
```

`async-trait` plutôt que les `async fn` natives : le port est un objet (`Arc<dyn TileSource>`) choisi
par configuration ; une allocation par appel est négligeable devant une lecture de tuile.

### 2.3 Contexte de requête et journal des échecs

Le port est partagé : les échecs sont collectés **par requête**, dans un contexte explicite
(équivalent du `TileFailureLog` coroutine, vc:`M/TileFailures.kt:10-31`).

```rust
// ord-domain::ctx ; Deadline : T01 §3.3 (Instant std + annulation), converti ici en tokio::time::Instant
pub struct RequestCtx { pub request_id: RequestId, pub deadline: Deadline, log: std::sync::Mutex<DataLog> }
pub struct DataLog {
    pub failures: Vec<TileFailure>,          // dédoublonné par (source, tuile), premier échec gardé
    pub stale_served: Vec<(SourceId, TileKey)>,
    pub oldest_read: BTreeMap<SourceId, SystemTime>,  // `read_at` de data_versions (T01 §9)
    pub counters: ReadCounters,              // hits mémoire / disque, téléchargés, octets, 304
}
pub struct TileFailure { pub source: SourceId, pub tile: TileKey, pub kind: FailureKind,
    pub status: Option<u16>, pub retry_after: Option<Duration> /* Retry-After du 503, T01 §4.4 */,
    pub message: String /* sans URL ni coordonnées */ }
pub enum FailureKind { Network, RateLimited, Server, Invalid, Deadline, Offline }
```

Une tuile du cache négatif est **réinscrite** au journal de chaque requête qui la rencontre
(vc:`M/TileFailures.kt:61-73`). Le journal nourrit `terrain_fallback`, `dsm_unavailable`, la décision
de panne de l'orchestrateur et les logs (sans coordonnées).

## 3. Implémentations du port

| Implémentation | Usage | Spécificités |
|---|---|---|
| `XyzHttpSource` | VersaTiles public, Mapterhorn public, tuiles de couverture Mapterhorn | gabarit `{z}/{x}/{y}` ; table de sémantique des statuts par source |
| `WmsGetMapSource` | IGN LiDAR HD MNT et MNS | `BBOX` = emprise de la tuile XYZ en EPSG:4326 |
| `VersatilesFileSource` | VersaTiles en production | fichier local en lecture seule, sans cache disque |
| `PmtilesSource` | `coverage-index.pmtiles`, `coverage.pmtiles`, Mapterhorn auto-hébergé (plus tard) | fichier local (mmap) ou HTTP Range ; répertoires en mémoire |

**Sémantique des statuts HTTP** (`StatusPolicy` du descripteur) :

| Statut | VersaTiles | Mapterhorn | IGN WMS |
|---|---|---|---|
| 200 | corps → décodage | corps → décodage | corps → décodage strict (longueur exacte) |
| 204 | Empty (os:`tiles.rs:210-213`) | Failed | Failed |
| 304 | NotModified | NotModified | — |
| 404 | **Failed** : URL fausse, jamais « pas de rue » (os:`tiles.rs:211-212`) | **Absent** : au-delà du zoom local (exploration 05 §2) | Failed |
| 429, 503 | relance (§4.2) | relance | relance |
| autre 5xx | relance | relance | Failed après relances |

**WMS IGN** (gabarit de vc:`M/IgnLidarHdLayer.kt:107-112`) :
`SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=<couche>&STYLES=normal&CRS=EPSG:4326&BBOX=<s,w,n,e>&WIDTH=256&HEIGHT=256&FORMAT=image/x-bil;bits=32`,
couches `IGNF_LIDAR-HD_MNT_ELEVATION.ELEVATIONGRIDCOVERAGE.WGS84G` et `…_MNS_…`
(vc:`M/IgnLidarHdLayer.kt:32-35`). `BBOX` latitude d'abord (WMS 1.3.0, vc:`M/ElevationSource.kt:110-119`) ;
EPSG:4326 car le service rééchantillonne EPSG:3857 une ligne sur deux (vc:`M/IgnLidarHdLayer.kt:102-105`).
Une tuile hors du contour IGN (§7.1) est **Empty** sans réseau, disque ni entrée de cache
(vc:`M/IgnLidarHdCoverage.kt:226-235`). MNT et MNS partagent l'hôte, donc le budget.

**`.versatiles` local** (crate `versatiles`) : ouvert une fois au démarrage (index des blocs, bbox,
zooms, date OSM). Hors emprise ou hors zoom → **Absent** ; manquante dans l'emprise → **Empty** (le
planet omet les tuiles vides). Corps décompressé (brotli/gzip), lectures en `spawn_blocking`, LRU
mémoire seul.

**PMTiles** (crate `pmtiles`) : en-tête et répertoires en mémoire, une plage par tuile. En distant,
l'`ETag` de l'archive est contrôlé à l'ouverture et au balayage ; s'il change, l'espace de noms disque
est abandonné. `coverage-index.pmtiles` (1,5 Mo) est chargé entier en mémoire.

## 4. Réseau : client, budget par hôte, relances

### 4.1 Client et User-Agent

Un seul `reqwest::Client` (rustls, HTTP/2, gzip, brotli), connexion 4 s, requête 8 s [mesuré,
os:`tiles.rs:37-42`, `config.rs:62`], `Accept-Encoding: br, gzip`. User-Agent identifiable avec
contact, `osm-route-details/<version> (+https://github.com/glandais)` (modèle os:`config.rs:64-68`),
demandé par VersaTiles et Mapterhorn (exploration 05 §2, 06 §2), envoyé partout. Aucune URL ne vient
d'un appelant : les gabarits sont dans la seule configuration (contrat §7, SSRF).

### 4.2 `HostBudget` : cadence, concurrence, backoff

Un budget **par hôte, pour tout le processus** : des `Arc` dans un registre indexé par hôte, comme
`HostPacers` (vc:`M/TileRequestPacing.kt:87-103`). Il est local au processus : le service est
déployé en **une seule instance** (doc 01 §8.1, T01 §10), seul moyen de tenir les 20 req/s vers
l'IGN sans coordination entre réplicas. Il réunit :

- un **pacer** : créneaux de départ espacés de `1/débit`, dans l'ordre d'appel
  (vc:`M/TileRequestPacing.kt:105-124`), écrit avec `tokio::time` (testable en temps virtuel) ;
  `governor` avec un burst de 1 serait équivalent ;
- un **sémaphore** de requêtes en vol (os:`tiles.rs:201-205`) ;
- des **relances** sur 429/503 (et autres 5xx et erreurs de transport en XYZ), 6 essais au total,
  après `Retry-After` ou 1, 2, 4… s (vc:`M/TileRequestPacing.kt:46-69`). **Ajout** : un 429 avec
  `Retry-After` repousse le prochain créneau de **tout l'hôte**, la limite IGN étant par IP ;
- un **plafond** : aucune relance au-delà de l'échéance de la requête en attente (§2.3). La tuile
  passe en `Failed(Deadline)` et le repli s'applique.

| Hôte | Débit | En vol | Origine |
|---|---|---|---|
| `data.geopf.fr` (MNT + MNS) | 20 req/s | 12 | [publié] 40 req/s/IP, moitié gardée comme vcyclist (vc:`M/IgnLidarHdLayer.kt:62-65`) ; en vol ≈ débit × latence [à calibrer] |
| `tiles.mapterhorn.com` (4) ; `single-archive-tiles.…`, `download.mapterhorn.com` (2) | sans cadence | 4 / 2 | « peu de requêtes simultanées » (exploration 05 §2) [à calibrer] |
| `tiles.versatiles.org` | sans cadence (200 req/s publiés) | 8 | [publié] exploration 06 §1, os:`config.rs:61` |

**429 VersaTiles = signal d'exploitation** : exploration 06 §1 fixe la bascule sur un extrait
`.versatiles` local dès le premier `429`. Chaque `429` d'un hôte public incrémente
`upstream_responses_total{host, status="429"}` et produit un log `warn`. Ce compteur sert de
déclencheur à l'alerte. La bascule elle-même reste manuelle, par la configuration.

Mode **hors ligne** (`cache.offline`, `ORD_CACHE__OFFLINE` ; `TILES_OFFLINE` d'os:`config.rs:29-30`) : disque seul, sinon `Failed(Offline)`.

## 5. Caches

### 5.1 LRU mémoire décodé et dédoublonnage

Un `moka::future::Cache<TileKey, Arc<C::Tile>>` par source, pondéré en octets par `TileCodec::weight`
et borné par `max_capacity` (os:`tiles.rs:54-62`). `try_get_with` fusionne les chargements
concurrents d'une tuile (os:`tiles.rs:81-86`) ; moka ne garde jamais une erreur. Le chargement tourne
dans une tâche **détachée** (`tokio::spawn`) : un demandeur qui abandonne (504, client parti) annule
son attente, pas le téléchargement, et la tuile atterrit quand même sur disque (équivalent de
vc:`M/LruCache.kt:16-22`, « l'annulation d'un propriétaire n'appartient qu'à lui »). Le **cache négatif** (`moka::sync::Cache<(SourceId, TileKey), TileFailure>`, TTL 60 s [mesuré,
vc:`M/TileFailures.kt:86-88`]) est consulté avant le disque : pendant cette minute, la tuile est
`Failed` sans requête et réinscrite au journal de chaque requête qui la rencontre.

### 5.2 Cache disque robuste

Toute source distante a un cache disque, obligatoire pour l'IGN (doc 01 §8.3) ; inutile pour une
source locale. Règles de vc:`cli/.../DiskCachedElevation.kt:21-129`,
vc:`elevation/src/jvmMain/.../TileStore.kt:41-91` et os:`tiles.rs:100-165`, dans cet ordre :

1. lire l'entrée ; en-tête, somme de contrôle ou **décodage** en échec → **supprimer** l'entrée
   (corrompue, d'une version antérieure, tronquée par un crash) et continuer comme un défaut ;
2. entrée fraîche (§5.3) → la servir ;
3. sinon interroger la source avec `If-None-Match` / `If-Modified-Since` ; sur `304`, réécrire
   l'en-tête (nouveau `fetched_at`) et servir l'ancien corps ;
4. corps reçu → **décoder d'abord** ; un corps invalide (page d'erreur en 200, XML WMS, portail
   captif) est `Failed(Invalid)` et n'atteint jamais le disque (vc:`DiskCachedElevation.kt:76-79`) ;
   sinon écriture atomique, puis service ;
5. échec → servir l'entrée **périmée** si elle décode (*stale-if-error*, os:`tiles.rs:150-164`),
   notée dans `DataLog::stale_served` ;
6. une erreur d'écriture disque est journalisée et **n'échoue jamais la lecture**
   (vc:`TileStore.kt:104-114`).

**Enveloppe** : un fichier par tuile, en-tête et corps ensemble, publiés par un seul `rename`.

```rust
struct EnvelopeHeader { issue: StoredIssue /* Body | Empty | Absent */, meta: EntryMeta,
    ttl: TtlClass /* fixé par le codec */, body_len: u32, body_xxh3: u64 }  // précédé d'un magic + version
```

Empty et Absent sont persistés : ce sont des réponses, pas des échecs. Le corps est l'octet brut
reçu, sans ré-encodage ; option `zstd` sans perte pour le BIL IGN, que le WMS ne compresse pas
(vc:`M/IgnLidarHdLayer.kt:48-51`), désactivée par défaut, gain [à mesurer].

**Écriture atomique** : `tempfile::NamedTempFile::new_in(dossier)` puis `persist` (`rename` dans le
même dossier), nom temporaire unique par écriture (os:`tiles.rs:333` ne varie que par processus).
Pas de `fsync` : une entrée tronquée par un crash est détectée par la somme de contrôle et purgée à
la lecture suivante. La robustesse vient de l'autoréparation, pas de la durabilité.

**Arborescence** : `{racine}/{espace}/{z}/{x}/{y}.tile`. L'espace de noms = source + produit + version,
en slug lisible suivi d'un hash FNV-1a du gabarit, stable entre compilations (os:`tiles.rs:254-279`) :
`ign-lidar-hd-mnt-z15-<hash>`, `mapterhorn-<version>-<hash>`, `versatiles-osm-<hash>`. La clé vient du
`TileKey`, jamais de l'URL, qui en WMS ne nomme pas la tuile (vc:`TileStore.kt:7-14`).

**Balayage** au démarrage puis toutes les 6 h, en `spawn_blocking` (os:`main.rs:50-69`,
`tiles.rs:284-327`) : temporaires de plus d'une heure, entrées à TTL plus vieilles que 2 × TTL,
espaces de noms orphelins (version ou gabarit changé), puis plafond d'octets **par source** en
supprimant les plus anciennes.

### 5.3 Durées de vie et revalidation

| Source | Classe | Revalidation | Origine |
|---|---|---|---|
| VersaTiles HTTP | TTL 7 j (≥ `max-age` 6 h du serveur) | `If-None-Match` (ETag faible, `304`) | [publié/mesuré] exploration 06 §3, os:`config.rs:16-18,58` |
| Mapterhorn HTTP | **immuable** dans un espace versionné (doc 01 §8.3) | aucune ; espace changé si la version publiée change (§9) | doc 01 §8.3, exploration 05 §2 |
| IGN : sans no-data / avec no-data | immuable / TTL 30 j | aucune / refetch simple (pas de validateur connu) | doc 01 §8.2–8.3 [30 j à calibrer] |
| Couverture Mapterhorn (MVT) | TTL 7 j | `If-None-Match` ou `If-Modified-Since` | exploration 05 §1.6, §2 |
| `coverage-index.pmtiles` | rechargé chaque semaine | comparaison d'ETag | exploration 05 §1.7 |
| Échec (mémoire seulement) | 60 s | — | vc:`M/TileFailures.kt:88` |

### 5.4 Empreinte mémoire et dimensionnement

| Tuile décodée | Représentation | Poids |
|---|---|---|
| IGN 256 × 256 (entièrement no-data : variante `NoData`, ≈ 0) | `Box<[f32]>`, NaN = no-data | 256 Kio |
| Mapterhorn 512 × 512 | `Box<[f32]>` | 1 Mio |
| VersaTiles `streets` | lignes en unités globales, `[f64; 2]` | 16 o par sommet + 32 o par ligne + attributs internés (os:`tiles.rs:56-59`) ; **≈ 3 Mo** pour une tuile urbaine dense : hypothèse, à confirmer par le test d'empreinte du doc 01 §8.5 |

Corps MVT sur disque : 38 o à 470 Ko (médiane 63 Ko, 100 tuiles du cache d'osm-surface). Le `f32`,
au lieu de l'RGBA ré-encodé en Terrarium de vcyclist (vc:`M/Bil32.kt:23-29`), divise la mémoire par
quatre et évite la quantification au 1/256 m.

**200 km à froid** : MNT IGN ≈ 400 tuiles, 100 Mio (≈ 2 tuiles/km : 264 tuiles pour 130 km en
Savoie, vc:`M/ElevationSource.kt:84-91`, exploration 01 §6.3 ; T01 §3.3 retient le même chiffre) ;
MNS sur les ponts seulement au MVP, autant que le MNT en V1 (heuristiques) ; Mapterhorn z12 ≈ 25
tuiles, 25 Mio ; `streets` ≈ 90 tuiles (40 pour 90 km, exploration 02 §3), 50 à 270 Mo.

Défauts [à calibrer] (clés de T07 §1) : `cache.memory_elevation_mb = 512` (somme MNT + MNS + Mapterhorn),
`cache.memory_streets_mb = 512` (osm-surface : 256, os:`config.rs:60`). Les `Arc` tenus par les requêtes en
cours s'ajoutent au LRU : pic ≈ caches + N_requêtes × jeu de travail, donc la file bornée de
l'orchestrateur borne aussi la mémoire. Disque : `cache.disk_max_gb_ign = 20` (256 Ko par tuile) ;
`streets` ≈ 11 Mo pour 185 km (exploration 02 §3).

## 6. Sources d'élévation (couche basse de l'adaptateur élévation)

### 6.1 Décodage

- **BIL32 IGN** : exactement 256 × 256 × 4 octets, sinon `DecodeError` (avec mention « XML » si le
  corps commence par `<`, vc:`M/Bil32.kt:41-51`) ; f32 little-endian, lignes nord → sud ; ≤ −9000 ou
  NaN = no-data (vc:`M/Bil32.kt:17-21,61`). `all_valid` / `all_nodata` fixent la `TtlClass` et la
  variante `NoData`.
- **Terrarium WebP** : `image-webp` (Rust pur), `h = R·256 + G + B/256 − 32768`, 512 × 512
  (vc:`M/ElevationProviderConfig.kt:22`). Pas de no-data dans le format : RGB `(0,0,0)`
  (vc:`M/Tile.kt:13-16,43`) et toute valeur hors [−500 ; 9 000] m [à calibrer] sont du no-data.

```rust
pub enum ElevationTile { NoData, Grid { size: u16, values: Box<[f32]>, all_valid: bool } } // ord-domain::tiles
pub struct BilCodec;        // ord-adapters::bil        // TileCodec<Tile = ElevationTile>
pub struct TerrariumCodec;  // ord-adapters::terrarium, TileCodec<Tile = ElevationTile>
```

### 6.2 Échantillonnage : bilinéaire au centre du pixel

Un pixel `i` vaut pour son **centre** `i + ½`, toutes sources (doc 01 §10 ;
vc:`M/ElevationCalculator.kt:15-31`). Pour un point : (1) pixels flottants `(px, py)` au zoom de la
source, x linéaire en longitude, y en Mercator pour Terrarium et **linéaire en latitude dans la
`BBOX`** pour l'IGN (raster EPSG:4326 ; écart < 0,005 px à z15, vc:`M/IgnLidarHdLayer.kt:102-105`) ;
(2) `fx = px − ½`, `fy = py − ½`, `x0 = ⌊fx⌋`, `y0 = ⌊fy⌋`, poids `dx`, `dy`
(vc:`M/ElevationCalculator.kt:91-109`) ; (3) 4 voisins, débordement vers la tuile adjacente et
blocage au bord du monde (vc:`M/ElevationFunctions.kt:33-80`) ; (4) un voisin no-data, **même de
poids nul**, rend le point no-data (vc:`M/ElevationCalculator.kt:22-25`).

**Garde-fou de plausibilité des couches IGN** (MNT et MNS, règles 2026.11.7, décision de
l'utilisateur du 2026-11-02 ; `ElevationRules::ign_plausibility`, `ord-data::elevation`). Le
rééchantillonnage du WMS mêle le no-data `−9999` aux pixels valides voisins, qui sortent du format
comme des altitudes : sur `perso-53`, une bande de deux lignes de pixels à −297 … −953 m sur un
terrain à 340 m (à −945 m, la part de no-data vaut 1/8), bordée de pixels à 20–200 m, rendue
−944 m en `dem` (échec E1). Deux cas sont lus comme du no-data, et le point se replie comme sur un
vrai no-data (§6.3, cas 3 ; `NoData` pour le MNS) :

- un pixel sous le **plancher de −50 m** : le sol le plus bas de la couverture LiDAR HD est vers
  −3 m (Moëres, Camargue) ; un pixel mêlé passe sous −50 m dès 0,5 % de no-data au niveau de la mer ;
- un **carré d'interpolation mêlé** : ses 4 voisins valides s'écartent de plus de 20 m (MNT ;
  50 m sur le MNS) **et** un pixel no-data ou sous le plancher est dans la couronne de 2 pixels
  autour du carré, lue sur les tuiles déjà chargées (une tuile absente, en échec ou non planifiée
  n'y compte pas). Mesure sur le cache de banc du 2026-11-02 (11 892 tuiles MNT, 11 693 MNS) :
  les carrés d'écart > 20 m (MNT) ou > 50 m (MNS) proches d'un no-data en sont tous à 1 ou
  2 pixels (563 et 522 carrés ; 78 tuiles MNT et 75 MNS ont des valeurs entre −9000 et −50 m),
  aucun à 3–6 pixels ; les falaises, toits et arbres, loin du no-data, ne sont pas touchés. Sur le MNS,
  le bâti et les arbres au bord de l'eau (no-data) font des écarts de 20 à 50 m à 2–6 pixels du
  no-data : seuil de 50 m.

Le seuil de no-data du décodeur (§6.1) ne change pas : une tuile qui n'a que des valeurs mêlées
reste `all_valid` pour la `TtlClass`. Mapterhorn n'a pas ce garde-fou (son décodeur borne déjà les
valeurs). Le repli se lit dans `quality.terrain` (plage `mapterhorn_measured`) et dans les points
de rupture `terrain_source_change` ; aucune altitude rejetée n'atteint le profil, ni donc un point
inséré.

Lecture en trois phases, sans `await` par pixel : **planifier** les tuiles touchées (voisines
comprises à moins d'un demi-pixel d'un bord), **charger** par `get_many` (concurrence du
`HostBudget`), **échantillonner** de façon synchrone en O(N) points densifiés.

### 6.3 Chaîne de repli et origine (port de vc:`M/ElevationProvider.kt:157-171`)

```rust
// ord-domain::elevation (implémentation : ord-data::elevation)
pub enum TerrainOrigin { IgnLidarHd, MapterhornMeasured, Mapterhorn, None }   // = doc 02 §4.5
pub enum DemKind { Dtm, Glo30, Seam /* couture ≈ 150 m, interne */, Unknown }
pub enum Missing { NoData /* absent par construction */, Failed /* panne après repli */ }
pub struct TerrainSample {
    pub ele: Option<f32>, pub origin: TerrainOrigin,
    pub fallback_after_error: bool,  // repli subi (tuile IGN en échec), pas hors couverture
    pub dem_kind: DemKind,           // pour l'adaptation du seuil de D+ (GLO-30)
    pub missing: Option<Missing>,    // Some ⇔ ele = None : elevation_missing ou panne
}
pub enum DsmSample { Value(f32), NoData, Unavailable }
pub enum TerrainMode { Full, Sober }   // Sober : IGN lu seulement sur l'emprise des ponts
#[async_trait::async_trait]
pub trait TerrainReader: Send + Sync {
    /// Sol, avec repli Mapterhorn point par point.
    async fn sample_terrain(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<TerrainSample>;
    /// MNT IGN seul, sans repli (culées en mode sobre, contrôle du MNS) : None = no-data ou échec.
    async fn sample_ign_dtm(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<Option<f32>>;
    /// MNS IGN seul, **jamais de repli** (doc 01 §4, §5.5 invariant 10).
    async fn sample_dsm(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<DsmSample>;
    /// Taille de pixel au sol de la source lue (sondes de culée, T04 §3.1).
    fn pixel_size_m(&self, p: LatLon, origin: TerrainOrigin) -> f64;
}
```

Pour chaque point de `sample_terrain` : (1) en mode `Full`, tuile dans le contour IGN → MNT IGN,
origine `IgnLidarHd` ; (2) tuile IGN `Failed` → point **entier** relu dans Mapterhorn,
`fallback_after_error = true` ; (3) no-data IGN sur un des 4 voisins → point **entier** relu dans
Mapterhorn, jamais de mélange (vc:`M/ElevationCalculator.kt:33-43`), repli normal ; (4) valeur
Mapterhorn → origine par le référentiel (§7.3) ; tuile Mapterhorn `Absent` → `None`, `ele = None`, `missing = NoData` ;
`Failed` → `None`, `missing = Failed`, échec journalisé (l'orchestrateur décide de la panne). Jamais 0 m : le 0 m de
vcyclist (vc:`M/ElevationCalculator.kt:39-41`) devient une altitude absente (`ele_method: none`).

**Zoom Mapterhorn configurable** (défaut z12, doc 01 §5.1) : à z > 12, un 404 signifie « zoom local
dépassé » (exploration 05 §2) ; le lecteur redescend d'un zoom à la fois jusqu'à une tuile, le 404
étant persisté `Absent` immuable. Un 404 **à z13** marque aussi la zone « GLO-30 seul » (exploration
05 §1.5, fiable dans ce sens seulement).

`sample_dsm` n'a aucun repli : une tuile MNS en échec donne `Unavailable`, et la fusion interpole
avec `dsm_unavailable`. En V1, les heuristiques consomment `sample_dsm` et `sample_ign_dtm`.

## 7. Référentiel de couverture

### 7.1 Contour IGN (filtre réseau)

Les deux polygones grossiers de vcyclist (métropole, Corse ; 5 à 20 km au-delà de la frontière et
de la côte, vc:`M/IgnLidarHdCoverage.kt:7-125`), testés contre l'emprise de la tuile (coins,
sommets, croisements, vc:`M/IgnLidarHdCoverage.kt:130-178`), résultat mémoïsé par tuile. Simple filtre
devant le réseau, **jamais** une affirmation sur la donnée.

**Provenance et licence.** Fichier source :
`VC/elevation/src/commonMain/kotlin/io/github/glandais/elevation/IgnLidarHdCoverage.kt`
(constantes `METROPOLE`, polygone `lon, lat` tracé à la main dans le sens horaire depuis la mer du
Nord, et `CORSICA`, boîte 8,40–9,70° E × 41,25–43,15° N ; introduites par le commit vcyclist
`ee2b214` du 2026-10-14). vcyclist est sous **licence Apache 2.0** (`VC/LICENSE`) ; les sommets sont
recopiés une fois dans `ord-data::coverage` (copie gelée, sans dépendance de fichiers entre projets,
décision Q11), avec en tête du module la mention de provenance et de licence Apache 2.0 exigée pour
une reprise. Aucune donnée tierce (Natural Earth, Admin Express) n'entre dans ces polygones.

### 7.2 Zone France (qualité maximale)

**Décision** (doc 01 §10) : la zone France **est** le contour du §7.1, sans polygone de plus. Elle
sert à `outside_primary_zone` et à `mapterhorn_measured`. Test point dans polygone
(`geo::Contains`) après pré-filtre par bbox, sur les mêmes sommets embarqués. Conséquence assumée :
le contour est généreux (5 à 20 km au-delà de la frontière et de la côte) ; une bande de Suisse, de
Belgique ou du Kent est « en zone France » (pas d'`outside_primary_zone`, origine Mapterhorn
`mapterhorn_measured` hors GLO-30), et une bande côtière en mer aussi. Le bord exact de la France
n'est pas cherché : la zone ne qualifie que la provenance.

### 7.3 Détection GLO-30 et origine Mapterhorn

Règle de fusion de Mapterhorn (exploration 05 §1.3) : sources triées par maxzoom décroissant puis
par nom ; maxzoom = plus petit z dont le pixel Mercator (512 px) est plus fin que la résolution,
plancher 12. (1) **Index** `coverage-index.pmtiles` en mémoire : tuile z12 ne listant que `glo30` →
`Glo30`. (2) Sinon **polygones** `coverage` (MVT, attribut `source`), lus par le même port
(`XyzHttpSource` sur `single-archive-tiles.mapterhorn.com`, ou `PmtilesSource` par Range) à **z12 par
défaut** (une tuile par tuile d'élévation, assez pour un référentiel « à gros grain », doc 01 §4 ; z14
[à calibrer]) ; la première source, dans l'ordre de priorité, dont un polygone contient le point fixe
le modèle : `Glo30` si `glo30`, `Dtm` sinon. (3) Sans fichiers de couverture : seul le 404 à z13
(§6.3) détecte « GLO-30 seul », sinon `Unknown`.

Origine Mapterhorn : `MapterhornMeasured` si le point est en zone France **et** `dem_kind ∉
{Glo30, Seam}`, `Mapterhorn` sinon : exclusion sur GLO-30 **et** sur ses coutures, décidée (doc 01
§5.1, §10 ; contrat §4.5), car ni l'un ni l'autre n'est un MNT mesuré.

**Couture** (exploration 05 §1.6) : un point de MNT national à moins de `seam_m` = 150 m
[mesuré, mélange documenté] du bord d'un polygone `glo30` adjacent est marqué `DemKind::Seam`. Il
compte comme GLO-30 pour le D+ (T04 §5.4) et n'est jamais exposé dans le contrat. Une couture
entre deux MNT nationaux reste `Dtm`, parce que les deux sources sont des MNT.

**Issue #340** (fusion des polygones qui ne garde qu'un enregistrement) : l'index fait foi pour la
**présence**. Au chargement, et pour chaque tuile z12 lue, une source que l'index liste sans
qu'aucun de ses polygones ne touche la tuile est déclarée « polygones douteux » sur cette tuile.
Le point y prend le `DemKind` de la source prioritaire de l'index, `Glo30` si seule `glo30`
reste, sinon `Dtm`. L'anomalie est journalisée et comptée (`coverage_anomalies_total{source}`).
Le contrôle par `bounds.csv` (exploration 05 §1.6) est fait une fois par `tools/fetch-coverage`.

### 7.4 Emprises des sources configurées

`Extent::World | Bbox(Rect) | Polygons(MultiPolygon)`, lu dans les métadonnées `.versatiles`, l'en-tête
PMTiles ou la configuration. Il permet de répondre « hors emprise » (`Absent`) avant toute lecture, et
fournit à l'orchestrateur le test « aucune donnée d'aucune source » (422).

```rust
// ord-domain::coverage ; implémentation ord-data::coverage (hull, zone, index GLO-30, polygones, emprises)
#[async_trait] pub trait CoverageRef: Send + Sync {
    fn ign_may_have_data(&self, tile: TileKey) -> bool;
    fn in_primary_zone(&self, p: LatLon) -> bool;          // zonage (ord-core::coverage → CoverageRanges)
    async fn dem_kinds(&self, pts: &[LatLon], ctx: &RequestCtx) -> Vec<DemKind>;  // appelé par TerrainReader
    fn extent(&self, source: SourceId) -> &Extent;
}
```

## 8. Typologie : VersaTiles Shortbread z14 (couche basse de l'adaptateur typologie)

- **Source** : `https://tiles.versatiles.org/tiles/osm/{z}/{x}/{y}` (os:`config.rs:49`) au MVP,
  `.versatiles` en production ; z14, extent 4096, 1 unité ≈ 0,41 m à 47° N : géométries quantifiées
  et simplifiées (exploration 02 §3). `tiles.json` lu au démarrage (date OSM, `planetiler:buildtime`,
  `attribution`, os:`trace.rs:335-377`).
- **Décodage** (`ord-adapters::streets`) : prost écrit à la main depuis `vector_tile.proto` v2.1, sans crate MVT
  (os:`mvt.rs:9-66`) ; seul le nom des couches est lu pour sauter tout sauf `streets`
  (os:`mvt.rs:171-178`) ; gzip accepté (`flate2`, os:`mvt.rs:162-170`) ; corps vide = tuile vide ;
  commandes zigzag et `ClosePath` (os:`mvt.rs:243-286`) ; passage en **unités globales** (`Grid`,
  os:`geo.rs:19-69`) puis **découpe au carré exact de la tuile** avec un point de coupe canonique, pour
  que deux voisines calculent le même point (os:`mvt.rs:288-330`) ; attributs internés
  (os:`mvt.rs:188-197`), clés d'os:`mvt.rs:224-236`.
- **Couloir** (`ord-core::typology::corridor`, fonction pure) : tuiles à moins de `rayon` de la trace, échantillonnée tous les 100 m avec un décalage
  3 × 3 de ±rayon (os:`geo.rs:91-116`) ; `rayon = search_radius + 60 m = 100 m` [mesuré,
  os:`trace.rs:387`]. La chaîne typologie étend le couloir (ouvrage qui en sort) par le même `get_many`.

```rust
// ord-domain::geo
pub struct Grid { pub zoom: u8, pub extent: u32 }
// StreetTile, StreetLine, WayAttrs (modèle neutre) et StreetSchemaAdapter : ord-domain::street, T03 §3
// ord-adapters::streets
pub struct StreetsCodec<A: StreetSchemaAdapter> { adapter: A }  // TileCodec<Tile = StreetTile>, MVT → A::decode
// ord-core::typology::corridor
pub fn corridor_tiles(grid: Grid, pts: &[LatLon], radius_m: f64) -> Vec<TileKey>;
```

Les sommets restent en `[f64; 2]` unités globales (T03 §3.1) ; la quantification au 1/16 d'unité
n'est appliquée qu'aux clés de nœud du graphe (T03 §4.2).

## 9. Attributions et versions des données

Chaque `SourceDescriptor` porte son `Attribution`. Le registre ne compose que les sources
**configurées**, et il cite la source et son repli (doc 01 §8.6, contrat §4.7).

| Source | Mention courte | `/v1/attribution` |
|---|---|---|
| VersaTiles `streets` | « © OpenStreetMap contributors » (ODbL) | + « © ESA WorldCover project 2021 / Copernicus Sentinel » (CC BY 4.0), chaîne lue dans `tiles.json` (exploration 06 §5) |
| IGN MNT et MNS | « © IGN — LiDAR HD, Licence Ouverte Etalab 2.0 » (vc:`M/IgnLidarHdLayer.kt:66-70`) | lien geoservices.ign.fr/lidarhd |
| Mapterhorn | « Mapterhorn and its sources » + lien mapterhorn.com/attribution | liste complète tirée d'`attribution.json`, téléchargé au démarrage et mis en cache disque |
| OpenFreeMap (V1.x, si actif) | « © OpenMapTiles » (CC BY 4.0) en plus d'OSM | idem |

```rust
// ord-domain::catalog ; SourceRole, DataVersion, DataVersions : définis en T01 §9
pub struct Attribution { pub source: SourceId, pub short: String, pub licence: String, pub url: Option<String> }
pub trait DataCatalog: Send + Sync {
    fn attributions(&self) -> Vec<Attribution>;     // sources configurées seulement
    fn data_versions(&self, log: &DataLog) -> DataVersions; // sources utilisées ; read_at IGN tiré du log
    fn readiness(&self) -> Readiness;               // /health/ready : fichiers ouverts, dernier succès par hôte
}
```

Versions : Mapterhorn depuis `tiles.mapterhorn.com/tilejson.json` (lu au démarrage puis chaque
jour, entre dans l'espace de noms disque, §5.2) ; IGN = date de lecture (`read_at`) ; VersaTiles =
date OSM de `tiles.json` ou des métadonnées du fichier.

## 10. Concurrence, performance, cas limites

- **Runtime** : tokio multi-thread ; décodage (WebP, MVT, BIL) et lectures `.versatiles` en
  `spawn_blocking` ; tuiles immuables partagées sans verrou par `Arc` (vc:`M/Tile.kt:8-11`). Les deux
  chaînes lisent en parallèle ; budgets par hôte et dédoublonnage évitent toute double lecture.
- **Coût à froid, 200 km** : ≈ 400 tuiles MNT à 20 req/s, soit ≈ 20 s de cadence (contrat §7 : « de
  l'ordre de 20 s pour 130 km »), plus le MNS des ponts ; Mapterhorn (≈ 25) et `streets` (≈ 90) sur
  d'autres hôtes, en parallèle. L'IGN est le chemin critique ; le budget de requête de 90 s (T01
  §3.3) tient au MVP. En V1, le MNS lu sur tout le terrain mesuré (heuristiques) double le volume
  IGN : ≈ 800 tuiles pour 200 km, ≈ 40 s à 20 req/s, toujours sous le budget, sans conditionner
  les heuristiques à l'état du cache (déterminisme, doc 01 §3) ; chiffre à remesurer en V1.
  À chaud : quelques dizaines de ms (osm-surface < 100 ms, exploration 02 §7).
- **Cas limites** : pôles et antiméridien bornés en amont (plage Web Mercator) et blocage au bord du
  monde (§6.2) ; point sur un bord de tuile → tuile est/sud, voisines chargées ; tuile IGN dans le
  contour mais 100 % no-data (mer, ZICAD) → `NoData` 30 j, points relus dans Mapterhorn ; XML d'erreur
  WMS en 200 → refusé par la longueur ; `.versatiles` illisible → `/health/ready` en échec, jamais une
  réponse vide ; disque plein → écriture ignorée, lecture servie ; crash pendant une écriture →
  temporaire balayé, entrée tronquée purgée.
- **Métriques** (`metrics`) : hits mémoire/disque, téléchargements, `304`, périmées servies, échecs
  par type, attente du pacer, octets LRU et disque par source ; jamais de coordonnées.

**Crates** : `tokio` (runtime, temps virtuel en test) ; `reqwest` (rustls, HTTP/2, brotli, gzip,
éprouvé dans osm-surface) et `bytes` ; `moka` (LRU asynchrone pondéré, fusion des chargements) ;
`async-trait` (port en objet) ; `thiserror` ; `prost` + `flate2` (MVT, repris d'osm-surface) ;
`image-webp` (Terrarium, Rust pur) ; `versatiles` ; `pmtiles` (local et Range) ; `geo`
(point-dans-polygone) ; `tempfile` (écriture atomique) ; `xxhash-rust` (somme de contrôle) ; `zstd`
(option BIL) ; `serde`/`serde_json` (en-tête, TileJSON, `attribution.json`) ; `metrics`, `tracing`.

## 11. Interfaces (récapitulatif pour l'alignement)

Propriétaires exacts : carte des types, T01 §0.

**Exposé** — `ord-domain` (définis ici) : `TileKey`, `SourceId`, `TileCodec`, `TtlClass`,
`TileOutcome<T>`, `TileStore<T>`, `DecodeError`, `RequestCtx`, `DataLog`, `TileFailure`,
`FailureKind` · `ElevationTile`, `TerrainReader`, `TerrainSample`, `Missing`, `TerrainOrigin`,
`DemKind`, `DsmSample`, `TerrainMode` · `CoverageRef`, `Extent` · `DataCatalog`, `Attribution`,
`Readiness`, `Grid`. `ord-data` : `TileSource`, `Fetched`, `EntryMeta`, `FetchError`,
`TileReader<C>`, `DataConfig`. `ord-adapters` : `BilCodec`, `TerrariumCodec`, `StreetsCodec<A>`.

**Consommé** : `LatLon`, `Deadline`, `RequestId`, `DataVersion`, `DataVersions`, `SourceRole` (T01) ;
`StreetTile`, `StreetSchemaAdapter` (T03 §3, pour `StreetsCodec`) ; `DataConfig`, construit par
l'amorçage depuis `Config` (T07 §5) ; la façade `metrics` (Observabilité).

## 12. Points de tension

1. **Mapterhorn « immuable » (doc 01 §8.3) contre une URL publique non versionnée** : la version du
   TileJSON entre dans l'espace de noms disque ; si le TileJSON n'en publie pas (à vérifier), il
   faudra revalider par `If-Modified-Since` (30 j) et l'immuabilité ne sera qu'approchée.
2. **Relances contre échéance** : 6 essais (1, 2, 4… s) font jusqu'à 31 s. Coupées à l'échéance, elles
   transforment plus souvent qu'avec vcyclist un ralentissement IGN en `terrain_fallback`.
3. **Déterminisme en lecture distante** : *stale-if-error*, TTL des tuiles IGN no-data et
   revalidations VersaTiles peuvent changer une réponse ; conforme au `data_versions` « indicatif »
   (contrat §6), mais `stale_served` doit au moins être journalisé.
4. **« ≈ 3 Mo par tuile décodée »** n'est pas mesuré ; le dimensionnement des LRU (§5.4) et le pic
   mémoire en dépendent, à fixer par le test d'empreinte.
5. **Coutures Mapterhorn** (≈ 150 m de mélange) : non exprimables dans le contrat (pas d'origine
   « mixte »), gardées internes ; la provenance y est approchée.
