//! `ord-bench` (T05 §3, §5) : fait passer le service sur le corpus du banc et calcule les
//! métriques. Il ne recalcule aucune référence : il lit celles de `bench/`.
//!
//! `check` (empreintes du manifest, cohérence `criteria.yaml` ↔ README, Q1), `fill-cache` (cache
//! de banc local), `run --offline` (incrément 2 : E1 jugé, E2 indicatif, E2b jugé ; incrément 3 :
//! R1, R2, R3, M1, typologie à chaud ; incrément 4 : E3, E4, E5, S1, S2, S3, E1 hors ouvrage),
//! `latency`
//! (L1 à froid, live), `diagnose` (causes des plages M1 et R dans les tuiles du cache),
//! `diagnose-structures` (groupes d'ouvrages rejoués étape par étape), `dump-profils` (vidage
//! privé des profils de traces de calibration, `bench/private/profils/`) ; scénarios golden sur des ouvrages de calibration.
//! Les rapports n'écrivent **aucune coordonnée** (T05 §3.4).

// `json!` du rapport de campagne (notes des règles du banc) : au-delà de la limite par défaut.
#![recursion_limit = "256"]

pub mod campaign;
pub mod check;
pub mod coherence;
pub mod coverage;
pub mod diagnose;
pub mod dump;
pub mod golden;
pub mod gpx;
pub mod hash;
pub mod inserted;
pub mod judge;
pub mod manifest;
pub mod marche;
pub mod metrics;
pub mod off_network;
pub mod osm_surface;
pub mod ouvrages;
pub mod privacy;
pub mod reference_error;
pub mod report_check;
pub mod retard;
pub mod service;
pub mod structures_diag;
pub mod temoin_tags;
pub mod typology;
pub mod v1;
#[cfg(test)]
mod v11_tests;
#[cfg(test)]
mod v12_tests;
#[cfg(test)]
mod v13_tests;

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Emplacements du banc.
#[derive(Clone, Debug)]
pub struct BenchPaths {
    /// `bench/` du dépôt.
    pub bench: PathBuf,
    /// Cache de banc local, hors dépôt (T05 §5.3).
    pub cache: PathBuf,
    /// Traces personnelles (`bench/private/`, hors git) ; absentes en CI.
    pub private: PathBuf,
}

impl BenchPaths {
    pub fn new(bench: &Path, cache: Option<&Path>, private: Option<&Path>) -> Self {
        let cache = cache
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("ORD_BENCH_CACHE").map(PathBuf::from))
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
                home.join(".cache/osm-route-details/bench")
            });
        let private = private.map(Path::to_path_buf).unwrap_or_else(|| bench.join("private"));
        Self { bench: bench.to_path_buf(), cache, private }
    }

    /// `bench/` du dépôt, vu depuis ce crate.
    pub fn repo_bench() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench")
    }

    pub fn private_present(&self) -> bool {
        self.private.join("traces").is_dir()
    }

    /// Chemin d'un fichier du manifest : `private/…` est lu dans le dossier privé (déplaçable par
    /// `--private-dir`), le reste dans `bench/`.
    pub fn resolve(&self, fichier: &str) -> PathBuf {
        match fichier.strip_prefix("private/") {
            Some(rest) => self.private.join(rest),
            None => self.bench.join(fichier),
        }
    }
}

