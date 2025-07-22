pub mod factory;
pub mod route;
pub mod executor;

use crate::handler::config::{ChainId, HandlerId};
use crate::handler::registry::HandlerRegistry;
use crate::router::factory::ExchangeFactory;
use crate::router::route::{LoadedChain, PathConfig, PathRouter};
use crate::status::{ExchangeState, HandlerExecutionError};
use async_trait::async_trait;
use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use crate::exchange::Exchange;
use crate::router::executor::HandlerExecutor;

#[derive(Debug)]
pub enum RouterError {
    RouteNotFound,
    MethodNotSupported,
    HandlerNotFound(HandlerId),
    ChainNotFound(ChainId),
    ExecutionFailed(String),
    ExchangeCreationFailed(String),
    ExchangeTimeoutError(SystemTime, SystemTime, String),
    ExchangeStatusError(ExchangeState, i32),
}

impl RouterError {
    pub fn exchange_status_error(status: ExchangeState, code: i32) -> Self {
        RouterError::ExchangeStatusError(status, code)
    }

    pub fn exchange_timeout_error(
        start: SystemTime,
        end: SystemTime,
        id: impl Into<String>,
    ) -> Self {
        RouterError::ExchangeTimeoutError(start, end, id.into())
    }

    pub fn route_not_found() -> Self {
        RouterError::RouteNotFound
    }

    pub fn method_not_supported() -> Self {
        RouterError::MethodNotSupported
    }
    pub fn handler_not_found(id: HandlerId) -> Self {
        RouterError::HandlerNotFound(id)
    }
    pub fn chain_not_found(id: ChainId) -> Self {
        RouterError::ChainNotFound(id)
    }
    pub fn execution_failed(msg: String) -> Self {
        RouterError::ExecutionFailed(msg)
    }

    pub fn exchange_creation_failed(msg: String) -> Self {
        RouterError::ExchangeCreationFailed(msg)
    }

    pub fn handler_execution_failed(e: HandlerExecutionError) -> Self {
        RouterError::ExecutionFailed(e.to_string())
    }
}

impl Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::RouteNotFound => write!(f, "Route not found"),
            RouterError::MethodNotSupported => write!(f, "Method not supported"),
            RouterError::HandlerNotFound(id) => write!(f, "Handler not found: {:?}", id),
            RouterError::ChainNotFound(id) => write!(f, "Chain not found: {:?}", id),
            RouterError::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            RouterError::ExchangeCreationFailed(msg) => {
                write!(f, "Exchange creation failed: {}", msg)
            }
            RouterError::ExchangeTimeoutError(start, end, id) => {
                let duration = end
                    .duration_since(*start)
                    .unwrap_or_else(|_| Duration::from_secs(0));
                write!(
                    f,
                    "Exchanged timed out after {}ms while executing handler '{}'",
                    duration.as_millis(),
                    id
                )
            }
            RouterError::ExchangeStatusError(state, code) => {
                write!(
                    f,
                    "Returning status code {} because state was marked as {}",
                    code, state
                )
            }
        }
    }
}

impl std::error::Error for RouterError {}

/// Main router trait that processes requests through handler chains
#[async_trait]
pub trait Router<Req, In, Out, Meta>
where
    Self: Send + Sync,
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Process a request through the router
    async fn route(&self, request: Req) -> Result<Out, RouterError>;
}



