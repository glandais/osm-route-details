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

/// Valeur de `bloquant` d'un critère de série conditionné à sa couverture (`serie_v12.E3`,
/// README §4 quinquies.2, criteria `serie_v12_e3_si_couverture`).
pub const BLOQUANT_SI_COUVERTURE: &str = "si_couverture_atteinte";

/// Couverture d'un critère comptée **avant toute mesure** (manifest `serie_<clé>.couverture.<crit>`,
/// écrite par `tools/couverture_v1.py`, sans le service), comparée au minimum de jugement de
/// l'incrément (`minimums_for`). Renvoie `(atteinte, détail)` ; le détail (comptes du manifest et
/// minimum, aucun identifiant) est rapporté au jugement.
///
/// Les clés du manifest ne sont pas celles de `criteria.yaml` (E3 : `mnh_10m` pour
/// `ponts_mnh_10m_min`) : chaque minimum se lit sur la clé du manifest de même nom, sinon sur la
/// plus longue qui y est contenue. Aucun refus deviné : couverture absente du manifest, minimum
/// sans clé correspondante ou minimum du manifest différent de celui de `criteria.yaml` = erreur,
/// le gel et le jugement s'arrêtent.
pub fn coverage_before_measure(
    bench: &Path,
    increment: &str,
    criterion: &str,
) -> anyhow::Result<(bool, Value)> {
    let manifest = std::fs::read_to_string(bench.join("manifest.yaml")).context("manifest.yaml")?;
    let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&manifest).context("manifest.yaml illisible")?;
    let key = increment_key(increment);
    let cov: Value = serde_json::to_value(&root[key.as_str()]["couverture"][criterion])?;
    let reached = &cov["atteint"];
    anyhow::ensure!(
        reached.is_object(),
        "{criterion} de {increment} : couverture comptée avant toute mesure absente du manifest \
         ({key}.couverture.{criterion}.atteint) : bloquant ou non, indécidable"
    );
    let mins = minimums_for(bench, increment)?;
    let mins = mins[criterion].as_object().with_context(|| {
        format!("{criterion} : minimum de jugement absent de criteria.yaml (couverture.minimums)")
    })?;
    let lookup = |obj: &Value, k: &str| -> Option<(String, f64)> {
        let o = obj.as_object()?;
        if let Some(v) = o.get(k).and_then(Value::as_f64) {
            return Some((k.to_owned(), v));
        }
        o.iter()
            .filter(|(m, v)| k.contains(m.as_str()) && v.is_number())
            .max_by_key(|(m, _)| m.len())
            .and_then(|(m, v)| Some((m.clone(), v.as_f64()?)))
    };
    let mut ok = true;
    let mut detail = serde_json::Map::new();
    for (k, min) in mins {
        let Some(min) = min.as_f64() else { continue };
        let (mk, got) = lookup(reached, k).with_context(|| {
            format!("{criterion} de {increment} : minimum {k} sans compte au manifest ({key}.couverture)")
        })?;
        if let Some((_, declared)) = lookup(&cov["minimum"], k) {
            anyhow::ensure!(
                (declared - min).abs() < 1e-9,
                "{criterion} de {increment} : minimum {k} du manifest ({declared}) différent de criteria.yaml ({min})"
            );
        }
        ok &= got >= min;
        detail.insert(k.clone(), json!({"manifest": mk, "atteint": got, "minimum": min}));
    }
    // Statut déclaré par data-init (`statut`, b34b23f : E3 de V1.3 `rapporte_non_bloquant`) :
    // contrôlé contre les comptes, jamais lu à leur place ; contraire ou inconnu = erreur.
    let mut out = json!({"source": format!("manifest {key}.couverture.{criterion}"), "atteinte": ok, "comptes": detail});
    if let Some(statut) = declared_status(&cov, &key, increment, criterion)? {
        anyhow::ensure!(
            !ok,
            "{criterion} de {increment} : statut {statut} au manifest ({key}.couverture.{criterion}), \
             mais la couverture comptée atteint le minimum de jugement"
        );
        out["statut_manifest"] = json!(statut);
    }
    Ok((ok, out))
}

/// Statut de couverture déclaré au manifest pour un critère non bloquant faute de couverture
/// (`serie_v13.couverture.E3.statut`, b34b23f ; README §4 septies.2).
pub const STATUT_NON_BLOQUANT: &str = "rapporte_non_bloquant";

