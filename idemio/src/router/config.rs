use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

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