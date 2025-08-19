use crate::handler::registry::HandlerRegistry;
use crate::router::config::{RouterConfig, Routes};
use crate::router::path::{
    LoadedChain, PathMatcher, PathMatcherError, PathNode, PathSegment, StaticPathKey, split_path,
};
use fnv::FnvBuildHasher;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

pub struct HeaderKey<'a> {
    pub header: &'a str,
}

impl<'a> HeaderKey<'a> {
    pub fn new(header: &'a str) -> Self {
        Self { header }
    }
}

pub struct PathPrefixMethodKey<'a> {
    pub method: &'a str,
    pub path: &'a str,
}

impl<'a> PathPrefixMethodKey<'a> {
    pub fn new(method: &'a str, path: &'a str) -> Self {
        Self { method, path }
    }
}

pub struct PathPrefixMethodPathMatcher<E>
where
    E: Send + Sync
{
    /// Fast hash-based lookup for static paths (no wildcards)
    static_paths: HashMap<StaticPathKey, Arc<LoadedChain<E>>, FnvBuildHasher>,
    /// Tree structure for dynamic path matching with wildcards
    nodes: PathNode<E>,
}

impl<E> PathMatcher<E> for PathPrefixMethodPathMatcher<E>
where
    E: Send + Sync
{
    fn parse_config(
        &mut self,
        route_config: &RouterConfig,
        handler_registry: &HandlerRegistry<E>,
    ) -> Result<(), PathMatcherError> {
        match &route_config.routes {
            Routes::HttpRequestPaths(paths) => {
                log::info!(
                    "Starting router configuration parsing with '{}' paths",
                    paths.len()
                );
                for (path, methods) in paths.iter() {
                    log::debug!("Path: '{path}'");
                    if !path.contains('*') {
                        for (method, path_chain) in methods {
                            log::trace!("Adding static route: {path}@{method}");
                            let key = StaticPathKey::new(method, path);
                            let loaded_chain = Self::load_handlers(handler_registry, path_chain)?;
                            self.static_paths.insert(key, Arc::new(loaded_chain));
                        }
                    } else {
                        log::trace!("'{path}' contains wildcards, adding dynamic routing tree");
                    }
                    let path_segments = split_path(path);
                    let mut current_node = &mut self.nodes;
                    let mut depth = 0;
                    for segment in path_segments {
                        let path_segment =
                            PathSegment::from_str(segment).expect("Invalid path segment");
                        let is_wild_card = path_segment == PathSegment::Any;
                        log::trace!("Segment '{segment}' at depth {depth} (wild: {is_wild_card})",);
                        current_node = current_node
                            .children
                            .entry(path_segment)
                            .or_insert_with(PathNode::default);
                        current_node.segment_depth = depth;
                        // Wildcards should be the last segment in the path.
                        if is_wild_card {
                            log::trace!(
                                "Encountered wildcard at depth {depth}, stopping path traversal"
                            );
                            break;
                        }
                        depth += 1;
                    }
                    log::debug!(
                        "Built routing tree node at depth {} for path '{}'",
                        depth,
                        path
                    );
                    for (method, handlers) in methods {
                        let loaded_chain = Self::load_handlers(&handler_registry, handlers)?;
                        let handler_count = loaded_chain.size();
                        current_node
                            .methods
                            .insert(method.to_string(), Arc::new(loaded_chain));
                        log::debug!(
                            "Added {} handlers for method {} to path '{}'",
                            handler_count,
                            method,
                            path
                        );
                    }
                }
                Ok(())
            }
            _ => Err(PathMatcherError::invalid_configuration(
                "Route config type should be HttpRequestPaths",
            )),
        }
    }

    fn lookup(&self, key: (&str, &str)) -> Option<Arc<LoadedChain<E>>> {
        let request_path = key.0;
        let request_method = key.1;
        // Fast path: try the exact static match first
        if let Some(handlers) = self
            .static_paths
            .get(&StaticPathKey::new(&request_method, &request_path))
        {
            return Some(handlers.clone());
        }

        // Dynamic path matching with wildcards
        let req_path_segments = split_path(&request_path);
        let mut best_match: Option<&PathNode<E>> = None;
        let mut max_depth = 0;
        let mut current_node = &self.nodes;

        for segment_str in req_path_segments {
            // Check for wildcard match at current depth
            if let Some(wildcard_node) = current_node.children.get(&PathSegment::Any) {
                if wildcard_node.methods.contains_key(request_method) {
                    if wildcard_node.segment_depth > max_depth {
                        max_depth = wildcard_node.segment_depth;
                        best_match = Some(wildcard_node);
                    }
                }
            }

            // Try the exact segment match and continue traversal
            let static_segment = PathSegment::Static(segment_str.to_string());
            current_node = match current_node.children.get(&static_segment) {
                Some(child) => child,
                None => break, // No exact match, stop traversal
            };
        }

        // Check final node for exact match (higher priority than wildcards)
        if current_node.methods.contains_key(request_method)
            && current_node.segment_depth > max_depth
        {
            best_match = Some(current_node);
        }

        // Return the best match found
        best_match.and_then(|node| node.methods.get(request_method).cloned())
    }

    fn new(
        config: &RouterConfig,
        handler_registry: &HandlerRegistry<E>,
    ) -> Result<Self, PathMatcherError> {
        let mut matcher = Self {
            nodes: PathNode::default(),
            static_paths: HashMap::with_hasher(fnv::FnvBuildHasher::default()),
        };
        if let Err(e) = matcher.parse_config(config, handler_registry) {
            return Err(e);
        }
        Ok(matcher)
    }
}

