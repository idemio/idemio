pub mod config;
pub mod exchange;
pub mod factory;
pub mod route;

use crate::handler::config::{ChainId, Executable, HandlerId};
use crate::handler::registry::HandlerRegistry;
use crate::handler::{Handler, SharedHandler};
use crate::router::config::{PathSegment, RouteInfo, RouterConfig};
use crate::router::exchange::Exchange;
use crate::router::factory::ExchangeFactory;
use crate::router::route::DynamicRouteTable;
use crate::status::Code;
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Display;
use std::marker::PhantomData;
use std::str::FromStr;
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
    handlers: HashMap<HandlerId, SharedHandler<I, O, M>>,
    chains: HashMap<ChainId, Vec<SharedHandler<I, O, M>>>,
    exchange_factory: Arc<F>,
    route_table: DynamicRouteTable<I, O, M>,
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
        config: &RouterConfig,
        exchange_factory: F,
    ) -> Self {
        let mut router = Self {
            handlers: HashMap::new(),
            chains: HashMap::new(),
            phantom: PhantomData::default(),
            exchange_factory: Arc::new(exchange_factory),
            route_table: DynamicRouteTable::new(),
        };

        if let Err(_)=  router.parse_config(registry, config) {
            panic!("Failed to parse router config");
        }
        router
    }

    fn parse_config(&mut self, registry: &HandlerRegistry<I, O, M>, router_config: &RouterConfig) -> Result<(), ()> {
        
        // Build chains
        for (chain_id, handlers) in router_config.chains.iter() {
            let chain_id = chain_id.clone();
            let mut chained_handlers = Vec::new();
            for handler_id in handlers {
                let handler = match registry.find_with_id(handler_id) {
                    Ok(found) => found,
                    Err(_) => return Err(())
                };
                chained_handlers.push(handler);
            }
            self.chains.insert(chain_id, chained_handlers);
        }

        for (path, config) in router_config.paths.iter() {
            let path_segments = path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| PathSegment::from_str(s).unwrap())
                .collect::<Vec<_>>();

            for (config_method, config_handlers) in &config.methods {
                let mut method_handlers = Vec::new();
                for executable in config_handlers {
                    match executable {
                        Executable::Handler(handler_id) => {
                            let handler = match registry.find_with_id(handler_id) {
                                Ok(found) => found,
                                Err(_) => return Err(())
                            };
                            method_handlers.push(handler);
                        }
                        Executable::Chain(chain_id) => {
                            let chained_handlers = self.chains.get(chain_id).ok_or(())?;
                            method_handlers.extend(chained_handlers.iter().cloned());
                        }
                    }
                }

                // Determine if this is a static or dynamic path
                let has_wildcards = path_segments
                    .iter()
                    .any(|segment| matches!(segment, PathSegment::Any(_)));

                // Add to the appropriate route table
                if has_wildcards {
                    // Dynamic route with wildcards
                    self.route_table.add_dynamic_route(
                        config_method.clone(),
                        path_segments.clone(),
                        method_handlers,
                    );
                } else {
                    // Static route
                    self.route_table.add_static_route(
                        config_method.clone(),
                        path.clone(),
                        method_handlers,
                    );
                }
            }
        }
        Ok(())
    }

    fn get_handlers_from_route_info(&self, route_info: &RouteInfo) -> Option<Arc<Vec<SharedHandler<I, O, M>>>> {
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

    pub fn handlers(&self) -> &HashMap<HandlerId, SharedHandler<I, O, M>> {
        &self.handlers
    }

    pub fn chains(&self) -> &HashMap<ChainId, Vec<SharedHandler<I, O, M>>> {
        &self.chains
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

        log::trace!("Extracted route info from request: {}", route_info);
        let executables = self
            .get_handlers_from_route_info(&route_info)
            .ok_or(RouterError::RouteNotFound)?;
        let mut exchange = self.exchange_factory.create_exchange(request).await?;
        log::debug!("Created new exchange {}", exchange.uuid());
        self.execute_handlers(&executables, &mut exchange)
            .await?;
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
