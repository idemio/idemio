use crate::exchange::Exchange;
use async_trait::async_trait;
use thiserror::Error;

/// A factory trait for creating exchanges from incoming requests.
///
/// This trait defines the interface for extracting routing information from requests
/// and creating Exchange objects to handle request processing. It provides abstraction
/// over different request formats and protocols.
///
/// # Type Parameters
///
/// * `Request` - The incoming request type (e.g., HTTP request, custom protocol message)
/// * `In` - The input data type that handlers will process
/// * `Out` - The output data type that the termination handler will produce and response handlers will process.
/// * `Meta` - Metadata type associated with the request (e.g., headers, connection info)
///
/// All type parameters must implement `Send + Sync` for thread safety.
#[async_trait]
pub trait ExchangeFactory<Request, In, Out, Meta>
where
    Self: Send + Sync,
    Request: Send + Sync,
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync
{

    // TODO - change the output to generic 'key' which will be used by the path matcher.
    async fn extract_route_info<'a>(
        &self,
        request: &'a Request,
    ) -> Result<(&'a str, &'a str), ExchangeFactoryError>;

    /// Creates an Exchange object from an incoming request.
    ///
    /// # Parameters
    ///
    /// * `request` - The incoming request to be converted into an Exchange. This takes ownership
    ///   of the request, allowing the factory to consume and transform it as needed.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing an `Exchange<In, Out, Meta>` object configured with:
    /// * Input stream or data extracted from the request
    /// * Metadata extracted from the request
    /// * Proper initialization for handler processing
    ///
    /// Returns `ExchangeFactoryError` if the exchange cannot be created from the request.
    ///
    /// # Behavior
    ///
    /// This method consumes the request and should extract all necessary information,
    /// including request body, headers, and other metadata. The created Exchange should
    /// be properly initialized and ready for handler processing. Any I/O operations
    /// or parsing errors should be converted to appropriate ExchangeFactoryError variants.
    async fn create_exchange<'req>(
        &self,
        request: Request,
    ) -> Result<Exchange<'req, In, Out, Meta>, ExchangeFactoryError>;
}

/// Errors that can occur during exchange factory operations.
///
/// This enum defines the various error conditions that can arise when extracting
/// routing information or creating exchanges from requests.
#[derive(Error, Debug)]
pub enum ExchangeFactoryError {
    /// Indicates that required routing information could not be extracted from the request.
    ///
    /// This typically occurs when:
    /// - The request format is malformed or unsupported
    /// - Required routing fields (path, method) are missing or invalid
    /// - The request cannot be parsed according to the expected protocol
    #[error("{message}")]
    MissingRouteInfo { message: String },

    /// Indicates that an exchange could not be created from the request.
    ///
    /// This typically occurs when:
    /// - The request body cannot be processed or streamed
    /// - Required metadata is missing or malformed
    /// - I/O errors occur during request processing
    /// - The request format is incompatible with the exchange system
    #[error("{message}")]
    InvalidExchange { message: String },
}

impl ExchangeFactoryError {
    /// Creates a new `MissingRouteInfo` error with the provided message.
    ///
    /// # Parameters
    ///
    /// * `msg` - A message describing the specific routing information that is missing
    ///
    /// # Returns
    ///
    /// A new `ExchangeFactoryError::MissingRouteInfo` variant
    #[inline]
    pub(crate) fn missing_route_info(msg: impl Into<String>) -> Self {
        ExchangeFactoryError::MissingRouteInfo {
            message: msg.into(),
        }
    }

    /// Creates a new `InvalidExchange` error with the provided message.
    ///
    /// # Parameters
    ///
    /// * `msg` - A message describing why the exchange creation failed
    ///
    /// # Returns
    ///
    /// A new `ExchangeFactoryError::InvalidExchange` variant
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
    use crate::exchange::collector::hyper::HyperBytesCollector;
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
    impl ExchangeFactory<Request<Incoming>, Bytes, BoxBody<Bytes, std::io::Error>, Parts>
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

        /// Creates an Exchange from a Hyper HTTP request with streaming body support.
        ///
        /// # Parameters
        ///
        /// * `request` - The Hyper HTTP request to be converted (takes ownership)
        ///
        /// # Returns
        ///
        /// Returns an `Exchange<Bytes, BoxBody<Bytes, std::io::Error>, Parts>` configured with:
        /// * HTTP request body as a streaming byte source with `HyperBytesCollector`
        /// * HTTP request parts (headers, method, URI, version, extensions) as metadata
        /// * Proper initialization for HTTP request processing pipeline
        ///
        /// # Errors
        ///
        /// This method currently does not return errors as the request decomposition
        /// is infallible. Future versions may add validation that could result in
        /// `ExchangeFactoryError::InvalidExchange`.
        ///
        /// # Behavior
        ///
        /// This method performs the following operations:
        /// 1. Creates a new exchange with a unique UUID
        /// 2. Decomposes the HTTP request into parts (metadata) and body (stream)
        /// 3. Converts the Hyper body stream into the exchange system's stream format
        /// 4. Sets up the `HyperBytesCollector` for efficient byte stream processing
        /// 5. Attaches the HTTP parts as metadata for handler access
        ///
        /// The body stream is processed lazily, allowing for efficient handling of
        /// large payloads without excessive memory usage. Stream errors are properly
        /// converted and propagated through the exchange system.
        async fn create_exchange<'req>(
            &self,
            request: Request<Incoming>,
        ) -> Result<
            Exchange<'req, Bytes, BoxBody<Bytes, std::io::Error>, Parts>,
            ExchangeFactoryError,
        > {
            let mut exchange = Exchange::new();
            let (parts, body) = request.into_parts();

            // Create a stream from the hyper body, converting errors to the exchange system format
            let body_stream = body.into_data_stream().map(|chunk| match chunk {
                Ok(data) => Ok(data),
                Err(e) => Err(Box::new(e) as Box<dyn Error + Send + Sync>),
            });

            // Set the input stream with HyperBytesCollector for optimal byte handling
            exchange.set_input_stream_with_collector(
                Box::pin(body_stream),
                Box::new(HyperBytesCollector),
            );

            // Attach HTTP metadata (headers, method, URI, etc.) for handler access
            exchange.set_metadata(parts);
            Ok(exchange)
        }
    }
}