use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// ConfigStructure defines the pathing mechanism used by the router.
/// You can configure the router to use a single service or a shared service.
///
/// # Structure Diagram
/// ```text
// ┌───────────────┐
// │ConfigStructure├───┤ Root configuration.
// └┬──────────────┘
//  │   ┌──────┐
//  ├──►│Single├───┤ A single service with defined routes (i.e. web app).
//  │   └┬─────┘
//  │    │  ┌──────────┐
//  │    ├─►│ Handlers ├───┤ The handlers to be loaded on startup.
//  │    │  ├──────────┤
//  │    ├─►│  Chains  ├───┤ Lists of handlers.
//  │    │  ├──────────┤
//  │    └─►│  Routes  ├───┤ Handlers to be invoked based on route info.
//  │       └──────────┘
//  │   ┌──────┐
//  └──►│Shared├───┤ Multiple services sharing an instance (i.e. an api gateway).
//      └┬─────┘
//       │  ┌──────────┐
//       └─►│ Service  ├───┤ Multiple services define their own handlers, chains, and routes.
//          └┬─────────┘
//           │  ┌──────────┐
//           ├─►│ Handlers │
//           │  ├──────────┤
//           ├─►│  Chains  │
//           │  ├──────────┤
//           └─►│  Routes  │
//              └──────────┘
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub enum ConfigStructure {

    #[serde( alias = "single", alias = "SINGLE")]
    Single(RouterConfig),

    #[serde( alias = "shared", alias = "SHARED")]
    Shared(SharedConfig),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum RouteType {
    None,

    #[serde(rename = "header", alias = "Header", alias = "HEADER")]
    Header(String),

    #[serde(rename = "path", alias = "Path", alias = "PATH")]
    Path,
}

impl RouteType {
    pub fn header(header_name: impl Into<String>) -> Self {
        RouteType::Header(header_name.into())
    }

    pub fn path() -> Self {
        RouteType::Path
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SharedConfig {
    key: RouteType,
    services: HashMap<String, RouterConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    pub(crate) handlers: HashSet<String>,
    pub(crate) chains: HashMap<String, Vec<String>>,
    pub(crate) routes: Routes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Routes {
    #[serde(rename = "path", alias = "Path", alias = "PATH")]
    HttpRequestPaths(HashMap<String, HashMap<String, PathChain>>),

    #[serde(rename = "header", alias = "Header", alias = "HEADER")]
    HttpHeaderPaths(HashMap<String, HashMap<String, PathChain>>),
    //TcpIpPaths(HashMap<String, HashMap<String, PathChain>>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathChain {
    #[serde(skip_serializing_if = "Option::is_none", rename = "request")]
    pub(crate) request_handlers: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "termination")]
    pub(crate) termination_handler: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "response")]
    pub(crate) response_handlers: Option<Vec<String>>,
}

impl PathChain {
    pub fn new() -> Self {
        Self {
            request_handlers: None,
            termination_handler: None,
            response_handlers: None,
        }
    }

    fn add_request_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.request_handlers
            .get_or_insert_with(Vec::new)
            .push(handler.into());
        self
    }

    fn termination_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.termination_handler = Some(handler.into());
        self
    }

    fn add_response_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.response_handlers
            .get_or_insert_with(Vec::new)
            .push(handler.into());
        self

    }
}

impl Default for PathChain {
    fn default() -> Self {
        PathChain::new()
    }
}

pub mod builder {
    use super::*;

    /// Core service configuration logic that can be shared between single and shared builders
    pub struct ServiceConfigCore {
        handlers: HashSet<String>,
        chains: HashMap<String, Vec<String>>,
        routes: HashMap<String, HashMap<String, PathChain>>,
    }

    impl Default for ServiceConfigCore {
        fn default() -> Self {
            ServiceConfigCore::new()
        }
    }

    impl ServiceConfigCore {
        pub fn new() -> Self {
            Self {
                handlers: HashSet::new(),
                chains: HashMap::new(),
                routes: HashMap::new(),
            }
        }

        pub fn add_handler(&mut self, handler_name: impl Into<String>) {
            let handler = handler_name.into();
            if !self.handlers.contains(&handler) {
                self.handlers.insert(handler);
            }
        }

        pub fn add_handlers(&mut self, handler_names: &[impl AsRef<str>]) {
            for handler_name in handler_names {
                let handler = handler_name.as_ref().to_string();
                if !self.handlers.contains(&handler) {
                    self.handlers.insert(handler);
                }
            }
        }

