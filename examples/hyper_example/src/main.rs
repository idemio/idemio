mod modify;

use async_trait::async_trait;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::http::response;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;

use idemio::exchange::{Exchange, InnerData};
use idemio::handler::{
    HandlerError, HandlerId, HandlerRegistry, LabeledHandler, MiddlewareHandler,
    MiddlewareResponse, MiddlewareResult, TerminationHandler,
};
use idemio::router::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
};
use idemio::router::{
    HttpPathMethodMatcher, RouteKey, RouteKeyMatcher, RouteKeyParser, Router, RouterError,
};
use idemio::Handler;
use tokio::net::TcpListener;

type HyperRequest = Request<BoxBody<Bytes, std::io::Error>>;
type HyperResponse = Response<BoxBody<Bytes, std::io::Error>>;
async fn collect_body(body: BoxBody<Bytes, std::io::Error>) -> Bytes {
    body.collect()
        .await
        .expect("Could not collect boxed body")
        .to_bytes()
}
pub struct HyperRouteKeyParser;
impl RouteKeyParser<HyperRequest> for HyperRouteKeyParser {
    fn as_route_key<'a>(&self, request: &'a HyperRequest) -> RouteKey<'a> {
        RouteKey::new(request.uri().path(), request.method().as_str())
    }
}

// Simplified type alias for the complete router
type HyperRouter = idemio::router::Router<
    HyperRequest,
    HyperResponse,
    HyperRouteKeyParser,
    HttpPathMethodMatcher<HyperRequest, HyperResponse>,
>;

#[derive(Debug, Handler)]
struct IdempotentLoggingHandler;
#[async_trait]
impl<T> MiddlewareHandler<T> for IdempotentLoggingHandler
where
    T: Send + Sync,
{
    async fn exec(&self, exchange: &mut Exchange<T>) -> MiddlewareResult {
        println!("uuid={}", exchange.uuid().to_string());
        MiddlewareResponse::ok()
    }
}

#[derive(Debug, Handler)]
struct GreetingHandler;

#[async_trait]
impl TerminationHandler<HyperRequest, HyperResponse> for GreetingHandler {
    async fn exec(
        &self,
        exchange: InnerData<HyperRequest>,
    ) -> Result<InnerData<HyperResponse>, HandlerError> {
        let (parts, body) = exchange.data.into_parts();
        let input_bytes = collect_body(body).await;
        let input_str = String::from_utf8_lossy(&input_bytes).to_string();
        let response = "Hello World!";
        let response_bytes = Bytes::from(response.as_bytes());
        let body = Full::new(response_bytes)
            .map_err(|_| unreachable!("Infallible"))
            .boxed();
        let response = response::Builder::new()
            .status(StatusCode::OK)
            .body(body)
            .unwrap();
        Ok(InnerData::new(response))
    }
}
#[derive(Debug, Handler)]
struct EchoHandler;

#[async_trait]
impl TerminationHandler<HyperRequest, HyperResponse> for EchoHandler {
    async fn exec(
        &self,
        request: InnerData<HyperRequest>,
    ) -> Result<InnerData<HyperResponse>, HandlerError> {
        let (parts, body) = request.data.into_parts();
        let input_bytes = collect_body(body).await;
        let input_str = String::from_utf8_lossy(&input_bytes).to_string();
        let processed_input: String = input_str.chars().rev().collect();
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
        Ok(InnerData::new(response))
    }
}

// Updated function using the new RouterBuilder
fn create_router() -> HyperRouter {
    let mut handler_registry = HandlerRegistry::new();

    // Register greeting handler
    handler_registry
        .register_termination_handler(GreetingHandler::handler_id(), GreetingHandler)
        .unwrap();

    // Register echo handler
    handler_registry
        .register_termination_handler(EchoHandler::handler_id(), EchoHandler)
        .unwrap();

    // Register idempotent logging handler
    handler_registry
        .register_request_handler(
            IdempotentLoggingHandler::handler_id(),
            IdempotentLoggingHandler,
        )
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
    let factory = HyperRouteKeyParser;
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
            println!("Error handling request: {}", e);
            let (status_code, error_message) = match e {
                RouterError::MissingRoute { .. } => (404, "Route not found"),
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
