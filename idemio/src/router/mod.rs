pub mod exchange;
pub mod factory;
pub mod route;

use crate::handler::SharedHandler;
use crate::handler::config::{ChainId, HandlerId};
use crate::handler::registry::HandlerRegistry;
use crate::router::exchange::Exchange;
use crate::router::factory::ExchangeFactory;
use crate::router::route::{PathConfig, PathRouter, RouteInfo};
use crate::status::Code;
use async_trait::async_trait;
use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::Arc;

#[derive(Debug)]
pub enum RouterError {
    RouteNotFound,
    MethodNotSupported,
    HandlerNotFound(HandlerId),
    ChainNotFound(ChainId),
    ExecutionFailed(String),
    ExchangeCreationFailed(String),
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
            Err(_) => todo!()
        };
        Self {
            phantom: PhantomData::default(),
            exchange_factory: Arc::new(exchange_factory),
            route_table 
        }
    }

    fn get_handlers_from_route_info(
        &self,
        route_info: RouteInfo,
    ) -> Option<Arc<Vec<SharedHandler<I, O, M>>>> {
        self.route_table.lookup(route_info)
    }

    async fn execute_handlers(
        &self,
        executables: &[SharedHandler<I, O, M>],
        exchange: &mut Exchange<I, O, M>,
    ) -> Result<(), RouterError> {
        for handler in executables {
            let status = handler
                .exec(exchange)
                .await
                .map_err(|e| RouterError::ExecutionFailed(e.message.to_string()))?;

            if status.code().all_flags(Code::REQUEST_COMPLETED) {
                return Ok(());
            }

            if status.code().is_error() {
                return Err(RouterError::ExecutionFailed(format!(
                    "Handler returned error status: {:?}",
                    status.code()
                )));
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

        log::trace!("Extracted route info from request: {}@{}", &route_info.path, &route_info.method);
        let executables = self
            .get_handlers_from_route_info(route_info)
            .ok_or(RouterError::RouteNotFound)?;
        let mut exchange = self.exchange_factory.create_exchange(request).await?;
        log::debug!("Created new exchange {}", exchange.uuid());
        self.execute_handlers(&executables, &mut exchange).await?;
        exchange
            .take_output()
            .map_err(|e| RouterError::ExecutionFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    #[tokio::test]
    async fn test_custom_exchange_factory() {
        struct CustomExchangeFactory;

        #[async_trait]
        impl ExchangeFactory<String, String, String, ()> for CustomExchangeFactory {
            async fn extract_route_info(&self, request: &String) -> Result<RouteInfo, RouterError> {
                let parts: Vec<&str> = request.split(':').collect();
                if parts.len() >= 2 {
                    Ok(RouteInfo::new(parts[1].to_string(), parts[0].to_string()))
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
                let parts: Vec<&str> = request.split(':').collect();
                if parts.len() >= 3 {
                    exchange.save_input(parts[2].to_string());
                } else {
                    exchange.save_input("".to_string());
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
