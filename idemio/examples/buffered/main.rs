use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::http::request::Parts;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use serde::{Deserialize, Serialize};

use hyper_util::rt::TokioIo;
use idemio::config::{Config, HandlerConfig, ProgrammaticConfigProvider};
use idemio::exchange::Exchange;
use idemio::handler::Handler;
use idemio::handler::HandlerId;
use idemio::handler::registry::HandlerRegistry;
use idemio::router::config::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
};
use idemio::router::executor::DefaultExecutor;
use idemio::router::factory::hyper::HyperExchangeFactory;
use idemio::router::path::PathMatcher;
use idemio::router::path::http::{PathPrefixMethodKey, PathPrefixMethodPathMatcher};
use idemio::router::{RequestRouter, Router, RouterComponents, RouterError};
use idemio::status::{ExchangeState, HandlerStatus};
use tokio::net::TcpListener;

// Define the RouterComponents implementation for Hyper
struct HyperComponents;

impl
    RouterComponents<
        PathPrefixMethodKey<'_>,
        Request<Incoming>,
        Bytes,
        BoxBody<Bytes, std::io::Error>,
        Parts,
    > for HyperComponents
{
    type PathMatcher = PathPrefixMethodPathMatcher<Bytes, BoxBody<Bytes, std::io::Error>, Parts>;
    type Factory = HyperExchangeFactory;
    type Executor = DefaultExecutor;
}

// Type alias for cleaner signatures
type HyperRouter<'a> = RequestRouter<
    PathPrefixMethodKey<'a>,
    Request<Incoming>,
    Bytes,
    BoxBody<Bytes, std::io::Error>,
    Parts,
    HyperComponents,
>;

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct IdempotentLoggingHandlerConfig;

#[derive(Debug)]
struct IdempotentLoggingHandler;

#[async_trait]
impl Handler<Bytes, BoxBody<Bytes, std::io::Error>, Parts> for IdempotentLoggingHandler {
    async fn exec<'a>(
        &self,
        _exchange: &mut Exchange<'a, Bytes, BoxBody<Bytes, std::io::Error>, Parts>,
    ) -> Result<HandlerStatus, Infallible> {
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
impl Handler<Bytes, BoxBody<Bytes, std::io::Error>, Parts> for GreetingHandler {
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, Bytes, BoxBody<Bytes, std::io::Error>, Parts>,
    ) -> Result<HandlerStatus, Infallible> {
        let input = match exchange.take_input().await {
            Ok(input) => input,
            Err(e) => {
                return Ok(HandlerStatus::new(ExchangeState::SERVER_ERROR)
                    .message(format!("Could not consume input from exchange: {}", e)));
            }
        };
        let input_str = String::from_utf8_lossy(&input).to_string();
        let response_text = &self.config.config().get().response_text;
        let response = if input_str.trim().is_empty() {
            response_text.clone()
        } else {
            format!("{} {}", response_text, input_str.trim())
        };
        let response_bytes = Bytes::from(response.into_bytes());
        exchange.set_output(
            Full::new(response_bytes)
                .map_err(|_| unreachable!("Infallible"))
                .boxed(),
        );
        Ok(HandlerStatus::new(
            ExchangeState::OK | ExchangeState::EXCHANGE_COMPLETED,
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
impl Handler<Bytes, BoxBody<Bytes, std::io::Error>, Parts> for EchoHandler {
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, Bytes, BoxBody<Bytes, std::io::Error>, Parts>,
    ) -> Result<HandlerStatus, Infallible> {
        let input = match exchange.take_input().await {
            Ok(input) => input,
            Err(e) => {
                return Ok(HandlerStatus::new(ExchangeState::SERVER_ERROR)
                    .message(format!("Could not consume input from exchange: {}", e)));
            }
        };
        let input_str = String::from_utf8_lossy(&input).to_string();

        let processed_input = if self.config.config().get().reverse {
            input_str.chars().rev().collect()
        } else {
            input_str
        };

        let response = format!("Echo: {}", processed_input);
        let response_bytes = Bytes::from(response.into_bytes());
        exchange.set_output(
            Full::new(response_bytes)
                .map_err(|_| unreachable!("Infallible"))
                .boxed(),
        );
        Ok(HandlerStatus::new(
            ExchangeState::OK | ExchangeState::EXCHANGE_COMPLETED,
        ))
    }

    fn name(&self) -> &str {
        "echo_handler"
    }
}

// Updated function with simplified return type and new constructor
#[rustfmt::skip]
fn create_router<'a>() -> HyperRouter<'a> {
    let mut handler_registry = HandlerRegistry::new();

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
        .register_handler(greeting_handler_id, handler)
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
        .register_handler(echo_handler_id, handler)
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
        .register_handler(idempotent_logging_handler_id, handler)
        .unwrap();

    let router_config = SingleServiceConfigBuilder::new()
        .route("/echo")
            .post()
                .request_handler("idempotent_logging_handler")
                .termination_handler("echo_handler")
            .end_method()
        .end_route()
        .route("/greet")
            .get()
                .request_handler("idempotent_logging_handler")
                .termination_handler("greeting_handler")
            .end_method()
        .end_route()
        .route("/api/*")
            .get()
                .request_handler("idempotent_logging_handler")
                .termination_handler("greeting_handler")
            .end_method()
        .end_route()
        .build();
    // Create the router using the new constructor signature
    let matcher = PathPrefixMethodPathMatcher::new(&router_config, &handler_registry).unwrap();
    RequestRouter::new(
        matcher,
        HyperExchangeFactory,
        DefaultExecutor,
    )
    .unwrap()
}

async fn handle_request(
    req: Request<Incoming>,
    router: Arc<HyperRouter<'_>>,
) -> Result<Response<BoxBody<Bytes, std::io::Error>>, Box<dyn std::error::Error + Send + Sync>> {
    // Extract the path for logging
    let path = req.uri().path().to_string();
    let method = req.method().to_string();
    println!("Received request: {} {}", method, path);

    // Use the router to handle the request
    match router.route(req).await {
        Ok(response_body) => {
            let body = response_body;
            // Create a successful HTTP response
            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "text/plain")
                .body(body)?)
        }
        Err(e) => {
            // Handle routing errors
            println!("Error handling request: {}", e);
            let (status_code, error_message) = match e {
                RouterError::MissingRoute { .. } => (404, "Route not found"),
                RouterError::InvalidExchange { .. } => (400, "Bad request"),
                _ => (500, "Internal server error"),
            };

            // Create an error HTTP response
            Ok(Response::builder()
                .status(status_code)
                .header("Content-Type", "text/plain")
                .body(
                    Full::new(Bytes::from(format!("{}: {}", error_message, e)))
                        .map_err(|_| unreachable!("Infallible"))
                        .boxed(),
                )?)
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

    println!("Idemio Server running on http://{}", addr);
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
            if let Err(err) = http1::Builder::new()
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
