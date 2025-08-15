use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};
use idemio::exchange::Exchange;
use idemio::handler::registry::HandlerRegistry;
use idemio::handler::{Handler, HandlerId};
use idemio::router::config::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
};
use idemio::router::route::PathMatcher;
use idemio::status::{ExchangeState, HandlerStatus};
use std::convert::Infallible;
use std::hint::black_box;
// This is a simple benchmark to aid development of the router

#[derive(Debug)]
struct DummyHandler;

#[async_trait]
impl Handler<(), (), ()> for DummyHandler {
    async fn exec<'a>(
        &self,
        _exchange: &mut Exchange<'a, (), (), ()>,
    ) -> Result<HandlerStatus, Infallible> {
        Ok(HandlerStatus::new(ExchangeState::OK))
    }

    fn name(&self) -> &str {
        "dummy"
    }
}

fn create_populated_dynamic_route_table_v2(num_routes: usize) -> PathMatcher<(), (), ()> {
    let mut registry = HandlerRegistry::new();
    registry
        .register_handler(HandlerId::new("test1"), DummyHandler)
        .unwrap();
    registry
        .register_handler(HandlerId::new("test2"), DummyHandler)
        .unwrap();
    registry
        .register_handler(HandlerId::new("test3"), DummyHandler)
        .unwrap();
    registry
        .register_handler(HandlerId::new("test4"), DummyHandler)
        .unwrap();

    let mut builder = SingleServiceConfigBuilder::new();

    // Build routes using the service builder pattern
    for i in 1..num_routes {
        let path = format!("/test/{}", i);
        match i % 4 {
            0 => {
                builder = builder
                    .route(path)
                    .get()
                    .request_handlers(&["test1", "test2", "test3"])
                    .termination_handler("test4")
                    .end_method()
                    .end_route();
            }
            _ => {
                builder = builder
                    .route(path)
                    .post()
                    .request_handler("test1")
                    .termination_handler("test4")
                    .end_method()
                    .end_route();
            }
        }
    }

    // Add the wildcard route
    builder = builder
        .route("/test/abc/*")
        .get()
        .request_handlers(&["test1", "test2", "test3"])
        .termination_handler("test4")
        .response_handlers(&["test1", "test2", "test3"])
        .end_method()
        .end_route();

    let config = builder.build();
    PathMatcher::new(&config, &registry).unwrap()
}

fn bench_dynamic_route_table(c: &mut Criterion) {
    let table = create_populated_dynamic_route_table_v2(1000);

    c.bench_function("dynamic_route_table_v2", |b| {
        b.iter(|| {
            black_box(table.lookup("/test/12345", "GET"));
        });
    });
}

criterion_group!(benches, bench_dynamic_route_table);
criterion_main!(benches);
