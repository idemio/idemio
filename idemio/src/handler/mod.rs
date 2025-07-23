pub mod registry;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use crate::status::{HandlerStatus};
use async_trait::async_trait;
use std::sync::Arc;
use crate::exchange::Exchange;

pub type SharedBufferedHandler<I, O, M> = Arc<dyn Handler<I, O, M>>;

#[async_trait]
pub trait Handler<In, Out, Meta>
where
    Self: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, In, Out, Meta>,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChainId {
    chain_hash: u64,
}

impl ChainId {
    pub fn new(id: impl Into<String>) -> Self {
        let mut hasher = fnv::FnvHasher::default();
        id.into().hash(&mut hasher);
        let hash = hasher.finish();
        Self { chain_hash: hash }
    }
}

impl Display for ChainId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChainId({})", self.chain_hash)
    }
}