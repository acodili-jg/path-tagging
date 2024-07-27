use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use derive_new::new;
use itertools::Itertools;
use linked_hash_set::LinkedHashSet;
use thiserror::Error;

#[derive(Clone, Debug, Default, Eq, new, PartialEq, getset::Getters, getset::MutGetters)]
#[getset(get = "pub", get_mut = "pub")]
pub struct PathMetadata {
    tags: HashSet<String>,
}

/// A raw tag.
#[derive(
    Clone,
    Debug,
    Default,
    Eq,
    new,
    PartialEq,
    getset::Getters,
    getset::MutGetters,
    serde::Deserialize,
    serde::Serialize,
)]
#[getset(get = "pub", get_mut = "pub")]
pub struct RawTag {
    /// Tags whose paths are included in this tag. This is the inverse of
    /// [`inheritedTags`].
    include_tags: HashSet<String>,
    /// Tags for [`paths`] to inherit.
    inherited_tags: HashSet<String>,
    /// Paths declared to this tag inherits tags through [`inheritedTags`] if
    /// any.
    paths: HashSet<PathBuf>,
}

#[derive(Clone, Debug, getset::Getters, getset::MutGetters)]
#[getset(get = "pub", get_mut = "pub")]
pub struct ResolvedTags {
    raw: RawTag,
    tags: HashMap<String, RawTag>,
}

