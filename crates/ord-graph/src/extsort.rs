//! Tri externe d'enregistrements de taille fixe (construction hors mémoire, note
//! `docs/design/technique/ord-graph-build-hors-memoire.md` §4).
//!
//! Les enregistrements s'accumulent dans un tampon borné ; tampon plein, il est trié (en parallèle) et écrit en
//! **série** dans le dossier temporaire. La sortie fusionne les séries par un tas, à plusieurs niveaux au-delà de
//! [`K_MAX`] séries ; si rien n'a débordé, elle sort du tampon sans toucher au disque.
//!
//! **Déterminisme** : la clé de tri doit être **totale** (deux enregistrements de même clé sont identiques). L'ordre
//! de sortie ne dépend alors ni du budget, qui change le découpage en séries, ni du nombre de fils.
//!
//! Lectures et écritures tamponnées, pas de `mmap` : le RSS ne compte que les tampons.
//!
//! Le tampon d'un tri grandit par doublement jusqu'à sa part de budget, jamais au-delà.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use rayon::slice::ParallelSliceMut;
use zerocopy::{FromBytes, Immutable, IntoBytes, Unaligned};

/// Nombre maximal de séries fusionnées en une fois.
pub const K_MAX: usize = 64;
/// Tampon minimal de lecture ou d'écriture d'un fichier temporaire.
const MIN_IO_BUF: usize = 16 << 10;
/// Taille d'un tampon d'écriture séquentielle.
pub const IO_BUF: usize = 1 << 20;

/// Un enregistrement de taille fixe : une structure `zerocopy` d'alignement 1 (champs petit-boutistes de
/// `zerocopy::little_endian`), dont les octets en mémoire sont les octets sur disque.
pub trait Record: FromBytes + IntoBytes + Immutable + Unaligned + Copy + Send + Sync {
    /// Octets sur disque.
    const SIZE: usize = size_of::<Self>();
}

impl<T: FromBytes + IntoBytes + Immutable + Unaligned + Copy + Send + Sync> Record for T {}

/// Préfixe des dossiers de construction et de leurs verrous.
const DIR_PREFIX: &str = ".ordg-build-";

/// Départage deux dossiers créés par le même processus.
static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

/// Dossier temporaire d'une construction : créé neuf sous un nom unique, jamais réutilisé, supprimé avec son
/// contenu à la fin (succès ou échec). Il compte les octets présents et leur pic.
///
/// Un fichier `<dossier>.lock`, à côté, reste verrouillé tant que la construction vit. Une construction tuée
/// (signal, mémoire) laisse son dossier ; la suivante le reconnaît à son verrou libre et le supprime.
#[derive(Debug)]
pub struct TmpDir {
    path: PathBuf,
    lock_path: PathBuf,
    /// Tenu ouvert : le verrou tombe avec le processus.
    _lock: File,
    stale_removed: Vec<PathBuf>,
    current: AtomicU64,
    peak: AtomicU64,
    seq: AtomicU64,
    runs: Mutex<Vec<(String, u64)>>,
}

