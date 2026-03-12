pub mod registry;

use crate::config::ConfigProvider;
use crate::handler::registry::HandlerRegistry;
use crate::status::HandlerStatus;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub type SharedHandler<E> = Arc<dyn Handler<E>>;

#[async_trait]
pub trait Handler<E>: Send + Sync
where
    E: Send + Sync,
{
    async fn exec(&self, exchange: &mut E) -> Result<HandlerStatus, Infallible>;
}

//#[async_trait]
//impl Handler<Exchange<BoxBody<Bytes, std::io::Error>, BoxBody<Bytes, std::io::Error>, Parts>>
//for IdempotentLoggingHandler
//{
//    async fn exec(
//        &self,
//        _exchange: &mut Exchange<
//            BoxBody<Bytes, std::io::Error>,
//            BoxBody<Bytes, std::io::Error>,
//            Parts,
//        >,
//    ) -> Result<HandlerStatus, Infallible> {
//        println!("Processing request with idempotent logging handler");
//        Ok(HandlerStatus::new(ExchangeState::LIVE))
//    }
//}

#[macro_export]
macro_rules! idemio_handler {
    (
        $handler_struct:ty,
        $input:ty,
        $output:ty,
        $metadata:ty,
        |$handler:ident, $exchange:ident| $implementation:block
    ) => {
        #[async_trait]
        impl Handler<Exchange<$input, $output, $metadata>> for $handler_struct {
            async fn exec(&self, inner_exchange: &mut Exchange<$input, $output, $metadata>) -> Result<HandlerStatus, Infallible> {
                let $handler = &self;
                let $exchange = inner_exchange;
                $implementation
            }
        }
    };
}

// How to load a handler from a config provider and register it with the handler registry?
pub trait LoadableHandler<E>: Handler<E>
where
    E: Send + Sync,
{
    fn load_and_register<C>(
        registry: &mut HandlerRegistry<E>,
        config_provider: impl ConfigProvider<C>,
    ) -> Self
    where
        C: Default + DeserializeOwned;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandlerId {
    handler_hash: u64,
}

impl HandlerId {
    pub fn new(id: impl Into<String>) -> Self {
        let mut hasher = fnv::FnvHasher::default();
        id.into().hash(&mut hasher);
        let hash = hasher.finish();
        Self { handler_hash: hash }
    }
}

impl Display for HandlerId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.handler_hash)
    }
}
