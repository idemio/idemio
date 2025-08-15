//! Router configuration structures and builders
//!
//! This module provides the configuration system for the router, supporting both
//! single service configurations and shared multiservice configurations. It includes
//! builder patterns for constructing complex routing configurations programmatically.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Top-level configuration structure for router services
///
/// This enum represents the two main configuration patterns supported:
/// - Single service configuration for simple routing scenarios
/// - Shared configuration for multiservice routing with different routing keys
#[derive(Debug, Serialize, Deserialize)]
pub enum ConfigStructure {
    /// Single service configuration
    ///
    /// Use this for simple routing scenarios where all routes belong to a single service.
    /// This is the most common configuration type for straightforward applications.
    #[serde(alias = "single", alias = "SINGLE")]
    Single(RouterConfig),

    /// Shared configuration for multiple services
    ///
    /// Use this for complex routing scenarios where multiple services need to be
    /// routed based on different criteria (headers, paths, etc.). Each service
    /// can have its own routing configuration.
    #[serde(alias = "shared", alias = "SHARED")]
    Shared(SharedConfig),
}

/// Types of routing keys supported by the router
///
/// This enum defines the different ways requests can be routed to services
/// in a shared configuration setup.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum RouteType {
    /// No specific routing - uses default routing behavior
    None,

    /// Route based on HTTP header value
    ///
    /// The string parameter specifies which header to examine for routing decisions.
    /// This is useful for service versioning, tenant isolation, or API key routing.
    #[serde(rename = "header", alias = "Header", alias = "HEADER")]
    Header(String),

    /// Route based on a request path
    ///
    /// This is the standard HTTP path-based routing used by most web applications.
    #[serde(rename = "path", alias = "Path", alias = "PATH")]
    Path,
}

impl RouteType {
    /// Create a header-based routing type
    ///
    /// # Arguments
    ///
    /// * `header_name` - The name of the HTTP header to use for routing
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let route_type = RouteType::header("X-Service-Version");
    /// ```
    pub fn header(header_name: impl Into<String>) -> Self {
        Self::Header(header_name.into())
    }

    /// Create a path-based routing type
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let route_type = RouteType::path();
    /// ```
    pub fn path() -> Self {
        Self::Path
    }
}

/// Configuration for shared multiservice routing
///
/// This structure defines how multiple services should be routed based on
/// a shared routing key. Each service can have its own complete router configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct SharedConfig {
    /// The routing key type that determines how to select services
    key: RouteType,
    /// Map of service names to their individual router configurations
    services: HashMap<String, RouterConfig>,
}

/// Main router configuration structure
///
/// This structure defines all the routing information for a service, including
/// - Available handlers
/// - Handler chains
/// - Route definitions mapping paths/methods to handler chains
///
/// # Fields
///
/// * `handlers` - Set of all available handler names
/// * `chains` - Named sequences of handlers that can be reused
/// * `routes` - Route definitions mapping requests to handler chains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Set of all handler names available for routing
    pub(crate) handlers: HashSet<String>,
    /// Named handler chains for reusability
    pub(crate) chains: HashMap<String, Vec<String>>,
    /// Route definitions specifying how requests are handled
    pub(crate) routes: Routes,
}

/// Route definition structures for different routing types
///
/// This enum supports different types of routing mechanisms, allowing
/// flexibility in how requests are matched to handler chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Routes {
    /// HTTP request path-based routing
    ///
    /// Maps paths to HTTP methods to handler chains. This is the standard
    /// web application routing pattern.
    ///
    /// Structure: `path -> method -> PathChain`
    #[serde(rename = "path", alias = "Path", alias = "PATH")]
    HttpRequestPaths(HashMap<String, HashMap<String, PathChain>>),

    /// HTTP-header-based routing
    ///
    /// Maps header values to HTTP methods to handler chains. This is useful
    /// for service versioning, tenant routing, or API key-based routing.
    ///
    /// Structure: `header_value -> method -> PathChain`
    #[serde(rename = "header", alias = "Header", alias = "HEADER")]
    HttpHeaderPaths(HashMap<String, HashMap<String, PathChain>>),
}

