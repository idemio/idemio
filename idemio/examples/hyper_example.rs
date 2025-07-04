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
use idemio::config::config::{
    Config, HandlerConfig, ProgrammaticConfigProvider,
};
use idemio::handler::Handler;
use idemio::handler::config::HandlerId;
use idemio::router::config::{ChainId, PathConfig};
use idemio::router::exchange::Exchange;
use idemio::router::factory::hyper::HyperExchangeFactory;
use idemio::router::{IdemioRouter, Router, config::RouterConfig};
use idemio::status::{Code, HandlerExecutionError, HandlerStatus};
use tokio::net::TcpListener;

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct IdempotentLoggingHandlerConfig;

#[derive(Debug)]
struct IdempotentLoggingHandler {
    config: HandlerConfig<IdempotentLoggingHandlerConfig>,
}

#[async_trait]
impl Handler<Bytes, Bytes, Parts> for IdempotentLoggingHandler {
    async fn exec(
        &self,
        exchange: &mut Exchange<Bytes, Bytes, Parts>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        println!("Print something here");
        Ok(HandlerStatus::new(Code::OK))
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
    async fn exec(
        &self,
        exchange: &mut Exchange<Bytes, Bytes, Parts>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        let input = exchange.take_input().map_err(|e| HandlerExecutionError {
            message: format!("Failed to get input: {}", e).into(),
        })?;
        let input_str = String::from_utf8_lossy(&input).to_string();
        let response = format!("Hello, {}!", input_str);
        let response_bytes = Bytes::from(response.into_bytes());
        exchange.save_output(response_bytes);
        Ok(HandlerStatus::new(Code::OK | Code::REQUEST_COMPLETED))
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct EchoHandlerConfig {
    reverse: bool,
}

#[derive(Debug)]
// Handler for echo endpoint
struct EchoHandler {
    config: HandlerConfig<EchoHandlerConfig>,
}

#[async_trait]
impl Handler<Bytes, Bytes, Parts> for EchoHandler {
    async fn exec(
        &self,
        exchange: &mut Exchange<Bytes, Bytes, Parts>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        let input = exchange.take_input().map_err(|e| HandlerExecutionError {
            message: format!("Failed to get input: {}", e).into(),
        })?;
        let input_str = String::from_utf8_lossy(&input).to_string();
        let response = format!("Echo: {}", input_str);
        let response_bytes = Bytes::from(response.into_bytes());
        exchange.save_output(response_bytes);
        Ok(HandlerStatus::new(Code::OK | Code::REQUEST_COMPLETED))
    }
}

// TODO - make it so that we can load these routes from a file (see config module + idemio-macro crate)
// Create a router with appropriate handlers and routes
fn create_router() -> IdemioRouter<Request<Incoming>, Bytes, Bytes, Parts, HyperExchangeFactory> {
    let mut handler_registry = idemio::handler::registry::HandlerRegistry::new();

    // Register greeting handler
    let greeting_handler_id = "greeting_handler";
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: GreetingHandlerConfig {
            response_text: "Hello, World!".to_string(),
        },
    })
    .unwrap();
    handler_config
        .id(greeting_handler_id.to_string())
        .inner_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<GreetingHandlerConfig> = handler_config.build();
    let handler = Arc::new(GreetingHandler {
        config: handler_config,
    });
    handler_registry
        .register_handler(greeting_handler_id, handler)
        .unwrap();

    // Register echo handler
    let echo_handler_id = "echo_handler";
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: EchoHandlerConfig { reverse: false },
    })
    .unwrap();
    handler_config
        .id(echo_handler_id.to_string())
        .inner_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<EchoHandlerConfig> = handler_config.build();
    let handler = Arc::new(EchoHandler {
        config: handler_config,
    });
    handler_registry
        .register_handler(echo_handler_id, handler)
        .unwrap();

    // Register idempotent logging handler
    let idempotent_logging_handler_id = "idempotent_logging_handler";
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: IdempotentLoggingHandlerConfig {},
    })
    .unwrap();
    handler_config
        .id(idempotent_logging_handler_id.to_string())
        .inner_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<IdempotentLoggingHandlerConfig> = handler_config.build();
    let handler = Arc::new(IdempotentLoggingHandler {
        config: handler_config,
    });
    handler_registry
        .register_handler(idempotent_logging_handler_id, handler)
        .unwrap();

    // Define the chain configuration
    let mut chains = HashMap::new();
    chains.insert(
        ChainId::new("@default"),
        vec![HandlerId::new("idempotent_logging_handler")],
    );

    // Define the path configuration
    let mut paths = HashMap::new();
    
    // Configure echo endpoint
    let mut path_config = PathConfig {
        methods: HashMap::new(),
    };
    path_config.methods.insert(
        "POST".to_string(),
        vec![
            idemio::router::config::Executable::Chain(ChainId::new("@default")),
            idemio::router::config::Executable::Handler(HandlerId::new("echo_handler")),
        ],
    );
    paths.insert("/echo".to_string(), path_config);
    
    // Configure greeting endpoint
    let mut path_config = PathConfig {
        methods: HashMap::new(),
    };
    path_config.methods.insert(
        "GET".to_string(),
        vec![
            idemio::router::config::Executable::Chain(ChainId::new("@default")),
            idemio::router::config::Executable::Handler(HandlerId::new("greeting_handler")),
        ],
    );
    paths.insert("/greet".to_string(), path_config);

    // Create router configuration
    let router_config = RouterConfig { chains, paths };

    // Create the router
    IdemioRouter::new(&handler_registry, &router_config, HyperExchangeFactory)
}

async fn handle_request(
    req: Request<Incoming>,
    router: Arc<IdemioRouter<Request<Incoming>, Bytes, Bytes, Parts, HyperExchangeFactory>>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, Box<dyn std::error::Error + Send + Sync>> {
    // Extract the path for logging
    let path = req.uri().path().to_string();
    println!("Received request: {} {}", req.method(), path);

    // Use the router to handle the request
    match router.route(req).await {
        Ok(response_body) => {
            let body = Full::new(Bytes::from(response_body)).boxed();
            // Create a successful HTTP response
            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "text/plain")
                .body(body)?)
        }
        Err(e) => {
            // Handle routing errors
            println!("Error handling request: {}", e);
            let status_code = match e {
                idemio::router::RouterError::RouteNotFound => 404,
                idemio::router::RouterError::MethodNotSupported => 405,
                _ => 500,
            };

            // Create an error HTTP response
            Ok(Response::builder()
                .status(status_code)
                .header("Content-Type", "text/plain")
                .body(Full::new(Bytes::from(format!("Error: {}", e))).boxed())?)
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let router = Arc::new(create_router());
    // This address is localhost
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    // Bind to the port and listen for incoming TCP connections
    let listener = TcpListener::bind(addr).await?;

    println!("Server running on http://{}", addr);
    println!("Available endpoints:");
    println!("  GET  /greet?name=YourName - Returns a greeting message");
    println!("  POST /echo - Echoes back the request body");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let router_clone = router.clone();
        tokio::task::spawn(async move {
            // Handle the connection from the client using HTTP/2 with an executor and pass any
            // HTTP requests received on that connection to the `hello` function
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
