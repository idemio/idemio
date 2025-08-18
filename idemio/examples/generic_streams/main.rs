use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{StreamExt, TryStreamExt, stream};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::http::request::Parts;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

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
use idemio::router::path::{PathMatcher, PathPrefixMethodKey, PathPrefixMethodPathMatcher};
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
struct StreamingLoggerConfig {
    log_prefix: String,
}

#[derive(Debug)]
struct StreamingLoggerHandler {
    config: HandlerConfig<StreamingLoggerConfig>,
}

#[async_trait]
impl Handler<Bytes, BoxBody<Bytes, std::io::Error>, Parts> for StreamingLoggerHandler {
    async fn exec<'a>(
        &self,
        _exchange: &mut Exchange<'a, Bytes, BoxBody<Bytes, std::io::Error>, Parts>,
    ) -> Result<HandlerStatus, Infallible> {
        let prefix = &self.config.config().get().log_prefix;
        println!("{}: Processing streaming request", prefix);
        Ok(HandlerStatus::new(ExchangeState::OK))
    }

    fn name(&self) -> &str {
        "streaming_logger_handler"
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct StreamProcessorConfig {
    chunk_delay_ms: u64,
    transform_mode: String, // "uppercase", "reverse", "echo"
}

#[derive(Debug)]
struct StreamProcessorHandler {
    config: HandlerConfig<StreamProcessorConfig>,
}

#[async_trait]
impl Handler<Bytes, BoxBody<Bytes, std::io::Error>, Parts> for StreamProcessorHandler {
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, Bytes, BoxBody<Bytes, std::io::Error>, Parts>,
    ) -> Result<HandlerStatus, Infallible> {
        // Get configuration
        let config = self.config.config().get();
        let delay = Duration::from_millis(config.chunk_delay_ms);
        let transform_mode = config.transform_mode.clone(); // Clone to avoid borrowing issues

        // Take the input stream from the exchange
        match exchange.take_input_stream() {
            Ok(input_stream) => {
                // Collect all chunks first to avoid lifetime issues
                let chunks: Vec<_> = input_stream.try_collect().await.unwrap_or_else(|e| {
                    log::error!("Error collecting input stream: {}", e);
                    vec![]
                });

                // Create a static stream from processed chunks
                let processed_chunks: Vec<_> = chunks
                    .into_iter()
                    .map(|bytes| {
                        let input_str = String::from_utf8_lossy(&bytes);
                        let transformed = match transform_mode.as_str() {
                            "uppercase" => input_str.to_uppercase(),
                            "reverse" => input_str.chars().rev().collect(),
                            "echo" | _ => input_str.to_string(),
                        };
                        Bytes::from(format!("Processed: {}\n", transformed))
                    })
                    .collect();

                // Create output stream with delays
                let output_stream = stream::iter(processed_chunks).then(move |chunk| async move {
                    sleep(delay).await;
                    Ok(Frame::data(chunk))
                });

                // Convert the stream to a BoxBody with explicit type bounds
                let body: BoxBody<Bytes, std::io::Error> =
                    BodyExt::boxed(StreamBody::new(output_stream));
                exchange.set_output(body);

                Ok(HandlerStatus::new(
                    ExchangeState::OK | ExchangeState::EXCHANGE_COMPLETED,
                ))
            }
            Err(e) => {
                log::error!("Error taking input stream: {}", e);

                // Create an error response stream
                let error_message = format!("Stream processing error: {}", e);
                let error_stream =
                    stream::once(async move { Ok(Frame::data(Bytes::from(error_message))) });

                let error_body: BoxBody<Bytes, std::io::Error> =
                    BodyExt::boxed(StreamBody::new(error_stream));
                exchange.set_output(error_body);

                Ok(HandlerStatus::new(ExchangeState::SERVER_ERROR))
            }
        }
    }

    fn name(&self) -> &str {
        "stream_processor_handler"
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct DataGeneratorConfig {
    chunk_count: usize,
    chunk_size: usize,
    interval_ms: u64,
}

#[derive(Debug)]
struct DataGeneratorHandler {
    config: HandlerConfig<DataGeneratorConfig>,
}

#[async_trait]
impl Handler<Bytes, BoxBody<Bytes, std::io::Error>, Parts> for DataGeneratorHandler {
    async fn exec<'a>(
        &self,
        exchange: &mut Exchange<'a, Bytes, BoxBody<Bytes, std::io::Error>, Parts>,
    ) -> Result<HandlerStatus, Infallible> {
        let config = self.config.config().get();
        let chunk_count = config.chunk_count;
        let chunk_size = config.chunk_size;
        let interval = Duration::from_millis(config.interval_ms);

        // Generate a stream of data chunks
        let data_stream = stream::iter(0..chunk_count).then(move |i| {
            let interval_clone = interval;
            let chunk_size_clone = chunk_size;
            async move {
                if i > 0 {
                    sleep(interval_clone).await;
                }

                let chunk_data = format!(
                    "Chunk {} of {}: {}\n",
                    i + 1,
                    chunk_count,
                    "data".repeat(chunk_size_clone / 4)
                );

                Ok(Frame::data(Bytes::from(chunk_data)))
            }
        });

        let body = BodyExt::boxed(StreamBody::new(data_stream));
        exchange.set_output(body);

        Ok(HandlerStatus::new(
            ExchangeState::OK | ExchangeState::EXCHANGE_COMPLETED,
        ))
    }

    fn name(&self) -> &str {
        "data_generator_handler"
    }
}

// Updated function to create a router with streaming handlers
#[rustfmt::skip]
fn create_streaming_router<'a>() -> HyperRouter<'a> {
    let mut handler_registry = HandlerRegistry::new();

    // Register streaming logger handler
    let logger_id = HandlerId::new("streaming_logger");
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: StreamingLoggerConfig {
            log_prefix: "STREAM".to_string(),
        },
    }).unwrap();
    handler_config
        .id(logger_id.to_string())
        .handler_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<StreamingLoggerConfig> = handler_config.build();
    let handler = StreamingLoggerHandler { config: handler_config };
    handler_registry.register_handler(logger_id, handler).unwrap();

    // Register stream processor handler
    let processor_id = HandlerId::new("stream_processor");
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: StreamProcessorConfig {
            chunk_delay_ms: 100,
            transform_mode: "uppercase".to_string(),
        },
    }).unwrap();
    handler_config
        .id(processor_id.to_string())
        .handler_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<StreamProcessorConfig> = handler_config.build();
    let handler = StreamProcessorHandler { config: handler_config };
    handler_registry.register_handler(processor_id, handler).unwrap();

    // Register data generator handler
    let generator_id = HandlerId::new("data_generator");
    let mut handler_config = HandlerConfig::builder();
    let inner_config = Config::new(ProgrammaticConfigProvider {
        config: DataGeneratorConfig {
            chunk_count: 10,
            chunk_size: 50,
            interval_ms: 200,
        },
    }).unwrap();
    handler_config
        .id(generator_id.to_string())
        .handler_config(inner_config)
        .enabled(true);
    let handler_config: HandlerConfig<DataGeneratorConfig> = handler_config.build();
    let handler = DataGeneratorHandler { config: handler_config };
    handler_registry.register_handler(generator_id, handler).unwrap();

    let router_config = SingleServiceConfigBuilder::new()
        .route("/stream/process")
            .post()
                .request_handler("streaming_logger")
                .termination_handler("stream_processor")
            .end_method()
        .end_route()
        .route("/stream/generate")
            .get()
                .request_handler("streaming_logger")
                .termination_handler("data_generator")
            .end_method()
        .end_route()
        .route("/stream/reverse")
            .post()
                .request_handler("streaming_logger")
                .termination_handler("stream_processor")
            .end_method()
        .end_route()
        .build();

    let matcher = PathPrefixMethodPathMatcher::new(&router_config, &handler_registry).unwrap();
    RequestRouter::new(
        matcher,
        HyperExchangeFactory,
        DefaultExecutor,
    ).unwrap()
}