/// Handler chain definition for a specific route
///
/// This structure defines the complete handler execution pipeline for a route,
/// including request processing, termination, and response processing phases.
/// All phases are optional, providing flexibility in handler chain design.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathChain {
    /// Handlers executed during the request processing phase
    ///
    /// These handlers run before the termination handler and can modify
    /// the request, perform authentication, validation, etc.
    #[serde(skip_serializing_if = "Option::is_none", rename = "request")]
    pub(crate) request_handlers: Option<Vec<String>>,

    /// The termination handler that produces the final response
    ///
    /// This handler is responsible for generating the actual response content.
    /// Only one termination handler is allowed per route.
    #[serde(skip_serializing_if = "Option::is_none", rename = "termination")]
    pub(crate) termination_handler: Option<String>,

    /// Handlers executed during the response processing phase
    ///
    /// These handlers run after the termination handler and can modify
    /// the response, add headers, perform logging, etc.
    #[serde(skip_serializing_if = "Option::is_none", rename = "response")]
    pub(crate) response_handlers: Option<Vec<String>>,
}

impl PathChain {
    /// Create a new empty path chain
    ///
    /// # Returns
    ///
    /// A new `PathChain` with all handler lists set to `None`
    pub fn new() -> Self {
        Self {
            request_handlers: None,
            termination_handler: None,
            response_handlers: None,
        }
    }

    /// Add a handler to the request processing phase
    ///
    /// # Arguments
    ///
    /// * `handler` - Name of the handler to add to the request phase
    ///
    /// # Returns
    ///
    /// A mutable reference to self for method chaining
    fn add_request_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.request_handlers
            .get_or_insert_with(Vec::new)
            .push(handler.into());
        self
    }

    /// Set the termination handler for this path chain
    ///
    /// # Arguments
    ///
    /// * `handler` - Name of the termination handler
    ///
    /// # Returns
    ///
    /// A mutable reference to self for method chaining
    fn termination_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.termination_handler = Some(handler.into());
        self
    }

    /// Add a handler to the response processing phase
    ///
    /// # Arguments
    ///
    /// * `handler` - Name of the handler to add to the response phase
    ///
    /// # Returns
    ///
    /// A mutable reference to self for method chaining
    fn add_response_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.response_handlers
            .get_or_insert_with(Vec::new)
            .push(handler.into());
        self
    }
}

impl Default for PathChain {
    /// Create a default empty path chain
    fn default() -> Self {
        Self::new()
    }
}

/// Builder patterns for constructing router configurations
///
/// This module provides fluent builder APIs for creating complex router configurations
/// programmatically. It supports both single service and shared multiservice configurations.
pub mod builder {
    use super::*;

    /// Core configuration data shared across builders
    ///
    /// This structure holds the fundamental routing data that gets built up
    /// through the various builder patterns.
    pub struct ServiceConfigCore {
        /// Set of available handler names
        handlers: HashSet<String>,
        /// Named handler chains
        chains: HashMap<String, Vec<String>>,
        /// Route mappings from paths to methods to path chains
        routes: HashMap<String, HashMap<String, PathChain>>,
    }

    impl Default for ServiceConfigCore {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ServiceConfigCore {
        /// Create a new empty service configuration core
        pub fn new() -> Self {
            Self {
                handlers: HashSet::new(),
                chains: HashMap::new(),
                routes: HashMap::new(),
            }
        }

        /// Add a single handler to the configuration
        ///
        /// # Arguments
        ///
        /// * `handler_name` - Name of the handler to register
        pub fn add_handler(&mut self, handler_name: impl Into<String>) {
            self.handlers.insert(handler_name.into());
        }

