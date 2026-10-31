//! Catalogue des sources configurées : attributions et versions déclarées (T02 §9).
//!
//! La version de Mapterhorn est lue au démarrage (`startup::mapterhorn_version`) ou fixée par la
//! configuration ; la date de lecture IGN (`read_at`) vient du journal de la requête.

use ord_domain::catalog::{Attribution, DataCatalog, DataVersion, DataVersions, Readiness, SourceRole};
use ord_domain::ctx::DataLog;
use ord_domain::tiles::SourceId;

/// Sources configurées, telles que décrites par la configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredSources {
    /// MNT IGN réellement lu par le lecteur de terrain (IGN activé et mode `direct`).
    pub ign_enabled: bool,
    pub ign_zoom: u8,
    pub mapterhorn_zoom: u8,
    pub mapterhorn_version: Option<String>,
    /// Version de l'index de couverture chargé (`None` : pas d'index).
    pub coverage_version: Option<String>,
    pub streets_zoom: u8,
}

/// Date `AAAA-MM-JJ` (UTC) d'un instant, sans dépendance de calendrier.
pub fn utc_date(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Catalogue sans I/O. Les versions lues en ligne (`tiles.json`, `tilejson.json`) et la liste
/// complète des sources Mapterhorn viendront avec les sources réelles.
#[derive(Clone, Debug)]
pub struct StaticCatalog {
    sources: ConfiguredSources,
}

impl StaticCatalog {
    pub fn new(sources: ConfiguredSources) -> Self {
        Self { sources }
    }

    /// Sources d'élévation configurées, dans l'ordre de priorité.
    pub fn elevation_sources(&self) -> Vec<DataVersion> {
        let mut out = Vec::new();
        if self.sources.ign_enabled {
            out.push(DataVersion {
                source: "ign-lidar-hd",
                id: None,
                date: None,
                zoom: Some(self.sources.ign_zoom),
                // Produits configurés ; une réponse ne liste que ceux qu'elle a lus.
                products: vec!["mnt", "mns"],
                role: SourceRole::Primary,
                indicative: true,
            });
        }
        out.push(DataVersion {
            source: "mapterhorn",
            id: self.sources.mapterhorn_version.clone(),
            date: None,
            zoom: Some(self.sources.mapterhorn_zoom),
            products: Vec::new(),
            role: if self.sources.ign_enabled { SourceRole::Fallback } else { SourceRole::Primary },
            indicative: true,
        });
        if let Some(version) = &self.sources.coverage_version {
            out.push(DataVersion {
                source: "mapterhorn-coverage",
                id: None,
                date: Some(version.clone()),
                zoom: Some(12),
                products: vec!["coverage-index", "coverage"],
                role: SourceRole::Coverage,
                indicative: true,
            });
        }
        out
    }

    fn typology_source(&self) -> DataVersion {
        DataVersion {
            source: "versatiles-shortbread",
            id: None,
            date: None,
            zoom: Some(self.sources.streets_zoom),
            products: Vec::new(),
            role: SourceRole::Primary,
            indicative: true,
        }
    }
}

impl DataCatalog for StaticCatalog {
    fn attributions(&self) -> Vec<Attribution> {
        let mut out = vec![
            Attribution {
                source: SourceId::Streets,
                short: "© OpenStreetMap contributors".into(),
                licence: "ODbL-1.0".into(),
                url: Some("https://www.openstreetmap.org/copyright".into()),
            },
            Attribution {
                source: SourceId::Streets,
                short: "© ESA WorldCover project 2021 / Contains modified Copernicus Sentinel data (2021) processed by ESA WorldCover consortium".into(),
                licence: "CC-BY-4.0".into(),
                url: Some("https://esa-worldcover.org".into()),
            },
        ];
        if self.sources.ign_enabled {
            out.push(Attribution {
                source: SourceId::IgnDtm,
                short: "© IGN — LiDAR HD".into(),
                licence: "Licence Ouverte Etalab 2.0".into(),
                url: Some("https://geoservices.ign.fr/lidarhd".into()),
            });
        }
        out.push(Attribution {
            source: SourceId::Mapterhorn,
            short: "Mapterhorn and its sources".into(),
            licence: "see https://mapterhorn.com/attribution".into(),
            url: Some("https://mapterhorn.com/attribution".into()),
        });
        out
    }

    /// Sources **utilisées** par la requête, lues dans son journal (T01 §9).
    fn data_versions(&self, log: &DataLog) -> DataVersions {
        let used = |ids: &[SourceId]| ids.iter().any(|id| log.oldest_read.contains_key(id));
        let typology = used(&[SourceId::Streets]).then(|| self.typology_source());
        let elevation = self
            .elevation_sources()
            .into_iter()
            .filter_map(|mut v| match v.source {
                "ign-lidar-hd" => {
                    let read = [SourceId::IgnDtm, SourceId::IgnDsm]
                        .iter()
                        .filter_map(|id| log.oldest_read.get(id))
                        .min()
                        .copied()?;
                    // Version IGN = date de la plus ancienne lecture (T02 §9) ; produits lus.
                    v.date = Some(utc_date(read));
                    v.products = [(SourceId::IgnDtm, "mnt"), (SourceId::IgnDsm, "mns")]
                        .iter()
                        .filter(|(id, _)| log.oldest_read.contains_key(id))
                        .map(|(_, p)| *p)
                        .collect();
                    Some(v)
                }
                _ => used(&[SourceId::Mapterhorn]).then_some(v),
            })
            .collect();
        DataVersions { typology, elevation }
    }

    fn readiness(&self) -> Readiness {
        // Aucune donnée locale à l'incrément 0 : rien qui puisse être illisible.
        Readiness { ready: true, checks: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(ign_enabled: bool) -> ConfiguredSources {
        ConfiguredSources {
            ign_enabled,
            ign_zoom: 15,
            mapterhorn_zoom: 12,
            mapterhorn_version: None,
            coverage_version: None,
            streets_zoom: 14,
        }
    }

    #[test]
    fn mapterhorn_is_fallback_only_when_ign_is_enabled() {
        let with_ign = StaticCatalog::new(sources(true)).elevation_sources();
        assert_eq!(with_ign.len(), 2);
        assert_eq!(with_ign[1].role, SourceRole::Fallback);
        let without = StaticCatalog::new(sources(false));
        assert_eq!(without.elevation_sources()[0].role, SourceRole::Primary);
        assert!(!without.attributions().iter().any(|a| a.source == SourceId::IgnDtm));
    }

    #[test]
    fn data_versions_list_used_sources_only() {
        let catalog = StaticCatalog::new(sources(true));
        let mut log = DataLog::default();
        assert_eq!(catalog.data_versions(&log), DataVersions::default());
        log.oldest_read.insert(SourceId::Mapterhorn, std::time::UNIX_EPOCH);
        let v = catalog.data_versions(&log);
        assert!(v.typology.is_none());
        assert_eq!(v.elevation.iter().map(|e| e.source).collect::<Vec<_>>(), ["mapterhorn"]);
        log.oldest_read
            .insert(SourceId::IgnDtm, std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_793_404_800));
        let v = catalog.data_versions(&log);
        assert_eq!(v.elevation[0].source, "ign-lidar-hd");
        assert_eq!(v.elevation[0].date.as_deref(), Some("2026-10-31"));
    }
}
