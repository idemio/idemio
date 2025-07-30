use crate::handler::registry::{HandlerRegistry, HandlerRegistryError};
use crate::handler::{Handler, HandlerId};
use crate::router::config::Routes;
pub use crate::router::config::{PathChain, RouterConfig};
use fnv::{FnvBuildHasher, FnvHasher};
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::iter::Filter;
use std::str::{FromStr, Split};
use std::sync::Arc;

/// A collection of handlers that are ready to be executed in a specific order.
///
/// `LoadedChain` represents a complete request processing pipeline consisting of:
/// - Request handlers: Process incoming data before the main handler
/// - Termination handler: The main handler that processes the request and generates output
/// - Response handlers: Process outgoing data after the main handler
pub struct LoadedChain<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    request_handlers: Vec<Arc<dyn Handler<In, Out, Meta>>>,
    termination_handler: Arc<dyn Handler<In, Out, Meta>>,
    response_handlers: Vec<Arc<dyn Handler<In, Out, Meta>>>,
}

impl<In, Out, Meta> LoadedChain<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Creates a new `LoadedChain` with the specified handlers.
    ///
    /// # Parameters
    /// - `request_handlers`: Vector of handlers to execute before the termination handler
    /// - `termination_handler`: The main handler that processes the request
    /// - `response_handlers`: Vector of handlers to execute after the termination handler
    ///
    /// # Returns
    /// A new `LoadedChain` instance containing the provided handlers.
    pub(crate) fn new(
        request_handlers: Vec<Arc<dyn Handler<In, Out, Meta>>>,
        termination_handler: Arc<dyn Handler<In, Out, Meta>>,
        response_handlers: Vec<Arc<dyn Handler<In, Out, Meta>>>,
    ) -> Self {
        Self {
            request_handlers,
            termination_handler,
            response_handlers,
        }
    }

    /// Returns the total number of handlers in this chain.
    ///
    /// # Returns
    /// The total count of all handlers (request + termination + response handlers).
    ///
    /// # Examples
    /// ```text
    ///  let chain = LoadedChain::new(
    ///     vec![handler1, handler2], // 2 request handlers
    ///     termination_handler,      // 1 termination handler
    ///     vec![response_handler]    // 1 response handler
    /// );
    ///
    /// chain.size() -> 2 + 1 + 1 = 4
    /// ```
    ///
    /// # Behavior
    /// This method always returns at least 1, as every chain must have a termination handler.
    /// The calculation is: request_handlers.len() + 1 + response_handlers.len().
    pub fn size(&self) -> usize {
        self.request_handlers.len() + 1 + self.response_handlers.len()
    }

    /// Returns a reference to the request handlers vector.
    pub fn request_handlers(&self) -> &Vec<Arc<dyn Handler<In, Out, Meta>>> {
        &self.request_handlers
    }

    /// Returns a reference to the termination handler.
    pub fn termination_handler(&self) -> &Arc<dyn Handler<In, Out, Meta>> {
        &self.termination_handler
    }

    /// Returns a reference to the response handlers vector.
    pub fn response_handlers(&self) -> &Vec<Arc<dyn Handler<In, Out, Meta>>> {
        &self.response_handlers
    }
}

/// A key used for fast lookup of static paths (paths without wildcards) in the router.
///
/// This is an internal optimization structure that uses FNV hashing for efficient
/// method and path combination lookups.
#[derive(Clone, PartialEq, Eq, Hash)]
struct StaticPathKey {
    method_hash: u64,
    path_hash: u64,
}

impl StaticPathKey {
    /// Creates a new StaticPathKey from method and path strings.
    ///
    /// # Parameters
    /// - `method`: HTTP method string (e.g., "GET", "POST")
    /// - `path`: URL path string (e.g., "/api/users")
    ///
    /// # Returns
    /// A new StaticPathKey with hashed method and path values.
    pub fn new(method: impl AsRef<str>, path: impl AsRef<str>) -> Self {
        let path = path.as_ref();
        let method = method.as_ref();
        let mut path_hasher = FnvHasher::default();
        path.hash(&mut path_hasher);
        let path_hash = path_hasher.finish();
        let mut method_hasher = FnvHasher::default();
        method.hash(&mut method_hasher);
        let method_hash = method_hasher.finish();
        Self {
            method_hash,
            path_hash,
        }
    }
}

/// Represents a segment in a URL path, which can be either static text or a wildcard.
///
/// Path segments are used to build the routing tree structure for dynamic path matching.
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PathSegment {
    /// A static path segment containing literal text
    Static(String),
    /// A wildcard segment that matches any value
    Any,
}

