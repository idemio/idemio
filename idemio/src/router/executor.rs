use crate::exchange::{Exchange, ExchangeError};
use crate::router::route::LoadedChain;
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
pub trait HandlerExecutor<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Executes a chain of handlers on the provided exchange.
    ///
    /// # Parameters
    ///
    /// * `executables` - An `Arc<LoadedChain<In, Out, Meta>>` containing the handler chain
    ///   to be executed, including request handlers, termination handler, and response handlers
    /// * `exchange` - A mutable reference to the exchange object that carries the request/response
    ///   data and metadata through the handler chain
    ///
    /// # Returns
    ///
    /// Returns `Result<Out, ExecutorError>` where:
    /// * `Ok(Out)` - The output produced by the handler chain execution
    /// * `Err(ExecutorError)` - An error that occurred during execution, including handler
    ///   failures, timeouts, or exchange-related errors
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::sync::Arc;
    /// use idemio::exchange::Exchange;
    /// use idemio::router::executor::{DefaultExecutor, ExecutorError, HandlerExecutor};
    /// use idemio::router::route::LoadedChain;
    ///
    /// async fn process_request<In, Out, Meta>(
    ///     executor: &DefaultExecutor,
    ///     chain: Arc<LoadedChain<In, Out, Meta>>,
    ///     mut exchange: Exchange<'_, In, Out, Meta>
    /// ) -> Result<Out, ExecutorError>
    /// where
    ///     In: Send + Sync,
    ///     Out: Send + Sync,
    ///     Meta: Send + Sync,
    /// {
    ///     executor.execute_handlers(chain, &mut exchange).await
    /// }
    /// ```
    ///
    /// # Behavior
    ///
    /// The execution typically follows this pattern:
    /// 1. Execute request handlers in sequence
    /// 2. Execute termination handler
    /// 3. Execute response handlers in sequence
    /// 4. Extract and return the final output
    ///
    /// Handlers can return different status codes that control the flow:
    /// - `in_flight`: Continue to the next handler
    /// - `completed`: Stop execution and return the output
    /// - `error`: Stop execution with an error and return the output
    ///
    /// Any unknown status codes will result in an `ExecutorError::State`.
    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<In, Out, Meta>>,
        exchange: &mut Exchange<In, Out, Meta>,
    ) -> Result<Out, ExecutorError>;

    /// Extracts the output from an exchange after handler execution.
    ///
    /// This is a default implementation that attempts to take the output from the exchange.
    /// If the extraction fails, it returns an `ExecutorError::Output` with the underlying
    /// exchange error as the source.
    ///
    /// # Parameters
    ///
    /// * `exchange` - A mutable reference to the exchange from which to extract the output
    ///
    /// # Returns
    ///
    /// Returns `Result<Out, ExecutorError>` where:
    /// * `Ok(Out)` - The extracted output from the exchange
    /// * `Err(ExecutorError::Output)` - An error occurred while extracting the output
    async fn return_output(exchange: &mut Exchange<In, Out, Meta>) -> Result<Out, ExecutorError> {
        match exchange.take_output().await {
            Ok(out) => Ok(out),
            Err(e) => Err(ExecutorError::output_read_error(e)),
        }
    }
}

/// Represents errors that can occur during handler execution.
///
/// This enum encapsulates various failure modes that can happen when executing
/// handler chains, providing detailed error information for debugging and handling.
#[derive(Error, Debug)]
pub enum ExecutorError {
    /// Indicates that the exchange is in an unknown or unexpected state.
    ///
    /// This error occurs when a handler returns a status code that is not recognized
    /// or expected by the executor, preventing normal processing flow continuation.
    #[error("Exchange is in an unknown state: {state}")]
    State { state: ExchangeState },

    /// Indicates a failure to extract output from the exchange.
    ///
    /// This error occurs when the executor cannot successfully retrieve the output
    /// from the exchange after handler processing, typically due to the output
    /// not being set or being in an invalid state.
    #[error("Failed to extract output from exchange.")]
    Output {
        #[source]
        source: ExchangeError,
    },
}

