mod registry;

use async_trait::async_trait;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use thiserror::Error;
use crate::exchange::{Exchange, ExchangeError, InnerData};
use crate::attachments::Attachments;

pub use registry::{HandlerRegistryError, HandlerRegistry};
pub type MiddlewareResult = Result<MiddlewareResponse, HandlerError>;

pub trait LabeledHandler {}

#[async_trait]
pub trait MiddlewareHandler<T>: LabeledHandler + Send + Sync
where
    T: Send + Sync
{
    async fn exec(&self, exchange: &mut Exchange<T>) -> MiddlewareResult;
}

#[async_trait]
pub trait TerminationHandler<I, O>: LabeledHandler + Send + Sync
where
    I: Send + Sync,
    O: Send + Sync
{
    async fn exec(&self, data: InnerData<I>) -> Result<InnerData<O>, HandlerError>;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MiddlewareResponse {
    #[default]
    Continue,
    Break
}

impl MiddlewareResponse {
    #[inline]
    pub const fn ok() -> MiddlewareResult {
        Ok(Self::Continue)
    }
    #[inline]
    pub const fn stop() -> MiddlewareResult {
        Ok(Self::Break)
    }
}

#[derive(Debug, Error)]
pub enum HandlerError
where
    Self: Send + Sync
{
    #[error("No handler found with the name '{handler_id}'.")]
    HandlerNotFound {
        handler_id: &'static str
    },

    #[error("Error occurred while executing handler '{handler_id}'.")]
    HandlerException {
        handler_id: &'static str,

        #[source]
        source: Box<dyn std::error::Error + Send + Sync>
    },

    #[error("Handler '{handler_id}' could not find required data.")]
    MissingDataError {
        handler_id: &'static str,

        #[source]
        source: ExchangeError
    },
}

impl HandlerError {

    #[inline]
    pub fn handler_exception<H, E>(handler_id: &'static str, source: E) -> HandlerError
    where
        E: std::error::Error + Send + Sync + 'static
    {
        HandlerError::HandlerException {
            handler_id,
            source: Box::new(source)
        }
    }

    #[inline]
    pub const fn missing_data(handler_id: &'static str, source: ExchangeError) -> HandlerError {
        HandlerError::MissingDataError {
            handler_id,
            source
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandlerId {
    handler_hash: u64,
}

impl HandlerId {
    pub fn new(id: impl Into<String>) -> Self {
        let mut hasher = fnv::FnvHasher::default();
        let handler_id = id.into();
        handler_id.hash(&mut hasher);
        let hash = hasher.finish();
        Self {
            handler_hash: hash,
        }
    }
}

impl From<&str> for HandlerId {
    fn from(value: &str) -> Self {
        HandlerId::new(value)
    }
}

impl Display for HandlerId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.handler_hash)
    }
}