/// Default implementation of the Router trait
pub struct IdemioRouter<'a, Req, In, Out, Meta, Factory, Exec>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Exec: HandlerExecutor<In, Out, Meta> + Send + Sync,
    Factory: ExchangeFactory<Req, In, Out, Meta, Exchange<'a, In, Out, Meta>>,
    Req: Send,
{
    phantom: PhantomData<(Req, &'a ())>,
    exchange_factory: Arc<Factory>,
    handler_executor: Arc<Exec>,
    route_table: PathRouter<In, Out, Meta>,
}

impl<'a, Req, In, Out, Meta, Factory, Exec> IdemioRouter<'a, Req, In, Out, Meta, Factory, Exec>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Exec: HandlerExecutor<In, Out, Meta> + Send + Sync,
    Factory: ExchangeFactory<Req, In, Out, Meta, Exchange<'a, In, Out, Meta>>,
    Req: Send,
{
    pub fn new(
        registry: &HandlerRegistry<In, Out, Meta>,
        config: &PathConfig,
        exchange_factory: Factory,
        handler_executor: Exec,
    ) -> Self {
        let route_table = match PathRouter::new(config, registry) {
            Ok(table) => table,
            Err(_) => todo!(),
        };
        Self {
            phantom: PhantomData::default(),
            exchange_factory: Arc::new(exchange_factory),
            handler_executor: Arc::new(handler_executor),
            route_table,
        }
    }

    fn get_handlers_from_route_info(
        &self,
        request_path: &str,
        request_method: &str,
    ) -> Option<Arc<LoadedChain<In, Out, Meta>>> {
        self.route_table.lookup(request_path, request_method)
    }

    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<In, Out, Meta>>,
        exchange: &mut Exchange<'a, In, Out, Meta>,
    ) -> Result<(), RouterError> {
        self.handler_executor
            .execute_handlers(executables, exchange)
            .await
            .map_err(|_| RouterError::ExecutionFailed("Handler execution failed".to_string()))?;
        Ok(())
    }

}

#[async_trait]
impl<'a, Req, In, Out, Meta, Factory, Exec> Router<Req, In, Out, Meta>
for IdemioRouter<'a, Req, In, Out, Meta, Factory, Exec>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Exec: HandlerExecutor<In, Out, Meta> + Send + Sync,
    Factory: ExchangeFactory<Req, In, Out, Meta, Exchange<'a, In, Out, Meta>>,
{
    async fn route(&self, request: Req) -> Result<Out, RouterError> {
        let (path, method) = self.exchange_factory.extract_route_info(&request).await?;
        log::trace!("Extracted route info from request: {}@{}", &path, &method);

        let executables = self
            .get_handlers_from_route_info(path, method)
            .ok_or(RouterError::RouteNotFound)?;

        let mut exchange = self.exchange_factory.create_exchange(request).await?;

        self.execute_handlers(executables, &mut exchange).await?;

        // Use UnifiedExchange's take_output method
        exchange.take_output().await
            .map_err(|e| RouterError::ExecutionFailed(format!("Failed to get output: {}", e)))
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    #[tokio::test]
    async fn test_custom_exchange_factory() {
        /// This is a custom exchange where the request format follows the following pattern:
        /// String(<method>:<path>:<body)
        /// e.g
        /// "GET:/endpoint:body text"
        struct CustomExchangeFactory;

        #[async_trait]
        impl<'b> ExchangeFactory<String, String, String, (), Exchange<'b, String, String, ()>>
            for CustomExchangeFactory
        {
            async fn extract_route_info<'a>(
                &self,
                request: &'a String,
            ) -> Result<(&'a str, &'a str), RouterError> {
                let mut parts = request.split(':');
                if let (Some(method), Some(path), Some(_)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    Ok((path, method))
                } else {
                    Err(RouterError::ExchangeCreationFailed(
                        "Invalid format".to_string(),
                    ))
                }
            }

            async fn create_exchange(
                &self,
                request: String,
            ) -> Result<Exchange<'b, String, String, ()>, RouterError> {
                let mut exchange = Exchange::new();
                let mut parts = request.split(':');
                if let (Some(_), Some(_), Some(body)) = (parts.next(), parts.next(), parts.next()) {
                    exchange.save_input(body.to_string());
                }
                Ok(exchange)
            }
        }

        let factory = CustomExchangeFactory;
        let custom_request = "GET:/test:body_content".to_string();

        let (path, method) = factory.extract_route_info(&custom_request).await.unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/test");

        let mut exchange = factory.create_exchange(custom_request).await.unwrap();
        assert_eq!(exchange.input().await.unwrap(), "body_content");
    }
}
