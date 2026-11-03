//! Couverture d'une campagne par critère bloquant (README §4 ter.4, `criteria.yaml`
//! `serie_tenue_a_l_ecart.couverture`), comptée **après** les exclusions du banc : seuls les cas,
//! ouvrages et témoins effectivement jugés comptent (hors graphe vélo, limite des données,
//! absent des données, référence indisponible ou incertaine, passage retiré au niveau, passage
//! sur ouvrage parallèle ou par-dessus un croisement sont déjà hors des mesures jugées).
//!
//! `measure` est rapportée par toute campagne (informative en calibration). Au jugement (à partir
//! de V1), `apply_minimums` compare au **minimum de jugement** (pas à la cible majorée du tirage) :
//! un critère sous son minimum vaut refus, comme un critère sauté (README §2, point 3).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context;
use serde_json::{json, Value};

/// Longueur et MNH au milieu d'un ouvrage du corpus (sous-comptes E3, E4).
pub type OuvrageInfo = BTreeMap<String, (f64, Option<f64>)>;

fn r(x: f64, d: i32) -> f64 {
    let k = 10f64.powi(d);
    (x * k).round() / k
}

/// Minimums de jugement (`criteria.yaml`, `serie_tenue_a_l_ecart.couverture.minimums`), en JSON.
pub fn minimums(bench: &Path) -> anyhow::Result<Value> {
    let text = std::fs::read_to_string(bench.join("criteria.yaml")).context("criteria.yaml")?;
    let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).context("criteria.yaml illisible")?;
    let m = &root["serie_tenue_a_l_ecart"]["couverture"]["minimums"];
    anyhow::ensure!(m.is_mapping(), "criteria.yaml : couverture.minimums absent");
    Ok(serde_json::to_value(m)?)
}

/// Clé de `criteria.yaml` (`serie_tenue_a_l_ecart.serie_<clé>`) des règles propres à la série
/// d'un incrément : `V1.1` → `serie_v11`.
pub fn increment_key(increment: &str) -> String {
    format!("serie_{}", increment.to_lowercase().replace('.', ""))
}

/// Règles propres à la série d'un incrément (`serie_tenue_a_l_ecart.serie_v11` pour V1.1, README
/// §4 ter.10, 7b1a4e9), en JSON ; `Null` si la série n'en a pas.
pub fn series_rules(bench: &Path, increment: &str) -> anyhow::Result<Value> {
    let text = std::fs::read_to_string(bench.join("criteria.yaml")).context("criteria.yaml")?;
    let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).context("criteria.yaml illisible")?;
    Ok(serde_json::to_value(&root["serie_tenue_a_l_ecart"][increment_key(increment).as_str()])?)
}

/// Minimums de jugement d'un incrément : ceux de `minimums`, groupes E2b exigés remplacés par
/// `serie_<clé>.E2b.groupes_exiges` s'il existe (V1.1 : `dplus` et `plaine`, le groupe `ouvrage`
/// rapporté sans minimum). Un critère non bloquant de la série (V1.1 : E3) garde son minimum :
/// son verdict `sous_minimum` est rapporté sans refuser (`judge::blocking_criteria_for`).
pub fn minimums_for(bench: &Path, increment: &str) -> anyhow::Result<Value> {
    let mut m = minimums(bench)?;
    apply_series_rules(&mut m, &series_rules(bench, increment)?);
    Ok(m)
}

/// Groupes E2b exigés d'une série (`E2b.groupes_exiges`) reportés dans les minimums.
pub fn apply_series_rules(minimums: &mut Value, rules: &Value) {
    if let Some(groups) = rules["E2b"]["groupes_exiges"].as_array() {
        if minimums["E2b"].is_object() {
            minimums["E2b"]["groupes"] = Value::Array(groups.clone());
        }
    }
}

