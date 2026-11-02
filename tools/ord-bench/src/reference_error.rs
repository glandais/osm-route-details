//! Erreur de la référence de revêtement, hors décompte de R1, R2 et R3 (README §4 bis.8, décision
//! de l'utilisateur, 2026-11-02 : 562d729 ; criteria `serie_tenue_a_l_ecart.erreur_reference_R`).
//!
//! Quand la référence est la réponse `trace_attributes` de Valhalla (R1/R2 des parcours planifiés,
//! R3 des traces perso ; jamais les parcours d'osm-surface), une plage de `<id>.surface.csv` est
//! une erreur de la référence si Valhalla y rend une classe non revêtue et que sa voie
//! (`way_id`) porte, dans l'instantané OSM daté à la date des données du serveur, un `surface` de
//! revêtement dur sans ambiguïté. La plage sort du décompte (ni accord ni désaccord, ni dans la
//! longueur comparée) et est rapportée à part : longueur et part par cas.
//!
//! Instantané : `private/osm-tags/<date>.json` (`tools/osm_tags_reference.py`), empreinté au
//! manifest (`perso_v1.osm_tags_reference`), lu sans réseau ; empreinte différente : refus ; voie
//! absente : pas d'exclusion. Les identifiants de voies ne sortent jamais de ce module : seules des
//! abscisses et des longueurs en sortent.

use std::collections::BTreeSet;
use std::ops::Range;

use anyhow::{bail, Context};
use serde_json::Value;

use crate::typology::{Class, Ref};

/// Classes non revêtues de Valhalla visées (valeurs exactes de `edge.surface`).
pub const VALHALLA_UNPAVED: [&str; 5] = ["compacted", "dirt", "gravel", "path", "impassable"];
/// `surface` OSM de revêtement dur sans ambiguïté (valeurs exactes ; `paved`, `sett`,
/// `paving_stones:lanes`, `unhewn_cobblestone` et les autres n'en sont pas).
pub const OSM_HARD: [&str; 5] = ["asphalt", "concrete", "concrete:lanes", "concrete:plates", "paving_stones"];

/// Voies de l'instantané qui portent un revêtement dur. Pas de `Debug` dérivé : les identifiants
/// de voies sont privés.
pub struct OsmTags {
    date: String,
    hard: BTreeSet<u64>,
}

impl std::fmt::Debug for OsmTags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OsmTags").field("date", &self.date).field("voies_dures", &self.hard.len()).finish()
    }
}

/// Date d'instantané ramenée à la minute : `2026-11-01T13:01Z` et `2026-11-01T13:01:00Z` sont la
/// même date. Secondes non nulles gardées.
pub fn date_key(s: &str) -> String {
    let t = s.trim().trim_end_matches('Z');
    let t = t.strip_suffix(":00").filter(|x| x.matches(':').count() == 1).unwrap_or(t);
    format!("{t}Z")
}

/// Fichier de l'instantané d'une date : `private/osm-tags/2026-11-01T13-01Z.json`.
pub fn snapshot_file(date: &str) -> String {
    format!("private/osm-tags/{}.json", date_key(date).replace(':', "-"))
}

impl OsmTags {
    /// Instantané lu depuis son JSON (`{date, tags: {way_id: {surface, …}}}`), daté `date`.
    pub fn from_json(bytes: &[u8], date: &str) -> anyhow::Result<Self> {
        let v: Value = serde_json::from_slice(bytes).context("instantané des tags OSM illisible")?;
        let own = v["date"].as_str().context("instantané des tags OSM sans date")?;
        if date_key(own) != date_key(date) {
            bail!("instantané des tags OSM daté {own}, pas {date}");
        }
        let tags = v["tags"].as_object().context("instantané des tags OSM sans `tags`")?;
        let hard = tags
            .iter()
            .filter(|(_, t)| t["surface"].as_str().is_some_and(|s| OSM_HARD.contains(&s)))
            .filter_map(|(k, _)| k.parse().ok())
            .collect();
        Ok(Self { date: date_key(date), hard })
    }

