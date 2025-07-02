use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
pub use crate::handler::config::{ChainId, Executable, HandlerId};

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PathSegment {
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

#[derive(Debug, Clone)]
pub struct PathConfig {
    pub methods: HashMap<String, Vec<Executable>>,
}

pub struct RouterConfig {
    pub chains: HashMap<ChainId, Vec<HandlerId>>,
    pub paths: HashMap<String, PathConfig>,
}

#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub path: String,
    pub method: String,
}

impl Display for RouteInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RouteInfo {{ path: {}, method: {} }}",
            self.path, self.method
        )
    }
}

impl RouteInfo {
    pub fn new(path: impl Into<String>, method: impl Into<String>) -> Self {
        Self { path: path.into(), method: method.into() }
    }
}
