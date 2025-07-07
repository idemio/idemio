use crate::router::exchange::Exchange;
use crate::router::{RouteInfo, RouterError};
use async_trait::async_trait;

#[async_trait]
pub trait ExchangeFactory<R, I, O, M>
where
    Self: Send + Sync,
    R: Send + Sync,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    async fn extract_route_info(&self, request: &R) -> Result<RouteInfo, RouterError>;
    
    async fn create_exchange(&self, request: R) -> Result<Exchange<I, O, M>, RouterError>;
}

#[cfg(feature = "hyper")]
pub mod hyper {
    use async_trait::async_trait;
    use http_body_util::BodyExt;
    use hyper::Request;

    use crate::router::RouterError;
    use crate::router::exchange::Exchange;
    use crate::router::factory::ExchangeFactory;
    use hyper::body::{Bytes, Incoming};
    use hyper::http::request::Parts;
    use crate::router::route::RouteInfo;

    pub struct HyperExchangeFactory;

    #[async_trait]
    impl ExchangeFactory<Request<Incoming>, Bytes, Bytes, Parts> for HyperExchangeFactory {
        async fn extract_route_info(
            &self,
            request: &Request<Incoming>,
        ) -> Result<RouteInfo, RouterError> {
            let method = request.method().as_str();
            let path = request.uri().path();
            Ok(RouteInfo::new(path, method))
        }

        async fn create_exchange(
            &self,
            request: Request<Incoming>,
        ) -> Result<Exchange<Bytes, Bytes, Parts>, RouterError> {
            let mut exchange = Exchange::new();
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
