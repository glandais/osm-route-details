//! Métriques de typologie du banc (bench §3, §4) : R1, R2 (parcours d'osm-surface), R3 (traces
//! perso recalées, contre `trace_attributes` de Stadia), M1 (appariement), et latence de la
//! typologie à chaud. Aucune coordonnée n'est écrite : longueurs et parts seulement.
//!
//! Table de classes commune (choix du banc, le plus simple) : la classe Stadia est ramenée au
//! vocabulaire gelé du service, `surface.class` ∈ {paved, unpaved, unknown}. Revêtu : Valhalla
//! `paved_smooth`, `paved`, `paved_rough` (tribly `PAVED`, `PAVED_ROUGH`) ; non revêtu : tout ce
//! qui est pire que `paved_rough` (`unpaved` de Valhalla) ; inconnu sinon.

use serde_json::{json, Value};

/// Classe de revêtement du vocabulaire du service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Paved,
    Unpaved,
    Unknown,
}

impl Class {
    pub fn of_service(s: &str) -> Self {
        match s {
            "paved" => Self::Paved,
            "unpaved" => Self::Unpaved,
            _ => Self::Unknown,
        }
    }

    /// `edge.surface` de Valhalla (réponse `trace_attributes` de Stadia).
    pub fn of_valhalla(s: &str) -> Self {
        match s.trim() {
            "paved_smooth" | "paved" | "paved_rough" => Self::Paved,
            "compacted" | "dirt" | "gravel" | "path" | "impassable" => Self::Unpaved,
            _ => Self::Unknown,
        }
    }

    /// Classe de tribly (`tracks[0].surfaces[].surface`), dérivée de Valhalla.
    pub fn of_tribly(s: &str) -> Self {
        match s {
            "PAVED" | "PAVED_ROUGH" => Self::Paved,
            "COMPACTED" | "GRAVEL" | "DIRT" | "PATH" | "IMPASSABLE" => Self::Unpaved,
            _ => Self::Unknown,
        }
    }
}

/// Plage du service, en abscisse de la référence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ours {
    pub from: f64,
    pub to: f64,
    pub class: Class,
    pub matched: bool,
}

/// Plage de référence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ref {
    pub from: f64,
    pub to: f64,
    pub class: Class,
}

/// Accord en longueur (balayage des deux partitions, comme l'éval d'osm-surface).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Agreement {
    /// R1 : classe égale, sur toute la longueur commune.
    pub class: f64,
    /// R1 hors inconnu Stadia.
    pub class_known: f64,
    /// R2 : revêtu / non revêtu, sur les mètres classables des deux côtés.
    pub paved: f64,
    /// M1 : part non appariée (`matched = false`) de la longueur du service.
    pub unmatched: f64,
    /// M1 : part `surface.class = unknown` de la longueur du service.
    pub unknown: f64,
    pub compared_m: f64,
}

pub fn agreement(ours: &[Ours], reference: &[Ref]) -> Agreement {
    let (mut total, mut agree, mut known, mut agree_known, mut paved, mut agree_paved) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut i, mut j) = (0, 0);
    while i < ours.len() && j < reference.len() {
        let from = ours[i].from.max(reference[j].from);
        let to = ours[i].to.min(reference[j].to);
        if to > from {
            let l = to - from;
            let (us, rf) = (ours[i].class, reference[j].class);
            total += l;
            if us == rf {
                agree += l;
            }
            if rf != Class::Unknown {
                known += l;
                if us == rf {
                    agree_known += l;
                }
            }
            if us != Class::Unknown && rf != Class::Unknown {
                paved += l;
                if us == rf {
                    agree_paved += l;
                }
            }
        }
        if ours[i].to <= reference[j].to {
            i += 1;
        } else {
            j += 1;
        }
    }
    let length: f64 = ours.iter().map(|o| o.to - o.from).sum();
    let share = |f: &dyn Fn(&Ours) -> bool| {
        ours.iter().filter(|o| f(o)).map(|o| o.to - o.from).sum::<f64>() / length.max(1e-9)
    };
    Agreement {
        class: agree / total.max(1e-9),
        class_known: agree_known / known.max(1e-9),
        paved: agree_paved / paved.max(1e-9),
        unmatched: share(&|o| !o.matched),
        unknown: share(&|o| o.class == Class::Unknown),
        compared_m: total,
    }
}