    /// Instantané de la date des données du serveur Valhalla (`perso_v1.recalage.donnees_serveur`) :
    /// entrée `perso_v1.osm_tags_reference` du manifest pour `private/osm-tags/<date>.json`, à la
    /// même date, empreinte vérifiée. Aucun instantané à cette date : refus (le README n'en prévoit
    /// pas d'autre source, et le banc ne lit pas le réseau).
    pub fn load(paths: &crate::BenchPaths, manifest: &crate::manifest::Manifest) -> anyhow::Result<Self> {
        let v1 = manifest.v1.as_ref().context("erreur de la référence R : section perso_v1 absente")?;
        let date = v1
            .donnees_serveur
            .as_deref()
            .context("erreur de la référence R : date des données du serveur Valhalla absente du manifest")?;
        let fichier = snapshot_file(date);
        let Some((f, sum, d)) = v1.osm_tags.as_ref().filter(|(f, _, _)| *f == fichier) else {
            bail!(
                "erreur de la référence R (README §4 bis.8) : aucun instantané des tags OSM au manifest \
                 pour la date des données du serveur Valhalla ({date}, {fichier} attendu)"
            );
        };
        if date_key(d) != date_key(date) {
            bail!("osm_tags_reference daté {d} au manifest, données du serveur Valhalla du {date}");
        }
        let bytes = std::fs::read(paths.resolve(f)).with_context(|| format!("{f} : instantané absent"))?;
        if crate::hash::sha256_bytes(&bytes) != *sum {
            bail!("{f} : empreinte différente de celle du manifest");
        }
        Self::from_json(&bytes, date)
    }

    fn is_hard(&self, way: u64) -> bool {
        self.hard.contains(&way)
    }
}