/// Révision de l'outil de mesure à `rules_version` égale : suffixe du dossier de rapport
/// (`<date>-<rules_version>-<révision>`), remis à `None` à la prochaine montée des règles.
/// Remise à `None` avec les règles 2026.10.4 (incrément 4 : métriques d'ouvrage).
/// `bench1` (règles 2026.10.6) : périmètre lu sur la voie de l'ouvrage, détail des bandes de
/// portail retenues, erreur des ouvrages hors graphe contre leur référence. `bench2` : clés
/// privées (`_…`) retirées à tous les niveaux du rapport. `bench3` (README §4 bis complété en
/// 657d2c8) : balcon de moins de 120 m entre tunnels OSM, contrôle `hors_graphe_velo` satisfait
/// par une voie parallèle du graphe ; un passage par traversée de l'ouvrage ; `limite_donnees` sur
/// toute galerie `avalanche_protector` du corpus. Remise à `None` avec les règles 2026.10.7. `bench1`
/// (règles 2026.10.7, README §4 bis complété en 022710f et 4304838) : décalage local E3 sur les
/// approches, référence incertaine, bande sur un ouvrage des tuiles, plausibilité E4 (25 %) ;
/// passages des ouvrages de calibration dans les traces tenues à l'écart (extraits). `bench2`
/// (e48170f) : passage en hauteur sur un pont (test de niveau) ; repli 1–3 km des extraits lu sur la
/// trace (baro − MNT), stations par côté rapportées ; `diagnose-structures` sur les extraits.
/// Remise à `None` avec les règles 2026.10.8. `bench1` (règles 2026.10.8) : géométries et baro
/// recalés régénérés par le banc en 74cdd88 (raccords de tronçons Stadia), outil inchangé. `bench2`
/// (règles 2026.10.8, banc 53225a8 et adbf6f1) : baro des points insérés gardé à 30 m d'une voie
/// des tuiles de la campagne ; recalages régénérés (colonne `insere`, pas de baro dans les trous de
/// la trace brute). Remise à `None` avec les règles 2026.10.9 (incrément 5 : points de rupture ;
/// campagne dans les deux modes, `distance` et `input_with_breakpoints`). `bench1` (règles
/// 2026.10.9) : en mode `input_with_breakpoints`, références des ouvrages (MNT des approches E3,
/// test de niveau, E5, bandes de portail E4, repli 1–3 km) et positions des passages lues en
/// mode `distance` ; seule la sortie testée vient du mode `input_with_breakpoints` (E2b garde
/// la sortie non corrigée de ce mode). `bench2` (règles 2026.10.9, banc 405f956, criteria.yaml
/// `preset_baro: dem`, README §4 bis.5) : D+ baro d'E2b au preset `dem` (`E2b.dplus_baro_dem_m`,
/// `E2b.ratio*`), le même que le service et sa référence sans correction ; E2 reste en
/// `barometric`. `bench3` (règles 2026.10.9, incrément 6) : campagne paramétrée par série
/// (`run_series`, pour `judge`), `increment_plan` 6 ; mesures de calibration inchangées ; service
/// avec admission, budget et annulation (sans effet sur les sorties) ; `latency --profile warm`
/// (L2). Remise à `None` avec les règles 2026.10.10 (tunnels OSM en série : balcons de moins de
/// 120 m, droite entre les portails extérieurs de la série) ; règles du banc du 2026-11-01
/// (README §4 bis.4, 6 et 7 : limite des données hors décompte d'E4/E3 et S1, M1 hors réseau,
/// R3 non bloquant) appliquées dans la même révision. `bench1` (règles 2026.10.10, README §4 ter,
/// règles V1 du 2026-11-01 : 304f767, 16f6097, c1d231c) : S1 en recouvrement continu de
/// [s_A, s_B] (tolérance de 2,5 m), attribution d'un passage par la trace brute
/// (`passage_sur_ouvrage_parallele`), M1 hors graphe vélo avec contrôle `matched: false` +
/// `unmatched_range` ; campagne `increment` V1. Remise à `None` avec les règles 2026.11.1 (état
/// « hors graphe » rendu avec la typologie de la route fermée suivie, api 1.1). `bench1` (règles
/// 2026.11.3, README §4 ter après la calibration 2026.11.2 : ec72f13, 09dcda0, 0b590ff) : M1 hors
/// graphe vélo resserrée (routes motorisées, plages ≥ 50 m, rails exclus), S2 sans les plages hors
/// graphe vélo des témoins, attribution « un tablier, un ouvrage » (couche `bridges` des tuiles).
/// Remise à `None` avec les règles 2026.11.4 (galeries couvertes détectées sur le MNT).
/// Reste à `None` avec les règles 2026.11.5 (couvertures en altitude
/// seule) ; README §4 ter.6 précisé en eb4c4cc (`covered=yes` lu sur la voie des stations du témoin)
/// appliqué dans la même révision. `bench1` (règles 2026.11.5 inchangées) : série V1 lue
/// (`perso_v1` : traces, ouvrages et croisements E5, témoins S2, itinéraires planifiés E4 / S2 /
/// R1-R2), série du MVP jugée devenue calibration, recalages du Valhalla FOSSGIS (6aba29e),
/// plages `covered` mesurées contre le MNT (cc59b18), couverture rapportée par critère. Remise à
/// `None` avec les règles 2026.11.6 (pont au-dessus de la trace) ; révision du banc à la même
/// campagne : référence d'E5 au croisement (800d4e0), E1 plaine incertaine (4423741), ouvrages
/// réels des témoins V1 (57e9d95). `bench1` (règles 2026.11.6 inchangées, README §4 ter.7 après
/// la calibration 2026.11.6 : f1bfda3, e477dd4, 504c800, e9430ac ; manifest 6ed1da7) : E5 au baro
/// corrigé toléré à 2 m, baro incertain (côtés à plus de 2 m, ou côté à moins de 3 points), bosse
/// MNT de 1 à 2 m incertaine, approches des plages `O` depuis leurs bouts ; empreinte des ouvrages
/// réels des témoins V1 lue au manifest (`temoins_v1_tags`). `bench2` (règles 2026.11.6
/// inchangées, README §4 ter.7 et 9 après la calibration 2026.11.6-bench1 : 1477b5f, b922ce4,
/// 6bc77a5) : retard du baro estimé par cas (δ de −50 à +200 m, écart robuste du résidu baro − MNT)
/// et corrigé pour E1, E3 et E5, baro brut pour E2 et E2b ; points de baro d'un côté d'E5 comptés
/// bruts (`baro.csv`), plus les stations de la grille ; côté sans décalage incertain, sans repli
/// sur le MNT. `bench3` (règles 2026.11.6 inchangées, README §4 ter.7 et 9 après la calibration
/// 2026.11.6-bench2 : 085d26e, 7dab276) : points de baro d'un côté d'E5 comptés à leur abscisse
/// corrigée du retard (s_p − δ), fenêtres masquées lues à cette abscisse ; lectures du retard
/// (dérive par médiane glissante de 5 km, MNT sans correction du mode `distance`, extraits au
/// baro brut, traces tenues à l'écart en cas entiers au jugement) et d'E5 (baro corrigé au
/// croisement, côté sans décalage incertain sans repli) vérifiées conformes. `bench4` (règles
/// 2026.11.6 inchangées, README §4 ter.9 après la calibration 2026.11.6-bench3 : 30e8024, 48ca44a,
/// 87b0ea8) : au jugement, extraits des traces de calibration au δ gelé de leur trace
/// (`data/retard-baro-v1.json`, empreinte du manifest vérifiée), sans réestimation, baro brut si
/// la trace n'y figure pas ; δ calculés de la calibration contrôlés contre ce fichier. `bench5`
/// (règles 2026.11.6 inchangées, après le jugement V1 interrompu : témoin aux extrémités non
/// retrouvées) : fenêtres des extraits des traces de l'autre série calculées comme les masques de
/// la calibration, sur les stations du mode `distance` au pas de la grille (et non plus sur les
/// sommets de la géométrie recalée), tronçons `.partN` recollés comme le service ; élément de
/// `passages_masques` retrouvé mais sans fenêtre, témoin V1 aux extrémités non retrouvées ou
/// fenêtre sans extrait : cas sauté, jamais une perte silencieuse ; élément qu'aucune station
/// n'approche à 40 m (non retrouvé, comme en calibration) : rapporté (`passage_non_retrouve`),
/// sans interrompre le jugement (décision de l'utilisateur, 2026-11-02). Remise à `None` avec
/// les règles 2026.11.7 (garde-fou de plausibilité des valeurs IGN). Reste à `None` avec les
/// règles 2026.11.8 (point de rupture au croisement d'un pont au-dessus de la trace) ; règles du
/// banc du 2026-11-02, après le jugement V1 (README §4 ter.7 et §4 bis.8 : f38b745, 2e511e5,
/// 562d729), appliquées dans la même révision, la première campagne des règles 2026.11.8 n'ayant
/// pas encore tourné : référence MNT d'E5 incertaine (`approche_sur_ouvrage`) si une approche
/// 20–50 m chevauche un autre ouvrage des tuiles, hors voies de l'ouvrage du dessus, testée avant
/// la bosse ; plages d'erreur de la référence Valhalla (classe non revêtue sur une voie OSM en
/// revêtement dur, instantané daté `osm_tags_reference`, empreinte vérifiée) hors décompte de R1,
/// R2 et R3, rapportées à part. Reste à `None` avec les règles 2026.11.9 (`surface` brut et
/// `bicycle` normalisé des tuiles VersaTiles shortbread@1.1), campagne
/// `2026-11-02-2026.11.9` sans révision. Série V1 jugée lue en calibration (README §2, point 3 ;
/// décision de l'utilisateur, 2026-11-02 : `Manifest::with_v1_judged`, plus de masques ni
/// d'extraits) : changement d'outil postérieur à cette campagne, mesuré pour la première fois par
/// la campagne des règles 2026.11.10 (`bicycle=no` sans effet sur les voies non routières), qui
/// remet la révision à `None` ; à rapporter `bench1` s'il était mesuré sous 2026.11.9. `bench1`
/// (règles 2026.11.10 inchangées, après la campagne `2026-11-02-2026.11.10` ; décisions de
/// l'utilisateur, 2026-11-02) : E1, marche du baro (README §4 ter.11, 3f8a810 : |Δ court| et
/// |Δ long| > 10 m de même signe, MNT lisse, δ appliqué avant ; référence incertaine, rapportée,
/// non jugée) ; contrôle M1 hors graphe vélo satisfait par une voie du graphe vélo distincte de la
/// route fermée (62bd46e, `passe_si_un_de`) ; série tenue à l'écart V1.1 lue (`perso_v11`, à défaut
/// `serie_v11` ; 7b1a4e9, 22dbba7) : en calibration, aucun cas V1.1 ouvert, ses éléments masqués
/// sur les cas de calibration ; au jugement V1.1, cas jugés entiers sans masque ni extrait, E3 non
/// bloquant, minimum E2b sur `dplus` et `plaine` ; instantané OSM de R1, R2 et R3 choisi par la
/// date du recalage de chaque cas. `bench2` (règles 2026.11.10 inchangées, après la campagne
/// `2026-11-02-2026.11.10-bench1` ; README §4 ter.6, affc041) : un témoin S2 à extrémités (`debut`,
/// `fin`) se mesure entre elles, même sur un itinéraire planifié (`pl-e4-cajarc-s0`, `-s1`) ; seul
/// un témoin sans extrémités couvre tout l'itinéraire. `bench3` (règles 2026.11.11 : service
/// 8510a0b, goldens ff60cfd ; règles du banc pour V1.2, README §4 quater, décisions de
/// l'utilisateur du 2026-11-03 : d6fd42c, f8f81a6, 2744983, 633255f, 8559a21), révision gardée à
/// la montée des règles : E1, zones de pente du baro sans relief
/// masquées sur [s − 300, s + 350] (baro corrigé du δ > 15 % sur 50 m, MNT < 10 %), hors du
/// décalage, de la tendance, du RMSE, du P95 et des km comparés, et avant la marche, détectée hors
/// de ces zones ; E5, référence baro incertaine (`baro_bruite`) si le bruit baro de la trace
/// dépasse 1,0 m, après le retrait, avant les côtés ; E5 jugé et couvert par site (ids réunis s'ils
/// sont le même ouvrage ou croisés par un même passage à 40 m au plus l'un de l'autre, de proche en
/// proche ; un passage compté en échec met le site en échec) ; R1/R2, voies aux tags `highway`,
/// `surface` ou `tracktype` différents entre la date des tuiles et celle de la référence (deux
/// instantanés `osm_tags_toutes` du manifest) exclues et rapportées (`tags_differents`), avant
/// l'erreur de la référence, plages d'erreur coupées aux sauts de s_origine ; défauts d'outil
/// corrigés : itinéraire `par_cas` d'une série jugée passé en calibration, `fill-cache` sans
/// téléchargement de tuile de rue hors de la campagne datée du manifest (`tuiles_*`). `bench4`
/// (règles 2026.11.11 inchangées ; e37e60e), série tenue à l'écart V1.2 lue (`perso_v12`, `serie_v12` ; README §4 quinquies) comme
/// V1.1 : en calibration (MVP, V1, V1.1), aucun cas V1.2 ouvert, ses éléments (`nv-948` et
/// suivants) masqués sur les cas de calibration ; au jugement V1.2, cas jugés entiers sans masque
/// ni extrait, Q1 non bloquant, E3 bloquant si la couverture comptée avant toute mesure (manifest
/// `serie_v12.couverture.E3`) atteint son minimum, sinon rapporté sans refus (e57dd1e ; un sous-minimum mesuré au jugement refuse : 65ea59b) ; ouvrages
/// V1.1 du complément (`complement_v11`, `nv-840` à `nv-947`, ee5d7c1) mesurés en calibration.
/// `bench5` (règles 2026.11.12 ; après le jugement V1.2, décision de l'utilisateur du
/// 2026-11-04) : extrémités d'un témoin S2, la fin cherchée d'abord sur tout le cas, puis le début
/// dans `[d_fin − 1,5 × longueur déclarée, d_fin]` (à défaut, à rebours, dans
/// `[d_fin, d_fin + 1,5 × longueur déclarée]`) ; une trace qui repasse par le début ne rallonge
/// plus le témoin (`perso-196-s0` : 1 645 m mesurés au lieu de 567 m). `bench6` (règles
/// 2026.11.12 inchangées ; règles du banc après le jugement V1.2, README §4 sexies, décisions de
/// l'utilisateur du 2026-11-04) : trace marquée au manifest `reference_baro: periode_exclue`
/// (départ de 2015-05 à 2020-09) ou `non_qualifiee` (avant 2014-12 sans qualification
/// individuelle) hors des références baro d'E1, E2, E2b, E3 et E5, rapportée
/// (`periode_baro_exclue`), cas de M1 et de la typologie ; marque contrôlée contre `mois` par
/// `check` (d4aad9a, 1c9edf8, a8bee85) ; garde de bruit (`noise_m` > 1,0 m) d'E3 (passage hors de
/// la médiane du pont) et d'E2b (trace hors de la médiane du groupe), rapportée (`baro_bruite`,
/// befb602) ; critère jugé groupe par groupe si sa couverture par groupe est atteinte avant mesure
/// (`si_couverture_atteinte_par_groupe`, dc76a6e, pour E2b de V1.3 ; sections des séries après V1
/// lues par `SECTIONS`, aucune section V1.3 au manifest). `bench7` (règles 2026.11.12 inchangées ;
/// série tenue à l'écart V1.3 lue, manifest `perso_v13`, `serie_v13`, `tuiles_2026_11_04`,
/// b34b23f ; README §4 septies, 13b6ba4, dc76a6e) comme V1.2 : en calibration (MVP, V1, V1.1 et
/// V1.2, jugée), aucun cas V1.3 ouvert, ses éléments (`nv-1267` et suivants) masqués sur les cas
/// de calibration — ces masques modifient la calibration, comme `bench4` pour V1.2 ; au jugement
/// V1.3, cas jugés entiers sans masque ni extrait, E3 et E5 sur ses seuls éléments, Q1 non
/// bloquant, E3 bloquant si sa couverture comptée avant toute mesure atteint son minimum (sinon
/// rapporté ; `statut` du manifest contrôlé contre les comptes), E2b jugé groupe par groupe.
pub const BENCH_REVISION: Option<&str> = Some("bench7");

