//! Répartition générale des figures par type (lot 4, 4c, §15.3). **Provisoire** : lentille seule, jusqu'à la
//! sous-commande `shape-common` du banc qui écrit les constantes tirées des boucles réelles.

use crate::{FigureMix, RideType};

pub(crate) fn general(_ride: RideType) -> FigureMix {
    FigureMix::LENS_ONLY
}