        /// Add multiple handlers to the configuration
        ///
        /// # Arguments
        ///
        /// * `handler_names` - Slice of handler names to register
        pub fn add_handlers(&mut self, handler_names: &[impl AsRef<str>]) {
            for name in handler_names {
                self.handlers.insert(name.as_ref().to_string());
            }
        }

        /// Add a named handler chain to the configuration
        ///
        /// # Arguments
        ///
        /// * `chain_name` - Name of the chain for later reference
        /// * `handler_names` - Ordered list of handlers in the chain
        pub fn add_chain(
            &mut self,
            chain_name: impl Into<String>,
            handler_names: &[impl AsRef<str>],
        ) {
            let chain = handler_names
                .iter()
                .map(|name| name.as_ref().to_string())
                .collect();
            self.chains.insert(chain_name.into(), chain);
        }

        /// Ensure a route path exists in the configuration
        ///
        /// # Arguments
        ///
        /// * `path` - The route path to ensure exists
        pub fn ensure_route_exists(&mut self, path: &str) {
            self.routes.entry(path.to_string()).or_default();
        }

        /// Add a method handler to a specific path
        ///
        /// # Arguments
        ///
        /// * `path` - The route path
        /// * `method` - HTTP method (GET, POST, etc.)
        /// * `path_chain` - Handler chain configuration for this path/method
        pub fn add_method(&mut self, path: &str, method: String, path_chain: PathChain) {
            self.routes
                .entry(path.to_string())
                .or_default()
                .insert(method, path_chain);
        }

        /// Build the final router configuration
        ///
        /// # Returns
        ///
        /// A complete `RouterConfig` ready for use
        pub fn build(self) -> RouterConfig {
            RouterConfig {
                handlers: self.handlers,
                chains: self.chains,
                routes: Routes::HttpRequestPaths(self.routes),
            }
        }
    }

    /// Trait for service-level configuration builders
    ///
    /// This trait provides the common interface for building service configurations,
    /// supporting both single and shared service scenarios.
    pub trait ServiceBuilder: Sized {
        /// The route builder type for this service builder
        type RouteBuilder;

        /// Get mutable access to the core configuration
        fn core(&mut self) -> &mut ServiceConfigCore;

        /// Add a single handler to the service
        ///
        /// # Arguments
        ///
        /// * `handler_name` - Name of the handler to add
        fn handler(mut self, handler_name: impl Into<String>) -> Self {
            self.core().add_handler(handler_name);
            self
        }

        /// Add multiple handlers to the service
        ///
        /// # Arguments
        ///
        /// * `handler_names` - Slice of handler names to add
        fn handlers(mut self, handler_names: &[impl AsRef<str>]) -> Self {
            self.core().add_handlers(handler_names);
            self
        }

        /// Add a named handler chain to the service
        ///
        /// # Arguments
        ///
        /// * `chain_name` - Name of the chain
        /// * `handler_names` - Ordered handlers in the chain
        fn chain(
            mut self,
            chain_name: impl Into<String>,
            handler_names: &[impl AsRef<str>],
        ) -> Self {
            self.core().add_chain(chain_name, handler_names);
            self
        }

        /// Start building a route configuration
        ///
        /// # Arguments
        ///
        /// * `path` - The route path to configure
        fn route(self, path: impl Into<String>) -> Self::RouteBuilder;
    }

    /// Trait for route-level configuration builders
    ///
    /// This trait provides the interface for configuring individual routes
    /// and their associated HTTP methods.
    pub trait RouteBuilder: Sized {
        /// The method builder type for this route builder
        type MethodBuilder;
        /// The parent service builder type
        type ServiceBuilder;

        /// Create a method builder for a specific HTTP method
        ///
        /// # Arguments
        ///
        /// * `method` - The HTTP method to configure
        fn create_method_builder(self, method: impl Into<String>) -> Self::MethodBuilder;

