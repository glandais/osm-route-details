//! Empreintes SHA-256 : fichiers du manifest et cache de banc (`DataFingerprint`, T05 §4).

use std::path::Path;

use sha2::{Digest, Sha256};

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    Ok(sha256_bytes(&std::fs::read(path)?))
}

/// Empreinte du cache de banc : SHA-256 de la liste triée `(espace de noms, z/x/y, SHA-256 du
/// contenu)`, avec le nombre de tuiles et la taille totale.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CacheFingerprint {
    pub sha256: String,
    pub tiles: usize,
    pub bytes: u64,
}

pub fn cache_fingerprint(root: &Path) -> std::io::Result<CacheFingerprint> {
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut bytes = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for entry in read {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "tile") {
                let content = std::fs::read(&path)?;
                bytes += content.len() as u64;
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                entries.push((rel, sha256_bytes(&content)));
            }
        }
    }
    entries.sort();
    let mut h = Sha256::new();
    for (rel, sum) in &entries {
        h.update(rel.as_bytes());
        h.update(b"\0");
        h.update(sum.as_bytes());
        h.update(b"\n");
    }
    Ok(CacheFingerprint { sha256: hex(&h.finalize()), tiles: entries.len(), bytes })
}
