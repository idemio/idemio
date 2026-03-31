mod config;
mod path;
mod route;
pub mod builder;
pub use config::{RouterConfig};
pub use path::{LoadedChain, PathMatcherError, RouteKeyMatcher};
pub use route::{RouteKey, RouteKeyParser};

#[cfg(feature = "http")]
pub use path::http::{HttpPathMethodKey, HttpPathMethodMatcher, HttpPathSegment};

use crate::exchange::Exchange;
use crate::handler::{MiddlewareResponse, MiddlewareResult, MiddlewareHandler};
use std::marker::PhantomData;
use std::sync::Arc;
use thiserror::Error;

/// Routes requests to appropriate handlers and returning responses.
pub struct Router<I, O, P, M>
where
    Self: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    P: RouteKeyParser<I>,
    M: RouteKeyMatcher<I, O> + Send + Sync,
{
    pub _phantom: PhantomData<(I, O)>,
    pub matcher: M,
    pub parser: P,
}

impl<I, O, P, M> Router<I, O, P, M>
where
    Self: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    P: RouteKeyParser<I> + Send + Sync,
    M: RouteKeyMatcher<I, O> + Send + Sync,
{
    pub fn new(parser: P, matcher: M) -> Self {
        Self {
            _phantom: PhantomData,
            parser,
            matcher,
        }
    }
    /// Routes a request through the routing system and returns a response.
    pub async fn route(&self, request: I) -> Result<O, RouterError> {
        let route_key = self.parser.as_route_key(&request);
        let handler_chain = self
            .matcher
            .lookup(route_key)
            .ok_or_else(|| {
                todo!()
            })?;
        let result = self.execute_handlers(handler_chain, request).await?;

        Ok(result)
    }

    /// Executes a chain of handlers.
    /// If the response from executing any of the handlers in the chain is a 'break' or an 'error', return the status.
    async fn execute_handler_chain<T>(
        handlers: &Vec<Arc<dyn MiddlewareHandler<T>>>,
        exchange: &mut Exchange<T>,
    ) -> Option<MiddlewareResult>
    where
        T: Send + Sync,
    {
        for handler in handlers {
            match handler.exec(exchange).await {
                Ok(flow) => {
                    if let MiddlewareResponse::Break = flow {
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
        request: I,
    ) -> Result<O, RouterError> {
        let mut request_exchange = Exchange::new(request);
        let request_handlers = executables.request_handlers();

        // Early exit
        if let Some(status) =
            Self::execute_handler_chain::<I>(&request_handlers, &mut request_exchange).await
        {
            match status {
                Ok(flow) => {
                    if let MiddlewareResponse::Break = flow {
                        todo!("Early return")
                    }
                }
                Err(error) => todo!("Convert error '{error}' into generic O."),
            }
        }

        // Execute the Termination Handler
        let (uuid, data) = request_exchange.take_data();
        let mut response_exchange = match executables
            .termination_handler()
            .exec(data)
            .await
        {
            Ok(output) => Exchange::from((uuid, output)),
            Err(error) => todo!("Convert error '{error}' into generic O."),
        };
        let response_handlers = executables.response_handlers();

        // Check for early exit on response handlers.
        if let Some(status) =
            Self::execute_handler_chain::<O>(response_handlers, &mut response_exchange).await
        {
            match status {
                Ok(flow) => {
                    if let MiddlewareResponse::Break = flow {
                        todo!("Handle early exit on response handlers")
                    }
                }
                Err(error) => todo!("Convert error '{error}' into generic O."),
            }
        }
        Ok(response_exchange.take_data().1.data)
    }
}

// Original RouterError
#[derive(Error, Debug)]
pub enum RouterError {
    #[error("Matching route for key ({key1} -- {key2}) was not found.")]
    MissingRoute { key1: String, key2: String },
    #[error("{message}")]
    ExecutionFailure { message: String },
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
}
