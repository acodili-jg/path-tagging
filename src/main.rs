use std::convert::Infallible;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;
use itertools::Itertools;
use thiserror::Error;

use path_tagging::{RawTag, ResolvedTags};

fn main() -> Result<(), self::Error> {
    let args = Arguments::parse();
    args.subcommand.execute()?;
    Ok(())
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
    fn execute(self) -> Result<(), self::Error> {
        match self {
            Self::Get { tags: query } => {
                let paths = ResolvedTags::try_from(RawTag::query(query.into_iter().collect()))
                    .expect("TODO")
                    .intersection();

                Ok(())
            }
            it => todo!("{it:#?}"),
        }
    }
}

#[derive(Clone, Debug)]
struct Paths {
    inner: std::vec::IntoIter<PathBuf>,
}

impl FromStr for Paths {
    type Err = Infallible;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = std::env::split_paths(s).collect_vec().into_iter();
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
