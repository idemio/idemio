use crate::handler::HandlerRegistry;
use crate::router::config::{RouterConfig};
use crate::router::path::{LoadedChain, PathMatcherError, RouteKeyMatcher};
use crate::router::route::RouteKey;
use fnv::FnvBuildHasher;
use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use std::iter::Filter;
use std::str::{FromStr, Split};
use std::sync::Arc;

/// Splits a path string into individual segments, filtering out empty segments.
fn split_path(path: &'_ str) -> Filter<Split<'_, char>, fn(&&str) -> bool> {
    path.split('/').filter(|s| !s.is_empty())
}

/// Represents a segment in a URL path, which can be either a static text or a wildcard.
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum HttpPathSegment<'a> {
    /// A static path segment containing literal text that must match exactly.
    Static(Cow<'a, str>),

    /// A wildcard segment that matches any single path segment value.
    Any,
}

impl Display for HttpPathSegment<'_> {
    /// Formats the path segment for display purposes.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpPathSegment::Static(s) => write!(f, "{}", s),
            HttpPathSegment::Any => write!(f, "*"),
        }
    }
}

impl<'a> From<&'a str> for HttpPathSegment<'a> {
    fn from(s: &'a str) -> Self {
        if s == "*" {
            HttpPathSegment::Any
        } else {
            HttpPathSegment::Static(Cow::Borrowed(s))
        }
    }
}

impl FromStr for HttpPathSegment<'_> {
    type Err = Infallible;

    /// Parses a string into a PathSegment.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "*" {
            Ok(HttpPathSegment::Any)
        } else {
            Ok(HttpPathSegment::Static(Cow::Owned(s.to_string())))
        }
    }
}

/// A node in the path routing tree structure.
struct HttpPathMethodNode<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{
    /// Child nodes indexed by path segment (static text or wildcard)
    children: HashMap<HttpPathSegment<'static>, HttpPathMethodNode<I, O>, FnvBuildHasher>,
    /// HTTP method handlers available at this path depth
    methods: HashMap<String, Arc<LoadedChain<I, O>>, FnvBuildHasher>,
}

impl<I, O> Default for HttpPathMethodNode<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{
    fn default() -> Self {
        Self {
            children: HashMap::with_hasher(FnvBuildHasher::default()),
            methods: HashMap::with_hasher(FnvBuildHasher::default()),
        }
    }
}

#[derive(Default, Hash)]
pub struct HttpPathMethodKey<'a> {
    pub path: &'a str,
    pub method: &'a str,
}

impl<'a> HttpPathMethodKey<'a> {
    pub fn new(method: &'a str, path: &'a str) -> Self {
        Self { method, path }
    }

    #[inline]
    pub fn with_method(mut self, method: &'a str) -> Self {
        self.method = method;
        self
    }

    #[inline]
    pub fn with_path(mut self, path: &'a str) -> Self {
        self.path = path;
        self
    }
}

impl<'a> From<(&'a str, &'a str)> for HttpPathMethodKey<'a> {
    fn from(value: (&'a str, &'a str)) -> Self {
        HttpPathMethodKey {
            path: value.0,
            method: value.1,
        }
    }
}

pub struct HttpPathMethodMatcher<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{
    /// Tree structure for dynamic-path matching with wildcards
    nodes: HttpPathMethodNode<I, O>,
}

impl<I, O> RouteKeyMatcher<I, O> for HttpPathMethodMatcher<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{
    fn parse_config(
        &mut self,
        route_config: &RouterConfig,
        handler_registry: &HandlerRegistry<I, O>,
    ) -> Result<(), PathMatcherError> {
        log::info!(
            "Starting router configuration parsing with '{}' paths",
            route_config.paths.len()
        );
        for (index, (path, methods)) in route_config.paths.iter().enumerate() {
            log::debug!("Path {index}: '{path}'");
            let path_segments = split_path(path);
            let mut current_node = &mut self.nodes;
            for segment in path_segments {
                let path_segment = HttpPathSegment::from_str(segment).unwrap();
                let is_wild_card = path_segment == HttpPathSegment::Any;
                current_node = current_node
                    .children
                    .entry(path_segment)
                    .or_insert_with(HttpPathMethodNode::default);

                if is_wild_card {
                    break;
                }
            }
            for (method, handlers) in methods {
                let chain = Self::load_handlers(&handler_registry, handlers)?;
                let count = chain.size();
                current_node
                    .methods
                    .insert(method.to_string(), Arc::new(chain));

                log::debug!("Added {count} handlers for method {method} to path '{path}'");
            }
        }
        Ok(())
    }

    fn lookup(&self, key: RouteKey<'_>) -> Option<Arc<LoadedChain<I, O>>> {
        let (path, method) = match (key.path, key.method) {
            (Some(path), Some(method)) => (path, method),
            _ => return None,
        };

        // Dynamic path matching with wildcards
        let segments = split_path(&path);
        let mut best: Option<&HttpPathMethodNode<I, O>> = None;
        let mut current = &self.nodes;
        for segment_str in segments {
            current
                .children
                .get(&HttpPathSegment::Any)
                .and_then(|node| match node.methods.contains_key(method) {
                    true => Some(node),
                    false => None,
                })
                .and_then(|node| best.replace(node));

            let static_segment = HttpPathSegment::Static(Cow::Borrowed(segment_str));
            current = match current.children.get(&static_segment) {
                Some(child) => child,
                None => break,
            };
        }
        current
            .methods
            .contains_key(method)
            .then(|| best.replace(current));
        best.and_then(|node| node.methods.get(method).cloned())
    }

