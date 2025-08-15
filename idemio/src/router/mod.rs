pub mod config;
pub mod executor;
pub mod factory;
pub mod path;

use crate::handler::registry::HandlerRegistry;
use crate::router::config::RouterConfig;
use crate::router::executor::{ExecutorError, HandlerExecutor};
use crate::router::factory::{ExchangeFactory, ExchangeFactoryError};
use crate::router::path::{PathMatcherError, PathMatcherTrait};
use async_trait::async_trait;
use std::marker::PhantomData;
use std::sync::Arc;
use thiserror::Error;

#[async_trait]
pub trait Router<Req, Out>
where
    Self: Send + Sync,
    Req: Send + Sync,
    Out: Send + Sync,
{
    async fn route(&self, request: Req) -> Result<Out, RouterError>;
}

pub trait RouterComponents<Key, Req, In, Out, Meta>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    type PathMatcher: PathMatcherTrait<Key, In, Out, Meta> + Send + Sync;
    /// Factory for creating exchanges from requests
    type Factory: ExchangeFactory<Key, Req, In, Out, Meta> + Send + Sync;
    /// Executor for running handler chains
    type Executor: HandlerExecutor<In, Out, Meta> + Send + Sync;
}

// TODO - Implement other routers for different path matching configurations (header match, tcp/ip match, etc.)
pub struct RequestRouter<Key, Req, In, Out, Meta, Components>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Components: RouterComponents<Key, Req, In, Out, Meta>,
{
    /// PhantomData to maintain type information for the request type
    phantom: PhantomData<Req>,
    /// Factory for creating exchanges from requests
    factory: Arc<Components::Factory>,
    /// Executor for running handler chains
    executor: Arc<Components::Executor>,
    /// Path matcher for routing requests to handler chains
    matcher: Arc<Components::PathMatcher>,
}

impl<Key, Req, In, Out, Meta, Components> RequestRouter<Key, Req, In, Out, Meta, Components>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Components: RouterComponents<Key, Req, In, Out, Meta>,
{

    pub fn new(
        registry: &HandlerRegistry<In, Out, Meta>,
        config: &RouterConfig,
        matcher: Components::PathMatcher,
        exchange_factory: Components::Factory,
        handler_executor: Components::Executor,
    ) -> Result<Self, RouterError> {
//        let matcher = match PathPrefixMethodPathMatcher::new(config, registry) {
//            Ok(table) => table,
//            Err(e) => {
//                return Err(RouterError::path_matcher_error(e));
//            }
//        };
        Ok(Self {
            phantom: PhantomData::default(),
            factory: Arc::new(exchange_factory),
            executor: Arc::new(handler_executor),
            matcher: Arc::new(matcher),
        })
    }
}

#[async_trait]
impl<Key, Req, In, Out, Meta, Components> Router<Req, Out>
for RequestRouter<Key, Req, In, Out, Meta, Components>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Components: RouterComponents<Key, Req, In, Out, Meta>,
{
    async fn route(&self, request: Req) -> Result<Out, RouterError> {
        let key = self
            .factory
            .extract_route_info(&request)
            .await
            .map_err(|e| RouterError::invalid_exchange(e))?;

        //log::trace!("Extracted route info from request: {}@{}", &path, &method);

        let executables = self
            .matcher
            .lookup(key)
            .ok_or(RouterError::missing_route("TODO"))?;

        //log::trace!("Found handler chain for request: {}@{}", &path, &method);

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
    use crate::router::path::PathPrefixMethodKey;

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
        impl ExchangeFactory<PathPrefixMethodKey, String, String, String, ()> for CustomExchangeFactory {
            async fn extract_route_info(
                &self,
                request: &String,
            ) -> Result<PathPrefixMethodKey, ExchangeFactoryError> {
                let mut parts = request.split(':');
                if let (Some(method), Some(path), Some(_)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    Ok(PathPrefixMethodKey::new(method, path))
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

        let key = factory.extract_route_info(&custom_request).await.unwrap();
        assert_eq!(key.method, "GET");
        assert_eq!(key.path, "/test");

        let mut exchange = factory.create_exchange(custom_request).await.unwrap();
        assert_eq!(exchange.input().await.unwrap(), "body_content");
    }
}