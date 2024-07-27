use std::collections::HashSet;
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
    /// Gets paths all contained in the given tags.
    ///
    /// Paths containing all the given tags are displayed; displays nothing when
    /// none are found.
    Get {
        /// The tags that paths must have.
        #[arg(required = true)]
        tags: Vec<String>,
    },

    /// Lists all the tags that occur in the given paths.
    ///
    /// All tags contained in any of the given paths are displayed; displays
    /// nothing when none of the paths are tagged.
    List {
        /// The paths to collect tags from.
        ///
        /// On most Unix platforms, the separator is `:` and on Windows it is
        /// `;`.
        paths: Paths,
    },

    /// Tag paths.
    ///
    /// Adds tags to the given paths.
    Tag {
        /// The paths to tag.
        ///
        /// On most Unix platforms, the separator is `:` and on Windows it is
        /// `;`.
        paths: Paths,

        /// The tags to add to the given paths.
        #[arg(required = true)]
        tags: Vec<String>,
    },

    /// Untag paths.
    ///
    /// Removes tags from the given paths.
    Untag {
        /// The paths to untag.
        ///
        /// On most Unix platforms, the separator is `:` and on Windows it is
        /// `;`.
        paths: Paths,

        /// The tags to remove from the given paths.
        #[arg(required = true)]
        tags: Vec<String>,
    },

    /// Clear all the tags for the given paths.
    Clear {
        /// The paths to clear tags.
        ///
        /// On most Unix platforms, the separator is `:` and on Windows it is
        /// `;`.
        paths: Paths,
    },
}

impl Subcommand {
    fn execute(self) {
        match self {
            Self::Get { tags } => Self::execute_get(tags),
            Self::List { paths } => Self::execute_list(paths),
            Self::Tag { paths, tags } => Self::execute_tag(paths, tags),
            Self::Untag { paths, tags } => Self::execute_untag(paths, tags),
            Self::Clear { paths } => Self::execute_clear(paths),
        }
    }

    fn execute_get(query: Vec<String>) {
        match ResolvedTags::try_from(RawTag::query(HashSet::from_iter(query))) {
            Ok(paths) => {
                let mut paths = Vec::from_iter(paths.intersection());
                paths.sort();
                for path in paths {
                    println!("{}", path.display());
                }
            }
            Err(cause) => log::error!("Unable to search by tag: {cause}"),
        };
    }

    fn execute_list(paths: Paths) {
        let tags = paths
            .filter_map(load_meta)
            .flat_map(|meta| meta.tags().clone())
            .collect();
        match ResolvedTags::try_from(RawTag::query(tags)) {
            Ok(tag) => {
                let mut tags = Vec::from_iter(tag.all_tags());
                tags.sort();
                for tag in tags {
                    println!("{tag}");
                }
            }
            Err(cause) => log::error!("Unable list tags: {cause}"),
        };
    }

    fn execute_tag(paths: Paths, tags: Vec<String>) {
        for key in &tags {
            let Some(mut tag) = load_tag(key) else {
                continue;
            };
            tag.paths_mut().extend(paths.clone());
            save_tag(key, &tag);
        }

        for path in paths {
            let Some(mut meta) = load_meta(&path) else {
                continue;
            };
            meta.tags_mut().extend(tags.iter().cloned());
            save_meta(path, &meta);
        }
    }

    fn execute_untag(paths: Paths, tags: Vec<String>) {
        for key in &tags {
            let Some(mut tag) = load_tag(key) else {
                continue;
            };
            for path in paths.clone() {
                tag.paths_mut().remove(&path);
            }
            save_tag(key, &tag);
        }

        for path in paths {
            let Some(mut meta) = load_meta(&path) else {
                continue;
            };
            for tag in &tags {
                meta.tags_mut().remove(tag);
            }
            save_meta(path, &meta);
        }
    }

    fn execute_clear(paths: Paths) {
        let metas = paths
            .filter_map(|path| Some((load_meta(&path)?, path)))
            .collect_vec();
        let tags = metas
            .iter()
            .flat_map(|(meta, _)| meta.tags().clone())
            .collect();
        let mut query = match ResolvedTags::try_from(RawTag::query(tags)) {
            Ok(query) => query,
            Err(cause) => {
                log::error!("Unable retrieve tag data for clearing: {cause}");
                return;
            }
        };

        for (mut meta, path) in metas {
            for key in meta.tags_mut().drain() {
                if let Some(tag) = query.tags_mut().get_mut(&key) {
                    tag.paths_mut().remove(&path);
                }
            }
            save_meta(path, &meta);
        }

        for key in query.raw().include_tags() {
            if let Some(tag) = query.tags().get(key) {
                save_tag(key, tag);
            }
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

fn load_tag(key: &str) -> Option<RawTag> {
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

#[inline]
fn save_meta<P: AsRef<Path>>(path: P, meta: &PathMetadata) {
    let path = path.as_ref();
    if let Err(cause) = meta.save(path) {
        log::warn!(
            "Unable to save metadata for path {}: {cause}",
            path.display()
        );
    }
}

#[inline]
fn save_tag(key: &str, tag: &RawTag) {
    if let Err(cause) = tag.save(key) {
        log::warn!("Unable to save tag {key:?}: {cause}");
    }
}

fn set_union(mut lhs: HashSet<String>, mut rhs: HashSet<String>) -> HashSet<String> {
    if lhs.capacity() >= rhs.capacity() {
        lhs.extend(rhs);
        lhs
    } else {
        rhs.extend(lhs);
        rhs
    }
}

/// On most Unix platforms, the separator is `:` and on Windows it is `;`.
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
