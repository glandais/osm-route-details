//! Le service appelé **en processus** (T05 §5.1) : routeur axum complet, couches comprises, sur le
//! cache de banc. Aucune coordonnée n'est écrite par ce module.

use std::path::Path;

use anyhow::{bail, Context};
use axum::body::Body;
use axum::http::{header, Method, Request};
use http_body_util::BodyExt;
use ord_data::elevation::PixelConvention;
use ord_server::config::Config;
use ord_server::{router, AppState, Wiring};
use serde_json::{json, Value};
use tower::ServiceExt;

/// Point envoyé au service.
#[derive(Clone, Copy, PartialEq)]
pub struct Pt {
    pub lat: f64,
    pub lon: f64,
}

impl std::fmt::Debug for Pt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pt(..)")
    }
}

/// Mode de sortie dans lequel le banc lit le service (README §2.2, étape 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchMode {
    /// Requêtes telles quelles (`distance` au pas de la grille, `input`).
    Reference,
    /// Mode par défaut `input_with_breakpoints` : une requête `distance` est envoyée dans ce mode
    /// et sa sortie rééchantillonnée sur la grille demandée (étape 3) ; une requête `input` aussi,
    /// réduite à ses points d'entrée.
    Breakpoints,
}

impl BenchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reference => "distance",
            Self::Breakpoints => "input_with_breakpoints",
        }
    }
}

/// Bilan d'une réponse `input_with_breakpoints` : longueur, points d'entrée et insérés, plus
/// grand nombre de points insérés sur une fenêtre glissante d'un km. Aucune position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InsertedStats {
    pub length_m: f64,
    pub input: usize,
    pub inserted: usize,
    pub max_per_km: usize,
}

#[derive(Clone)]
pub struct Service {
    app: axum::Router,
    state: AppState,
    mode: BenchMode,
    stats: std::sync::Arc<std::sync::Mutex<Vec<InsertedStats>>>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Service(..)")
    }
}

/// Variante du service mesurée par le banc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant {
    pub convention: PixelConvention,
    /// IGN LiDAR HD lu (défaut du service) ; `false` : Mapterhorn seul (mesures de l'incrément 1,
    /// test de méthode `vc-strava`, correction du demi-pixel sur Mapterhorn).
    pub ign: bool,
}

impl Variant {
    /// Le service tel qu'il est déployé.
    pub const DEFAULT: Self = Self { convention: PixelConvention::Center, ign: true };
    /// Mapterhorn seul, centres de pixel.
    pub const MAPTERHORN: Self = Self { convention: PixelConvention::Center, ign: false };
}

/// Campagne de tuiles de rues verrouillée pour ce processus (`fill-cache` d'un manifest qui déclare
/// une campagne datée `tuiles_*`) : tuiles de rues lues sur disque seulement, entrées périmées
/// comprises, jamais téléchargées ni revalidées ; l'altitude reste en ligne.
static STREETS_LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Verrouille les tuiles de rues pour le reste du processus (`fill-cache`).
pub fn lock_streets() {
    STREETS_LOCKED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Tuiles de rues verrouillées dans ce processus ?
pub fn streets_locked() -> bool {
    STREETS_LOCKED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Configuration du service pour le banc : cache disque = cache de banc ; tuiles de rues sur
/// disque seulement si la campagne est verrouillée (`lock_streets`).
pub fn bench_config(cache_dir: &Path, offline: bool, variant: Variant) -> Config {
    bench_config_with(cache_dir, offline, variant, streets_locked())
}

/// `bench_config`, verrou des tuiles de rues explicite.
pub fn bench_config_with(cache_dir: &Path, offline: bool, variant: Variant, streets_locked: bool) -> Config {
    let mut config = Config::default();
    config.cache.dir = cache_dir.to_path_buf();
    config.cache.offline = offline;
    config.cache.offline_streets = streets_locked;
    config.elevation.ign.enabled = variant.ign;
    // VersaTiles public : peu de requêtes à la fois, jamais de rafale (exploration 06).
    config.typology.concurrency = 2;
    config
}

/// Tuiles de rues absentes du cache d'une campagne verrouillée (échec `Offline` d'une lecture en
/// ligne) : refus bruyant, aucun téléchargement n'a eu lieu.
pub fn ensure_streets_cached<T>(outcomes: &[ord_domain::tiles::TileOutcome<T>]) -> anyhow::Result<()> {
    let missing = outcomes
        .iter()
        .filter(|o| {
            matches!(o, ord_domain::tiles::TileOutcome::Failed(f) if f.kind == ord_domain::ctx::FailureKind::Offline)
        })
        .count();
    if missing > 0 {
        bail!(
            "campagne de tuiles verrouillée : {missing} tuile(s) de rues hors de la campagne datée en \
             cache, non téléchargée(s) ; fill-cache refusé (README §2.1, manifest tuiles_*)"
        );
    }
    Ok(())
}

/// Campagne de tuiles de rues du manifest que `fill-cache` verrouille : celle dont l'espace de
/// cache est l'espace des tuiles de rues du service. Aucune campagne déclarée : `None` (tuiles
/// lues en ligne). Campagnes déclarées sans celle de cet espace, ou espace absent du cache : refus.
pub fn locked_campaign<'a>(
    manifest: &'a crate::manifest::Manifest,
    cache_dir: &Path,
) -> anyhow::Result<Option<&'a crate::manifest::TileCampaign>> {
    if manifest.tuiles.is_empty() {
        return Ok(None);
    }
    let ns =
        ord_server::wiring::streets_namespace(&bench_config_with(cache_dir, true, Variant::DEFAULT, true));
    let Some(c) = manifest.tuiles.iter().rev().find(|c| c.espace_cache == ns) else {
        bail!(
            "campagne de tuiles du manifest sur un autre espace de cache que celui du service ({ns}) : refus"
        );
    };
    if !cache_dir.join(&ns).is_dir() {
        bail!(
            "campagne de tuiles {} absente du cache de banc ({ns}) : à remplir par l'outil de la campagne",
            c.id
        );
    }
    Ok(Some(c))
}

