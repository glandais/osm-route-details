//! Arrondis de sortie, fixés dans l'assembleur et versionnés (T01 §9).

/// Altitudes et distances : au centimètre.
pub(crate) fn cm(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Ratios : à 1e-4.
pub(crate) fn ratio(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// Coordonnées : à 1e-6 degré.
pub(crate) fn coord(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

/// Pentes : au 0,01 %.
pub(crate) fn pct(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
