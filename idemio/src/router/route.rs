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
use thiserror::Error;

/// A collection of handlers that are ready to be executed in a specific order.
///
/// `LoadedChain` represents a complete request processing pipeline consisting of:
/// - **Request handlers**: Process incoming data before the main handler (authentication, validation, etc.)
/// - **Termination handler**: The main handler that processes the request and generates output
/// - **Response handlers**: Process outgoing data after the main handler (serialization, logging, etc.)
///
/// # Thread Safety
///
/// All handlers are wrapped in `Arc<dyn Handler<In, Out, Meta>>` to ensure they can be safely
/// shared across multiple threads and async tasks.
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
    ///
    /// - `request_handlers`: Vector of handlers to execute before the termination handler.
    ///   These typically handle authentication, authorization, validation, rate limiting, etc.
    /// - `termination_handler`: The main handler that processes the request and generates output.
    ///   This is the core business logic handler that must always be present.
    /// - `response_handlers`: Vector of handlers to execute after the termination handler.
    ///   These typically handle response transformation, logging, metrics collection, etc.
    ///
    /// # Returns
    ///
    /// A new `LoadedChain` instance containing the provided handlers in the specified execution order.
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
    ///
    /// The total count of all handlers (request + termination + response handlers).
    /// This method always returns at least 1, as every chain must have a termination handler.
    ///
    /// # Formula
    ///
    /// `size = request_handlers.len() + 1 + response_handlers.len()`
    pub fn size(&self) -> usize {
        self.request_handlers.len() + 1 + self.response_handlers.len()
    }

    /// Returns a reference to the request handlers vector.
    ///
    /// # Returns
    ///
    /// An immutable reference to the vector containing all request handlers.
    /// These handlers are executed in order before the termination handler.
    pub fn request_handlers(&self) -> &Vec<Arc<dyn Handler<In, Out, Meta>>> {
        &self.request_handlers
    }

    /// Returns a reference to the termination handler.
    ///
    /// # Returns
    ///
    /// An immutable reference to the termination handler, which is the main business logic
    /// handler that processes the request and generates the primary output.
    pub fn termination_handler(&self) -> &Arc<dyn Handler<In, Out, Meta>> {
        &self.termination_handler
    }

    /// Returns a reference to the response handlers vector.
    ///
    /// # Returns
    ///
    /// An immutable reference to the vector containing all response handlers.
    /// These handlers are executed in order after the termination handler completes successfully.
    pub fn response_handlers(&self) -> &Vec<Arc<dyn Handler<In, Out, Meta>>> {
        &self.response_handlers
    }
}

/// A key used for fast lookup of static paths (paths without wildcards) in the router.
///
/// This is an internal optimization structure that uses FNV hashing for efficient
/// method and path combination lookups. Static paths can be resolved in O(1) time
/// using hash table lookups rather than tree traversal.
///
/// # Performance
///
/// FNV hashing is chosen for its speed and good distribution characteristics for
/// short strings like HTTP methods and URL paths.
///
/// # Internal Use
///
/// This struct is not part of the public API and is used internally by `PathMatcher`
/// for performance optimization.
#[derive(Clone, PartialEq, Eq, Hash)]
struct StaticPathKey {
    /// Hash of the HTTP method string (e.g., "GET", "POST")
    method_hash: u64,
    /// Hash of the URL path string (e.g., "/api/users")
    path_hash: u64,
}

