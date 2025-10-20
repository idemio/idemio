
use fnv::FnvHasher;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use thiserror::Error;
use uuid::Uuid;

/// A generic exchange container that manages input/output data flow with metadata and attachments.
///
/// # Type Parameters
/// - `I`: Input data type that must implement `Send + Sync`
/// - `O`: Output data type that must implement `Send + Sync`  
/// - `M`: Metadata type that must implement `Send + Sync`
///
/// # Behavior
/// Provides a structured way to handle data exchange with support for:
/// - Input/output data management with optional buffering
/// - Metadata attachment for contextual information
/// - Listener callbacks for data processing
/// - Type-safe attachments collection
/// - Unique identification via UUID
pub struct Exchange<I, O, M>
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    uuid: Uuid,
    metadata: Option<M>,
    input: Option<I>,
    output: Option<O>,
    input_listeners: Vec<Callback<I>>,
    output_listeners: Vec<Callback<O>>,
    attachments: Attachments,
}

impl<I, O, M> Exchange<I, O, M>
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    /// Creates a new exchange instance with a randomly generated UUID.
    ///
    /// # Returns
    /// A new `Self` instance with empty input, output, metadata, and listeners,
    /// plus a new attachments collection and auto-generated UUID.
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Exchange;
    /// use hyper::body::Bytes;
    ///
    /// let exchange: Exchange<Bytes, Bytes, String> = Exchange::new();
    /// ```
    ///
    /// # Behavior
    /// Initializes all optional fields to `None` and creates empty listener vectors.
    /// Each exchange gets a unique V4 UUID for identification.
    pub fn new() -> Self {
        Self {
            uuid: Uuid::new_v4(),
            metadata: None,
            input: None,
            output: None,
            input_listeners: Vec::new(),
            output_listeners: Vec::new(),
            attachments: Attachments::new(),
        }
    }

    /// Creates a new exchange instance with a specific UUID.
    ///
    /// # Parameters
    /// - `uuid`: The UUID to assign to this exchange instance
    ///
    /// # Returns
    /// A new `Self` instance with the provided UUID and empty data fields.
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Exchange;
    /// use uuid::Uuid;
    /// use hyper::body::Bytes;
    ///
    /// let uuid = Uuid::new_v4();
    /// let exchange: Exchange<Bytes, Bytes, String> = Exchange::new_with_uuid(uuid);
    /// ```
    pub fn new_with_uuid(uuid: Uuid) -> Self {
        Self {
            uuid,
            metadata: None,
            input: None,
            output: None,
            input_listeners: Vec::new(),
            output_listeners: Vec::new(),
            attachments: Attachments::new(),
        }
    }

    /// Sets the metadata for this exchange.
    ///
    /// # Parameters
    /// - `metadata`: The metadata value of type `M` to store
    pub fn set_metadata(&mut self, metadata: M) {
        self.metadata = Some(metadata);
    }

    /// Retrieves a reference to the stored metadata.
    ///
    /// # Returns
    /// `Result<&M, ExchangeError>` where:
    /// - `Ok(&M)` contains a reference to the metadata
    /// - `Err(ExchangeError)` if no metadata has been set
    ///
    /// # Errors
    /// Returns `ExchangeError::Read` if metadata has not been set.
    pub fn metadata(&self) -> Result<&M, ExchangeError> {
        match &self.metadata {
            None => Err(ExchangeError::read_error(
                &self.uuid,
                "Metadata has not been set",
            )),
            Some(metadata) => Ok(metadata),
        }
    }

    /// Returns a reference to the attachments' collection.
    pub fn attachments(&self) -> &Attachments {
        &self.attachments
    }

    /// Returns a mutable reference to the attachments' collection.
    pub fn attachments_mut(&mut self) -> &mut Attachments {
        &mut self.attachments
    }

    /// Adds a callback listener for input processing.
    ///
    /// # Parameters
    /// - `callback`: A closure that takes `&mut I` and `&mut Attachments` and implements
    ///   `FnMut + Send + Sync + 'static`
    pub fn add_input_listener(
        &mut self,
        callback: impl FnMut(&mut I, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.input_listeners.push(Callback::new(callback));
    }

    /// Adds a callback listener for output processing.
    ///
    /// # Parameters
    /// - `callback`: A closure that takes `&mut O` and `&mut Attachments` and implements
    ///   `FnMut + Send + Sync + 'static`
    pub fn add_output_listener(
        &mut self,
        callback: impl FnMut(&mut O, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.output_listeners.push(Callback::new(callback));
    }

    /// Returns a reference to the exchange's unique identifier.
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Sets the input data for this exchange.
    pub fn set_input(&mut self, input: I) {
        self.input = Some(input);
    }

    /// Retrieves a reference to the stored input data.
    pub async fn input(&self) -> Result<&I, ExchangeError> {
        match &self.input {
            Some(val) => Ok(val),
            None => Err(ExchangeError::read_error(&self.uuid, "No input available")),
        }
    }

    /// Retrieves mutable reference to the stored input data.
    pub async fn input_mut(&mut self) -> Result<&mut I, ExchangeError> {
        match &mut self.input {
            Some(val) => Ok(val),
            None => Err(ExchangeError::read_error(&self.uuid, "No input available"))
        }
    }

    /// Consumes and returns the stored input data, executing all input listeners.
    pub async fn take_input(&mut self) -> Result<I, ExchangeError> {
        match self.input.take() {
            Some(mut val) => {
                for mut callback in &mut self.input_listeners.drain(..) {
                    callback.invoke(&mut val, &mut self.attachments);
                }
                Ok(val)
            }
            None => Err(ExchangeError::take_error(
                &self.uuid,
                "No input available to take",
            )),
        }
    }

    /// Sets the output data for this exchange.
    pub fn set_output(&mut self, output: O) {
        self.output = Some(output);
    }

    /// Retrieves a reference to the stored output data.
    pub async fn output(&self) -> Result<&O, ExchangeError> {
        match &self.output {
            Some(val) => Ok(val),
            None => Err(ExchangeError::read_error(&self.uuid, "No output available")),
        }
    }

    /// Retrieves mutable reference to the stored output data.
    pub async fn output_mut(&mut self) -> Result<&mut O, ExchangeError> {
        match &mut self.output {
            Some(val) => Ok(val),
            None => Err(ExchangeError::read_error(&self.uuid, "No output available")),
        }
    }

    /// Consumes and returns the stored output data, executing all output listeners.
    pub async fn take_output(&mut self) -> Result<O, ExchangeError> {
        match self.output.take() {
            Some(mut val) => {
                // Execute callbacks
                for mut callback in &mut self.output_listeners.drain(..) {
                    callback.invoke(&mut val, &mut self.attachments);
                }
                Ok(val)
            }
            None => Err(ExchangeError::take_error(
                &self.uuid,
                "No output available to take",
            )),
        }
    }

    /// Checks if input data is currently available.
    pub fn has_input(&self) -> bool {
        self.input.is_some()
    }

    /// Checks if output data is currently available.
    pub fn has_output(&self) -> bool {
        self.output.is_some()
    }
}

pub struct Attachments {
    attachments: HashMap<AttachmentKey, Box<dyn Any + Send + Sync>, fnv::FnvBuildHasher>,
}

impl Attachments {
    /// Creates a new empty attachments' collection.
    pub fn new() -> Self {
        Self {
            attachments: HashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    /// Adds a typed value to the attachment collection.
    ///
    /// # Parameters
    /// - `key`: A string-like key that can be converted via `AsRef<str>`
    /// - `value`: A value of type `K` that implements `Send + Sync + 'static`
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Attachments;
    ///
    /// let mut attachments = Attachments::new();
    /// attachments.add::<u32>("user_id", 123u32);
    /// attachments.add::<String>("username", "alice".to_string());
    /// ```
    pub fn add<K>(&mut self, key: impl AsRef<str>, value: K)
    where
        K: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<K>();
        self.attachments
            .insert(AttachmentKey::new(key, type_id), Box::new(value));
    }

    /// Retrieves a reference to a typed value.
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Attachments;
    ///
    /// let mut attachments = Attachments::new();
    /// attachments.add::<u32>("user_id", 123u32);
    ///
    /// let user_id: Option<&u32> = attachments.get("user_id");
    /// assert_eq!(user_id, Some(&123));
    /// ```
    pub fn get<K>(&self, key: impl AsRef<str>) -> Option<&K>
    where
        K: Send + 'static,
    {
        let type_id = TypeId::of::<K>();
        if let Some(option_any) = self.attachments.get(&AttachmentKey::new(key, type_id)) {
            option_any.downcast_ref::<K>()
        } else {
            None
        }
    }

    /// Retrieves a mutable reference to a typed value.
    pub fn get_mut<K>(&mut self, key: impl AsRef<str>) -> Option<&mut K>
    where
        K: Send + 'static,
    {
        let type_id = TypeId::of::<K>();
        if let Some(option_any) = self.attachments.get_mut(&AttachmentKey::new(key, type_id)) {
            option_any.downcast_mut::<K>()
        } else {
            None
        }
    }
}

#[derive(Error, Debug)]
pub enum ExchangeError {
    #[error("Exchange '{uuid}' has already been completed")]
    Completed { uuid: Uuid },

    #[error("Read error occurred for exchange '{uuid}'. {message}")]
    Read { uuid: Uuid, message: String },

    #[error("Take error occurred for exchange '{uuid}'. {message}")]
    Take { uuid: Uuid, message: String },

    #[error("Callback error occurred for exchange '{uuid}'. {message}")]
    Callback { uuid: Uuid, message: String },
}

impl ExchangeError {
    #[inline]
    pub const fn exchange_completed(uuid: &Uuid) -> Self {
        ExchangeError::Completed { uuid: *uuid }
    }

    #[inline]
    pub fn read_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::Read {
            uuid: *uuid,
            message: msg.into(),
        }
    }

    #[inline]
    pub(crate) fn take_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::Take {
            uuid: *uuid,
            message: msg.into(),
        }
    }

    #[inline]
    pub(crate) fn callback_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::Callback {
            uuid: *uuid,
            message: msg.into(),
        }
    }
}