impl TmpDir {
    /// Crée `parent/.ordg-build-<pid>-<unique>/`, après avoir supprimé les dossiers laissés là par des
    /// constructions mortes ([`TmpDir::stale_removed`]).
    pub fn create(parent: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(parent)?;
        let stale_removed = remove_stale(parent);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let name = format!(
            "{DIR_PREFIX}{}-{nanos:08x}-{}",
            std::process::id(),
            DIR_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let path = parent.join(&name);
        let lock_path = parent.join(format!("{name}.lock"));
        // Le verrou d'abord : un dossier visible a toujours son verrou déjà pris.
        let lock = std::fs::OpenOptions::new().write(true).create_new(true).open(&lock_path)?;
        if lock.try_lock().is_err() {
            // Système de fichiers sans verrou : sans fichier de verrou, personne ne prendra ce dossier pour un
            // reste.
            let _ = std::fs::remove_file(&lock_path);
        }
        if let Err(e) = std::fs::create_dir(&path) {
            let _ = std::fs::remove_file(&lock_path);
            return Err(e);
        }
        Ok(Self {
            path,
            lock_path,
            _lock: lock,
            stale_removed,
            current: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            seq: AtomicU64::new(0),
            runs: Mutex::new(Vec::new()),
        })
    }

    /// Dossiers de constructions mortes supprimés à la création de celui-ci.
    pub fn stale_removed(&self) -> &[PathBuf] {
        &self.stale_removed
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Chemin neuf pour un fichier nommé `name`.
    pub fn file(&self, name: &str) -> PathBuf {
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        self.path.join(format!("{n:06}-{name}"))
    }

    /// Compte `n` octets écrits.
    pub fn added(&self, n: u64) {
        let cur = self.current.fetch_add(n, Ordering::Relaxed) + n;
        self.peak.fetch_max(cur, Ordering::Relaxed);
    }

    /// Supprime un fichier temporaire et décompte sa taille.
    pub fn remove(&self, path: &Path) {
        if let Ok(m) = std::fs::metadata(path) {
            self.current.fetch_sub(m.len().min(self.current.load(Ordering::Relaxed)), Ordering::Relaxed);
        }
        let _ = std::fs::remove_file(path);
    }

    /// Pic d'octets présents à la fois.
    pub fn peak_bytes(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }

    /// Séries écrites par tri (nom, nombre), dans l'ordre de fin des tris.
    pub fn runs(&self) -> Vec<(String, u64)> {
        self.runs.lock().map(|r| r.clone()).unwrap_or_default()
    }

    fn record_runs(&self, name: &str, n: u64) {
        if let Ok(mut r) = self.runs.lock() {
            r.push((name.to_string(), n));
        }
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// Supprime de `parent` les dossiers de construction dont le verrou est libre : leur processus est mort. Un
/// dossier sans fichier de verrou (autre origine) ou au verrou tenu (construction en cours, même d'un autre
/// processus) n'est pas touché. Rend les dossiers supprimés.
fn remove_stale(parent: &Path) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(parent) else { return removed };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(DIR_PREFIX) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let lock_path = parent.join(format!("{name}.lock"));
        let Ok(lock) = File::open(&lock_path) else { continue };
        if lock.try_lock().is_err() {
            continue;
        }
        let dir = entry.path();
        if std::fs::remove_dir_all(&dir).is_ok() {
            let _ = std::fs::remove_file(&lock_path);
            removed.push(dir);
        }
    }
    removed
}

/// Écrit des enregistrements à la suite dans un fichier temporaire.
#[derive(Debug)]
pub struct RecWriter<'t, R: Record> {
    tmp: &'t TmpDir,
    path: PathBuf,
    w: BufWriter<File>,
    len: u64,
    _r: std::marker::PhantomData<R>,
}

impl<'t, R: Record> RecWriter<'t, R> {
    pub fn create(tmp: &'t TmpDir, name: &str, buf_bytes: usize) -> std::io::Result<Self> {
        let path = tmp.file(name);
        let file = File::create(&path)?;
        Ok(Self {
            tmp,
            path,
            w: BufWriter::with_capacity(buf_bytes.max(MIN_IO_BUF), file),
            len: 0,
            _r: std::marker::PhantomData,
        })
    }

    pub fn push(&mut self, r: &R) -> std::io::Result<()> {
        self.push_all(std::slice::from_ref(r))
    }

    /// Écrit une suite d'enregistrements d'un coup.
    pub fn push_all(&mut self, rs: &[R]) -> std::io::Result<()> {
        let bytes = rs.as_bytes();
        self.w.write_all(bytes)?;
        self.len += rs.len() as u64;
        self.tmp.added(bytes.len() as u64);
        Ok(())
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Ferme le fichier ; rend son chemin et son nombre d'enregistrements.
    pub fn finish(mut self) -> std::io::Result<(PathBuf, u64)> {
        self.w.flush()?;
        Ok((self.path.clone(), self.len))
    }
}

/// Lit en flux un fichier d'enregistrements ; le fichier est supprimé quand le lecteur est lâché si
/// `delete` est vrai.
#[derive(Debug)]
pub struct RecReader<'t, R: Record> {
    tmp: &'t TmpDir,
    path: PathBuf,
    r: BufReader<File>,
    left: u64,
    delete: bool,
    peeked: Option<R>,
}

impl<'t, R: Record> RecReader<'t, R> {
    pub fn open(
        tmp: &'t TmpDir,
        path: &Path,
        count: u64,
        buf_bytes: usize,
        delete: bool,
    ) -> std::io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            tmp,
            path: path.to_path_buf(),
            r: BufReader::with_capacity(buf_bytes.max(MIN_IO_BUF), file),
            left: count,
            delete,
            peeked: None,
        })
    }

    pub fn pull(&mut self) -> std::io::Result<Option<R>> {
        if let Some(r) = self.peeked.take() {
            return Ok(Some(r));
        }
        if self.left == 0 {
            return Ok(None);
        }
        let mut rec = R::new_zeroed();
        self.r.read_exact(rec.as_mut_bytes())?;
        self.left -= 1;
        Ok(Some(rec))
    }

    pub fn peek(&mut self) -> std::io::Result<Option<&R>> {
        if self.peeked.is_none() {
            self.peeked = self.pull()?;
        }
        Ok(self.peeked.as_ref())
    }
}

