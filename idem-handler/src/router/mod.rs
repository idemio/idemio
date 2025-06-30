pub mod route_table;

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::str::FromStr;
use std::sync::Arc;
use async_trait::async_trait;

use crate::handler::Handler;
use crate::exchange::Exchange;
use crate::status::{Code};

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PathSegment {
    Static(String),
    Any(String),
}

impl FromStr for PathSegment {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "*" {
            Ok(PathSegment::Any(s.to_string()))
        } else {
            Ok(PathSegment::Static(s.to_string()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChainId(pub String);

impl Display for ChainId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChainId({})", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandlerId(pub String);

impl Display for HandlerId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "HandlerId({})", self.0)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum Executable {
    Handler(HandlerId),
    Chain(ChainId),
}

impl Display for Executable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Executable::Handler(id) => write!(f, "Handler({})", id),
            Executable::Chain(id) => write!(f, "Chain({})", id),
        }
    }
}

impl FromStr for Executable {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("@") {
            Ok(Executable::Chain(ChainId(s.to_string())))
        } else {
            Ok(Executable::Handler(HandlerId(s.to_string())))
        }
    }
}

#[derive(Debug, Clone)]
pub struct PathConfig {
    pub methods: HashMap<String, Vec<Executable>>,
}

pub struct RouterConfig {
    pub chains: HashMap<ChainId, Vec<HandlerId>>,
    pub paths: HashMap<String, PathConfig>,
}

/// Represents routing information extracted from a request
#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub path: String,
    pub method: String,
}

impl Display for RouteInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "RouteInfo {{ path: {}, method: {} }}", self.path, self.method)
    }
}

impl RouteInfo {
    pub fn new(path: String, method: String) -> Self {
        Self { path, method }
    }
}

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
            RouterError::ExchangeCreationFailed(msg) => write!(f, "Exchange creation failed: {}", msg),
        }
    }
}

impl std::error::Error for RouterError {}

/// Trait for creating exchanges from incoming requests
#[async_trait]
pub trait ExchangeFactory<R, I, O, M>: Send + Sync
where
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    /// Extract routing information from the request
    async fn extract_route_info(&self, request: &R) -> Result<RouteInfo, RouterError>;

    /// Create an exchange from the request
    async fn create_exchange(&self, request: R) -> Result<Exchange<I, O, M>, RouterError>;
}

/// Main router trait that processes requests through handler chains
#[async_trait]
pub trait Router<R, I, O, M>: Send + Sync
where
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    /// Process a request through the router
    async fn route(&self, request: R) -> Result<O, RouterError>;

    /// Execute a specific handler chain directly
    async fn execute_chain(&self, chain_id: &ChainId, exchange: &mut Exchange<I, O, M>) -> Result<(), RouterError>;
}

/// Default implementation of the Router trait
pub struct DefaultRouter<R, I, O, M, F>
where
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
    F: ExchangeFactory<R, I, O, M>, 
    R: Send
{
    phantom: PhantomData<R>,
    handlers: HashMap<HandlerId, Arc<dyn Handler<I, O, M> + Send + Sync>>,
    chains: HashMap<ChainId, Vec<HandlerId>>,
    exchange_factory: Arc<F>,
    route_table: route_table::DynamicRouteTable,
}