impl Service {
    /// Service en processus, après ses lectures de démarrage (version Mapterhorn, index de
    /// couverture), relues dans le cache de banc en mode hors ligne.
    pub async fn start(cache_dir: &Path, offline: bool, variant: Variant) -> anyhow::Result<Self> {
        let config = bench_config(cache_dir, offline, variant);
        let state = AppState::bootstrap(&config, Wiring { pixel_convention: variant.convention }).await?;
        Ok(Self::new(state))
    }

    fn new(state: AppState) -> Self {
        Self { app: router(state.clone()), state, mode: BenchMode::Reference, stats: Default::default() }
    }

    /// Le même service, lu dans ce mode.
    pub fn with_mode(&self, mode: BenchMode) -> Self {
        Self { mode, stats: Default::default(), ..self.clone() }
    }

    pub fn mode(&self) -> BenchMode {
        self.mode
    }

    /// Bilans des réponses `input_with_breakpoints` depuis le dernier appel (vidés).
    pub fn take_stats(&self) -> Vec<InsertedStats> {
        std::mem::take(&mut *self.stats.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Service en processus sur une configuration donnée (diagnostics : `debug` permis).
    pub async fn start_with(config: &Config) -> anyhow::Result<Self> {
        let state = AppState::bootstrap(config, Wiring { pixel_convention: PixelConvention::Center }).await?;
        Ok(Self::new(state))
    }

    /// État du service (diagnostics : sources et règles de l'orchestrateur).
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// `POST /v1/route-details` ; une réponse non 200 est une erreur qui porte le `code` du
    /// problème RFC 9457 (jamais de coordonnée).
    pub async fn route_details(&self, points: &[Pt], options: Value) -> anyhow::Result<Value> {
        if self.mode == BenchMode::Breakpoints {
            return self.route_details_breakpoints(points, options).await;
        }
        self.post(points, options).await
    }

    /// Mode `input_with_breakpoints` : la requête `distance` (ou `input`) est envoyée dans le mode
    /// par défaut, puis sa sortie est rééchantillonnée sur la grille demandée (ou réduite aux
    /// points d'entrée), pour que les métriques lisent le même profil qu'en mode de référence.
    async fn route_details_breakpoints(&self, points: &[Pt], mut options: Value) -> anyhow::Result<Value> {
        let requested = options["sampling"]["mode"].as_str().unwrap_or("input_with_breakpoints").to_owned();
        let step = options["sampling"]["step_m"].as_f64();
        options["sampling"] = json!({"mode": "input_with_breakpoints"});
        let mut r = self.post(points, options).await?;
        let pts = r["points"].as_array().cloned().unwrap_or_default();
        if !pts.is_empty() {
            let inserted: Vec<f64> =
                pts.iter().filter(|p| p["origin"] == "inserted").filter_map(|p| p["d_m"].as_f64()).collect();
            let mut max_per_km = 0;
            for (i, &x) in inserted.iter().enumerate() {
                max_per_km = max_per_km.max(inserted[i..].partition_point(|&y| y < x + 1_000.0));
            }
            let stats = InsertedStats {
                length_m: r["summary"]["length_m"].as_f64().unwrap_or(0.0),
                input: pts.len() - inserted.len(),
                inserted: inserted.len(),
                max_per_km,
            };
            self.stats.lock().unwrap_or_else(|e| e.into_inner()).push(stats);
        }
        match (requested.as_str(), step) {
            ("input", _) => {
                let inputs: Vec<Value> = pts.into_iter().filter(|p| p["origin"] == "input").collect();
                r["points"] = Value::Array(inputs);
            }
            ("distance", Some(step)) => {
                r["points"] = Value::Array(resample(points, &pts, step)?);
            }
            _ => {}
        }
        Ok(r)
    }

    async fn post(&self, points: &[Pt], options: Value) -> anyhow::Result<Value> {
        let pts: Vec<Value> = points.iter().map(|p| json!({"lat": p.lat, "lon": p.lon})).collect();
        let body = json!({"points": pts, "options": options}).to_string();
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/route-details")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .context("requête")?;
        let response = self.app.clone().oneshot(req).await.context("appel du routeur")?;
        let status = response.status();
        let bytes = response.into_body().collect().await.context("corps de réponse")?.to_bytes();
        let value: Value = serde_json::from_slice(&bytes).context("réponse JSON")?;
        if !status.is_success() {
            bail!("service : HTTP {} `{}`", status.as_u16(), value["code"].as_str().unwrap_or("?"));
        }
        Ok(value)
    }
}

/// Méthode retenue pour une station de la grille entre deux points de la sortie : `none` si l'un
/// des deux est sans altitude ; sinon une méthode d'ouvrage (`dsm_deck`, puis
/// `interpolated_structure`) si l'un des deux en porte une, la station étant interpolée vers un
/// point d'ouvrage ; sinon celle du point le plus proche.
fn grid_method<'a>(a: &'a str, b: &'a str, t: f64) -> &'a str {
    for m in ["none", "dsm_deck", "interpolated_structure"] {
        if a == m || b == m {
            return m;
        }
    }
    if t <= 0.5 {
        a
    } else {
        b
    }
}

/// Sortie `input_with_breakpoints` rééchantillonnée sur la grille `k × step` (plus la fin) :
/// interpolation linéaire de l'altitude entre deux points consécutifs (`null` si l'une manque) et
/// de la position (points d'entrée : position envoyée ; points insérés : position rendue). Entre
/// deux points consécutifs de la sortie, la trace d'origine est droite : tous les points d'entrée
/// y sont.
pub fn resample(request: &[Pt], points: &[Value], step: f64) -> anyhow::Result<Vec<Value>> {
    struct P<'a> {
        d: f64,
        lat: f64,
        lon: f64,
        ele: f64,
        method: &'a str,
        index: u64,
    }
    let mut src: Vec<P<'_>> = Vec::with_capacity(points.len());
    for p in points {
        let index = p["index"].as_u64().context("index")?;
        let (lat, lon) = if p["origin"] == "input" {
            let q = request.get(index as usize).context("indice d'entrée hors de la requête")?;
            (q.lat, q.lon)
        } else {
            (p["lat"].as_f64().context("lat")?, p["lon"].as_f64().context("lon")?)
        };
        src.push(P {
            d: p["d_m"].as_f64().context("d_m")?,
            lat,
            lon,
            ele: p["ele"].as_f64().unwrap_or(f64::NAN),
            method: p["ele_method"].as_str().unwrap_or("none"),
            index,
        });
    }
    let Some(last) = src.last() else { return Ok(Vec::new()) };
    let length = last.d;
    let mut grid: Vec<f64> = (0..).map(|k| k as f64 * step).take_while(|x| *x <= length + 1e-3).collect();
    if grid.last().is_some_and(|x| length - x > 1e-3) {
        grid.push(length);
    }
    let mut out = Vec::with_capacity(grid.len());
    for x in grid {
        let x = x.min(length);
        let i = src.partition_point(|p| p.d <= x).saturating_sub(1).min(src.len() - 1);
        let j = (i + 1).min(src.len() - 1);
        let (a, b) = (&src[i], &src[j]);
        let t = if j > i && b.d > a.d { ((x - a.d) / (b.d - a.d)).clamp(0.0, 1.0) } else { 0.0 };
        let ele = if t == 0.0 { a.ele } else { a.ele + t * (b.ele - a.ele) };
        let method = if t == 0.0 { a.method } else { grid_method(a.method, b.method, t) };
        out.push(json!({
            "d_m": x,
            "index": a.index,
            "lat": a.lat + t * (b.lat - a.lat),
            "lon": a.lon + t * (b.lon - a.lon),
            "ele": if ele.is_nan() { Value::Null } else { json!(ele) },
            "ele_method": method,
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Campagne de tuiles verrouillée, sans réseau : la configuration en ligne garde l'altitude en
    /// ligne mais lit les rues sur disque seulement ; une tuile de rues absente du cache échoue
    /// `Offline` avant tout transport (URL jamais appelée) et le remplissage est refusé ; sans
    /// verrou, rien ne change. Le manifest désigne la campagne par son espace de cache.
    #[tokio::test]
    async fn a_locked_campaign_never_downloads_street_tiles() {
        use ord_domain::tiles::{TileKey, TileOutcome};
        let dir = tempfile::tempdir().unwrap();
        let mut config = bench_config_with(dir.path(), false, Variant::DEFAULT, true);
        assert!(!config.cache.offline && config.cache.offline_streets);
        assert!(!bench_config_with(dir.path(), false, Variant::DEFAULT, false).cache.offline_streets);
        // URL injoignable par construction : un transport tenté échouerait `Network`, pas `Offline`.
        config.typology.url = "http://127.0.0.1:9/{z}/{x}/{y}".into();
        let chain = ord_server::wiring::typology_chain(&config).unwrap();
        let ctx = ord_domain::ctx::RequestCtx::new(
            ord_domain::budget::RequestId("test".into()),
            ord_domain::budget::Deadline::after(std::time::Duration::from_secs(5)),
        );
        let keys = [TileKey { z: 14, x: 8000, y: 5700 }];
        let outcomes = chain.store().get_many(&keys, &ctx).await;
        assert!(
            matches!(&outcomes[0], TileOutcome::Failed(f) if f.kind == ord_domain::ctx::FailureKind::Offline),
            "tuile de rues hors du cache lue hors ligne"
        );
        let e = ensure_streets_cached(&outcomes).unwrap_err();
        assert!(e.to_string().contains("campagne de tuiles verrouillée"), "{e}");
        assert!(ensure_streets_cached::<()>(&[]).is_ok());
        // Campagne du manifest : espace du service exigé, présent dans le cache.
        let ns = ord_server::wiring::streets_namespace(&bench_config_with(
            dir.path(),
            true,
            Variant::DEFAULT,
            true,
        ));
        let campaign = |espace: &str| crate::manifest::Manifest {
            tuiles: vec![crate::manifest::TileCampaign {
                id: "tuiles_2026_11_02".into(),
                espace_cache: espace.into(),
                date_osm: None,
            }],
            ..Default::default()
        };
        assert!(locked_campaign(&crate::manifest::Manifest::default(), dir.path()).unwrap().is_none());
        assert!(locked_campaign(&campaign("autre"), dir.path()).is_err());
        let e = locked_campaign(&campaign(&ns), dir.path()).unwrap_err();
        assert!(e.to_string().contains("absente du cache"), "{e}");
        std::fs::create_dir_all(dir.path().join(&ns)).unwrap();
        assert_eq!(locked_campaign(&campaign(&ns), dir.path()).unwrap().unwrap().id, "tuiles_2026_11_02");
    }

    #[test]
    fn breakpoint_output_is_resampled_on_the_grid() {
        let request = [Pt { lat: 45.0, lon: 6.0 }, Pt { lat: 45.001, lon: 6.0 }];
        let points = vec![
            json!({"d_m": 0.0, "index": 0, "origin": "input", "ele": 10.0, "ele_method": "dem"}),
            json!({"d_m": 50.0, "index": 0, "origin": "inserted", "lat": 45.00045, "lon": 6.0, "ele": 20.0, "ele_method": "dsm_deck", "reasons": ["structure_start"]}),
            json!({"d_m": 111.0, "index": 1, "origin": "input", "ele": null, "ele_method": "none"}),
        ];
        let g = resample(&request, &points, 25.0).unwrap();
        let d: Vec<f64> = g.iter().map(|p| p["d_m"].as_f64().unwrap()).collect();
        assert_eq!(d, [0.0, 25.0, 50.0, 75.0, 100.0, 111.0]);
        assert_eq!(g[1]["ele"], 15.0);
        assert_eq!(g[1]["ele_method"], "dsm_deck", "interpolée vers un point d'ouvrage");
        assert_eq!(g[0]["ele_method"], "dem");
        assert!(g[3]["ele"].is_null() && g[3]["ele_method"] == "none");
        assert!((g[1]["lat"].as_f64().unwrap() - 45.000225).abs() < 1e-9);
        assert_eq!(g[5]["index"], 1);
    }
}
