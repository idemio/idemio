use std::collections::HashSet;
use crate::handler::{MiddlewareHandler, TerminationHandler};
use crate::handler::{HandlerId};
use dashmap::{DashMap, Entry};
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during handler registry operations.
#[derive(Error, Debug)]
pub enum HandlerRegistryError {
    /// Indicates that a handler with the specified ID could not be found in the registry.
    #[error("Handler with id '{handler_id}' not found.")]
    MissingHandler { handler_id: HandlerId },

    /// Indicates that registration failed due to a handler ID conflict.
    #[error("Handler with id '{handler_id}' already exists.")]
    ConflictingHandlerId { handler_id: HandlerId },
}

impl HandlerRegistryError {
    /// Creates a new `MissingHandler` error with the specified handler ID.
    #[inline]
    pub(crate) const fn missing_handler(id: HandlerId) -> Self {
        Self::MissingHandler { handler_id: id }
    }

    /// Creates a new `ConflictingHandlerId` error with the specified handler ID.
    #[inline]
    pub(crate) const fn conflicting_handler_id(id: HandlerId) -> Self {
        Self::ConflictingHandlerId { handler_id: id }
    }
}

/// A generic trait for registry implementations that manage typed items with handler IDs.
pub trait Registry<T>
where
    T: Send + Sync,
{
    /// Retrieves an item from the registry by its handler ID.
    fn find_with_id(&self, id: &HandlerId) -> Result<Arc<T>, HandlerRegistryError>;

    /// Registers a new item in the registry with the specified handler ID.
    fn register_handler(
        &mut self,
        handler_id: &HandlerId,
        handler: T,
    ) -> Result<(), HandlerRegistryError>;
}

/// A thread-safe registry for managing handler instances with unique identifiers.
pub struct HandlerRegistry<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{
    /// A set of all previously registered handlers.
    /// Contains request, termination, and response handler ids
    registered_ids: HashSet<HandlerId>,

    /// Registered request middlewares.
    /// These handlers deal with the input data.
    request_handlers: DashMap<HandlerId, Arc<dyn MiddlewareHandler<I>>, fnv::FnvBuildHasher>,

    /// Registered termination handlers.
    /// These handlers deal with converting input data into the expected output data.
    termination_handlers: DashMap<HandlerId, Arc<dyn TerminationHandler<I, O>>, fnv::FnvBuildHasher>,

    /// Registered response handlers.
    /// These handlers deal with the output data.
    response_handlers: DashMap<HandlerId, Arc<dyn MiddlewareHandler<O>>, fnv::FnvBuildHasher>,
}