async fn handle_streaming_request(
    req: Request<Incoming>,
    router: Arc<HyperRouter<'_>>,
) -> Result<Response<BoxBody<Bytes, std::io::Error>>, Box<dyn std::error::Error + Send + Sync>> {
    let path = req.uri().path().to_string();
    let method = req.method().to_string();
    println!("Received streaming request: {} {}", method, path);

    match router.route(req).await {
        Ok(response_body) => {
            // For streaming responses, set appropriate headers
            Ok(Response::builder()
                .status(200)
                .header("Content-Type", "text/plain; charset=utf-8")
                .header("Transfer-Encoding", "chunked")
                .header("Cache-Control", "no-cache")
                .body(response_body)?)
        }
        Err(e) => {
            println!("Error handling streaming request: {}", e);
            let (status_code, error_message) = match e {
                RouterError::MissingRoute { .. } => (404, "Route not found"),
                RouterError::InvalidExchange { .. } => (400, "Bad request"),
                _ => (500, "Internal server error"),
            };

            let error_stream = stream::once(async move {
                Ok(Frame::data(Bytes::from(format!(
                    "{}: {}",
                    error_message, e
                ))))
            });

            Ok(Response::builder()
                .status(status_code)
                .header("Content-Type", "text/plain")
                .body(BodyExt::boxed(StreamBody::new(error_stream)))?)
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize simple logging
    env_logger::init();

    let router = Arc::new(create_streaming_router());
    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));

    let listener = TcpListener::bind(addr).await?;

    println!("Idemio Streaming Server running on http://{}", addr);
    println!("Available streaming endpoints:");
    println!("  POST /stream/process      - Process incoming stream data with transformations");
    println!("  GET  /stream/generate     - Generate a stream of data chunks");
    println!("  POST /stream/reverse      - Reverse incoming stream data");
    println!();
    println!("Streaming Examples:");
    println!("  curl http://127.0.0.1:3001/stream/generate");
    println!("  curl -X POST -d 'hello world streaming' http://127.0.0.1:3001/stream/process");
    println!(
        "  echo 'streaming data test' | curl -X POST -T- http://127.0.0.1:3001/stream/reverse"
    );
    println!();
    println!("For better streaming visualization, try:");
    println!("  curl -N http://127.0.0.1:3001/stream/generate");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let router_clone = router.clone();

        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |req| handle_streaming_request(req, router_clone.clone())),
                )
                .await
            {
                eprintln!("Error serving connection: {}", err);
            }
        });
    }
}