/// Couverture d'une passe : `cases` = traces perso mesurées, `routes` = parcours R1/R2, `items` =
/// mesures d'ouvrage et de témoin (clés privées comprises, jamais rapportées ici), `q1` = négatifs
/// de la série mesurée. Aucune coordonnée ni longueur d'ouvrage : des comptes et des sommes.
pub fn measure(
    cases: &[Value],
    routes: &[Value],
    items: &[Value],
    info: &OuvrageInfo,
    q1: usize,
    step_m: f64,
) -> Value {
    let num = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    // Km comparés d'E1 : stations comparées hors zones de pente du baro (README §4 quater.3).
    let km_compared = |c: &Value| {
        let masked = c["zones_pente_baro"]["stations_comparees_masquees"].as_f64().unwrap_or(0.0);
        (num(&c["diagnostics"]["stations_comparees"]) - masked) * step_m / 1000.0
    };
    let km_total = |c: &Value| num(&c["diagnostics"]["stations"]) * step_m / 1000.0;
    let groupe = |c: &Value| c["groupe"].as_str().unwrap_or("").to_owned();
    // E1 : traces dplus / plaine dont E1 est mesuré.
    let e1: Vec<&Value> = cases
        .iter()
        .filter(|c| {
            matches!(groupe(c).as_str(), "dplus" | "plaine")
                && num(&c["metriques"]["E1.rmse_m"]).is_finite()
                && !crate::campaign::e1_uncertain(c)
        })
        .collect();
    let mut e2b: BTreeMap<String, usize> = BTreeMap::new();
    for g in ["dplus", "plaine", "ouvrage"] {
        e2b.insert(g.into(), 0);
    }
    for c in cases.iter().filter(|c| num(&c["metriques"]["E2b.ratio"]).is_finite()) {
        if let Some(n) = e2b.get_mut(&groupe(c)) {
            *n += 1;
        }
    }
    let by = |m: &'static str| items.iter().filter(move |i| i["metrique"] == m);
    let limited = |i: &Value| i["limite_donnees"].is_string();
    let id = |i: &Value| i["ouvrage"].as_str().unwrap_or("").to_owned();
    let e3: BTreeSet<String> = by("E3")
        .filter(|i| !limited(i) && i["reference_incertaine"] != true)
        .filter(|i| num(&i["ecart_milieu_m"]).is_finite() && num(&i["ecart_max_m"]).is_finite())
        .map(id)
        .collect();
    let len = |o: &String| info.get(o).map_or(0.0, |x| x.0);
    let mnh = |o: &String| info.get(o).and_then(|x| x.1).unwrap_or(f64::NAN);
    let e4: BTreeSet<String> = by("E4").filter(|i| !limited(i) && i["pass"].is_boolean()).map(id).collect();
    // E5 en sites (README §4 quater.4) : sites avec un passage compté ; clé `croisements` gardée.
    let e5 = crate::ouvrages::e5_sites(items).iter().filter(|s| s.pass().is_some()).count();
    let mut s1: BTreeMap<String, f64> = BTreeMap::new();
    for i in items.iter().filter(|i| !limited(i)) {
        if let Some(e) = i["S1.emprise_m"].as_f64().or_else(|| i["_S1.emprise_m"].as_f64()) {
            *s1.entry(id(i)).or_default() += e;
        }
    }
    let s2: Vec<&Value> = by("S2").collect();
    let natures: BTreeSet<&str> = s2.iter().filter_map(|i| i["nature"].as_str()).collect();
    let (mut r_km, mut r_unpaved, mut r_classed) = (0.0, 0.0, 0.0);
    for c in routes {
        r_km += num(&c["longueur_m"]) / 1000.0;
        r_unpaved += num(&c["reference_non_revetu_m"]);
        r_classed += num(&c["reference_classee_m"]);
    }
    json!({
        "E1": {"traces_france_fiables": e1.len(), "km_compares": r(e1.iter().map(|c| km_compared(c)).sum(), 1)},
        "E2b": {"traces_par_groupe": e2b},
        "E3": {"ponts": e3.len(), "ponts_mnh_10m_min": e3.iter().filter(|o| mnh(o) >= 10.0).count(),
            "ponts_plus_de_150m": e3.iter().filter(|o| len(o) > 150.0).count()},
        "E4": {"tunnels_osm": e4.len(), "tunnels_plus_de_300m": e4.iter().filter(|o| len(o) > 300.0).count()},
        "E5": {"croisements": e5},
        "S1": {"ouvrages": s1.len(), "emprise_m": r(s1.values().sum(), 1)},
        "S2": {"temoins": s2.len(), "km": r(s2.iter().map(|i| num(&i["longueur_m"])).filter(|x| x.is_finite()).sum::<f64>() / 1000.0, 3),
            "natures_min": natures.len()},
        "R1_R2": {"parcours": routes.len(), "km": r(r_km, 1),
            "part_non_revetu_min": if r_classed > 0.0 { r(r_unpaved / r_classed, 4) } else { 0.0 }},
        "M1": {"traces": cases.len(), "km": r(cases.iter().map(km_total).filter(|x| x.is_finite()).sum(), 1)},
        "Q1": {"traces_negatives": q1},
    })
}

