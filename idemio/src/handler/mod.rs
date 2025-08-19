pub mod registry;

use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use crate::status::{HandlerStatus};
use async_trait::async_trait;
use std::sync::Arc;

pub type SharedBufferedHandler<E> = Arc<dyn Handler<E>>;

#[async_trait]
pub trait Handler<E>: Send + Sync
where
    E: Send + Sync
{
    async fn exec(
        &self,
        exchange: &mut E,
    ) -> Result<HandlerStatus, Infallible>;
    
    fn name(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandlerId {
    handler_hash: u64,
}

impl HandlerId {
    pub fn new(id: impl Into<String>) -> Self {
        let mut hasher = fnv::FnvHasher::default();
        id.into().hash(&mut hasher);
        let hash = hasher.finish();
        Self { handler_hash: hash }
    }
}

impl Display for HandlerId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.handler_hash)
    }
}