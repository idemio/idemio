#[cfg(feature = "http")]
pub mod http;

use crate::handler::{HandlerRegistry, HandlerRegistryError};
use crate::handler::{MiddlewareHandler, HandlerId, TerminationHandler};
use crate::router::config::{PathChain, RouterConfig};
use crate::router::route::RouteKey;
use std::sync::Arc;
use thiserror::Error;

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

pub type MiddlewareChain<T> = Vec<Arc<dyn MiddlewareHandler<T>>>;
pub type TerminationPoint<I, O> = Arc<dyn TerminationHandler<I, O>>;
/// A collection of handlers that are ready to be executed in a specific order.
///
/// # Behavior
/// `LoadedChain` represents a complete request processing pipeline consisting of:
/// - **Request handlers**: Process incoming data before the main handler (authentication, validation, etc.)
/// - **Termination handler**: The main handler that processes the request and generates output
/// - **Response handlers**: Process outgoing data after the main handler (serialization, logging, etc.)
///
/// All handlers are wrapped in `Arc<dyn Handler<Exchange<I, O>>>` to ensure they can be safely
/// shared across multiple threads and async tasks.
pub struct LoadedChain<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{
    request_handlers: MiddlewareChain<I>,
    termination_handler: TerminationPoint<I, O>,
    response_handlers: MiddlewareChain<O>,
}

impl<I, O> LoadedChain<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
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
    pub fn new(
        request_handlers: MiddlewareChain<I>,
        termination_handler: TerminationPoint<I, O>,
        response_handlers: MiddlewareChain<O>,
    ) -> Self {
        Self {
            request_handlers,
            termination_handler,
            response_handlers,
        }
    }

    /// Returns the total number of handlers in this chain.
    pub fn size(&self) -> usize {
        self.request_handlers.len() + 1 + self.response_handlers.len()
    }

    /// Returns a reference to the request handlers vector.
    pub fn request_handlers(&self) -> &MiddlewareChain<I> {
        &self.request_handlers
    }

    /// Returns a reference to the termination handler.
    pub fn termination_handler(&self) -> &TerminationPoint<I, O> {
        &self.termination_handler
    }

    /// Returns a reference to the response handlers vector.
    pub fn response_handlers(&self) -> &MiddlewareChain<O> {
        &self.response_handlers
    }
}

/// A trait for matching URL paths to handler chains in the routing system.
pub trait RouteKeyMatcher<I, O>
where
    I: Send + Sync,
    O: Send + Sync
{
    /// Parses router configuration and populates the matcher with routes.
    fn parse_config(
        &mut self,
        route_config: &RouterConfig,
        handler_registry: &HandlerRegistry<I, O>,
    ) -> Result<(), PathMatcherError>;

    /// Looks up a handler chain for the given path and method combination.
    fn lookup(&self, key: RouteKey<'_>) -> Option<Arc<LoadedChain<I, O>>>;

    /// Creates a new PathMatcher instance from configuration and handler registry.
    fn new(
        config: &RouterConfig,
        handler_registry: &HandlerRegistry<I, O>,
    ) -> Result<Self, PathMatcherError>
    where
        Self: Sized;

    /// Loads handlers from the registry and creates a complete handler chain.
    fn load_handlers(
        handler_registry: &HandlerRegistry<I, O>,
        path_chain: &PathChain,
    ) -> Result<LoadedChain<I, O>, PathMatcherError> {
        let mut loaded_request_handlers = Vec::new();
        if let Some(request_handlers) = &path_chain.request_handlers {
            for handler in request_handlers {
                loaded_request_handlers.push(
                    handler_registry
                        .get_request_handler(&HandlerId::new(handler))
                        .map_err(|e| PathMatcherError::registry_error(e))?,
                );
            }
        }
        let termination_handler = handler_registry
            .get_termination_handler(&HandlerId::new(&path_chain.termination_handler))
            .map_err(|e| PathMatcherError::registry_error(e))?;
        let mut loaded_response_handlers = Vec::new();
        if let Some(response_handlers) = &path_chain.response_handlers {
            for handler in response_handlers {
                loaded_response_handlers.push(
                    handler_registry
                        .get_response_handler(&HandlerId::new(handler))
                        .map_err(|e| PathMatcherError::registry_error(e))?,
                );
            }
        }
        Ok(LoadedChain::new(
            loaded_request_handlers,
            termination_handler,
            loaded_response_handlers,
        ))
    }
}