/// Plages d'erreur de la référence dans `<id>.surface.csv` (abscisse du fichier), triées,
/// contiguës réunies. Colonnes lues par leur en-tête (`s_debut_m`, `s_fin_m`, `surface`, `use`,
/// `way_id`) ; les discontinuités n'ont pas de référence et ne sont jamais une erreur.
pub fn error_ranges(csv: &str, tags: &OsmTags) -> Vec<Range<f64>> {
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split(',').map(str::trim).collect();
    let col = |name: &str| header.iter().position(|h| *h == name);
    let (Some(a), Some(b), Some(surface), Some(way)) =
        (col("s_debut_m"), col("s_fin_m"), col("surface"), col("way_id"))
    else {
        return Vec::new();
    };
    let usage = col("use");
    let mut out: Vec<Range<f64>> = lines
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            if usage.and_then(|u| f.get(u)).is_some_and(|u| *u == "discontinuite") {
                return None;
            }
            if !VALHALLA_UNPAVED.contains(f.get(surface)?) {
                return None;
            }
            let w = f.get(way)?.parse::<f64>().ok().filter(|w| w.is_finite() && *w >= 0.0)? as u64;
            if !tags.is_hard(w) {
                return None;
            }
            let (from, to) = (f.get(a)?.parse::<f64>().ok()?, f.get(b)?.parse::<f64>().ok()?);
            (to > from).then_some(from..to)
        })
        .collect();
    out.sort_by(|x, y| x.start.total_cmp(&y.start));
    let mut merged: Vec<Range<f64>> = Vec::new();
    for r in out {
        match merged.last_mut() {
            Some(last) if r.start <= last.end + 1e-6 => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

/// Plages de la référence privées des plages `cut` (triées, disjointes).
pub fn without(reference: &[Ref], cut: &[Range<f64>]) -> Vec<Ref> {
    let mut out = Vec::new();
    for r in reference {
        let mut from = r.from;
        for c in cut.iter().filter(|c| c.start < r.to && c.end > r.from) {
            if c.start > from {
                out.push(Ref { from, to: c.start, ..*r });
            }
            from = from.max(c.end);
        }
        if r.to > from {
            out.push(Ref { from, ..*r });
        }
    }
    out
}

/// Référence après exclusion, et ce qui est rapporté à part.
#[derive(Clone, Debug, PartialEq)]
pub struct Exclusion {
    pub reference: Vec<Ref>,
    /// Longueur de référence retirée (m).
    pub excluded_m: f64,
    /// Longueur de référence classée avant exclusion (m) : sans discontinuités ni plages
    /// `UNKNOWN`, base de la part (README §4 bis.8, lecture de 40c65cd).
    pub reference_m: f64,
}

impl Exclusion {
    pub fn part(&self) -> f64 {
        if self.reference_m > 0.0 {
            self.excluded_m / self.reference_m
        } else {
            0.0
        }
    }

    /// Champs rapportés dans `typologie` du cas (`<prefix>.erreur_reference_m`, `…_part`).
    pub fn json_into(&self, typo: &mut Value, prefix: &str) {
        typo[format!("{prefix}.erreur_reference_m")] = serde_json::json!(round(self.excluded_m, 1));
        typo[format!("{prefix}.erreur_reference_part")] = serde_json::json!(round(self.part(), 4));
        typo[format!("{prefix}.reference_m")] = serde_json::json!(round(self.reference_m, 1));
    }
}

fn round(x: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (x * k).round() / k + 0.0
}

fn length(r: &[Ref]) -> f64 {
    r.iter().map(|x| x.to - x.from).sum()
}

/// Longueur de référence classée (40c65cd) : sans les plages `UNKNOWN` (les discontinuités du
/// recalage ne sont déjà pas des plages de référence).
fn classed(r: &[Ref]) -> f64 {
    r.iter().filter(|x| x.class != Class::Unknown).map(|x| x.to - x.from).sum()
}

/// Exclut les plages `cut` de la référence.
pub fn exclude(reference: Vec<Ref>, cut: &[Range<f64>]) -> Exclusion {
    let reference_m = classed(&reference);
    let kept = without(&reference, cut);
    let excluded_m = (length(&reference) - length(&kept)).max(0.0);
    Exclusion { reference: kept, excluded_m, reference_m }
}

/// Parcours planifié R1/R2 : plages d'erreur ramenées de l'abscisse recalée de `surface.csv` à
/// celle de l'itinéraire, par les points appariés de `<id>.baro.csv` (`s_recale_m` →
/// `s_origine_m`, points non insérés, interpolation monotone, bornée aux extrémités), comme
/// `tools/reference_r12_v1.py` qui a écrit la référence.
pub fn to_origin(cut: &[Range<f64>], baro_csv: &str) -> anyhow::Result<Vec<Range<f64>>> {
    let mut lines = baro_csv.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split(',').map(str::trim).collect();
    let col = |name: &str| {
        header.iter().position(|h| *h == name).with_context(|| format!("baro.csv sans `{name}`"))
    };
    let (r, o, ins) = (col("s_recale_m")?, col("s_origine_m")?, col("insere")?);
    let mut m: Vec<(f64, f64)> = lines
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            if *f.get(ins)? != "0" {
                return None;
            }
            Some((f.get(r)?.parse().ok()?, f.get(o)?.parse().ok()?))
        })
        .collect();
    if m.is_empty() {
        bail!("baro.csv sans point apparié");
    }
    m.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let (xr, xo): (Vec<f64>, Vec<f64>) = m.into_iter().unzip();
    // `bisect_left` de reference_r12_v1.py.
    let orig = |x: f64| -> f64 {
        let i = xr.partition_point(|v| *v < x);
        if i == 0 {
            return xo[0];
        }
        if i >= xr.len() {
            return xo[xo.len() - 1];
        }
        let (a, b) = (xr[i - 1], xr[i]);
        let t = if b > a { (x - a) / (b - a) } else { 0.0 };
        xo[i - 1] + t * (xo[i] - xo[i - 1])
    };
    Ok(cut.iter().map(|c| orig(c.start)..orig(c.end)).filter(|c| c.end > c.start).collect())
}