    fn new(
        config: &RouterConfig,
        handler_registry: &HandlerRegistry<I, O>,
    ) -> Result<Self, PathMatcherError> {
        let mut matcher = Self {
            nodes: HttpPathMethodNode::default(),
        };
        if let Err(e) = matcher.parse_config(config, handler_registry) {
            return Err(e);
        }
        Ok(matcher)
    }
}

#[cfg(test)]
mod test {
    use crate::exchange::InnerData;
    use crate::handler::HandlerId;
    use crate::handler::{HandlerError, LabeledHandler, TerminationHandler};
    use crate::router::path::{http::HttpPathMethodMatcher, RouteKeyMatcher};
    use crate::router::route::RouteKey;
    use async_trait::async_trait;
    use idemio_macro::Handler;
    use crate::router::builder::{MethodBuilder, RouteBuilder, ServiceBuilder, SingleServiceConfigBuilder};

    /// A simple test handler that does nothing but return an OK status.
    #[derive(Handler)]
    struct DummyHandler;
    #[async_trait]
    impl TerminationHandler<(), ()> for DummyHandler {
        async fn exec(&self, _data: InnerData<()>) -> Result<InnerData<()>, HandlerError> {
            Ok(InnerData::new(()))
        }
    }

    #[test]
    fn test_lookup_static_path() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/users")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key = ("/api/users", "GET").into();
        let result = matcher.lookup(key);
        assert!(result.is_some());
    }

    #[test]
    fn test_lookup_wildcard_path() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/v1/*")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key = ("/api/v1/users", "GET").into();
        let result = matcher.lookup(key);
        assert!(result.is_some());
    }

    #[test]
    fn test_lookup_wildcard_with_multiple_segments() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/v1/*")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key = ("/api/v1/users/123/profile", "GET").into();
        let result = matcher.lookup(key);
        assert!(result.is_some());
    }

    #[test]
    fn test_lookup_no_matching_path() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/users")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key = RouteKey::new("/api/invalid", "GET");
        let result = matcher.lookup(key);
        assert!(result.is_none());
    }

    #[test]
    fn test_lookup_no_matching_method() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/users")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key = RouteKey::new("/api/users", "POST");
        let result = matcher.lookup(key);
        assert!(result.is_none());
    }

    #[test]
    fn test_lookup_missing_path() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/users")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key = RouteKey::default().with_method("GET");
        let result = matcher.lookup(key);
        assert!(result.is_none());
    }

    #[test]
    fn test_lookup_missing_method() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/users")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key = RouteKey::default().with_path("/api/users");
        let result = matcher.lookup(key);
        assert!(result.is_none());
    }

    #[test]
    fn test_lookup_multiple_methods_same_path() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();
        registry
            .register_termination_handler("handler2".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/users")
            .get()
            .termination_handler("handler1")
            .end_method()
            .post()
            .termination_handler("handler2")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let get_key = RouteKey::default()
            .with_path("/api/users")
            .with_method("GET");
        let get_result = matcher.lookup(get_key);
        assert!(get_result.is_some());

        let post_key = RouteKey::default()
            .with_path("/api/users")
            .with_method("POST");
        let post_result = matcher.lookup(post_key);
        assert!(post_result.is_some());

        let put_key = RouteKey::default()
            .with_path("/api/users")
            .with_method("PUT");
        let put_result = matcher.lookup(put_key);
        assert!(put_result.is_none());
    }

    #[test]
    fn test_lookup_nested_static_paths() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();
        registry
            .register_termination_handler("handler2".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/users")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .route("/api/users/profile")
            .get()
            .termination_handler("handler2")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key1 = RouteKey::default()
            .with_path("/api/users")
            .with_method("GET");
        let result1 = matcher.lookup(key1);
        assert!(result1.is_some());

        let key2 = RouteKey::default()
            .with_path("/api/users/profile")
            .with_method("GET");
        let result2 = matcher.lookup(key2);
        assert!(result2.is_some());
    }

    #[test]
    fn test_lookup_wildcard_precedence() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();
        registry
            .register_termination_handler("handler2".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/*")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .route("/api/users/profile")
            .get()
            .termination_handler("handler2")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        // More specific path should match
        let key = RouteKey::default()
            .with_path("/api/users/profile")
            .with_method("GET");
        let result = matcher.lookup(key);
        assert!(result.is_some());

        // Wildcard should still match partial paths
        let key2 = RouteKey::default()
            .with_path("/api/other")
            .with_method("GET");
        let result2 = matcher.lookup(key2);
        assert!(result2.is_some());
    }

    #[test]
    fn test_lookup_root_path() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        let key = RouteKey::default().with_path("/").with_method("GET");
        let result = matcher.lookup(key);
        assert!(result.is_some());
    }

    #[test]
    fn test_lookup_trailing_slash() {
        let mut registry = crate::handler::HandlerRegistry::<(), ()>::new();
        registry
            .register_termination_handler("handler1".into(), DummyHandler)
            .unwrap();

        let config = SingleServiceConfigBuilder::new()
            .route("/api/users")
            .get()
            .termination_handler("handler1")
            .end_method()
            .end_route()
            .build();

        let matcher = HttpPathMethodMatcher::new(&config, &registry).unwrap();

        // Path with trailing slash should match (due to split_path filtering empty segments)
        let key = RouteKey::default()
            .with_path("/api/users/")
            .with_method("GET");
        let result = matcher.lookup(key);
        assert!(result.is_some());
    }
}