/// `statut` de `serie_<clé>.couverture.<crit>` : absent, ou `rapporte_non_bloquant` ; toute autre
/// valeur est refusée plutôt que devinée.
fn declared_status(
    cov: &Value,
    key: &str,
    increment: &str,
    criterion: &str,
) -> anyhow::Result<Option<String>> {
    match &cov["statut"] {
        Value::Null => Ok(None),
        Value::String(s) if s == STATUT_NON_BLOQUANT => Ok(Some(s.clone())),
        other => anyhow::bail!(
            "{criterion} de {increment} : statut {other} inconnu au manifest ({key}.couverture.{criterion})"
        ),
    }
}

/// Valeur de `bloquant` d'un critère jugé groupe par groupe, chaque groupe bloquant si sa
/// couverture comptée avant toute mesure atteint le minimum (`serie_v13.E2b`, README §4 septies.2,
/// criteria `serie_v13_e3_e2b_si_couverture`, dc76a6e).
pub const BLOQUANT_SI_COUVERTURE_PAR_GROUPE: &str = "si_couverture_atteinte_par_groupe";

/// Couverture d'un critère par groupe (E2b) comptée **avant toute mesure** : manifest
/// `serie_<clé>.couverture.<crit>.atteint.<groupe>` (un compte de traces par groupe, garde de
/// bruit comprise, README §4 septies.2), comparé au minimum par groupe de `criteria.yaml`
/// (`couverture.minimums.<crit>`, sa seule valeur numérique). Groupes : `serie_<clé>.<crit>.groupes`,
/// sinon ceux des minimums. Renvoie, par groupe, `atteint`, et le détail rapporté au jugement.
/// Aucun refus deviné : couverture absente, groupe sans compte ou minimum du manifest différent =
/// erreur, le gel et le jugement s'arrêtent.
pub fn coverage_before_measure_by_group(
    bench: &Path,
    increment: &str,
    criterion: &str,
) -> anyhow::Result<(BTreeMap<String, bool>, Value)> {
    let manifest = std::fs::read_to_string(bench.join("manifest.yaml")).context("manifest.yaml")?;
    let root: serde_yaml_ng::Value = serde_yaml_ng::from_str(&manifest).context("manifest.yaml illisible")?;
    let key = increment_key(increment);
    let cov: Value = serde_json::to_value(&root[key.as_str()]["couverture"][criterion])?;
    // Un statut global n'a pas de sens groupe par groupe : refusé plutôt que deviné.
    anyhow::ensure!(
        cov["statut"].is_null(),
        "{criterion} de {increment} : statut {} au manifest ({key}.couverture.{criterion}), non prévu \
         pour un critère jugé groupe par groupe",
        cov["statut"]
    );
    anyhow::ensure!(
        cov["atteint"].is_object(),
        "{criterion} de {increment} : couverture par groupe comptée avant toute mesure absente du \
         manifest ({key}.couverture.{criterion}.atteint) : bloquant ou non, indécidable"
    );
    let mins = minimums_for(bench, increment)?;
    let mins = mins[criterion].as_object().with_context(|| {
        format!("{criterion} : minimum de jugement absent de criteria.yaml (couverture.minimums)")
    })?;
    let numeric: Vec<(&String, f64)> = mins.iter().filter_map(|(k, v)| Some((k, v.as_f64()?))).collect();
    let [(min_key, min)] = numeric[..] else {
        anyhow::bail!(
            "{criterion} : un seul minimum par groupe attendu dans criteria.yaml, {} lus",
            numeric.len()
        );
    };
    if let Some(declared) = cov["minimum"].as_object().and_then(|o| {
        o.iter().filter(|(m, v)| min_key.contains(m.as_str()) && v.is_number()).max_by_key(|(m, _)| m.len())
    }) {
        let declared = declared.1.as_f64().unwrap_or(f64::NAN);
        anyhow::ensure!(
            (declared - min).abs() < 1e-9,
            "{criterion} de {increment} : minimum par groupe du manifest ({declared}) différent de criteria.yaml ({min})"
        );
    }
    let rules = series_rules(bench, increment)?;
    let groups: Vec<String> = rules[criterion]["groupes"]
        .as_array()
        .or_else(|| mins.get("groupes").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|g| g.as_str().map(str::to_owned))
        .collect();
    anyhow::ensure!(!groups.is_empty(), "{criterion} de {increment} : aucun groupe à juger");
    let mut out = BTreeMap::new();
    let mut detail = serde_json::Map::new();
    for g in groups {
        let got = cov["atteint"][g.as_str()].as_f64().with_context(|| {
            format!("{criterion} de {increment} : groupe {g} sans compte au manifest ({key}.couverture.{criterion}.atteint)")
        })?;
        detail.insert(g.clone(), json!({"atteint": got, "minimum": min, "bloquant": got >= min}));
        out.insert(g, got >= min);
    }
    Ok((
        out,
        json!({"source": format!("manifest {key}.couverture.{criterion}"), "minimum": min_key, "groupes": detail}),
    ))
}

