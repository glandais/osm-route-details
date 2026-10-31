//! Catalogue des sources configurées : attributions et versions déclarées (T02 §9).

use ord_domain::catalog::{Attribution, DataCatalog, DataVersion, DataVersions, Readiness, SourceRole};
use ord_domain::ctx::DataLog;
use ord_domain::tiles::SourceId;

/// Sources configurées, telles que décrites par la configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredSources {
    pub ign_enabled: bool,
    pub ign_zoom: u8,
    pub mapterhorn_zoom: u8,
    pub mapterhorn_version: Option<String>,
    pub streets_zoom: u8,
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

    fn data_versions(&self, _log: &DataLog) -> DataVersions {
        DataVersions { typology: Some(self.typology_source()), elevation: self.elevation_sources() }
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
}
