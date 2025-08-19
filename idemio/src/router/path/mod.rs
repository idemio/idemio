
pub mod http;

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::iter::Filter;
use std::str::{FromStr, Split};
use std::sync::Arc;
use fnv::{FnvBuildHasher, FnvHasher};
use thiserror::Error;
use crate::handler::{Handler, HandlerId};
use crate::handler::registry::{HandlerRegistry, HandlerRegistryError};
use crate::router::config::{PathChain, RouterConfig};

/// Splits a path string into individual segments, filtering out empty segments.
///
/// # Parameters
/// - `path`: Path string to split (e.g., "/api/v1/users", "/health")
///
/// # Returns
/// An iterator over non-empty path segments. Leading and trailing slashes are ignored,
/// and consecutive slashes are treated as a single separator.
fn split_path(path: &str) -> Filter<Split<char>, fn(&&str) -> bool> {
    path.split('/').filter(|s| !s.is_empty())
}

/// Errors that can occur during PathMatcher construction and operation.
#[derive(Error, Debug)]
pub enum PathMatcherError {
    #[error("Invalid path configuration. {message}")]
    InvalidConfiguration { message: String },

    #[error("Path '{path}' is invalid.")]
    InvalidPath { path: String },

    #[error("Method '{method}' is invalid.")]
    InvalidMethod { method: String },

    #[error("Failed to register handler.")]
    HandlerRegistryError {
        #[source]
        source: HandlerRegistryError,
    },
}

impl PathMatcherError {
    //    #[inline]
    //    pub(crate) fn invalid_path(path: impl Into<String>) -> Self {
    //        Self::InvalidPath { path: path.into() }
    //    }

    #[inline]
    pub(crate) fn invalid_configuration(message: impl Into<String>) -> Self {
        Self::InvalidConfiguration {
            message: message.into(),
        }
    }

    #[inline]
    pub(crate) fn invalid_method(method: impl Into<String>) -> Self {
        Self::InvalidMethod {
            method: method.into(),
        }
    }

    #[inline]
    pub(crate) const fn registry_error(registry_error: HandlerRegistryError) -> Self {
        Self::HandlerRegistryError {
            source: registry_error,
        }
    }
}

/// A collection of handlers that are ready to be executed in a specific order.
///
/// # Behavior
/// `LoadedChain` represents a complete request processing pipeline consisting of:
/// - **Request handlers**: Process incoming data before the main handler (authentication, validation, etc.)
/// - **Termination handler**: The main handler that processes the request and generates output
/// - **Response handlers**: Process outgoing data after the main handler (serialization, logging, etc.)
///
/// All handlers are wrapped in `Arc<dyn Handler<E>>` to ensure they can be safely
/// shared across multiple threads and async tasks.
pub struct LoadedChain<E>
where
    E: Send + Sync,
{
    request_handlers: Vec<Arc<dyn Handler<E>>>,
    termination_handler: Arc<dyn Handler<E>>,
    response_handlers: Vec<Arc<dyn Handler<E>>>,
}