/// Contrôle d'un parcours planifié : chaque plage d'erreur ramenée à l'itinéraire tombe sur une
/// plage non revêtue de la référence empreintée (à 1 m près, arrondis de la référence) ; sinon
/// `surface.csv` et la référence ne se correspondent pas, refus.
pub fn check_on_unpaved(cut: &[Range<f64>], reference: &[Ref]) -> anyhow::Result<()> {
    for c in cut {
        let unpaved: f64 = reference
            .iter()
            .filter(|r| r.class == Class::Unpaved)
            .map(|r| (r.to.min(c.end) - r.from.max(c.start)).max(0.0))
            .sum();
        if unpaved < c.end - c.start - 1.0 {
            bail!("erreur de la référence R : surface.csv ne correspond pas à la référence empreintée");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: &str = r#"{"date":"2026-11-01T13:01:00Z","source":"overpass","tags":{
        "11":{"surface":"asphalt","highway":"tertiary"},
        "12":{"surface":"paved","highway":"unclassified"},
        "13":{"highway":"track"},
        "14":{"surface":"concrete:plates","highway":"track"},
        "15":{"surface":"paving_stones:lanes","highway":"track"}},"voies":5}"#;

    const CSV: &str = "s_debut_m,s_fin_m,surface,road_class,use,way_id\n\
        0.0,100.0,dirt,unclassified,road,11\n\
        100.0,150.0,gravel,track,road,14\n\
        150.0,250.0,paved_smooth,tertiary,road,11\n\
        250.0,300.0,dirt,unclassified,road,12\n\
        300.0,350.0,gravel,track,road,13\n\
        350.0,400.0,compacted,track,road,15\n\
        400.0,450.0,dirt,track,road,99\n\
        450.0,500.0,,,discontinuite,\n\
        500.0,600.0,compacted,track,road,11.0\n";

    fn tags() -> OsmTags {
        OsmTags::from_json(SNAPSHOT.as_bytes(), "2026-11-01T13:01Z").unwrap()
    }

    #[test]
    fn snapshot_dates_and_file() {
        assert_eq!(date_key("2026-11-01T13:01Z"), "2026-11-01T13:01Z");
        assert_eq!(date_key("2026-11-01T13:01:00Z"), "2026-11-01T13:01Z");
        assert_eq!(date_key("2026-11-01T13:01:30Z"), "2026-11-01T13:01:30Z");
        assert_eq!(snapshot_file("2026-11-01T13:01Z"), "private/osm-tags/2026-11-01T13-01Z.json");
        let e = OsmTags::from_json(SNAPSHOT.as_bytes(), "2026-11-02T08:00Z").unwrap_err();
        assert!(e.to_string().contains("daté"), "{e}");
    }

    /// 562d729 : Valhalla non revêtu sur une voie OSM en revêtement dur = erreur de la référence ;
    /// pas `paved`, ni `paving_stones:lanes`, ni une voie sans `surface` ; pas Valhalla revêtu ;
    /// voie absente de l'instantané : pas d'exclusion.
    #[test]
    fn unpaved_valhalla_on_a_hard_osm_surface_is_a_reference_error() {
        let cut = error_ranges(CSV, &tags());
        assert_eq!(cut, vec![0.0..150.0, 500.0..600.0]);
        // Voie absente de l'instantané (99) : rien n'est exclu sur 400–450.
        assert!(!cut.iter().any(|c| c.start < 450.0 && c.end > 400.0));
        // Sens inverse (Valhalla revêtu, OSM dur) : jamais.
        assert!(!cut.iter().any(|c| c.start < 250.0 && c.end > 150.0));
    }

    /// L'exclusion retire les plages de la référence : ni accord, ni désaccord, ni longueur comparée.
    #[test]
    fn excluded_ranges_leave_the_count() {
        use crate::typology::{agreement, read_stadia_surface, Ours, Source};
        let reference = read_stadia_surface(CSV);
        let ours =
            [Ours { from: 0.0, to: 600.0, class: Class::Paved, matched: true, source: Source::Tagged }];
        let before = agreement(&ours, &reference);
        let x = exclude(reference, &error_ranges(CSV, &tags()));
        let after = agreement(&ours, &x.reference);
        assert_eq!(x.reference_m, 550.0);
        assert_eq!(x.excluded_m, 250.0);
        assert!((x.part() - 250.0 / 550.0).abs() < 1e-12);
        assert_eq!(before.compared_m, 550.0);
        assert_eq!(after.compared_m, 300.0);
        // Restent : 150–250 revêtu (accord), 250–450 non revêtu (désaccord).
        assert!((after.paved - 100.0 / 300.0).abs() < 1e-12, "{after:?}");
        let mut typo = serde_json::json!({});
        x.json_into(&mut typo, "R3");
        assert_eq!(typo["R3.erreur_reference_m"], 250.0);
        assert_eq!(typo["R3.erreur_reference_part"], 0.4545);
        // Longueurs et part seulement : aucun identifiant de voie dans ce qui sort.
        let keys: Vec<&String> = typo.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["R3.erreur_reference_m", "R3.erreur_reference_part", "R3.reference_m"]);
    }

    /// Une voie dure dont aucune plage n'est non revêtue chez Valhalla : rien n'est exclu.
    #[test]
    fn no_exclusion_without_an_unpaved_valhalla_class() {
        let csv = "s_debut_m,s_fin_m,surface,road_class,use,way_id\n0,100,paved,tertiary,road,11\n\
                   100,150,,service,road,12\n";
        assert!(error_ranges(csv, &tags()).is_empty());
        // Base de la part : référence classée, la plage sans classe (UNKNOWN) n'y est pas (40c65cd).
        let x = exclude(crate::typology::read_stadia_surface(csv), &[]);
        assert_eq!((x.excluded_m, x.reference_m), (0.0, 100.0));
    }

    #[test]
    fn planned_ranges_are_brought_back_to_the_route() {
        let baro = "s_recale_m,s_origine_m,z,insere\n0,10,1,0\n50,60,1,1\n100,110,1,0\n200,230,1,0\n";
        let cut = to_origin(&[0.0..100.0, 150.0..300.0], baro).unwrap();
        assert_eq!(cut, vec![10.0..110.0, 170.0..230.0]);
        let reference = vec![
            Ref { from: 0.0, to: 110.0, class: Class::Unpaved },
            Ref { from: 110.0, to: 170.0, class: Class::Paved },
            Ref { from: 170.0, to: 230.5, class: Class::Unpaved },
        ];
        check_on_unpaved(&cut, &reference).unwrap();
        let reference = vec![Ref { from: 0.0, to: 300.0, class: Class::Paved }];
        assert!(check_on_unpaved(&cut, &reference).is_err());
    }

    /// Instantané lu par le manifest : empreinte fausse refusée, date sans instantané refusée.
    #[test]
    fn the_snapshot_is_read_from_the_manifest_and_its_fingerprint_checked() {
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("private");
        std::fs::create_dir_all(private.join("osm-tags")).unwrap();
        std::fs::write(private.join("osm-tags/2026-11-01T13-01Z.json"), SNAPSHOT).unwrap();
        let paths = crate::BenchPaths::new(dir.path(), Some(dir.path()), Some(&private));
        let sum = crate::hash::sha256_bytes(SNAPSHOT.as_bytes());
        let manifest = |sum: &str, date: &str| crate::manifest::Manifest {
            v1: Some(crate::manifest::PersoV1 {
                donnees_serveur: Some(date.into()),
                osm_tags: Some((
                    "private/osm-tags/2026-11-01T13-01Z.json".into(),
                    sum.into(),
                    "2026-11-01T13:01:00Z".into(),
                )),
                ..Default::default()
            }),
            ..Default::default()
        };
        let t = OsmTags::load(&paths, &manifest(&sum, "2026-11-01T13:01Z")).unwrap();
        assert_eq!(error_ranges(CSV, &t), vec![0.0..150.0, 500.0..600.0]);
        let e = OsmTags::load(&paths, &manifest(&"0".repeat(64), "2026-11-01T13:01Z")).unwrap_err();
        assert!(e.to_string().contains("empreinte"), "{e}");
        let e = OsmTags::load(&paths, &manifest(&sum, "2026-11-08T09:00Z")).unwrap_err();
        assert!(e.to_string().contains("aucun instantané"), "{e}");
        // Debug sans identifiant de voie.
        assert_eq!(format!("{t:?}"), "OsmTags { date: \"2026-11-01T13:01Z\", voies_dures: 2 }");
    }
}