        pub fn add_chain(
            &mut self,
            chain_name: impl Into<String>,
            handler_names: &[impl AsRef<str>],
        ) {
            let chain_handlers: Vec<String> = handler_names
                .iter()
                .map(|h| h.as_ref().to_string())
                .collect();

            // Auto-register handlers that aren't already registered
            for handler in &chain_handlers {
                if !self.handlers.contains(handler) {
                    self.handlers.insert(handler.clone());
                }
            }

            self.chains.insert(chain_name.into(), chain_handlers);
        }

        pub fn ensure_route_exists(&mut self, path: &str) {
            if !self.routes.contains_key(path) {
                self.routes.insert(path.to_string(), HashMap::new());
            }
        }

        pub fn add_method(&mut self, path: &str, method: String, path_chain: PathChain) {
            // Auto-register handlers from the path chain
            if let Some(ref handler) = path_chain.termination_handler {
                self.add_handler(handler.clone());
            }

            if let Some(ref handlers) = path_chain.request_handlers {
                for handler in handlers {
                    self.add_handler(handler.clone());
                }
            }

            if let Some(ref handlers) = path_chain.response_handlers {
                for handler in handlers {
                    self.add_handler(handler.clone());
                }
            }

            match self.routes.get_mut(path) {
                Some(x) => x,
                None => {
                    self.routes.insert(path.to_string(), HashMap::new());
                    self.routes.get_mut(path).unwrap()
                }
            }
            .insert(method, path_chain);
        }

        pub fn build(self) -> RouterConfig {
            RouterConfig {
                handlers: self.handlers,
                chains: self.chains,
                routes: Routes::HttpRequestPaths(self.routes),
            }
        }
    }

    /// Trait for service builders that can configure handlers, chains, and routes
    pub trait ServiceBuilder: Sized {
        type RouteBuilder;

        fn core(&mut self) -> &mut ServiceConfigCore;

        fn handler(mut self, handler_name: impl Into<String>) -> Self {
            self.core().add_handler(handler_name);
            self
        }

        fn handlers(mut self, handler_names: &[impl AsRef<str>]) -> Self {
            self.core().add_handlers(handler_names);
            self
        }

        fn chain(
            mut self,
            chain_name: impl Into<String>,
            handler_names: &[impl AsRef<str>],
        ) -> Self {
            self.core().add_chain(chain_name, handler_names);
            self
        }

        fn route(self, path: impl Into<String>) -> Self::RouteBuilder;
    }

    /// Trait for route builders that can configure HTTP methods
    pub trait RouteBuilder: Sized {
        type MethodBuilder;
        type ServiceBuilder;

        fn create_method_builder(self, method: impl Into<String>) -> Self::MethodBuilder;

        /// Return to the service builder to configure other routes or complete the configuration
        fn end_route(self) -> Self::ServiceBuilder;

        fn head(self) -> Self::MethodBuilder {
            self.create_method_builder("HEAD")
        }

        fn options(self) -> Self::MethodBuilder {
            self.create_method_builder("OPTIONS")
        }

        fn get(self) -> Self::MethodBuilder {
            self.create_method_builder("GET")
        }

        fn post(self) -> Self::MethodBuilder {
            self.create_method_builder("POST")
        }

        fn put(self) -> Self::MethodBuilder {
            self.create_method_builder("PUT")
        }

        fn delete(self) -> Self::MethodBuilder {
            self.create_method_builder("DELETE")
        }

        fn patch(self) -> Self::MethodBuilder {
            self.create_method_builder("PATCH")
        }
    }

    /// Trait for method builders that can configure handler chains
    pub trait MethodBuilder: Sized {
        type RouteBuilder;

        fn path_chain(&mut self) -> &mut PathChain;
        fn chains(&self) -> &HashMap<String, Vec<String>>;

        fn request_handlers(mut self, handlers: &[impl AsRef<str>]) -> Self {
            for handler in handlers {
                self.path_chain().add_request_handler(handler.as_ref());
            }
            self
        }

        fn request_handler(mut self, handler: impl Into<String>) -> Self {
            self.path_chain().add_request_handler(handler);
            self
        }

        fn termination_handler(mut self, handler: impl Into<String>) -> Self {
            self.path_chain().termination_handler(handler);
            self
        }

        fn response_handlers(mut self, handlers: &[impl AsRef<str>]) -> Self {
            for handler in handlers {
                self.path_chain().add_response_handler(handler.as_ref());
            }
            self
        }

        fn response_handler(mut self, handler: impl Into<String>) -> Self {
            self.path_chain().add_response_handler(handler);
            self
        }

