use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};
use idemio::handler::Handler;
use idemio::handler::registry::HandlerRegistry;
use idemio::router::exchange::Exchange;
use idemio::router::route::{PathChain, PathConfig, PathRouter};
use idemio::status::{ExchangeState, HandlerExecutionError, HandlerStatus};
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;
use idemio::handler::config::HandlerId;
// This is a simple benchmark to aid development of the router

#[derive(Debug)]
struct DummyHandler;

#[async_trait]
impl Handler<(), (), ()> for DummyHandler {
    async fn exec(
        &self,
        _exchange: &mut Exchange<(), (), ()>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        Ok(HandlerStatus::new(ExchangeState::OK))
    }

    fn name(&self) -> &str {
        "dummy"
    }
}

fn create_populated_dynamic_route_table_v2(num_routes: usize) -> PathRouter<(), (), ()> {
    let mut registry = HandlerRegistry::new();
    registry
        .register_handler(&HandlerId::new("test1"), Arc::new(DummyHandler))
        .unwrap();
    registry
        .register_handler(&HandlerId::new("test2"), Arc::new(DummyHandler))
        .unwrap();
    registry
        .register_handler(&HandlerId::new("test3"), Arc::new(DummyHandler))
        .unwrap();
    registry
        .register_handler(&HandlerId::new("test4"), Arc::new(DummyHandler))
        .unwrap();

    let mut paths = HashMap::new();
    for i in 1..num_routes {
        let mut methods = HashMap::new();
        let path = format!("/test/{}", i);
        match i % 4 {
            0 => {
                let chain = PathChain {
                    request_handlers: vec!["test1".to_string(), "test2".to_string(), "test3".to_string()],
                    termination_handler: "test4".to_string(),
                    response_handlers: vec![]
                };
                methods.insert(
                    "GET".to_string(),
                    chain,
                );
            }
            _ => {
                let chain = PathChain {
                    request_handlers: vec!["test1".to_string()],
                    termination_handler: "test4".to_string(),
                    response_handlers: vec![]
                };
                methods.insert(
                    "POST".to_string(),
                    chain,
                );
            }
        }
        paths.insert(path, methods);
    }

    let path = "/test/abc/*".to_string();
    let chain = PathChain {
        request_handlers: vec!["test1".to_string(), "test2".to_string(), "test3".to_string()],
        termination_handler: "test4".to_string(),
        response_handlers: vec!["test1".to_string(), "test2".to_string(), "test3".to_string()],
    };
    let mut methods = HashMap::new();
    methods.insert(
        "GET".to_string(),
        chain,
    );
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


criterion_group!(
    benches,
    bench_dynamic_route_table
);
criterion_main!(benches);
