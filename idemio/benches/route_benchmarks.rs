use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};
use idemio::handler::Handler;
use idemio::handler::registry::HandlerRegistry;
use idemio::router::exchange::Exchange;
use idemio::router::route::{PathConfig, PathRouter, RouteInfo};
use idemio::status::{Code, HandlerExecutionError, HandlerStatus};
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;

// This is a simple benchmark to aid development of the router

#[derive(Debug)]
struct DummyHandler;

#[async_trait]
impl Handler<(), (), ()> for DummyHandler {
    async fn exec(
        &self,
        _exchange: &mut Exchange<(), (), ()>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        Ok(HandlerStatus::new(Code::OK))
    }
}

fn create_populated_dynamic_route_table_v2(num_routes: usize) -> PathRouter<(), (), ()> {
    let mut registry = HandlerRegistry::new();
    registry
        .register_handler("test1", Arc::new(DummyHandler))
        .unwrap();
    registry
        .register_handler("test2", Arc::new(DummyHandler))
        .unwrap();
    registry
        .register_handler("test3", Arc::new(DummyHandler))
        .unwrap();
    registry
        .register_handler("test4", Arc::new(DummyHandler))
        .unwrap();

    let mut paths = HashMap::new();
    for i in 0..num_routes {
        let mut methods = HashMap::new();
        let path = format!("/test/{}", i);
        match i % 4 {
            0 => {
                methods.insert(
                    "GET".to_string(),
                    vec![
                        "test1".to_string(),
                        "test2".to_string(),
                        "test3".to_string(),
                        "test4".to_string(),
                    ],
                );
            }
            _ => {
                methods.insert(
                    "POST".to_string(),
                    vec![
                        "test1".to_string(),
                        "test2".to_string(),
                        "test3".to_string(),
                        "test4".to_string(),
                    ],
                );
            }
        }
        paths.insert(path, methods);
    }

    let config = PathConfig {
        chains: HashMap::new(),
        paths,
    };
    PathRouter::new(&config, &registry).unwrap()
}

fn bench_dynamic_route_table_v2(c: &mut Criterion) {
    let table = create_populated_dynamic_route_table_v2(1000);

    c.bench_function("dynamic_route_table_v2", |b| {
        b.iter(|| {
            black_box(table.lookup(RouteInfo::new("/test/12345", "GET")))
        });
    });
}

fn match_traditional_trie(segments: &[&str], method: &str) -> bool {
    // Simplified version of traditional matching logic
    let path_parts = segments.join("/");

    // This is a simple linear search through all patterns
    // Actual trie implementations would be more complex but follow similar principles
    let patterns = [
        ("/api/users/1", "GET"),
        ("/api/users/2", "GET"),
        ("/api/users/500", "GET"),
    ];

    for (pattern, pattern_method) in &patterns {
        if *pattern_method == method
            && pattern.split('/').collect::<Vec<&str>>().join("/") == path_parts
        {
            return true;
        }
    }

    false
}

fn bench_traditional_path_matching(c: &mut Criterion) {
    let segments = ["api", "users", "500"];

    c.bench_function("traditional_path_matching", |b| {
        b.iter(|| {
            black_box(match_traditional_trie(&segments, "GET"));
        });
    });
}

criterion_group!(
    benches,
    bench_dynamic_route_table_v2,
    bench_traditional_path_matching
);
criterion_main!(benches);