        fn request_chain(mut self, chain_name: impl AsRef<str>) -> Self {
            if let Some(chain_handlers) = self.chains().get(chain_name.as_ref()) {
                let handlers: Vec<String> = chain_handlers.clone();
                for handler in handlers {
                    self.path_chain().add_request_handler(handler);
                }
            }
            self
        }

        fn response_chain(mut self, chain_name: impl AsRef<str>) -> Self {
            if let Some(chain_handlers) = self.chains().get(chain_name.as_ref()) {
                let handlers: Vec<String> = chain_handlers.clone();
                for handler in handlers {
                    self.path_chain().add_response_handler(handler);
                }
            }
            self
        }

        fn end_method(self) -> Self::RouteBuilder;
    }

    pub struct SingleServiceConfigBuilder {
        core: ServiceConfigCore,
    }

    impl Default for SingleServiceConfigBuilder {
        fn default() -> Self {
            SingleServiceConfigBuilder::new()
        }
    }

    impl SingleServiceConfigBuilder {
        pub fn new() -> Self {
            Self {
                core: ServiceConfigCore::new(),
            }
        }

        pub fn build(self) -> RouterConfig {
            self.core.build()
        }
    }

    impl ServiceBuilder for SingleServiceConfigBuilder {
        type RouteBuilder = SingleServiceRouteBuilder;

        fn core(&mut self) -> &mut ServiceConfigCore {
            &mut self.core
        }

        fn route(mut self, path: impl Into<String>) -> Self::RouteBuilder {
            let path = path.into();
            self.core.ensure_route_exists(&path);

            SingleServiceRouteBuilder {
                config_builder: self,
                current_path: path,
            }
        }
    }

    pub struct SingleServiceRouteBuilder {
        config_builder: SingleServiceConfigBuilder,
        current_path: String,
    }

    impl RouteBuilder for SingleServiceRouteBuilder {
        type MethodBuilder = SingleServiceMethodBuilder;
        type ServiceBuilder = SingleServiceConfigBuilder;

        fn create_method_builder(self, method: impl Into<String>) -> Self::MethodBuilder {
            SingleServiceMethodBuilder {
                route_builder: self,
                method: method.into(),
                path_chain: PathChain::new(),
            }
        }

        fn end_route(self) -> Self::ServiceBuilder {
            self.config_builder
        }
    }

    pub struct SingleServiceMethodBuilder {
        route_builder: SingleServiceRouteBuilder,
        method: String,
        path_chain: PathChain,
    }

    impl MethodBuilder for SingleServiceMethodBuilder {
        type RouteBuilder = SingleServiceRouteBuilder;

        fn path_chain(&mut self) -> &mut PathChain {
            &mut self.path_chain
        }

        fn chains(&self) -> &HashMap<String, Vec<String>> {
            &self.route_builder.config_builder.core.chains
        }

        fn end_method(self) -> Self::RouteBuilder {
            let mut route_builder = self.route_builder;
            route_builder.config_builder.core.add_method(
                &route_builder.current_path,
                self.method,
                self.path_chain,
            );
            route_builder
        }
    }

    pub struct SharedConfigBuilder {
        key: RouteType,
        services: HashMap<String, RouterConfig>,
    }

    impl SharedConfigBuilder {
        pub fn new() -> Self {
            Self {
                key: RouteType::None,
                services: HashMap::new(),
            }
        }

        pub fn route_type(mut self, route_type: RouteType) -> Self {
            self.key = route_type;
            self
        }

        pub fn service(self, service_name: impl Into<String>) -> SharedServiceBuilder {
            SharedServiceBuilder {
                shared_builder: self,
                service_name: service_name.into(),
                core: ServiceConfigCore::new(),
            }
        }

        pub fn build(self) -> SharedConfig {
            SharedConfig {
                key: self.key,
                services: self.services,
            }
        }
    }

    pub struct SharedServiceBuilder {
        shared_builder: SharedConfigBuilder,
        service_name: String,
        core: ServiceConfigCore,
    }

    impl SharedServiceBuilder {
        pub fn end_service(mut self) -> SharedConfigBuilder {
            let router_config = self.core.build();
            self.shared_builder
                .services
                .insert(self.service_name, router_config);
            self.shared_builder
        }
    }

    impl ServiceBuilder for SharedServiceBuilder {
        type RouteBuilder = SharedServiceRouteBuilder;

        fn core(&mut self) -> &mut ServiceConfigCore {
            &mut self.core
        }

        fn route(mut self, path: impl Into<String>) -> Self::RouteBuilder {
            let path = path.into();
            self.core.ensure_route_exists(&path);

            SharedServiceRouteBuilder {
                service_builder: self,
                current_path: path,
            }
        }
    }

