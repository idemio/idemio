use std::sync::Arc;
use async_trait::async_trait;
use crate::router::route::LoadedChain;

#[async_trait]
pub trait HandlerExecutor<Ex, In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<In, Out, Meta>>,
        exchange: &mut Ex,
    ) -> Result<(), ()>;
}