impl Display for PathSegment {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PathSegment::Static(s) => write!(f, "{}", s),
            PathSegment::Any => write!(f, "*"),
        }
    }
}

impl FromStr for PathSegment {
    type Err = Infallible;

    /// Parses a string into a PathSegment.
    ///
    /// # Parameters
    /// - `s`: String slice to parse, "*" for wildcard, anything else for static
    ///
    /// # Returns
    /// Always succeeds with either PathSegment::Any for "*" or PathSegment::Static for other values.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "*" {
            Ok(PathSegment::Any)
        } else {
            Ok(PathSegment::Static(s.to_string()))
        }
    }
}

/// A node in the path routing tree structure.
///
/// Each node represents a path segment and can contain child nodes and method handlers.
struct PathNode<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    children: HashMap<PathSegment, PathNode<In, Out, Meta>, FnvBuildHasher>,
    segment_depth: u64,
    methods: HashMap<String, Arc<LoadedChain<In, Out, Meta>>, FnvBuildHasher>,
}

impl<In, Out, Meta> Default for PathNode<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    fn default() -> Self {
        Self {
            children: HashMap::with_hasher(FnvBuildHasher::default()),
            methods: HashMap::with_hasher(FnvBuildHasher::default()),
            segment_depth: 0,
        }
    }
}

/// A high-performance router for matching HTTP paths to handler chains.
///
/// The router uses a hybrid approach:
/// - Static paths (without wildcards) use a fast hash-based lookup
/// - Dynamic paths (with wildcards) use a tree-based matching algorithm
///
/// Wildcard segments ("*") match any path segment and are resolved with longest-prefix matching.
pub struct PathRouter<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    static_paths: HashMap<StaticPathKey, Arc<LoadedChain<In, Out, Meta>>, FnvBuildHasher>,
    nodes: PathNode<In, Out, Meta>,
}

