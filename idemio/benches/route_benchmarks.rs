//use std::hint::black_box;
//use std::sync::Arc;
//use criterion::{criterion_group, criterion_main, Criterion};
//
//use idemio::router::{route::DynamicRouteTable, route::RouteTable, config::PathSegment, config::RouteInfo, config::Executable, config::HandlerId, config::ChainId};
//
//// Helper function to create test executables
//fn create_test_executables() -> Arc<Vec<Executable>> {
//    Arc::new(vec![
//        Executable::Handler(HandlerId::new("test_handler")),
//        Executable::Chain(ChainId::new("@test_chain")),
//    ])
//}
//
//// Helper function to create a route table with many routes
//fn create_populated_route_table(num_routes: usize) -> RouteTable {
//    let table = RouteTable::new();
//    let executables = create_test_executables();
//
//    for i in 0..num_routes {
//        table.insert(
//            "GET".to_string(),
//            format!("/api/users/{}", i),
//            executables.clone()
//        );
//    }
//
//    table
//}
//
//// Helper function to create a dynamic route table with many routes
//fn create_populated_dynamic_route_table(num_routes: usize) -> DynamicRouteTable {
//    let table = DynamicRouteTable::new();
//    let executables = create_test_executables();
//
//    // Add some static routes
//    for i in 0..num_routes / 2 {
//        table.add_static_route(
//            "GET".to_string(),
//            format!("/api/static/{}", i),
//            executables.clone()
//        );
//    }
//
//    // Add some dynamic routes
//    for i in 0..num_routes / 2 {
//        let path_segments = vec![
//            PathSegment::Static("api".to_string()),
//            PathSegment::Static(format!("dynamic{}", i)),
//            PathSegment::Any("*".to_string())
//        ];
//        table.add_dynamic_route("GET".to_string(), path_segments, executables.clone());
//    }
//
//    table
//}
//
//fn bench_route_table_lookup(c: &mut Criterion) {
//    let table = create_populated_route_table(1000);
//    let path = "/api/users/500"; // Middle of the range for average case
//
//    c.bench_function("route_table_lookup", |b| {
//        b.iter(|| {
//            black_box(table.get("GET", path));
//        });
//    });
//}
//
//fn bench_dynamic_route_table_static_lookup(c: &mut Criterion) {
//    let table = create_populated_dynamic_route_table(1000);
//    let route_info = RouteInfo::new("/api/static/250".to_string(), "GET".to_string());
//
//    c.bench_function("dynamic_route_table_static_lookup", |b| {
//        b.iter(|| {
//            black_box(table.lookup(&route_info));
//        });
//    });
//}
//
//fn bench_dynamic_route_table_dynamic_lookup(c: &mut Criterion) {
//    let table = create_populated_dynamic_route_table(1000);
//    let route_info = RouteInfo::new("/api/dynamic250/12345".to_string(), "GET".to_string());
//
//    c.bench_function("dynamic_route_table_dynamic_lookup", |b| {
//        b.iter(|| {
//            black_box(table.lookup(&route_info));
//        });
//    });
//}
//
//// Traditional path matching benchmark for comparison
//fn match_traditional_trie(segments: &[&str], method: &str) -> bool {
//    // Simplified version of traditional matching logic
//    let path_parts = segments.join("/");
//
//    // This is a simple linear search through all patterns
//    // Actual trie implementations would be more complex but follow similar principles
//    let patterns = [
//        ("/api/users/1", "GET"),
//        ("/api/users/2", "GET"),
//        // ... imagine 998 more patterns here
//        ("/api/users/500", "GET"),
//    ];
//
//    for (pattern, pattern_method) in &patterns {
//        if *pattern_method == method && pattern.split('/').collect::<Vec<&str>>().join("/") == path_parts {
//            return true;
//        }
//    }
//
//    false
//}
//
//fn bench_traditional_path_matching(c: &mut Criterion) {
//    let segments = ["api", "users", "500"];
//
//    c.bench_function("traditional_path_matching", |b| {
//        b.iter(|| {
//            black_box(match_traditional_trie(&segments, "GET"));
//        });
//    });
//}
//
//criterion_group!(
//    benches,
//    bench_route_table_lookup,
//    bench_dynamic_route_table_static_lookup,
//    bench_dynamic_route_table_dynamic_lookup,
//    bench_traditional_path_matching
//);
//criterion_main!(benches);