impl<R, I, O, M, F> DefaultRouter<R, I, O, M, F>
where
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
    F: ExchangeFactory<R, I, O, M>
{
    pub fn new(config: RouterConfig, exchange_factory: F) -> Self {
        let mut router = Self {
            handlers: HashMap::new(),
            chains: config.chains.clone(),
            phantom: PhantomData::default(),
            exchange_factory: Arc::new(exchange_factory),
            route_table: route_table::DynamicRouteTable::new(),
        };

        router.parse_paths(&config);
        router
    }

    pub fn add_handler(&mut self, id: HandlerId, handler: Arc<dyn Handler<I, O, M> + Send + Sync>) -> &mut Self {
        self.handlers.insert(id, handler);
        self
    }

    // TODO - pre-register all handlers in registry. Build handler chains using Arc<...>
    fn parse_paths(&mut self, router_config: &RouterConfig) {
        for (path, config) in router_config.paths.iter() {
            let path_segments = path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| PathSegment::from_str(s).unwrap())
                .collect::<Vec<_>>();

            for (config_method, config_handlers) in &config.methods {
                let executables = Arc::new(config_handlers.clone());

                // Determine if this is a static or dynamic path
                let has_wildcards = path_segments.iter().any(|segment| {
                    matches!(segment, PathSegment::Any(_))
                });

                // Add to the appropriate route table
                if has_wildcards {
                    // Dynamic route with wildcards
                    self.route_table.add_dynamic_route(
                        config_method.clone(),
                        path_segments.clone(),
                        executables
                    );
                } else {
                    // Static route
                    self.route_table.add_static_route(
                        config_method.clone(),
                        path.clone(),
                        executables
                    );
                }
            }
        }
    }

    fn get_handlers_from_route_info(&self, route_info: &RouteInfo) -> Option<Arc<Vec<Executable>>> {
        self.route_table.lookup(route_info)
    }

    async fn execute_executables(
        &self,
        executables: &[Executable],
        exchange: &mut Exchange<I, O, M>,
    ) -> Result<(), RouterError> {
        for executable in executables {
            match executable {
                Executable::Handler(handler_id) => {
                    let handler = self.handlers.get(handler_id)
                        .ok_or_else(|| RouterError::HandlerNotFound(handler_id.clone()))?;

                    let status = handler.exec(exchange).await
                        .map_err(|e| RouterError::ExecutionFailed(e.message.to_string()))?;

                    if status.code().all_flags(Code::REQUEST_COMPLETED) {
                        return Ok(());
                    }

                    if status.code().is_error() {
                        return Err(RouterError::ExecutionFailed(
                            format!("Handler {:?} returned error status: {:?}", handler_id, status.code())
                        ));
                    }
                }
                Executable::Chain(chain_id) => {
                    self.execute_chain(chain_id, exchange).await?;
                }
            }
        }
        Ok(())
    }

    pub fn handlers(&self) -> &HashMap<HandlerId, Arc<dyn Handler<I, O, M> + Send + Sync>> {
        &self.handlers
    }

    pub fn chains(&self) -> &HashMap<ChainId, Vec<HandlerId>> {
        &self.chains
    }
}

#[async_trait]
impl<R, I, O, M, F> Router<R, I, O, M> for DefaultRouter<R, I, O, M, F>
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
        log::trace!("Found matching handler chain for request: {:?}", executables);
        let mut exchange = self.exchange_factory.create_exchange(request).await?;
        log::debug!("Created new exchange {}", exchange.uuid());
        self.execute_executables(&executables, &mut exchange).await?;
        exchange.take_output()
            .map_err(|e| RouterError::ExecutionFailed(e.to_string()))
    }

    async fn execute_chain(&self, chain_id: &ChainId, exchange: &mut Exchange<I, O, M>) -> Result<(), RouterError> {
        let handler_ids = self.chains.get(chain_id)
            .ok_or_else(|| RouterError::ChainNotFound(chain_id.clone()))?;

        for handler_id in handler_ids {
            let handler = self.handlers.get(handler_id)
                .ok_or_else(|| RouterError::HandlerNotFound(handler_id.clone()))?;

            let status = handler.exec(exchange).await
                .map_err(|e| RouterError::ExecutionFailed(e.message.to_string()))?;

            if status.code().all_flags(Code::REQUEST_COMPLETED) {
                return Ok(());
            }

            if status.code().is_error() {
                return Err(RouterError::ExecutionFailed(
                    format!("Handler {:?} in chain {:?} returned error status: {:?}",
                            handler_id, chain_id, status.code())
                ));
            }
        }
        Ok(())
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
                    Err(RouterError::ExchangeCreationFailed("Invalid format".to_string()))
                }
            }

            async fn create_exchange(&self, request: String) -> Result<Exchange<String, String, ()>, RouterError> {
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

