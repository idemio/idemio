use crate::router::RouterError;
use async_trait::async_trait;
use crate::exchange::Exchange;

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
    Meta: Send + Sync,
{
    /// Extracts routing information from an incoming request.
    ///
    /// # Parameters
    ///
    /// * `request` - A reference to the incoming request from which to extract routing information
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing a tuple of:
    /// * `&str` - The request path (e.g., "/api/users")
    /// * `&str` - The HTTP method or protocol action (e.g., "GET", "POST")
    ///
    /// Returns `RouterError` if the routing information cannot be extracted from the request.
    ///
    /// # Examples
    /// See [HyperExchangeFactory](hyper/struct.HyperExchangeFactory.html) for an example.
    ///
    /// # Behavior
    ///
    /// This method should be lightweight and fast as it's called early in the request
    /// processing pipeline. It should not consume the request or perform expensive operations.
    /// The returned string references must have the same lifetime as the input request.
    async fn extract_route_info<'req>(
        &self,
        request: &'req Request,
    ) -> Result<(&'req str, &'req str), RouterError>;

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
    /// Returns `RouterError` if the exchange cannot be created from the request.
    ///
    /// # Examples
    /// See [HyperExchangeFactory](hyper/struct.HyperExchangeFactory.html) for an example.
    ///
    /// # Behavior
    ///
    /// This method consumes the request and should extract all necessary information, 
    /// including request body, headers, and other metadata. The created Exchange should
    /// be properly initialized and ready for handler processing. Any I/O operations
    /// or parsing errors should be converted to appropriate RouterError variants.
    async fn create_exchange<'req>(&self, request: Request) -> Result<Exchange<'req, In, Out, Meta>, RouterError>;
}

#[cfg(feature = "hyper")]
pub mod hyper {
    use futures_util::StreamExt;
    use std::error::Error;
    use std::marker::PhantomData;
    use async_trait::async_trait;
    use http_body_util::BodyExt;
    use http_body_util::combinators::BoxBody;
    use hyper::Request;
    use crate::exchange::Exchange;
    use crate::router::RouterError;
    use crate::router::factory::ExchangeFactory;
    use hyper::body::{Bytes, Incoming};
    use hyper::http::request::Parts;
    use crate::exchange::collector::hyper::HyperBytesCollector;

    /// Factory implementation for creating exchanges from Hyper HTTP requests.
    ///
    /// This struct provides a concrete implementation of `ExchangeFactory` for HTTP requests
    /// using the Hyper library. It handles HTTP-specific request parsing and creates exchanges
    /// suitable for HTTP request processing.
    ///
    /// # Type Specialization
    ///
    /// This factory is specialized for:
    /// * `Request<Incoming>` - Hyper HTTP requests with streaming bodies
    /// * `Bytes` - Input/output data as byte arrays
    /// * `Parts` - HTTP request metadata (headers, method, URI, etc.)
    ///
    /// # Examples
    /// See `example` for an example of how to use this factory.
    ///
    /// # Behavior
    ///
    /// The factory handles HTTP request body streaming and converts Hyper's body format
    /// into a format suitable for the exchange system. It preserves all HTTP metadata
    /// and provides error handling for malformed requests.
    pub struct HyperExchangeFactory;

    #[async_trait]
    impl ExchangeFactory<
        Request<Incoming>,
        Bytes,
        BoxBody<Bytes, std::io::Error>,
        Parts
    > for HyperExchangeFactory
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
        /// * `&str` - The request path from the URI (e.g., "/api/users")
        /// * `&str` - The HTTP method as a string (e.g., "GET", "POST")
        ///
        /// # Behavior
        ///
        /// This method extracts routing information directly from the HTTP request
        /// without consuming it. It uses Hyper's built-in method and URI parsing.
        async fn extract_route_info<'req>(
            &self,
            request: &'req Request<Incoming>,
        ) -> Result<(&'req str, &'req str), RouterError> {
            let method = request.method().as_str();
            let path = request.uri().path();
            Ok((path, method))
        }

        /// Creates an Exchange from a Hyper HTTP request.
        ///
        /// # Parameters
        ///
        /// * `request` - The Hyper HTTP request to be converted (takes ownership)
        ///
        /// # Returns
        ///
        /// Returns an `Exchange<Bytes, BoxBody<Bytes, std::io::Error, Parts>` configured with:
        /// * HTTP request body as a byte stream with BytesCollector
        /// * HTTP request parts (headers, method, URI) as metadata
        /// * Proper initialization for HTTP request processing
        ///
        /// # Behavior
        ///
        /// This method decomposes the HTTP request into parts and body, converts the body
        /// into a streaming format compatible with the exchange system, and preserves
        /// all HTTP metadata. Any streaming errors are converted to router errors.
        /// The body stream is processed lazily and can handle large payloads efficiently.
        async fn create_exchange<'req>(
            &self,
            request: Request<Incoming>,
        ) -> Result<Exchange<'req, Bytes, BoxBody<Bytes, std::io::Error>, Parts>, RouterError> {
            let mut exchange = Exchange::new();
            let (parts, body) = request.into_parts();

            // Create a stream from the hyper body
            let body_stream = body
                .into_data_stream()
                .map(|chunk| {
                    match chunk {
                        Ok(data) => Ok(data),
                        Err(e) => Err(Box::new(e) as Box<dyn Error + Send + Sync>)
                    }
                });

            // Set the input stream with a BytesCollector
            exchange.set_input_stream_with_collector(
                Box::pin(body_stream),
                Box::new(HyperBytesCollector),
            );

            exchange.set_metadata(parts);
            Ok(exchange)
        }
    }
}