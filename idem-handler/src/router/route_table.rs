use dashmap::DashMap;
use fnv::FnvHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use dashmap::mapref::one::Ref;
use crate::router::{Executable, PathSegment, RouteInfo};

/// A pre-computed route table that stores method and path combinations
/// for fast lookups during request routing
pub struct RouteTable {
    // Using DashMap for concurrent access without locking the entire table
    route_map: DashMap<RouteKey, Arc<Vec<Executable>>, fnv::FnvBuildHasher>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteKey {
    method: String,
    path_hash: u64,
}

impl RouteKey {
    pub fn new(method: String, path: &str) -> Self {
        let mut hasher = FnvHasher::default();
        path.hash(&mut hasher);
        let path_hash = hasher.finish();

        Self {
            method,
            path_hash,
        }
    }

    pub fn from_str(method: &str, path: &str) -> Self {
        let mut hasher = FnvHasher::default();
        path.hash(&mut hasher);
        let path_hash = hasher.finish();

        Self {
            method: method.to_string(),
            path_hash,
        }
    }
}

impl RouteTable {
    pub fn new() -> Self {
        Self {
            route_map: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    pub fn insert(&self, method: String, path: String, executables: Arc<Vec<Executable>>) {
        let key = RouteKey::new(method, &path);
        self.route_map.insert(key, executables);
    }

    pub fn get(&self, method: &str, path: &str) -> Option<Arc<Vec<Executable>>> {
        let key = RouteKey::from_str(method, path);
        self.route_map.get(&key).map(|entry| entry.clone())
    }

    pub fn contains(&self, method: &str, path: &str) -> bool {
        let key = RouteKey::from_str(method, path);
        self.route_map.contains_key(&key)
    }

    pub fn remove(&self, method: &str, path: &str) -> Option<Arc<Vec<Executable>>> {
        let key = RouteKey::from_str(method, path);
        self.route_map.remove(&key).map(|(_, executables)| executables)
    }

    /// Clear all entries from the route table
    pub fn clear(&self) {
        self.route_map.clear();
    }
}

/// A more sophisticated route table that handles dynamic path segments with wildcards
pub struct DynamicRouteTable {
    // Exact static routes for direct lookups (method + path)
    static_routes: DashMap<RouteKey, Arc<Vec<Executable>>, fnv::FnvBuildHasher>,
    // Dynamic routes with path patterns that need pattern matching
    dynamic_patterns: DashMap<String, HashMap<PathSegment, Vec<(Vec<PathSegment>, Arc<Vec<Executable>>)>>, fnv::FnvBuildHasher>,
}

impl DynamicRouteTable {
    pub fn new() -> Self {
        Self {
            static_routes: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
            dynamic_patterns: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    /// Add a static route
    pub fn add_static_route(&self, method: String, path: String, executables: Arc<Vec<Executable>>) {
        let key = RouteKey::new(method, &path);
        self.static_routes.insert(key, executables);
    }

    /// Add a dynamic route with wildcard segments
    pub fn add_dynamic_route(
        &self, 
        method: String, 
        path_segments: Vec<PathSegment>, 
        executables: Arc<Vec<Executable>>
    ) {
        if path_segments.is_empty() {
            return;
        }

        let first_segment = &path_segments[0];
        let mut entry = self.dynamic_patterns.entry(method.clone())
            .or_insert_with(HashMap::new);

        let patterns = entry.entry(first_segment.clone())
            .or_insert_with(Vec::new);

        // Store the path segments directly along with the executables
        patterns.push((path_segments, executables));
    }


    /// Look up a route by the route info
    pub fn lookup(&self, route_info: &RouteInfo) -> Option<Arc<Vec<Executable>>> {
        // First try exact match for better performance
        let key = RouteKey::from_str(&route_info.method, &route_info.path);
        if let Some(executables) = self.static_routes.get(&key) {
            return Some(executables.clone());
        }

        // If not found, try dynamic route matching
        self.match_dynamic_route(route_info)
    }

    /// Match a route against dynamic patterns
    fn match_dynamic_route(&self, route_info: &RouteInfo) -> Option<Arc<Vec<Executable>>> {
        let method_patterns = self.dynamic_patterns.get(&route_info.method)?;

        // Split the path into segments
        let path_segments: Vec<&str> = route_info.path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if path_segments.is_empty() {
            return None;
        }

        // Try to match against patterns starting with this segment
        let first_segment = path_segments[0];

        // Create static segment once to avoid repeated allocations
        let static_segment = PathSegment::Static(first_segment.to_string());
        if let Some(executables) = Self::check_patterns(&path_segments, &static_segment, &method_patterns) {
            return Some(executables);
        }

        // Create wildcard segment once
        let wildcard = PathSegment::Any("*".to_string());
        if let Some(executables) = Self::check_patterns(&path_segments, &wildcard, &method_patterns) {
            return Some(executables);
        }
        None
    }
    
    fn check_patterns(
        path_segments: &[&str],
        segment: &PathSegment,
        method_patterns: &Ref<String, HashMap<PathSegment, Vec<(Vec<PathSegment>, Arc<Vec<Executable>>)>>>
    ) -> Option<Arc<Vec<Executable>>> {
        let patterns = method_patterns.get(segment)?;
        for (pattern_segments, executables) in patterns {
            if Self::matches_segments(pattern_segments, path_segments) {
                return Some(executables.clone());
            }
        }
        None
    }

    /// Check if a path matches a pattern
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
                },
                PathSegment::Any(_) => {
                    // Wildcard matches any segment
                    continue;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{ChainId, HandlerId, RouteInfo};

    fn create_test_executables() -> Arc<Vec<Executable>> {
        Arc::new(vec![
            Executable::Handler(HandlerId("test_handler".to_string())),
            Executable::Chain(ChainId("@test_chain".to_string())),
        ])
    }

    #[test]
    fn test_route_table_static_routes() {
        let route_table = RouteTable::new();
        let executables = create_test_executables();

        route_table.insert("GET".to_string(), "/api/users".to_string(), executables.clone());

        // Test successful lookup
        let result = route_table.get("GET", "/api/users");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), executables);

        // Test unsuccessful lookup - different method
        let result = route_table.get("POST", "/api/users");
        assert!(result.is_none());

        // Test unsuccessful lookup - different path
        let result = route_table.get("GET", "/api/products");
        assert!(result.is_none());
    }

    #[test]
    fn test_dynamic_route_table() {
        let route_table = DynamicRouteTable::new();
        let executables1 = create_test_executables();
        let executables2 = Arc::new(vec![Executable::Handler(HandlerId("user_handler".to_string()))]);

        // Add a static route
        route_table.add_static_route("GET".to_string(), "/api/static".to_string(), executables1.clone());

        // Add a dynamic route
        let path_segments = vec![
            PathSegment::Static("api".to_string()),
            PathSegment::Static("users".to_string()),
            PathSegment::Any("*".to_string())
        ];
        route_table.add_dynamic_route("GET".to_string(), path_segments, executables2.clone());

        // Test static route lookup
        let route_info1 = RouteInfo::new("/api/static".to_string(), "GET".to_string());
        let result1 = route_table.lookup(&route_info1);
        assert!(result1.is_some());
        assert_eq!(result1.unwrap(), executables1);

        // Test dynamic route lookup
        let route_info2 = RouteInfo::new("/api/users/123".to_string(), "GET".to_string());
        let result2 = route_table.lookup(&route_info2);
        assert!(result2.is_some());
        assert_eq!(result2.unwrap(), executables2);

        // Test non-matching route
        let route_info3 = RouteInfo::new("/api/products/123".to_string(), "GET".to_string());
        let result3 = route_table.lookup(&route_info3);
        assert!(result3.is_none());
    }
}