/// Plages du service à partir des réponses (sections `typology`), tronçons recollés : les
/// `from_m`/`to_m` du tronçon N sont décalés de `offsets[N]`, puis passés par `to_ref`.
pub fn ours_of(responses: &[Value], offsets: &[f64], to_ref: &dyn Fn(f64) -> f64) -> Vec<Ours> {
    let mut out = Vec::new();
    for (r, off) in responses.iter().zip(offsets) {
        for s in r["segments"].as_array().into_iter().flatten() {
            let (Some(a), Some(b)) = (s["from_m"].as_f64(), s["to_m"].as_f64()) else { continue };
            let (from, to) = (to_ref(a + off), to_ref(b + off));
            if to > from {
                out.push(Ours {
                    from,
                    to,
                    class: Class::of_service(s["surface"]["class"].as_str().unwrap_or("")),
                    matched: s["matched"].as_bool().unwrap_or(false),
                });
            }
        }
    }
    out
}

/// `<id>.surface.csv` de `stadia_match.py` : `s_debut_m,s_fin_m,surface,…`, triée par abscisse.
pub fn read_stadia_surface(text: &str) -> Vec<Ref> {
    let mut out: Vec<Ref> = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            Some(Ref {
                from: f.first()?.trim().parse().ok()?,
                to: f.get(1)?.trim().parse().ok()?,
                class: Class::of_valhalla(f.get(2).copied().unwrap_or("")),
            })
        })
        .filter(|r| r.to > r.from)
        .collect();
    out.sort_by(|a, b| a.from.total_cmp(&b.from));
    out
}

/// Seuils R1/R2/R3 et M1 (`criteria.yaml`).
pub const R_CLASS_MIN: f64 = 0.85;
pub const R_PAVED_MIN: f64 = 0.97;
pub const M1_UNMATCHED_MAX: f64 = 0.01;
pub const M1_UNKNOWN_MAX: f64 = 0.02;

fn r4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0 + 0.0
}

/// Métriques d'un cas, au format de `results.json`.
pub fn metrics_json(a: &Agreement, prefix: &str) -> Value {
    json!({
        format!("{prefix}.accord_classe"): r4(a.class),
        format!("{prefix}.accord_classe_hors_inconnu"): r4(a.class_known),
        format!("{prefix}.accord_revetu_non_revetu"): r4(a.paved),
        "M1.non_apparie": r4(a.unmatched),
        "M1.unknown": r4(a.unknown),
        "compare_m": (a.compared_m * 10.0).round() / 10.0
    })
}

pub fn r_pass(a: &Agreement) -> bool {
    a.class >= R_CLASS_MIN && a.class_known >= R_CLASS_MIN && a.paved >= R_PAVED_MIN
}

pub fn m1_pass(a: &Agreement) -> bool {
    a.unmatched <= M1_UNMATCHED_MAX && a.unknown <= M1_UNKNOWN_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_agreement() {
        let ours = [
            Ours { from: 0.0, to: 50.0, class: Class::Paved, matched: true },
            Ours { from: 50.0, to: 90.0, class: Class::Unpaved, matched: true },
            Ours { from: 90.0, to: 100.0, class: Class::Unknown, matched: false },
        ];
        let reference = [
            Ref { from: 0.0, to: 60.0, class: Class::Paved },
            Ref { from: 60.0, to: 80.0, class: Class::Unpaved },
            Ref { from: 80.0, to: 100.0, class: Class::Unknown },
        ];
        let a = agreement(&ours, &reference);
        assert!((a.class - 0.8).abs() < 1e-9, "{a:?}"); // 50 + 20 + 10 (inconnu = inconnu)
        assert!((a.class_known - 70.0 / 80.0).abs() < 1e-9);
        assert!((a.paved - 70.0 / 80.0).abs() < 1e-9);
        assert!((a.unmatched - 0.1).abs() < 1e-9 && (a.unknown - 0.1).abs() < 1e-9);
        assert!(!m1_pass(&a) && !r_pass(&a));
    }

    #[test]
    fn class_tables() {
        assert_eq!(Class::of_valhalla("paved_rough"), Class::Paved);
        assert_eq!(Class::of_valhalla("compacted"), Class::Unpaved);
        assert_eq!(Class::of_valhalla(""), Class::Unknown);
        assert_eq!(Class::of_tribly("DIRT"), Class::Unpaved);
        assert_eq!(Class::of_tribly("UNKNOWN"), Class::Unknown);
        let csv =
            "s_debut_m,s_fin_m,surface,road_class,use,way_id\n10,20,gravel,x,y,1\n0,10,paved_smooth,x,y,2\n";
        let r = read_stadia_surface(csv);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].from, r[0].class), (0.0, Class::Paved));
    }
}