/// Groupes bloquants d'un critère jugé groupe par groupe (`coverage_before_measure_by_group`) :
/// lus dans le détail rapporté (`groupes.<g>.bloquant`) ; `None` pour un critère qui ne l'est pas.
pub fn blocking_groups(detail: &Value) -> Option<(Vec<String>, Vec<String>)> {
    let groups = detail["groupes"].as_object()?;
    let (on, off): (Vec<_>, Vec<_>) = groups.iter().partition(|(_, g)| g["bloquant"] == true);
    Some((
        on.into_iter().map(|(k, _)| k.clone()).collect(),
        off.into_iter().map(|(k, _)| k.clone()).collect(),
    ))
}

/// Jugement groupe par groupe d'un critère (E2b, README §4 septies.2) : dans chaque mode, le
/// verdict du critère ne porte que sur ses groupes bloquants (verdicts de `agregats.<crit>.<g>`) ;
/// les autres groupes sont rapportés (`<crit>_groupes_rapportes`, avec leur verdict, et leurs
/// manques mesurés, `<crit>_groupes_rapportes_sous_minimum`), sans refuser. Le minimum mesuré ne
/// vaut plus que pour les groupes bloquants (`minimums.<crit>.groupes` restreint) : à appliquer
/// avant `apply_minimums`. Aucun groupe bloquant : verdict inchangé (critère rapporté entier).
pub fn apply_group_blocking(
    results: &mut Value,
    minimums: &mut Value,
    criterion: &str,
    on: &[String],
    off: &[String],
) {
    let all_mins = minimums.clone();
    for mode in [None, Some("input_with_breakpoints")] {
        let base = match mode {
            None => &mut *results,
            Some(k) => &mut results[k],
        };
        let agg = base["agregats"][criterion].clone();
        let verdict_of = |g: &String| agg[g.as_str()]["verdict"].as_str().unwrap_or("sans_trace").to_owned();
        let lacks = shortfalls(&base["couverture"], &all_mins);
        let Some(verdicts) = base["verdicts"].as_object_mut() else { continue };
        let reported: Vec<Value> =
            off.iter().map(|g| json!({"groupe": g, "verdict": verdict_of(g)})).collect();
        let reported_lacks: Vec<String> = lacks
            .get(criterion)
            .into_iter()
            .flatten()
            .filter(|l| off.iter().any(|g| l.split_whitespace().nth(1) == Some(g.as_str())))
            .cloned()
            .collect();
        verdicts.insert(format!("{criterion}_groupes_rapportes"), json!(reported));
        verdicts.insert(format!("{criterion}_groupes_rapportes_sous_minimum"), json!(reported_lacks));
        if on.is_empty() {
            continue;
        }
        let v: Vec<String> = on.iter().map(verdict_of).collect();
        let fails: Vec<&String> = on.iter().zip(&v).filter(|(_, v)| *v == "fail").map(|(g, _)| g).collect();
        let verdict = if !fails.is_empty() {
            "fail"
        } else if v.iter().any(|v| v == "pass") {
            "pass"
        } else if v.iter().any(|v| v == "non_discriminant") {
            "non_discriminant"
        } else {
            "skipped"
        };
        verdicts.insert(criterion.to_owned(), json!(verdict));
        verdicts.insert(format!("{criterion}_groupes_en_echec"), json!(fails));
    }
    if minimums[criterion].is_object() {
        minimums[criterion]["groupes"] = json!(on);
    }
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
                && !crate::campaign::reference_baro_excluded(c)
        })
        .collect();
    let mut e2b: BTreeMap<String, usize> = BTreeMap::new();
    for g in ["dplus", "plaine", "ouvrage"] {
        e2b.insert(g.into(), 0);
    }
    // Traces hors des références baro ou au baro bruité (README §4 sexies.2 et .3) : hors d'E2b.
    let e2b_counted = |c: &&Value| {
        num(&c["metriques"]["E2b.ratio"]).is_finite()
            && !crate::campaign::reference_baro_excluded(c)
            && !crate::campaign::e2b_noisy(c)
    };
    for c in cases.iter().filter(e2b_counted) {
        if let Some(n) = e2b.get_mut(&groupe(c)) {
            *n += 1;
        }
    }
    let by = |m: &'static str| items.iter().filter(move |i| i["metrique"] == m);
    let limited = |i: &Value| i["limite_donnees"].is_string();
    let id = |i: &Value| i["ouvrage"].as_str().unwrap_or("").to_owned();
    let e3: BTreeSet<String> = by("E3")
        .filter(|i| {
            !limited(i) && i["reference_incertaine"] != true && !i["reference_baro_exclue"].is_string()
        })
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

    /// Critère jugé groupe par groupe (README §4 septies.2, dc76a6e ; série fictive `V9`, aucune
    /// clé d'une série réelle) : un groupe sous son minimum avant mesure est rapporté, les autres
    /// restent bloquants ; leur sous-minimum mesuré refuse, celui du groupe rapporté non.
    #[test]
    fn a_criterion_by_group_blocks_only_on_groups_reached_before_measure() {
        let dir = tempfile::tempdir().unwrap();
        let b = dir.path();
        std::fs::write(
            b.join("criteria.yaml"),
            "criteres:\n  - {id: E2b, bloquant: true}\nserie_tenue_a_l_ecart:\n  couverture:\n    minimums:\n\
             \x20     E2b: {traces_par_groupe: 3, groupes: [dplus, plaine, ouvrage]}\n\
             \x20 serie_v9:\n    E2b: {bloquant: si_couverture_atteinte_par_groupe, groupes: [dplus, plaine, ouvrage]}\n",
        )
        .unwrap();
        let manifest = |atteint: &str, minimum: u32| {
            std::fs::write(
                b.join("manifest.yaml"),
                format!("serie_v9:\n  couverture:\n    E2b: {{atteint: {{{atteint}}}, minimum: {{par_groupe: {minimum}}}}}\n"),
            )
            .unwrap();
        };
        manifest("dplus: 4, plaine: 7, ouvrage: 2", 3);
        let (groups, detail) = coverage_before_measure_by_group(b, "V9", "E2b").unwrap();
        assert_eq!(groups.values().copied().collect::<Vec<_>>(), [true, false, true], "{detail}");
        let (on, off) = blocking_groups(&detail).unwrap();
        assert_eq!((on.clone(), off.clone()), (ids(&["dplus", "plaine"]), ids(&["ouvrage"])));
        let c = crate::judge::conditional_blocking(b, "V9").unwrap();
        assert!(c["E2b"].0 && crate::judge::blocking_criteria_for(b, "V9").unwrap().contains("E2b"));
        // Le groupe rapporté échoue et manque de traces ; les groupes bloquants passent.
        let mode = || {
            json!({"verdicts": {"E2b": "fail", "E2b_groupes_en_echec": ["ouvrage"]},
                "agregats": {"E2b": {"dplus": {"verdict": "pass"}, "plaine": {"verdict": "pass"}, "ouvrage": {"verdict": "fail"}}},
                "couverture": {"E2b": {"traces_par_groupe": {"dplus": 3, "plaine": 5, "ouvrage": 1}}}})
        };
        let mut results = mode();
        results["input_with_breakpoints"] = mode();
        // Mode `input_with_breakpoints` : un groupe bloquant sous son minimum mesuré refuse.
        results["input_with_breakpoints"]["couverture"]["E2b"]["traces_par_groupe"]["plaine"] = json!(2);
        let mut mins = minimums_for(b, "V9").unwrap();
        apply_group_blocking(&mut results, &mut mins, "E2b", &on, &off);
        apply_minimums(&mut results, &mins);
        let v = &results["verdicts"];
        assert_eq!(v["E2b"], "pass", "{v}");
        assert_eq!(v["E2b_groupes_rapportes"], json!([{"groupe": "ouvrage", "verdict": "fail"}]));
        assert_eq!(v["E2b_groupes_rapportes_sous_minimum"], json!(["traces_par_groupe ouvrage 1 < 3"]));
        let bp = &results["input_with_breakpoints"]["verdicts"];
        assert_eq!(bp["E2b"], "sous_minimum", "{bp}");
        assert_eq!(bp["E2b_sous_minimum"], json!(["traces_par_groupe plaine 2 < 3"]));
        // Aucun groupe atteint : critère rapporté entier ; indécidable = erreur.
        manifest("dplus: 1, plaine: 2, ouvrage: 0", 3);
        assert!(!crate::judge::blocking_criteria_for(b, "V9").unwrap().contains("E2b"));
        manifest("dplus: 4, plaine: 7", 3);
        assert!(crate::judge::conditional_blocking(b, "V9").is_err());
        manifest("dplus: 4, plaine: 7, ouvrage: 3", 2);
        assert!(crate::judge::conditional_blocking(b, "V9").is_err());
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
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
