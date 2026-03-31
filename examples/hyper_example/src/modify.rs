use crate::HandlerId;
use async_trait::async_trait;
use idemio::exchange::Exchange;
use idemio::Handler;
use idemio::handler::{MiddlewareResult, MiddlewareHandler};
use idemio::handler::LabeledHandler;
use crate::HyperRequest;

#[derive(Handler)]
pub struct TransformBodyHandler;
#[async_trait]
impl MiddlewareHandler<HyperRequest> for TransformBodyHandler {
    async fn exec(&self, exchange: &mut Exchange<HyperRequest>) -> MiddlewareResult {
        let body = exchange.data_mut().body_mut();
        
        todo!()
    }
}