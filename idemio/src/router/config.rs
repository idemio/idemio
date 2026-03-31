use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

/// Top-level configuration structure for router services
#[derive(Debug, Serialize, Deserialize)]
pub enum ConfigStructure {
    /// Single service configuration
    #[serde(alias = "single", alias = "SINGLE")]
    Single(RouterConfig),

    /// Shared configuration for multiple services
    #[serde(alias = "shared", alias = "SHARED")]
    Shared(SharedConfig),
}

/// Types of routing keys supported by the router
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum RouteType {
    /// No specific routing - uses default routing behavior
    None,

    /// Route based on HTTP header value
    #[serde(rename = "header", alias = "Header", alias = "HEADER")]
    Header(String),

    /// Route based on a request path
    #[serde(rename = "path", alias = "Path", alias = "PATH")]
    Path,
}

impl RouteType {
    /// Create a header-based routing type
    pub fn header(header_name: impl Into<String>) -> Self {
        Self::Header(header_name.into())
    }

    /// Create a path-based routing type
    pub fn path() -> Self {
        Self::Path
    }
}

/// Configuration for shared multiservice routing
///
/// This structure defines how multiple services should be routed based on
/// a shared routing key. Each service can have its own complete router configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct SharedConfig {
    /// The routing key type that determines how to select services
    pub(crate) key: RouteType,
    /// Map of service names to their individual router configurations
    pub(crate) services: HashMap<String, RouterConfig>,
}

/// Main router configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Set of all handler names available for routing
    pub handlers: HashSet<String>,
    /// Named handler chains for reusability
    pub chains: HashMap<String, Vec<String>>,
    /// Route definitions specifying how requests are handled
    pub paths: HashMap<String, HashMap<String, PathChain>>,
}

/// Handler chain definition for a specific route.
///
/// This structure defines the complete handler execution pipeline for a route,
/// including request processing, termination, and response processing phases.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathChain {
    /// Handlers executed during the request processing phase.
    ///
    /// These handlers run before the termination handler
    #[serde(skip_serializing_if = "Option::is_none", rename = "request")]
    pub request_handlers: Option<Vec<String>>,

    /// The termination handler that produces the final response.
    ///
    /// This handler is responsible for generating the actual response content.
    /// Only one termination handler is allowed per route.
    #[serde(rename = "termination")]
    pub termination_handler: String,

    /// Handlers executed during the response processing phase.
    ///
    /// These handlers run after the termination handler.
    #[serde(skip_serializing_if = "Option::is_none", rename = "response")]
    pub response_handlers: Option<Vec<String>>,
}

impl PathChain {
    /// Create a new empty path chain
    ///
    /// # Returns
    ///
    /// A new `PathChain` with all handler lists set to `None`
    pub fn new() -> Self {
        Self {
            request_handlers: None,
            termination_handler: "default".to_string(),
            response_handlers: None,
        }
    }

    /// Add a handler to the request processing phase
    pub(crate) fn add_request_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.request_handlers
            .get_or_insert_with(Vec::new)
            .push(handler.into());
        self
    }

    /// Set the termination handler for this path chain
    pub(crate) fn termination_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.termination_handler = handler.into();
        self
    }

    /// Add a handler to the response processing phase
    pub(crate) fn add_response_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.response_handlers
            .get_or_insert_with(Vec::new)
            .push(handler.into());
        self
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json;

    /// Test loading shared configuration from JSON
    #[test]
    fn load_shared_config() {
        let json_config = r#"
        {
            "shared": {
                "key": "path",
                "services": {
                    "api_v1": {
                        "handlers": ["auth", "handler1"],
                        "chains": {},
                        "routes": {
                            "path": {
                                "/users": {
                                    "GET": {
                                        "request": ["auth"],
                                        "termination": "handler1"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "#;

        let config: ConfigStructure = serde_json::from_str(json_config).unwrap();
        match config {
            ConfigStructure::Shared(shared) => {
                assert_eq!(shared.key, RouteType::Path);
                assert!(shared.services.contains_key("api_v1"));
            }
            _ => panic!("Expected shared configuration"),
        }
    }

    /// Test loading a single service configuration from JSON
    #[test]
    fn load_single_config() {
        let json_config = r#"
        {
            "single": {
                "handlers": ["handler1", "handler2"],
                "chains": {
                    "auth_chain": ["handler1", "handler2"]
                },
                "routes": {
                    "path": {
                        "/test": {
                            "GET": {
                                "termination": "handler1"
                            }
                        }
                    }
                }
            }
        }
        "#;

        let config: ConfigStructure = serde_json::from_str(json_config).unwrap();
        match config {
            ConfigStructure::Single(single) => {
                assert!(single.handlers.contains("handler1"));
                assert!(single.handlers.contains("handler2"));
                assert!(single.chains.contains_key("auth_chain"));
            }
            _ => panic!("Expected single configuration"),
        }
    }
}