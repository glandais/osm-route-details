# Technique 07 — Configuration et `/v1/capabilities`

> Statut : conception technique, extraite de [T01](01-architecture.md) (ex-§7 et fin de l'ex-§5.4).
> Elle applique la conception générale ([01](../01-conception-generale.md), §8.1) et le contrat
> ([02](../02-contrat-interface.md), §3.2, §7). Module : `ord-server` (`Config`, `Capabilities`).
> Les paramètres par défaut indiquent leur origine : **[M]** mesuré ou éprouvé, **[C]** à calibrer
> au MVP. Références : `OS/` = osm-surface, `VC/` = vcyclist (T01, en-tête).

## 1. Sources et priorité

Défauts compilés < fichier TOML (`--config <chemin>` ou `ORD_CONFIG`, facultatif) < variables
d'environnement `ORD_<SECTION>__<CLÉ>` (ex. `ORD_LIMITS__MAX_POINTS`). Le doc 01 §8.1 impose que
**toute** la configuration passe par l'environnement : chaque clé du fichier a son équivalent
`ORD_…`, le fichier n'est qu'une commodité. Validation **au démarrage**, échec immédiat avec le
chemin de la clé (comme les `require` d'`ApiConfig`, `VC/api/.../ApiConfig.kt:44-54`). La
configuration effective est journalisée au démarrage, sans secret (osm-surface,
`OS/src/main.rs:19-20`).

| Section | Clés principales (défaut) |
|---|---|
| `server` | `listen_addr` (`0.0.0.0:8080`), `metrics_addr` (`0.0.0.0:9090`), `shutdown_grace_s` (= budget, 90) |
| `limits` | bornes d'**entrée** seulement (sortie non bornée en points, doc 02 §7) : `max_points` (16 000), `max_length_m` (200 000), `max_body_bytes` (8 Mio), `debug_allowed` (false) ; `densify_step_m` = 5 est une règle (`ElevationRules::station_step_m`), pas une clé |
| `limits.options` | bornes publiées (§2) : `sampling.step_m` ∈ [5 ; 1 000] m, défaut 25 m ; `elevation.smoothing_m` ∈ {0} ∪ [10 ; 1 000] m, défaut 0 |
| `budget` | `request_budget_s` (90, T01 §3.3), `max_in_flight` (nb CPU), `max_queued` (2 × in_flight), `compute_permits` (nb CPU), `per_request_fetch_concurrency` (10), `retry_after_*` (T01 §4.4) |
| `typology` | `source` = `versatiles_http` \| `versatiles_file` ; `url` (`https://tiles.versatiles.org/tiles/osm/{z}/{x}/{y}`, `OS/src/config.rs:49`) ou `path` ; `zoom` (14) ; `concurrency` (8) |
| `elevation.ign` | `enabled` (true), `wms_url` (Géoplateforme WMS-R), couches MNT / MNS, `zoom` (15), `rate_per_s` (20, cadence **du processus**, §3), `max_attempts` (6), `mode` = `direct` \| `sober` (doc 01 §5.1, défaut `direct`) |
| `elevation.mapterhorn` | `url` (`https://tiles.mapterhorn.com/{z}/{x}/{y}.webp`), `zoom` (12), `concurrency` (4), `version` |
| `coverage` | chemins des fichiers de couverture Mapterhorn (`coverage-index.pmtiles`, `coverage.pmtiles`) ; le contour LiDAR HD, qui sert aussi de zone France, est **embarqué** dans le binaire (T02 §7.1–7.2), pas configurable |
| `cache` | `dir` (`/var/cache/osm-route-details`), `disk_max_gb_ign` (20), `ttl` par source (VersaTiles 7 j **[M]**, IGN avec no-data 30 j **[C]**, Mapterhorn immuable), `negative_ttl_s` (60), `memory_elevation_mb` (512), `memory_streets_mb` (512), `offline` (false), `offline_streets` (false : tuiles de rues sur disque seulement, sans revalidation, altitude en ligne ; campagne de tuiles figée du banc) — détail T02 §5 |
| `http_client` | `user_agent`, `connect_timeout_s` (4 **[M]**, `OS/src/tiles.rs:38`), `fetch_timeout_s` (8 **[M]**, `OS/src/config.rs:27-28`) |
| `log` | `format` = `json` \| `text`, `filter` (`RUST_LOG` respecté) |

**User-Agent** : `osm-route-details/<CARGO_PKG_VERSION> (+https://github.com/glandais)` par défaut
(condition VersaTiles, courtoisie Mapterhorn et IGN ; explorations 05, 06), jamais vide. **Aucune
URL fournie par le client** (doc 02 §7, SSRF) : elles ne viennent que de la configuration.

## 2. Bornes des options

Décidées (doc 01 §10), publiées dans `/v1/capabilities` et inscrites au contrat (doc 02 §3.2) ;
vérifiées par `validate` (T01 §4.2, étape 6), toute valeur hors bornes → 400 `invalid_input` avec
son pointeur.

| Option | Bornes | Défaut |
|---|---|---|
| `sampling.step_m` (mode `distance`) | [5 ; 1 000] m (5 m = pas des stations internes) | 25 m |
| `elevation.smoothing_m` | 0 (désactivé) ou [10 ; 1 000] m | 0 |

## 3. Une seule instance

La cadence IGN (`elevation.ign.rate_per_s`, 20 req/s, moitié des 40 req/s/IP publiés) et les
budgets par hôte sont **locaux au processus** (T02 §4.2). La v1 se déploie donc en **une seule
instance** (doc 01 §8.1) ; la mise à l'échelle horizontale (plusieurs réplicas derrière une même IP,
qui devraient se partager la cadence) est hors v1. `Config::validate` ne peut pas le vérifier : la
règle est portée par la documentation de déploiement (T01 §10).

## 4. `/v1/capabilities`

Construit une fois au démarrage par `ord-server`, à partir de `Config`, `Rules`,
`SchemaCapabilities` et `DataCatalog`. Il est sérialisé en octets précalculés (T01 §4.1).

```rust
pub struct Capabilities {
    pub api_version: &'static str, pub rules_version: &'static str,
    pub limits: Limits,                 // max_points, max_length_m, max_body_bytes, station_step_m, densified_max
    pub defaults: RequestOptions, pub option_bounds: OptionBounds,   // step_m, smoothing_m (§2)
    pub enums: EnumCatalog,             // toutes les valeurs publiques, raisons et codes d'avertissement compris
    pub primary_zone: &'static str,     // "FR-metropole+corse" (contour : T02 §7.2)
    pub data: DataFeatures,             // ford_detectable, surface_detail, bridge_dsm, heuristics
    pub elevation_sources: Vec<DataVersion>, // IGN (zoom, mode direct|sober), Mapterhorn (zoom, role fallback)
    pub rules: Rules,                   // seuils des ruptures, gap_fill_max_m, presets, glo30_threshold_m,
                                        // correction des ouvrages (dont tunnel_series_gap_m)
}
```
Les règles de correction des ouvrages (`ElevationRules::structures`, `CorrectionRules`) y sont
publiées : sondes de culée et de portail, gardes de pente `max_tunnel_chord_grade` et
`max_bridge_chord_grade`, et `tunnel_series_gap_m` = 120 m (règles 2026.10.10) : tronçon à l'air
libre en deçà duquel deux tunnels OSM consécutifs forment une série, corrigée d'une seule droite
entre ses portails extérieurs (T04 §3.1). Ce ne sont pas des clés de configuration : les changer
monte `RULES_VERSION`.
`ford_detectable` = `caps.ford_ways || caps.ford_nodes` ; `surface_detail` = `caps.surface == Raw` ;
`bridge_dsm` = IGN actif ; `heuristics` = `ElevationRules::heuristics` (T04 §7.1).

## 5. Interfaces

```rust
// ord-server (désérialisé par figment) ; sous-structures passées aux crates qui les consomment
pub struct Config { pub server: ServerConfig, pub limits: LimitsConfig, pub budget: BudgetConfig,
                    pub typology: TypologySourceConfig, pub elevation: ElevationSourcesConfig,
                    pub coverage: CoverageConfig, pub cache: CacheConfig,
                    pub http_client: HttpClientConfig, pub log: LogConfig }
impl Config { pub fn load(args: &Args) -> Result<Config, ConfigError>; pub fn validate(&self) -> Result<(), ConfigError>; }
pub struct OptionBounds { pub step_m: (f64, f64) /* 5, 1000 */, pub step_m_default: f64 /* 25 */,
                          pub smoothing_m: (f64, f64) /* 10, 1000 ; 0 admis à part */ }
```

`DataConfig` (T02 §11) est construit par l'amorçage depuis `Config`.
