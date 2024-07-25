use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Parser;
use itertools::Itertools;
use thiserror::Error;

use path_tagging::{IoTagError, PathMetadata, RawTag, ResolvedTags};

fn main() {
    let args = Arguments::parse();
    args.subcommand.execute();
}

#[derive(Debug, Error)]
enum Error {
    #[error("I/O Error")]
    Io(#[from] io::Error),
    #[error("Deserialization Error")]
    De(#[from] serde_json::Error),
}

#[derive(Debug, Parser)]
struct Arguments {
    #[command(subcommand)]
    subcommand: Subcommand,
}

#[derive(Debug, clap::Subcommand)]
enum Subcommand {
    // TODO list files in tag intersections
    Get { tags: Vec<String> },
    // TODO lists all tags when empty
    // TODO lists union of tags from paths
    List { paths: Paths },

    Tag { paths: Paths, tags: Vec<String> },
    Untag { paths: Paths, tags: Vec<String> },
    Clear { paths: Paths },
}

impl Subcommand {
    fn execute(self) {
        match self {
            Self::Get { tags: query } => {
                let paths = match ResolvedTags::try_from(RawTag::query(query.into_iter().collect()))
                {
                    Ok(paths) => paths,
                    Err(cause) => {
                        log::error!("Unable to search by tag: {cause}");
                        return;
                    }
                };
                let mut paths = paths.intersection().into_iter().collect_vec();
                paths.sort();
                for path in paths {
                    println!("{}", path.display());
                }
            }
            Self::Tag { paths, tags } => {
                for key in &tags {
                    let Some(mut tag) = load_tag(key) else {
                        continue;
                    };
                    tag.paths_mut().extend(paths.clone());
                    if let Err(cause) = tag.save(key) {
                        log::warn!("Unable to save tag {key:?}: {cause}");
                    }
                }

                for path in paths {
                    let Some(mut meta) = load_meta(&path) else {
                        continue;
                    };
                    meta.tags_mut().extend(tags.iter().cloned());
                    if let Err(cause) = meta.save(&path) {
                        log::warn!(
                            "Unable to save metadata for path {}: {cause}",
                            path.display()
                        );
                    }
                }
            }
            Self::Untag { paths, tags } => {
                for key in &tags {
                    let Some(mut tag) = load_tag(key) else {
                        continue;
                    };
                    for path in paths.clone() {
                        tag.paths_mut().remove(&path);
                    }
                    if let Err(cause) = tag.save(key) {
                        log::warn!("Unable to save tag {key:?}: {cause}");
                    }
                }

                for path in paths {
                    let Some(mut meta) = load_meta(&path) else {
                        continue;
                    };
                    for tag in &tags {
                        meta.tags_mut().remove(tag);
                    }
                    if let Err(cause) = meta.save(&path) {
                        log::warn!(
                            "Unable to save metadata for path {}: {cause}",
                            path.display()
                        );
                    }
                }
            }
            it => todo!("{it:#?}"),
        }
    }
}

fn load_tag<P: AsRef<Path>>(key: P) -> Option<RawTag> {
    let key = key.as_ref();
    match RawTag::load(key) {
        Ok(tag) => Some(tag),
        Err(IoTagError::Io(cause)) if matches!(cause.kind(), io::ErrorKind::NotFound) => {
            log::info!("Fallback to default for tag {key:?} since it doesn't exist: {cause}");
            Some(RawTag::default())
        }
        Err(cause) => {
            log::warn!("Unable to load tag {key:?}: {cause}");
            None
        }
    }
}

fn load_meta<P: AsRef<Path>>(path: P) -> Option<PathMetadata> {
    let path = path.as_ref();
    match PathMetadata::load(path) {
        Ok(meta) => Some(meta),
        Err(cause) if matches!(cause.kind(), io::ErrorKind::NotFound) => {
            log::info!(
                "Fallback to default metadata for path {} since it doesn't exist: {cause}",
                path.display()
            );
            Some(PathMetadata::default())
        }
        Err(cause) => {
            log::warn!(
                "Unable to load metadata for path {}: {cause}",
                path.display()
            );
            None
        }
    }
}

#[derive(Clone, Debug)]
struct Paths {
    inner: std::vec::IntoIter<PathBuf>,
}

impl FromStr for Paths {
    type Err = io::Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = std::env::split_paths(s)
            .map(std::path::absolute)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter();
        Ok(Self { inner })
    }
}

impl Iterator for Paths {
    type Item = PathBuf;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}
