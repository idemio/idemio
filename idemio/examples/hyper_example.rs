
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::http::request::Parts;
use hyper::server::conn::http2;
use hyper::service::service_fn;
use hyper::{Request, Response};
use serde::{Deserialize, Serialize};

use hyper_util::rt::{TokioExecutor, TokioIo};
use idemio::config::{Config, HandlerConfig, ProgrammaticConfigProvider};
use idemio::exchange::Exchange;
use idemio::handler::Handler;
use idemio::handler::config::HandlerId;
use idemio::router::{IdemioRouter, Router};
use idemio::router::executor::DefaultExecutor;
use idemio::router::factory::hyper::HyperExchangeFactory;
use idemio::router::route::{PathConfig, PathChain};
use idemio::status::{ExchangeState, HandlerExecutionError, HandlerStatus};
use tokio::net::TcpListener;

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct IdempotentLoggingHandlerConfig;

#[derive(Debug)]
struct IdempotentLoggingHandler;

#[async_trait]
impl Handler<Bytes, Bytes, Parts> for IdempotentLoggingHandler {
    async fn exec<'a>(
        &self,
        _exchange: &mut Exchange<'a, Bytes, Bytes, Parts>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        println!("Processing request with idempotent logging handler");
        Ok(HandlerStatus::new(ExchangeState::OK))
    }

    fn name(&self) -> &str {
        "idempotent_logging_handler"
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct GreetingHandlerConfig {
    response_text: String,
}

#[derive(Debug)]
struct GreetingHandler {
    config: HandlerConfig<GreetingHandlerConfig>,
}

#[async_trait]
impl Handler<Bytes, Bytes, Parts> for GreetingHandler {
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, Bytes, Bytes, Parts>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        let input = exchange
            .take_input()
            .await
            .map_err(|e| HandlerExecutionError {
                message: format!("Failed to get input: {}", e).into(),
            })?;
        let input_str = String::from_utf8_lossy(&input).to_string();
        let response_text = &self.config.config().get().response_text;
        let response = if input_str.trim().is_empty() {
            response_text.clone()
        } else {
            format!("{} {}", response_text, input_str.trim())
        };
        let response_bytes = Bytes::from(response.into_bytes());
        exchange.save_output(response_bytes);
        Ok(HandlerStatus::new(
            ExchangeState::OK | ExchangeState::REQUEST_COMPLETED,
        ))
    }

    fn name(&self) -> &str {
        "greeting_handler"
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct EchoHandlerConfig {
    reverse: bool,
}

#[derive(Debug)]
struct EchoHandler {
    config: HandlerConfig<EchoHandlerConfig>,
}

#[async_trait]
impl Handler<Bytes, Bytes, Parts> for EchoHandler {
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, Bytes, Bytes, Parts>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        let input = exchange
            .take_input()
            .await
            .map_err(|e| HandlerExecutionError {
                message: format!("Failed to get input: {}", e).into(),
            })?;
        let input_str = String::from_utf8_lossy(&input).to_string();

        let processed_input = if self.config.config().get().reverse {
            input_str.chars().rev().collect()
        } else {
            input_str
        };

        let response = format!("Echo: {}", processed_input);
        let response_bytes = Bytes::from(response.into_bytes());
        exchange.save_output(response_bytes);
        Ok(HandlerStatus::new(
            ExchangeState::OK | ExchangeState::REQUEST_COMPLETED,
        ))
    }

    fn name(&self) -> &str {
        "echo_handler"
    }
}

// Create a router with appropriate handlers and routes using the corrected PathRouter structure
fn create_router<'a>()
    -> IdemioRouter<'a, Request<Incoming>, Bytes, Bytes, Parts, HyperExchangeFactory, DefaultExecutor> {
    let mut handler_registry = idemio::handler::registry::HandlerRegistry::new();

    // Register greeting handler
    let greeting_handler_id = HandlerId::new("greeting_handler");
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: GreetingHandlerConfig {
            response_text: "Hello, World!".to_string(),
        },
    })
        .unwrap();
    handler_config
        .id(greeting_handler_id.to_string())
        .handler_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<GreetingHandlerConfig> = handler_config.build();
    let handler = GreetingHandler {
        config: handler_config,
    };
    handler_registry
        .register_handler(&greeting_handler_id, handler)
        .unwrap();

    // Register echo handler
    let echo_handler_id = HandlerId::new("echo_handler");
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: EchoHandlerConfig { reverse: false },
    })
        .unwrap();
    handler_config
        .id(echo_handler_id.to_string())
        .handler_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<EchoHandlerConfig> = handler_config.build();
    let handler = EchoHandler {
        config: handler_config,
    };
    handler_registry
        .register_handler(&echo_handler_id, handler)
        .unwrap();

    // Register idempotent logging handler
    let idempotent_logging_handler_id = HandlerId::new("idempotent_logging_handler");
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: IdempotentLoggingHandlerConfig {},
    })
        .unwrap();
    handler_config
        .id(idempotent_logging_handler_id.to_string())
        .handler_config(inner_config)
        .enabled(true);
    
    let handler = IdempotentLoggingHandler;
    handler_registry
        .register_handler(&idempotent_logging_handler_id, handler)
        .unwrap();

    // Create path configuration using the PathRouter structure
    let mut paths = HashMap::new();

    // Configure echo endpoint with POST method
    let mut echo_methods = HashMap::new();
    let echo_chain = PathChain {
        request_handlers: vec!["idempotent_logging_handler".to_string()],
        termination_handler: "echo_handler".to_string(),
        response_handlers: vec![],
    };
    echo_methods.insert("POST".to_string(), echo_chain);
    paths.insert("/echo".to_string(), echo_methods);

    // Configure greeting endpoint with GET method
    let mut greet_methods = HashMap::new();
    let greet_chain = PathChain {
        request_handlers: vec!["idempotent_logging_handler".to_string()],
        termination_handler: "greeting_handler".to_string(),
        response_handlers: vec![],
    };
    greet_methods.insert("GET".to_string(), greet_chain);
    paths.insert("/greet".to_string(), greet_methods);

    // Add wildcard route for demonstration
    let mut wildcard_methods = HashMap::new();
    let wildcard_chain = PathChain {
        request_handlers: vec!["idempotent_logging_handler".to_string()],
        termination_handler: "greeting_handler".to_string(),
        response_handlers: vec![],
    };
    wildcard_methods.insert("GET".to_string(), wildcard_chain);
    paths.insert("/api/*".to_string(), wildcard_methods);

    // Create router configuration matching PathRouter expectations
    let router_config = PathConfig {
        chains: HashMap::new(), // Not used in this PathRouter implementation
        paths,
    };

    // Create the router
    IdemioRouter::new(&handler_registry, &router_config, HyperExchangeFactory, DefaultExecutor)
}

