use async_trait::async_trait;
use thiserror::Error;

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
    ) -> Result<(&'a str, &'a str), ExchangeFactoryError>;

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
    use crate::router::factory::{ExchangeFactory, ExchangeFactoryError};
    use async_trait::async_trait;
    use futures_util::StreamExt;
    use http_body_util::BodyExt;
    use http_body_util::combinators::BoxBody;
    use hyper::Request;
    use hyper::body::{Bytes, Incoming};
    use hyper::http::request::Parts;
    use std::error::Error;
    use std::marker::PhantomData;
    use crate::router::path::http::PathPrefixMethodKey;

    /// Factory implementation for creating exchanges from Hyper HTTP requests.
    ///
    /// This struct provides a concrete implementation of `ExchangeFactory` for HTTP requests
    /// using the Hyper library. It handles HTTP-specific request parsing and creates exchanges
    /// suitable for HTTP request processing with streaming body support.
    ///
    /// # Type Specialization
    ///
    /// This factory is specialized for:
    /// * `Request<Incoming>` - Hyper HTTP requests with streaming bodies
    /// * `Bytes` - Input/output data as byte arrays from Hyper's body system
    /// * `BoxBody<Bytes, std::io::Error>` - Output type for HTTP responses with proper error handling
    /// * `Parts` - HTTP request metadata (headers, method, URI, version, extensions)
    ///
    /// # Thread Safety
    ///
    /// This factory is thread-safe and can be shared across multiple async tasks.
    /// It contains no mutable state and all operations are stateless.
    ///
    /// # Behavior
    ///
    /// The factory handles HTTP request body streaming efficiently by:
    /// - Converting Hyper's `Incoming` body stream to the exchange system's stream format
    /// - Preserving all HTTP metadata in the exchange
    /// - Using `HyperBytesCollector` for optimal byte stream collection
    /// - Providing proper error handling for I/O and parsing errors
    pub struct HyperExchangeFactory;

    #[async_trait]
    impl ExchangeFactory<Request<Incoming>, Exchange<BoxBody<Bytes, std::io::Error>, BoxBody<Bytes, std::io::Error>, Parts>>
    for HyperExchangeFactory
    {
        /// Extracts HTTP method and path from a Hyper request.
        ///
        /// # Parameters
        ///
        /// * `request` - A reference to the Hyper HTTP request
        ///
        /// # Returns
        ///
        /// Returns a tuple containing:
        /// * `&str` - The request path from the URI (e.g., "/api/users", "/health")
        /// * `&str` - The HTTP method as a string (e.g., "GET", "POST", "PUT", "DELETE")
        ///
        /// # Errors
        ///
        /// This method is infallible for well-formed Hyper requests as the URI and method
        /// are always available in a valid HTTP request.
        ///
        /// # Behavior
        ///
        /// This method extracts routing information directly from the HTTP request
        /// without consuming it. It uses Hyper's built-in method and URI parsing,
        /// which are guaranteed to be present in valid HTTP requests.
        async fn extract_route_info<'a>(
            &self,
            request: &'a Request<Incoming>,
        ) -> Result<(&'a str, &'a str), ExchangeFactoryError> {
            let method = request.method().as_str();
            let path = request.uri().path();
            Ok((path, method))
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