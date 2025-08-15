use crate::handler::Handler;
use crate::handler::HandlerId;
use dashmap::{DashMap, Entry};
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during handler registry operations.
///
/// This enum defines the various error conditions that can arise when managing
/// handlers in the registry system, including handler lookup and registration failures.
#[derive(Error, Debug)]
pub enum HandlerRegistryError {
    /// Indicates that a handler with the specified ID could not be found in the registry.
    ///
    /// This error occurs when attempting to retrieve a handler that has not been
    /// registered or has been removed from the registry. The error includes the
    /// specific handler ID not found to aid in debugging.
    #[error("Handler with id '{handler_id}' not found.")]
    MissingHandler { handler_id: HandlerId },

    /// Indicates that registration failed due to a handler ID conflict.
    ///
    /// This error occurs when attempting to register a handler with an ID that
    /// is already in use by another handler. Handler IDs must be unique within
    /// a registry instance to ensure proper handler resolution.
    #[error("Handler with id '{handler_id}' already exists.")]
    ConflictingHandlerId { handler_id: HandlerId },
}

impl HandlerRegistryError {
    /// Creates a new `MissingHandler` error with the specified handler ID.
    ///
    /// # Parameters
    ///
    /// * `id` - The handler ID that could not be found
    ///
    /// # Returns
    ///
    /// A new `HandlerRegistryError::MissingHandler` variant
    #[inline]
    pub(crate) const fn missing_handler(id: HandlerId) -> Self {
        Self::MissingHandler { handler_id: id }
    }

    /// Creates a new `ConflictingHandlerId` error with the specified handler ID.
    ///
    /// # Parameters
    ///
    /// * `id` - The conflicting handler ID that caused the registration failure
    ///
    /// # Returns
    ///
    /// A new `HandlerRegistryError::ConflictingHandlerId` variant
    #[inline]
    pub(crate) const fn conflicting_handler_id(id: HandlerId) -> Self {
        Self::ConflictingHandlerId { handler_id: id }
    }
}

/// A generic trait for registry implementations that manage typed items with handler IDs.
///
/// This trait provides a common interface for registry systems that store and retrieve
/// items by `HandlerId`. It's designed to be flexible enough to support different
/// storage backends while maintaining consistent error handling.
///
/// # Type Parameters
///
/// * `T` - The type of items stored in the registry (must be `Send + Sync` for thread safety)
///
/// # Thread Safety
///
/// Implementations of this trait should be thread-safe when `T` implements `Send + Sync`.
pub trait Registry<T>
where
    T: Send + Sync,
{
    /// Retrieves an item from the registry by its handler ID.
    ///
    /// # Parameters
    ///
    /// * `id` - A reference to the handler ID to look up
    ///
    /// # Returns
    ///
    /// Returns `Ok(Arc<T>)` containing the requested item if found, or
    /// `Err(HandlerRegistryError::MissingHandler)` if no item with the
    /// specified ID exists in the registry.
    ///
    /// # Behavior
    ///
    /// The returned item is wrapped in an `Arc` to enable safe sharing across
    /// multiple threads and handlers. The registry retains ownership of the
    /// original item.
    fn find_with_id(&self, id: &HandlerId) -> Result<Arc<T>, HandlerRegistryError>;

    /// Registers a new item in the registry with the specified handler ID.
    ///
    /// # Parameters
    ///
    /// * `handler_id` - A reference to the unique identifier for the item
    /// * `handler` - The item to be registered
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if registration succeeds, or
    /// `Err(HandlerRegistryError::ConflictingHandlerId)` if an item with
    /// the same ID already exists in the registry.
    ///
    /// # Behavior
    ///
    /// The item is stored internally and can be retrieved later using the
    /// provided handler ID. Handler IDs must be unique within the registry.
    fn register_handler(
        &mut self,
        handler_id: &HandlerId,
        handler: T,
    ) -> Result<(), HandlerRegistryError>;
}