impl<R: Record> Drop for RecReader<'_, R> {
    fn drop(&mut self) {
        if self.delete {
            self.tmp.remove(&self.path);
        }
    }
}

/// Tri externe : `push` puis `finish`, qui rend la sortie triée en flux.
pub struct ExtSort<'t, R: Record, K: Ord + Send, F: Fn(&R) -> K + Sync> {
    tmp: &'t TmpDir,
    name: String,
    budget: usize,
    cap: usize,
    buf: Vec<R>,
    runs: Vec<(PathBuf, u64)>,
    spilled: u64,
    key: F,
}

impl<R: Record, K: Ord + Send, F: Fn(&R) -> K + Sync> std::fmt::Debug for ExtSort<'_, R, K, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtSort")
            .field("name", &self.name)
            .field("runs", &self.runs.len())
            .finish_non_exhaustive()
    }
}

impl<'t, R: Record, K: Ord + Send, F: Fn(&R) -> K + Sync> ExtSort<'t, R, K, F> {
    /// `budget` : octets du tampon de tri (au moins 16 enregistrements).
    pub fn new(tmp: &'t TmpDir, name: &str, budget: usize, key: F) -> Self {
        let cap = (budget / std::mem::size_of::<R>().max(1)).max(16);
        Self { tmp, name: name.to_string(), budget, cap, buf: Vec::new(), runs: Vec::new(), spilled: 0, key }
    }

    pub fn push(&mut self, r: R) -> std::io::Result<()> {
        if self.buf.len() == self.buf.capacity() {
            // Croissance par doublement, mais jamais au-delà de `cap` : la capacité réservée reste dans la part
            // de budget du tri. (`len < cap` ici : le tampon plein vient d'être vidé.)
            let target = (self.buf.capacity() * 2).max(1 << 16).min(self.cap);
            self.buf.reserve_exact(target - self.buf.len());
        }
        self.buf.push(r);
        if self.buf.len() >= self.cap {
            self.spill()?;
        }
        Ok(())
    }

    fn spill(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let key = &self.key;
        self.buf.par_sort_unstable_by(|a, b| key(a).cmp(&key(b)));
        let mut w = RecWriter::<R>::create(self.tmp, &format!("{}.run", self.name), IO_BUF)?;
        w.push_all(&self.buf)?;
        self.runs.push(w.finish()?);
        self.spilled += 1;
        self.buf.clear();
        Ok(())
    }

    /// Fin des entrées : rend la sortie triée.
    pub fn finish(mut self) -> std::io::Result<Sorted<'t, R, K, F>> {
        if self.runs.is_empty() {
            let key = &self.key;
            self.buf.par_sort_unstable_by(|a, b| key(a).cmp(&key(b)));
            self.tmp.record_runs(&self.name, 0);
            let buf = std::mem::take(&mut self.buf);
            return Ok(Sorted { inner: Inner::Mem(buf.into_iter()), key: self.key });
        }
        self.spill()?;
        self.buf = Vec::new();
        // Fusions intermédiaires tant qu'il reste plus de K_MAX séries.
        while self.runs.len() > K_MAX {
            let group: Vec<_> = self.runs.drain(..K_MAX).collect();
            let mut merge = Merge::open(self.tmp, &group, self.budget, &self.key)?;
            let mut w = RecWriter::<R>::create(self.tmp, &format!("{}.run", self.name), IO_BUF)?;
            while let Some(r) = merge.next(&self.key)? {
                w.push(&r)?;
            }
            drop(merge);
            self.runs.push(w.finish()?);
            self.spilled += 1;
        }
        self.tmp.record_runs(&self.name, self.spilled);
        let merge = Merge::open(self.tmp, &self.runs, self.budget, &self.key)?;
        Ok(Sorted { inner: Inner::Merge(merge), key: self.key })
    }
}

