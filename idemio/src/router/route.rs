use crate::handler::SharedHandler;
use crate::handler::registry::HandlerRegistry;
use crate::router::{PathSegment, RouteInfo};
use dashmap::DashMap;
use dashmap::mapref::one::Ref;
use fnv::FnvHasher;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;

pub struct PathConfig {
    chains: HashMap<String, Vec<String>>,
    paths: HashMap<String, HashMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey {
    method_hash: u64,
    path_hash: u64,
}

impl PathKey {
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        let path = path.into();
        let method = method.into();

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

#[derive(Debug)]
struct PathNode<I, O, M> {
    children: HashMap<PathSegment, PathNode<I, O, M>>,
    depth: u64,
    methods: HashMap<String, Arc<Vec<SharedHandler<I, O, M>>>>,
}

impl<I, O, M> Default for PathNode<I, O, M> {
    fn default() -> Self {
        Self {
            children: HashMap::new(),
            methods: HashMap::new(),
            depth: 0,
        }
    }
}

pub struct PathRouter<I, O, M> {
    static_paths: DashMap<PathKey, Arc<Vec<SharedHandler<I, O, M>>>, fnv::FnvBuildHasher>,
    nodes: PathNode<I, O, M>,
}

impl<I, O, M> PathRouter<I, O, M> {
    fn new(route_config: &PathConfig, registry: &HandlerRegistry<I, O, M>) -> Self {
        let mut table = Self {
            static_paths: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
            nodes: PathNode::default(),
        };
        table.parse_config(route_config, registry);
        table
    }