        /// Finish configuring this route and return to the service builder
        fn end_route(self) -> Self::ServiceBuilder;

        /// Configure the HEAD method for this route
        fn head(self) -> Self::MethodBuilder {
            self.create_method_builder("HEAD")
        }

        /// Configure the OPTIONS method for this route
        fn options(self) -> Self::MethodBuilder {
            self.create_method_builder("OPTIONS")
        }

        /// Configure the GET method for this route
        fn get(self) -> Self::MethodBuilder {
            self.create_method_builder("GET")
        }

        /// Configure the POST method for this route
        fn post(self) -> Self::MethodBuilder {
            self.create_method_builder("POST")
        }

        /// Configure the PUT method for this route
        fn put(self) -> Self::MethodBuilder {
            self.create_method_builder("PUT")
        }

        /// Configure the DELETE method for this route
        fn delete(self) -> Self::MethodBuilder {
            self.create_method_builder("DELETE")
        }

        /// Configure the PATCH method for this route
        fn patch(self) -> Self::MethodBuilder {
            self.create_method_builder("PATCH")
        }
    }

    /// Trait for method-level configuration builders
    ///
    /// This trait provides the interface for configuring handler chains
    /// for specific route/method combinations.
    pub trait MethodBuilder: Sized {
        /// The parent route builder type
        type RouteBuilder;

        /// Get mutable access to the path chain being built
        fn path_chain(&mut self) -> &mut PathChain;

        /// Get access to the available handler chains
        fn chains(&self) -> &HashMap<String, Vec<String>>;

        /// Set multiple request handlers
        ///
        /// # Arguments
        ///
        /// * `handlers` - List of handler names for request processing
        fn request_handlers(mut self, handlers: &[impl AsRef<str>]) -> Self {
            let path_chain = self.path_chain();
            for handler in handlers {
                path_chain.add_request_handler(handler.as_ref().to_string());
            }
            self
        }

        /// Add a single request handler
        ///
        /// # Arguments
        ///
        /// * `handler` - Handler name for request processing
        fn request_handler(mut self, handler: impl Into<String>) -> Self {
            self.path_chain().add_request_handler(handler);
            self
        }

        /// Set the termination handler
        ///
        /// # Arguments
        ///
        /// * `handler` - Handler name for response generation
        fn termination_handler(mut self, handler: impl Into<String>) -> Self {
            self.path_chain().termination_handler(handler);
            self
        }

        /// Set multiple response handlers
        ///
        /// # Arguments
        ///
        /// * `handlers` - List of handler names for response processing
        fn response_handlers(mut self, handlers: &[impl AsRef<str>]) -> Self {
            let path_chain = self.path_chain();
            for handler in handlers {
                path_chain.add_response_handler(handler.as_ref().to_string());
            }
            self
        }

        /// Add a single response handler
        ///
        /// # Arguments
        ///
        /// * `handler` - Handler name for response processing
        fn response_handler(mut self, handler: impl Into<String>) -> Self {
            self.path_chain().add_response_handler(handler);
            self
        }

        /// Use a pre-defined chain for request processing
        ///
        /// # Arguments
        ///
        /// * `chain_name` - Name of the chain to use
        fn request_chain(mut self, chain_name: impl AsRef<str>) -> Self {
            if let Some(chain_handlers) = self.chains().get(chain_name.as_ref()) {
                let handlers: Vec<String> = chain_handlers.clone();
                for handler in handlers {
                    self.path_chain().add_request_handler(handler);
                }
            }
            self
        }

        /// Use a pre-defined chain for response processing
        ///
        /// # Arguments
        ///
        /// * `chain_name` - Name of the chain to use
        fn response_chain(mut self, chain_name: impl AsRef<str>) -> Self {
            if let Some(chain_handlers) = self.chains().get(chain_name.as_ref()) {
                let handlers: Vec<String> = chain_handlers.clone();
                for handler in handlers {
                    self.path_chain().add_response_handler(handler);
                }
            }
            self
        }

