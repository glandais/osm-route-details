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
//! manifest (`perso_v1.osm_tags_reference` ; V1.1 : `serie_v11.osm_tags_reference`), celui de la
//! date du recalage du cas, lu sans réseau ; empreinte différente ou absent : refus ; voie
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

    /// Instantané de la date des données du serveur Valhalla de `perso_v1` (calibration) : voir
    /// `load_for`.
    pub fn load(paths: &crate::BenchPaths, manifest: &crate::manifest::Manifest) -> anyhow::Result<Self> {
        let v1 = manifest.v1.as_ref().context("erreur de la référence R : section perso_v1 absente")?;
        Self::load_for(paths, v1)
    }

    /// Instantané d'un cas : celui de la date de son propre recalage (section du cas,
    /// `Manifest::recalage_section` ; V1.1 : `serie_v11.recalage`, `serie_v11.osm_tags_reference`).
    pub fn load_case(
        paths: &crate::BenchPaths,
        manifest: &crate::manifest::Manifest,
        id: &str,
    ) -> anyhow::Result<Self> {
        let sec = manifest
            .recalage_section(id)
            .context("erreur de la référence R : aucune section de recalage au manifest")?;
        Self::load_for(paths, sec)
    }

    /// Instantané de la date des données du serveur Valhalla d'une section (`recalage.donnees_serveur`) :
    /// entrée `osm_tags_reference` de la section pour `private/osm-tags/<date>.json`, à la même
    /// date, empreinte vérifiée. Aucun instantané à cette date : refus (le README n'en prévoit pas
    /// d'autre source, et le banc ne lit pas le réseau).
    pub fn load_for(paths: &crate::BenchPaths, sec: &crate::manifest::PersoV1) -> anyhow::Result<Self> {
        let date = sec
            .donnees_serveur
            .as_deref()
            .context("erreur de la référence R : date des données du serveur Valhalla absente du manifest")?;
        let fichier = snapshot_file(date);
        let Some((f, sum, d)) = sec.osm_tags.as_ref().filter(|(f, _, _)| *f == fichier) else {
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

    /// Date de l'instantané (à la minute).
    pub fn date(&self) -> &str {
        &self.date
    }

    fn is_hard(&self, way: u64) -> bool {
        self.hard.contains(&way)
    }
}

/// Tags comparés entre les deux dates (README §4 quater.1, criteria
/// `reference_R1_R2_v12.tags_differents.tags`).
pub const COMPARED_TAGS: [&str; 3] = ["highway", "surface", "tracktype"];

/// Instantané de toutes les voies d'une référence (`osm_tags_reference.py --toutes`) : voies
/// demandées et tags comparés de celles trouvées. Pas de `Debug` dérivé (identifiants privés).
pub struct WayTags {
    date: String,
    asked: BTreeSet<u64>,
    tags: std::collections::BTreeMap<u64, [Option<String>; 3]>,
}

impl std::fmt::Debug for WayTags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WayTags").field("date", &self.date).field("voies", &self.asked.len()).finish()
    }
}

impl WayTags {
    /// Instantané lu depuis son JSON (`{date, voies_retenues: toutes, demandees, tags}`), daté `date`.
    pub fn from_json(bytes: &[u8], date: &str) -> anyhow::Result<Self> {
        let v: Value = serde_json::from_slice(bytes).context("instantané des tags OSM illisible")?;
        let own = v["date"].as_str().context("instantané des tags OSM sans date")?;
        if date_key(own) != date_key(date) {
            bail!("instantané des tags OSM daté {own}, pas {date}");
        }
        if v["voies_retenues"] != "toutes" {
            bail!(
                "instantané des tags OSM du {own} : voies_retenues ≠ toutes (osm_tags_reference.py --toutes)"
            );
        }
        let asked = v["demandees"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        let tags = v["tags"]
            .as_object()
            .context("instantané des tags OSM sans `tags`")?
            .iter()
            .filter_map(|(k, t)| {
                Some((k.parse().ok()?, COMPARED_TAGS.map(|n| t[n].as_str().map(str::to_owned))))
            })
            .collect();
        Ok(Self { date: date_key(date), asked, tags })
    }

    /// Instantané d'une date parmi les entrées `osm_tags_toutes` d'une section, empreinte vérifiée.
    fn load(
        paths: &crate::BenchPaths,
        entries: &[(String, String, String)],
        date: &str,
    ) -> anyhow::Result<Self> {
        let Some((f, sum, _)) = entries.iter().find(|(_, _, d)| date_key(d) == date_key(date)) else {
            bail!(
                "tags différents entre dates (README §4 quater.1) : aucun instantané osm_tags_toutes au \
                 manifest à la date {date}"
            );
        };
        let bytes = std::fs::read(paths.resolve(f)).with_context(|| format!("{f} : instantané absent"))?;
        if crate::hash::sha256_bytes(&bytes) != *sum {
            bail!("{f} : empreinte différente de celle du manifest");
        }
        Self::from_json(&bytes, date)
    }
}

/// Les deux instantanés de toutes les voies d'une section : date OSM des tuiles de la campagne et
/// date des données du serveur Valhalla de la référence (README §4 quater.1, d6fd42c).
pub struct TagsBetweenDates {
    tiles: WayTags,
    reference: WayTags,
}

impl std::fmt::Debug for TagsBetweenDates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TagsBetweenDates")
            .field("tuiles", &self.tiles.date)
            .field("reference", &self.reference.date)
            .finish_non_exhaustive()
    }
}

