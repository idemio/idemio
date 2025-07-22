use crate::router::RouterError;
use async_trait::async_trait;

#[async_trait]
pub trait ExchangeFactory<Request, Input, Output, Metadata, ExchangeType>
where
    Self: Send + Sync,
    Request: Send + Sync,
    Input: Send + Sync,
    Output: Send + Sync,
    Metadata: Send + Sync,
{
    async fn extract_route_info<'a>(
        &self,
        request: &'a Request,
    ) -> Result<(&'a str, &'a str), RouterError>;

    async fn create_exchange(&self, request: Request) -> Result<ExchangeType, RouterError>;
}

#[cfg(feature = "hyper")]
pub mod hyper {
    use futures_util::StreamExt;
use std::error::Error;
    use async_trait::async_trait;
    use http_body_util::BodyExt;
    use hyper::Request;
    use crate::exchange::Exchange;
    use crate::router::RouterError;
    use crate::router::factory::ExchangeFactory;
    use hyper::body::{Bytes, Incoming};
    use hyper::http::request::Parts;
    use crate::exchange::collector::hyper::BytesCollector;

    pub struct HyperExchangeFactory;

    #[async_trait]
    impl<'a> ExchangeFactory<
        Request<Incoming>,
        Bytes,
        Bytes,
        Parts,
        Exchange<'a, Bytes, Bytes, Parts>,
    > for HyperExchangeFactory
    {
        async fn extract_route_info<'b>(
            &self,
            request: &'b Request<Incoming>,
        ) -> Result<(&'b str, &'b str), RouterError> {
            let method = request.method().as_str();
            let path = request.uri().path();
            Ok((path, method))
        }

        async fn create_exchange(
            &self,
            request: Request<Incoming>,
        ) -> Result<Exchange<'a, Bytes, Bytes, Parts>, RouterError> {
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
                Box::new(BytesCollector),
            );

            exchange.set_metadata(parts);
            Ok(exchange)
        }

    }
}

