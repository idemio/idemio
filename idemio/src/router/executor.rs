use crate::exchange::{Exchange, ExchangeError};
use crate::router::route::LoadedChain;
use async_trait::async_trait;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

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
    ///
    /// use idemio::router::route::LoadedChain;
    ///
    /// async fn process_request<In, Out, Meta>(
    ///     executor: &DefaultExecutor,
    ///     chain: Arc<LoadedChain<In, Out, Meta>>,
    ///     mut exchange: Exchange<In, Out, Meta>
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
    /// Any unknown status codes will result in an `ExecutorError::FailedToExecute`.
    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<In, Out, Meta>>,
        exchange: &mut Exchange<In, Out, Meta>,
    ) -> Result<Out, ExecutorError>;

    /// Extracts the output from an exchange after handler execution.
    async fn return_output(exchange: &mut Exchange<In, Out, Meta>) -> Result<Out, ExecutorError> {
        match exchange.take_output().await {
            Ok(out) => Ok(out),
            Err(e) => Err(ExecutorError::exchange_error(e)),
        }
    }
}

/// Represents errors that can occur during handler execution.
///
/// This enum encapsulates various failure modes that can happen when executing
/// handler chains, providing detailed error information for debugging and handling.
#[derive(Debug)]
pub enum ExecutorError {
    /// Indicates that handler execution failed for a general reason.
    ///
    /// Contains a descriptive message explaining why the execution failed.
    FailedToExecute(String),

    /// Indicates that a handler exceeded its execution timeout.
    ///
    /// Contains a descriptive message about which handler timed out and any relevant details.
    HandlerTimeout(String),

    /// Wraps an error that occurred within the exchange system.
    ///
    /// This variant encapsulates `ExchangeError` instances that can occur during
    /// exchange operations like reading input, writing output, or managing metadata.
    ExchangeError(ExchangeError),
}

impl ExecutorError {
    /// Creates a new `FailedToExecute` error with the provided message.
    pub fn failed_to_execute(msg: impl Into<String>) -> Self {
        ExecutorError::FailedToExecute(msg.into())
    }

    /// Creates a new `HandlerTimeout` error with the provided message.
    pub fn handler_timeout(msg: impl Into<String>) -> Self {
        ExecutorError::HandlerTimeout(msg.into())
    }

    /// Creates a new `ExchangeError` by wrapping an existing `ExchangeError`.
    pub fn exchange_error(err: ExchangeError) -> Self {
        ExecutorError::ExchangeError(err)
    }
}

impl Display for ExecutorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutorError::FailedToExecute(msg) => write!(f, "Failed to execute: {}", msg),
            ExecutorError::HandlerTimeout(msg) => write!(f, "Handler timeout: {}", msg),
            ExecutorError::ExchangeError(err) => write!(f, "{}", err),
        }
    }
}

impl std::error::Error for ExecutorError {}

/// A default implementation of the `HandlerExecutor` trait.
///
/// This struct provides a standard execution strategy for handler chains,
/// following the typical request → termination → response handler flow.
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
    ///     mut exchange: Exchange<In, Out, Meta>
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
    ///    - Unknown state: Returns an error
    ///
    /// 2. **Termination Handler**: Executes the single termination handler. If it returns:
    ///    - `completed` or `error`: Returns the output
    ///    - `in_flight`: Continues to response handlers
    ///    - Unknown state: Returns an error
    ///
    /// 3. **Response Handlers**: Executes each response handler in order with the same
    ///    status handling as request handlers
    ///
    /// 4. **Final Output**: If all handlers complete with `in_flight` status, extracts
    ///    the final output from the exchange
    ///
    /// Handler execution uses `.unwrap()` on the result because handlers are expected
    /// to return `Infallible` results, making unwrap safe in this context.
    ///
    /// Any errors encountered during output extraction are converted to `ExecutorError::ExchangeError`.
    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<In, Out, Meta>>,
        exchange: &mut Exchange<In, Out, Meta>,
    ) -> Result<Out, ExecutorError> {
        let request_handlers = executables.request_handlers();
        for handler in request_handlers {
            // Handler result is Infallible, so unwrap is safe
            let status = handler.exec(exchange).await.unwrap();
            if status.code.is_in_flight() {
                continue;
            } else if status.code.is_completed() || status.code.is_error() {
                return Ok(Self::return_output(exchange).await?);
            } else {
                return Err(ExecutorError::failed_to_execute(format!(
                    "Unknown state '{:b}' returned from handler",
                    status.code
                )));
            }
        }

        let termination_handler = executables.termination_handler();
        // Handler result is Infallible, so unwrap is safe
        let status = termination_handler.exec(exchange).await.unwrap();
        if status.code.is_completed() || status.code.is_error() {
            return Ok(Self::return_output(exchange).await?);
        } else if !status.code.is_in_flight() {
            return Err(ExecutorError::failed_to_execute(format!(
                "Unknown state '{:b}' returned from handler",
                status.code
            )));
        }

        let response_handlers = executables.response_handlers();
        for handler in response_handlers {
            // Handler result is Infallible, so unwrap is safe
            let status = handler.exec(exchange).await.unwrap();
            if status.code.is_in_flight() {
                continue;
            } else if status.code.is_completed() || status.code.is_error() {
                return Ok(Self::return_output(exchange).await?);
            } else {
                return Err(ExecutorError::failed_to_execute(format!(
                    "Unknown state '{:b}' returned from handler",
                    status.code
                )));
            }
        }

        match exchange.take_output().await {
            Ok(out) => Ok(out),
            Err(e) => Err(ExecutorError::exchange_error(e)),
        }
    }
}