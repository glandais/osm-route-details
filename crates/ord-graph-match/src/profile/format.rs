//! Format en clair du fichier de profil (§14.6) : un en-tête JSON précédé de sa longueur, puis les trois tables de 72
//! `u16`. Le chiffrement (`ORDPENC1`) est l'affaire du service ; ce module ne connaît que les octets en clair.

use ord_loops::{Taste, TASTE_CLASSES};
use serde::{Deserialize, Serialize};

use super::learn::{Common, LearnParams};

/// Version du format en clair. Un format 2 (formes du 4c, destinations du 4d) montera ce numéro ; un format inconnu est
/// refusé en lecture sans jamais être effacé.
pub const FORMAT: u32 = 1;

/// L'en-tête du profil.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub format: u32,
    /// Secondes depuis l'époque, à l'écriture.
    pub learned_at: i64,
    /// Date de dépôt de la surcouche dont les boucles ont servi, secondes depuis l'époque.
    pub overlay_deposited_at: i64,
    /// `Common::hash` (16 hexadécimaux) : la table commune sur laquelle le profil a été appris.
    pub common: String,
    /// `LearnParams::hash` (16 hexadécimaux) : les réglages de l'apprentissage.
    pub learn: String,
    /// Version du moteur qui a écrit.
    pub engine: String,
    /// Pas finis (0 à 3).
    pub steps: u8,
    /// Boucles et paires comptées au dernier pas fini, par type.
    pub loops: [u32; 3],
    pub pairs: [u32; 3],
    pub normalized: bool,
    /// Date du dernier profil **complet** (tous les pas, ou ensemble vide), secondes depuis l'époque. Un profil partiel
    /// (1 ou 2 pas, budget épuisé) la recopie de celui qu'il remplace ; tant qu'aucun profil complet n'existe, c'est le
    /// début de la série de profils partiels. Absent (écrit sans) : la date d'apprentissage. Décision 6 du 17 novembre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete_at: Option<i64>,
}

impl ProfileMeta {
    /// Le profil est complet : les `steps` pas du réglage ont tous été joués, ou il n'y avait aucune boucle à rejouer.
    pub fn is_complete(&self, steps: u8) -> bool {
        self.steps >= steps || self.loops == [0, 0, 0]
    }

    /// Depuis quand le service n'a plus de profil complet pour cette équipe (secondes depuis l'époque).
    pub fn complete_since(&self) -> i64 {
        self.complete_at.unwrap_or(self.learned_at)
    }
}

/// Pourquoi un profil n'est pas servi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Incompatible {
    /// Illisible, format inconnu, tables mal formées, `q8` nul.
    Format,
    /// Appris pour une autre table commune ou d'autres réglages que ceux du moteur qui lit.
    Settings,
}

const TABLES_LEN: usize = 3 * TASTE_CLASSES * 2;

/// Hexadécimal sur 16 chiffres.
pub fn hex16(h: u64) -> String {
    format!("{h:016x}")
}

/// Les octets en clair d'un profil.
pub fn encode(meta: &ProfileMeta, taste: &Taste) -> Vec<u8> {
    let json = serde_json::to_vec(meta).expect("l'en-tête se sérialise");
    let mut out = Vec::with_capacity(4 + json.len() + TABLES_LEN);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    for t in &taste.q8 {
        for q in t {
            out.extend_from_slice(&q.to_le_bytes());
        }
    }
    out
}