        /// Finish configuring this method and return to the route builder
        fn end_method(self) -> Self::RouteBuilder;
    }

    /// Builder for single service configurations
    ///
    /// This builder creates configurations for single-service routing scenarios,
    /// which is the most common use case for simple applications.
    pub struct SingleServiceConfigBuilder {
        /// Core configuration being built
        core: ServiceConfigCore,
    }

    impl Default for SingleServiceConfigBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SingleServiceConfigBuilder {
        /// Create a new single service configuration builder
        pub fn new() -> Self {
            Self {
                core: ServiceConfigCore::new(),
            }
        }

        /// Build the final router configuration
        ///
        /// # Returns
        ///
        /// A complete `RouterConfig` ready for use
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
            let path_str = path.into();
            self.core.ensure_route_exists(&path_str);
            SingleServiceRouteBuilder {
                config_builder: self,
                current_path: path_str,
            }
        }
    }

    /// Route builder for single service configurations
    pub struct SingleServiceRouteBuilder {
        /// Parent configuration builder
        config_builder: SingleServiceConfigBuilder,
        /// Path being configured
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

    /// Method builder for single service configurations
    pub struct SingleServiceMethodBuilder {
        /// Parent route builder
        route_builder: SingleServiceRouteBuilder,
        /// HTTP method being configured
        method: String,
        /// Path chain being built
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

    /// Builder for shared multiservice configurations
    ///
    /// This builder creates configurations for complex routing scenarios where
    /// multiple services need different routing strategies.
    pub struct SharedConfigBuilder {
        /// The routing key type
        key: RouteType,
        /// Map of services being configured
        services: HashMap<String, RouterConfig>,
    }

    impl SharedConfigBuilder {
        /// Create a new shared configuration builder
        pub fn new() -> Self {
            Self {
                key: RouteType::None,
                services: HashMap::new(),
            }
        }

        /// Set the routing type for service selection
        ///
        /// # Arguments
        ///
        /// * `route_type` - How to route between services
        pub fn route_type(mut self, route_type: RouteType) -> Self {
            self.key = route_type;
            self
        }

        /// Start configuring a specific service
        ///
        /// # Arguments
        ///
        /// * `service_name` - Name of the service to configure
        pub fn service(self, service_name: impl Into<String>) -> SharedServiceBuilder {
            SharedServiceBuilder {
                shared_builder: self,
                service_name: service_name.into(),
                core: ServiceConfigCore::new(),
            }
        }

        /// Build the final shared configuration
        ///
        /// # Returns
        ///
        /// A complete `SharedConfig` ready for use
        pub fn build(self) -> SharedConfig {
            SharedConfig {
                key: self.key,
                services: self.services,
            }
        }
    }

    /// Service builder for shared configurations
    pub struct SharedServiceBuilder {
        /// Parent shared configuration builder
        shared_builder: SharedConfigBuilder,
        /// Name of the service being configured
        service_name: String,
        /// Core configuration for this service
        core: ServiceConfigCore,
    }

    impl SharedServiceBuilder {
        /// Finish configuring this service and return to the shared builder
        pub fn end_service(mut self) -> SharedConfigBuilder {
            let config = self.core.build();
            self.shared_builder
                .services
                .insert(self.service_name, config);
            self.shared_builder
        }
    }

    impl ServiceBuilder for SharedServiceBuilder {
        type RouteBuilder = SharedServiceRouteBuilder;

        fn core(&mut self) -> &mut ServiceConfigCore {
            &mut self.core
        }

        fn route(mut self, path: impl Into<String>) -> Self::RouteBuilder {
            let path_str = path.into();
            self.core.ensure_route_exists(&path_str);
            SharedServiceRouteBuilder {
                service_builder: self,
                current_path: path_str,
            }
        }
    }

