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

/// A concrete router implementation that processes requests through configured components.
pub struct RequestRouter<Request, Exchange, Factory, Executor, Matcher>
where
    Request: Send + Sync,
    Exchange: Send + Sync,
    Factory: ExchangeFactory<Request, Exchange> + Send + Sync,
    Executor: HandlerExecutor<Exchange> + Send + Sync,
    Matcher: PathMatcher<Exchange> + Send + Sync,
{
    pub factory: Factory,
    pub executor: Executor,
    pub matcher: Matcher,
    pub _phantom: PhantomData<(Request, Exchange)>,
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
    async fn route(&self, request: Request) -> Result<Executor::Output, RouterError> {
        let route_key = self
            .factory
            .extract_route_info(&request)
            .await
            .map_err(|e| {
                RouterError::invalid_exchange("Failed to extract route info from request", e)
            })?;

        let handler_chain = self
            .matcher
            .lookup(route_key)
            .ok_or_else(|| todo!("Handle missing route error"))?;

        let mut exchange = self.factory.create_exchange(request).await.map_err(|e| {
            RouterError::invalid_exchange("Failed to create exchange from request", e)
        })?;

        let result = self
            .executor
            .execute_handlers(handler_chain, &mut exchange)
            .await
            .map_err(crate::router::RouterError::execution_failure)?;

        Ok(result)
    }
}
