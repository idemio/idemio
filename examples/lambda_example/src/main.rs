use async_trait::async_trait;
use idemio::exchange::Exchange;
use idemio::handler::registry::HandlerRegistry;
use idemio::handler::{Handler, HandlerError, HandlerFlow, HandlerId, HandlerResponse};
use idemio::router::config::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
};
use idemio::router::factory::{ExchangeFactory, ExchangeFactoryError, IntoExchange, RouteInfo};
use idemio::router::path::http::HttpPathMethodMatcher;
use idemio::router::path::PathMatcher;
use idemio::router::{Router};
use lambda_http::aws_lambda_events::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use lambda_http::{lambda_runtime, service_fn, Body, Error, LambdaEvent};
use lambda_runtime::tracing::init_default_subscriber;
use std::convert::Infallible;
use std::sync::Arc;

type LambdaExchange = Exchange<ApiGatewayProxyRequest, ApiGatewayProxyResponse>;
struct LambdaExchangeFactory;

impl ExchangeFactory<ApiGatewayProxyRequest, ApiGatewayProxyResponse> for LambdaExchangeFactory {
    fn extract_route_info<'a>(&self, request: &'a ApiGatewayProxyRequest) -> RouteInfo<'a> {
        let path = match request.path.as_ref() {
            None => None,
            Some(val) => Some(val.as_str()),
        };
        let method = Some(request.http_method.as_str());
        RouteInfo { path, method }
    }

    fn create_exchange<'req>(&self, request: ApiGatewayProxyRequest) -> Exchange<ApiGatewayProxyRequest, ApiGatewayProxyResponse> {
        let mut exchange = Exchange::new();
        exchange.set_input(request);
        exchange
    }
}

type AwsLambdaRouter = Router<
    ApiGatewayProxyRequest,
    ApiGatewayProxyResponse,
    LambdaExchangeFactory,
    HttpPathMethodMatcher<ApiGatewayProxyRequest, ApiGatewayProxyResponse>,
>;

struct TestLambdaHandler;

#[async_trait]
impl Handler<ApiGatewayProxyRequest, ApiGatewayProxyResponse> for TestLambdaHandler {
    
    fn id(&self) -> &'static str {
        "TestLambdaHandler"
    }

    async fn exec(&self, exchange: &mut LambdaExchange) -> HandlerResponse {
        let input = match exchange.take_input().await {
            Ok(input) => input,
            Err(e) => {
                return Err(HandlerError::missing_data(self.id(), e));
            }
        };
        let body = input.body.unwrap_or("NoBody".to_string()) + " - TestLambdaHandler";
        let mut response = ApiGatewayProxyResponse::default();
        response.body = Some(Body::Text(body));
        exchange.set_output(response);
        HandlerFlow::ok()
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
    let factory = LambdaExchangeFactory;
    Router::new(factory, matcher)
}

async fn entry(
    event: LambdaEvent<ApiGatewayProxyRequest>,
    router: Arc<AwsLambdaRouter>,
) -> Result<ApiGatewayProxyResponse, Error> {
    let request = event.payload;
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
        .build()?
        .block_on(async {
            init_default_subscriber();
            lambda_runtime::run(service_fn(|event| entry(event, router.clone()))).await
        })
}
