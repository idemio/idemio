use crate::router::RouterError;
use async_trait::async_trait;

#[async_trait]
pub trait ExchangeFactory<Request, Input, Output, Metadata, ExchangeType>
where
    Self: Send + Sync,
    ExchangeType: Send + Sync,
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
    use async_trait::async_trait;
    use http_body_util::BodyExt;
    use hyper::Request;

    use crate::exchange::buffered::BufferedExchange;
    use crate::router::RouterError;
    use crate::router::factory::ExchangeFactory;
    use hyper::body::{Bytes, Incoming};
    use hyper::http::request::Parts;

    pub struct HyperExchangeFactory;

    #[async_trait]
    impl
        ExchangeFactory<
            Request<Incoming>,
            Bytes,
            Bytes,
            Parts,
            BufferedExchange<Bytes, Bytes, Parts>,
        > for HyperExchangeFactory
    {
        async fn extract_route_info<'a>(
            &self,
            request: &'a Request<Incoming>,
        ) -> Result<(&'a str, &'a str), RouterError> {
            let method = request.method().as_str();
            let path = request.uri().path();
            Ok((path, method))
        }

        async fn create_exchange(
            &self,
            request: Request<Incoming>,
        ) -> Result<BufferedExchange<Bytes, Bytes, Parts>, RouterError> {
            let mut exchange = BufferedExchange::new();
            let (parts, body) = request.into_parts();
            let collected = body.collect().await;
            let body_bytes = collected
                .map_err(|e| RouterError::ExchangeCreationFailed(e.to_string()))?
                .to_bytes();
            exchange.save_input(body_bytes);
            exchange.add_metadata(parts);
            Ok(exchange)
        }
    }
}
