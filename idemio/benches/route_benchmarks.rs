use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, Criterion};
use idemio::exchange::{Exchange, InnerData};
use idemio::handler::{
    HandlerError, MiddlewareResponse, HandlerId, HandlerRegistry, MiddlewareResult, LabeledHandler,
    MiddlewareHandler, TerminationHandler,
};
use idemio::router::{HttpPathMethodMatcher, RouteKey, RouteKeyMatcher};
use idemio::router::builder::{MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder};
use std::hint::black_box;
use idemio::Attachments;
use idemio_macro::Handler;

#[derive(Debug, Handler)]
struct DummyMiddlewareHandler;

#[async_trait]
impl MiddlewareHandler<()> for DummyMiddlewareHandler {
    async fn exec(&self, _exchange: &mut Exchange<()>) -> MiddlewareResult {
        MiddlewareResponse::ok()
    }
}
#[derive(Handler)]
struct DummyTerminationHandler;

#[async_trait]
impl TerminationHandler<(), ()> for DummyTerminationHandler {
    async fn exec(&self, data: InnerData<()>) -> Result<InnerData<()>, HandlerError> {
        Ok(InnerData::new(()))
    }
}

fn create_populated_dynamic_route_table_v2(num_routes: usize) -> HttpPathMethodMatcher<(), ()> {
    let mut registry = HandlerRegistry::new();
    registry
        .register_request_handler(HandlerId::new("test1req"), DummyMiddlewareHandler)
        .unwrap();
    registry
        .register_request_handler(HandlerId::new("test2req"), DummyMiddlewareHandler)
        .unwrap();
    registry
        .register_request_handler(HandlerId::new("test3req"), DummyMiddlewareHandler)
        .unwrap();
    registry
        .register_response_handler(HandlerId::new("test1res"), DummyMiddlewareHandler)
        .unwrap();
    registry
        .register_response_handler(HandlerId::new("test2res"), DummyMiddlewareHandler)
        .unwrap();
    registry
        .register_response_handler(HandlerId::new("test3res"), DummyMiddlewareHandler)
        .unwrap();
    registry
        .register_termination_handler(HandlerId::new("test4term"), DummyTerminationHandler)
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
                    .request_handlers(&["test1req", "test2req", "test3req"])
                    .termination_handler("test4term")
                    .response_handlers(&["test1res"])
                    .end_method()
                    .end_route();
            }
            _ => {
                builder = builder
                    .route(path)
                    .get()
                    .request_handler("test1req")
                    .termination_handler("test4term")
                    .response_handlers(&["test1res"])
                    .end_method()
                    .end_route();
            }
        }
    }

    // Add the wildcard route
    builder = builder
        .route("/test/abc/*")
        .get()
        .request_handlers(&["test1req", "test2req", "test3req"])
        .termination_handler("test4term")
        .response_handlers(&["test1res", "test2res", "test3res"])
        .end_method()
        .end_route();

    let config = builder.build();
    HttpPathMethodMatcher::new(&config, &registry).unwrap()
}

fn bench_dynamic_route_table(c: &mut Criterion) {
    let table = create_populated_dynamic_route_table_v2(1000);
    c.bench_function("dynamic_route_table_v2", |b| {
        b.iter(|| {
            black_box(table.lookup(RouteKey::new("GET", "/test/12345")));
        });
    });
}

criterion_group!(benches, bench_dynamic_route_table);
criterion_main!(benches);