impl TagsBetweenDates {
    pub fn new(tiles: WayTags, reference: WayTags) -> Self {
        Self { tiles, reference }
    }

    /// Instantanés d'une section (`osm_tags_toutes`) : `None` si la section n'en déclare aucun
    /// (règle non appliquée, rapportée) ; déclarés mais celui d'une des deux dates absent, illisible
    /// ou d'empreinte différente : refus. Date des tuiles = campagne de la section.
    pub fn load_for(
        paths: &crate::BenchPaths,
        manifest: &crate::manifest::Manifest,
        sec: &crate::manifest::PersoV1,
    ) -> anyhow::Result<Option<Self>> {
        if sec.osm_tags_toutes.is_empty() {
            return Ok(None);
        }
        let tiles = manifest.tile_campaign(sec).and_then(|c| c.date_osm.clone()).context(
            "tags différents entre dates : date OSM des tuiles absente du manifest (tuiles_*.date_osm)",
        )?;
        let reference = sec.donnees_serveur.as_deref().context(
            "tags différents entre dates : date des données du serveur Valhalla absente du manifest",
        )?;
        Ok(Some(Self {
            tiles: WayTags::load(paths, &sec.osm_tags_toutes, &tiles)?,
            reference: WayTags::load(paths, &sec.osm_tags_toutes, reference)?,
        }))
    }

    /// La voie diffère-t-elle entre les deux dates (un des tags comparés, ou absente d'une seule) ?
    /// Voie non demandée à l'une des dates : instantané incomplet, refus.
    fn differs(&self, way: u64) -> anyhow::Result<bool> {
        if !self.tiles.asked.contains(&way) || !self.reference.asked.contains(&way) {
            bail!(
                "tags différents entre dates : voie de la référence absente d'un instantané (non demandée)"
            );
        }
        Ok(self.tiles.tags.get(&way) != self.reference.tags.get(&way))
    }
}

