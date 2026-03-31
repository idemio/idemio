use std::collections::{HashMap, HashSet};
use crate::router::config::{PathChain};
use crate::router::RouterConfig;

#[derive(Default)]
/// 'Core' configuration data shared across builders
pub struct ServiceConfigCore {
    /// Set of available handler names
    handlers: HashSet<String>,
    /// Named handler chains
    chains: HashMap<String, Vec<String>>,
    /// Route mappings from paths to methods to path chains
    paths: HashMap<String, HashMap<String, PathChain>>,
}

impl ServiceConfigCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a single handler to the configuration
    pub fn add_handler(&mut self, handler_name: impl Into<String>) {
        self.handlers.insert(handler_name.into());
    }

    /// Add multiple handlers to the configuration
    pub fn add_handlers(&mut self, handler_names: &[impl AsRef<str>]) {
        for name in handler_names {
            self.handlers.insert(name.as_ref().to_string());
        }
    }

    /// Add a named handler chain to the configuration
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
    pub fn ensure_route_exists(&mut self, path: &str) {
        self.paths.entry(path.to_string()).or_default();
    }

    /// Add a method handler to a specific path
    pub fn add_method(&mut self, path: &str, method: String, path_chain: PathChain) {
        self.paths
            .entry(path.to_string())
            .or_default()
            .insert(method, path_chain);
    }

    /// Build the final router configuration
    pub fn build(self) -> RouterConfig {
        RouterConfig {
            handlers: self.handlers,
            chains: self.chains,
            paths: self.paths,
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
    fn handler(mut self, handler_name: impl Into<String>) -> Self {
        self.core().add_handler(handler_name);
        self
    }

    /// Add multiple handlers to the service
    fn handlers(mut self, handler_names: &[impl AsRef<str>]) -> Self {
        self.core().add_handlers(handler_names);
        self
    }

    /// Add a named handler chain to the service
    fn chain(
        mut self,
        chain_name: impl Into<String>,
        handler_names: &[impl AsRef<str>],
    ) -> Self {
        self.core().add_chain(chain_name, handler_names);
        self
    }

    /// Start building a route configuration
    fn route(self, path: impl Into<String>) -> Self::RouteBuilder;
}

/// Trait for route-level configuration builders
pub trait RouteBuilder: Sized {
    /// The method builder type for this route builder
    type MethodBuilder;
    /// The parent service builder type
    type ServiceBuilder;

    /// Create a method builder for a specific HTTP method
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
pub trait MethodBuilder: Sized {
    /// The parent route builder type
    type RouteBuilder;

    /// Get mutable access to the path chain being built
    fn path_chain(&mut self) -> &mut PathChain;

    /// Get access to the available handler chains
    fn chains(&self) -> &HashMap<String, Vec<String>>;

    /// Set multiple request handlers
    fn request_handlers(mut self, handlers: &[impl AsRef<str>]) -> Self {
        let path_chain = self.path_chain();
        for handler in handlers {
            path_chain.add_request_handler(handler.as_ref().to_string());
        }
        self
    }

    /// Add a single request handler
    fn request_handler(mut self, handler: impl Into<String>) -> Self {
        self.path_chain().add_request_handler(handler);
        self
    }

    /// Set the termination handler
    fn termination_handler(mut self, handler: impl Into<String>) -> Self {
        self.path_chain().termination_handler(handler);
        self
    }

    /// Set multiple response handlers
    fn response_handlers(mut self, handlers: &[impl AsRef<str>]) -> Self {
        let path_chain = self.path_chain();
        for handler in handlers {
            path_chain.add_response_handler(handler.as_ref().to_string());
        }
        self
    }

    /// Add a single response handler
    fn response_handler(mut self, handler: impl Into<String>) -> Self {
        self.path_chain().add_response_handler(handler);
        self
    }

    /// Use a pre-defined chain for request processing
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
#[derive(Default)]
pub struct SingleServiceConfigBuilder {
    /// Core configuration being built
    core: ServiceConfigCore,
}

impl SingleServiceConfigBuilder {
    /// Create a new single service configuration builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the final router configuration
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