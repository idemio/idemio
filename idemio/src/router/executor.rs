use crate::exchange::{Exchange, ExchangeError};
use crate::router::path::LoadedChain;
use crate::status::ExchangeState;
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

/// A trait for executing handler chains in an asynchronous request/response processing system.
///
/// This trait defines the contract for executing a series of handlers on an exchange,
/// providing a standardized way to process requests through various middleware and
/// termination handlers.
#[async_trait]
pub trait HandlerExecutor<Exchange>
where
    Exchange: Send + Sync,
{
    /// The output type that this executor returns after processing
    type Output: Send + Sync;

    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<Exchange>>,
        exchange: &mut Exchange,
    ) -> Result<Self::Output, ExecutorError>;

    async fn return_output(exchange: &mut Exchange) -> Result<Self::Output, ExecutorError>;
}


#[derive(Error, Debug)]
pub enum ExecutorError {

    #[error("Exchange is in an unknown state: {state}")]
    State { state: ExchangeState },

    #[error("Failed to extract output from exchange.")]
    Output {
        #[source]
        source: ExchangeError,
    },
}

impl ExecutorError {

    pub fn output_read_error(err: ExchangeError) -> Self {
        ExecutorError::Output { source: err }
    }

    pub fn unknown_exchange_state(state: ExchangeState) -> Self {
        ExecutorError::State { state }
    }
}

pub struct DefaultExecutor<Output> {
    pub _phantom: std::marker::PhantomData<Output>,
}

#[async_trait]
impl<E, Output> HandlerExecutor<E> for DefaultExecutor<Output>
where
    E: ExtractOutput<Output> + Send + Sync,
    Output: Send + Sync,

{
    type Output = Output;

    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<E>>,
        exchange: &mut E,
    ) -> Result<Self::Output, ExecutorError> {
        let request_handlers = executables.request_handlers();
        for handler in request_handlers {
            let status = handler.exec(exchange).await.unwrap();
            if status.code.is_in_flight() {
                continue;
            } else if status.code.is_completed() || status.code.is_error() {
                return Ok(Self::return_output(exchange).await?);
            } else {
                return Err(ExecutorError::unknown_exchange_state(status.code));
            }
        }

        // Execute termination handler
        let termination_handler = executables.termination_handler();
        // Handler result is Infallible, so unwrap is safe
        let status = termination_handler.exec(exchange).await.unwrap();
        if status.code.is_completed() || status.code.is_error() {
            return Ok(Self::return_output(exchange).await?);
        } else if !status.code.is_in_flight() {
            return Err(ExecutorError::unknown_exchange_state(status.code));
        }

        // Execute response handlers in sequence
        let response_handlers = executables.response_handlers();
        for handler in response_handlers {
            // Handler result is Infallible, so unwrap is safe
            let status = handler.exec(exchange).await.unwrap();
            if status.code.is_in_flight() {
                continue;
            } else if status.code.is_completed() || status.code.is_error() {
                return Ok(Self::return_output(exchange).await?);
            } else {
                return Err(ExecutorError::unknown_exchange_state(status.code));
            }
        }

        // Final output extraction if all handlers returned in_flight
        exchange
            .extract_output()
            .await
            .map_err(|e| ExecutorError::output_read_error(e))
    }

    async fn return_output(exchange: &mut E) -> Result<Self::Output, ExecutorError> {
        exchange.extract_output().await.map_err(|e| ExecutorError::output_read_error(e))
    }
}

/// Helper trait to extract output from an exchange
/// This abstracts the output extraction logic and makes it testable
#[async_trait]
pub trait ExtractOutput<Output> {
    async fn extract_output(&mut self) -> Result<Output, ExchangeError>;
}

#[async_trait]
impl<In, Out, Meta> ExtractOutput<Out> for Exchange<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    async fn extract_output(&mut self) -> Result<Out, ExchangeError> {
        self.take_output().await
    }
}
