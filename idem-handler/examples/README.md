# Idem Handler Examples

This directory contains examples demonstrating how to use the Idem Handler library.

## Hyper HTTP Server Example

The `hyper_example.rs` demonstrates how to integrate Idem Handler with the Hyper HTTP server library.

To run the example:

```bash
cargo run --example hyper_example
```

Once the server is running, you can test it with these requests:

1. **Greeting endpoint**: `GET http://localhost:3000/greet?name=World`

   ```bash
   curl "http://localhost:3000/greet?name=World"
   # Response: Hello, World!
   ```

2. **Echo endpoint**: `POST http://localhost:3000/echo`

   ```bash
   curl -X POST -d "This is a test message" http://localhost:3000/echo
   # Response: Echo: This is a test message
   ```

3. **Time endpoint**: `GET http://localhost:3000/time`

   ```bash
   curl http://localhost:3000/time
   # Response: Current time: 2025-06-30 14:30:45
   ```

## Route Table Example

The `route_table_example.rs` demonstrates how the route table system works for fast route lookups.

To run the example:

```bash
cargo run --example route_table_example
```

This example shows how the pre-computed route tables provide significant performance improvements for applications with many routes.
