//! Petits outils propres à l'outil : dates. L'aléa et le polygone `.poly` vivent dans `ord_graph::util`.

pub use ord_graph::util::{sample_without_replacement, Poly, SplitMix64};

/// Date civile `AAAAMMJJ` d'un horodatage Unix (algorithme de Howard Hinnant).
pub fn yyyymmdd(epoch_s: i64) -> String {
    let z = epoch_s.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(yyyymmdd(0), "19700101");
        assert_eq!(yyyymmdd(1_794_691_241), "20261114"); // 2026-11-14T21:20:41Z
        assert_eq!(yyyymmdd(951_782_400), "20000229");
    }
}
