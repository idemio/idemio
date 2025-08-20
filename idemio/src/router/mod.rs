pub mod config;
pub mod executor;
pub mod factory;
pub mod path;

use crate::router::executor::{ExecutorError, HandlerExecutor};
use crate::router::factory::{ExchangeFactory, ExchangeFactoryError};
use crate::router::path::{PathMatcher, PathMatcherError};
use async_trait::async_trait;
use std::marker::PhantomData;
use thiserror::Error;

/// A trait for routing requests to appropriate handlers and returning responses.
///
/// # Type Parameters
/// - `Request`: Input request type that must implement `Send + Sync`
/// - `Response`: Output response type that must implement `Send + Sync`
///
/// # Behavior
/// Provides an asynchronous interface for processing requests through a routing system.
/// Implementations should handle request matching, handler execution, and response generation.
#[async_trait]
pub trait Router<Request, Response>
where
    Self: Send + Sync,
    Request: Send + Sync,
    Response: Send + Sync,
{
    /// Routes a request through the routing system and returns a response.
    ///
    /// # Parameters
    /// - `request`: The incoming request to be processed
    ///
    /// # Returns
    /// `Result<Response, RouterError>` where:
    /// - `Ok(Response)` contains the processed response
    /// - `Err(RouterError)` if routing, matching, or execution fails
    ///
    /// # Errors
    /// `RouterError` variants for different failure scenarios:
    /// - `MissingRoute` if no matching route is found
    /// - `ExecutionFailure` if handler execution fails
    /// - `InvalidExchange` if exchange creation fails
    /// - `PathMatcherError` if path matching fails
    ///
    /// # Behavior
    /// Implementations should extract routing information from the request,
    /// find appropriate handlers, execute them, and return the result.
    async fn route(&self, request: Request) -> Result<Response, RouterError>;
}

// Original RouterError
#[derive(Error, Debug)]
pub enum RouterError {
    #[error("Matching route for key ({key1} -- {key2}) was not found.")]
    MissingRoute { key1: String, key2: String },
    #[error("Error while executing handlers.")]
    ExecutionFailure {
        #[source]
        source: ExecutorError,
    },
    #[error("Error while creating a new exchange. {message}")]
    InvalidExchange {
        message: String,
        #[source]
        source: ExchangeFactoryError,
    },
    #[error("Error while building path matcher.")]
    PathMatcherError {
        #[source]
        source: PathMatcherError,
    },
}

impl RouterError {

    #[inline]
    pub fn missing_route(key1: impl Into<String>, key2: impl Into<String>) -> Self {
        RouterError::MissingRoute {
            key1: key1.into(),
            key2: key2.into(),
        }
    }

    #[inline]
    pub const fn path_matcher_error(err: PathMatcherError) -> Self {
        RouterError::PathMatcherError { source: err }
    }

    #[inline]
    pub const fn execution_failure(err: ExecutorError) -> Self {
        RouterError::ExecutionFailure { source: err }
    }

    #[inline]
    pub fn invalid_exchange(msg: impl Into<String>, err: ExchangeFactoryError) -> Self {
        RouterError::InvalidExchange {
            message: msg.into(),
            source: err,
        }
    }
}

/// A builder for constructing Router instances using the type state pattern.
///
/// # Type Parameters
/// - `Request`: The request type that will be processed by the router
/// - `Exchange`: The exchange type used for internal request/response handling
/// - `State`: The current build state, defaults to `Empty`
///
/// # Behavior
/// Uses the type state pattern to ensure all required components (factory, executor, matcher)
/// are provided before a router can be built. Each method transitions to the next state,
/// preventing invalid configurations and ensuring compile-time correctness.
pub struct RouterBuilder<Request, Exchange, State = Empty> {
    _phantom: PhantomData<(Request, Exchange)>,
    state: State,
}

// Type states
/// Initial empty state for RouterBuilder.
///
/// # Behavior
/// Represents the starting state where no components have been configured yet.
/// The next required step is to add a factory component.
pub struct Empty;

/// State indicating an exchange factory has been configured.
///
/// # Type Parameters
/// - `Factory`: The factory type that implements `ExchangeFactory`
///
/// # Behavior
/// Represents the state after a factory has been added. The next required
/// component is an executor.
pub struct WithFactory<Factory> {
    factory: Factory,
}

/// State indicating both factory and executor have been configured.
///
/// # Type Parameters
/// - `Factory`: The factory type that implements `ExchangeFactory`
/// - `Executor`: The executor type that implements `HandlerExecutor`
///
/// # Behavior
/// Represents the state after factory and executor have been added. The final
/// required component is a path matcher.
pub struct WithExecutor<Factory, Executor> {
    factory: Factory,
    executor: Executor,
}