#[derive(Debug, Error)]
pub enum IoTagError {
    #[error("unable to access this executable's directory")]
    Resolve(io::Error),
    #[error("i/o errors")]
    Io(#[from] io::Error),
    #[error("(de)serialization error")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Error, new)]
pub enum ResolveError {
    #[error("unable to resolve due to dependency load error")]
    Load {
        path: ResolvePath,
        #[source]
        source: IoTagError,
    },
    #[error("unable to load due to (de)serialization errors")]
    Cyclic { path: ResolvePath },
}

#[derive(Clone, Debug)]
pub struct ResolvePath {
    inner: std::vec::IntoIter<String>,
}

impl RawTag {
    #[inline]
    #[must_use]
    pub fn query(include_tags: HashSet<String>) -> Self {
        Self {
            include_tags,
            ..Default::default()
        }
    }

    #[inline]
    pub fn resolve<P: AsRef<Path>>(name: P) -> io::Result<PathBuf> {
        let name = name.as_ref();
        Ok(if name.is_absolute() {
            name.into()
        } else {
            let mut path = std::env::current_exe()?;
            path.pop();
            path.push(".tags");
            path.push(name);
            path.set_extension("json");
            path
        })
    }

    /// Loads a raw tag.
    ///
    /// Resolution starts relative to the current executable's directory or the
    /// given path if absolute.
    ///
    /// # Errors
    ///
    /// Following are possible causes for errors:
    ///  * relative path resolution fails
    ///  * I/O error when reading bytes
    ///  * parsing error
    #[inline]
    pub fn load<P: AsRef<Path>>(name: P) -> Result<Self, IoTagError> {
        let path = Self::resolve(name).map_err(IoTagError::Resolve)?;
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    #[inline]
    pub fn save<P: AsRef<Path>>(&self, name: P) -> Result<(), IoTagError> {
        let path = Self::resolve(name).map_err(IoTagError::Resolve)?;
        if self.is_empty() {
            std::fs::remove_file(path)?;
        } else {
            std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        }
        Ok(())
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.include_tags.is_empty() && self.inherited_tags.is_empty() && self.paths.is_empty()
    }
}

impl ResolvePath {
    #[inline]
    fn new(path: LinkedHashSet<String>, cause: String) -> Self {
        // FIXME: is there a better way to get an at-least debuggable iterator?
        let mut inner = path.into_iter().collect_vec();
        inner.push(cause);
        inner.into_iter().collect()
    }
}

impl ResolvedTags {
    #[must_use]
    pub fn contains(&self, path: &PathBuf) -> bool {
        self.raw.paths.contains(path)
            || self
                .raw
                .include_tags
                .iter()
                .filter_map(|key| self.tags.get(key))
                .any(|tag| tag.paths.contains(path))
    }

    #[inline]
    #[must_use]
    pub fn union(&self) -> HashSet<PathBuf> {
        Self::union_at(&self.tags, &self.raw)
    }

    #[inline]
    #[must_use]
    pub fn union_at(tags: &HashMap<String, RawTag>, tag: &RawTag) -> HashSet<PathBuf> {
        let mut set = HashSet::new();
        Self::union_helper(tags, tag, &mut set);
        set
    }

    fn union_helper(tags: &HashMap<String, RawTag>, raw: &RawTag, set: &mut HashSet<PathBuf>) {
        for tag in raw.include_tags.iter().filter_map(|key| tags.get(key)) {
            Self::union_helper(tags, tag, set);
        }
        set.extend(raw.paths.iter().cloned());
    }

    #[must_use]
    pub fn intersection(&self) -> HashSet<PathBuf> {
        fn fallible_intersection(
            lhs: Option<HashSet<PathBuf>>,
            rhs: Option<HashSet<PathBuf>>,
        ) -> Option<HashSet<PathBuf>> {
            let mut lhs = lhs?;
            let mut rhs = rhs?;
            if rhs.capacity() < lhs.capacity() {
                std::mem::swap(&mut lhs, &mut rhs);
            }
            lhs.retain(|path| rhs.contains(path));
            Some(lhs)
        }

        let mut set = self
            .raw
            .include_tags
            .iter()
            .map(|key| Some(Self::union_at(&self.tags, self.tags.get(key)?)))
            .tree_reduce(fallible_intersection)
            .flatten()
            .unwrap_or_default();

        set.extend(self.raw.paths.iter().cloned());
        set
    }

    #[must_use]
    pub fn all_tags(&self) -> HashSet<String> {
        fn helper(tags: &mut HashSet<String>, raws: &HashMap<String, RawTag>, raw: &RawTag) {
            for tag in raw.inherited_tags() {
                if tags.insert(tag.clone()) {
                    if let Some(raw) = raws.get(tag) {
                        helper(tags, raws, raw);
                    }
                }
            }
        }

        let mut set = HashSet::new();
        for tag in self.raw.include_tags() {
            if set.insert(tag.clone()) {
                if let Some(raw) = self.tags.get(tag) {
                    helper(&mut set, &self.tags, raw);
                }
            }
        }
        set
    }
}

impl From<ResolvedTags> for RawTag {
    #[inline]
    fn from(resolved: ResolvedTags) -> Self {
        resolved.raw
    }
}

impl FromIterator<String> for ResolvePath {
    #[inline]
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        let inner = iter.into_iter().collect_vec().into_iter();
        Self { inner }
    }
}

impl Iterator for ResolvePath {
    type Item = String;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl PathMetadata {
    pub const EXTENSION_PREFIX: &'static str = ".tag.list";

    #[inline]
    pub fn resolve<P: AsRef<Path>>(path: P) -> PathBuf {
        let path = path.as_ref();
        if path.is_file() {
            let mut file_name = path.file_name().unwrap_or_default().to_os_string();
            file_name.push(Self::EXTENSION_PREFIX);
            path.with_file_name(file_name)
        } else {
            path.join(Self::EXTENSION_PREFIX)
        }
    }

    #[inline]
    pub fn load<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = Self::resolve(path.as_ref());
        let tags = std::fs::read_to_string(path)?.lines().map_into().collect();
        Ok(Self::new(tags))
    }

    #[inline]
    pub fn save<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let path = Self::resolve(path.as_ref());
        if self.is_empty() {
            std::fs::remove_file(path)?;
        } else {
            std::fs::write(path, self.tags.iter().join("\n"))?;
        }
        Ok(())
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

impl TryFrom<RawTag> for ResolvedTags {
    type Error = ResolveError;

    #[inline]
    fn try_from(raw: RawTag) -> Result<Self, Self::Error> {
        fn helper(
            mut path: LinkedHashSet<String>,
            tags: &mut HashMap<String, RawTag>,
            raw: &RawTag,
        ) -> Result<LinkedHashSet<String>, ResolveError> {
            let keys = raw.include_tags.union(raw.inherited_tags());
            for key in keys {
                if path.contains(key) {
                    return Err(ResolveError::new_cyclic(ResolvePath::new(
                        path,
                        key.clone(),
                    )));
                }

                path.insert(key.clone());

                let tag = match RawTag::load(key) {
                    Ok(tag) => Some(tag),
                    Err(IoTagError::Resolve(_)) => None,
                    Err(IoTagError::Io(cause))
                        if matches!(cause.kind(), io::ErrorKind::NotFound) =>
                    {
                        None
                    }
                    Err(cause) => {
                        return Err(ResolveError::new_load(path.into_iter().collect(), cause))
                    }
                };

                let key = path.pop_back();
                if let Some(tag) = tag {
                    path = helper(path, tags, &tag)?;
                    // SAFETY: assert insert was called once before this
                    let key = unsafe { key.unwrap_unchecked() };
                    tags.insert(key, tag);
                }
            }
            Ok(path)
        }

        let path = LinkedHashSet::new();
        let mut tags = HashMap::new();
        helper(path, &mut tags, &raw)?;
        Ok(Self { raw, tags })
    }
}
