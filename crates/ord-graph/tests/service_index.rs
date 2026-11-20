//! Fichier d'index de service (`.ordi`, note `ord-loops-planete.md` §5) : aller-retour des tables, refus d'un fichier
//! d'un autre graphe ou d'autres vues, d'un fichier tronqué ou corrompu, de tables non triées.

use std::path::Path;

use ord_graph::build::{build_graph, BuildOptions, MemSource};
use ord_graph::format::{write_graph, GraphFile};
use ord_graph::service_index::{
    write_index, IndexData, IndexError, IndexFile, ViewTables, NODE_SAMPLE_EVERY,
};
use ord_graph::view::{build_view, ViewSpec};

const EPOCH: i64 = 1_700_000_000;

/// Une petite grille de rues et de chemins, sa base et ses vues `road` et `gravel`.
fn files(dir: &Path, epoch: i64, extra_way: bool) -> (GraphFile<Vec<u8>>, [GraphFile<Vec<u8>>; 2]) {
    let mut s = MemSource::default();
    let id = |i: i64, j: i64| 1 + i * 10 + j;
    for i in 0..6 {
        for j in 0..6 {
            s.node(id(i, j), 47.0 + i as f64 * 0.004, -1.5 + j as f64 * 0.006);
        }
    }
    let mut w = 0;
    for i in 0..6 {
        for j in 0..6 {
            for (di, dj) in [(0, 1), (1, 0)] {
                if i + di < 6 && j + dj < 6 {
                    w += 1;
                    let hw = if (i + j) % 3 == 0 { "track" } else { "residential" };
                    s.way(w, &[id(i, j), id(i + di, j + dj)], &[("highway", hw)]);
                }
            }
        }
    }
    if extra_way {
        s.way(999, &[id(0, 0), id(5, 5)], &[("highway", "tertiary")]);
    }
    let write = |name: &str, data: &mut ord_graph::GraphData| {
        let p = dir.join(name);
        write_graph(&p, data, "test", epoch).unwrap();
        GraphFile::open(std::fs::read(p).unwrap()).unwrap()
    };
    let base =
        write("b.ordg", &mut build_graph(&mut s, &BuildOptions { zoom: 14 }, "t", &mut |_| {}).unwrap());
    let view = |n: &str| {
        write(&format!("b.{n}.ordv"), &mut build_view(&base, &ViewSpec::parse(n).unwrap()).unwrap())
    };
    let views = [view("road"), view("gravel")];
    (base, views)
}

fn data(base: &GraphFile<Vec<u8>>, views: &[GraphFile<Vec<u8>>; 2]) -> IndexData {
    let mut sample = Vec::new();
    let mut bbox = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
    let mut seen = 0u32;
    for ci in 0..base.n_cells() {
        let cv = base.cell(ci);
        for l in 0..cv.info().n_nodes as usize {
            let (lat, lon) = cv.node(l);
            bbox = [bbox[0].min(lat), bbox[1].max(lat), bbox[2].min(lon), bbox[3].max(lon)];
            if seen.is_multiple_of(NODE_SAMPLE_EVERY) {
                sample.push((lat, lon));
            }
            seen += 1;
        }
    }
    let tables = |vi: usize, salt: u32| {
        let n = views[vi].header().edges as u32;
        ViewTables {
            foreign: (0..n)
                .step_by(3)
                .map(|c| ((c % 7 + salt) << 16 | 5, c))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            short: (0..n).step_by(2).map(|c| (c, c * 10 % 3_000)).collect(),
            lmax_dm: 1234 + salt,
        }
    };
    IndexData {
        views: [tables(0, 1), tables(1, 2)],
        cap_dm: 3_000,
        wide_seg: vec![0, 3, 7],
        wide_end: vec![3],
        node_bbox: Some(bbox),
        node_sample: sample,
    }
}

fn as_tuples(p: &[ord_graph::service_index::Pair]) -> Vec<(u32, u32)> {
    p.iter().map(|x| (x[0].get(), x[1].get())).collect()
}

