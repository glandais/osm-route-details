//! Cloisonnement des environnements (note lot 3a §2) : tout se dérive du secret K de l'appelant, le service n'en garde
//! aucun. Le nom du dossier, celui du fichier et la clé qui chiffre le fichier sortent de K par HMAC-SHA256 à séparation
//! de domaine ; un appelant ne peut donc **nommer** que ses propres fichiers.
//!
//! Rien d'ici n'est journalisé : ni K, ni la référence d'équipe, ni les noms dérivés (hors `ns8`, préfixe non secret).

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

/// Longueur du secret, en octets.
pub const KEY_LEN: usize = 32;
/// Longueur du secret en base64url sans remplissage.
pub const KEY_B64_LEN: usize = 43;
/// Longueur maximale d'une référence d'équipe.
pub const TEAM_MAX_LEN: usize = 64;

/// Magie et version du fichier chiffré.
pub const MAGIC: &[u8; 8] = b"ORDOENC1";
/// Magie du fichier de profil de goût (`.ordp.enc`, 3b) : même clé, même enveloppe, autre domaine. La magie entre dans les
/// données associées : un profil ne s'ouvre pas comme surcouche, ni l'inverse.
pub const MAGIC_PROFILE: &[u8; 8] = b"ORDPENC1";
const VERSION: u8 = 1;
const TAG_LEN: usize = 16;

/// Ce que l'on dérive de K et de la référence d'équipe.
pub struct Credentials {
    /// Nom de dossier (32 hex).
    pub ns: String,
    /// Nom de fichier, sans extension (64 hex).
    pub team: String,
    enc: LessSafeKey,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("ns8", &self.ns8()).finish_non_exhaustive()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn mac(key: &hmac::Key, label: &[u8], extra: &[u8]) -> hmac::Tag {
    let mut ctx = hmac::Context::with_key(key);
    ctx.update(label);
    ctx.update(extra);
    ctx.sign()
}

impl Credentials {
    /// `ns = hex(HMAC(K, "ord/ns/v1")[..16])`, `team = hex(HMAC(K, "ord/team/v1\0" ‖ team))`,
    /// `enc = HMAC(K, "ord/enc/v1")` (clé ChaCha20-Poly1305).
    pub fn derive(k: &[u8; KEY_LEN], team_ref: &str) -> Self {
        let key = hmac::Key::new(hmac::HMAC_SHA256, k);
        let ns = hex(&mac(&key, b"ord/ns/v1", b"").as_ref()[..16]);
        let team = hex(mac(&key, b"ord/team/v1\0", team_ref.as_bytes()).as_ref());
        let enc_key = mac(&key, b"ord/enc/v1", b"");
        let enc = LessSafeKey::new(
            UnboundKey::new(&CHACHA20_POLY1305, enc_key.as_ref()).expect("clé de 32 octets"),
        );
        Self { ns, team, enc }
    }

    /// Dossier d'espace seul (suppression de tout l'espace) : la référence d'équipe n'y est pas utilisée.
    pub fn derive_namespace(k: &[u8; KEY_LEN]) -> Self {
        Self::derive(k, "")
    }

    /// Les 8 premiers caractères du nom d'espace : l'étiquette des journaux (non secrète).
    pub fn ns8(&self) -> &str {
        &self.ns[..8]
    }

    fn aad(&self, magic: &[u8; 8], graph_id: &str) -> Vec<u8> {
        let mut aad = Vec::with_capacity(8 + graph_id.len() + self.ns.len() + self.team.len() + 3);
        aad.extend_from_slice(magic);
        for part in [graph_id, &self.ns, &self.team] {
            aad.push(0);
            aad.extend_from_slice(part.as_bytes());
        }
        aad
    }

    /// `magie ‖ version ‖ nonce ‖ chiffré+étiquette`. Les données associées lient le fichier à son graphe, à son
    /// espace et à son équipe : un fichier déplacé ne se déchiffre pas.
    pub fn seal(&self, graph_id: &str, plain: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.seal_with(MAGIC, graph_id, plain)
    }

    /// Comme [`Credentials::seal`], sous la magie donnée (`MAGIC` : une surcouche, `MAGIC_PROFILE` : un profil).
    pub fn seal_with(&self, magic: &[u8; 8], graph_id: &str, plain: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce).map_err(|_| CryptoError::Random)?;
        let mut body = plain.to_vec();
        self.enc
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(self.aad(magic, graph_id)),
                &mut body,
            )
            .map_err(|_| CryptoError::Seal)?;
        let mut out = Vec::with_capacity(magic.len() + 1 + NONCE_LEN + body.len());
        out.extend_from_slice(magic);
        out.push(VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&body);
        Ok(out)
    }

    /// Déchiffre ; toute altération, tout autre graphe, espace, équipe ou clé donne [`CryptoError::Open`].
    pub fn open(&self, graph_id: &str, file: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.open_with(MAGIC, graph_id, file)
    }

    /// Comme [`Credentials::open`], sous la magie donnée.
    pub fn open_with(&self, magic: &[u8; 8], graph_id: &str, file: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let head = magic.len() + 1 + NONCE_LEN;
        if file.len() < head + TAG_LEN || &file[..magic.len()] != magic || file[magic.len()] != VERSION {
            return Err(CryptoError::Open);
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&file[magic.len() + 1..head]);
        let mut body = file[head..].to_vec();
        let plain = self
            .enc
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(self.aad(magic, graph_id)),
                &mut body,
            )
            .map_err(|_| CryptoError::Open)?;
        let n = plain.len();
        body.truncate(n);
        Ok(body)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("random source unavailable")]
    Random,
    #[error("encryption failed")]
    Seal,
    #[error("file does not decrypt")]
    Open,
}

