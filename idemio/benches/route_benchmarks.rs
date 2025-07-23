use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};
use idemio::exchange::Exchange;
use idemio::handler::{Handler, HandlerId};
use idemio::handler::registry::HandlerRegistry;
use idemio::router::route::{PathChain, PathConfig, PathRouter};
use idemio::status::{ExchangeState, HandlerStatus};
use std::collections::HashMap;
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

fn create_populated_dynamic_route_table_v2(num_routes: usize) -> PathRouter<(), (), ()> {
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

    let mut paths = HashMap::new();
    for i in 1..num_routes {
        let mut methods = HashMap::new();
        let path = format!("/test/{}", i);
        match i % 4 {
            0 => {
                let mut chain = PathChain::new();
                chain
                    .add_request_handler("test1")
                    .add_request_handler("test2")
                    .add_request_handler("test3")
                    .set_termination_handler("test4");
                methods.insert("GET".to_string(), chain);
            }
            _ => {
                let mut chain = PathChain::new();
                chain
                    .add_request_handler("test1")
                    .set_termination_handler("test4");
                methods.insert("POST".to_string(), chain);
            }
        }
        paths.insert(path, methods);
    }

    let path = "/test/abc/*".to_string();
    let mut chain = PathChain::new();
    chain
        .add_request_handler("test1")
        .add_request_handler("test2")
        .add_request_handler("test3")
        .set_termination_handler("test4")
        .add_response_handler("test1")
        .add_response_handler("test2")
        .add_response_handler("test3");
    let mut methods = HashMap::new();
    methods.insert("GET".to_string(), chain);
    paths.insert(path, methods);

    let config = PathConfig {
        chains: HashMap::new(),
        paths,
    };
    PathRouter::new(&config, &registry).unwrap()
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