#[derive(PartialOrd, PartialEq, Hash, Eq)]
pub struct AttachmentKey {
    key_hash: u64,
    type_hash: u64,
}

impl AttachmentKey {
    pub fn new(key: impl AsRef<str>, type_id: TypeId) -> Self {
        let key = key.as_ref();
        let mut key_hasher = FnvHasher::default();
        key.hash(&mut key_hasher);
        let key_hash = key_hasher.finish();

        let mut type_hasher = FnvHasher::default();
        type_id.hash(&mut type_hasher);
        let type_hash = type_hasher.finish();

        Self {
            key_hash,
            type_hash,
        }
    }
}

pub struct Callback<T> {
    callback: Box<dyn FnMut(&mut T, &mut Attachments) + Send + Sync>,
}

impl<T> Callback<T>
where
    T: Send,
{
    pub fn new(callback: impl FnMut(&mut T, &mut Attachments) + Send + Sync + 'static) -> Self {
        Self {
            callback: Box::new(callback),
        }
    }

    pub fn invoke(&mut self, write: &mut T, attachments: &mut Attachments) {
        (self.callback)(write, attachments);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::exchange::Attachments;
    use hyper::body::Bytes;

    struct TestStruct;

    #[test]
    fn test_attachments() {
        let mut attachments = Attachments::new();
        let key1 = "test_key1";
        let key2 = "test_key2";
        let key3 = "test_key3";
        let key4 = "test_key4";
        {
            attachments.add::<u64>(key1, 1);
            attachments.add::<String>(key2, String::from("test"));
            attachments.add::<bool>(key3, true);
            let test_struct = TestStruct;
            attachments.add::<TestStruct>(key4, test_struct);
        }

        {
            assert!(attachments.get::<u64>(key1).is_some());
            assert!(attachments.get::<String>(key2).is_some());
            assert!(attachments.get::<bool>(key3).is_some());
            assert!(attachments.get::<TestStruct>(key4).is_some());
        }
    }

    #[tokio::test]
    async fn test_unified_exchange_buffered() {
        let mut exchange: Exchange<Bytes, Bytes, ()> = Exchange::new();

        // Test buffered input
        exchange.set_input(Bytes::from("test input"));
        assert!(exchange.has_input());

        // Test buffered output
        exchange.set_output(Bytes::from("test output"));
        assert!(exchange.has_output());

        let output = exchange.take_output().await.unwrap();
        assert_eq!(output, Bytes::from("test output"));
    }

    #[tokio::test]
    async fn test_unified_exchange_callbacks() {
        let mut exchange: Exchange<Bytes, Bytes, ()> = Exchange::new();

        exchange.add_input_listener(|input: &mut Bytes, _| {
            let mut new_data = b"prefix:".to_vec();
            new_data.extend_from_slice(input);
            *input = Bytes::from(new_data);
        });

        exchange.set_input(Bytes::from("test"));
        let input = exchange.take_input().await.unwrap();
        assert_eq!(input, Bytes::from("prefix:test"));
    }

    #[tokio::test]
    async fn test_unified_exchange_metadata() {
        let mut exchange: Exchange<(), (), String> = Exchange::new();

        exchange.set_metadata("test metadata".to_string());
        let metadata = exchange.metadata().unwrap();
        assert_eq!(metadata, "test metadata");
    }
}