pub mod config;
pub mod executor;
pub mod factory;
pub mod route;

use crate::handler::registry::HandlerRegistry;
use crate::router::config::RouterConfig;
use crate::router::executor::{ExecutorError, HandlerExecutor};
use crate::router::factory::{ExchangeFactory, ExchangeFactoryError};
use crate::router::route::{PathMatcher, PathMatcherError};
use async_trait::async_trait;
use std::marker::PhantomData;
use std::sync::Arc;
use thiserror::Error;

/// Main router trait that processes requests through handler chains
///
/// The router is responsible for matching incoming requests to configured routes
/// and executing the appropriate handler chains. It acts as the central dispatch
/// mechanism for the framework.
///
/// # Type Parameters
///
/// * `Req` - The request type that will be routed
/// * `In` - The input type for handlers in the chain
/// * `Out` - The output type produced by handlers
/// * `Meta` - Metadata type associated with the exchange
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
    ///
    /// This method performs the core routing functionality:
    /// 1. Extracts routing information from the request
    /// 2. Matches the request to a configured route
    /// 3. Creates an exchange object for handler execution
    /// 4. Executes the matched handler chain
    /// 5. Returns the final output
    ///
    /// # Arguments
    ///
    /// * `request` - The incoming request to be routed
    ///
    /// # Returns
    ///
    /// * `Ok(Out)` - The output produced by the handler chain
    /// * `Err(RouterError)` - An error if routing or execution fails
    async fn route(&self, request: Req) -> Result<Out, RouterError>;
}

/// Trait defining the components required for router functionality
///
/// This trait allows customization of the router's core components:
/// the exchange factory and the handler executor. Different implementations
/// can provide specialized behavior for different types of requests and responses.
///
/// # Type Parameters
///
/// * `Req` - The request type
/// * `In` - The input type for handlers
/// * `Out` - The output type produced by handlers
/// * `Meta` - Metadata type for exchanges
pub trait RouterComponents<Req, In, Out, Meta>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Factory for creating exchanges from requests
    type Factory: ExchangeFactory<Req, In, Out, Meta> + Send + Sync;
    /// Executor for running handler chains
    type Executor: HandlerExecutor<In, Out, Meta> + Send + Sync;
}

// TODO - Implement other routers for different path matching configurations (header match, tcp/ip match, etc.)

/// Default implementation of the Router trait
///
/// `RequestRouter` is a concrete implementation that uses configurable components
/// for exchange creation and handler execution. It supports path-based routing
/// and can be customized through the `RouterComponents` trait.
///
/// # Type Parameters
///
/// * `Req` - The request type to be routed
/// * `In` - The input type for handler chains
/// * `Out` - The output type produced by handlers
/// * `Meta` - Metadata type for exchanges
/// * `Components` - The router components providing factory and executor implementations
///
/// # Example
///
/// ```rust,ignore
/// use crate::router::{RequestRouter, RouterComponents};
/// use crate::handler::registry::HandlerRegistry;
/// use crate::router::config::RouterConfig;
///
/// // Assuming you have implementations of RouterComponents
/// struct MyComponents;
/// impl RouterComponents<MyRequest, String, String, ()> for MyComponents {
///     type Factory = MyExchangeFactory;
///     type Executor = MyHandlerExecutor;
/// }
///
/// let router = RequestRouter::<MyRequest, String, String, (), MyComponents>::new(
///     &registry,
///     &config,
///     factory,
///     executor
/// )?;
/// ```
pub struct RequestRouter<Req, In, Out, Meta, Components>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Components: RouterComponents<Req, In, Out, Meta>,
{
    /// PhantomData to maintain type information for the request type
    phantom: PhantomData<Req>,
    /// Factory for creating exchanges from requests
    factory: Arc<Components::Factory>,
    /// Executor for running handler chains
    executor: Arc<Components::Executor>,
    /// Path matcher for routing requests to handler chains
    matcher: PathMatcher<In, Out, Meta>,
}

impl<Req, In, Out, Meta, Components> RequestRouter<Req, In, Out, Meta, Components>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Components: RouterComponents<Req, In, Out, Meta>,
{
    /// Create a new RequestRouter instance
    ///
    /// # Arguments
    ///
    /// * `registry` - Handler registry containing all available handlers
    /// * `config` - Router configuration specifying routes and handler chains
    /// * `exchange_factory` - Factory for creating exchanges from requests
    /// * `handler_executor` - Executor for running handler chains
    ///
    /// # Returns
    ///
    /// * `Ok(RequestRouter)` - A new router instance ready for use
    /// * `Err(RouterError)` - An error if router initialization fails
    ///
    /// # Errors
    ///
    /// This method can fail if:
    /// * The path matcher cannot be built from the provided configuration
    /// * The configuration contains invalid route definitions
    pub fn new(
        registry: &HandlerRegistry<In, Out, Meta>,
        config: &RouterConfig,
        exchange_factory: Components::Factory,
        handler_executor: Components::Executor,
    ) -> Result<Self, RouterError> {
        let matcher = match PathMatcher::new(config, registry) {
            Ok(table) => table,
            Err(e) => {
                return Err(RouterError::path_matcher_error(e));
            }
        };
        Ok(Self {
            phantom: PhantomData::default(),
            factory: Arc::new(exchange_factory),
            executor: Arc::new(handler_executor),
            matcher,
        })
    }
}

