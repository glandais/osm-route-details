//! Source OSM : fichier `.osm.pbf` trié `Type_then_ID`, lu par `osmpbf` en lecture (pas de `mmap`, qui
//! gonflerait le RSS). Les blobs sont décodés en parallèle par lots puis traités dans l'ordre du fichier :
//! mêmes octets de sortie quel que soit le nombre de fils.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use osmpbf::{Blob, BlobReader, BlobType, PrimitiveBlock, RelMemberType};
use rayon::prelude::*;
use xxhash_rust::xxh3::Xxh3;

use crate::build::{
    BuildError, ElementSink, NodeBatchFn, OsmRelation, OsmSource, OsmWay, SourceMeta, TaggedNodeFn,
};

/// Nœuds d'un blob, dans l'ordre, et « le blob contient déjà des ways ou des relations ».
type DecodedNodes = (Vec<(i64, i32, i32)>, bool);

#[derive(Debug)]
pub struct PbfSource {
    path: PathBuf,
    meta: SourceMeta,
    /// Blobs décodés par lot ; `None` : 4 par fil de rayon.
    batch_len: Option<usize>,
    /// Plafond d'un lot en octets du fichier (blobs compressés) ; `None` : pas de plafond. Un lot décodé pèse
    /// ≈ 14 × ses octets compressés, et les blobs vont de ≈ 65 Ko (Geofabrik) à ≈ 1,8 Mo (`planet-dump-ng`) :
    /// sans plafond, 64 blobs de la planète décodés tiennent ≈ 1,5 Gio hors de tout budget.
    batch_bytes: Option<u64>,
}

fn err(e: impl std::fmt::Display) -> BuildError {
    BuildError::Source(e.to_string())
}

impl PbfSource {
    /// Ouvre l'extrait, lit son en-tête (tri exigé) et calcule sa taille et son xxh3.
    pub fn open(path: &Path) -> Result<Self, BuildError> {
        let mut reader = BlobReader::from_path(path).map_err(err)?;
        let first = reader.next().ok_or_else(|| err("fichier vide"))?.map_err(err)?;
        let header = first.to_headerblock().map_err(err)?;
        if !header
            .optional_features()
            .iter()
            .chain(header.required_features())
            .any(|f| f == "Sort.Type_then_ID")
        {
            return Err(BuildError::NotSorted("l'en-tête ne déclare pas Sort.Type_then_ID".into()));
        }
        let mut meta = SourceMeta {
            replication_timestamp: header.osmosis_replication_timestamp().unwrap_or(0),
            replication_sequence: header.osmosis_replication_sequence_number().unwrap_or(0),
            ..Default::default()
        };
        let mut hasher = Xxh3::new();
        let mut file = std::fs::File::open(path).map_err(err)?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buf).map_err(err)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            meta.size += n as u64;
        }
        meta.xxh3 = hasher.digest();
        Ok(Self { path: path.to_path_buf(), meta, batch_len: None, batch_bytes: None })
    }

    /// Fixe la taille des lots de blobs (au moins 1) : les tests la réduisent pour que même un PBF de trois
    /// blobs traverse plusieurs frontières de lot, où un réordonnancement casserait le déterminisme.
    #[must_use]
    pub fn with_batch_len(mut self, n: usize) -> Self {
        self.batch_len = Some(n.max(1));
        self
    }

    /// Plafonne un lot à `n` octets du fichier (au moins un blob par lot), en plus du nombre de blobs : la
    /// construction hors mémoire y met une part de son budget.
    #[must_use]
    pub fn with_batch_bytes(mut self, n: u64) -> Self {
        self.batch_bytes = Some(n.max(1));
        self
    }

    /// Lots de blobs de données, dans l'ordre ; `f` rend `false` pour arrêter.
    fn batches(&self, mut f: impl FnMut(Vec<Blob>) -> Result<bool, BuildError>) -> Result<(), BuildError> {
        let batch_len = self.batch_len.unwrap_or_else(|| rayon::current_num_threads() * 4);
        let reader = BlobReader::from_path(&self.path).map_err(err)?;
        let mut batch = Vec::with_capacity(batch_len);
        // Position dans le fichier du premier blob du lot (`from_path` donne la position de chaque blob).
        let mut start = 0u64;
        for blob in reader {
            let blob = blob.map_err(err)?;
            if !matches!(blob.get_type(), BlobType::OsmData) {
                continue;
            }
            let at = blob.offset().map_or(0, |o| o.0);
            // Les blobs déjà dans le lot couvrent `at - start` octets : au plafond, le lot part sans ce blob.
            if let Some(cap) = self.batch_bytes {
                if !batch.is_empty()
                    && at.saturating_sub(start) >= cap
                    && !f(std::mem::replace(&mut batch, Vec::with_capacity(batch_len)))?
                {
                    return Ok(());
                }
            }
            if batch.is_empty() {
                start = at;
            }
            batch.push(blob);
            if batch.len() == batch_len && !f(std::mem::replace(&mut batch, Vec::with_capacity(batch_len)))? {
                return Ok(());
            }
        }
        if !batch.is_empty() {
            f(batch)?;
        }
        Ok(())
    }
}