/// Règles de calcul non fixées par le banc (T05 §3.1) : constantes hachées dans `results.json`
/// (`bench_rules_sha256`) et gelées avec les seuils.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BenchRules {
    /// Point baro écarté au-delà de cette distance de son point apparié (bench §2.1, étape 3).
    pub match_max_m: f64,
    /// Grille curviligne commune du service et de la référence (bench §2.2, étape 3).
    pub grid_step_m: f64,
    /// Pas des points envoyés quand la géométrie recalée dépasse 16 000 sommets (T05 §3.4).
    pub request_resample_m: f64,
    /// Au-delà, deux points baro consécutifs ne sont pas interpolés (trou de référence).
    pub baro_gap_max_m: f64,
    /// Point inséré (`insere = 1`) : baro gardé à cette distance au plus d'une ligne de la couche
    /// `streets` (tuile et 8 voisines, cache de la campagne ; bench §2.1, étape 3).
    pub inserted_street_max_m: f64,
    /// Passage d'un ouvrage : trace à moins de cette distance de ses points de référence.
    pub passage_tol_m: f64,
    /// Demi-largeur de la fenêtre masquée autour d'un ouvrage d'une autre série.
    pub mask_half_m: f64,
    /// Retrait de la dérive lente : médiane glissante sur cette fenêtre (bench §2.2, étape 4).
    pub detrend_window_m: f64,
    /// Pas de calcul de cette médiane glissante, interpolée entre deux calculs.
    pub detrend_step_m: f64,
}

impl Default for BenchRules {
    fn default() -> Self {
        Self {
            match_max_m: 30.0,
            grid_step_m: 5.0,
            request_resample_m: 10.0,
            baro_gap_max_m: 100.0,
            inserted_street_max_m: 30.0,
            passage_tol_m: 40.0,
            mask_half_m: 500.0,
            detrend_window_m: 5_000.0,
            detrend_step_m: 50.0,
        }
    }
}
