use async_trait::async_trait;
use idemio::exchange::InnerData;
use idemio::handler::{
    HandlerError, HandlerId, HandlerRegistry, LabeledHandler, TerminationHandler,
};
use idemio::router::builder::{
    MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
};
use idemio::router::{HttpPathMethodMatcher, RouteKey, RouteKeyMatcher, RouteKeyParser, Router};
use idemio::Handler;
use lambda_http::aws_lambda_events::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
use lambda_http::{lambda_runtime, service_fn, Body, Error, LambdaEvent};
use lambda_runtime::tracing::init_default_subscriber;
use std::sync::Arc;

struct LambdaRouteParser;
impl RouteKeyParser<ApiGatewayProxyRequest> for LambdaRouteParser {
    fn as_route_key<'a>(&self, request: &'a ApiGatewayProxyRequest) -> RouteKey<'a> {
        let path = request.path.as_ref().map(|val| val.as_str());
        let method = Some(request.http_method.as_str());
        RouteKey { path, method }
    }
}

type AwsLambdaRouter = Router<
    ApiGatewayProxyRequest,
    ApiGatewayProxyResponse,
    LambdaRouteParser,
    HttpPathMethodMatcher<ApiGatewayProxyRequest, ApiGatewayProxyResponse>,
>;

#[derive(Handler)]
struct LambdaEchoHandler;

#[async_trait]
impl TerminationHandler<ApiGatewayProxyRequest, ApiGatewayProxyResponse> for LambdaEchoHandler {
    async fn exec(
        &self,
        data: InnerData<ApiGatewayProxyRequest>,
    ) -> Result<InnerData<ApiGatewayProxyResponse>, HandlerError> {
        let input = data.data;
        let attachments = data.attachments;

        let body = input.body.unwrap_or("NoBody".to_string());
        let mut response = ApiGatewayProxyResponse::default();
        response.is_base64_encoded = input.is_base64_encoded;
        response.body = Some(Body::Text(body));

        Ok(InnerData::new(response))
    }
}

fn create_router() -> AwsLambdaRouter {
    let mut handler_registry = HandlerRegistry::new();
    let handler = LambdaEchoHandler;
    handler_registry
        .register_termination_handler(HandlerId::new(LambdaEchoHandler::id()), handler)
        .unwrap();
    let router_config = SingleServiceConfigBuilder::new()
        .route("/test")
        .get()
        .request_handler("TestLambdaHandler")
        .end_method()
        .end_route()
        .build();
    let matcher = HttpPathMethodMatcher::new(&router_config, &handler_registry).unwrap();
    Router::new(LambdaRouteParser, matcher)
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
            response.body = Some(Body::Text(format!("{}", e)));
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