/// Final state indicating all required components have been configured.
///
/// # Type Parameters
/// - `Factory`: The factory type that implements `ExchangeFactory`
/// - `Executor`: The executor type that implements `HandlerExecutor`
/// - `Matcher`: The matcher type that implements `PathMatcher`
///
/// # Behavior
/// Represents the complete state where all components are available and
/// a router can be built.
pub struct Complete<Factory, Executor, Matcher> {
    factory: Factory,
    executor: Executor,
    matcher: Matcher,
}

/// A concrete router implementation that processes requests through configured components.
///
/// # Type Parameters
/// - `Request`: The request type to be processed
/// - `Exchange`: The exchange type used for internal processing
/// - `Factory`: The exchange factory type
/// - `Executor`: The handler executor type
/// - `Matcher`: The path matcher type
///
/// # Behavior
/// Coordinates between factory, executor, and matcher components to process requests:
/// - Uses factory to extract routing information and create exchanges
/// - Uses matcher to find appropriate handler chains
/// - Uses executor to run handlers and produce responses
pub struct RequestRouter<Request, Exchange, Factory, Executor, Matcher>
where
    Request: Send + Sync,
    Exchange: Send + Sync,
    Factory: ExchangeFactory<Request, Exchange> + Send + Sync,
    Executor: HandlerExecutor<Exchange> + Send + Sync,
    Matcher: PathMatcher<Exchange> + Send + Sync,
{
    factory: Factory,
    executor: Executor,
    matcher: Matcher,
    _phantom: PhantomData<(Request, Exchange)>,
}

// Empty state
impl<Request, Exchange> RouterBuilder<Request, Exchange, Empty>
where
    Request: Send + Sync,
    Exchange: Send + Sync,
{
    /// Creates a new RouterBuilder in the empty state.
    ///
    /// # Returns
    /// A new `RouterBuilder` instance ready to be configured with components.
    ///
    /// # Behavior
    /// Initializes the builder with phantom data for type tracking and empty state.
    /// The builder must be configured with factory, executor, and matcher before
    /// it can build a router.
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
            state: Empty,
        }
    }

    /// Adds an exchange factory to the builder and transitions to WithFactory state.
    ///
    /// # Parameters
    /// - `factory`: An implementation of `ExchangeFactory` that creates exchanges from requests
    ///
    /// # Returns
    /// A `RouterBuilder` in the `WithFactory` state with the provided factory.
    ///
    /// # Behavior
    /// Consumes the current builder and returns a new one with the factory configured.
    /// The factory will be used to extract routing information and create exchanges
    /// during request processing.
    pub fn factory<F>(self, factory: F) -> RouterBuilder<Request, Exchange, WithFactory<F>>
    where
        F: ExchangeFactory<Request, Exchange> + Send + Sync,
    {
        RouterBuilder {
            _phantom: PhantomData,
            state: WithFactory { factory },
        }
    }
}

// With factory state
impl<Request, Exchange, Factory> RouterBuilder<Request, Exchange, WithFactory<Factory>>
where
    Request: Send + Sync,
    Exchange: Send + Sync,
    Factory: ExchangeFactory<Request, Exchange> + Send + Sync,
{
    /// Adds a handler executor to the builder and transitions to WithExecutor state.
    ///
    /// # Parameters
    /// - `executor`: An implementation of `HandlerExecutor` that executes handler chains
    ///
    /// # Returns
    /// A `RouterBuilder` in the `WithExecutor` state with both factory and executor configured.
    ///
    /// # Behavior
    /// Consumes the current builder and returns a new one with the executor configured
    /// alongside the previously added factory. The executor will be responsible for
    /// running handler chains on exchanges.
    pub fn executor<Executor>(self, executor: Executor) -> RouterBuilder<Request, Exchange, WithExecutor<Factory, Executor>>
    where
        Executor: HandlerExecutor<Exchange> + Send + Sync,
    {
        RouterBuilder {
            _phantom: PhantomData,
            state: WithExecutor {
                factory: self.state.factory,
                executor,
            },
        }
    }
}