    pub struct SharedServiceRouteBuilder {
        service_builder: SharedServiceBuilder,
        current_path: String,
    }

    impl RouteBuilder for SharedServiceRouteBuilder {
        type MethodBuilder = SharedServiceMethodBuilder;
        type ServiceBuilder = SharedServiceBuilder;

        fn create_method_builder(self, method: impl Into<String>) -> Self::MethodBuilder {
            SharedServiceMethodBuilder {
                route_builder: self,
                method: method.into(),
                path_chain: PathChain::new(),
            }
        }

        fn end_route(self) -> Self::ServiceBuilder {
            self.service_builder
        }
    }

    pub struct SharedServiceMethodBuilder {
        route_builder: SharedServiceRouteBuilder,
        method: String,
        path_chain: PathChain,
    }

    impl MethodBuilder for SharedServiceMethodBuilder {
        type RouteBuilder = SharedServiceRouteBuilder;

        fn path_chain(&mut self) -> &mut PathChain {
            &mut self.path_chain
        }

        fn chains(&self) -> &HashMap<String, Vec<String>> {
            &self.route_builder.service_builder.core.chains
        }

        fn end_method(self) -> Self::RouteBuilder {
            let mut route_builder = self.route_builder;
            route_builder.service_builder.core.add_method(
                &route_builder.current_path,
                self.method,
                self.path_chain,
            );
            route_builder
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        #[rustfmt::skip]
        fn test_single_service_multiple_methods_same_route() {
            let config = SingleServiceConfigBuilder::new()
                .chain("auth_chain", &["cors", "auth"])
                .route("/api/users")
                    .get()
                        .request_chain("auth_chain")
                        .termination_handler("get_users")
                        .end_method()
                    .post()
                        .request_chain("auth_chain")
                        .request_handler("validator")
                        .termination_handler("create_user")
                        .end_method()
                    .put()
                        .termination_handler("update_user")
                        .end_method()
                    .delete()
                        .termination_handler("delete_user")
                        .end_method()
                    .end_route()
                .route("/api/orders")
                    .get()
                        .termination_handler("get_orders")
                        .end_method()
                    .end_route()
                .build();

            println!("{:#?}", config);

            // Verify handlers were registered
            assert!(config.handlers.contains(&"get_users".to_string()));
            assert!(config.handlers.contains(&"create_user".to_string()));
            assert!(config.handlers.contains(&"update_user".to_string()));
            assert!(config.handlers.contains(&"delete_user".to_string()));
            assert!(config.handlers.contains(&"get_orders".to_string()));

            // Verify routes were configured
            if let Routes::HttpRequestPaths(routes) = &config.routes {
                assert!(routes.contains_key("/api/users"));
                assert!(routes.contains_key("/api/orders"));

                let user_routes = &routes["/api/users"];
                assert!(user_routes.contains_key("GET"));
                assert!(user_routes.contains_key("POST"));
                assert!(user_routes.contains_key("PUT"));
                assert!(user_routes.contains_key("DELETE"));

                let order_routes = &routes["/api/orders"];
                assert!(order_routes.contains_key("GET"));
            }
        }

        #[test]
        #[rustfmt::skip]
        fn test_shared_service_multiple_methods_same_route() {
            let config = SharedConfigBuilder::new()
                .route_type(RouteType::header("x-service-id"))
                .service("user_service")
                    .chain("user_auth", &["auth_middleware", "user_validator"])
                    .route("/api/users")
                        .get()
                            .request_chain("user_auth")
                            .termination_handler("get_users")
                        .end_method()
                        .post()
                            .request_chain("user_auth")
                            .request_handler("create_validator")
                            .termination_handler("create_user")
                            .response_handler("audit_logger")
                        .end_method()
                        .put()
                            .termination_handler("update_user")
                        .end_method()
                        .delete()
                            .termination_handler("delete_user")
                        .end_method()
                        .end_route()
                    .route("/api/profile")
                        .get()
                            .termination_handler("get_profile")
                        .end_method()
                    .end_route()
                .end_service()
                .service("order_service")
                    .route("/api/orders")
                        .get()
                            .termination_handler("list_orders")
                        .end_method()
                        .post()
                            .termination_handler("create_order")
                        .end_method()
                    .end_route()
                .end_service()
                .build();

            println!("{:#?}", config);

            // Verify structure
            assert_eq!(config.services.len(), 2);
            assert!(config.services.contains_key("user_service"));
            assert!(config.services.contains_key("order_service"));

            // Verify user service routes
            let user_service = &config.services["user_service"];
            if let Routes::HttpRequestPaths(routes) = &user_service.routes {
                assert!(routes.contains_key("/api/users"));
                assert!(routes.contains_key("/api/profile"));

                let user_routes = &routes["/api/users"];
                assert!(user_routes.contains_key("GET"));
                assert!(user_routes.contains_key("POST"));
                assert!(user_routes.contains_key("PUT"));
                assert!(user_routes.contains_key("DELETE"));
            }

            // Verify order service routes
            let order_service = &config.services["order_service"];
            if let Routes::HttpRequestPaths(routes) = &order_service.routes {
                assert!(routes.contains_key("/api/orders"));

                let order_routes = &routes["/api/orders"];
                assert!(order_routes.contains_key("GET"));
                assert!(order_routes.contains_key("POST"));
            }
        }

        #[test]
        #[rustfmt::skip]
        fn test_route_builder_chaining() {
            // This test demonstrates the improved fluent API
            let config = SingleServiceConfigBuilder::new()
                .handler("common_handler")
                .route("/api/resource")
                    // Multiple methods on same route
                    .get()
                        .termination_handler("get_handler")
                        .end_method()
                    .post()
                        .request_handler("validator")
                        .termination_handler("create_handler")
                        .response_handler("logger")
                        .end_method()
                    .put()
                        .termination_handler("update_handler")
                        .end_method()
                    // Move to next route
                    .end_route()
                .route("/api/another")
                    .get()
                        .termination_handler("another_get")
                        .end_method()
                    .end_route()
                .build();

            // Verify both routes exist with their methods
            if let Routes::HttpRequestPaths(routes) = &config.routes {
                assert_eq!(routes.len(), 2);

                let resource_routes = &routes["/api/resource"];
                assert_eq!(resource_routes.len(), 3); // GET, POST, PUT

                let another_routes = &routes["/api/another"];
                assert_eq!(another_routes.len(), 1); // GET
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::router::config::{ConfigStructure, RouteType};

    #[test]
    fn load_shared_config() {
        let config_string = r#"
            {
              "Shared": {
                "key": {
                  "header": "x-service-name"
                },
                "services": {
                  "user_service": {
                    "handlers": [
                      "user_auth",
                      "user_validator",
                      "get_users",
                      "create_user",
                      "audit_logger"
                    ],
                    "chains": {
                      "user_auth_chain": ["user_auth", "user_validator"]
                    },
                    "routes": {
                      "path": {
                        "/users": {
                          "GET": {
                            "request": ["user_auth"],
                            "termination": "get_users"
                          },
                          "POST": {
                            "request": ["user_auth", "user_validator"],
                            "termination": "create_user",
                            "response": ["audit_logger"]
                          }
                        }
                      }
                    }
                  },
                  "order_service": {
                    "handlers": [
                      "order_auth",
                      "get_orders",
                      "create_order"
                    ],
                    "chains": {},
                    "routes": {
                      "path": {
                        "/orders": {
                          "GET": {
                            "request": ["order_auth"],
                            "termination": "get_orders"
                          },
                          "POST": {
                            "request": ["order_auth"],
                            "termination": "create_order"
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
        "#;
        let config = serde_json::from_str::<ConfigStructure>(config_string);
        assert!(config.is_ok());
        let config = config.unwrap();
        match config {
            ConfigStructure::Shared(shared_config) => {
                assert_eq!(shared_config.key, RouteType::header("x-service-name"));
            }
            _ => assert!(false, "Expected Shared config"),
        }
    }

    #[test]
    fn load_single_config() {
        let config_string = r#"{
          "Single": {
            "handlers": [
              "cors_handler",
              "auth_handler",
              "validator",
              "get_users",
              "create_user",
              "logger"
            ],
            "chains": {
              "auth_chain": ["cors_handler", "auth_handler"],
              "validation_chain": ["validator", "logger"]
            },
            "routes": {
              "path": {
                "/api/users": {
                  "GET": {
                    "request": ["cors_handler", "auth_handler"],
                    "termination": "get_users"
                  },
                  "POST": {
                    "request": ["cors_handler", "auth_handler", "validator"],
                    "termination": "create_user",
                    "response": ["logger"]
                  }
                },
                "/api/health": {
                  "GET": {
                    "termination": "health_check"
                  }
                }
              }
            }
          }
        }
        "#;
        let config = serde_json::from_str::<ConfigStructure>(config_string);
        assert!(config.is_ok());
        let config = config.unwrap();
        match config {
            ConfigStructure::Single(single) => {
                assert!(single.handlers.contains(&"cors_handler".to_string()));
            }
            _ => assert!(false, "Expected single config"),
        }
    }
}
