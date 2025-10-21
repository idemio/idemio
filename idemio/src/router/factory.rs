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
    pub(crate) fn missing_route_info(msg: impl Into<String>) -> Self {
        ExchangeFactoryError::MissingRouteInfo {
            message: msg.into(),
        }
    }
    #[inline]
    pub(crate) fn invalid_exchange(msg: impl Into<String>) -> Self {
        ExchangeFactoryError::InvalidExchange {
            message: msg.into(),
        }
    }
}

#[cfg(feature = "hyper")]
pub mod hyper {
    use crate::exchange::Exchange;
    use crate::router::factory::{ExchangeFactory, ExchangeFactoryError, RouteInfo};
    use async_trait::async_trait;
    use futures_util::StreamExt;
    use http_body_util::BodyExt;
    use http_body_util::combinators::BoxBody;
    use hyper::Request;
    use hyper::body::{Bytes, Incoming};
    use hyper::http::request::Parts;
    use std::error::Error;
    use std::marker::PhantomData;
    use crate::router::path::http::HttpPathMethodKey;

    /// Factory implementation for creating exchanges from Hyper HTTP requests.
    pub struct HyperExchangeFactory;

    #[async_trait]
    impl ExchangeFactory<Request<Incoming>, Exchange<BoxBody<Bytes, std::io::Error>, BoxBody<Bytes, std::io::Error>, Parts>>
    for HyperExchangeFactory
    {
        /// Extracts HTTP method and path from a Hyper request.
        async fn extract_route_info<'a>(
            &self,
            request: &'a Request<Incoming>,
        ) -> Result<RouteInfo<'a>, ExchangeFactoryError> {
            Ok(RouteInfo {
                path: Some(request.uri().path()),
                method: Some(request.method().as_str()),
            })
        }

        async fn create_exchange<'req>(
            &self,
            request: Request<Incoming>,
        ) -> Result<
            Exchange<BoxBody<Bytes, std::io::Error>, BoxBody<Bytes, std::io::Error>, Parts>,
            ExchangeFactoryError,
        > {
            let mut exchange = Exchange::new();
            let (parts, body) = request.into_parts();
            let boxed_body = body.map_err(|e| todo!("Convert to correct error type")).boxed();
            exchange.set_input(boxed_body);
            exchange.set_metadata(parts);
            Ok(exchange)
        }
    }
}