/// A thread-safe registry for managing handler instances with unique identifiers.
///
/// This struct provides a concurrent, high-performance registry implementation for storing
/// and retrieving handler instances. It uses `DashMap` for thread-safe concurrent access
/// and FNV hashing for optimal performance with string-based handler IDs.
///
/// # Type Parameters
///
/// * `In` - The input data type that registered handlers will process
/// * `Out` - The output data type that registered handlers will produce
/// * `Meta` - The metadata type associated with handler processing contexts
///
/// All type parameters must implement `Send + Sync` for thread safety.
///
/// # Thread Safety
///
/// This registry is fully thread-safe and can be safely shared across multiple async tasks
/// and threads. All operations are atomic and lock-free where possible. The internal
/// `DashMap` provides concurrent read/write access without blocking.
///
/// # Performance
///
/// The registry uses FNV (Fowler-Noll-Vo) hashing, which is optimized for string-based keys
/// like handler IDs. This provides fast hash computation and good distribution for typical
/// handler identifier patterns.
///
/// # Memory Management
///
/// Handlers are stored as `Arc<dyn Handler<In, Out, Meta>>` enabling efficient sharing
/// and automatic cleanup when no longer referenced. The registry maintains strong
/// references to all registered handlers.
pub struct HandlerRegistry<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    handlers: DashMap<HandlerId, Arc<dyn Handler<In, Out, Meta>>, fnv::FnvBuildHasher>,
}

