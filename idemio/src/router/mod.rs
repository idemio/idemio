pub mod config;
pub mod executor;
pub mod factory;
pub mod route;

use crate::handler::registry::HandlerRegistry;
use crate::router::config::RouterConfig;
use crate::router::executor::{ExecutorError, HandlerExecutor};
use crate::router::factory::ExchangeFactory;
use crate::router::route::PathRouter;
use crate::status::ExchangeState;
use async_trait::async_trait;
use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Main router trait that processes requests through handler chains
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
    async fn route(&self, request: Req) -> Result<Out, RouterError>;
}

pub trait RouterComponents<Req, In, Out, Meta>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    type Factory: ExchangeFactory<Req, In, Out, Meta> + Send + Sync;
    type Executor: HandlerExecutor<In, Out, Meta> + Send + Sync;
}

/// Default implementation of the Router trait
pub struct RequestRouter<Req, In, Out, Meta, Components>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Components: RouterComponents<Req, In, Out, Meta>,
{
    phantom: PhantomData<Req>,
    factory: Arc<Components::Factory>,
    executor: Arc<Components::Executor>,
    router: PathRouter<In, Out, Meta>,
}

impl<Req, In, Out, Meta, Components> RequestRouter<Req, In, Out, Meta, Components>
where
    Req: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
    Components: RouterComponents<Req, In, Out, Meta>,
{
    pub fn new(
        registry: &HandlerRegistry<In, Out, Meta>,
        config: &RouterConfig,
        exchange_factory: Components::Factory,
        handler_executor: Components::Executor,
    ) -> Result<Self, RouterError> {
        let route_table = match PathRouter::new(config, registry) {
            Ok(table) => table,
            Err(e) => todo!(),
        };
        Ok(Self {
            phantom: PhantomData::default(),
            factory: Arc::new(exchange_factory),
            executor: Arc::new(handler_executor),
            router: route_table,
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
    async fn route(&self, request: Req) -> Result<Out, RouterError> {
        let (path, method) = self.factory.extract_route_info(&request).await?;
        log::trace!("Extracted route info from request: {}@{}", &path, &method);

        let executables = self
            .router
            .lookup(path, method)
            .ok_or(RouterError::route_not_found(path))?;
        log::trace!("Found handler chain for request: {}@{}", &path, &method);
        let mut exchange = self.factory.create_exchange(request).await?;

        //self.execute_handlers(executables, &mut exchange).await?;
        let result = self
            .executor
            .execute_handlers(executables, &mut exchange)
            .await
            .map_err(|e| RouterError::execution_failed(e))?;
        Ok(result)
    }
}

#[derive(Debug)]
pub enum RouterError {
    RouteNotFound(String),
    MethodNotSupported(String),
    ExecutionFailed(ExecutorError),
    ExchangeCreationFailed(String),
    ExchangeTimeoutError(SystemTime, SystemTime, String),
    ExchangeStatusError(ExchangeState, i32),
}

impl RouterError {
    pub fn exchange_status_error(status: ExchangeState, code: i32) -> Self {
        RouterError::ExchangeStatusError(status, code)
    }

    pub fn exchange_timeout_error(
        start: SystemTime,
        end: SystemTime,
        id: impl Into<String>,
    ) -> Self {
        RouterError::ExchangeTimeoutError(start, end, id.into())
    }

    pub fn route_not_found(route: impl Into<String>) -> Self {
        RouterError::RouteNotFound(route.into())
    }

    pub fn method_not_supported(method: impl Into<String>) -> Self {
        RouterError::MethodNotSupported(method.into())
    }
    pub fn execution_failed(err: ExecutorError) -> Self {
        RouterError::ExecutionFailed(err)
    }

    pub fn exchange_creation_failed(msg: impl Into<String>) -> Self {
        RouterError::ExchangeCreationFailed(msg.into())
    }
}

impl Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::RouteNotFound(route) => write!(f, "Route '{}' not found", route),
            RouterError::MethodNotSupported(method) => {
                write!(f, "Method '{}' not supported", method)
            }
            RouterError::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            RouterError::ExchangeCreationFailed(msg) => {
                write!(f, "Exchange creation failed: {}", msg)
            }
            RouterError::ExchangeTimeoutError(start, end, id) => {
                let duration = end
                    .duration_since(*start)
                    .unwrap_or_else(|_| Duration::from_secs(0));
                write!(
                    f,
                    "Exchanged timed out after {}ms while executing handler '{}'",
                    duration.as_millis(),
                    id
                )
            }
            RouterError::ExchangeStatusError(state, code) => {
                write!(
                    f,
                    "Returning status code {} because state was marked as {}",
                    code, state
                )
            }
        }
    }
}

impl std::error::Error for RouterError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Exchange;
    use async_trait::async_trait;

    #[tokio::test]
    async fn test_custom_exchange_factory() {
        /// This is a custom exchange where the request format follows the following pattern:
        /// String(<method>:<path>:<body)
        /// e.g
        /// "GET:/endpoint:body text"
        struct CustomExchangeFactory;

        #[async_trait]
        impl ExchangeFactory<String, String, String, ()> for CustomExchangeFactory {
            async fn extract_route_info<'a>(
                &self,
                request: &'a String,
            ) -> Result<(&'a str, &'a str), RouterError> {
                let mut parts = request.split(':');
                if let (Some(method), Some(path), Some(_)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    Ok((path, method))
                } else {
                    Err(RouterError::ExchangeCreationFailed(
                        "Invalid format".to_string(),
                    ))
                }
            }

            async fn create_exchange<'a>(
                &self,
                request: String,
            ) -> Result<Exchange<'a, String, String, ()>, RouterError> {
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
