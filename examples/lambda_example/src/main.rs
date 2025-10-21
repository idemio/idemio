use async_trait::async_trait;
use idemio::exchange::Exchange;
use idemio::handler::registry::HandlerRegistry;
use idemio::handler::{Handler, HandlerId};
use idemio::router::config::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
};
use idemio::router::executor::DefaultExecutor;
use idemio::router::factory::{ExchangeFactory, ExchangeFactoryError, RouteInfo};
use idemio::router::path::http::HttpPathMethodMatcher;
use idemio::router::path::PathMatcher;
use idemio::router::{RequestRouter, Router, RouterBuilder};
use idemio::status::{ExchangeState, HandlerStatus};
use lambda_http::aws_lambda_events::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use lambda_http::{lambda_runtime, service_fn, Body, Context, Error, LambdaEvent};
use lambda_runtime::tracing::init_default_subscriber;
use std::convert::Infallible;
use std::sync::Arc;

type LambdaExchange = Exchange<ApiGatewayProxyRequest, ApiGatewayProxyResponse, Context>;
type LambdaPathRouter = HttpPathMethodMatcher<LambdaExchange>;
type IncomingLambdaRequest = ApiGatewayProxyRequest;
type OutgoingLambdaResponse = ApiGatewayProxyResponse;
struct LambdaExchangeFactory;

#[async_trait]
impl ExchangeFactory<IncomingLambdaRequest, LambdaExchange> for LambdaExchangeFactory {
    async fn extract_route_info<'a>(
        &self,
        request: &'a IncomingLambdaRequest,
    ) -> Result<RouteInfo<'a>, ExchangeFactoryError> {
        let path = match request.path.as_ref() {
            None => None,
            Some(val) => Some(val.as_str()),
        };
        let method = Some(request.http_method.as_str());
        Ok(RouteInfo { path, method })
    }

    async fn create_exchange<'req>(
        &self,
        request: IncomingLambdaRequest,
    ) -> Result<LambdaExchange, ExchangeFactoryError> {
        let mut exchange = Exchange::new();
        exchange.set_input(request);
        Ok(exchange)
    }
}

type AwsLambdaRouter = RequestRouter<
    IncomingLambdaRequest,
    LambdaExchange,
    LambdaExchangeFactory,
    DefaultExecutor<OutgoingLambdaResponse>,
    LambdaPathRouter,
>;

struct TestLambdaHandler;

#[async_trait]
impl Handler<LambdaExchange> for TestLambdaHandler {
    async fn exec(&self, exchange: &mut LambdaExchange) -> Result<HandlerStatus, Infallible> {
        let input = match exchange.take_input().await {
            Ok(input) => input,
            Err(e) => {
                return Ok(
                    HandlerStatus::new(idemio::status::ExchangeState::SERVER_ERROR)
                        .message(format!("Could not consume input from exchange: {}", e)),
                );
            }
        };
        let body = input.body.unwrap_or("NoBody".to_string()) + " - TestLambdaHandler";
        let mut response = ApiGatewayProxyResponse::default();
        response.body = Some(Body::Text(body));
        exchange.set_output(response);
        Ok(HandlerStatus::new(
            ExchangeState::OK | ExchangeState::EXCHANGE_COMPLETED,
        ))
    }

    fn name(&self) -> &str {
        "TestLambdaHandler"
    }
}

fn create_router() -> AwsLambdaRouter {
    let mut handler_registry = HandlerRegistry::new();
    let handler = TestLambdaHandler;
    handler_registry
        .register_handler(HandlerId::new("TestLambdaHandler"), handler)
        .unwrap();
    let router_config = SingleServiceConfigBuilder::new()
        .route("/test")
        .get()
        .request_handler("TestLambdaHandler")
        .end_method()
        .end_route()
        .build();
    let matcher = HttpPathMethodMatcher::new(&router_config, &handler_registry).unwrap();
    let executor: DefaultExecutor<OutgoingLambdaResponse> = DefaultExecutor {
        _phantom: Default::default(),
    };
    let factory = LambdaExchangeFactory;
    RouterBuilder::new()
        .factory(factory)
        .executor(executor)
        .matcher(matcher)
        .build()
}

async fn entry(
    event: LambdaEvent<ApiGatewayProxyRequest>,
    router: Arc<AwsLambdaRouter>,
) -> Result<ApiGatewayProxyResponse, Error> {
    let request = event.payload;
    let context = event.context;
    match router.route(request).await {
        Ok(response) => Ok(response),
        Err(e) => {
            let mut response = ApiGatewayProxyResponse::default();
            response.body = Some(Body::Text(format!("Error: {}", e)));
            Ok(response)
        }
    }
}

fn main() -> Result<(), Error> {
    let router = Arc::new(create_router());

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            init_default_subscriber();
            lambda_runtime::run(service_fn(|event| entry(event, router.clone()))).await
        })
}