impl<In, Out, Meta> PathRouter<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Creates a new PathRouter from the provided configuration and handler registry.
    ///
    /// # Parameters
    /// - `route_config`: Configuration containing path definitions and handler chains
    /// - `registry`: Registry containing all available handlers referenced in the config
    ///
    /// # Returns
    /// A Result containing the constructed PathRouter or a PathRouterError if the configuration is invalid.
    ///
    /// # Behavior
    /// - Validates all handler references to exist in the registry
    /// - Builds internal routing structures for both static and dynamic paths
    /// - Static paths (no wildcards) are stored in a hash map for O(1) lookup
    /// - Dynamic paths are stored in a tree structure for pattern matching
    /// - Returns an error if any handler is missing or configuration is invalid
    pub fn new(
        route_config: &RouterConfig,
        registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<Self, PathRouterError> {
        let mut router = Self {
            static_paths: HashMap::with_hasher(FnvBuildHasher::default()),
            nodes: PathNode::default(),
        };
        if let Err(e) = router.parse_config(route_config, registry) {
            return Err(e);
        }
        Ok(router)
    }

    /// Finds a handler by ID in the registry.
    ///
    /// # Parameters
    /// - `handler`: String identifier for the handler
    /// - `handler_registry`: Registry to search in
    ///
    /// # Returns
    /// Result containing the handler Arc or PathRouterError if not found.
    fn find_in_registry(
        handler: &str,
        handler_registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<Arc<dyn Handler<In, Out, Meta>>, PathRouterError> {
        let handler_id = HandlerId::new(handler);
        match handler_registry.find_with_id(&handler_id) {
            Ok(handler) => Ok(handler),
            Err(e) => Err(PathRouterError::registry_error(e)),
        }
    }

    /// Finds multiple handlers by their IDs in the registry.
    ///
    /// # Parameters
    /// - `handlers`: Slice of handler ID strings
    /// - `handler_registry`: Registry to search in
    ///
    /// # Returns
    /// Result containing a vector of handler Arcs or PathRouterError if any handler is missing.
    fn find_all_in_registry(
        handlers: &[String],
        handler_registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<Vec<Arc<dyn Handler<In, Out, Meta>>>, PathRouterError> {
        let mut registered_handlers = vec![];
        for handler in handlers {
            let registered_handler = Self::find_in_registry(handler, handler_registry)?;
            registered_handlers.push(registered_handler);
        }
        Ok(registered_handlers)
    }

    /// Loads handlers from the registry based on a path chain configuration.
    ///
    /// # Parameters
    /// - `handler_registry`: Registry containing available handlers
    /// - `path_chain`: Configuration specifying which handlers to load
    ///
    /// # Returns
    /// Result containing a LoadedChain or PathRouterError if handlers are missing or invalid.
    fn load_handlers(
        handler_registry: &HandlerRegistry<In, Out, Meta>,
        path_chain: &PathChain,
    ) -> Result<LoadedChain<In, Out, Meta>, PathRouterError> {
        let registered_request_handlers = match &path_chain.request_handlers {
            Some(handlers) => Self::find_all_in_registry(handlers, handler_registry)?,
            None => vec![],
        };
        let registered_termination_handler = match &path_chain.termination_handler {
            Some(handler) => Self::find_in_registry(handler, handler_registry)?,
            None => {
                return Err(PathRouterError::invalid_method(
                    "No termination handler specified",
                ));
            }
        };
        let registered_response_handlers = match &path_chain.response_handlers {
            Some(handlers) => Self::find_all_in_registry(handlers, handler_registry)?,
            None => vec![],
        };
        Ok(LoadedChain::new(
            registered_request_handlers,
            registered_termination_handler,
            registered_response_handlers,
        ))
    }

    /// Parses the route configuration and builds internal routing structures.
    ///
    /// # Parameters
    /// - `route_config`: Configuration containing path and handler definitions
    /// - `handler_registry`: Registry containing available handlers
    ///
    /// # Returns
    /// Result indicating success or PathRouterError if configuration is invalid.
    fn parse_config(
        &mut self,
        route_config: &RouterConfig,
        handler_registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<(), PathRouterError> {
        match &route_config.routes {
            Routes::HttpRequestPaths(paths) => {
                log::info!(
                    "Starting router configuration parsing with '{}' paths",
                    paths.len()
                );
                for (path, methods) in paths.iter() {
                    log::debug!("Processing path: '{}'", path);
                    // static paths (ones that do not contain '*') should be added to the fast path.
                    if !path.contains('*') {
                        log::trace!("Path '{}' is static, adding to fast path lookup", path);
                        for (method, path_chain) in methods {
                            log::trace!("Adding static route: {} {}", method, path);
                            let key = StaticPathKey::new(method, path);
                            let loaded_chain = Self::load_handlers(handler_registry, path_chain)?;
                            self.static_paths.insert(key, Arc::new(loaded_chain));
                        }
                    } else {
                        log::trace!(
                            "Path '{}' contains wildcards, will be added to dynamic routing tree",
                            path
                        );
                    }
                    let path_segments = Self::split_path(path);
                    let mut current_node = &mut self.nodes;
                    let mut depth = 0;
                    for segment in path_segments {
                        let path_segment =
                            PathSegment::from_str(segment).expect("Invalid path segment");
                        let is_wild_card = path_segment == PathSegment::Any;
                        log::trace!(
                            "Processing segment '{}' at depth {} (wildcard: {})",
                            segment,
                            depth,
                            is_wild_card
                        );
                        current_node = current_node
                            .children
                            .entry(path_segment)
                            .or_insert_with(PathNode::default);
                        current_node.segment_depth = depth;
                        // wildcards should be the last segment in the path.
                        if is_wild_card {
                            log::trace!(
                                "Encountered wildcard at depth {}, stopping path traversal",
                                depth
                            );
                            break;
                        }
                        depth += 1;
                    }
                    log::debug!(
                        "Built routing tree node at depth {} for path '{}'",
                        depth - 1,
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
            Routes::HttpHeaderPaths(_) => todo!("implement header routing... and others"),
        }
    }

    /// Splits a path string into individual segments, filtering out empty segments.
    ///
    /// # Parameters
    /// - `path`: Path string to split (e.g., "/api/v1/users")
    ///
    /// # Returns
    /// An iterator over non-empty path segments.
    fn split_path(path: &str) -> Filter<Split<char>, fn(&&str) -> bool> {
        path.split('/').filter(|s| !s.is_empty())
    }

    /// Looks up a handler chain for the given request path and HTTP method.
    ///
    /// # Parameters
    /// - `request_path`: The URL path to match (e.g., "/api/v1/users/123")
    /// - `request_method`: The HTTP method (e.g., "GET", "POST")
    ///
    /// # Returns
    /// An Option containing the Arc-wrapped LoadedChain if a match is found, None otherwise.
    ///
    /// # Behavior
    ///     - First attempts fast hash-based lookup for static paths
    ///     - Falls back to tree traversal for dynamic paths with wildcards
    ///     - Uses longest-prefix matching when multiple wildcard patterns could match
    ///     - Returns None if no matching route is found for the method/path combination
    ///     - Wildcard segments ("*") match exactly one path segment
    ///
    pub fn lookup(
        &self,
        request_path: &str,
        request_method: &str,
    ) -> Option<Arc<LoadedChain<In, Out, Meta>>> {
        if let Some(handlers) = self
            .static_paths
            .get(&StaticPathKey::new(request_method, request_path))
        {
            return Some(handlers.clone());
        }
        let req_path_segments = Self::split_path(request_path);
        let mut best_match: Option<&PathNode<In, Out, Meta>> = None;
        let mut max_depth = 0;
        let mut current_node = &self.nodes;
        for segment_str in req_path_segments {
            if let Some(wildcard_node) = current_node.children.get(&PathSegment::Any) {
                if wildcard_node.methods.contains_key(request_method) {
                    if wildcard_node.segment_depth > max_depth {
                        max_depth = wildcard_node.segment_depth;
                        best_match = Some(wildcard_node);
                    }
                }
            }
            // Try exact match
            let static_segment = PathSegment::Static(segment_str.to_string());
            current_node = match current_node.children.get(&static_segment) {
                Some(child) => child,
                None => break,
            };
        }
        // Check final node for exact match
        if current_node.methods.contains_key(request_method)
            && current_node.segment_depth > max_depth
        {
            best_match = Some(current_node);
        }
        best_match.and_then(|node| node.methods.get(request_method).cloned())
    }
}

/// Errors that can occur during PathRouter construction and operation.
///
/// This enum represents all possible error conditions when working with the PathRouter,
/// including configuration issues and handler registry problems.
#[derive(Debug)]
pub enum PathRouterError {
    /// An invalid path was provided in the configuration
    InvalidPath(String),
    /// An invalid HTTP method or missing termination handler was specified
    InvalidMethod(String),
    /// An error occurred while accessing the handler registry
    HandlerRegistryError(HandlerRegistryError),
}

impl PathRouterError {
    /// Creates a new InvalidPath error.
    #[inline]
    pub(crate) fn invalid_path(path: impl Into<String>) -> Self {
        Self::InvalidPath(path.into())
    }

    /// Creates a new InvalidMethod error.
    #[inline]
    pub(crate) fn invalid_method(method: impl Into<String>) -> Self {
        Self::InvalidMethod(method.into())
    }

    /// Creates a new HandlerRegistryError wrapper.
    #[inline]
    pub(crate) const fn registry_error(registry_error: HandlerRegistryError) -> Self {
        Self::HandlerRegistryError(registry_error)
    }
}

impl Display for PathRouterError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PathRouterError::InvalidPath(path) => {
                write!(f, "Invalid path: {}", path)
            }
            PathRouterError::InvalidMethod(method) => {
                write!(f, "Invalid method: {}", method)
            }
            PathRouterError::HandlerRegistryError(error) => {
                write!(f, "{}", error)
            }
        }
    }
}

impl std::error::Error for PathRouterError {}

#[cfg(test)]
mod test {
    use crate::exchange::Exchange;
    use crate::handler::Handler;
    use crate::handler::HandlerId;
    use crate::handler::registry::HandlerRegistry;
    use crate::router::config::Routes;
    use crate::router::config::builder::{
        MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder,
    };
    use crate::router::route::{PathChain, PathRouter, RouterConfig};
    use crate::status::{ExchangeState, HandlerStatus};
    use async_trait::async_trait;
    use std::collections::{HashMap, HashSet};
    use std::convert::Infallible;

    #[derive(Debug)]
    struct DummyHandler;

    #[async_trait]
    impl Handler<(), (), ()> for DummyHandler {
        async fn exec<'a>(
            &self,
            _exchange: &mut Exchange<'a, (), (), ()>,
        ) -> Result<HandlerStatus, Infallible> {
            Ok(HandlerStatus::new(ExchangeState::OK))
        }

        fn name(&self) -> &str {
            "DummyHandler"
        }
    }

    #[test]
    #[rustfmt::skip]
    fn router_v2_test() {
        let mut registry = HandlerRegistry::<(), (), ()>::new();
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

        let config = SingleServiceConfigBuilder::new()
            .with_chain("test_chain", &["test1", "test2", "test6", "test7", "test8", "test9"])
            .start_route("/api/v1/*")
                .post()
                    .with_request_chain("test_chain")
                    .with_termination_handler("test3")
                    .with_response_handler("test4")
                .end_method()
                .get()
                    .with_request_chain("test_chain")
                    .with_termination_handler("test3")
                    .with_response_handler("test4")
                .end_method()
            .end_route()
            .build();

        let table = PathRouter::new(&config, &registry).unwrap();

        let result = table.lookup("/api/v1/users", "GET");
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6);

        let result = table.lookup("/api/v1/someOtherEndpoint", "GET");
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6);

        let result = table.lookup("/invalid", "GET");
        assert!(result.is_none());

        let result = table.lookup("/api/v1/users/somethingElse", "GET");
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6);
    }
}