/// Plages de `<id>.surface.csv` (abscisse du fichier, toutes classes, hors discontinuités) dont la
/// voie diffère entre les deux dates, triées, contiguës réunies.
pub fn differing_ranges(csv: &str, tags: &TagsBetweenDates) -> anyhow::Result<Vec<Range<f64>>> {
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split(',').map(str::trim).collect();
    let col = |name: &str| header.iter().position(|h| *h == name);
    let (Some(a), Some(b), Some(way)) = (col("s_debut_m"), col("s_fin_m"), col("way_id")) else {
        bail!("surface.csv sans s_debut_m, s_fin_m ou way_id");
    };
    let usage = col("use");
    let mut out = Vec::new();
    for l in lines {
        let f: Vec<&str> = l.split(',').map(str::trim).collect();
        if usage.and_then(|u| f.get(u)).is_some_and(|u| *u == "discontinuite") {
            continue;
        }
        let Some(w) = f.get(way).and_then(|w| w.parse::<f64>().ok()).filter(|w| w.is_finite() && *w >= 0.0)
        else {
            continue;
        };
        if !tags.differs(w as u64)? {
            continue;
        }
        let (Some(from), Some(to)) =
            (f.get(a).and_then(|x| x.parse::<f64>().ok()), f.get(b).and_then(|x| x.parse::<f64>().ok()))
        else {
            continue;
        };
        if to > from {
            out.push(from..to);
        }
    }
    out.sort_by(|x: &Range<f64>, y| x.start.total_cmp(&y.start));
    let mut merged: Vec<Range<f64>> = Vec::new();
    for r in out {
        match merged.last_mut() {
            Some(last) if r.start <= last.end + 1e-6 => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    Ok(merged)
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

    /// Voies aux tags différents entre dates (README §4 quater.1) : `<prefix>.tags_differents`
    /// (longueur et part de la référence classée avant exclusion).
    pub fn tags_json_into(&self, typo: &mut Value, prefix: &str) {
        typo[format!("{prefix}.tags_differents")] =
            serde_json::json!({"longueur_m": round(self.excluded_m, 1), "part": round(self.part(), 4)});
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
    // Sauts de s_origine (README §4 quater.1, d6fd42c) : la référence y est `UNKNOWN`, une plage
    // ramenée ne s'y étire pas.
    let jumps: Vec<Range<f64>> = (1..xr.len())
        .filter(|&i| (xo[i] - xo[i - 1]) - (xr[i] - xr[i - 1]) > S_ORIGINE_JUMP_M)
        .map(|i| xo[i - 1]..xo[i])
        .collect();
    let mapped: Vec<Range<f64>> =
        cut.iter().map(|c| orig(c.start)..orig(c.end)).filter(|c| c.end > c.start).collect();
    Ok(minus(&mapped, &jumps))
}

/// Saut de s_origine entre deux points appariés consécutifs (Δs_origine − Δs_recale, m) au-delà
/// duquel la plage de l'itinéraire est `UNKNOWN` (`baro_gap_max_m`, README §4 quater.1).
pub const S_ORIGINE_JUMP_M: f64 = 100.0;

/// Plages `cut` privées des plages `holes` (triées en sortie, vides retirées).
pub fn minus(cut: &[Range<f64>], holes: &[Range<f64>]) -> Vec<Range<f64>> {
    let mut out = Vec::new();
    for c in cut {
        let mut from = c.start;
        let mut hs: Vec<&Range<f64>> = holes.iter().filter(|h| h.start < c.end && h.end > c.start).collect();
        hs.sort_by(|a, b| a.start.total_cmp(&b.start));
        for h in hs {
            if h.start > from {
                out.push(from..h.start);
            }
            from = from.max(h.end);
        }
        if c.end > from {
            out.push(from..c.end);
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
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

    /// Saut de s_origine (README §4 quater.1) : Δs_origine − Δs_recale > 100 m, la plage ramenée
    /// ne s'étire pas sur l'aller-retour ; 100 m tout juste : pas un saut.
    #[test]
    fn mapped_ranges_stop_at_s_origine_jumps() {
        let baro = "s_recale_m,s_origine_m,insere\n0,0,0\n100,100,0\n113,588,0\n300,775,0\n400,975,0\n";
        let cut = to_origin(&[50.0..200.0], baro).unwrap();
        assert_eq!(cut.len(), 2, "{cut:?}");
        assert_eq!(cut[0], 50.0..100.0);
        assert!((cut[1].start - 588.0).abs() < 1e-9 && (cut[1].end - 675.0).abs() < 1e-9, "{cut:?}");
        let baro = "s_recale_m,s_origine_m,insere\n0,0,0\n100,100,0\n110,210,0\n";
        assert_eq!(to_origin(&[50.0..110.0], baro).unwrap(), vec![50.0..210.0]);
        assert_eq!(minus(&[0.0..10.0, 20.0..30.0], &[5.0..25.0]), vec![0.0..5.0, 25.0..30.0]);
    }

    fn way_tags(date: &str, tags: &str) -> WayTags {
        let json =
            format!(r#"{{"date":"{date}","voies_retenues":"toutes","demandees":[1,2,3,4,5],"tags":{tags}}}"#);
        WayTags::from_json(json.as_bytes(), date).unwrap()
    }

    /// Tags différents entre dates (README §4 quater.1) : `tracktype` changé, voie absente d'une
    /// seule date ; mêmes tags ou absente des deux : gardée ; toutes classes ; discontinuités
    /// ignorées ; voie non demandée : refus ; instantané sans `--toutes` : refus.
    #[test]
    fn ways_whose_tags_differ_between_dates_leave_r1_r2() {
        let tiles = way_tags(
            "2026-10-19T00:00:04Z",
            r#"{"1":{"highway":"track","tracktype":"grade1"},"2":{"highway":"tertiary","surface":"asphalt"},"3":{"highway":"path"}}"#,
        );
        let reference = way_tags(
            "2026-11-01T13:01:00Z",
            r#"{"1":{"highway":"track","tracktype":"grade2"},"2":{"highway":"tertiary","surface":"asphalt","name":"x"},"4":{"highway":"service"}}"#,
        );
        let t = TagsBetweenDates::new(tiles, reference);
        let csv = "s_debut_m,s_fin_m,surface,road_class,use,way_id\n0,100,gravel,track,road,1\n\
                   100,200,paved,tertiary,road,2\n200,250,dirt,path,road,3\n250,300,paved,service,road,4\n\
                   300,350,,,discontinuite,\n350,400,paved,tertiary,road,5\n";
        assert_eq!(differing_ranges(csv, &t).unwrap(), vec![0.0..100.0, 200.0..300.0]);
        let e = differing_ranges("s_debut_m,s_fin_m,surface,way_id\n0,10,paved,9\n", &t).unwrap_err();
        assert!(e.to_string().contains("non demandée"), "{e}");
        let e = WayTags::from_json(SNAPSHOT.as_bytes(), "2026-11-01T13:01Z").unwrap_err();
        assert!(e.to_string().contains("toutes"), "{e}");
        assert_eq!(
            format!("{t:?}"),
            "TagsBetweenDates { tuiles: \"2026-10-19T00:00:04Z\", reference: \"2026-11-01T13:01Z\", .. }"
        );
    }

    /// Instantanés `osm_tags_toutes` d'une section : aucun déclaré, règle non appliquée (`None`) ;
    /// déclarés, celui de la date des tuiles (campagne `tuiles_*`) et celui de la date de la
    /// référence lus et vérifiés ; l'un des deux absent : refus.
    #[test]
    fn both_dated_snapshots_come_from_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("private");
        std::fs::create_dir_all(private.join("osm-tags")).unwrap();
        let body = |date: &str| {
            format!(
                r#"{{"date":"{date}","voies_retenues":"toutes","demandees":[1],"tags":{{"1":{{"highway":"track"}}}}}}"#
            )
        };
        let (a, b) = (body("2026-10-19T00:00:04Z"), body("2026-11-01T13:01:00Z"));
        std::fs::write(private.join("osm-tags/a.json"), &a).unwrap();
        std::fs::write(private.join("osm-tags/b.json"), &b).unwrap();
        let paths = crate::BenchPaths::new(dir.path(), Some(dir.path()), Some(&private));
        let entry = |f: &str, body: &str, date: &str| {
            (
                format!("private/osm-tags/{f}.json"),
                crate::hash::sha256_bytes(body.as_bytes()),
                date.to_owned(),
            )
        };
        let manifest = crate::manifest::Manifest {
            tuiles: vec![crate::manifest::TileCampaign {
                id: "tuiles_2026_11_02".into(),
                espace_cache: "versatiles".into(),
                date_osm: Some("2026-10-19T00:00:04Z".into()),
            }],
            ..Default::default()
        };
        let sec = |toutes: Vec<(String, String, String)>| crate::manifest::PersoV1 {
            donnees_serveur: Some("2026-11-01T13:01Z".into()),
            osm_tags_toutes: toutes,
            ..Default::default()
        };
        assert!(TagsBetweenDates::load_for(&paths, &manifest, &sec(Vec::new())).unwrap().is_none());
        let both = sec(vec![entry("a", &a, "2026-10-19T00:00:04Z"), entry("b", &b, "2026-11-01T13:01:00Z")]);
        let t = TagsBetweenDates::load_for(&paths, &manifest, &both).unwrap().unwrap();
        assert!(!t.differs(1).unwrap());
        let one = sec(vec![entry("b", &b, "2026-11-01T13:01:00Z")]);
        let e = TagsBetweenDates::load_for(&paths, &manifest, &one).unwrap_err();
        assert!(e.to_string().contains("2026-10-19"), "{e}");
        let wrong = sec(vec![entry("a", &b, "2026-10-19T00:00:04Z"), entry("b", &b, "2026-11-01T13:01:00Z")]);
        let e = TagsBetweenDates::load_for(&paths, &manifest, &wrong).unwrap_err();
        assert!(e.to_string().contains("empreinte"), "{e}");
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
