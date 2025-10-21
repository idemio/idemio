pub mod http;

use crate::handler::registry::{HandlerRegistry, HandlerRegistryError};
use crate::handler::{Handler, HandlerId};
use crate::router::config::{PathChain, RouterConfig};
use std::sync::Arc;
use thiserror::Error;
use crate::router::factory::RouteInfo;

/// Errors that can occur during PathMatcher construction and operation.
#[derive(Error, Debug)]
pub enum PathMatcherError {
    #[error("Invalid path configuration. {message}")]
    InvalidConfiguration { message: String },

    #[error("Path '{path}' is invalid.")]
    InvalidPath { path: String },

    #[error("Method '{method}' is invalid.")]
    InvalidMethod { method: String },

    #[error("Failed to register handler.")]
    HandlerRegistryError {
        #[source]
        source: HandlerRegistryError,
    },
}

impl PathMatcherError {
    //    #[inline]
    //    pub(crate) fn invalid_path(path: impl Into<String>) -> Self {
    //        Self::InvalidPath { path: path.into() }
    //    }

    #[inline]
    pub(crate) fn invalid_configuration(message: impl Into<String>) -> Self {
        Self::InvalidConfiguration {
            message: message.into(),
        }
    }

    #[inline]
    pub(crate) fn invalid_method(method: impl Into<String>) -> Self {
        Self::InvalidMethod {
            method: method.into(),
        }
    }

    #[inline]
    pub(crate) const fn registry_error(registry_error: HandlerRegistryError) -> Self {
        Self::HandlerRegistryError {
            source: registry_error,
        }
    }
}

/// A collection of handlers that are ready to be executed in a specific order.
///
/// # Behavior
/// `LoadedChain` represents a complete request processing pipeline consisting of:
/// - **Request handlers**: Process incoming data before the main handler (authentication, validation, etc.)
/// - **Termination handler**: The main handler that processes the request and generates output
/// - **Response handlers**: Process outgoing data after the main handler (serialization, logging, etc.)
///
/// All handlers are wrapped in `Arc<dyn Handler<E>>` to ensure they can be safely
/// shared across multiple threads and async tasks.
pub struct LoadedChain<E>
where
    E: Send + Sync,
{
    request_handlers: Vec<Arc<dyn Handler<E>>>,
    termination_handler: Arc<dyn Handler<E>>,
    response_handlers: Vec<Arc<dyn Handler<E>>>,
}

impl<E> LoadedChain<E>
where
    E: Send + Sync,
{
    /// Creates a new `LoadedChain` with the specified handlers.
    ///
    /// # Parameters
    /// - `request_handlers`: Vector of handlers to execute before the termination handler.
    ///   These typically handle authentication, authorization, validation, rate limiting, etc.
    /// - `termination_handler`: The main handler that processes the request and generates output.
    ///   This is the core business logic handler that must always be present.
    /// - `response_handlers`: Vector of handlers to execute after the termination handler.
    ///   These typically handle response transformation, logging, metrics collection, etc.
    pub(crate) fn new(
        request_handlers: Vec<Arc<dyn Handler<E>>>,
        termination_handler: Arc<dyn Handler<E>>,
        response_handlers: Vec<Arc<dyn Handler<E>>>,
    ) -> Self {
        Self {
            request_handlers,
            termination_handler,
            response_handlers,
        }
    }

    /// Returns the total number of handlers in this chain.
    ///
    /// # Behavior
    /// Calculates the size using the formula: `request_handlers.len() + 1 + response_handlers.len()`.
    pub fn size(&self) -> usize {
        self.request_handlers.len() + 1 + self.response_handlers.len()
    }

    /// Returns a reference to the request handlers vector.
    pub fn request_handlers(&self) -> &Vec<Arc<dyn Handler<E>>> {
        &self.request_handlers
    }

    /// Returns a reference to the termination handler.
    pub fn termination_handler(&self) -> &Arc<dyn Handler<E>> {
        &self.termination_handler
    }

    /// Returns a reference to the response handlers vector.
    pub fn response_handlers(&self) -> &Vec<Arc<dyn Handler<E>>> {
        &self.response_handlers
    }
}

/// A trait for matching URL paths to handler chains in the routing system.
pub trait PathMatcher<Exchange>
where
    Exchange: Send + Sync,
{
    /// Parses router configuration and populates the matcher with routes.
    fn parse_config(
        &mut self,
        route_config: &RouterConfig,
        handler_registry: &HandlerRegistry<Exchange>,
    ) -> Result<(), PathMatcherError>;

    /// Looks up a handler chain for the given path and method combination.
    fn lookup(&self, key: RouteInfo<'_>) -> Option<Arc<LoadedChain<Exchange>>>;

    /// Creates a new PathMatcher instance from configuration and handler registry.
    fn new(
        config: &RouterConfig,
        handler_registry: &HandlerRegistry<Exchange>,
    ) -> Result<Self, PathMatcherError>
    where
        Self: Sized;

    /// Finds a single handler in the registry by name.
    fn find_in_registry(
        handler: &str,
        handler_registry: &HandlerRegistry<Exchange>,
    ) -> Result<Arc<dyn Handler<Exchange>>, PathMatcherError> {
        let handler_id = HandlerId::new(handler);
        match handler_registry.find_with_id(&handler_id) {
            Ok(handler) => Ok(handler),
            Err(e) => Err(PathMatcherError::registry_error(e)),
        }
    }

    /// Finds multiple handlers in the registry by their names.
    fn find_all_in_registry(
        handlers: &[String],
        handler_registry: &HandlerRegistry<Exchange>,
    ) -> Result<Vec<Arc<dyn Handler<Exchange>>>, PathMatcherError> {
        let mut registered_handlers = vec![];
        for handler in handlers {
            let registered_handler = Self::find_in_registry(handler, handler_registry)?;
            registered_handlers.push(registered_handler);
        }
        Ok(registered_handlers)
    }

    /// Loads handlers from the registry and creates a complete handler chain.
    fn load_handlers(
        handler_registry: &HandlerRegistry<Exchange>,
        path_chain: &PathChain,
    ) -> Result<LoadedChain<Exchange>, PathMatcherError> {
        let registered_request_handlers = match &path_chain.request_handlers {
            Some(handlers) => Self::find_all_in_registry(handlers, handler_registry)?,
            None => vec![],
        };
        let registered_termination_handler = match &path_chain.termination_handler {
            Some(handler) => Self::find_in_registry(handler, handler_registry)?,
            None => {
                return Err(PathMatcherError::invalid_method(
                    "No termination handler specified",
                ));
            }
        };
        let registered_response_handlers = match &path_chain.response_handlers {
            Some(handlers) => Self::find_all_in_registry(handlers, handler_registry)?,
            None => vec![],
        };
        Ok(LoadedChain::new(
            registered_request_handlers,
            registered_termination_handler,
            registered_response_handlers,
        ))
    }
}
