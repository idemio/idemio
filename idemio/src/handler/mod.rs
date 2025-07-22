pub mod registry;
pub mod config;
use crate::status::{HandlerExecutionError, HandlerStatus};
use async_trait::async_trait;
use std::sync::Arc;
use crate::exchange::Exchange;

pub type SharedBufferedHandler<I, O, M> = Arc<dyn Handler<I, O, M>>;

#[async_trait]
pub trait Handler<I, O, M>: Send + Sync
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, I, O, M>,
    ) -> Result<HandlerStatus, HandlerExecutionError>;
    
    fn name(&self) -> &str;
}