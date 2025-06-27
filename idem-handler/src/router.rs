use std::collections::HashMap;
use std::convert::Infallible;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Eq, PartialEq, Hash)]
enum PathSegment {
    Static(String),
    Any(String),
}

impl FromStr for PathSegment {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "*" {
            Ok(PathSegment::Any(s.to_string()))
        } else {
            Ok(PathSegment::Static(s.to_string()))
        }
    }
}

struct PathNode {
    children: HashMap<PathSegment, PathNode>,
    methods: HashMap<String, Arc<Vec<Executable>>>,
}

impl PathNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            methods: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ChainId(String);
#[derive(Debug, Clone)]
pub(crate) struct HandlerId(String);

#[derive(Debug, Clone)]
pub(crate) enum Executable {
    Handler(HandlerId),
    Chain(ChainId),
}

impl FromStr for Executable {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("@") {
            Ok(Executable::Chain(ChainId(s.to_string())))
        } else {
            Ok(Executable::Handler(HandlerId(s.to_string())))
        }
    }
}

#[derive(Debug, Clone)]
pub struct PathConfig {
    pub methods: HashMap<String, Vec<Executable>>,
}

pub struct RouterConfig {
    pub chains: HashMap<ChainId, Vec<HandlerId>>,
    pub paths: HashMap<String, PathConfig>,
}

pub struct Router {
    root: PathNode,
}

impl Router {
    pub fn new(config: &RouterConfig) -> Self {
        let router = Self {
            root: PathNode::new(),
        };
        let router = Router::parse_paths(router, config);
        router
    }

    fn parse_paths(mut router: Router, router_config: &RouterConfig) -> Router {
        for (path, config) in router_config.paths.iter() {
            let path_segments = path
                .split('/')
                // conversion is infallible - unwrap is ok.
                .map(|s| PathSegment::from_str(s).unwrap())
                .collect::<Vec<_>>();
            let mut current_node = &mut router.root;
            for segment in path_segments {
                current_node = current_node
                    .children
                    .entry(segment)
                    .or_insert_with(PathNode::new);
            }
            for (config_method, config_handlers) in &config.methods {
                current_node.methods.insert(config_method.clone(), Arc::new(config_handlers.clone()));
            }
        }
        router
    }
    
    fn get_handlers_from_path_and_method(&self, path: &str, method: &str) -> Option<Arc<Vec<Executable>>> {
        let segments = path.split('/').collect::<Vec<_>>();
        self.find_matching_path(&segments, 0, &self.root, method)
    }
    
    fn find_matching_path(&self, segments: &[&str], current_index: usize, current_node: &PathNode, method: &str) -> Option<Arc<Vec<Executable>>> {
        if current_index >= segments.len() {
            current_node.methods.get(method).cloned()
        } else {
            let current_segment = segments[current_index];
            for (segment, child) in &current_node.children {
                match segment {
                    PathSegment::Static(s) if s == current_segment => {
                        return self.find_matching_path(segments, current_index + 1, child, method);
                    }
                    PathSegment::Any(_) => {
                        return self.find_matching_path(segments, current_index + 1, child, method);
                    },
                    _ => continue
                }
            }
            None
        }
    }
}