// With executor state
impl<Request, Exchange, Factory, Executor> RouterBuilder<Request, Exchange, WithExecutor<Factory, Executor>>
where
    Request: Send + Sync,
    Exchange: Send + Sync,
    Factory: ExchangeFactory<Request, Exchange> + Send + Sync,
    Executor: HandlerExecutor<Exchange> + Send + Sync,
{
    /// Adds a path matcher to the builder and transitions to Complete state.
    ///
    /// # Parameters
    /// - `matcher`: An implementation of `PathMatcher` that matches routes to handler chains
    ///
    /// # Returns
    /// A `RouterBuilder` in the `Complete` state with all required components configured.
    ///
    /// # Behavior
    /// Consumes the current builder and returns a new one with the matcher configured
    /// alongside the previously added factory and executor. The matcher will be used
    /// to find appropriate handler chains for incoming requests.
    pub fn matcher<Matcher>(self, matcher: Matcher) -> RouterBuilder<Request, Exchange, Complete<Factory, Executor, Matcher>>
    where
        Matcher: PathMatcher<Exchange> + Send + Sync,
    {
        RouterBuilder {
            _phantom: PhantomData,
            state: Complete {
                factory: self.state.factory,
                executor: self.state.executor,
                matcher,
            },
        }
    }
}

// Complete state - ready to build
impl<Request, Exchange, Factory, Executor, Matcher> RouterBuilder<Request, Exchange, Complete<Factory, Executor, Matcher>>
where
    Request: Send + Sync,
    Exchange: Send + Sync,
    Factory: ExchangeFactory<Request, Exchange> + Send + Sync,
    Executor: HandlerExecutor<Exchange> + Send + Sync,
    Matcher: PathMatcher<Exchange> + Send + Sync,
{
    /// Builds and returns a configured RequestRouter instance.
    ///
    /// # Returns
    /// A `RequestRouter` configured with all provided components wrapped in Arc for sharing.
    ///
    /// # Behavior
    /// Consumes the complete builder and creates a RequestRouter with all components
    /// wrapped in Arc references for efficient sharing across async tasks. The router
    /// is ready to process requests through the configured pipeline.
    pub fn build(self) -> RequestRouter<Request, Exchange, Factory, Executor, Matcher> {
        RequestRouter {
            factory: self.state.factory,
            executor: self.state.executor,
            matcher: self.state.matcher,
            _phantom: PhantomData,
        }
    }
}

// Router trait implementation
#[async_trait]
impl<Request, Exchange, Factory, Executor, Matcher> Router<Request, Executor::Output>
for RequestRouter<Request, Exchange, Factory, Executor, Matcher>
where
    Request: Send + Sync,
    Exchange: Send + Sync,
    Factory: ExchangeFactory<Request, Exchange> + Send + Sync,
    Executor: HandlerExecutor<Exchange> + Send + Sync,
    Executor::Output: Send + Sync,
    Matcher: PathMatcher<Exchange> + Send + Sync,
{
    /// Routes a request through the configured pipeline and returns the result.
    ///
    /// # Parameters
    /// - `request`: The incoming request to be processed
    ///
    /// # Returns
    /// `Result<Executor::Output, RouterError>` where:
    /// - `Ok(Executor::Output)` contains the processed response from the handler chain
    /// - `Err(RouterError)` if any step in the routing process fails
    ///
    /// # Errors
    /// `RouterError::InvalidExchange` if:
    /// - Route information extraction from the request fails
    /// - Exchange creation from the request fails
    ///
    /// Returns `RouterError::MissingRoute` if no handler chain matches the extracted route key.
    ///
    /// Returns `RouterError::ExecutionFailure` if the handler chain execution fails.
    ///
    /// # Behavior
    /// Implements the complete request routing pipeline:
    /// 1. Extracts routing information from the request using factory
    /// 2. It looks up the appropriate handler chain using matcher
    /// 3. Creates an exchange from the request using factory
    /// 4. Executes the handler chain on the exchange using executor
    /// 5. Returns the final result from the executor
    async fn route(&self, request: Request) -> Result<Executor::Output, crate::router::RouterError> {
        let route_key = self
            .factory
            .extract_route_info(&request)
            .await
            .map_err(|e| {
                crate::router::RouterError::invalid_exchange(
                    "Failed to extract route info from request",
                    e,
                )
            })?;

        let handler_chain = self
            .matcher
            .lookup(route_key)
            .ok_or_else(|| crate::router::RouterError::missing_route(route_key.0, route_key.1))?;

        let mut exchange = self.factory.create_exchange(request).await.map_err(|e| {
            crate::router::RouterError::invalid_exchange(
                "Failed to create exchange from request",
                e,
            )
        })?;

        let result = self
            .executor
            .execute_handlers(handler_chain, &mut exchange)
            .await
            .map_err(crate::router::RouterError::execution_failure)?;

        Ok(result)
    }
}