pub mod registry;
pub mod config;
use crate::status::{HandlerExecutionError, HandlerStatus};
use async_trait::async_trait;
use std::sync::Arc;


use crate::exchange::buffered::BufferedExchange;

#[cfg(feature = "stream")]
use crate::exchange::streaming::StreamingExchange;

pub type SharedBufferedHandler<I, O, M> = Arc<dyn BufferedHandler<I, O, M>>;

#[cfg(feature = "stream")]
pub type SharedStreamingHandler<I, O, M> = Arc<dyn StreamingHandler<I, O, M>>;

pub enum Handler<I, O, M> 
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    Buffered(Arc<dyn BufferedHandler<I, O, M>>),
    #[cfg(feature = "stream")]
    Streaming(Arc<dyn StreamingHandler<I, O, M>>),
}

#[async_trait]
pub trait BufferedHandler<I, O, M>: Send
where
    Self: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    async fn exec(
        &self,
        exchange: &mut BufferedExchange<I, O, M>,
    ) -> Result<HandlerStatus, HandlerExecutionError>;
    
    fn name(&self) -> &str;
}

#[cfg(feature = "stream")]
#[async_trait]
pub trait StreamingHandler<I, O, M> 
where
    Self: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    async fn exec(
        &self,
        exchange: &mut StreamingExchange<I, O, M>,
    );
    fn name(&self) -> &str;
}