use crate::handler::Handler;
use crate::handler::HandlerId;
use dashmap::{DashMap, Entry};
use std::sync::Arc;
use thiserror::Error;
use crate::exchange::Exchange;

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
    handlers: DashMap<HandlerId, Arc<dyn Handler<I, O>>, fnv::FnvBuildHasher>,
}

impl<I, O> HandlerRegistry<I, O>
where
    I: Send + Sync,
    O: Send + Sync,
{
    /// Creates a new empty handler registry.
    pub fn new() -> Self {
        Self {
            handlers: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    /// Retrieves a handler from the registry by its identifier.
    pub fn find_with_id(
        &self,
        id: &HandlerId,
    ) -> Result<Arc<dyn Handler<I, O>>, HandlerRegistryError> {
        match self.handlers.get(id) {
            None => Err(HandlerRegistryError::missing_handler(id.clone())),
            Some(handler) => Ok(handler.value().clone()),
        }
    }

    /// Registers a new handler in the registry with the specified identifier.
    pub fn register_handler(
        &mut self,
        handler_id: HandlerId,
        handler: impl Handler<I, O> + 'static,
    ) -> Result<(), HandlerRegistryError> {
        let handler = Arc::new(handler);
        match self.handlers.entry(handler_id.clone()) {
            Entry::Occupied(_) => Err(HandlerRegistryError::conflicting_handler_id(handler_id)),
            Entry::Vacant(entry) => {
                entry.insert(handler);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Exchange;
    use crate::handler::{Handler, HandlerError, HandlerFlow, HandlerResponse};
    use async_trait::async_trait;
    use std::convert::Infallible;

    // Test handler implementations for comprehensive testing
    #[derive(Debug)]
    struct TestHandler {
        name: String,
    }

    impl TestHandler {
        fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }
    }

    #[async_trait]
    impl Handler<String, String> for TestHandler {
        fn id(&self) -> &'static str {
            "TestHandler"
        }

        async fn exec(
            &self,
            _exchange: &mut Exchange<String, String>,
        ) -> HandlerResponse {
            HandlerFlow::ok()
        }
    }

    #[derive(Debug)]
    struct AnotherTestHandler;

    #[async_trait]
    impl Handler<String, String> for AnotherTestHandler {
        fn id(&self) -> &'static str {
            "AnotherTestHandler"
        }

        async fn exec(
            &self,
            _exchange: &mut Exchange<String, String>,
        ) -> HandlerResponse {
            HandlerFlow::ok()
        }
    }

    #[test]
    fn test_register_multiple_handlers_success() {
        let mut registry = HandlerRegistry::<String, String>::new();

        let handler1_id = HandlerId::new("handler_1");
        let handler1 = TestHandler::new("handler_1");

        let handler2_id = HandlerId::new("handler_2");
        let handler2 = AnotherTestHandler;

        let result1 = registry.register_handler(handler1_id.clone(), handler1);
        let result2 = registry.register_handler(handler2_id.clone(), handler2);

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(registry.handlers.len(), 2);
        assert!(registry.handlers.contains_key(&handler1_id));
        assert!(registry.handlers.contains_key(&handler2_id));
    }

    #[test]
    fn test_register_handler_with_conflicting_id() {
        let mut registry = HandlerRegistry::<String, String>::new();
        let handler_id = HandlerId::new("duplicate_handler");

        let handler1 = TestHandler::new("first_handler");
        let handler2 = TestHandler::new("second_handler");

        // Register the first handler successfully
        let result1 = registry.register_handler(handler_id.clone(), handler1);
        assert!(result1.is_ok());

        // Try to register a second handler with the same ID
        let result2 = registry.register_handler(handler_id.clone(), handler2);
        assert!(result2.is_err());

        // Registry should still contain only the first handler
        assert_eq!(registry.handlers.len(), 1);
    }

    #[test]
    fn test_find_nonexistent_handler() {
        let registry = HandlerRegistry::<String, String>::new();
        let nonexistent_id = HandlerId::new("nonexistent_handler");

        let result: Result<Arc<dyn Handler<String, String>>, _> = registry.find_with_id(&nonexistent_id);

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
            .register_handler(handler1_id.clone(), handler1)
            .unwrap();
        registry
            .register_handler(handler2_id.clone(), handler2)
            .unwrap();
        registry
            .register_handler(handler3_id.clone(), handler3)
            .unwrap();

        // Find the middle handler
        let result = registry.find_with_id(&handler2_id);
        assert!(result.is_ok());

        // Find the last handler
        let result = registry.find_with_id(&handler3_id);
        assert!(result.is_ok());

        // Find the first handler
        let result = registry.find_with_id(&handler1_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_registry_operations() {
        let registry = HandlerRegistry::<(), ()>::new();
        let some_id = HandlerId::new("any_id");
        let result = registry.find_with_id(&some_id);
        assert!(result.is_err());
    }
}
