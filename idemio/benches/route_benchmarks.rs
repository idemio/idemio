use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;
use async_trait::async_trait;
use criterion::{criterion_group, criterion_main, Criterion};
use idemio::handler::{Handler, SharedHandler};
use idemio::handler::registry::HandlerRegistry;
use idemio::router::{route::DynamicRouteTable, config::PathSegment, config::RouteInfo, config::Executable, config::HandlerId, config::ChainId};
use idemio::router::exchange::Exchange;
use idemio::router::route::{PathConfig, PathRouter};
use idemio::status::{Code, HandlerExecutionError, HandlerStatus};

// This is a simple benchmark to aid development of the router

fn create_test_executables() -> Arc<Vec<Executable>> {
    Arc::new(vec![
        Executable::Handler(HandlerId::new("test_handler")),
        Executable::Chain(ChainId::new("@test_chain")),
    ])
}

#[derive(Debug)]
struct DummyHandler;

#[async_trait]
impl Handler<(), (), ()> for DummyHandler {
    async fn exec(&self, _exchange: &mut Exchange<(), (), ()>) -> Result<HandlerStatus, HandlerExecutionError> {
        Ok(HandlerStatus::new(Code::OK))
    }
}

fn create_populated_dynamic_route_table_v2(num_routes: usize) -> PathRouter<(), (), ()> {
    let mut registry = HandlerRegistry::new();
    registry.register_handler("test1", Arc::new(DummyHandler)).unwrap();
    registry.register_handler("test2", Arc::new(DummyHandler)).unwrap();
    registry.register_handler("test3", Arc::new(DummyHandler)).unwrap();
    registry.register_handler("test4", Arc::new(DummyHandler)).unwrap();

    let mut paths = HashMap::new();
    for i in 0..num_routes {
        let mut methods = HashMap::new();
        let path = format!("/test/{}", i);
        match i % 4 {
            0 => {
                methods.insert("GET".to_string(), vec![
                    "test1".to_string(),
                    "test2".to_string(),
                    "test3".to_string(),
                    "test4".to_string()
                ]);
            },
            _ => {
                methods.insert("POST".to_string(), vec![
                    "test1".to_string(),
                    "test2".to_string(),
                    "test3".to_string(),
                    "test4".to_string()
                ]);
            }
        }
        paths.insert(path, methods);
    }
    
    let config = PathConfig {
        chains: HashMap::new(),
        paths
    };
    PathRouter::new(&config, &registry)
}

fn create_populated_dynamic_route_table(num_routes: usize) -> DynamicRouteTable<(), (), ()> {
    let table = DynamicRouteTable::new();
    let executables = create_test_executables();

    let static_handlers: Vec<SharedHandler<(), (), ()>> = vec![Arc::new(DummyHandler)];
    
    // Add some static routes
    table.add_static_route(
        "GET".to_string(),
        "/api/static".to_string(),
        static_handlers.clone(),
    );

    // Add some dynamic routes
    for i in 0..num_routes - 1 {
        let path_segments = vec![
            PathSegment::Static("api".to_string()),
            PathSegment::Static(format!("dynamic{}", i)),
            PathSegment::Any
        ];
        table.add_dynamic_route("GET".to_string(), path_segments, static_handlers.clone());
    }
    table
}

fn bench_dynamic_route_table_static_lookup(c: &mut Criterion) {
    let table = create_populated_dynamic_route_table(1000);
    let route_info = RouteInfo::new("/api/static/250".to_string(), "GET".to_string());

    c.bench_function("dynamic_route_table_static_lookup", |b| {
        b.iter(|| {
            black_box(table.lookup(&route_info));
        });
    });
}

fn bench_dynamic_route_table_dynamic_lookup(c: &mut Criterion) {
    let table = create_populated_dynamic_route_table(1000);
    let route_info = RouteInfo::new("/api/dynamic250/12345".to_string(), "GET".to_string());

    c.bench_function("dynamic_route_table_dynamic_lookup", |b| {
        b.iter(|| {
            black_box(table.lookup(&route_info));
        });
    });
}

fn bench_dynamic_route_table_v2(c: &mut Criterion) {
    let table = create_populated_dynamic_route_table_v2(1000);
    
    c.bench_function("dynamic_route_table_v2", |b| {
        b.iter(|| {
            black_box(table.lookup("/test/12345", "GET"))
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
        // ... imagine 998 more patterns here
        ("/api/users/500", "GET"),
    ];

    for (pattern, pattern_method) in &patterns {
        if *pattern_method == method && pattern.split('/').collect::<Vec<&str>>().join("/") == path_parts {
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
    bench_dynamic_route_table_static_lookup,
    bench_dynamic_route_table_dynamic_lookup,
    bench_traditional_path_matching
);
criterion_main!(benches);