#[async_trait]
impl<Req, In, Out, Meta, Components> Router<Req, In, Out, Meta>
for RequestRouter<Req, In, Out, Meta, Components>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Components: RouterComponents<Req, In, Out, Meta>,
{
    /// Process a request through the router
    ///
    /// This implementation follows these steps:
    /// 1. Extract routing information (path and method) from the request
    /// 2. Look up the appropriate handler chain using path matcher
    /// 3. Create an exchange object from the request
    /// 4. Execute the handler chain with exchange
    /// 5. Return the final output
    ///
    /// # Arguments
    ///
    /// * `request` - The incoming request to route
    ///
    /// # Returns
    ///
    /// * `Ok(Out)` - The output produced by the handler chain
    /// * `Err(RouterError)` - Various routing and execution errors
    ///
    /// # Errors
    ///
    /// This method can return errors for:
    /// * `RouterError::InvalidExchange` - Request could not be processed into an exchange
    /// * `RouterError::MissingRoute` - No handler chain found for the request path/method
    /// * `RouterError::ExecutionFailure` - Handler chain execution failed
    async fn route(&self, request: Req) -> Result<Out, RouterError> {
        let (path, method) = self
            .factory
            .extract_route_info(&request)
            .await
            .map_err(|e| RouterError::invalid_exchange(e))?;

        log::trace!("Extracted route info from request: {}@{}", &path, &method);

        let executables = self
            .matcher
            .lookup(path, method)
            .ok_or(RouterError::missing_route(path))?;

        log::trace!("Found handler chain for request: {}@{}", &path, &method);

        let mut exchange = self
            .factory
            .create_exchange(request)
            .await
            .map_err(|e| RouterError::invalid_exchange(e))?;

        let result = self
            .executor
            .execute_handlers(executables, &mut exchange)
            .await
            .map_err(|e| RouterError::execution_failure(e))?;
        Ok(result)
    }
}

/// Errors that can occur during router operations
///
/// This enum represents all the possible error conditions that can arise
/// when routing requests through the system.
#[derive(Error, Debug)]
pub enum RouterError {
    /// The requested route was not found in the configuration
    ///
    /// This occurs when a request is made to a path/method combination
    /// that has no configured handler chain.
    #[error("Route '{route}' was not found.")]
    MissingRoute { route: String },

    /// An error occurred while executing the handler chain
    ///
    /// This wraps errors from the handler executor, such as handler
    /// failures or exchange processing errors.
    #[error("Error while executing handlers.")]
    ExecutionFailure {
        #[source]
        source: ExecutorError,
    },

    /// An error occurred while creating an exchange from the request
    ///
    /// This wraps errors from the exchange factory, typically indicating
    /// problems with request parsing or exchange initialization.
    #[error("Error while creating a new exchange.")]
    InvalidExchange {
        #[source]
        source: ExchangeFactoryError,
    },

    /// An error occurred while building the path matcher
    ///
    /// This typically happens during router initialization when the
    /// configuration contains invalid route definitions.
    #[error("Error while building path matcher.")]
    PathMatcherError {
        #[source]
        source: PathMatcherError,
    },
}

impl RouterError {
    /// Create a new `MissingRoute` error
    ///
    /// # Arguments
    ///
    /// * `route` - The route path that was not found
    #[inline]
    pub fn missing_route(route: impl Into<String>) -> Self {
        RouterError::MissingRoute {
            route: route.into(),
        }
    }

    /// Create a new `PathMatcherError` error
    ///
    /// # Arguments
    ///
    /// * `err` - The underlying path matcher error
    #[inline]
    pub const fn path_matcher_error(err: PathMatcherError) -> Self {
        RouterError::PathMatcherError { source: err }
    }

    /// Create a new `ExecutionFailure` error
    ///
    /// # Arguments
    ///
    /// * `err` - The underlying executor error
    #[inline]
    pub const fn execution_failure(err: ExecutorError) -> Self {
        RouterError::ExecutionFailure { source: err }
    }

    /// Create a new `InvalidExchange` error
    ///
    /// # Arguments
    ///
    /// * `err` - The underlying exchange factory error
    #[inline]
    pub const fn invalid_exchange(err: ExchangeFactoryError) -> Self {
        RouterError::InvalidExchange { source: err }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Exchange;
    use async_trait::async_trait;

    /// Test demonstrating custom exchange factory implementation
    ///
    /// This test shows how to create a custom exchange factory that can parse
    /// requests in a specific format and create appropriate exchanges.
    #[tokio::test]
    async fn test_custom_exchange_factory() {
        /// Custom exchange factory for string-based requests
        ///
        /// This factory expects requests in the format: "METHOD:PATH:BODY,"
        /// For example, "GET:/endpoint:body text"
        struct CustomExchangeFactory;

        #[async_trait]
        impl ExchangeFactory<String, String, String, ()> for CustomExchangeFactory {
            async fn extract_route_info<'a>(
                &self,
                request: &'a String,
            ) -> Result<(&'a str, &'a str), ExchangeFactoryError> {
                let mut parts = request.split(':');
                if let (Some(method), Some(path), Some(_)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    Ok((path, method))
                } else {
                    Err(ExchangeFactoryError::invalid_exchange("Invalid format"))
                }
            }

            async fn create_exchange<'a>(
                &self,
                request: String,
            ) -> Result<Exchange<'a, String, String, ()>, ExchangeFactoryError> {
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