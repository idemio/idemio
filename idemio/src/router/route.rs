use crate::router::{PathSegment, RouteInfo};
use dashmap::DashMap;
use dashmap::mapref::one::Ref;
use fnv::FnvHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use crate::handler::SharedHandler;

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

    pub fn from_str(method: impl Into<String>, path: impl Into<String>) -> Self {
        let mut hasher = FnvHasher::default();
        path.into().hash(&mut hasher);
        let path_hash = hasher.finish();
        Self {
            method: method.into(),
            path_hash,
        }
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
        let key = RouteKey::from_str(&route_info.method, &route_info.path);
        if let Some(executables) = self.static_routes.get(&key) {
            return Some(executables.clone());
        }
        self.match_dynamic_route(route_info)
    }
    
    
    fn match_dynamic_route(&self, route_info: &RouteInfo) -> Option<Arc<Vec<SharedHandler<I, O, M>>>> {
        let method_patterns = self.dynamic_patterns.get(&route_info.method)?;
        let path_segments: Vec<&str> = route_info
            .path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
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
        let wildcard = PathSegment::Any("*".to_string());
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
                PathSegment::Any(_) => {
                    continue;
                }
            }
        }

        true
    }
}

