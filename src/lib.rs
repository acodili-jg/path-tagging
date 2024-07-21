use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use derive_new::new;
use itertools::Itertools;
use linked_hash_set::LinkedHashSet;
use thiserror::Error;

/// A raw tag.
#[derive(Debug, Default, Eq, new, PartialEq, serde::Deserialize, serde::Serialize)]
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

#[derive(Debug)]
pub struct ResolvedTag {
    include_tags: HashMap<String, Rc<RefCell<ResolvedTag>>>,
    inherited_tags: HashMap<String, Rc<RefCell<ResolvedTag>>>,
    paths: HashSet<PathBuf>,
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("unable to access this executable's directory")]
    Resolve(io::Error),
    #[error("unable to load due to i/o errors")]
    Io(#[from] io::Error),
    #[error("unable to load due to (de)serialization errors")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Error, new)]
pub enum ResolveError {
    #[error("unable to resolve due to dependency load error")]
    Load {
        path: ResolvePath,
        #[source]
        source: LoadError,
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
    pub fn load<P: AsRef<Path>>(name: P) -> Result<Self, LoadError> {
        let name = name.as_ref();
        let path = if name.is_absolute() {
            name.into()
        } else {
            let mut path = std::env::current_exe().map_err(LoadError::Resolve)?;
            path.pop();
            path.push(".tags");
            path.push(name);
            path.set_extension("json");
            path
        };
        dbg!(&path);
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    #[inline]
    #[must_use]
    pub const fn include_tags(&self) -> &HashSet<String> {
        &self.include_tags
    }

    #[inline]
    #[must_use]
    pub const fn inherited_tags(&self) -> &HashSet<String> {
        &self.inherited_tags
    }

    #[inline]
    #[must_use]
    pub const fn paths(&self) -> &HashSet<PathBuf> {
        &self.paths
    }

    #[inline]
    pub fn include_tags_mut(&mut self) -> &HashSet<String> {
        &mut self.include_tags
    }

    #[inline]
    pub fn inherited_tags_mut(&mut self) -> &HashSet<String> {
        &mut self.inherited_tags
    }

    #[inline]
    pub fn paths_mut(&mut self) -> &HashSet<PathBuf> {
        &mut self.paths
    }
}

impl ResolvePath {
    #[inline]
    fn from(inner: LinkedHashSet<String>) -> Self {
        // FIXME: is there a better way to get an at-least debuggable iterator?
        let inner = inner.into_iter().collect_vec().into_iter();
        Self { inner }
    }
}

impl ResolvedTag {
    #[must_use]
    pub fn contains(&self, path: &PathBuf) -> bool {
        self.paths.contains(path)
            || self
                .include_tags
                .values()
                .any(|tag| tag.borrow().contains(path))
    }

    #[inline]
    #[must_use]
    pub fn union(&self) -> HashSet<PathBuf> {
        let mut set = HashSet::new();
        self.union_helper(&mut set);
        set
    }

    fn union_helper(&self, set: &mut HashSet<PathBuf>) {
        for include in self.include_tags.values() {
            include.borrow().union_helper(set);
        }
        set.extend(self.paths.iter().cloned());
    }

    #[must_use]
    pub fn intersection(&self) -> HashSet<PathBuf> {
        dbg!(self);
        let mut set = self
            .include_tags
            .values()
            .map(|tag| tag.borrow().union())
            .tree_reduce(|mut lhs, mut rhs| {
                if rhs.capacity() < lhs.capacity() {
                    std::mem::swap(&mut lhs, &mut rhs);
                }
                lhs.retain(|path| rhs.contains(path));
                lhs
            })
            .unwrap_or_default();

        set.extend(self.paths.iter().cloned());
        set
    }
}

impl From<ResolvedTag> for RawTag {
    #[inline]
    fn from(resolved: ResolvedTag) -> Self {
        Self {
            include_tags: resolved.include_tags.into_keys().collect(),
            inherited_tags: resolved.inherited_tags.into_keys().collect(),
            paths: resolved.paths,
        }
    }
}

impl Iterator for ResolvePath {
    type Item = String;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl TryFrom<RawTag> for ResolvedTag {
    type Error = ResolveError;

    #[inline]
    fn try_from(raw: RawTag) -> Result<Self, Self::Error> {
        fn get_tag(
            path: &mut Option<LinkedHashSet<String>>,
            tags: &mut HashMap<String, Rc<RefCell<ResolvedTag>>>,
            key: String,
        ) -> Result<(String, Rc<RefCell<ResolvedTag>>), ResolveError> {
            // ASSERTION: path should not be none when there is no error.

            if unsafe { path.as_ref().unwrap_unchecked() }.contains(&key) {
                return Err(ResolveError::new_cyclic(ResolvePath::from(unsafe {
                    path.take().unwrap_unchecked()
                })));
            }

            if let Some(tag) = tags.get(&key) {
                return Ok((key, Rc::clone(tag)));
            }

            let raw = match RawTag::load(&key) {
                Ok(raw) => raw,
                Err(LoadError::Resolve(_)) => RawTag::default(),
                Err(LoadError::Io(cause)) if matches!(cause.kind(), io::ErrorKind::NotFound) => {
                    RawTag::default()
                }
                Err(source) => {
                    return Err(ResolveError::new_load(
                        ResolvePath::from(unsafe { path.take().unwrap_unchecked() }),
                        source,
                    ))
                }
            };

            unsafe { path.as_mut().unwrap_unchecked() }.insert(key);
            let tag = Rc::new(RefCell::new(helper(path, tags, raw)?));

            // ASSERTION: There will always be a value *before* but each
            //            `get_tag` **should** only pop once here.
            let key = unsafe {
                path.as_mut()
                    .unwrap_unchecked()
                    .pop_back()
                    .unwrap_unchecked()
            };
            tags.insert(key.clone(), Rc::clone(&tag));
            Ok((key, tag))
        }

        fn helper(
            path: &mut Option<LinkedHashSet<String>>,
            tags: &mut HashMap<String, Rc<RefCell<ResolvedTag>>>,
            raw: RawTag,
        ) -> Result<ResolvedTag, ResolveError> {
            Ok(ResolvedTag {
                include_tags: raw
                    .include_tags
                    .into_iter()
                    .map(|key| get_tag(path, tags, key))
                    .try_collect()?,
                inherited_tags: raw
                    .inherited_tags
                    .into_iter()
                    .map(|key| get_tag(path, tags, key))
                    .try_collect()?,
                paths: raw.paths,
            })
        }

        helper(&mut Some(LinkedHashSet::new()), &mut HashMap::new(), raw)
    }
}