impl<In, Out, Meta> HandlerRegistry<In, Out, Meta>
where
    In: Send + Sync,
    Out: Send + Sync,
    Meta: Send + Sync,
{
    /// Creates a new empty handler registry.
    ///
    /// # Returns
    ///
    /// A new `HandlerRegistry` instance with no registered handlers, configured
    /// with FNV hashing for optimal performance.
    ///
    /// # Behavior
    ///
    /// The registry is initialized with an empty concurrent hash map using FNV
    /// (Fowler-Noll-Vo) hashing, which provides excellent performance for
    /// string-based keys like handler identifiers.
    pub fn new() -> Self {
        Self {
            handlers: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    /// Retrieves a handler from the registry by its identifier.
    ///
    /// # Parameters
    ///
    /// * `id` - A reference to the handler ID to look up
    ///
    /// # Returns
    ///
    /// Returns `Ok(Arc<dyn Handler<In, Out, Meta>>)` containing the requested handler
    /// if found, or `Err(HandlerRegistryError::MissingHandler)` if no handler with
    /// the specified ID exists in the registry.
    ///
    /// # Behavior
    ///
    /// This method performs a thread-safe lookup in the concurrent hash map.
    /// The returned handler is wrapped in an `Arc` enabling safe sharing across
    /// multiple execution contexts. The registry retains ownership of the handler.
    ///
    /// # Thread Safety
    ///
    /// This method is fully thread-safe and can be called concurrently from multiple
    /// threads without synchronization. The underlying `DashMap` handles all necessary
    /// locking internally.
    pub(crate) fn find_with_id(
        &self,
        id: &HandlerId,
    ) -> Result<Arc<dyn Handler<In, Out, Meta>>, HandlerRegistryError> {
        match self.handlers.get(id) {
            None => Err(HandlerRegistryError::missing_handler(id.clone())),
            Some(handler) => Ok(handler.value().clone()),
        }
    }

    /// Registers a new handler in the registry with the specified identifier.
    ///
    /// # Parameters
    ///
    /// * `handler_id` - The unique identifier for the handler (takes ownership for internal storage)
    /// * `handler` - The handler implementation to register (must implement `Handler<In, Out, Meta>`)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if registration succeeds, or `Err(HandlerRegistryError::ConflictingHandlerId)`
    /// if a handler with the same ID already exists in the registry.
    ///
    /// # Errors
    ///
    /// This method returns an error if:
    /// * A handler with the same `handler_id` already exists in the registry
    ///
    /// # Behavior
    ///
    /// The handler is wrapped in an `Arc` and stored in the concurrent hash map.
    /// If a handler with the same ID already exists, the registration fails and
    /// the existing handler remains unchanged. Handler IDs must be unique within
    /// the registry instance.
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and uses atomic operations to prevent race conditions
    /// during handler registration. The check-and-insert operation is atomic, ensuring
    /// that duplicate registrations are properly detected even under concurrent access.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::convert::Infallible;
    /// use async_trait::async_trait;
    /// use idemio::exchange::Exchange;
    /// use idemio::handler::{registry::HandlerRegistry, Handler, HandlerId};
    /// use idemio::status::{ExchangeState, HandlerStatus};
    ///
    ///
    /// pub struct MyHandler;
    ///
    /// #[async_trait]
    /// impl Handler<String, String, ()> for MyHandler {
    ///     async fn exec<'a>(
    ///         &self,
    ///         exchange: &mut Exchange<'a, String, String, ()>
    ///     ) -> Result<HandlerStatus, Infallible>
    ///     {
    ///         Ok(HandlerStatus::new(ExchangeState::OK))
    ///     }
    ///
    ///     fn name(&self) -> &str {
    ///         "MyHandler"
    ///     }
    ///
    /// }
    /// let mut registry = HandlerRegistry::<String, String, ()>::new();
    /// let handler_id = HandlerId::new("my_handler");
    /// let handler = MyHandler;
    ///
    /// let result = registry.register_handler(handler_id, handler);
    /// assert!(result.is_ok());
    /// ```
    pub fn register_handler(
        &mut self,
        handler_id: HandlerId,
        handler: impl Handler<In, Out, Meta> + 'static,
    ) -> Result<(), HandlerRegistryError> {
        match self.handlers.entry(handler_id.clone()) {
            Entry::Occupied(_) => Err(HandlerRegistryError::conflicting_handler_id(handler_id)),
            Entry::Vacant(entry) => {
                entry.insert(Arc::new(handler));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Exchange;
    use crate::handler::Handler;
    use crate::status::{ExchangeState, HandlerStatus};
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
    impl Handler<String, String, ()> for TestHandler {
        async fn exec<'a>(
            &self,
            _exchange: &mut Exchange<'a, String, String, ()>,
        ) -> Result<HandlerStatus, Infallible> {
            Ok(HandlerStatus::new(ExchangeState::OK))
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[derive(Debug)]
    struct AnotherTestHandler;

    #[async_trait]
    impl Handler<String, String, ()> for AnotherTestHandler {
        async fn exec<'a>(
            &self,
            _exchange: &mut Exchange<'a, String, String, ()>,
        ) -> Result<HandlerStatus, Infallible> {
            Ok(HandlerStatus::new(ExchangeState::EXCHANGE_COMPLETED))
        }

        fn name(&self) -> &str {
            "another_test_handler"
        }
    }

    #[test]
    fn test_register_multiple_handlers_success() {
        let mut registry = HandlerRegistry::<String, String, ()>::new();

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
        let mut registry = HandlerRegistry::<String, String, ()>::new();
        let handler_id = HandlerId::new("duplicate_handler");

        let handler1 = TestHandler::new("first_handler");
        let handler2 = TestHandler::new("second_handler");

        // Register the first handler successfully
        let result1 = registry.register_handler(handler_id.clone(), handler1);
        assert!(result1.is_ok());

        // Try to register second handler with same ID
        let result2 = registry.register_handler(handler_id.clone(), handler2);
        assert!(result2.is_err());

        // Registry should still contain only the first handler
        assert_eq!(registry.handlers.len(), 1);
    }

    #[test]
    fn test_find_nonexistent_handler() {
        let registry = HandlerRegistry::<String, String, ()>::new();
        let nonexistent_id = HandlerId::new("nonexistent_handler");

        let result = registry.find_with_id(&nonexistent_id);

        assert!(result.is_err());
    }

    #[test]
    fn test_find_handler_after_multiple_registrations() {
        let mut registry = HandlerRegistry::<String, String, ()>::new();

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
        assert_eq!(result.unwrap().name(), "handler_beta");

        // Find the last handler
        let result = registry.find_with_id(&handler3_id);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "another_test_handler");

        // Find the first handler
        let result = registry.find_with_id(&handler1_id);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "handler_alpha");
    }

    #[test]
    fn test_handler_registry_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let mut registry = HandlerRegistry::<String, String, ()>::new();

        // Pre-register some handlers
        for i in 0..5 {
            let handler_id = HandlerId::new(format!("handler_{}", i));
            let handler = TestHandler::new(format!("handler_{}", i));
            registry.register_handler(handler_id, handler).unwrap();
        }

        let registry = Arc::new(registry);
        let mut handles = vec![];

        // Spawn multiple threads to concurrently access the registry
        for i in 0..5 {
            let registry_clone = Arc::clone(&registry);
            let handle = thread::spawn(move || {
                let handler_id = HandlerId::new(format!("handler_{}", i));
                let result = registry_clone.find_with_id(&handler_id);
                assert!(result.is_ok());
                result.unwrap().name().to_string()
            });
            handles.push(handle);
        }

        // Collect results
        let mut results = vec![];
        for handle in handles {
            results.push(handle.join().unwrap());
        }

        // Verify all handlers were found correctly
        assert_eq!(results.len(), 5);
        for i in 0..5 {
            assert!(results.contains(&format!("handler_{}", i)));
        }
    }

    #[test]
    fn test_empty_registry_operations() {
        let registry = HandlerRegistry::<(), (), ()>::new();
        let some_id = HandlerId::new("any_id");
        let result = registry.find_with_id(&some_id);
        assert!(result.is_err());
    }
}