mod config;
mod path;
mod route;

pub use config::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
    SingleServiceMethodBuilder, SingleServiceRouteBuilder,
};
pub use path::{LoadedChain, PathMatcherError, RouteMatcher};
pub use route::{RouteKey, RouteKeyParser};

#[cfg(feature = "http")]
pub use path::http::{HeaderKey, HttpPathMethodKey, HttpPathMethodMatcher, HttpPathSegment};

use crate::exchange::Exchange;
use crate::handler::{HandlerFlow, HandlerResponse, MiddlewareHandler};
use std::marker::PhantomData;
use std::sync::Arc;
use thiserror::Error;

/// Routes requests to appropriate handlers and returning responses.
pub struct Router<I, O, Parser, Matcher>
where
    Self: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    Parser: RouteKeyParser<I>,
    Matcher: RouteMatcher<I, O> + Send + Sync,
{
    pub _phantom: PhantomData<(I, O)>,
    pub matcher: Matcher,
    pub parser: Parser,
}

impl<I, O, Parser, Matcher> Router<I, O, Parser, Matcher>
where
    Self: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    Parser: RouteKeyParser<I> + Send + Sync,
    Matcher: RouteMatcher<I, O> + Send + Sync,
{
    pub fn new(parser: Parser, matcher: Matcher) -> Self {
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
            .ok_or_else(|| todo!("Handle missing route error"))?;
        let result = self.execute_handlers(handler_chain, request).await?;

        Ok(result)
    }

    /// Executes a chain of handlers.
    /// If the response from executing any of the handlers in the chain is a 'break' or an 'error', return the status.
    async fn execute_handler_chain<T>(
        handlers: &Vec<Arc<dyn MiddlewareHandler<T>>>,
        exchange: &mut Exchange<T>,
    ) -> Option<HandlerResponse>
    where
        T: Send + Sync,
    {
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
                    if let HandlerFlow::Break = flow {
                        todo!("Early return")
                    }
                }
                Err(error) => todo!("Convert error '{error}' into generic O."),
            }
        }

        // Execute the Termination Handler
        let mut response_exchange = match executables
            .termination_handler()
            .exec(request_exchange)
            .await
        {
            Ok(output) => Exchange::new(output),
            Err(error) => todo!("Convert error '{error}' into generic O."),
        };
        let response_handlers = executables.response_handlers();

        // Check for early exit on response handlers.
        if let Some(status) =
            Self::execute_handler_chain::<O>(response_handlers, &mut response_exchange).await
        {
            match status {
                Ok(flow) => {
                    if let HandlerFlow::Break = flow {
                        todo!("Handle early exit on response handlers")
                    }
                }
                Err(error) => todo!("Convert error '{error}' into generic O."),
            }
        }
        response_exchange
            .take_data()
            .map_err(|e| todo!("Handle read error on response"))
    }

    async fn return_output<T>(exchange: &mut Exchange<T>) -> Result<T, RouterError>
    where
        T: Send + Sync,
    {
        exchange
            .take_data()
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
