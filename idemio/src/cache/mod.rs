mod file;

use std::sync::Arc;
use dashmap::{DashMap, Entry};
use fnv::FnvBuildHasher;

pub trait CacheError
where
    Self: std::error::Error
{
    fn handle(msg: impl Into<String>) -> Self;
}

pub trait Cache<D> {

    // TODO - remove this type and make it so all caches return the same error type. Include the id/name of the cache in the error message.
    type Error: CacheError;
    fn inner(&self) -> &DashMap<String, Arc<D>, FnvBuildHasher>;
    fn cache_id(&self) -> &'static str;

    fn get(&self, key: impl Into<String>) -> Result<Arc<D>, Self::Error> {
        match self.inner().entry(key.into()) {
            Entry::Occupied(found) => Ok(found.get().clone()),
            Entry::Vacant(_) => Err(Self::Error::handle("Key not found")),
        }
    }

    fn get_or_resolve<F>(
        &self,
        key: impl Into<String>,
        resolver: F,
    ) -> Result<Arc<D>, Self::Error>
    where
        F: FnOnce() -> Result<D, Self::Error>,
    {
        match self.inner().entry(key.into()) {
            Entry::Occupied(found) => Ok(found.get().clone()),
            Entry::Vacant(vacant) => {
                let value = resolver()?;
                let value = Arc::new(value);
                vacant.insert(value.clone());
                Ok(value)
            }
        }
    }

    fn init(&mut self, key: impl Into<String>, value: D) -> Result<(), Self::Error> {
        match self.inner().entry(key.into()) {
            Entry::Occupied(_) => Err(Self::Error::handle("Key already exists")),
            Entry::Vacant(vacant) => {
                let value = Arc::new(value);
                vacant.insert(value.clone());
                Ok(())
            }
        }
    }
}