/// Décode le secret : 43 caractères base64url sans remplissage (`A-Za-z0-9_-`), forme canonique, soit 32 octets.
pub fn decode_key(text: &str) -> Option<[u8; KEY_LEN]> {
    let b = text.as_bytes();
    if b.len() != KEY_B64_LEN {
        return None;
    }
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    };
    let mut out = [0u8; KEY_LEN];
    let (mut acc, mut bits, mut n) = (0u32, 0u32, 0usize);
    for &c in b {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out[n] = (acc >> bits) as u8;
            n += 1;
            acc &= (1 << bits) - 1;
        }
    }
    // 43 × 6 = 258 bits : 256 utiles, 2 de remplissage qui doivent être nuls (une seule écriture par clé).
    (n == KEY_LEN && acc == 0).then_some(out)
}

/// Référence d'équipe : `[A-Za-z0-9_-]{1,64}`.
pub fn valid_team_ref(s: &str) -> bool {
    (1..=TEAM_MAX_LEN).contains(&s.len())
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// Encode en base64url sans remplissage (tests et outillage du backend).
pub fn encode_key(k: &[u8; KEY_LEN]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(KEY_B64_LEN);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in k {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(char::from(ALPHABET[((acc >> bits) & 63) as usize]));
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((acc << (6 - bits)) & 63) as usize]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> [u8; KEY_LEN] {
        let mut k = [0u8; KEY_LEN];
        for (i, b) in k.iter_mut().enumerate() {
            *b = n.wrapping_mul(31).wrapping_add(i as u8);
        }
        k
    }

    #[test]
    fn base64url_round_trip_and_strictness() {
        for n in 0..8 {
            let k = key(n);
            let t = encode_key(&k);
            assert_eq!(t.len(), KEY_B64_LEN);
            assert_eq!(decode_key(&t), Some(k));
        }
        let t = encode_key(&key(3));
        assert_eq!(decode_key(&t[..42]), None, "31 octets et demi");
        assert_eq!(decode_key(&format!("{t}A")), None);
        assert_eq!(decode_key(&t.replace(&t[..1], "+")), None, "hors alphabet");
        // Dernier caractère non canonique : les 2 bits de remplissage ne sont pas nuls.
        assert_eq!(decode_key(&format!("{}B", &t[..42])), None);
    }

    #[test]
    fn derivation_separates_keys_teams_and_roles() {
        let (a, b) = (key(1), key(2));
        let a1 = Credentials::derive(&a, "team-1");
        let a2 = Credentials::derive(&a, "team-2");
        let b1 = Credentials::derive(&b, "team-1");
        assert_eq!(a1.ns, a2.ns, "même environnement, même dossier");
        assert_ne!(a1.team, a2.team);
        assert_ne!(a1.ns, b1.ns, "autre clé, autre dossier");
        assert_ne!(a1.team, b1.team);
        assert_eq!((a1.ns.len(), a1.team.len()), (32, 64));
        assert!(a1.ns.bytes().chain(a1.team.bytes()).all(|c| c.is_ascii_hexdigit()));
        assert_eq!(Credentials::derive(&a, "team-1").team, a1.team, "déterministe");
    }

    #[test]
    fn seal_open_and_every_binding() {
        let c = Credentials::derive(&key(1), "t");
        let sealed = c.seal("g1", b"contenu").unwrap();
        assert!(!sealed.windows(7).any(|w| w == b"contenu"));
        assert_eq!(c.open("g1", &sealed).unwrap(), b"contenu");
        assert!(c.open("g2", &sealed).is_err(), "autre graphe");
        assert!(Credentials::derive(&key(1), "u").open("g1", &sealed).is_err(), "autre équipe");
        assert!(Credentials::derive(&key(2), "t").open("g1", &sealed).is_err(), "autre clé");
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(c.open("g1", &tampered).is_err());
        assert!(c.open("g1", &sealed[..20]).is_err());
        let again = c.seal("g1", b"contenu").unwrap();
        assert_ne!(sealed, again, "nonce aléatoire à chaque écriture");
    }

    #[test]
    fn team_refs() {
        assert!(valid_team_ref("0abc_DEF-9"));
        for bad in ["", "../x", "a/b", "a.b", "a b", &"x".repeat(65), "é"] {
            assert!(!valid_team_ref(bad), "{bad}");
        }
    }

    #[test]
    fn a_profile_sealed_under_its_own_magic_does_not_open_as_an_overlay_and_the_other_way_round() {
        let c = Credentials::derive(&key(1), "team");
        let (plain, graph) = (b"tables".as_slice(), "g1");
        let profile = c.seal_with(MAGIC_PROFILE, graph, plain).unwrap();
        let overlay = c.seal(graph, plain).unwrap();
        assert_eq!(&profile[..8], MAGIC_PROFILE);
        assert_eq!(&overlay[..8], MAGIC, "l'enveloppe d'une surcouche est inchangée");
        assert_eq!(c.open_with(MAGIC_PROFILE, graph, &profile).unwrap(), plain);
        assert_eq!(c.open(graph, &overlay).unwrap(), plain);
        assert_eq!(c.open(graph, &profile), Err(CryptoError::Open));
        assert_eq!(c.open_with(MAGIC_PROFILE, graph, &overlay), Err(CryptoError::Open));
        // Même avec la magie réécrite à la main : la magie est dans les données associées.
        let mut forged = profile.clone();
        forged[..8].copy_from_slice(MAGIC);
        assert_eq!(c.open(graph, &forged), Err(CryptoError::Open));
        // Un profil ne se déplace pas : autre équipe, autre graphe.
        assert!(Credentials::derive(&key(1), "other").open_with(MAGIC_PROFILE, graph, &profile).is_err());
        assert!(c.open_with(MAGIC_PROFILE, "g2", &profile).is_err());
    }
}
