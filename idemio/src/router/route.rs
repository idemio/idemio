use crate::handler::SharedHandler;
use crate::handler::registry::HandlerRegistry;
use fnv::{FnvBuildHasher, FnvHasher};
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::iter::Filter;
use std::str::{FromStr, Split};
use std::sync::Arc;

#[derive(Debug)]
pub enum PathRouterError {
    InvalidPath(String),
    InvalidMethod(String),
    UnknownHandler(String),
}

#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub path: String,
    pub method: String,
}

impl RouteInfo {
    pub fn new(path: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            method: method.into(),
        }
    }
}

impl PathRouterError {
    pub fn invalid_path(path: impl Into<String>) -> Self {
        Self::InvalidPath(path.into())
    }
    pub fn invalid_method(method: impl Into<String>) -> Self {
        Self::InvalidMethod(method.into())
    }
    pub fn unknown_handler(handler: impl Into<String>) -> Self {
        Self::UnknownHandler(handler.into())
    }
}

impl Display for PathRouterError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PathRouterError::InvalidPath(path) => {
                write!(f, "Invalid path: {}", path)
            },
            PathRouterError::InvalidMethod(method) => {
                write!(f, "Invalid method: {}", method)
            },
            PathRouterError::UnknownHandler(handler) => {
                write!(f, "Handler '{}' could not be found in the handler registry.", handler)
            }
        }
    }
}

impl std::error::Error for PathRouterError {}

pub struct PathConfig {
    pub chains: HashMap<String, Vec<String>>,
    pub paths: HashMap<String, HashMap<String, Vec<String>>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PathKey {
    method_hash: u64,
    path_hash: u64,
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PathSegment {
    Static(String),
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

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "*" {
            Ok(PathSegment::Any)
        } else {
            Ok(PathSegment::Static(s.to_string()))
        }
    }
}

impl PathKey {
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

struct PathNode<I, O, M> {
    children: HashMap<PathSegment, PathNode<I, O, M>, FnvBuildHasher>,
    segment_depth: u64,
    methods: HashMap<String, Arc<Vec<SharedHandler<I, O, M>>>, FnvBuildHasher>,
}

impl<I, O, M> Default for PathNode<I, O, M> {
    fn default() -> Self {
        Self {
            children: HashMap::with_hasher(FnvBuildHasher::default()),
            methods: HashMap::with_hasher(FnvBuildHasher::default()),
            segment_depth: 0,
        }
    }
}

pub struct PathRouter<I, O, M> {
    static_paths: HashMap<PathKey, Arc<Vec<SharedHandler<I, O, M>>>, FnvBuildHasher>,
    nodes: PathNode<I, O, M>,
}

impl<I, O, M> PathRouter<I, O, M> {
    pub fn new(route_config: &PathConfig, registry: &HandlerRegistry<I, O, M>) -> Result<Self, PathRouterError> {
        let mut table = Self {
            static_paths: HashMap::with_hasher(FnvBuildHasher::default()),
            nodes: PathNode::default(),
        };
        if let Err(e) = table.parse_config(route_config, registry) {
            return Err(e);       
        }
        Ok(table)
    }

    fn parse_config(
        &mut self,
        route_config: &PathConfig,
        handler_registry: &HandlerRegistry<I, O, M>,
    ) -> Result<(), PathRouterError> {
        for (path, methods) in route_config.paths.iter() {
            
            // static paths (ones that do not contain '*') should be added to the fast path.
            if !path.contains('*') {
                for (method, handlers) in methods {
                    let key = PathKey::new(method, path);
                    let mut registered_handlers: Vec<SharedHandler<I, O, M>> = vec![];
                    for handler in handlers {
                        let registered_handler = match handler_registry.find(handler) {
                            Ok(handler) => handler,
                            Err(_) => {
                                return Err(PathRouterError::unknown_handler(handler))
                            }
                        };
                        registered_handlers.push(registered_handler.clone());
                    }
                    self.static_paths.insert(key, Arc::new(registered_handlers));
                }
            }
            
            let path_segments = Self::split_path(path);
            let mut current_node = &mut self.nodes;
            let mut depth = 0;
            
            for segment in path_segments {
                
                // segment has an infallible return value, so unwrap is safe.
                let path_segment = PathSegment::from_str(segment).unwrap();
                let is_wild_card = path_segment == PathSegment::Any;
                
                current_node = current_node
                    .children
                    .entry(path_segment)
                    .or_insert_with(PathNode::default);
                current_node.segment_depth = depth;
                depth += 1;

                // wildcards should be the last segment in the path.
                if is_wild_card {
                    break;
                }
            }

            for (method, handlers) in methods {
                let mut registered_handlers = vec![];
                for handler in handlers {
                    let registered_handler = match handler_registry.find(handler) {
                        Ok(handler) => handler,
                        Err(_) => {
                            return Err(PathRouterError::unknown_handler(handler))
                        }
                    };
                    registered_handlers.push(registered_handler);
                }
                current_node
                    .methods
                    .insert(method.to_string(), Arc::new(registered_handlers));
            }
        }
        Ok(())
    }
    fn split_path(path: &str) -> Filter<Split<char>, fn(&&str) -> bool> {
        path.split('/').filter(|s| !s.is_empty())
    }