#[test]
fn tables_come_back_as_written_and_the_file_belongs_to_its_graph() {
    let dir = tempfile::tempdir().unwrap();
    let (base, views) = files(dir.path(), EPOCH, false);
    let d = data(&base, &views);
    let path = dir.path().join("b.ordi");
    let len = write_index(&path, &base, [&views[0], &views[1]], &d, br#"{"tool":"test"}"#, 42).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), len);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o444);
    }
    let f = IndexFile::open(std::fs::read(&path).unwrap()).unwrap();
    for (vi, t) in d.views.iter().enumerate() {
        assert_eq!(as_tuples(f.foreign(vi)), t.foreign);
        assert_eq!(as_tuples(f.short(vi)), t.short);
        assert_eq!(f.lmax_dm(vi), t.lmax_dm);
    }
    assert_eq!(f.cap_dm(), 3_000);
    assert_eq!(f.wide_seg().iter().map(|e| e.get()).collect::<Vec<_>>(), d.wide_seg);
    assert_eq!(f.wide_end().iter().map(|e| e.get()).collect::<Vec<_>>(), d.wide_end);
    assert_eq!(f.node_bbox(), d.node_bbox);
    let sample: Vec<(i32, i32)> =
        f.node_sample().iter().map(|p| (p[0].get() as i32, p[1].get() as i32)).collect();
    assert_eq!(sample, d.node_sample);
    assert_eq!(f.meta(), br#"{"tool":"test"}"#);
    assert_eq!(f.header().built_epoch, 42);
    f.check_for(&base, [&views[0], &views[1]]).expect("son graphe");

    // Vues inversées, autre construction de la même base, autre graphe : refusés.
    assert!(matches!(f.check_for(&base, [&views[1], &views[0]]), Err(IndexError::Mismatch(_))));
    let other = tempfile::tempdir().unwrap();
    let (b2, v2) = files(other.path(), EPOCH + 1, false);
    assert!(matches!(f.check_for(&b2, [&v2[0], &v2[1]]), Err(IndexError::Mismatch(_))));
    assert!(matches!(f.check_for(&base, [&v2[0], &v2[1]]), Err(IndexError::Mismatch(_))));
    let (b3, v3) = files(other.path(), EPOCH, true);
    assert!(matches!(f.check_for(&b3, [&v3[0], &v3[1]]), Err(IndexError::Mismatch(_))));
}

#[test]
fn a_truncated_or_altered_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (base, views) = files(dir.path(), EPOCH, false);
    let path = dir.path().join("b.ordi");
    write_index(&path, &base, [&views[0], &views[1]], &data(&base, &views), b"{}", 0).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(matches!(IndexFile::open(bytes[..bytes.len() - 8].to_vec()), Err(IndexError::Truncated(_))));
    assert!(matches!(IndexFile::open(bytes[..100].to_vec()), Err(IndexError::Truncated(_))));
    let mut bad = bytes.clone();
    bad[0] = b'X';
    assert!(matches!(IndexFile::open(bad), Err(IndexError::BadMagic)));
    // Un octet de chaque section, et un de la table.
    let mut flipped = 0;
    for at in (256..bytes.len()).step_by(37) {
        let mut b = bytes.clone();
        b[at] ^= 0x40;
        if let Err(e) = IndexFile::open(b) {
            assert!(matches!(e, IndexError::Corrupt(_) | IndexError::Truncated(_)), "{e}");
            flipped += 1;
        }
    }
    // Tous refusés, sauf les octets de bourrage entre sections (hors empreinte, sans effet).
    assert!(flipped * 10 >= (bytes.len() - 256) / 37 * 9, "{flipped}");
}

#[test]
fn unsorted_tables_or_long_short_components_are_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let (base, views) = files(dir.path(), EPOCH, false);
    let path = dir.path().join("b.ordi");
    let mut d = data(&base, &views);
    d.views[0].foreign.swap(0, 1);
    assert!(matches!(
        write_index(&path, &base, [&views[0], &views[1]], &d, b"{}", 0),
        Err(IndexError::Corrupt(_))
    ));
    let mut d = data(&base, &views);
    d.views[1].short.push((u32::MAX, 3_000));
    assert!(matches!(
        write_index(&path, &base, [&views[0], &views[1]], &d, b"{}", 0),
        Err(IndexError::Corrupt(_))
    ));
    let mut d = data(&base, &views);
    d.wide_end = vec![4, 4];
    assert!(matches!(
        write_index(&path, &base, [&views[0], &views[1]], &d, b"{}", 0),
        Err(IndexError::Corrupt(_))
    ));
    // Les vues dans le mauvais ordre : refusé avant d'écrire.
    let d = data(&base, &views);
    assert!(matches!(
        write_index(&path, &base, [&views[1], &views[0]], &d, b"{}", 0),
        Err(IndexError::Mismatch(_))
    ));
    assert!(!path.exists());
}
