use async_trait::async_trait;
use thiserror::Error;

pub struct RouteInfo<'a> {
    pub path: Option<&'a str>,
    pub method: Option<&'a str>,
}

impl<'a> RouteInfo<'a> {
    pub fn new(path: &'a str, method: &'a str) -> Self {
        Self { path: Some(path), method: Some(method) }
    }
}

#[async_trait]
pub trait ExchangeFactory<Request, E>
where
    Self: Send + Sync,
    Request: Send + Sync,
    E: Send + Sync,
{

    async fn extract_route_info<'a>(
        &self,
        request: &'a Request,
    ) -> Result<RouteInfo<'a>, ExchangeFactoryError>;

    async fn create_exchange<'req>(
        &self,
        request: Request,
    ) -> Result<E, ExchangeFactoryError>;
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