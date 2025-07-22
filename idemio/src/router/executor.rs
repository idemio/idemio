use crate::router::route::LoadedChain;
use async_trait::async_trait;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use crate::exchange::Exchange;

#[async_trait]
pub trait HandlerExecutor<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    async fn execute_handlers(
        &self,
        executables: Arc<LoadedChain<In, Out, Meta>>,
        exchange: &mut Exchange<In, Out, Meta>,
    ) -> Result<(), ExecutorError>;
}

#[derive(Debug)]
pub enum ExecutorError {
    FailedToExecute(String),
    HandlerTimeout(String),
}

impl ExecutorError {
    pub fn failed_to_execute(msg: impl Into<String>) -> Self {
        ExecutorError::FailedToExecute(msg.into())
    }
    pub fn handler_timeout(msg: impl Into<String>) -> Self {
        ExecutorError::HandlerTimeout(msg.into())
    }
}

impl Display for ExecutorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutorError::FailedToExecute(msg) => write!(f, "Failed to execute: {}", msg),
            ExecutorError::HandlerTimeout(msg) => write!(f, "Handler timeout: {}", msg),
        }
    }
}

impl std::error::Error for ExecutorError {}

pub struct DefaultExecutor;

#[async_trait]
impl<In, Out, Meta> HandlerExecutor<In, Out, Meta> for DefaultExecutor 
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    async fn execute_handlers(&self, executables: Arc<LoadedChain<In, Out, Meta>>, exchange: &mut Exchange<In, Out, Meta>) -> Result<(), ExecutorError> {
        let request_handlers = &executables.request_handlers;
        for handler in request_handlers {
            if let Err(e) = handler.exec(exchange).await {
                todo!("Handle handler execution failure on request chain")
            }
        }
        let termination_handler = &executables.termination_handler;
        if let Err(e) = termination_handler.exec(exchange).await {
            todo!("Handle termination handler execution failure")
        }

        let response_handlers = &executables.response_handlers;
        for handler in response_handlers {
            if let Err(e) = handler.exec(exchange).await {
                todo!("Handle handler execution failure on response chain")
            }
        }
        Ok(())
    }
}