async fn handle_request(
    req: Request<Incoming>,
    router: Arc<IdemioRouter<'_, Request<Incoming>, Bytes, Bytes, Parts, HyperExchangeFactory, DefaultExecutor>>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, Box<dyn std::error::Error + Send + Sync>> {
    // Extract the path for logging
    let path = req.uri().path().to_string();
    let method = req.method().to_string();
    println!("Received request: {} {}", method, path);

    // Use the router to handle the request
    match router.route(req).await {
        Ok(response_body) => {
            let body = Full::new(response_body).boxed();
            // Create a successful HTTP response
            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "text/plain")
                .header("X-Powered-By", "Idemio")
                .body(body)?)
        }
        Err(e) => {
            // Handle routing errors
            println!("Error handling request: {}", e);
            let (status_code, error_message) = match e {
                idemio::router::RouterError::RouteNotFound => (404, "Route not found"),
                idemio::router::RouterError::MethodNotSupported => (405, "Method not allowed"),
                idemio::router::RouterError::ExchangeCreationFailed(_) => (400, "Bad request"),
                idemio::router::RouterError::ExecutionFailed(_) => (500, "Internal server error"),
                _ => (500, "Internal server error"),
            };

            // Create an error HTTP response
            Ok(Response::builder()
                .status(status_code)
                .header("Content-Type", "text/plain")
                .body(Full::new(Bytes::from(format!("{}: {}", error_message, e))).boxed())?)
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize simple logging
    env_logger::init();

    let router = Arc::new(create_router());
    // This address is localhost
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    // Bind to the port and listen for incoming TCP connections
    let listener = TcpListener::bind(addr).await?;

    println!("🚀 Idemio Server running on http://{}", addr);
    println!("Available endpoints:");
    println!("  GET  /greet           - Returns a greeting message");
    println!("  POST /echo            - Echoes back the request body");
    println!("  GET  /api/*           - Wildcard route for any /api/ path");
    println!();
    println!("Examples:");
    println!("  curl http://127.0.0.1:3000/greet");
    println!("  curl -X POST -d 'Hello World' http://127.0.0.1:3000/echo");
    println!("  curl http://127.0.0.1:3000/api/anything");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let router_clone = router.clone();
        tokio::task::spawn(async move {
            // Handle the connection from the client using HTTP/2 with an executor
            if let Err(err) = http2::Builder::new(TokioExecutor::new())
                .serve_connection(
                    io,
                    service_fn(move |req| handle_request(req, router_clone.clone())),
                )
                .await
            {
                eprintln!("Error serving connection: {}", err);
            }
        });
    }
}