impl<E> LoadedChain<E>
where
    E: Send + Sync
{
    /// Creates a new `LoadedChain` with the specified handlers.
    ///
    /// # Parameters
    /// - `request_handlers`: Vector of handlers to execute before the termination handler.
    ///   These typically handle authentication, authorization, validation, rate limiting, etc.
    /// - `termination_handler`: The main handler that processes the request and generates output.
    ///   This is the core business logic handler that must always be present.
    /// - `response_handlers`: Vector of handlers to execute after the termination handler.
    ///   These typically handle response transformation, logging, metrics collection, etc.
    ///
    /// # Returns
    /// A new `LoadedChain` instance containing the provided handlers in the specified execution order.
    ///
    /// # Behavior
    /// Stores all handlers in their respective categories for ordered execution during request processing.
    pub(crate) fn new(
        request_handlers: Vec<Arc<dyn Handler<E>>>,
        termination_handler: Arc<dyn Handler<E>>,
        response_handlers: Vec<Arc<dyn Handler<E>>>,
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
    /// This method always returns at least 1, as every chain must have a termination handler.
    ///
    /// # Behavior
    /// Calculates the size using the formula: `request_handlers.len() + 1 + response_handlers.len()`.
    pub fn size(&self) -> usize {
        self.request_handlers.len() + 1 + self.response_handlers.len()
    }

    /// Returns a reference to the request handlers vector.
    ///
    /// # Returns
    /// An immutable reference to the vector containing all request handlers.
    /// These handlers are executed in order before the termination handler.
    ///
    /// # Behavior
    /// Provides read-only access to the request handlers for inspection or execution.
    pub fn request_handlers(&self) -> &Vec<Arc<dyn Handler<E>>> {
        &self.request_handlers
    }

    /// Returns a reference to the termination handler.
    ///
    /// # Returns
    /// An immutable reference to the termination handler, which is the main business logic
    /// handler that processes the request and generates the primary output.
    ///
    /// # Behavior
    /// Provides read-only access to the termination handler for inspection or execution.
    pub fn termination_handler(&self) -> &Arc<dyn Handler<E>> {
        &self.termination_handler
    }

    /// Returns a reference to the response handlers vector.
    ///
    /// # Returns
    /// An immutable reference to the vector containing all response handlers.
    /// These handlers are executed in order after the termination handler completes successfully.
    ///
    /// # Behavior
    /// Provides read-only access to the response handlers for inspection or execution.
    pub fn response_handlers(&self) -> &Vec<Arc<dyn Handler<E>>> {
        &self.response_handlers
    }
}

/// A key used for fast lookup of static paths (paths without wildcards) in the router.
///
/// # Behavior
/// This is an internal optimization structure that uses FNV hashing for efficient
/// method and path combination lookups. Static paths can be resolved in O(1) time
/// using hash table lookups rather than tree traversal.
///
/// FNV hashing is chosen for its speed and good distribution characteristics for
/// short strings like HTTP methods and URL paths.
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
    /// - `method`: HTTP method string (e.g., "GET", "POST", "PUT", "DELETE")
    /// - `path`: URL path string (e.g., "/api/users", "/health", "/api/v1/orders")
    ///
    /// # Returns
    /// A new StaticPathKey with hashed method and path values for fast comparison.
    ///
    /// # Behavior
    /// Performs two FNV hash calculations, which are very fast operations.
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
/// # Behavior
/// Path segments are the building blocks of the routing tree structure used for
/// dynamic path matching. Each segment in a URL path (separated by '/') becomes
/// a PathSegment in the routing tree.
///
/// Wildcard behavior:
/// - `Static` segments must match exactly
/// - `Any` segments (wildcards) match any single path segment
/// - Wildcards use longest-prefix matching when multiple patterns could apply
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PathSegment {
    /// A static path segment containing literal text that must match exactly.
    ///
    /// # Behavior
    /// Static segments require exact string matching during route resolution.
    Static(String),

    /// A wildcard segment that matches any single path segment value.
    ///
    /// # Behavior
    /// Represented by "*" in path patterns. Matches exactly one path segment,
    /// not multiple segments or empty segments.
    Any,
}

impl Display for PathSegment {
    /// Formats the path segment for display purposes.
    ///
    /// # Returns
    /// - Static segments return their contained string
    /// - Any segments return "*"
    ///
    /// # Behavior
    /// Provides a human-readable representation of the path segment.
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
    /// - `s`: String slice to parse. "*" creates a wildcard, anything else creates a static segment.
    ///
    /// # Returns
    /// Always succeeds with either `PathSegment::Any` for "*" or `PathSegment::Static` for other values.
    /// This operation is infallible as any string can be converted to a path segment.
    ///
    /// # Behavior
    /// Converts string representations into strongly-typed path segments for routing tree construction.
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
/// # Behavior
/// Each node represents a path segment and can contain:
/// - Child nodes for deeper path segments
/// - Method handlers for HTTP methods at this path depth
/// - Depth information for longest-prefix matching
///
/// The tree structure allows efficient matching of both static and dynamic paths.
/// Uses FNV hashing for fast child node and method lookups.
struct PathNode<E>
where
    E: Send + Sync
{
    /// Child nodes indexed by path segment (static text or wildcard)
    children: HashMap<PathSegment, PathNode<E>, FnvBuildHasher>,
    /// Depth of this node in the routing tree (0 = root)
    segment_depth: u64,
    /// HTTP method handlers available at this path depth
    methods: HashMap<String, Arc<LoadedChain<E>>, FnvBuildHasher>,
}

impl<E> Default for PathNode<E>
where
    E: Send + Sync
{
    /// Creates a new empty PathNode with default FNV-hashed collections.
    ///
    /// # Returns
    /// A new `PathNode` with empty children and methods collections, and depth set to 0.
    ///
    /// # Behavior
    /// Initializes the node with FNV hashers for optimal performance with string keys.
    fn default() -> Self {
        Self {
            children: HashMap::with_hasher(FnvBuildHasher::default()),
            methods: HashMap::with_hasher(FnvBuildHasher::default()),
            segment_depth: 0,
        }
    }
}

/// A trait for matching URL paths to handler chains in the routing system.
///
/// # Type Parameters
/// - `E`: The exchange type that will be processed by the matched handlers
///
/// # Behavior
/// Implementations provide path-to-handler mapping functionality with support for:
/// - Static path matching for exact routes
/// - Dynamic path matching with wildcards
/// - HTTP method-based routing
/// - Handler chain resolution and loading
/// - Configuration parsing from routing definitions
pub trait PathMatcher<E>
where
    E: Send + Sync
{
    /// Parses router configuration and populates the matcher with routes.
    ///
    /// # Parameters
    /// - `route_config`: The router configuration containing path definitions
    /// - `handler_registry`: Registry containing available handlers
    ///
    /// # Returns
    /// `Result<(), PathMatcherError>` where:
    /// - `Ok(())` indicates successful configuration parsing
    /// - `Err(PathMatcherError)` if configuration is invalid or handlers are missing
    ///
    /// # Errors
    /// `PathMatcherError` variants for:
    /// - `InvalidConfiguration` if the configuration format is malformed
    /// - `HandlerRegistryError` if referenced handlers are not found in the registry
    ///
    /// # Behavior
    /// Processes the configuration to build internal routing structures,
    /// validates all referenced handlers exist, and prepares the matcher for lookups.
    fn parse_config(
        &mut self,
        route_config: &RouterConfig,
        handler_registry: &HandlerRegistry<E>,
    ) -> Result<(), PathMatcherError>;

    /// Looks up a handler chain for the given path and method combination.
    ///
    /// # Parameters
    /// - `key`: A tuple containing `(path, method)` where path is the URL path
    ///   and method is the HTTP method string
    ///
    /// # Returns
    /// `Option<Arc<LoadedChain<E>>>` where:
    /// - `Some(Arc<LoadedChain<E>>)` contains the matched handler chain
    /// - `None` if no matching route is found
    ///
    /// # Behavior
    /// Searches the internal routing structures to find the best matching handler chain.
    /// Supports both exact static matches and wildcard pattern matching with
    /// longest-prefix matching for ambiguous cases.
    fn lookup(&self, key: (&str, &str)) -> Option<Arc<LoadedChain<E>>>;

    /// Creates a new PathMatcher instance from configuration and handler registry.
    ///
    /// # Parameters
    /// - `config`: The router configuration to parse
    /// - `handler_registry`: Registry containing available handlers
    ///
    /// # Returns
    /// `Result<Self, PathMatcherError>` where:
    /// - `Ok(Self)` contains the configured PathMatcher instance
    /// - `Err(PathMatcherError)` if creation fails
    ///
    /// # Errors
    /// `PathMatcherError` if configuration parsing or handler resolution fails.
    ///
    /// # Behavior
    /// Constructs a new matcher instance and immediately parses the provided configuration.
    /// This is a convenience method that combines instantiation and configuration.
    fn new(
        config: &RouterConfig,
        handler_registry: &HandlerRegistry<E>,
    ) -> Result<Self, PathMatcherError>
    where
        Self: Sized;

    /// Finds a single handler in the registry by name.
    ///
    /// # Parameters
    /// - `handler`: String name of the handler to find
    /// - `handler_registry`: Registry to search in
    ///
    /// # Returns
    /// `Result<Arc<dyn Handler<E>>, PathMatcherError>` where:
    /// - `Ok(Arc<dyn Handler<E>>)` contains the found handler
    /// - `Err(PathMatcherError)` if the handler is not found
    ///
    /// # Errors
    /// Returns `PathMatcherError::HandlerRegistryError` if the handler is not registered.
    ///
    /// # Behavior
    /// Creates a HandlerId from the handler name and performs registry lookup.
    /// This is a utility method for handler resolution during configuration parsing.
    fn find_in_registry(
        handler: &str,
        handler_registry: &HandlerRegistry<E>,
    ) -> Result<Arc<dyn Handler<E>>, PathMatcherError> {
        let handler_id = HandlerId::new(handler);
        match handler_registry.find_with_id(&handler_id) {
            Ok(handler) => Ok(handler),
            Err(e) => Err(PathMatcherError::registry_error(e)),
        }
    }

    /// Finds multiple handlers in the registry by their names.
    ///
    /// # Parameters
    /// - `handlers`: Slice of handler name strings to find
    /// - `handler_registry`: Registry to search in
    ///
    /// # Returns
    /// `Result<Vec<Arc<dyn Handler<E>>>, PathMatcherError>` where:
    /// - `Ok(Vec<Arc<dyn Handler<E>>>)` contains all found handlers in order
    /// - `Err(PathMatcherError)` if any handler is not found
    ///
    /// # Errors
    /// Returns `PathMatcherError::HandlerRegistryError` if any handler is not registered.
    /// The operation fails fast on the first missing handler.
    ///
    /// # Behavior
    /// Sequentially looks up each handler name and collects them into a vector.
    /// Used for loading handler chains that contain multiple handlers.
    fn find_all_in_registry(
        handlers: &[String],
        handler_registry: &HandlerRegistry<E>,
    ) -> Result<Vec<Arc<dyn Handler<E>>>, PathMatcherError> {
        let mut registered_handlers = vec![];
        for handler in handlers {
            let registered_handler = Self::find_in_registry(handler, handler_registry)?;
            registered_handlers.push(registered_handler);
        }
        Ok(registered_handlers)
    }

    /// Loads handlers from the registry and creates a complete handler chain.
    ///
    /// # Parameters
    /// - `handler_registry`: Registry containing available handlers
    /// - `path_chain`: Configuration defining the handler chain structure
    ///
    /// # Returns
    /// `Result<LoadedChain<E>, PathMatcherError>` where:
    /// - `Ok(LoadedChain<E>)` contains the complete loaded handler chain
    /// - `Err(PathMatcherError)` if handler loading fails
    ///
    /// # Errors
    /// Returns `PathMatcherError::InvalidMethod` if no termination handler is specified.
    /// Returns `PathMatcherError::HandlerRegistryError` if any referenced handler is missing.
    ///
    /// # Behavior
    /// Processes a PathChain configuration to:
    /// 1. Load optional request handlers from registry
    /// 2. Load the required termination handler
    /// 3. Load optional response handlers from registry
    /// 4. Construct and return a complete LoadedChain
    fn load_handlers(
        handler_registry: &HandlerRegistry<E>,
        path_chain: &PathChain,
    ) -> Result<LoadedChain<E>, PathMatcherError> {
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
}