impl ExecutorError {
    /// Creates a new `Output` error with the provided exchange error as the source.
    ///
    /// # Parameters
    ///
    /// * `err` - The underlying `ExchangeError` that caused the output extraction failure
    ///
    /// # Returns
    ///
    /// Returns a new `ExecutorError::Output` variant containing the source error.
    pub fn output_read_error(err: ExchangeError) -> Self {
        ExecutorError::Output { source: err }
    }

    /// Creates a new `State` error for an unknown exchange state.
    ///
    /// # Parameters
    ///
    /// * `state` - The unknown `ExchangeState` that was encountered
    ///
    /// # Returns
    ///
    /// Returns a new `ExecutorError::State` variant containing the problematic state.
    pub fn unknown_exchange_state(state: ExchangeState) -> Self {
        ExecutorError::State { state }
    }
}

/// A default implementation of the `HandlerExecutor` trait.
///
/// This struct provides a standard execution strategy for handler chains,
/// following the typical request → termination → response handler flow.
/// It implements a sequential execution model where handlers are processed
/// in order and can control the flow through their return status codes.
pub struct DefaultExecutor;

#[async_trait]
impl<In, Out, Meta> HandlerExecutor<In, Out, Meta> for DefaultExecutor
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Executes handlers in the standard sequence: request handlers, termination handler, then response handlers.
    ///
    /// # Parameters
    ///
    /// * `executables` - An `Arc<LoadedChain<In, Out, Meta>>` containing the complete handler chain
    /// * `exchange` - A mutable reference to the exchange that will be processed through the handlers
    ///
    /// # Returns
    ///
    /// Returns `Result<Out, ExecutorError>` containing either the processed output or an execution error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::sync::Arc;
    /// use idemio::exchange::Exchange;
    /// use idemio::router::executor::{DefaultExecutor, ExecutorError, HandlerExecutor};
    /// use idemio::router::route::LoadedChain;
    ///
    /// async fn process_with_default_executor<In, Out, Meta>(
    ///     chain: Arc<LoadedChain<In, Out, Meta>>,
    ///     mut exchange: Exchange<'_, In, Out, Meta>
    /// ) -> Result<Out, ExecutorError>
    /// where
    ///     In: Send + Sync,
    ///     Out: Send + Sync,
    ///     Meta: Send + Sync,
    /// {
    ///     let executor = DefaultExecutor;
    ///     executor.execute_handlers(chain, &mut exchange).await
    /// }
    /// ```
    ///
    /// # Behavior
    ///
    /// The execution follows this sequence:
    ///
    /// 1. **Request Handlers**: Executes each request handler in order. If a handler returns:
    ///    - `in_flight`: Continues to the next handler
    ///    - `completed` or `error`: Immediately returns the output
    ///    - Unknown state: Returns an `ExecutorError::State`
    ///
    /// 2. **Termination Handler**: Executes the single termination handler. If it returns:
    ///    - `completed` or `error`: Returns the output
    ///    - `in_flight`: Continues to response handlers
    ///    - Unknown state: Returns an `ExecutorError::State`
    ///
    /// 3. **Response Handlers**: Executes each response handler in order with the same
    ///    status handling as request handlers
    ///
    /// 4. **Final Output**: If all handlers are complete with `in_flight` status, extracts
    ///    the final output from the exchange
    ///
    /// # Error Handling
    ///
    /// Handler execution uses `.unwrap()` on the result because handlers are expected
    /// to return `Infallible` results, making unwrapping safe in this context.
    ///
    /// Any errors encountered during output extraction are converted to `ExecutorError::Output`
    /// with the underlying `ExchangeError` as the source.
    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<In, Out, Meta>>,
        exchange: &mut Exchange<In, Out, Meta>,
    ) -> Result<Out, ExecutorError> {
        // Execute request handlers in sequence
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
            .take_output()
            .await
            .map_err(|e| ExecutorError::output_read_error(e))
    }
}