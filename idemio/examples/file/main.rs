use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::http::request::Parts;
use hyper::server::conn::http1;
use hyper::service::service_fn;

use hyper::{Request, Response};
use serde::{Deserialize, Serialize};
use tokio::{fs::File, net::TcpListener};
use tokio_util::io::ReaderStream;

use hyper_util::rt::TokioIo;
use idemio::config::{Config, HandlerConfig, ProgrammaticConfigProvider};
use idemio::exchange::Exchange;
use idemio::handler::Handler;
use idemio::handler::HandlerId;
use idemio::router::config::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
};
use idemio::router::executor::DefaultExecutor;
use idemio::router::factory::hyper::HyperExchangeFactory;
use idemio::router::{RequestRouter, Router, RouterComponents, RouterError};
use idemio::status::{ExchangeState, HandlerStatus};

// Define the RouterComponents implementation for Hyper
struct HyperComponents;

impl RouterComponents<Request<Incoming>, Bytes, BoxBody<Bytes, std::io::Error>, Parts>
    for HyperComponents
{
    type Factory = HyperExchangeFactory;
    type Executor = DefaultExecutor;
}

// Type alias for cleaner signatures
type HyperRouter =
    RequestRouter<Request<Incoming>, Bytes, BoxBody<Bytes, std::io::Error>, Parts, HyperComponents>;

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct FileHandlerConfig {
    file_path: String,
}

#[derive(Debug)]
struct FileHandler {
    config: HandlerConfig<FileHandlerConfig>,
}

#[async_trait]
impl Handler<Bytes, BoxBody<Bytes, std::io::Error>, Parts> for FileHandler {
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, Bytes, BoxBody<Bytes, std::io::Error>, Parts>,
    ) -> Result<HandlerStatus, Infallible> {
        let file_path = &self.config.config().get().file_path;

        // Try to open the file
        match File::open(file_path).await {
            Ok(file) => {
                // Create a reader stream from the file
                let reader_stream = ReaderStream::new(file);
                let stream_body = StreamBody::new(reader_stream.map_ok(Frame::data));
                let boxed_body = stream_body.boxed();
                exchange.set_output(boxed_body);

                Ok(HandlerStatus::new(
                    ExchangeState::OK | ExchangeState::EXCHANGE_COMPLETED,
                ))
            }
            Err(e) => {
                let error_msg = format!("Error reading file '{}': {}", file_path, e);
                let error_bytes = Bytes::from(error_msg.into_bytes());
                let error_body = Full::new(error_bytes)
                    .map_err(|_| unreachable!("Infallible"))
                    .boxed();
                exchange.set_output(error_body);

                Ok(HandlerStatus::new(ExchangeState::SERVER_ERROR)
                    .message(format!("Could not read file: {}", e)))
            }
        }
    }

    fn name(&self) -> &str {
        "file_handler"
    }
}

#[rustfmt::skip]
fn create_router() -> HyperRouter {
    let mut handler_registry = idemio::handler::registry::HandlerRegistry::new();

    // Register file handler
    let file_handler_id = HandlerId::new("file_handler");
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: FileHandlerConfig {
            file_path: "idemio/examples/file/sample.txt".to_string(),
        },
    })
    .unwrap();
    handler_config
        .id(file_handler_id.to_string())
        .handler_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<FileHandlerConfig> = handler_config.build();
    let handler = FileHandler {
        config: handler_config,
    };
    handler_registry
        .register_handler(file_handler_id, handler)
        .unwrap();

    let router_config = SingleServiceConfigBuilder::new()
        .route("/file")
            .get()
                .termination_handler("file_handler")
            .end_method()
        .end_route()
        .build();
    
    // Create the router using the new constructor signature
    RequestRouter::new(
        &handler_registry,
        &router_config,
        HyperExchangeFactory,
        DefaultExecutor,
    )
    .unwrap()
}

async fn handle_request(
    req: Request<Incoming>,
    router: Arc<HyperRouter>,
) -> Result<Response<BoxBody<Bytes, std::io::Error>>, Box<dyn std::error::Error + Send + Sync>> {
    // Extract the path for logging
    let path = req.uri().path().to_string();
    let method = req.method().to_string();
    println!("Received request: {} {}", method, path);

    // Use the router to handle the request
    match router.route(req).await {
        Ok(response_body) => {
            let body = response_body;
            // Create a successful HTTP response with appropriate content type
            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "text/plain")
                .body(body)?)
        }
        Err(e) => {
            // Handle routing errors
            println!("Error handling request: {}", e);
            let (status_code, error_message) = match e {
                RouterError::MissingRoute{..} => (404, "Route not found"),
                RouterError::InvalidExchange{..} => (400, "Bad request"),
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

    println!("Idemio File Server running on http://{}", addr);
    println!("Available endpoints:");
    println!("  GET  /file - Returns the contents of sample.txt");
    println!();
    println!("Examples:");
    println!("  curl http://127.0.0.1:3000/file");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let router_clone = router.clone();
        tokio::task::spawn(async move {
            // Handle the connection from the client using HTTP/1
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
