use std::io;

use clap::Parser;
use thiserror::Error;

use path_tagging::{RawTag, ResolvedTag};

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
    List { paths: String },

    Tag { paths: String, tags: Vec<String> },
    Untag { paths: String, tags: Vec<String> },
    Clear { paths: String },
}

impl Subcommand {
    fn execute(self) -> Result<(), self::Error> {
        match self {
            Self::Get { tags: query } => {
                let paths = ResolvedTag::try_from(RawTag::query(query.into_iter().collect()))
                    .expect("TODO")
                    .intersection();

                dbg!(paths);
                Ok(())
            }
            it => todo!("{it:#?}"),
        }
    }
}
