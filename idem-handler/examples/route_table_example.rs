use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use idem_handler::router::route_table::{DynamicRouteTable, RouteTable};
use idem_handler::router::{Executable, HandlerId, ChainId, PathSegment, RouteInfo};

fn main() {
    println!("Route Table Example");

    // Create some sample executables
    let user_executables = Arc::new(vec![
        Executable::Handler(HandlerId("auth_handler".to_string())),
        Executable::Handler(HandlerId("user_handler".to_string())),
    ]);

    let product_executables = Arc::new(vec![
        Executable::Handler(HandlerId("auth_handler".to_string())),
        Executable::Handler(HandlerId("product_handler".to_string())),
    ]);

    let admin_executables = Arc::new(vec![
        Executable::Handler(HandlerId("auth_handler".to_string())),
        Executable::Chain(ChainId("@admin_chain".to_string())),
        Executable::Handler(HandlerId("admin_handler".to_string())),
    ]);

    // Example 1: Simple RouteTable for exact matches
    println!("\nExample 1: Simple RouteTable");
    let route_table = RouteTable::new();

    // Populate the table
    route_table.insert("GET".to_string(), "/api/users".to_string(), user_executables.clone());
    route_table.insert("GET".to_string(), "/api/products".to_string(), product_executables.clone());
    route_table.insert("GET".to_string(), "/api/admin".to_string(), admin_executables.clone());

    // Lookup examples
    println!("Lookup /api/users with GET: {:?}", route_table.get("GET", "/api/users").is_some());
    println!("Lookup /api/users with POST: {:?}", route_table.get("POST", "/api/users").is_some());
    println!("Lookup /api/unknown with GET: {:?}", route_table.get("GET", "/api/unknown").is_some());

    // Example 2: DynamicRouteTable for wildcard paths
    println!("\nExample 2: DynamicRouteTable");
    let dynamic_table = DynamicRouteTable::new();

    // Add static routes
    dynamic_table.add_static_route("GET".to_string(), "/api/static".to_string(), user_executables.clone());

    // Add dynamic routes with wildcards
    let user_id_path = vec![
        PathSegment::Static("api".to_string()),
        PathSegment::Static("users".to_string()),
        PathSegment::Any("*".to_string()) // User ID wildcard
    ];

    let product_id_path = vec![
        PathSegment::Static("api".to_string()),
        PathSegment::Static("products".to_string()),
        PathSegment::Any("*".to_string()) // Product ID wildcard
    ];

    dynamic_table.add_dynamic_route("GET".to_string(), user_id_path, user_executables.clone());
    dynamic_table.add_dynamic_route("GET".to_string(), product_id_path, product_executables.clone());

    // Lookup examples
    println!("Lookup /api/static with GET: {:?}", 
             dynamic_table.lookup(&RouteInfo::new("/api/static".to_string(), "GET".to_string())).is_some());

    println!("Lookup /api/users/123 with GET: {:?}", 
             dynamic_table.lookup(&RouteInfo::new("/api/users/123".to_string(), "GET".to_string())).is_some());

    println!("Lookup /api/products/456 with GET: {:?}", 
             dynamic_table.lookup(&RouteInfo::new("/api/products/456".to_string(), "GET".to_string())).is_some());

    println!("Lookup /api/unknown/789 with GET: {:?}", 
             dynamic_table.lookup(&RouteInfo::new("/api/unknown/789".to_string(), "GET".to_string())).is_some());

    // Example 3: Performance comparison
    println!("\nExample 3: Performance Comparison");
    performance_comparison();
}

fn performance_comparison() {
    // Create a large number of routes for testing
    const NUM_ROUTES: usize = 10000;

    // Create a simple route table
    let route_table = RouteTable::new();
    let executables = Arc::new(vec![Executable::Handler(HandlerId("test_handler".to_string()))]);

    println!("Populating tables with {} routes...", NUM_ROUTES);

    // Populate the table
    for i in 0..NUM_ROUTES {
        route_table.insert(
            "GET".to_string(), 
            format!("/api/resource/{}", i), 
            executables.clone()
        );
    }

    // Measure lookup time for the lookup table
    println!("Measuring lookup performance...");
    const NUM_LOOKUPS: usize = 100000;

    let start = Instant::now();
    for i in 0..NUM_LOOKUPS {
        let index = i % NUM_ROUTES;
        let _ = route_table.get("GET", &format!("/api/resource/{}", index));
    }
    let table_duration = start.elapsed();

    // Simulate traditional routing approach (linear search)
    let start = Instant::now();
    for i in 0..NUM_LOOKUPS {
        let index = i % NUM_ROUTES;
        let path = format!("/api/resource/{}", index);
        let method = "GET";

        // Simulate linear search through all routes
        let mut found = false;
        for j in 0..NUM_ROUTES {
            let test_path = format!("/api/resource/{}", j);
            if test_path == path && method == "GET" {
                found = true;
                break;
            }
        }
        assert!(found);
    }
    let linear_duration = start.elapsed();

    println!("Results for {} lookups in {} routes:", NUM_LOOKUPS, NUM_ROUTES);
    println!("  Hash-based route table: {:?}", table_duration);
    println!("  Traditional linear search: {:?}", linear_duration);
    println!("  Speed improvement: {:.2}x", linear_duration.as_nanos() as f64 / table_duration.as_nanos() as f64);
}
