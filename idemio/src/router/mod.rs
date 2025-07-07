pub mod exchange;
pub mod factory;
pub mod route;

use crate::handler::config::{ChainId, HandlerId};
use crate::handler::registry::HandlerRegistry;
use crate::router::exchange::Exchange;
use crate::router::factory::ExchangeFactory;
use crate::router::route::{LoadedChain, PathConfig, PathRouter, RouteInfo};
use crate::status::{ExchangeState, HandlerExecutionError};
use async_trait::async_trait;
use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

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
    
    pub fn exchange_timeout_error(start: SystemTime, end: SystemTime, id: impl Into<String>) -> Self {
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
            },
            RouterError::ExchangeTimeoutError(start, end, id) => {
                let duration = end.duration_since(*start).unwrap_or_else(|_| Duration::from_secs(0));
                write!(f, "Exchanged timed out after {}ms while executing handler '{}'", duration.as_millis(), id)
            },
            RouterError::ExchangeStatusError(state, code) => {
                write!(f, "Returning status code {} because state was marked as {}", code, state)
            }
        }
    }
}

impl std::error::Error for RouterError {}

/// Main router trait that processes requests through handler chains
#[async_trait]
pub trait Router<R, I, O, M>
where
    Self: Send + Sync,
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    /// Process a request through the router
    async fn route(&self, request: R) -> Result<O, RouterError>;
}

/// Default implementation of the Router trait
pub struct IdemioRouter<R, I, O, M, F>
where
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
    F: ExchangeFactory<R, I, O, M>,
    R: Send,
{
    phantom: PhantomData<R>,
    exchange_factory: Arc<F>,
    route_table: PathRouter<I, O, M>,
}

impl<R, I, O, M, F> IdemioRouter<R, I, O, M, F>
where
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
    F: ExchangeFactory<R, I, O, M>,
{
    pub fn new(
        registry: &HandlerRegistry<I, O, M>,
        config: &PathConfig,
        exchange_factory: F,
    ) -> Self {
        let route_table = match PathRouter::new(config, registry) {
            Ok(table) => table,
            Err(_) => todo!(),
        };
        Self {
            phantom: PhantomData::default(),
            exchange_factory: Arc::new(exchange_factory),
            route_table,
        }
    }

    fn get_handlers_from_route_info(
        &self,
        route_info: RouteInfo,
    ) -> Option<Arc<LoadedChain<I, O, M>>> {
        self.route_table.lookup(route_info)
    }

    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<I, O, M>>,
        exchange: &mut Exchange<I, O, M>,
    ) -> Result<(), RouterError> {
        for handler in executables.request_handlers.iter() {
            let start_time = SystemTime::now();
            let status = handler
                .exec(exchange)
                .await
                .map_err(|e| RouterError::handler_execution_failed(e))?;

            if status.code().all_flags(ExchangeState::REQUEST_COMPLETED) {
                return Ok(());
            }
            
            if status.code().all_flags(ExchangeState::TIMEOUT) {
                let end_time = SystemTime::now();
                return Err(RouterError::exchange_timeout_error(start_time, end_time, handler.name()))
            }
            
            if status.code().any_flags(ExchangeState::CLIENT_ERROR | ExchangeState::SERVER_ERROR) {
                log::warn!("Handler '{}' is disabled", handler.name());
            }

            if status.code().is_error() {
                todo!("Handle status code errors")
            }
        }
        executables
            .termination_handler
            .exec(exchange)
            .await
            .map_err(|e| RouterError::handler_execution_failed(e))?;
        for handler in executables.response_handlers.iter() {
            let start_time = std::time::SystemTime::now();
            
            let status = handler
                .exec(exchange)
                .await
                .map_err(|e| RouterError::handler_execution_failed(e))?;

            if status.code().all_flags(ExchangeState::RESPONSE_COMPLETED) {
                return Ok(());
            }

            if status.code().is_error() {
                todo!("handle status code errors")
            }
        }

        Ok(())
    }
}

#[async_trait]
impl<R, I, O, M, F> Router<R, I, O, M> for IdemioRouter<R, I, O, M, F>
where
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
    F: ExchangeFactory<R, I, O, M>,
{
    async fn route(&self, request: R) -> Result<O, RouterError> {
        let route_info = self.exchange_factory.extract_route_info(&request).await?;

        log::trace!(
            "Extracted route info from request: {}@{}",
            &route_info.path,
            &route_info.method
        );
        let executables = self
            .get_handlers_from_route_info(route_info)
            .ok_or(RouterError::RouteNotFound)?;
        let mut exchange = self.exchange_factory.create_exchange(request).await?;
        log::debug!("Created new exchange {}", exchange.uuid());
        self.execute_handlers(executables, &mut exchange).await?;
        exchange.take_output().map_err(|e| todo!())
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
        impl ExchangeFactory<String, String, String, ()> for CustomExchangeFactory {
            async fn extract_route_info(&self, request: &String) -> Result<RouteInfo, RouterError> {
                let mut parts = request.split(':');
                if let (Some(method), Some(path), Some(_)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    Ok(RouteInfo::new(path, method))
                } else {
                    Err(RouterError::ExchangeCreationFailed(
                        "Invalid format".to_string(),
                    ))
                }
            }

            async fn create_exchange(
                &self,
                request: String,
            ) -> Result<Exchange<String, String, ()>, RouterError> {
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

        let route_info = factory.extract_route_info(&custom_request).await.unwrap();
        assert_eq!(route_info.method, "GET");
        assert_eq!(route_info.path, "/test");

        let exchange = factory.create_exchange(custom_request).await.unwrap();
        assert_eq!(exchange.input().unwrap(), "body_content");
    }
}
