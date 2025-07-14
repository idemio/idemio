use std::sync::Arc;
use async_trait::async_trait;
use crate::exchange::RootExchange;
use crate::router::route::LoadedChain;

#[async_trait]
pub trait HandlerExecutor<Ex, In, Out, Meta>
where
    Ex: RootExchange<In, Out, Meta>,
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

#[cfg(feature = "hyper")]
pub mod hyper {
    use std::sync::Arc;
    use async_trait::async_trait;
    use crate::exchange::buffered::BufferedExchange;
    #[cfg(feature = "stream")]
    use crate::exchange::streaming::StreamingExchange;
    
    use crate::router::executor::HandlerExecutor;
    use crate::router::route::LoadedChain;
    
    #[cfg(feature = "stream")]
    pub struct HyperStreamingExecutor;

    #[cfg(feature = "stream")]
    #[async_trait]
    impl<I, O, M> HandlerExecutor<StreamingExchange<I, O, M>, I, O, M> for HyperStreamingExecutor 
    where
        I: Send + Sync,
        O: Send + Sync,
        M: Send + Sync,
    {
        async fn execute_handlers(&self, executables: Arc<LoadedChain<I, O, M>>, exchange: &mut StreamingExchange<I, O, M>) -> Result<(), ()> {
            todo!()
        }
    }
    
    pub struct HyperExecutor;
    
    #[async_trait]
    impl<I, O, M> HandlerExecutor<BufferedExchange<I, O, M>, I, O, M> for HyperExecutor 
    where
        I: Send + Sync,
        O: Send + Sync,
        M: Send + Sync,
    {
        async fn execute_handlers(&self, executables: Arc<LoadedChain<I, O, M>>, exchange: &mut BufferedExchange<I, O, M>) -> Result<(), ()> {
            todo!()
        }
    }
    
}