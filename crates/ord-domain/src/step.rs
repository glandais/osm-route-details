//! Calcul par étapes sans I/O : le cœur rend les tuiles qui lui manquent (T01 §1, §5).

use crate::tiles::{SourceId, TileKey};

#[derive(Debug)]
pub enum Step<T> {
    Done(T),
    Need(TilePlan),
}

/// Tuiles à lire, triées et sans doublon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TilePlan {
    pub requests: Vec<(SourceId, TileKey)>,
}

impl TilePlan {
    pub fn new(mut requests: Vec<(SourceId, TileKey)>) -> Self {
        requests.sort_unstable();
        requests.dedup();
        Self { requests }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_is_sorted_and_deduplicated() {
        let k = |x| TileKey { z: 14, x, y: 1 };
        let plan = TilePlan::new(vec![
            (SourceId::Streets, k(2)),
            (SourceId::Streets, k(1)),
            (SourceId::Streets, k(2)),
        ]);
        assert_eq!(plan.requests, vec![(SourceId::Streets, k(1)), (SourceId::Streets, k(2))]);
    }
}
