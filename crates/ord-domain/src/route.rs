//! Parcours préparé (T01 §5.1.1), produit par `ord-core::prepare`.

use std::fmt;

use crate::LatLon;

/// Station du parcours densifié : point d'entrée ou point interpolé tous les 5 m.
#[derive(Clone, Copy, PartialEq)]
pub struct RouteSample {
    pub at: LatLon,
    /// Distance cumulée depuis le début, en mètres.
    pub d_m: f64,
    /// Indice client si la station est un point d'entrée.
    pub input_index: Option<u32>,
    /// Indice client du dernier point d'entrée qui précède (ou égal).
    pub prev_input: u32,
}

impl fmt::Debug for RouteSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteSample")
            .field("d_m", &self.d_m)
            .field("input_index", &self.input_index)
            .field("prev_input", &self.prev_input)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq)]
pub struct PreparedRoute {
    pub samples: Vec<RouteSample>,
    /// Rang dans `samples` de chaque point d'entrée dédoublonné.
    pub input_pos: Vec<u32>,
    pub length_m: f64,
}

impl fmt::Debug for PreparedRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedRoute")
            .field("samples", &self.samples.len())
            .field("length_m", &self.length_m)
            .finish_non_exhaustive()
    }
}
