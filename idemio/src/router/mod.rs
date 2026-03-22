pub mod config;
pub mod factory;
pub mod path;

use crate::exchange::Exchange;
use crate::handler::{Handler, HandlerFlow, HandlerResponse};
use crate::router::factory::{ExchangeFactory, ExchangeFactoryError};
use crate::router::path::{LoadedChain, PathMatcher, PathMatcherError};
use std::marker::PhantomData;
use std::sync::Arc;
use thiserror::Error;

/// Routes requests to appropriate handlers and returning responses.
pub struct Router<I, O, Factory, Matcher>
where
    Self: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    Factory: ExchangeFactory<I, O> + Send + Sync,
    Matcher: PathMatcher<I, O> + Send + Sync,
{
    pub _phantom: PhantomData<(I, O)>,
    pub factory: Factory,
    pub matcher: Matcher,
}

impl<I, O, Factory, Matcher> Router<I, O, Factory, Matcher>
where
    Self: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    Factory: ExchangeFactory<I, O> + Send + Sync,
    Matcher: PathMatcher<I, O> + Send + Sync,
{
    pub fn new(factory: Factory, matcher: Matcher) -> Self {
        Self {
            _phantom: PhantomData,
            factory,
            matcher,
        }
    }
    /// Routes a request through the routing system and returns a response.
    pub async fn route(&self, request: I) -> Result<O, RouterError> {
        let route_key = self.factory.extract_route_info(&request);
        let handler_chain = self
            .matcher
            .lookup(route_key)
            .ok_or_else(|| todo!("Handle missing route error"))?;
        let mut exchange = self.factory.create_exchange(request);
        let result = self.execute_handlers(handler_chain, &mut exchange).await?;

        Ok(result)
    }

    /// Executes a chain of handlers.
    /// If the response from executing any of the handlers in the chain is a 'break' or an 'error', return the status.
    async fn execute_handler_chain(
        handlers: &Vec<Arc<dyn Handler<I, O>>>,
        exchange: &mut Exchange<I, O>,
    ) -> Option<HandlerResponse> {
        for handler in handlers {
            match handler.exec(exchange).await {
                Ok(flow) => {
                    if let HandlerFlow::Break = flow {
                        return Some(Ok(flow));
                    }
                }
                Err(error) => return Some(Err(error)),
            }
        }
        None
    }

    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<I, O>>,
        exchange: &mut Exchange<I, O>,
    ) -> Result<O, RouterError> {
        let request_handlers = executables.request_handlers();

        // Early exit
        if let Some(status) = Self::execute_handler_chain(&request_handlers, exchange).await {
            match status {
                Ok(flow) => {
                    if let HandlerFlow::Break = flow {
                        return Ok(Self::return_output(exchange).await?);
                    }
                }
                Err(error) => todo!("Convert error '{error}' into generic O."),
            }
        }

        // Execute the Termination Handler
        match executables.termination_handler().exec(exchange).await {
            Ok(flow) => {
                if let HandlerFlow::Break = flow {
                    return Ok(Self::return_output(exchange).await?);
                }
            }
            Err(error) => todo!("Convert error '{error}' into generic O."),
        }
        let response_handlers = executables.response_handlers();

        // Check for early exit on response handlers.
        if let Some(status) = Self::execute_handler_chain(response_handlers, exchange).await {
            match status {
                Ok(flow) => {
                    if let HandlerFlow::Break = flow {
                        return Ok(Self::return_output(exchange).await?);
                    }
                }
                Err(error) => todo!("Convert error '{error}' into generic O."),
            }
        }
        Ok(Self::return_output(exchange).await?)
    }

    async fn return_output(exchange: &mut Exchange<I, O>) -> Result<O, RouterError> {
        exchange
            .take_output()
            .await
            .map_err(|_| RouterError::MissingResponseError)
    }
}

// Original RouterError
#[derive(Error, Debug)]
pub enum RouterError {
    #[error("Matching route for key ({key1} -- {key2}) was not found.")]
    MissingRoute { key1: String, key2: String },
    #[error("{message}")]
    ExecutionFailure { message: String },
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
    #[error("No handler configured to produce a response.")]
    MissingResponseError,
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
    pub fn invalid_exchange(msg: impl Into<String>, err: ExchangeFactoryError) -> Self {
        RouterError::InvalidExchange {
            message: msg.into(),
            source: err,
        }
    }
}
