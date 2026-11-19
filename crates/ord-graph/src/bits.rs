//! Bitset de taille fixe, avec un répertoire de rangs optionnel : l'état par nœud ou par arête des vues hors mémoire
//! et du contrôle des vues (un bit par élément, 69 Mo pour les 553 M d'arêtes de la planète).

/// Bits par superbloc du répertoire de rangs.
const SUPER: usize = 512;

#[derive(Clone, Debug, Default)]
pub struct Bits {
    words: Vec<u64>,
    len: usize,
    /// Bits à 1 avant chaque superbloc ; vide tant que [`Bits::build_rank`] n'a pas été appelé.
    ranks: Vec<u64>,
}

impl Bits {
    pub fn new(len: usize) -> Self {
        Self { words: vec![0; len.div_ceil(64)], len, ranks: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> bool {
        self.words[i / 64] >> (i % 64) & 1 != 0
    }

    /// Met le bit `i` à 1 ; le répertoire de rangs, s'il existe, n'est plus juste.
    pub fn set(&mut self, i: usize) {
        self.words[i / 64] |= 1 << (i % 64);
        self.ranks.clear();
    }

    /// Met le bit `i` à 1 et rend son ancienne valeur.
    pub fn replace(&mut self, i: usize) -> bool {
        let was = self.get(i);
        self.set(i);
        was
    }

    pub fn count_ones(&self) -> u64 {
        self.words.iter().map(|w| u64::from(w.count_ones())).sum()
    }

    /// Calcule le répertoire de rangs (8 octets par 512 bits).
    pub fn build_rank(&mut self) {
        let per = SUPER / 64;
        let mut acc = 0u64;
        self.ranks = self
            .words
            .chunks(per)
            .map(|c| {
                let r = acc;
                acc += c.iter().map(|w| u64::from(w.count_ones())).sum::<u64>();
                r
            })
            .collect();
    }

    /// Bits à 1 strictement avant `i` (exige [`Bits::build_rank`]).
    pub fn rank(&self, i: usize) -> u64 {
        assert!(!self.ranks.is_empty() || self.words.is_empty(), "répertoire de rangs absent");
        let w = i / 64;
        let sb = w / (SUPER / 64);
        let mut r = self.ranks.get(sb).copied().unwrap_or(0);
        r += self.words[sb * (SUPER / 64)..w].iter().map(|x| u64::from(x.count_ones())).sum::<u64>();
        if !i.is_multiple_of(64) {
            r += u64::from((self.words[w] & ((1u64 << (i % 64)) - 1)).count_ones());
        }
        r
    }

    /// Positions des bits à 1, croissantes.
    pub fn ones(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(wi, &w)| {
            let mut w = w;
            std::iter::from_fn(move || {
                if w == 0 {
                    return None;
                }
                let t = w.trailing_zeros() as usize;
                w &= w - 1;
                Some(wi * 64 + t)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_and_ones_agree_with_a_plain_count() {
        let n = 5_000;
        let mut b = Bits::new(n);
        let picked: Vec<usize> = (0..n).filter(|i| (i * 7919) % 13 < 4 || *i == n - 1).collect();
        for &i in &picked {
            b.set(i);
        }
        b.build_rank();
        assert_eq!(b.ones().collect::<Vec<_>>(), picked);
        assert_eq!(b.count_ones(), picked.len() as u64);
        for i in 0..n {
            assert_eq!(b.rank(i), picked.iter().filter(|&&p| p < i).count() as u64, "rang de {i}");
        }
        assert!(b.replace(picked[0]));
        assert!(!b.replace(1) || picked.contains(&1));
    }
}
