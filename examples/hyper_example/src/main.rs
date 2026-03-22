use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::http::{request, response};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use idemio::config::{Config, HandlerConfig, ProgrammaticConfigProvider};
use idemio::exchange::Exchange;
use idemio::handler::registry::HandlerRegistry;
use idemio::handler::HandlerId;
use idemio::handler::{Handler, HandlerFlow, HandlerResponse};
use idemio::router::config::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
};
use idemio::router::factory::{ExchangeFactory, RouteInfo};
use idemio::router::path::http::HttpPathMethodMatcher;
use idemio::router::path::PathMatcher;
use idemio::router::{Router, RouterError};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
pub struct HyperExchangeFactory;
type HyperRequest = Request<BoxBody<Bytes, std::io::Error>>;
type HyperResponse = Response<BoxBody<Bytes, std::io::Error>>;

async fn request_into_parts(
    exchange: &mut Exchange<HyperRequest, HyperResponse>,
) -> (request::Parts, BoxBody<Bytes, std::io::Error>) {
    match exchange.take_input().await {
        Ok(input) => input.into_parts(),
        Err(e) => {
            todo!()
        }
    }
}

async fn response_into_parts(
    exchange: &mut Exchange<HyperRequest, HyperResponse>,
) -> (response::Parts, BoxBody<Bytes, std::io::Error>) {
    match exchange.take_output().await {
        Ok(output) => output.into_parts(),
        Err(e) => {
            todo!()
        }
    }
}

async fn collect_body(body: BoxBody<Bytes, std::io::Error>) -> Bytes {
    match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            log::error!("Could not collect body: {}", e);
            Bytes::new()
        }
    }
}

impl ExchangeFactory<HyperRequest, HyperResponse> for HyperExchangeFactory {
    /// Extracts HTTP method and path from a Hyper request.
    fn extract_route_info<'a>(&self, request: &'a HyperRequest) -> RouteInfo<'a> {
        RouteInfo::new(request.uri().path(), request.method().as_str())
    }

    fn create_exchange<'req>(
        &self,
        request: HyperRequest,
    ) -> Exchange<HyperRequest, HyperResponse> {
        let mut exchange = Exchange::new();
        exchange.set_input(request);
        exchange
    }
}

// Simplified type alias for the complete router
type HyperRouter = idemio::router::Router<
    HyperRequest,
    HyperResponse,
    HyperExchangeFactory,
    HttpPathMethodMatcher<HyperRequest, HyperResponse>,
>;

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct IdempotentLoggingHandlerConfig;

#[derive(Debug)]
struct IdempotentLoggingHandler;
#[async_trait]
impl<I, O> Handler<I, O> for IdempotentLoggingHandler
where
    I: Send + Sync,
    O: Send + Sync,
{
    fn id(&self) -> &'static str {
        "IdempotentLoggingHandler"
    }

    async fn exec(&self, exchange: &mut Exchange<I, O>) -> HandlerResponse {
        log::info!("uuid={}", exchange.uuid().to_string());
        HandlerFlow::ok()
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
impl Handler<HyperRequest, HyperResponse> for GreetingHandler {
    fn id(&self) -> &'static str {
        "GreetingHandler"
    }

    async fn exec(&self, exchange: &mut Exchange<HyperRequest, HyperResponse>) -> HandlerResponse {
        let (parts, body) = request_into_parts(exchange).await;
        let input_bytes = collect_body(body).await;

        let input_str = String::from_utf8_lossy(&input_bytes).to_string();
        let response_text = &self.config.config().get().response_text;
        let response = if input_str.trim().is_empty() {
            response_text.clone()
        } else {
            format!("{} {}", response_text, input_str.trim())
        };
        let response_bytes = Bytes::from(response.into_bytes());
        let body = Full::new(response_bytes)
            .map_err(|_| unreachable!("Infallible"))
            .boxed();
        let response = response::Builder::new()
            .status(StatusCode::OK)
            .body(body)
            .unwrap();
        exchange.set_output(response);
        HandlerFlow::ok()
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
impl Handler<HyperRequest, HyperResponse> for EchoHandler {
    fn id(&self) -> &'static str {
        "EchoHandler"
    }

    async fn exec(&self, exchange: &mut Exchange<HyperRequest, HyperResponse>) -> HandlerResponse {
        let (parts, body) = request_into_parts(exchange).await;
        let input_bytes = collect_body(body).await;
        let input_str = String::from_utf8_lossy(&input_bytes).to_string();
        let processed_input = if self.config.config().get().reverse {
            input_str.chars().rev().collect()
        } else {
            input_str
        };

        let response = format!("Echo: {}", processed_input);
        let response_bytes = Bytes::from(response.into_bytes());
        let mut response_header = HeaderMap::new();
        let content_type: &str = parts
            .headers
            .iter()
            .find_map(|(header, val)| {
                if header.to_string().to_lowercase() != "content-type" {
                    if let Ok(content_type) = val.to_str() {
                        return Some(content_type);
                    }
                }
                None
            })
            .unwrap_or("text/plain");
        response_header.insert("Content-Type", content_type.parse().unwrap());
        let body = Full::new(response_bytes)
            .map_err(|_| unreachable!(""))
            .boxed();
        let response = response::Builder::new()
            .status(StatusCode::OK)
            .body(body)
            .unwrap();
        exchange.set_output(response);
        HandlerFlow::ok()
    }
}

// Updated function using the new RouterBuilder
fn create_router() -> HyperRouter {
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
        config: EchoHandlerConfig { reverse: true },
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
    let matcher = HttpPathMethodMatcher::new(&router_config, &handler_registry).unwrap();
    let factory = HyperExchangeFactory;
    Router::new(factory, matcher)
}

async fn handle_request(
    req: Request<Incoming>,
    router: Arc<HyperRouter>,
) -> Result<Response<BoxBody<Bytes, std::io::Error>>, Box<dyn std::error::Error + Send + Sync>> {
    // Extract the path for logging
    let path = req.uri().path().to_string();
    let method = req.method().to_string();
    let (parts, body) = req.into_parts();
    let body = body.map_err(|e| todo!()).boxed();
    let req: HyperRequest = Request::from_parts(parts, body);
    println!("Received request: {} {}", method, path);

    // Use the router to handle the request
    match router.route(req).await {
        Ok(response_body) => Ok(response_body),
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