    fn parse_config(
        &mut self,
        route_config: &PathConfig,
        handler_registry: &HandlerRegistry<I, O, M>,
    ) {
        for (path, methods) in route_config.paths.iter() {
            if !path.contains('*') {
                for (method, handlers) in methods {
                    let key = PathKey::new(method, path);
                    let mut registered_handlers: Vec<SharedHandler<I, O, M>> = vec![];
                    for handler in handlers {
                        let registered_handler = match handler_registry.find(handler) {
                            Ok(handler) => handler,
                            Err(_) => {
                                panic!("Handler {:?} not found", handler);
                            }
                        };
                        registered_handlers.push(registered_handler.clone());
                    }
                    self.static_paths.insert(key, Arc::new(registered_handlers));
                }
            }
            let path_segments = path.split('/');
            let mut current_node = &mut self.nodes;
            let mut depth = 0;
            for segment in path_segments {
                let path_segment = PathSegment::from_str(segment).unwrap();
                let is_wild_card = path_segment == PathSegment::Any;
                current_node = current_node
                    .children
                    .entry(path_segment)
                    .or_insert_with(PathNode::default);
                current_node.depth = depth;
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
                            panic!("Handler {} not found", handler);
                        }
                    };
                    registered_handlers.push(registered_handler);
                }
                current_node
                    .methods
                    .insert(method.to_string(), Arc::new(registered_handlers));
            }
        }
    }

    pub fn lookup(&self, path: &str, method: &str) -> Option<Arc<Vec<SharedHandler<I, O, M>>>> {
        if let Some(handlers) = self.static_paths.get(&PathKey::new(method, path)) {
            return Some(handlers.clone());
        }

        let string_segments = path.split('/');
        let mut matching_paths: Vec<&PathNode<I, O, M>> = vec![];
        let mut current_node = &self.nodes;
        for string_segment in string_segments {
            let path_segment = PathSegment::from_str(string_segment).unwrap();

            // Incoming requests cannot contain wildcards.
            if path_segment == PathSegment::Any {
                return None;
            }

            if let Some(wild_card_child) = current_node.children.get(&PathSegment::Any) {
                matching_paths.push(wild_card_child);
            }

            // As soon as the path no longer matches, we can stop.
            current_node = match current_node.children.get(&path_segment) {
                None => break,
                Some(child) => child,
            };
        }
        if matching_paths.is_empty() {
            return None;
        }

        let mut max_depth = 0;
        let mut best_match: Option<&PathNode<I, O, M>> = None;
        for path in matching_paths {
            if path.depth > max_depth && path.methods.contains_key(method) {
                max_depth = path.depth;
                best_match = Some(path);
            }
        }

        if best_match.is_some() {
            best_match.unwrap().methods.get(method).cloned()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteKey {
    method: String,
    path_hash: u64,
}

impl RouteKey {
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        let mut hasher = FnvHasher::default();
        path.into().hash(&mut hasher);
        let path_hash = hasher.finish();
        let method = method.into();
        Self { method, path_hash }
    }
}

pub struct DynamicRouteTable<I, O, M> {
    static_routes: DashMap<RouteKey, Arc<Vec<SharedHandler<I, O, M>>>, fnv::FnvBuildHasher>,
    dynamic_patterns: DashMap<
        String,
        HashMap<PathSegment, Vec<(Vec<PathSegment>, Arc<Vec<SharedHandler<I, O, M>>>)>>,
        fnv::FnvBuildHasher,
    >,
}

impl<I, O, M> DynamicRouteTable<I, O, M> {
    pub fn new() -> Self {
        Self {
            static_routes: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
            dynamic_patterns: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    /// Add a static route
    pub fn add_static_route(
        &self,
        method: String,
        path: String,
        executables: Vec<SharedHandler<I, O, M>>,
    ) {
        let key = RouteKey::new(method, &path);
        self.static_routes.insert(key, Arc::new(executables));
    }

    /// Add a dynamic route with wildcard segments
    pub fn add_dynamic_route(
        &self,
        method: String,
        path_segments: Vec<PathSegment>,
        executables: Vec<SharedHandler<I, O, M>>,
    ) {
        if path_segments.is_empty() {
            return;
        }
        let first_segment = &path_segments[0];
        let mut entry = self
            .dynamic_patterns
            .entry(method)
            .or_insert_with(HashMap::new);
        let patterns = entry.entry(first_segment.clone()).or_insert_with(Vec::new);
        patterns.push((path_segments, Arc::new(executables)));
    }

    pub fn lookup(&self, route_info: &RouteInfo) -> Option<Arc<Vec<SharedHandler<I, O, M>>>> {
        let key = RouteKey::new(&route_info.method, &route_info.path);
        if let Some(executables) = self.static_routes.get(&key) {
            return Some(executables.clone());
        }
        self.match_dynamic_route(route_info)
    }

    fn match_dynamic_route(
        &self,
        route_info: &RouteInfo,
    ) -> Option<Arc<Vec<SharedHandler<I, O, M>>>> {
        let method_patterns = self.dynamic_patterns.get(&route_info.method)?;
        let path_segments: Vec<&str> = route_info.path.split('/').collect();
        if path_segments.is_empty() {
            return None;
        }
        let first_segment = path_segments[0];
        let static_segment = PathSegment::Static(first_segment.to_string());
        if let Some(executables) =
            Self::check_patterns(&path_segments, &static_segment, &method_patterns)
        {
            return Some(executables);
        }
        let wildcard = PathSegment::Any;
        if let Some(executables) = Self::check_patterns(&path_segments, &wildcard, &method_patterns)
        {
            return Some(executables);
        }
        None
    }

    fn check_patterns(
        path_segments: &[&str],
        segment: &PathSegment,
        method_patterns: &Ref<
            String,
            HashMap<PathSegment, Vec<(Vec<PathSegment>, Arc<Vec<SharedHandler<I, O, M>>>)>>,
        >,
    ) -> Option<Arc<Vec<SharedHandler<I, O, M>>>> {
        let patterns = method_patterns.get(segment)?;
        for (pattern_segments, executables) in patterns {
            if Self::matches_segments(pattern_segments, path_segments) {
                return Some(executables.clone());
            }
        }
        None
    }

    fn matches_segments(pattern_segments: &[PathSegment], path_segments: &[&str]) -> bool {
        if pattern_segments.len() != path_segments.len() {
            return false;
        }

        for (i, pattern_segment) in pattern_segments.iter().enumerate() {
            match pattern_segment {
                PathSegment::Static(s) => {
                    if s != path_segments[i] {
                        return false;
                    }
                }
                PathSegment::Any => {
                    continue;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod test {
    use crate::handler::Handler;
    use crate::handler::registry::HandlerRegistry;
    use crate::router::exchange::Exchange;
    use crate::router::route::{PathConfig, PathRouter};
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
            exchange: &mut Exchange<(), (), ()>,
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
            vec![
                "test1".to_string(),
                "test2".to_string(),
                "test3".to_string(),
                "test4".to_string(),
            ],
        );
        methods.insert(
            "GET".to_string(),
            vec![
                "test1".to_string(),
                "test2".to_string(),
                "test3".to_string(),
                "test4".to_string(),
            ],
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

        let table = PathRouter::new(&config, &registry);

        let result = table.lookup("/api/v1/users", "GET");
        assert!(result.is_some());

        let result = table.lookup("/api/v1/someOtherEndpoint", "GET");
        assert!(result.is_some());

        let result = table.lookup("/invalid", "GET");
        assert!(result.is_none());
    }
}