struct Merge<'t, R: Record, K: Ord> {
    readers: Vec<RecReader<'t, R>>,
    heap: BinaryHeap<Reverse<(K, usize)>>,
    heads: Vec<Option<R>>,
}

impl<'t, R: Record, K: Ord> Merge<'t, R, K> {
    fn open<F: Fn(&R) -> K>(
        tmp: &'t TmpDir,
        runs: &[(PathBuf, u64)],
        budget: usize,
        key: &F,
    ) -> std::io::Result<Self> {
        let per = budget / (runs.len() + 1);
        let mut readers = Vec::with_capacity(runs.len());
        for (p, n) in runs {
            readers.push(RecReader::open(tmp, p, *n, per, true)?);
        }
        let mut heap = BinaryHeap::with_capacity(runs.len());
        let mut heads = Vec::with_capacity(runs.len());
        for (i, r) in readers.iter_mut().enumerate() {
            let h = r.pull()?;
            if let Some(rec) = &h {
                heap.push(Reverse((key(rec), i)));
            }
            heads.push(h);
        }
        Ok(Self { readers, heap, heads })
    }

    fn next<F: Fn(&R) -> K>(&mut self, key: &F) -> std::io::Result<Option<R>> {
        let Some(Reverse((_, i))) = self.heap.pop() else { return Ok(None) };
        let out = self.heads[i].take();
        let h = self.readers[i].pull()?;
        if let Some(rec) = &h {
            self.heap.push(Reverse((key(rec), i)));
        }
        self.heads[i] = h;
        Ok(out)
    }
}

enum Inner<'t, R: Record, K: Ord> {
    Mem(std::vec::IntoIter<R>),
    Merge(Merge<'t, R, K>),
}

/// Sortie triée d'un [`ExtSort`], en flux.
pub struct Sorted<'t, R: Record, K: Ord, F: Fn(&R) -> K> {
    inner: Inner<'t, R, K>,
    key: F,
}

impl<R: Record, K: Ord, F: Fn(&R) -> K> std::fmt::Debug for Sorted<'_, R, K, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sorted").finish_non_exhaustive()
    }
}

impl<R: Record, K: Ord, F: Fn(&R) -> K> Sorted<'_, R, K, F> {
    pub fn pull(&mut self) -> std::io::Result<Option<R>> {
        match &mut self.inner {
            Inner::Mem(it) => Ok(it.next()),
            Inner::Merge(m) => m.next(&self.key),
        }
    }
}

/// Une sortie triée qu'on peut regarder avant de la consommer.
#[derive(Debug)]
pub struct Peekable<S, R> {
    s: S,
    peeked: Option<R>,
}

/// Source d'enregistrements en flux.
pub trait Stream<R> {
    fn next_rec(&mut self) -> std::io::Result<Option<R>>;
}

impl<R: Record, K: Ord, F: Fn(&R) -> K> Stream<R> for Sorted<'_, R, K, F> {
    fn next_rec(&mut self) -> std::io::Result<Option<R>> {
        self.pull()
    }
}

impl<R: Record> Stream<R> for RecReader<'_, R> {
    fn next_rec(&mut self) -> std::io::Result<Option<R>> {
        self.pull()
    }
}