    pub fn lookup(&self, route_info: RouteInfo) -> Option<Arc<Vec<SharedHandler<I, O, M>>>> {
        log::trace!("Looking up route for path: {}", route_info.path);
        
        let req_method = route_info.method;
        let req_path = route_info.path;

        if let Some(handlers) = self.static_paths.get(&PathKey::new(&req_method, &req_path)) {
            return Some(handlers.clone());
        }
        
        let req_path_segments = Self::split_path(&req_path);
        let mut matching_paths: Vec<&PathNode<I, O, M>> = vec![];
        let mut current_node = &self.nodes;
        let mut full_match = false;
        for req_path_segment in req_path_segments {
            let path_segment = PathSegment::from_str(req_path_segment).unwrap();

            // Incoming requests cannot contain wildcards.
            if path_segment == PathSegment::Any {
                return None;
            }

            if let Some(wild_card_path) = current_node.children.get(&PathSegment::Any) {
                if wild_card_path.methods.contains_key(&req_method) {
                    matching_paths.push(wild_card_path);    
                }
            }

            // As soon as the path no longer matches, we can stop.
            current_node = match current_node.children.get(&path_segment) {
                None => {
                    full_match = false;
                    break;
                },
                Some(child) => {
                    full_match = true;
                    child
                },
            };
        }

        if full_match && current_node.methods.contains_key(&req_method) {
            matching_paths.push(current_node);
        }

        if matching_paths.is_empty() {
            return None;
        }

        let mut max_depth = 0;
        let mut best_match: Option<&PathNode<I, O, M>> = None;
        
        for path in matching_paths {
            if path.segment_depth > max_depth {
                max_depth = path.segment_depth;
                best_match = Some(path);
            }
        }
        
        best_match.and_then(|path_node| path_node.methods.get(&req_method).cloned())
    }
}

#[cfg(test)]
mod test {
    use crate::handler::Handler;
    use crate::handler::registry::HandlerRegistry;
    use crate::router::exchange::Exchange;
    use crate::router::route::{PathConfig, PathRouter, RouteInfo};
    use crate::status::{Code, HandlerExecutionError, HandlerStatus};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Debug)]
    struct DummyHandler;
    #[async_trait]
    impl Handler<(), (), ()> for DummyHandler {
        async fn exec(
            &self,
            _exchange: &mut Exchange<(), (), ()>,
        ) -> Result<HandlerStatus, HandlerExecutionError> {
            Ok(HandlerStatus::new(Code::OK))
        }
    }

    #[test]
    fn router_v2_test() {
        let mut registry = HandlerRegistry::<(), (), ()>::new();
        registry
            .register_handler("test1", Arc::new(DummyHandler))
            .unwrap();
        registry
            .register_handler("test2", Arc::new(DummyHandler))
            .unwrap();
        registry
            .register_handler("test3", Arc::new(DummyHandler))
            .unwrap();
        registry
            .register_handler("test4", Arc::new(DummyHandler))
            .unwrap();

        let mut paths = HashMap::new();
        let mut methods = HashMap::new();
        methods.insert(
            "POST".to_string(),
            vec!["test1".to_string(), "test2".to_string()],
        );
        methods.insert(
            "GET".to_string(),
            vec!["test1".to_string(), "test2".to_string()],
        );
        paths.insert("/api/v1/users".to_string(), methods);
        let mut methods = HashMap::new();
        methods.insert(
            "GET".to_string(),
            vec![
                "test1".to_string(),
                "test2".to_string(),
                "test3".to_string(),
                "test4".to_string(),
            ],
        );
        paths.insert("/api/v1/*".to_string(), methods);

        let config = PathConfig {
            chains: HashMap::new(),
            paths,
        };

        let table = PathRouter::new(&config, &registry).unwrap();

        let route_info = RouteInfo {
            path: "/api/v1/users".to_string(),
            method: "GET".to_string(),
        };
        let result = table.lookup(route_info);
        assert!(result.is_some());

        let handlers = result.unwrap();
        assert_eq!(handlers.len(), 2);

        let route_info = RouteInfo {
            path: "/api/v1/someOtherEndpoint".to_string(),
            method: "GET".to_string(),
        };
        let result = table.lookup(route_info);
        assert!(result.is_some());

        let handlers = result.unwrap();
        assert_eq!(handlers.len(), 4);

        let route_info = RouteInfo {
            path: "/invalid".to_string(),
            method: "GET".to_string(),
        };
        let result = table.lookup(route_info);
        assert!(result.is_none());

        let route_info = RouteInfo {
            path: "/api/v1/users/somethingElse".to_string(),
            method: "GET".to_string(),
        };
        let result = table.lookup(route_info);
        assert!(result.is_some());
        let handlers = result.unwrap();
        assert_eq!(handlers.len(), 4);
    }
}