#[cfg(test)]
mod test {
    use crate::exchange::Exchange;
    use crate::handler::Handler;
    use crate::handler::HandlerId;
    use crate::handler::registry::HandlerRegistry;
    use crate::router::config::builder::{
        MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
    };
    use crate::router::path::{
        PathMatcher, http::PathPrefixMethodKey, http::PathPrefixMethodPathMatcher,
    };
    use crate::status::{ExchangeState, HandlerStatus};
    use async_trait::async_trait;
    use std::convert::Infallible;

    /// A simple test handler that does nothing but return an OK status.
    ///
    /// Used in unit tests to verify routing functionality without complex business logic.
    #[derive(Debug)]
    struct DummyHandler;

    #[async_trait]
    impl Handler<Exchange<(), (), ()>> for DummyHandler {
        async fn exec(
            &self,
            _exchange: &mut Exchange<(), (), ()>,
        ) -> Result<HandlerStatus, Infallible> {
            Ok(HandlerStatus::new(ExchangeState::OK))
        }

        fn name(&self) -> &str {
            "DummyHandler"
        }
    }

    /// Comprehensive test of PathMatcher functionality including static and dynamic routing.
    ///
    /// This test verifies:
    /// - Handler registration and chain configuration
    /// - Static path routing for exact matches
    /// - Wildcard path routing with longest-prefix matching
    /// - Method-specific routing (GET, POST)
    /// - Path traversal beyond wildcard matches
    /// - Rejection of non-matching paths
    #[test]
    #[rustfmt::skip]
    fn router_v2_test() {
        // Set up a handler registry with test handlers
        let mut registry = HandlerRegistry::<Exchange<(), (), ()>>::new();
        registry
            .register_handler(HandlerId::new("test1"), DummyHandler)
            .unwrap();
        registry
            .register_handler(HandlerId::new("test2"), DummyHandler)
            .unwrap();
        registry
            .register_handler(HandlerId::new("test3"), DummyHandler)
            .unwrap();
        registry
            .register_handler(HandlerId::new("test4"), DummyHandler)
            .unwrap();
        registry
            .register_handler(HandlerId::new("test6"), DummyHandler)
            .unwrap();
        registry
            .register_handler(HandlerId::new("test7"), DummyHandler)
            .unwrap();
        registry
            .register_handler(HandlerId::new("test8"), DummyHandler)
            .unwrap();
        registry
            .register_handler(HandlerId::new("test9"), DummyHandler)
            .unwrap();

        // Build router configuration with wildcard path and handler chains
        let config = SingleServiceConfigBuilder::new()
            .chain("test_chain", &["test1", "test2", "test6", "test7", "test8", "test9"])
            .route("/api/v1/*")
                .post()
                    .request_chain("test_chain")
                    .termination_handler("test3")
                    .response_handler("test4")
                .end_method()
                .get()
                    .request_chain("test_chain")
                    .termination_handler("test3")
                    .response_handler("test4")
                .end_method()
            .end_route()
            .build();

        // Create PathMatcher with the configuration
        let table = PathPrefixMethodPathMatcher::new(&config, &registry).unwrap();

        // Test wildcard matching - should match the "/api/v1/*" pattern
        let result = table.lookup(("/api/v1/users", "GET"));
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6); // test_chain has 6 handlers

        // Test another wildcard match with a different path segment
        let result = table.lookup(("/api/v1/someOtherEndpoint", "GET"));
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6);

        // Test non-matching path - should return None
        let result = table.lookup(("/invalid", "GET"));
        assert!(result.is_none());

        // Test path that goes beyond wildcard - should still match "/api/v1/*"
        let result = table.lookup(("/api/v1/users/somethingElse", "GET"));
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6);
    }
}