impl<I, O> HandlerRegistry<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{
    /// Creates a new empty handler registry.
    pub fn new() -> Self {
        Self {
            registered_ids: HashSet::new(),
            request_handlers: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
            termination_handlers: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
            response_handlers: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    pub fn get_request_handler(
        &self,
        id: &HandlerId
    ) -> Result<Arc<dyn MiddlewareHandler<I>>, HandlerRegistryError> {

        match self.request_handlers.get(id) {
            None => Err(HandlerRegistryError::missing_handler(*id)),
            Some(handler) => Ok(handler.value().clone()),
        }
    }

    pub fn get_response_handler(
        &self,
        id: &HandlerId
    ) -> Result<Arc<dyn MiddlewareHandler<O>>, HandlerRegistryError> {
        match self.response_handlers.get(id) {
            None => Err(HandlerRegistryError::missing_handler(*id)),
            Some(handler) => Ok(handler.value().clone()),
        }
    }

    pub fn get_termination_handler(
        &self,
        id: &HandlerId
    ) -> Result<Arc<dyn TerminationHandler<I, O>>, HandlerRegistryError> {
        match self.termination_handlers.get(id) {
            None => Err(HandlerRegistryError::missing_handler(*id)),
            Some(handler) => Ok(handler.value().clone()),
        }
    }

    fn register_middleware<T>(
        handler_id: HandlerId,
        handler: impl MiddlewareHandler<T> + 'static,
        map: &mut DashMap<HandlerId, Arc<dyn MiddlewareHandler<T>>, fnv::FnvBuildHasher>,
        registered_ids: &mut HashSet<HandlerId>
    ) -> Result<(), HandlerRegistryError>
    where
        T: Send + Sync
    {
        if registered_ids.contains(&handler_id) {
            return Err(HandlerRegistryError::conflicting_handler_id(handler_id));
        }
        let handler = Arc::new(handler);
        registered_ids.insert(handler_id);
        map.insert(handler_id, handler);
        Ok(())
    }

    pub fn register_request_handler(
        &mut self,
        handler_id: HandlerId,
        handler: impl MiddlewareHandler<I> + 'static,
    ) -> Result<(), HandlerRegistryError> {
        Self::register_middleware(handler_id, handler, &mut self.request_handlers, &mut self.registered_ids)
    }

    pub fn register_response_handler(
        &mut self,
        handler_id: HandlerId,
        handler: impl MiddlewareHandler<O> + 'static,
    ) -> Result<(), HandlerRegistryError> {
        Self::register_middleware(handler_id, handler, &mut self.response_handlers, &mut self.registered_ids)
    }

    pub fn register_termination_handler(
        &mut self,
        handler_id: HandlerId,
        handler: impl TerminationHandler<I, O> + 'static,
    ) -> Result<(), HandlerRegistryError> {
        if self.registered_ids.contains(&handler_id) {
            return Err(HandlerRegistryError::conflicting_handler_id(handler_id));
        }
        let handler = Arc::new(handler);
        self.registered_ids.insert(handler_id);
        self.termination_handlers.insert(handler_id, handler);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::{Exchange};
    use crate::handler::{MiddlewareResponse, MiddlewareResult, LabeledHandler};
    use async_trait::async_trait;
    use idemio_macro::Handler;

    // Test handler implementations for comprehensive testing
    #[derive(Debug, Handler)]
    struct TestHandler {
        name: String,
    }

    impl TestHandler {
        fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }
    }

    #[async_trait]
    impl MiddlewareHandler<String> for TestHandler {
        async fn exec(
            &self,
            _exchange: &mut Exchange<String>,
        ) -> MiddlewareResult {
            MiddlewareResponse::ok()
        }
    }

    #[derive(Debug, Handler)]
    struct AnotherTestHandler;
    #[async_trait]
    impl MiddlewareHandler<String> for AnotherTestHandler {
        async fn exec(
            &self,
            _exchange: &mut Exchange<String>,
        ) -> MiddlewareResult {
            MiddlewareResponse::ok()
        }
    }

    #[test]
    fn test_register_multiple_handlers_success() {
        let mut registry = HandlerRegistry::<String, String>::new();

        let handler1_id = HandlerId::new("handler_1");
        let handler1 = TestHandler::new("handler_1");

        let handler2_id = HandlerId::new("handler_2");
        let handler2 = AnotherTestHandler;

        let result1 = registry.register_request_handler(handler1_id.clone(), handler1);
        let result2 = registry.register_request_handler(handler2_id.clone(), handler2);

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(registry.request_handlers.len(), 2);
        assert!(registry.request_handlers.contains_key(&handler1_id));
        assert!(registry.request_handlers.contains_key(&handler2_id));
    }

    #[test]
    fn test_register_handler_with_conflicting_id() {
        let mut registry = HandlerRegistry::<String, String>::new();
        let handler_id = HandlerId::new("duplicate_handler");

        let handler1 = TestHandler::new("first_handler");
        let handler2 = TestHandler::new("second_handler");

        // Register the first handler successfully
        let result1 = registry.register_request_handler(handler_id.clone(), handler1);
        assert!(result1.is_ok());

        // Try to register a second handler with the same ID
        let result2 = registry.register_request_handler(handler_id.clone(), handler2);
        assert!(result2.is_err());

        // Registry should still contain only the first handler
        assert_eq!(registry.request_handlers.len(), 1);
    }

    #[test]
    fn test_find_nonexistent_handler() {
        let registry = HandlerRegistry::<String, String>::new();
        let nonexistent_id = HandlerId::new("nonexistent_handler");
        let result: Result<Arc<dyn MiddlewareHandler<String>>, _> = registry.get_request_handler(&nonexistent_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_handler_after_multiple_registrations() {
        let mut registry = HandlerRegistry::<String, String>::new();

        let handler1_id = HandlerId::new("handler_alpha");
        let handler1 = TestHandler::new("handler_alpha");

        let handler2_id = HandlerId::new("handler_beta");
        let handler2 = TestHandler::new("handler_beta");

        let handler3_id = HandlerId::new("handler_gamma");
        let handler3 = AnotherTestHandler;

        registry
            .register_request_handler(handler1_id.clone(), handler1)
            .unwrap();
        registry
            .register_request_handler(handler2_id.clone(), handler2)
            .unwrap();
        registry
            .register_request_handler(handler3_id.clone(), handler3)
            .unwrap();

        // Find the middle handler
        let result = registry.get_request_handler(&handler2_id);
        assert!(result.is_ok());

        // Find the last handler
        let result = registry.get_request_handler(&handler3_id);
        assert!(result.is_ok());

        // Find the first handler
        let result = registry.get_request_handler(&handler1_id);
        assert!(result.is_ok());
    }
}