impl<S: Stream<R>, R: Copy> Peekable<S, R> {
    pub fn new(s: S) -> Self {
        Self { s, peeked: None }
    }
    pub fn pull(&mut self) -> std::io::Result<Option<R>> {
        match self.peeked.take() {
            Some(r) => Ok(Some(r)),
            None => self.s.next_rec(),
        }
    }
    pub fn peek(&mut self) -> std::io::Result<Option<R>> {
        if self.peeked.is_none() {
            self.peeked = self.s.next_rec()?;
        }
        Ok(self.peeked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use zerocopy::little_endian::{U32, U64};
    use zerocopy::KnownLayout;

    #[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned, Clone, Copy, Debug)]
    #[repr(C)]
    struct Rec(U64, U32);

    type P = (u64, u32);

    fn sorted(input: &[P], budget: usize) -> (Vec<P>, u64) {
        let parent = tempfile::tempdir().unwrap();
        let tmp = TmpDir::create(parent.path()).unwrap();
        let mut s = ExtSort::new(&tmp, "p", budget, |r: &Rec| (r.0.get(), r.1.get()));
        for p in input {
            s.push(Rec(p.0.into(), p.1.into())).unwrap();
        }
        let mut out = s.finish().unwrap();
        let mut v = Vec::new();
        while let Some(r) = out.pull().unwrap() {
            v.push((r.0.get(), r.1.get()));
        }
        drop(out);
        let runs = tmp.runs()[0].1;
        // Tout est supprimé une fois la sortie consommée.
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
        (v, runs)
    }

    fn pseudo(n: usize) -> Vec<P> {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        (0..n)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x % 1000, i as u32 % 7)
            })
            .collect()
    }

    #[test]
    fn empty_input() {
        assert_eq!(sorted(&[], 1024), (vec![], 0));
    }

    #[test]
    fn single_run_stays_in_memory() {
        let input = pseudo(100);
        let (v, runs) = sorted(&input, 1 << 20);
        let mut want = input.clone();
        want.sort();
        assert_eq!(v, want);
        assert_eq!(runs, 0);
    }

    #[test]
    fn many_runs_and_multi_level_merge_with_equal_keys() {
        // 16 enregistrements par série : 10 000 / 16 = 625 séries, plus que K_MAX ; clés en double nombreuses.
        let input = pseudo(10_000);
        let (v, runs) = sorted(&input, 1);
        let mut want = input.clone();
        want.sort();
        assert_eq!(v, want);
        assert!(runs > K_MAX as u64 + 1, "{runs}");
        // Exactement K_MAX + 1 séries.
        let (v2, _) = sorted(&input[..16 * (K_MAX + 1)], 1);
        let mut want2 = input[..16 * (K_MAX + 1)].to_vec();
        want2.sort();
        assert_eq!(v2, want2);
    }

    #[test]
    fn buffer_capacity_stays_within_the_budget() {
        let parent = tempfile::tempdir().unwrap();
        let tmp = TmpDir::create(parent.path()).unwrap();
        // 100 000 enregistrements de 12 octets : la capacité double depuis 65 536, sans dépasser 100 000.
        let mut s = ExtSort::new(&tmp, "p", 1_200_000, |r: &Rec| (r.0.get(), r.1.get()));
        for i in 0..250_000u32 {
            s.push(Rec(u64::from(i).into(), i.into())).unwrap();
            assert!(s.buf.capacity() <= 100_000, "{}", s.buf.capacity());
        }
        assert_eq!(s.runs.len(), 2);
    }

    #[test]
    fn dead_build_dir_is_removed_live_one_is_kept() {
        let parent = tempfile::tempdir().unwrap();
        let live = TmpDir::create(parent.path()).unwrap();
        // Reste d'un processus mort : dossier et verrou, que personne ne tient.
        let dead = parent.path().join(format!("{DIR_PREFIX}dead"));
        std::fs::create_dir(&dead).unwrap();
        std::fs::write(dead.join("000000-run"), b"x").unwrap();
        std::fs::write(parent.path().join(format!("{DIR_PREFIX}dead.lock")), b"").unwrap();
        // Dossier du même préfixe sans verrou : pas à nous, laissé en place.
        let foreign = parent.path().join(format!("{DIR_PREFIX}foreign"));
        std::fs::create_dir(&foreign).unwrap();

        let second = TmpDir::create(parent.path()).unwrap();
        assert_eq!(second.stale_removed(), std::slice::from_ref(&dead));
        assert!(!dead.exists());
        assert!(!parent.path().join(format!("{DIR_PREFIX}dead.lock")).exists());
        assert!(foreign.exists());
        assert!(live.path().exists(), "une construction en cours garde son dossier");
        assert_ne!(live.path(), second.path());

        let (live_path, second_path) = (live.path().to_path_buf(), second.path().to_path_buf());
        drop(live);
        drop(second);
        assert!(!live_path.exists() && !second_path.exists());
        // Ne restent que le dossier étranger : ni dossier ni verrou des constructions finies.
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);
    }

    #[test]
    fn streaming_xxh3_equals_one_shot() {
        use xxhash_rust::xxh3::{xxh3_64, Xxh3};
        let data: Vec<u8> = (0..1_000_003u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        for chunk in [1usize, 7, 64, 240, 4096, 65_537] {
            let mut h = Xxh3::new();
            for c in data.chunks(chunk) {
                h.update(c);
            }
            assert_eq!(h.digest(), xxh3_64(&data), "morceaux de {chunk}");
        }
        assert_eq!(Xxh3::new().digest(), xxh3_64(&[]));
    }
}
