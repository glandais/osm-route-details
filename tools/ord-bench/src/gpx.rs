//! Lecture minimale des GPX du banc (`trkpt` et `rtept`, altitude facultative).

use std::path::Path;

use anyhow::Context;

#[derive(Clone, Copy, PartialEq)]
pub struct GpxPoint {
    pub lat: f64,
    pub lon: f64,
    pub ele: Option<f64>,
}

impl std::fmt::Debug for GpxPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GpxPoint(..)")
    }
}

pub fn parse(text: &str) -> anyhow::Result<Vec<GpxPoint>> {
    let doc = roxmltree::Document::parse(text).context("GPX illisible")?;
    let mut out = Vec::new();
    for node in doc.descendants().filter(|n| matches!(n.tag_name().name(), "trkpt" | "rtept")) {
        let attr = |name: &str| -> anyhow::Result<f64> {
            node.attribute(name)
                .context("attribut lat/lon manquant")?
                .trim()
                .parse::<f64>()
                .context("lat/lon invalide")
        };
        let ele = node
            .children()
            .find(|c| c.tag_name().name() == "ele")
            .and_then(|c| c.text())
            .and_then(|t| t.trim().parse::<f64>().ok());
        out.push(GpxPoint { lat: attr("lat")?, lon: attr("lon")?, ele });
    }
    Ok(out)
}

pub fn read(path: &Path) -> anyhow::Result<Vec<GpxPoint>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_track_and_route_points() {
        let g = r#"<?xml version="1.0"?><gpx xmlns="http://www.topografix.com/GPX/1/1"><trk><trkseg>
            <trkpt lat="1.5" lon="2.5"><ele>10.25</ele></trkpt><trkpt lat="1.6" lon="2.6"/></trkseg></trk>
            <rte><rtept lat="3" lon="4"/></rte></gpx>"#;
        let pts = parse(g).unwrap();
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0].ele, Some(10.25));
        assert_eq!(pts[1].ele, None);
        assert_eq!((pts[2].lat, pts[2].lon), (3.0, 4.0));
    }
}
