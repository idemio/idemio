use std::collections::HashMap;

pub struct PathConfig {
    pub chains: HashMap<String, Vec<String>>,
    pub paths: HashMap<String, HashMap<String, PathChain>>,
}

#[derive(Debug, Clone)]
pub struct PathChain {
    pub(crate) request_handlers: Option<Vec<String>>,
    pub(crate) termination_handler: Option<String>,
    pub(crate) response_handlers: Option<Vec<String>>,
}

impl PathChain {
    pub fn new() -> Self {
        Self {
            request_handlers: None,
            termination_handler: None,
            response_handlers: None,
        }
    }

    pub fn add_request_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        match &mut self.request_handlers {
            Some(handlers) => handlers.push(handler.into()),
            None => {
                self.request_handlers = Some(vec![handler.into()]);
            }
        }
        self
    }

    pub fn add_request_handlers(&mut self, handlers: Box<[impl Into<String>]>) -> &mut Self {
        for handler in handlers {
            self.add_request_handler(handler.into());
        }
        self
    }

    pub fn set_termination_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        self.termination_handler = Some(handler.into());
        self
    }
    pub fn add_response_handler(&mut self, handler: impl Into<String>) -> &mut Self {
        match &mut self.response_handlers {
            Some(handlers) => handlers.push(handler.into()),
            None => {
                self.response_handlers = Some(vec![handler.into()]);
            }
        }
        self
    }

    pub fn add_response_handlers(&mut self, handlers: Box<[impl Into<String>]>) -> &mut Self {
        for handler in handlers {
            self.add_response_handler(handler.into());
        }
        self
    }
}