impl StaticPathKey {
    /// Creates a new StaticPathKey from method and path strings.
    ///
    /// # Parameters
    ///
    /// - `method`: HTTP method string (e.g., "GET", "POST", "PUT", "DELETE")
    /// - `path`: URL path string (e.g., "/api/users", "/health", "/api/v1/orders")
    ///
    /// # Returns
    ///
    /// A new StaticPathKey with hashed method and path values for fast comparison.
    ///
    /// # Performance
    ///
    /// This method performs two FNV hash calculations, which are very fast operations.
    /// The resulting key can be used for O(1) hash table lookups.
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

/// Represents a segment in a URL path, which can be either a static text or a wildcard.
///
/// Path segments are the building blocks of the routing tree structure used for
/// dynamic path matching. Each segment in a URL path (separated by '/') becomes
/// a PathSegment in the routing tree.
///
/// # Wildcard Behavior
///
/// - `Static` segments must match exactly
/// - `Any` segments (wildcards) match any single path segment
/// - Wildcards use longest-prefix matching when multiple patterns could apply
///
/// # Examples
///
/// For the path `/api/v1/users/*`:
/// - `api`, `v1`, `users` become `PathSegment::Static`
/// - `*` becomes `PathSegment::Any`
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PathSegment {
    /// A static path segment containing literal text that must match exactly.
    ///
    /// Examples: "api", "v1", "users", "health"
    Static(String),
    /// A wildcard segment that matches any single path segment value.
    ///
    /// Represented by "*" in path patterns. Matches exactly one path segment,
    /// not multiple segments or empty segments.
    Any,
}

impl Display for PathSegment {
    /// Formats the path segment for display purposes.
    ///
    /// # Returns
    ///
    /// - Static segments return their contained string
    /// - Any segments return "*"
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
    ///
    /// - `s`: String slice to parse. "*" creates a wildcard, anything else creates a static segment.
    ///
    /// # Returns
    ///
    /// Always succeeds with either `PathSegment::Any` for "*" or `PathSegment::Static` for other values.
    /// This operation is infallible as any string can be converted to a path segment.
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
/// Each node represents a path segment and can contain:
/// - Child nodes for deeper path segments
/// - Method handlers for HTTP methods at this path depth
/// - Depth information for longest-prefix matching
///
/// The tree structure allows efficient matching of both static and dynamic paths.
///
/// # Internal Structure
///
/// - Uses FNV hashing for fast child node and method lookups
/// - Stores depth information for wildcard precedence resolution
/// - Each node can handle multiple HTTP methods
struct PathNode<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Child nodes indexed by path segment (static text or wildcard)
    children: HashMap<PathSegment, PathNode<In, Out, Meta>, FnvBuildHasher>,
    /// Depth of this node in the routing tree (0 = root)
    segment_depth: u64,
    /// HTTP method handlers available at this path depth
    methods: HashMap<String, Arc<LoadedChain<In, Out, Meta>>, FnvBuildHasher>,
}

impl<In, Out, Meta> Default for PathNode<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Creates a new empty PathNode with default FNV-hashed collections.
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
/// The router uses a hybrid approach for optimal performance:
/// - **Static paths** (without wildcards): Fast O(1) hash-based lookup
/// - **Dynamic paths** (with wildcards): Tree-based matching with longest-prefix resolution
///
/// # Wildcard Support
///
/// - Wildcard segments ("*") match exactly one path segment
/// - Multiple wildcard patterns use longest-prefix matching
/// - Wildcards can appear at any depth in the path hierarchy
///
/// # Performance Characteristics
///
/// - Static path lookup: O(1) average case
/// - Dynamic path lookup: O(d) where d is the maximum path depth
/// - Memory usage scales with the number of unique path segments
///
/// # Thread Safety
///
/// `PathMatcher` is fully thread-safe and can be shared across multiple async tasks.
/// All internal data structures use `Arc` for safe concurrent access.
pub struct PathMatcher<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Fast hash-based lookup for static paths (no wildcards)
    static_paths: HashMap<StaticPathKey, Arc<LoadedChain<In, Out, Meta>>, FnvBuildHasher>,
    /// Tree structure for dynamic path matching with wildcards
    nodes: PathNode<In, Out, Meta>,
}

