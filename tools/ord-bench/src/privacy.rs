//! Garde-fou de vie privée des rapports (T05 §3.4) : un rapport ne porte ni coordonnée ni
//! description d'un ouvrage perso. Les ouvrages `nv-NN` (dérivés des traces personnelles) ne
//! sont cités que par leur identifiant opaque : jamais leur nom, leur longueur OSM ni leurs points.
//!
//! Vérification avant toute écriture : aucune clé interdite ni clé privée (`_…`, mesures d'un
//! ouvrage perso réservées au calcul), à aucun niveau, et aucun nom d'ouvrage perso dans le texte
//! sérialisé.

use anyhow::bail;
use serde_json::Value;

use crate::BenchPaths;

/// Clés qui porteraient une position ou la description d'un ouvrage.
pub const FORBIDDEN_KEYS: [&str; 12] = [
    "lat",
    "lon",
    "latitude",
    "longitude",
    "coords",
    "pts",
    "points",
    "milieu",
    "nom",
    "name",
    "chain",
    "length_m",
];

/// Noms des ouvrages perso (`private/ouvrages-perso.json`), à ne jamais écrire.
pub fn private_names(paths: &BenchPaths) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(paths.private.join("ouvrages-perso.json")) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o["name"].as_str())
        .filter(|n| n.chars().count() >= 4)
        .map(str::to_owned)
        .collect()
}

fn forbidden_key(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) => m.iter().find_map(|(k, x)| {
            if FORBIDDEN_KEYS.contains(&k.as_str()) || k.starts_with('_') {
                Some(k.clone())
            } else {
                forbidden_key(x)
            }
        }),
        Value::Array(a) => a.iter().find_map(forbidden_key),
        _ => None,
    }
}

/// Refuse un rapport qui fuirait une position ou un ouvrage perso (sans dire lequel).
pub fn assert_report_clean(report: &Value, private_names: &[String]) -> anyhow::Result<()> {
    if let Some(k) = forbidden_key(report) {
        bail!("rapport refusé : clé interdite `{k}` (vie privée)");
    }
    let text = report.to_string();
    if private_names.iter().any(|n| text.contains(n.as_str())) {
        bail!("rapport refusé : nom d'un ouvrage perso présent (vie privée)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn opaque_ids_pass_descriptions_do_not() {
        let names = vec!["Pont Secret".to_owned()];
        assert!(assert_report_clean(&json!({"ouvrages": [{"id": "nv-12", "E3.err_milieu_m": 0.3}]}), &names)
            .is_ok());
        assert!(assert_report_clean(&json!({"ouvrages": [{"id": "nv-12", "name": "x"}]}), &names).is_err());
        assert!(assert_report_clean(&json!({"cas": [{"diag": {"lat": 1.0}}]}), &names).is_err());
        assert!(assert_report_clean(&json!({"note": "près du Pont Secret"}), &names).is_err());
        // Clé privée d'un ouvrage perso, même imbriquée dans un agrégat.
        let nested =
            json!({"agregats": {"E3": {"liste": [{"ouvrage": "nv-12", "_emprise_d_m": [1.0, 2.0]}]}}});
        assert!(assert_report_clean(&nested, &names).is_err());
    }
}