fn decode(batch: &[Blob]) -> Result<Vec<PrimitiveBlock>, BuildError> {
    batch.par_iter().map(|b| b.to_primitiveblock().map_err(err)).collect()
}

impl OsmSource for PbfSource {
    fn meta(&self) -> SourceMeta {
        self.meta
    }

    fn scan(&mut self, sink: &mut dyn ElementSink) -> Result<(), BuildError> {
        let mut refs: Vec<i64> = Vec::new();
        let mut members: Vec<i64> = Vec::new();
        let mut saw_relation = false;
        self.batches(|batch| {
            for block in decode(&batch)? {
                for group in block.groups() {
                    for way in group.ways() {
                        if saw_relation {
                            return Err(BuildError::NotSorted("way après une relation".into()));
                        }
                        refs.clear();
                        refs.extend(way.refs());
                        let tags: Vec<(&str, &str)> = way.tags().collect();
                        sink.way(&OsmWay { id: way.id(), refs: &refs, tags: &tags })?;
                    }
                    for rel in group.relations() {
                        saw_relation = true;
                        members.clear();
                        members.extend(
                            rel.members()
                                .filter(|m| m.member_type == RelMemberType::Way)
                                .map(|m| m.member_id),
                        );
                        let tags: Vec<(&str, &str)> = rel.tags().collect();
                        sink.relation(&OsmRelation { id: rel.id(), tags: &tags, way_members: &members })?;
                    }
                }
            }
            Ok(true)
        })
    }

    fn nodes(&mut self, f: &(dyn Fn(i64, i32, i32) + Sync)) -> Result<(), BuildError> {
        let past_nodes = AtomicBool::new(false);
        self.batches(|batch| {
            batch.par_iter().try_for_each(|blob| -> Result<(), BuildError> {
                let block = blob.to_primitiveblock().map_err(err)?;
                for group in block.groups() {
                    for n in group.dense_nodes() {
                        f(n.id(), n.decimicro_lat(), n.decimicro_lon());
                    }
                    for n in group.nodes() {
                        f(n.id(), n.decimicro_lat(), n.decimicro_lon());
                    }
                    if group.ways().next().is_some() || group.relations().next().is_some() {
                        past_nodes.store(true, Ordering::Relaxed);
                    }
                }
                Ok(())
            })?;
            // Trié par type : une fois les ways atteintes, il ne reste plus de nœud.
            Ok(!past_nodes.load(Ordering::Relaxed))
        })
    }
    fn tagged_nodes(&mut self, f: &TaggedNodeFn<'_>) -> Result<(), BuildError> {
        let past_nodes = AtomicBool::new(false);
        self.batches(|batch| {
            batch.par_iter().try_for_each(|blob| -> Result<(), BuildError> {
                let block = blob.to_primitiveblock().map_err(err)?;
                let mut tags: Vec<(&str, &str)> = Vec::new();
                for group in block.groups() {
                    for n in group.dense_nodes() {
                        tags.clear();
                        tags.extend(n.tags());
                        f(n.id(), n.decimicro_lat(), n.decimicro_lon(), &tags);
                    }
                    for n in group.nodes() {
                        tags.clear();
                        tags.extend(n.tags());
                        f(n.id(), n.decimicro_lat(), n.decimicro_lon(), &tags);
                    }
                    if group.ways().next().is_some() || group.relations().next().is_some() {
                        past_nodes.store(true, Ordering::Relaxed);
                    }
                }
                Ok(())
            })?;
            Ok(!past_nodes.load(Ordering::Relaxed))
        })
    }

    fn nodes_sorted(&mut self, f: &mut NodeBatchFn<'_>) -> Result<(), BuildError> {
        self.batches(|batch| {
            // Décodage en parallèle, livraison dans l'ordre du fichier : un lot par blob.
            let decoded: Vec<DecodedNodes> = batch
                .par_iter()
                .map(|blob| -> Result<_, BuildError> {
                    let block = blob.to_primitiveblock().map_err(err)?;
                    let mut nodes = Vec::new();
                    let mut past = false;
                    for group in block.groups() {
                        nodes.extend(
                            group.dense_nodes().map(|n| (n.id(), n.decimicro_lat(), n.decimicro_lon())),
                        );
                        nodes.extend(group.nodes().map(|n| (n.id(), n.decimicro_lat(), n.decimicro_lon())));
                        past |= group.ways().next().is_some() || group.relations().next().is_some();
                    }
                    Ok((nodes, past))
                })
                .collect::<Result<_, _>>()?;
            let mut past_nodes = false;
            for (nodes, past) in &decoded {
                if !nodes.is_empty() {
                    f(nodes)?;
                }
                past_nodes |= *past;
            }
            // Trié par type : une fois les ways atteintes, il ne reste plus de nœud.
            Ok(!past_nodes)
        })
    }
}
