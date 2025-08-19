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
    #[inline]
    pub(crate) fn invalid_path(path: impl Into<String>) -> Self {
        Self::InvalidPath { path: path.into() }
    }

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

pub trait PathMatcher<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    fn parse_config(
        &mut self,
        route_config: &RouterConfig,
        handler_registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<(), PathMatcherError>;

    fn lookup(&self, key: (&str, &str)) -> Option<Arc<LoadedChain<In, Out, Meta>>>;

    fn new(
        config: &RouterConfig,
        handler_registry: &HandlerRegistry<In, Out, Meta>,
    ) -> Result<Self, PathMatcherError>
    where
        Self: Sized;

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
}

