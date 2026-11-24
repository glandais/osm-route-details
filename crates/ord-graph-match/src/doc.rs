//! Le document `network_edges` d'une trace (api 1.8 de `/v1/route-details`, `options.edges`) et les codes qu'en
//! tirent l'outil et le service : famille de voie, sens, type de parcours.
//!
//! Les données sont **privées** : rien d'ici n'est journalisé.

use serde::Deserialize;

/// Familles de `wk` (type de voie), indices stables.
pub const WK_NAMES: [&str; 9] =
    ["absent", "major_road", "minor_road", "path", "cycleway", "track", "footway", "steps", "autre"];

pub fn wk_code(s: Option<&str>) -> u8 {
    match s {
        None => 0,
        Some("major_road") => 1,
        Some("minor_road") => 2,
        Some("path") => 3,
        Some("cycleway") => 4,
        Some("track") => 5,
        Some("footway") => 6,
        Some("steps") => 7,
        Some(_) => 8,
    }
}

/// `ow` : 0 = both (ou absent), 1 = forward, 2 = against.
pub const OW_NAMES: [&str; 3] = ["both", "forward", "against"];

pub fn ow_code(s: Option<&str>) -> u8 {
    match s {
        Some("forward") => 1,
        Some("against") => 2,
        _ => 0,
    }
}

/// Types de parcours de la surcouche, dans l'ordre des indices ; tout autre nom devient `MIXED`.
pub const TYPES: [&str; 4] = ["ROAD", "GRAVEL", "MTB", "MIXED"];

pub fn type_code(s: &str) -> u8 {
    TYPES.iter().position(|t| t.eq_ignore_ascii_case(s)).unwrap_or(3) as u8
}

/// Un document `network_edges` tel que lu : seuls les champs utiles, les autres sont ignorés.
#[derive(Debug, Default, Deserialize)]
pub struct Document {
    #[serde(default)]
    edges: Vec<RawEdge>,
    /// Longueur de la trace sur l'axe de `fm` et `tm`, m (ignorée du dépôt ; lue par la retouche, lot 7b).
    #[serde(default, deserialize_with = "lenient_number")]
    len: Option<f64>,
}

/// Un nombre, ou `None` pour tout autre type : un `len` racine mal typé ne rend pas le document illisible (le dépôt
/// ignorait cette clé avant la retouche).
fn lenient_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Ok(serde_json::Value::deserialize(d)?.as_f64())
}

#[derive(Debug, Deserialize)]
struct RawEdge {
    g: Option<String>,
    trv: Option<f64>,
    len: Option<f64>,
    ow: Option<String>,
    wk: Option<String>,
    #[serde(rename = "fn")]
    from_node: Option<i64>,
    #[serde(rename = "tn")]
    to_node: Option<i64>,
    fm: Option<f64>,
    tm: Option<f64>,
    #[serde(rename = "fo")]
    from_off: Option<f64>,
    #[serde(rename = "to")]
    to_off: Option<f64>,
    /// Classe de revêtement de route-details (`unpaved` pour le non-revêtu) : lue par le banc seulement.
    sc: Option<String>,
}

/// Une arête d'équipe, telle que lue (polyligne encore en texte).
#[derive(Debug)]
pub struct ParsedEdge {
    pub g: String,
    /// Mètres parcourus ; `NaN` si absent.
    pub trv: f32,
    pub len: f32,
    pub fm: f32,
    pub tm: f32,
    /// Entrée et sortie sur l'arête complète, depuis `fn` (`NaN` si absentes).
    pub from_off: f32,
    pub to_off: f32,
    pub from_node: i32,
    pub to_node: i32,
    pub wk: u8,
    pub ow: u8,
    /// `sc == "unpaved"` dans le document (la définition de `shapes.py`) ; faux si absent.
    pub sc_unpaved: bool,
}

fn f32_of(v: Option<f64>) -> f32 {
    v.map_or(f32::NAN, |x| x as f32)
}

fn node_of(v: Option<i64>) -> i32 {
    v.and_then(|x| i32::try_from(x).ok()).unwrap_or(-1)
}

impl Document {
    /// Longueur de l'axe de `fm` et `tm`, si le document la porte.
    pub fn len_m(&self) -> Option<f64> {
        self.len
    }

    /// Les arêtes du document, dépouillées de leurs noms de champs.
    pub fn into_edges(self) -> Vec<ParsedEdge> {
        self.edges
            .into_iter()
            .map(|e| ParsedEdge {
                g: e.g.unwrap_or_default(),
                trv: f32_of(e.trv),
                len: f32_of(e.len),
                fm: f32_of(e.fm),
                tm: f32_of(e.tm),
                from_off: f32_of(e.from_off),
                to_off: f32_of(e.to_off),
                from_node: node_of(e.from_node),
                to_node: node_of(e.to_node),
                wk: wk_code(e.wk.as_deref()),
                ow: ow_code(e.ow.as_deref()),
                sc_unpaved: e.sc.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("unpaved")),
            })
            .collect()
    }
}

/// Lit un document `network_edges` ; `None` s'il est illisible.
pub fn parse_document(bytes: &[u8]) -> Option<Vec<ParsedEdge>> {
    serde_json::from_slice::<Document>(bytes).ok().map(Document::into_edges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sc_is_read_and_defaults_to_not_unpaved() {
        let doc = br#"{"edges":[{"g":"a","trv":10,"sc":"unpaved"},{"g":"b","trv":20,"sc":"paved"},{"g":"c","trv":5}]}"#;
        let edges = parse_document(doc).unwrap();
        let flags: Vec<bool> = edges.iter().map(|e| e.sc_unpaved).collect();
        assert_eq!(flags, [true, false, false]);
    }

    #[test]
    fn a_root_len_of_another_type_does_not_make_the_document_unreadable() {
        let doc = Document::deserialize(&serde_json::json!({"len": "12", "edges": [{"g": "a"}]})).unwrap();
        assert_eq!(doc.len_m(), None);
        let doc = Document::deserialize(&serde_json::json!({"len": 12.5, "edges": []})).unwrap();
        assert_eq!(doc.len_m(), Some(12.5));
        assert!(parse_document(br#"{"len":"x","edges":[{"g":"a","trv":1}]}"#).is_some());
    }
}
