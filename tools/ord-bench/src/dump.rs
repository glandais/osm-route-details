//! `dump-profils` : vidage local et privé des profils de traces perso de calibration (analyse
//! d'E2b), sur la grille de 5 m de la campagne en mode `distance`. Un CSV par cas dans
//! `bench/private/profils/` (hors git), jamais dans `bench/reports/`. Aucune coordonnée.
//!
//! Colonnes : `s_m` (abscisse du service), `baro_e2_m` (référence E2 telle que la campagne la
//! calcule : baro transporté, points insérés retirés selon la règle, fenêtres masquées et
//! stations sans altitude vides), `baro_decale_m` (la même, moins le décalage global
//! baro − service d'E1), `ele_m`, `ele_brut_m` (`structure_correction: false`), `ouvrage` (id du
//! corpus de la correction, ouvrages perso en `nv-NN`, `hors_corpus` sinon, vide hors
//! correction), `ele_method`, `masque` (fenêtre d'un ouvrage d'une autre série : tout est vide).

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{bail, Context};

use crate::campaign::{case_dump, ensure_check, load_perso, ouvrage_refs};
use crate::check::check;
use crate::manifest::{Manifest, Serie, Trace};
use crate::service::{Service, Variant};
use crate::{BenchPaths, BenchRules};

/// Profils d'un cas, alignés sur la grille.
#[derive(Clone, Debug, Default)]
pub struct CaseProfiles {
    pub id: String,
    pub s: Vec<f64>,
    pub baro_e2: Vec<f64>,
    /// Décalage global baro − service d'E1 (médiane sur les stations au sol).
    pub offset_m: f64,
    pub ele: Vec<f64>,
    pub ele_raw: Vec<f64>,
    pub ouvrage: Vec<String>,
    pub method: Vec<String>,
    pub masked: Vec<bool>,
}

/// Traces demandées, toutes de la série de calibration ; une trace tenue à l'écart ou inconnue
/// est refusée avant toute lecture.
pub fn calibration_traces<'a>(traces: &'a [Trace], ids: &[String]) -> anyhow::Result<Vec<&'a Trace>> {
    if ids.is_empty() {
        bail!("aucune trace demandée");
    }
    ids.iter()
        .map(|id| {
            let t = traces.iter().find(|t| &t.id == id).with_context(|| format!("{id} : trace inconnue"))?;
            if t.serie != Serie::Calibration {
                bail!("{id} : trace de la série tenue à l'écart, refusée");
            }
            Ok(t)
        })
        .collect()
}

fn num(x: f64, digits: usize) -> String {
    if x.is_finite() {
        format!("{x:.digits$}")
    } else {
        String::new()
    }
}

/// CSV d'un cas.
pub fn to_csv(p: &CaseProfiles) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# cas={} decalage_baro_moins_service_m={}", p.id, num(p.offset_m, 3));
    out.push_str("s_m,baro_e2_m,baro_decale_m,ele_m,ele_brut_m,ouvrage,ele_method,masque\n");
    for i in 0..p.s.len() {
        if p.masked[i] {
            let _ = writeln!(out, "{},,,,,,,1", num(p.s[i], 2));
            continue;
        }
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},0",
            num(p.s[i], 2),
            num(p.baro_e2[i], 2),
            num(p.baro_e2[i] - p.offset_m, 2),
            num(p.ele[i], 2),
            num(p.ele_raw[i], 2),
            p.ouvrage[i],
            p.method[i],
        );
    }
    out
}

/// `dump-profils` : écrit un CSV par cas ; renvoie `(chemin, lignes de données)`.
pub async fn dump_profiles(paths: &BenchPaths, ids: &[String]) -> anyhow::Result<Vec<(PathBuf, usize)>> {
    let report = check(paths)?;
    ensure_check(&report)?;
    if !paths.private_present() {
        bail!("bench/private/ absent");
    }
    let manifest = Manifest::load(&paths.bench)?;
    let traces = calibration_traces(&manifest.traces, ids)?;
    let rules = BenchRules::default();
    let rules_core = ord_core::rules::Rules::default();
    let refs = ouvrage_refs(paths)?;
    let ouvrages = crate::ouvrages::calibration_ouvrages(paths, &manifest)?;
    let service = Service::start(&paths.cache, true, Variant::DEFAULT).await?;
    let dir = paths.private.join("profils");
    std::fs::create_dir_all(&dir).with_context(|| format!("création de {}", dir.display()))?;
    let mut written = Vec::new();
    for t in traces {
        let cases = load_perso(paths, t, &rules).map_err(|e| anyhow::anyhow!("{} : {e}", t.id))?;
        for mut case in cases {
            if let Err(reason) = crate::inserted::apply(&mut case, &paths.cache, None, &rules).await? {
                bail!("{} : {reason}", case.id);
            }
            let p = case_dump(&service, &case, &refs, &ouvrages, &rules, &rules_core).await?;
            let path = dir.join(format!("{}.csv", p.id));
            std::fs::write(&path, to_csv(&p))?;
            written.push((path, p.s.len()));
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(id: &str, serie: Serie) -> Trace {
        Trace {
            id: id.into(),
            groupe: "plaine".into(),
            serie,
            fichier: format!("private/traces/{id}.gpx"),
            sha256: None,
            sha256_trim1km: None,
            passages_masques: Vec::new(),
            verdict: None,
            dist_km: None,
            gain_baro_m: None,
            gain_mnt_m: None,
            noise_m: None,
        }
    }

    #[test]
    fn held_out_and_unknown_traces_are_refused() {
        let traces = [trace("perso-01", Serie::Calibration), trace("perso-02", Serie::TenueALEcart)];
        let ok = calibration_traces(&traces, &["perso-01".into()]).unwrap();
        assert_eq!(ok.len(), 1);
        let err = calibration_traces(&traces, &["perso-01".into(), "perso-02".into()]).unwrap_err();
        assert!(err.to_string().contains("tenue à l'écart"), "{err}");
        assert!(calibration_traces(&traces, &["perso-99".into()]).is_err());
        assert!(calibration_traces(&traces, &[]).is_err());
    }

    #[test]
    fn csv_blanks_masked_windows_and_has_no_coordinates() {
        let p = CaseProfiles {
            id: "perso-01".into(),
            s: vec![0.0, 5.0, 10.0],
            baro_e2: vec![12.0, f64::NAN, 14.0],
            offset_m: 2.0,
            ele: vec![10.0, 11.0, 12.5],
            ele_raw: vec![10.0, 11.0, 30.0],
            ouvrage: vec![String::new(), String::new(), "nv-01".into()],
            method: vec!["dem".into(), "dem".into(), "dsm_deck".into()],
            masked: vec![false, true, false],
        };
        let csv = to_csv(&p);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[1], "s_m,baro_e2_m,baro_decale_m,ele_m,ele_brut_m,ouvrage,ele_method,masque");
        assert_eq!(lines[2], "0.00,12.00,10.00,10.00,10.00,,dem,0");
        assert_eq!(lines[3], "5.00,,,,,,,1", "fenêtre masquée : rien du service");
        assert_eq!(lines[4], "10.00,14.00,12.00,12.50,30.00,nv-01,dsm_deck,0");
        assert!(!csv.contains("lat") && !csv.contains("lon"));
    }
}
