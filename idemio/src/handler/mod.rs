pub mod registry;
pub mod config;
use crate::router::exchange::Exchange;
use crate::status::{HandlerExecutionError, HandlerStatus};
use async_trait::async_trait;
use std::sync::Arc;

pub type SharedHandler<I, O, M> = Arc<dyn Handler<I, O, M> + Send + Sync>;

#[async_trait]
pub trait Handler<I, O, M>: Send
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    async fn exec(
        &self,
        exchange: &mut Exchange<I, O, M>,
    ) -> Result<HandlerStatus, HandlerExecutionError>;
    
    fn name(&self) -> &str;
}
