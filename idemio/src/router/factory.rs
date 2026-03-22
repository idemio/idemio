use async_trait::async_trait;
use thiserror::Error;
use crate::exchange::Exchange;

pub struct RouteInfo<'a> {
    pub path: Option<&'a str>,
    pub method: Option<&'a str>,
}

impl<'a> RouteInfo<'a> {
    pub fn new(path: &'a str, method: &'a str) -> Self {
        Self { path: Some(path), method: Some(method) }
    }
}

pub trait IntoRouteInfo<'a> {
    fn into_route_info(self) -> RouteInfo<'a>;
}


pub trait IntoExchange<I, O> 
where
    I: Send + Sync,
    O: Send + Sync,
{
    fn into_exchange(self) -> Result<Exchange<I, O>, ExchangeFactoryError>;
}

pub trait ExchangeFactory<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{

    fn extract_route_info<'a>(
        &self,
        request: &'a I,
    ) -> RouteInfo<'a>;

    fn create_exchange<'req>(
        &self,
        request: I,
    ) -> Exchange<I, O>;
}

#[derive(Error, Debug)]
pub enum ExchangeFactoryError {
    #[error("{message}")]
    MissingRouteInfo { message: String },
    #[error("{message}")]
    InvalidExchange { message: String },
}

impl ExchangeFactoryError {
    #[inline]
    pub fn missing_route_info(msg: impl Into<String>) -> Self {
        ExchangeFactoryError::MissingRouteInfo {
            message: msg.into(),
        }
    }
    #[inline]
    pub fn invalid_exchange(msg: impl Into<String>) -> Self {
        ExchangeFactoryError::InvalidExchange {
            message: msg.into(),
        }
    }
}