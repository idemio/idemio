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
#[async_trait]
pub trait Router<Request, Response>
where
    Self: Send + Sync,
    Request: Send + Sync,
    Response: Send + Sync,
{
    /// Routes a request through the routing system and returns a response.
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
pub struct RouterBuilder<Request, Exchange, State = Empty> {
    _phantom: PhantomData<(Request, Exchange)>,
    state: State,
}

/// Initial empty state for RouterBuilder.
pub struct Empty;

/// State indicating an exchange factory has been configured.
pub struct WithFactory<Factory> {
    factory: Factory,
}

/// State indicating both factory and executor have been configured.
pub struct WithExecutor<Factory, Executor> {
    factory: Factory,
    executor: Executor,
}

/// Final state indicating all required components have been configured.
pub struct Complete<Factory, Executor, Matcher> {
    factory: Factory,
    executor: Executor,
    matcher: Matcher,
}

/// A concrete router implementation that processes requests through configured components.
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
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
            state: Empty,
        }
    }

    /// Adds an exchange factory to the builder and transitions to WithFactory state.
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
            .ok_or_else(|| todo!("Handle missing route error"))?;

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