    /// Route builder for shared service configurations
    pub struct SharedServiceRouteBuilder {
        /// Parent service builder
        service_builder: SharedServiceBuilder,
        /// Path being configured
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

    /// Method builder for shared service configurations
    pub struct SharedServiceMethodBuilder {
        /// Parent route builder
        route_builder: SharedServiceRouteBuilder,
        /// HTTP method being configured
        method: String,
        /// Path chain being built
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

        /// Test building configurations with multiple methods on the same route
        #[test]
        #[rustfmt::skip]
        fn test_single_service_multiple_methods_same_route() {
            let config = SingleServiceConfigBuilder::new()
                .handler("handler1")
                .handler("handler2")
                .route("/api/test")
                    .get()
                        .request_handler("handler1")
                        .termination_handler("handler2")
                    .end_method()
                    .post()
                        .request_handler("handler1")
                        .termination_handler("handler2")
                    .end_method()
                .end_route()
                .build();

            assert!(config.handlers.contains("handler1"));
            assert!(config.handlers.contains("handler2"));
        }

        /// Test shared service configuration building
        #[test]
        #[rustfmt::skip]
        fn test_shared_service_multiple_methods_same_route() {
            let config = SharedConfigBuilder::new()
                .route_type(RouteType::path())
                .service("service1")
                    .handler("handler1")
                    .route("/api/test")
                        .get()
                            .termination_handler("handler1")
                        .end_method()
                    .end_route()
                .end_service()
                .build();

            assert!(config.services.contains_key("service1"));
        }

        /// Test fluent builder chaining
        #[test]
        #[rustfmt::skip]
        fn test_route_builder_chaining() {
            let config = SingleServiceConfigBuilder::new()
                .handlers(&["auth", "validate", "process", "respond"])
                .chain("request_chain", &["auth", "validate"])
                .chain("response_chain", &["respond"])
                .route("/api/users")
                    .get()
                        .request_chain("request_chain")
                        .termination_handler("process")
                        .response_chain("response_chain")
                    .end_method()
                    .post()
                        .request_handlers(&["auth", "validate"])
                        .termination_handler("process")
                        .response_handler("respond")
                    .end_method()
                .end_route()
                .build();

            assert_eq!(config.handlers.len(), 4);
            assert_eq!(config.chains.len(), 2);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json;

    /// Test loading shared configuration from JSON
    #[test]
    fn load_shared_config() {
        let json_config = r#"
        {
            "shared": {
                "key": "path",
                "services": {
                    "api_v1": {
                        "handlers": ["auth", "handler1"],
                        "chains": {},
                        "routes": {
                            "path": {
                                "/users": {
                                    "GET": {
                                        "request": ["auth"],
                                        "termination": "handler1"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "#;

        let config: ConfigStructure = serde_json::from_str(json_config).unwrap();
        match config {
            ConfigStructure::Shared(shared) => {
                assert_eq!(shared.key, RouteType::Path);
                assert!(shared.services.contains_key("api_v1"));
            }
            _ => panic!("Expected shared configuration"),
        }
    }

    /// Test loading a single service configuration from JSON
    #[test]
    fn load_single_config() {
        let json_config = r#"
        {
            "single": {
                "handlers": ["handler1", "handler2"],
                "chains": {
                    "auth_chain": ["handler1", "handler2"]
                },
                "routes": {
                    "path": {
                        "/test": {
                            "GET": {
                                "termination": "handler1"
                            }
                        }
                    }
                }
            }
        }
        "#;

        let config: ConfigStructure = serde_json::from_str(json_config).unwrap();
        match config {
            ConfigStructure::Single(single) => {
                assert!(single.handlers.contains("handler1"));
                assert!(single.handlers.contains("handler2"));
                assert!(single.chains.contains_key("auth_chain"));
            }
            _ => panic!("Expected single configuration"),
        }
    }
}