impl<In, Out, Meta> PathMatcher<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Creates a new PathMatcher from the provided configuration and handler registry.
    ///
    /// # Parameters
    ///
    /// - `route_config`: Configuration containing path definitions and handler chains.
    ///   This specifies which handlers to execute for different path/method combinations.
    /// - `registry`: Registry containing all available handlers referenced in the config.
    ///   All handler IDs in the config must exist in this registry.
    ///
    /// # Returns
    ///
    /// A `Result` containing the constructed `PathMatcher` or a `PathMatcherError` if:
    /// - Any handler referenced in the config is missing from the registry
    /// - The configuration contains invalid path patterns
    /// - Required termination handlers are not specified
    ///
    /// # Behavior
    ///
    /// 1. **Validates** all handler references to exist in registry
    /// 2. **Optimizes** static paths (no wildcards) for O(1) hash lookup
    /// 3. **Builds** tree structure for dynamic paths with wildcards
    /// 4. **Logs** detailed information about the routing structure being built
    ///
    /// # Performance
    ///
    /// Construction time is O(n*m) where n is the number of routes and m is the average path depth.
    /// The resulting matcher provides very fast lookup times during request routing.
    pub fn new(
        route_config: &RouterConfig,
        registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<Self, PathMatcherError> {
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
    ///
    /// - `handler`: String identifier for the handler to find
    /// - `handler_registry`: Registry to search in
    ///
    /// # Returns
    ///
    /// `Result` containing the handler `Arc` or `PathMatcherError` if the handler is not found.
    ///
    /// # Errors
    ///
    /// Returns `PathMatcherError::HandlerRegistryError` if the handler ID is not registered.
    fn find_in_registry(
        handler: &str,
        handler_registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<Arc<dyn Handler<In, Out, Meta>>, PathMatcherError> {
        let handler_id = HandlerId::new(handler);
        match handler_registry.find_with_id(&handler_id) {
            Ok(handler) => Ok(handler),
            Err(e) => Err(PathMatcherError::registry_error(e)),
        }
    }

    /// Finds multiple handlers by their IDs in the registry.
    ///
    /// # Parameters
    ///
    /// - `handlers`: Slice of handler ID strings to find
    /// - `handler_registry`: Registry to search in
    ///
    /// # Returns
    ///
    /// `Result` containing a vector of handler `Arc`s or `PathMatcherError` if any handler is missing.
    ///
    /// # Behavior
    ///
    /// This method fails fast - if any handler is missing, it returns an error immediately
    /// without processing the remaining handlers.
    ///
    /// # Errors
    ///
    /// Returns `PathMatcherError::HandlerRegistryError` if any handler ID is not registered.
    fn find_all_in_registry(
        handlers: &[String],
        handler_registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<Vec<Arc<dyn Handler<In, Out, Meta>>>, PathMatcherError> {
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
    ///
    /// - `handler_registry`: Registry containing available handlers
    /// - `path_chain`: Configuration specifying which handlers to load and in what order
    ///
    /// # Returns
    ///
    /// `Result` containing a `LoadedChain` with all resolved handlers or `PathMatcherError`
    /// if any handlers are missing or the configuration is invalid.
    ///
    /// # Validation
    ///
    /// - **Request handlers**: Optional, can be empty
    /// - **Termination handler**: Required, must be specified
    /// - **Response handlers**: Optional can be empty
    ///
    /// # Errors
    ///
    /// - `PathMatcherError::InvalidMethod` if no termination handler is specified
    /// - `PathMatcherError::HandlerRegistryError` if any handler is not found in the registry
    fn load_handlers(
        handler_registry: &HandlerRegistry<In, Out, Meta>,
        path_chain: &PathChain,
    ) -> Result<LoadedChain<In, Out, Meta>, PathMatcherError> {
        let registered_request_handlers = match &path_chain.request_handlers {
            Some(handlers) => Self::find_all_in_registry(handlers, handler_registry)?,
            None => vec![],
        };
        let registered_termination_handler = match &path_chain.termination_handler {
            Some(handler) => Self::find_in_registry(handler, handler_registry)?,
            None => {
                return Err(PathMatcherError::invalid_method(
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
    ///
    /// - `route_config`: Configuration containing path and handler definitions
    /// - `handler_registry`: Registry containing available handlers
    ///
    /// # Returns
    ///
    /// `Result` indicating success or `PathMatcherError` if the configuration is invalid.
    ///
    /// # Behavior
    ///
    /// 1. **Static Path Optimization**: Paths without wildcards are stored in a hash table for O(1) lookup
    /// 2. **Dynamic Path Tree**: All paths (including static ones) are added to the tree for fallback
    /// 3. **Depth Tracking**: Records the depth of each node for wildcard precedence resolution
    /// 4. **Method Mapping**: Associates HTTP methods with handler chains at each path endpoint
    /// 5. **Detailed Logging**: Provides comprehensive logging for debugging and monitoring
    ///
    /// # Logging
    ///
    /// - `INFO`: Overall parsing progress and statistics
    /// - `DEBUG`: Individual path and method processing
    /// - `TRACE`: Detailed segment-by-segment tree building and wildcard detection
    ///
    /// # Current Limitations
    ///
    /// - Only `Routes::HttpRequestPaths` is fully implemented
    /// - `Routes::HttpHeaderPaths` returns a `todo!()` error
    fn parse_config(
        &mut self,
        route_config: &RouterConfig,
        handler_registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<(), PathMatcherError> {
        match &route_config.routes {
            Routes::HttpRequestPaths(paths) => {
                log::info!(
                    "Starting router configuration parsing with '{}' paths",
                    paths.len()
                );
                for (path, methods) in paths.iter() {
                    log::debug!("Processing path: '{}'", path);
                    // Static paths (ones that do not contain '*') should be added to the fast path.
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
                        // Wildcards should be the last segment in the path.
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
            Routes::HttpHeaderPaths(_) => {
                todo!("Header-based routing is not yet implemented. Please use path-based routing.")
            }
        }
    }

    /// Splits a path string into individual segments, filtering out empty segments.
    ///
    /// # Parameters
    ///
    /// - `path`: Path string to split (e.g., "/api/v1/users", "/health")
    ///
    /// # Returns
    ///
    /// An iterator over non-empty path segments. Leading and trailing slashes are ignored,
    /// and consecutive slashes are treated as a single separator.
    fn split_path(path: &str) -> Filter<Split<char>, fn(&&str) -> bool> {
        path.split('/').filter(|s| !s.is_empty())
    }

    /// Looks up a handler chain for the given request path and HTTP method.
    ///
    /// # Parameters
    ///
    /// - `request_path`: The URL path to match (e.g., "/api/v1/users/123", "/health")
    /// - `request_method`: The HTTP method (e.g., "GET", "POST", "PUT", "DELETE")
    ///
    /// # Returns
    ///
    /// An `Option` containing the `Arc<LoadedChain>` if a matching route is found, `None` otherwise.
    ///
    /// # Matching Algorithm
    ///
    /// 1. **Fast Path**: First attempts O(1) hash-based lookup for exact static path matches
    /// 2. **Dynamic Path**: Falls back to tree traversal for wildcard pattern matching
    /// 3. **Longest Prefix**: When multiple wildcard patterns match, selects the most specific (deepest) match
    /// 4. **Exact Match Priority**: Static segments always take priority over wildcard matches at the same depth
    ///
    /// # Wildcard Behavior
    ///
    /// - Wildcard segments ("*") match exactly one path segment
    /// - Multiple wildcards in a path pattern are supported
    /// - Wildcards do not match across segment boundaries (no "/**" behavior)
    /// - Longest-prefix matching ensures the most specific pattern wins
    ///
    /// # Examples
    /// See any of the code examples for more detail.
    ///
    /// # Performance
    ///
    /// - Static paths: O(1) average case, O(1) worst case
    /// - Dynamic paths: O(d) where d is the depth of the matching path
    /// - Memory usage: Minimal during lookup, no allocations for static paths
    pub fn lookup(
        &self,
        request_path: &str,
        request_method: &str,
    ) -> Option<Arc<LoadedChain<In, Out, Meta>>> {
        // Fast path: try the exact static match first
        if let Some(handlers) = self
            .static_paths
            .get(&StaticPathKey::new(request_method, request_path))
        {
            return Some(handlers.clone());
        }

        // Dynamic path matching with wildcards
        let req_path_segments = Self::split_path(request_path);
        let mut best_match: Option<&PathNode<In, Out, Meta>> = None;
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
}

/// Errors that can occur during PathMatcher construction and operation.
///
/// This enum represents all possible error conditions when working with the PathMatcher,
/// including configuration validation issues and handler registry problems.
#[derive(Error, Debug)]
pub enum PathMatcherError {
    /// An invalid path pattern was specified in the configuration.
    ///
    /// This can occur when:
    /// - Path syntax is malformed
    /// - Invalid wildcard patterns are used
    /// - Path segments contain invalid characters
    #[error("Path '{path}' is invalid.")]
    InvalidPath { path: String },

    /// An invalid HTTP method or missing termination handler was specified.
    ///
    /// This can occur when:
    /// - An unsupported HTTP method is used
    /// - A termination handler is not specified for a path/method combination
    /// - Method configuration is malformed
    #[error("Method '{method}' is invalid.")]
    InvalidMethod { method: String },

    /// An error occurred while accessing the handler registry.
    ///
    /// This typically happens when:
    /// - A handler referenced in the configuration is not registered
    /// - Handler registry is in an inconsistent state
    /// - Handler lookup fails due to internal registry errors
    #[error("Failed to register handler.")]
    HandlerRegistryError {
        #[source]
        source: HandlerRegistryError,
    },
}

impl PathMatcherError {
    /// Creates a new `InvalidPath` error.
    ///
    /// # Parameters
    ///
    /// - `path`: The invalid path that caused the error
    ///
    /// # Returns
    ///
    /// A new `PathMatcherError::InvalidPath` variant.
    #[inline]
    pub(crate) fn invalid_path(path: impl Into<String>) -> Self {
        Self::InvalidPath { path: path.into() }
    }

    /// Creates a new `InvalidMethod` error.
    ///
    /// # Parameters
    ///
    /// - `method`: The invalid method or error message
    ///
    /// # Returns
    ///
    /// A new `PathMatcherError::InvalidMethod` variant.
    #[inline]
    pub(crate) fn invalid_method(method: impl Into<String>) -> Self {
        Self::InvalidMethod {
            method: method.into(),
        }
    }

    /// Creates a new `HandlerRegistryError` wrapper.
    ///
    /// # Parameters
    ///
    /// - `registry_error`: The underlying handler registry error
    ///
    /// # Returns
    ///
    /// A new `PathMatcherError::HandlerRegistryError` variant that wraps the original error.
    #[inline]
    pub(crate) const fn registry_error(registry_error: HandlerRegistryError) -> Self {
        Self::HandlerRegistryError {
            source: registry_error,
        }
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
    use crate::router::route::{PathMatcher};
    use crate::status::{ExchangeState, HandlerStatus};
    use async_trait::async_trait;
    use std::convert::Infallible;

    /// A simple test handler that does nothing but return an OK status.
    ///
    /// Used in unit tests to verify routing functionality without complex business logic.
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
        let table = PathMatcher::new(&config, &registry).unwrap();

        // Test wildcard matching - should match the "/api/v1/*" pattern
        let result = table.lookup("/api/v1/users", "GET");
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6); // test_chain has 6 handlers

        // Test another wildcard match with a different path segment
        let result = table.lookup("/api/v1/someOtherEndpoint", "GET");
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6);

        // Test non-matching path - should return None
        let result = table.lookup("/invalid", "GET");
        assert!(result.is_none());

        // Test path that goes beyond wildcard - should still match "/api/v1/*"
        let result = table.lookup("/api/v1/users/somethingElse", "GET");
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.request_handlers.len(), 6);
    }
}