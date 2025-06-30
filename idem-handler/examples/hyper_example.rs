use std::convert::Infallible;
use std::net::{SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use hyper::body::{Bytes};
use serde::{Deserialize, Serialize};

use idem_handler::config::config::{HandlerConfig, HandlerConfigBuilder};
use idem_handler::exchange::Exchange;
use idem_handler::handler::{ConfigurableHandler, Handler};
use idem_handler::router::{DefaultRouter, ExchangeFactory, HandlerId, Router, RouterConfig, RouteInfo};
use idem_handler::status::{Code, HandlerExecutionError, HandlerStatus};

#[derive(Default, Deserialize, Serialize)]
struct HandlerDefaultConfig {}

// Handler for greeting endpoint
struct GreetingHandler;

#[async_trait]
impl Handler<Bytes, Bytes, ()> for GreetingHandler {
    async fn exec(
        &self,
        exchange: &mut Exchange<Bytes, Bytes, ()>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        // Get input from the exchange
        let input = exchange.take_request().map_err(|e| HandlerExecutionError {
            message: format!("Failed to get input: {}", e).into(),
        })?;
        
        let input_str = String::from_utf8_lossy(&input).to_string();

        // Process the request and create a response
        let response = format!("Hello, {}!", input_str);
        
        let response_bytes = Bytes::from(response.into_bytes());

        // Save the output to the exchange
        exchange.save_output(response_bytes);

        // Return OK status
        Ok(HandlerStatus::new(Code::OK | Code::REQUEST_COMPLETED))
    }
}

// Handler for echo endpoint
struct EchoHandler;

#[async_trait]
impl Handler<Bytes, Bytes, ()> for EchoHandler {
    async fn exec(
        &self,
        exchange: &mut Exchange<Bytes, Bytes, ()>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        // Get input from the exchange
        let input = exchange.take_request().map_err(|e| HandlerExecutionError {
            message: format!("Failed to get input: {}", e).into(),
        })?;

        // Just echo back the input
        let input_str = String::from_utf8_lossy(&input).to_string();
        let response = format!("Echo: {}", input_str);
        
        let response_bytes = Bytes::from(response.into_bytes());

        // Save the output to the exchange
        exchange.save_output(response_bytes);

        // Return OK status
        Ok(HandlerStatus::new(Code::OK | Code::REQUEST_COMPLETED))
    }
}

// Handler for time endpoint
struct TimeHandler;

#[async_trait]
impl Handler<Bytes, Bytes, ()> for TimeHandler {
    async fn exec(
        &self,
        exchange: &mut Exchange<Bytes, Bytes, ()>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        // Get the current time
        let now = chrono::Local::now();
        let formatted_time = now.format("%Y-%m-%d %H:%M:%S").to_string();

        let response = format!("Current time: {}", formatted_time);
        let response_bytes = Bytes::from(response.into_bytes());
        // Save the output to the exchange
        exchange.save_output(response_bytes);

        // Return OK status
        Ok(HandlerStatus::new(Code::OK | Code::REQUEST_COMPLETED))
    }
}

// Custom exchange factory for Hyper requests
pub struct HyperExchangeFactory;

#[async_trait]
impl ExchangeFactory<Request<Body>, Bytes, Bytes, ()> for HyperExchangeFactory {
    async fn extract_route_info(&self, request: &Request<Body>) -> Result<RouteInfo, idem_handler::router::RouterError> {
        // Extract method and path from Hyper request
        let method = request.method().to_string();
        let path = request.uri().path().to_string();

        Ok(RouteInfo::new(path, method))
    }

    async fn create_exchange(&self, request: Request<Body>) -> Result<Exchange<Bytes, Bytes, ()>, idem_handler::router::RouterError> {
        // Create a new exchange
        let mut exchange = Exchange::new();
        

        // Extract body bytes asynchronously
        let body_bytes = hyper::body::to_bytes(request.into_body())
            .await
            .map_err(|e| idem_handler::router::RouterError::ExchangeCreationFailed(e.to_string()))?;

        // Convert bytes to string and save as input
        exchange.save_input(body_bytes);

        Ok(exchange)
    }
}

// Create a router with appropriate handlers and routes
fn create_router() -> DefaultRouter<Request<Body>, Bytes, Bytes, (), HyperExchangeFactory> {
    // Define the chain configuration
    let mut chains = std::collections::HashMap::new();

    // Define the path configuration
    let mut paths = std::collections::HashMap::new();

    // Configure greeting endpoint
    let mut greeting_config = idem_handler::router::PathConfig {
        methods: std::collections::HashMap::new(),
    };
    greeting_config.methods.insert(
        "GET".to_string(),
        vec![idem_handler::router::Executable::Handler(HandlerId("greeting_handler".to_string()))],
    );
    paths.insert("/greet".to_string(), greeting_config);

    // Configure echo endpoint
    let mut echo_config = idem_handler::router::PathConfig {
        methods: std::collections::HashMap::new(),
    };
    echo_config.methods.insert(
        "POST".to_string(),
        vec![idem_handler::router::Executable::Handler(HandlerId("echo_handler".to_string()))],
    );
    paths.insert("/echo".to_string(), echo_config);

    // Configure time endpoint
    let mut time_config = idem_handler::router::PathConfig {
        methods: std::collections::HashMap::new(),
    };
    time_config.methods.insert(
        "GET".to_string(),
        vec![idem_handler::router::Executable::Handler(HandlerId("time_handler".to_string()))],
    );
    paths.insert("/time".to_string(), time_config);

    // Create router configuration
    let router_config = RouterConfig { chains, paths };

    // Create the router
    let mut router = DefaultRouter::new(router_config, HyperExchangeFactory);

    // Create handler configurations
    let mut greeting_handler_config = HandlerConfigBuilder::new();
    greeting_handler_config
        .id("greeting_handler".to_string())
        .enabled(true);
    
    let greeting_handler_config: HandlerConfig<HandlerDefaultConfig> = greeting_handler_config.build();
    // Add handlers to the router
    router.add_handler(
        HandlerId("greeting_handler".to_string()),
        Arc::new(ConfigurableHandler::new(
            Arc::new(GreetingHandler),
            greeting_handler_config,
        )),
    );

    let mut echo_handler_config = HandlerConfigBuilder::new();
    echo_handler_config
        .id("echo_handler".to_string())
        .enabled(true);

    let echo_handler_config: HandlerConfig<HandlerDefaultConfig> = echo_handler_config.build();
    router.add_handler(
        HandlerId("echo_handler".to_string()),
        Arc::new(ConfigurableHandler::new(
            Arc::new(EchoHandler),
            echo_handler_config,
        )),
    );

    let mut time_handler_config = HandlerConfigBuilder::new();
    time_handler_config
        .id("time_handler".to_string())
        .enabled(true);

    let time_handler_config: HandlerConfig<HandlerDefaultConfig> = time_handler_config.build();
    router.add_handler(
        HandlerId("time_handler".to_string()),
        Arc::new(ConfigurableHandler::new(
            Arc::new(TimeHandler),
            time_handler_config,
        )),
    );

    router
}

async fn handle_request(
    req: Request<Body>,
    router: Arc<DefaultRouter<Request<Body>, Bytes, Bytes, (), HyperExchangeFactory>>,
) -> Result<Response<Body>, Infallible> {
    // Extract the path for logging
    let path = req.uri().path().to_string();
    println!("Received request: {} {}", req.method(), path);

    // Use the router to handle the request
    match router.route(req).await {
        Ok(response_body) => {
            // Create a successful HTTP response
            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "text/plain")
                .body(Body::from(response_body))
                .unwrap())
        }
        Err(e) => {
            // Handle routing errors
            println!("Error handling request: {}", e);
            let status_code = match e {
                idem_handler::router::RouterError::RouteNotFound => 404,
                idem_handler::router::RouterError::MethodNotSupported => 405,
                _ => 500,
            };

            // Create an error HTTP response
            Ok(Response::builder()
                .status(status_code)
                .header("Content-Type", "text/plain")
                .body(Body::from(format!("Error: {}", e)))
                .unwrap())
        }
    }
}

#[tokio::main]
async fn main() {
    // Create the router
    let router = Arc::new(create_router());

    // Configure the server address
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    // Create a service function that will handle requests
    let make_service = make_service_fn(move |_conn| {
        let router = router.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(req, router.clone())
            }))
        }
    });

    // Create and start the server
    let server = Server::bind(&addr).serve(make_service);

    println!("Server running on http://{}", addr);
    println!("Available endpoints:");
    println!("  GET  /greet?name=YourName - Returns a greeting message");
    println!("  POST /echo - Echoes back the request body");
    println!("  GET  /time - Returns the current server time");

    // Run the server
    if let Err(e) = server.await {
        eprintln!("Server error: {}", e);
    }
}