/// Relit un profil et le confronte à la table commune et aux réglages du moteur qui lit. `Format` : octets inutilisables
/// (jamais effacés pour autant) ; `Settings` : lisibles mais appris autrement.
pub fn decode(
    bytes: &[u8],
    common: &Common,
    learn: &LearnParams,
) -> Result<(ProfileMeta, Taste), Incompatible> {
    let len = bytes.get(..4).ok_or(Incompatible::Format)?;
    let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
    let json = bytes.get(4..4usize.saturating_add(len)).ok_or(Incompatible::Format)?;
    let meta: ProfileMeta = serde_json::from_slice(json).map_err(|_| Incompatible::Format)?;
    if meta.format != FORMAT {
        return Err(Incompatible::Format);
    }
    let tables = &bytes[4 + len..];
    if tables.len() != TABLES_LEN {
        return Err(Incompatible::Format);
    }
    let mut taste = Taste::neutral();
    for (i, c) in tables.as_chunks::<2>().0.iter().enumerate() {
        let q = u16::from_le_bytes([c[0], c[1]]);
        if q == 0 {
            return Err(Incompatible::Format);
        }
        taste.q8[i / TASTE_CLASSES][i % TASTE_CLASSES] = q;
    }
    let empty_set = meta.loops == [0, 0, 0];
    if meta.common != hex16(common.hash())
        || meta.learn != hex16(learn.hash())
        || meta.normalized != learn.normalize
        || (meta.steps == 0 && !empty_set)
    {
        return Err(Incompatible::Settings);
    }
    Ok((meta, taste))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Common, LearnParams, ProfileMeta, Taste) {
        let common = Common::engine(false);
        let learn = LearnParams::default();
        let meta = ProfileMeta {
            format: FORMAT,
            learned_at: 1_700_000_000,
            overlay_deposited_at: 1_699_999_000,
            common: hex16(common.hash()),
            learn: hex16(learn.hash()),
            engine: "1.10.0".into(),
            steps: 3,
            loops: [10, 5, 0],
            pairs: [20, 10, 0],
            normalized: learn.normalize,
            complete_at: None,
        };
        let mut taste = Taste::neutral();
        taste.q8[0][3] = 400;
        taste.q8[2][71] = 65;
        (common, learn, meta, taste)
    }

    #[test]
    fn a_profile_round_trips_in_the_clear() {
        let (common, learn, meta, taste) = fixture();
        let bytes = encode(&meta, &taste);
        let (m2, t2) = decode(&bytes, &common, &learn).unwrap();
        assert_eq!((m2, t2), (meta, taste));
    }

    #[test]
    fn completeness_and_the_date_of_the_last_complete_profile() {
        let (_, learn, meta, _) = fixture();
        assert!(meta.is_complete(learn.steps));
        let partial = ProfileMeta { steps: 2, ..meta.clone() };
        assert!(!partial.is_complete(learn.steps));
        // Aucune boucle : rien à rejouer, complet.
        assert!(ProfileMeta { steps: 0, loops: [0; 3], ..meta.clone() }.is_complete(learn.steps));
        // Sans date écrite : celle de l'apprentissage ; écrite : elle prime ; et elle survit à l'aller-retour.
        assert_eq!(partial.complete_since(), meta.learned_at);
        let carried = ProfileMeta { complete_at: Some(1_600_000_000), ..partial };
        assert_eq!(carried.complete_since(), 1_600_000_000);
        let (common, learn, _, taste) = fixture();
        let (back, _) = decode(&encode(&carried, &taste), &common, &learn).unwrap();
        assert_eq!(back, carried);
    }

    #[test]
    fn unknown_format_truncated_tables_and_a_zero_q8_are_format_errors() {
        let (common, learn, meta, taste) = fixture();
        let bytes = encode(&meta, &taste);
        let v2 = encode(&ProfileMeta { format: 2, ..meta.clone() }, &taste);
        assert_eq!(decode(&v2, &common, &learn), Err(Incompatible::Format));
        assert_eq!(decode(&bytes[..bytes.len() - 2], &common, &learn), Err(Incompatible::Format));
        assert_eq!(decode(&bytes[..3], &common, &learn), Err(Incompatible::Format));
        let mut long = bytes.clone();
        long.push(0);
        assert_eq!(decode(&long, &common, &learn), Err(Incompatible::Format));
        let mut zero = taste.clone();
        zero.q8[1][0] = 0;
        assert_eq!(decode(&encode(&meta, &zero), &common, &learn), Err(Incompatible::Format));
        assert_eq!(decode(b"\x05\0\0\0{nope", &common, &learn), Err(Incompatible::Format));
        // La longueur annoncée déborde.
        assert_eq!(decode(&[255, 255, 255, 255, 1], &common, &learn), Err(Incompatible::Format));
    }

    #[test]
    fn another_common_table_or_other_settings_are_settings_errors() {
        let (common, learn, meta, taste) = fixture();
        let bytes = encode(&meta, &taste);
        let other = LearnParams { n0: 26.0, ..learn.clone() };
        assert_eq!(decode(&bytes, &common, &other), Err(Incompatible::Settings));
        let capped = LearnParams { cap: super::super::collect::Cap::PerType(500), ..learn.clone() };
        assert_eq!(decode(&bytes, &common, &capped), Err(Incompatible::Settings));
        let wrong = encode(&ProfileMeta { common: hex16(common.hash() ^ 1), ..meta.clone() }, &taste);
        assert_eq!(decode(&wrong, &common, &learn), Err(Incompatible::Settings));
        // Zéro pas avec des boucles : jamais écrit, refusé quand même.
        let zero = encode(&ProfileMeta { steps: 0, ..meta.clone() }, &taste);
        assert_eq!(decode(&zero, &common, &learn), Err(Incompatible::Settings));
        let empty = encode(&ProfileMeta { steps: 0, loops: [0; 3], ..meta }, &taste);
        assert!(decode(&empty, &common, &learn).is_ok());
    }
}
