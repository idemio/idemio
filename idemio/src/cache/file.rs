use std::fmt::{Display, Formatter};
use std::sync::Arc;
use dashmap::DashMap;
use fnv::FnvBuildHasher;
use once_cell::sync::Lazy;
use crate::cache::{Cache, CacheError};

static FILE_CACHE: Lazy<FileCache> = Lazy::new(FileCache::new);


pub struct FileCache {
    files: DashMap<String, Arc<String>, FnvBuildHasher>,
}

impl FileCache {
    fn new() -> Self {
        Self {
            files: DashMap::with_hasher(FnvBuildHasher::default()),
        }
    }
}

impl Cache<String> for FileCache {
    type Error = FileCacheError;

    fn inner(&self) -> &DashMap<String, Arc<String>, FnvBuildHasher> {
        &self.files
    }

    fn cache_id(&self) -> &'static str {
        "FileCache"
    }
}

#[derive(Debug)]
pub struct FileCacheError {
    message: String,
}

impl Display for FileCacheError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for FileCacheError {}

impl CacheError for FileCacheError {
    fn handle(msg: impl Into<String>) -> Self {
        FileCacheError {
            message: msg.into(),
        }
    }
}