/// Composantes sous leur minimum, par critère : `E3 → ["ponts 10 < 12"]`. Une composante non
/// numérique (`M1.traces: toutes`, `R1_R2.regions`) n'est pas comptée ici (structurelle).
pub fn shortfalls(cov: &Value, minimums: &Value) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (crit, mins) in minimums.as_object().into_iter().flatten() {
        let c = &cov[crit];
        let mut push = |s: String| out.entry(crit.clone()).or_default().push(s);
        if c.is_null() {
            push("couverture non mesurée".into());
            continue;
        }
        for (k, min) in mins.as_object().into_iter().flatten() {
            let Some(min) = min.as_f64() else { continue };
            if k == "traces_par_groupe" {
                let groups: Vec<&str> =
                    mins["groupes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                for g in groups {
                    let got = c[k][g].as_f64().unwrap_or(0.0);
                    if got < min {
                        push(format!("{k} {g} {got} < {min}"));
                    }
                }
                continue;
            }
            let got = c[k].as_f64().unwrap_or(0.0);
            if got < min {
                push(format!("{k} {got} < {min}"));
            }
        }
    }
    out
}

/// Jugement (à partir de V1) : dans chaque mode, un critère sous son minimum de couverture prend
/// le verdict `sous_minimum` (s'il n'est pas déjà `fail`) et la liste de ses manques
/// (`<critère>_sous_minimum`). Couverture absente d'un mode : tous ses critères sont sous minimum.
pub fn apply_minimums(results: &mut Value, minimums: &Value) {
    for mode in [None, Some("input_with_breakpoints")] {
        let base = match mode {
            None => &mut *results,
            Some(k) => &mut results[k],
        };
        let lacks = shortfalls(&base["couverture"], minimums);
        let Some(verdicts) = base["verdicts"].as_object_mut() else { continue };
        for (crit, list) in lacks {
            if verdicts.get(&crit).and_then(Value::as_str) != Some("fail") {
                verdicts.insert(crit.clone(), json!("sous_minimum"));
            }
            verdicts.insert(format!("{crit}_sous_minimum"), json!(list));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mins() -> Value {
        json!({
            "E3": {"ponts": 12, "ponts_mnh_10m_min": 3, "ponts_plus_de_150m": 3},
            "E2b": {"traces_par_groupe": 3, "groupes": ["dplus", "plaine", "ouvrage"]},
            "M1": {"traces": "toutes", "km": 800},
        })
    }

    #[test]
    fn components_under_their_minimum_are_listed() {
        let cov = json!({
            "E3": {"ponts": 12, "ponts_mnh_10m_min": 2, "ponts_plus_de_150m": 5},
            "E2b": {"traces_par_groupe": {"dplus": 3, "plaine": 2, "ouvrage": 9}},
            "M1": {"traces": 40, "km": 900.0},
        });
        let s = shortfalls(&cov, &mins());
        assert_eq!(s["E3"], ["ponts_mnh_10m_min 2 < 3"]);
        assert_eq!(s["E2b"], ["traces_par_groupe plaine 2 < 3"]);
        assert!(!s.contains_key("M1"));
    }

    #[test]
    fn under_minimum_or_unmeasured_is_a_refusal_verdict() {
        let mut results = json!({
            "verdicts": {"E3": "pass", "E2b": "fail", "M1": "pass"},
            "couverture": {"E3": {"ponts": 11, "ponts_mnh_10m_min": 3, "ponts_plus_de_150m": 3},
                "E2b": {"traces_par_groupe": {"dplus": 1, "plaine": 3, "ouvrage": 3}}, "M1": {"km": 1000.0}},
            "input_with_breakpoints": {"verdicts": {"E3": "pass", "E2b": "pass", "M1": "pass"}}
        });
        apply_minimums(&mut results, &mins());
        assert_eq!(results["verdicts"]["E3"], "sous_minimum");
        // Un échec reste un échec, avec ses manques.
        assert_eq!(results["verdicts"]["E2b"], "fail");
        assert_eq!(results["verdicts"]["E2b_sous_minimum"][0], "traces_par_groupe dplus 1 < 3");
        assert_eq!(results["verdicts"]["M1"], "pass");
        // Mode sans couverture : rien n'est réputé couvert.
        for c in ["E3", "E2b", "M1"] {
            assert_eq!(results["input_with_breakpoints"]["verdicts"][c], "sous_minimum");
        }
    }

    /// V1.1 (README §4 ter.10, 7b1a4e9) : minimum E2b sur `dplus` et `plaine` seulement ; E3
    /// garde son minimum (rapporté, non bloquant au jugement).
    #[test]
    fn series_rules_of_v11_drop_the_ouvrage_group() {
        assert_eq!(increment_key("V1.1"), "serie_v11");
        assert_eq!(increment_key("V1"), "serie_v1");
        let mut m = mins();
        let rules = json!({"E3": {"bloquant": false}, "E2b": {"groupes_exiges": ["dplus", "plaine"], "ouvrage": "rapporte"}});
        apply_series_rules(&mut m, &rules);
        assert_eq!(m["E2b"]["groupes"], json!(["dplus", "plaine"]));
        assert_eq!(m["E3"]["ponts"], 12);
        let cov = json!({"E2b": {"traces_par_groupe": {"dplus": 7, "plaine": 29, "ouvrage": 0}},
            "E3": {"ponts": 1, "ponts_mnh_10m_min": 0, "ponts_plus_de_150m": 0}, "M1": {"km": 900.0}});
        let s = shortfalls(&cov, &m);
        assert!(!s.contains_key("E2b"), "{s:?}");
        assert_eq!(s["E3"].len(), 3);
        // Sans règle de série (V1) : inchangé.
        let mut m1 = mins();
        apply_series_rules(&mut m1, &Value::Null);
        assert_eq!(m1, mins());
        // Les règles du dépôt (criteria.yaml) sont celles-là.
        let bench = crate::BenchPaths::repo_bench();
        let m = minimums_for(&bench, "V1.1").unwrap();
        assert_eq!(m["E2b"]["groupes"], json!(["dplus", "plaine"]));
        assert_eq!(minimums_for(&bench, "V1").unwrap(), minimums(&bench).unwrap());
    }

    #[test]
    fn coverage_counts_judged_measures_only() {
        let items = vec![
            json!({"metrique": "E3", "ouvrage": "nv-1", "ecart_milieu_m": 0.5, "ecart_max_m": 1.0}),
            json!({"metrique": "E3", "ouvrage": "nv-1", "ecart_milieu_m": 0.4, "ecart_max_m": 1.0}),
            json!({"metrique": "E3", "ouvrage": "nv-2", "ecart_milieu_m": 0.5, "ecart_max_m": 1.0, "reference_incertaine": true}),
            json!({"metrique": "E3", "ouvrage": "nv-3", "ecart_milieu_m": null, "ecart_max_m": null}),
            json!({"metrique": "E4", "ouvrage": "nv-4", "pass": true}),
            json!({"metrique": "E4", "ouvrage": "nv-5", "pass": null, "reference_indisponible": true}),
            json!({"metrique": "E4", "ouvrage": "nv-6", "pass": true, "limite_donnees": "x"}),
            json!({"metrique": "E5", "ouvrage": "nv-7", "pass": false}),
            json!({"metrique": "E5_passage_dessus", "ouvrage": "nv-8"}),
            json!({"metrique": "S2", "temoin": "t", "nature": "digue", "longueur_m": 1500.0}),
            json!({"ouvrage": "nv-1", "_S1.emprise_m": 60.0}),
        ];
        let mut info = OuvrageInfo::new();
        info.insert("nv-1".into(), (200.0, Some(12.0)));
        info.insert("nv-4".into(), (400.0, None));
        let cov = measure(&[], &[], &items, &info, 2, 5.0);
        assert_eq!(cov["E3"], json!({"ponts": 1, "ponts_mnh_10m_min": 1, "ponts_plus_de_150m": 1}));
        assert_eq!(cov["E4"], json!({"tunnels_osm": 1, "tunnels_plus_de_300m": 1}));
        assert_eq!(cov["E5"]["croisements"], 1);
        assert_eq!(cov["S1"]["ouvrages"], 1);
        assert_eq!(cov["S2"], json!({"temoins": 1, "km": 1.5, "natures_min": 1}));
        assert_eq!(cov["Q1"]["traces_negatives"], 2);
    }
}
