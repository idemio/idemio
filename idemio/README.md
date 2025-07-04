# Idem Handler

Common handler mechanism used in other idemio products.

## Features

- Flexible handler system for processing requests
- Configurable execution chains
- Retry and timeout capabilities
- Pre-compiled route tables for efficient routing

## Pre-Compiled Route Tables

The router now includes an optimization that pre-compiles routes into lookup tables for faster routing. This implementation offers several benefits:

- **Faster Route Lookups**: Uses efficient hash-based lookups instead of tree traversal
- **Support for Static and Dynamic Routes**: Handles both exact path matches and wildcard patterns
- **Concurrent Access**: Uses thread-safe data structures for high-performance in multi-threaded environments
- **Fallback Mechanism**: Maintains compatibility with the original routing algorithm as a fallback

### Performance Benefits

The pre-compiled route tables can provide significant performance improvements, especially for applications with many routes:

- O(1) average lookup time instead of O(n) for path matching
- Reduced CPU usage for high-traffic services
- Lower latency for request processing

### Usage

The optimization is automatically applied when using the `DefaultRouter`. Routes are pre-compiled when the router is initialized, and lookups use the optimized tables whenever possible.

See the `examples/route_table_example.rs` for a demonstration of the performance benefits.

## License

Apache-2.0
