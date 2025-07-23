pub mod collector;
use crate::exchange::collector::CollectorError;
use fnv::FnvHasher;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use crate::exchange::collector::{StreamCollector, StreamOrValue};
use futures_util::{Stream, StreamExt, pin_mut};
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::pin::Pin;
use uuid::Uuid;
type StreamResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A stateful container for managing input and output data flows in a single exchange operation.
///
/// The Exchange struct represents a complete data processing pipeline that can handle various
/// flow patterns, including HTTP request/response cycles, TCP/IP communications, or any other
/// middleware-based data exchange. It provides flexible data handling through both buffered
/// and streaming approaches.
///
/// # Type Parameters
/// - `'a`: The lifetime parameter for streams and collectors
/// - `I`: The input data type, must implement `Send + Sync`
/// - `O`: The output data type, must implement `Send + Sync`
/// - `M`: The metadata type, must implement `Send + Sync`
///
/// # Features
/// - **Stateful Processing**: Stores data and context between different processing stages
/// - **Flexible Storage**: Supports both buffered values and streaming data
/// - **Memory Efficient**: Streams are only fully loaded when required
/// - **Stream Proxying**: Allows forwarding streams without consuming them
/// - **Listener Support**: Callbacks for input and output processing
/// - **Attachments**: Additional context storage for middleware communication
///
/// # Flow Patterns
///
/// ## Buffered → Buffered
/// ```text
/// ┌─────────────────────────────────────────────────────────────────┐
/// │                    BUFFERED → BUFFERED                          │
/// ├─────────────────────────────────────────────────────────────────┤
/// │                                                                 │
/// │  ┌─────────────┐    ┌─────────────────────┐    ┌─────────────┐  │
/// │  │   Request   │───▶│     Exchange        │───▶│  Response   │  │
/// │  │    Data     │    │                     │    │    Data     │  │
/// │  │             │    │  ┌───────────────┐  │    │             │  │
/// │  │ "Complete   │    │  │ Input Buffer  │  │    │ "Complete   │  │
/// │  │  Payload"   │    │  │  (Stored)     │  │    │  Result"    │  │
/// │  └─────────────┘    │  └───────────────┘  │    └─────────────┘  │
/// │                     │                     │                     │
/// │                     │  ┌───────────────┐  │                     │
/// │                     │  │Output Buffer  │  │                     │
/// │                     │  │  (Generated)  │  │                     │
/// │                     │  └───────────────┘  │                     │
/// │                     └─────────────────────┘                     │
/// │                                                                 │
/// │  Flow: Single value in → Processing → Single value out          │
/// │  Memory: Both request and response fully loaded in memory       │
/// └─────────────────────────────────────────────────────────────────┘
/// ```
///
/// ## Streaming → Buffered
/// ```text
/// ┌─────────────────────────────────────────────────────────────────┐
/// │                   STREAMING → BUFFERED                          │
/// ├─────────────────────────────────────────────────────────────────┤
/// │                                                                 │
/// │  ┌─────────────┐    ┌─────────────────────┐    ┌─────────────┐  │
/// │  │ Chunk 1     │───┐│     Exchange        │───▶│  Response   │  │
/// │  ├─────────────┤   ││                     │    │    Data     │  │
/// │  │ Chunk 2     │───┤│  ┌───────────────┐  │    │             │  │
/// │  ├─────────────┤   ││  │ Stream Input  │  │    │ "Complete   │  │
/// │  │ Chunk 3     │───┤│  │ + Collector   │  │    │  Result"    │  │
/// │  ├─────────────┤   ││  │               │  │    │             │  │
/// │  │   ...       │───┘│  └───────────────┘  │    └─────────────┘  │
/// │  └─────────────┘    │         │           │                     │
/// │                     │         ▼           │                     │
/// │                     │  ┌───────────────┐  │                     │
/// │                     │  │Output Buffer  │  │                     │
/// │                     │  │ (Collected &  │  │                     │
/// │                     │  │  Generated)   │  │                     │
/// │                     │  └───────────────┘  │                     │
/// │                     └─────────────────────┘                     │
/// │                                                                 │
/// │  Flow: Stream in → Collect → Process → Single value out         │
/// │  Memory: Request streamed, response buffered                    │
/// └─────────────────────────────────────────────────────────────────┘
/// ```
///
/// ## Buffered → Streaming
/// ```text
/// ┌─────────────────────────────────────────────────────────────────┐
/// │                   BUFFERED → STREAMING                          │
/// ├─────────────────────────────────────────────────────────────────┤
/// │                                                                 │
/// │  ┌─────────────┐    ┌─────────────────────┐    ┌─────────────┐  │
/// │  │   Request   │───▶│     Exchange        │───┐│ Chunk 1     │  │
/// │  │    Data     │    │                     │   ├┤─────────────┤  │
/// │  │             │    │  ┌───────────────┐  │   ├┤ Chunk 2     │  │
/// │  │ "Complete   │    │  │ Input Buffer  │  │   ├┤─────────────┤  │
/// │  │  Payload"   │    │  │  (Stored)     │  │   ├┤ Chunk 3     │  │
/// │  └─────────────┘    │  └───────────────┘  │   ├┤─────────────┤  │
/// │                     │         │           │   └┤   ...       │  │
/// │                     │         ▼           │    └─────────────┘  │
/// │                     │  ┌───────────────┐  │                     │
/// │                     │  │Stream Output  │  │                     │
/// │                     │  │  (Generated   │  │                     │
/// │                     │  │   as stream)  │  │                     │
/// │                     │  └───────────────┘  │                     │
/// │                     └─────────────────────┘                     │
/// │                                                                 │
/// │  Flow: Single value in → Process → Stream out                   │
/// │  Memory: Request buffered, response streamed                    │
/// └─────────────────────────────────────────────────────────────────┘
/// ```
///
///
/// ## Streaming → Streaming
/// ```text
/// ┌─────────────────────────────────────────────────────────────────┐
/// │                  STREAMING → STREAMING                          │
/// ├─────────────────────────────────────────────────────────────────┤
/// │                                                                 │
/// │  ┌─────────────┐    ┌─────────────────────┐    ┌─────────────┐  │
/// │  │ Chunk 1     │───┐│     Exchange        │───┐│ Chunk A     │  │
/// │  ├─────────────┤   ││                     │   ├┤─────────────┤  │
/// │  │ Chunk 2     │───┤│  ┌───────────────┐  │   ├┤ Chunk B     │  │
/// │  ├─────────────┤   ├┼─▶│ Stream Input  │──┼───┤│─────────────│  │
/// │  │ Chunk 3     │───┤│  │               │  │   ├┤ Chunk C     │  │
/// │  ├─────────────┤   ││  │               │  │   ├┤─────────────┤  │
/// │  │   ...       │───┘│  └───────────────┘  │   └┤   ...       │  │
/// │  └─────────────┘    │         │           │    └─────────────┘  │
/// │                     │         ▼           │                     │
/// │                     │  ┌───────────────┐  │                     │
/// │                     │  │Stream Output  │  │                     │
/// │                     │  │ (Processed    │  │                     │
/// │                     │  │  streaming)   │  │                     │
/// │                     │  └───────────────┘  │                     │
/// │                     └─────────────────────┘                     │
/// │                                                                 │
/// │  Flow: Stream in → Process chunk by chunk → Stream out          │
/// │  Memory: Both request and response streamed (low memory)        │
/// └─────────────────────────────────────────────────────────────────┘
/// ```
pub struct Exchange<'a, I, O, M>
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    uuid: Uuid,
    metadata: Option<M>,
    input: Option<StreamOrValue<'a, I>>,
    output: Option<StreamOrValue<'a, O>>,
    input_listeners: Vec<Callback<I>>,
    output_listeners: Vec<Callback<O>>,
    attachments: Attachments,
}

impl<'a, I, O, M> Exchange<'a, I, O, M>
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    /// Creates a new Exchange instance with a randomly generated UUID.
    ///
    /// # Returns
    /// Returns a new `Exchange<'a, I, O, M>` instance with all fields initialized to their default values:
    /// - A new random UUID
    /// - No metadata set
    /// - No input or output data
    /// - Empty listener collections
    /// - Empty attachments
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Exchange;
    ///
    /// // Create a new exchange for String input, Vec<u8> output, and i32 metadata
    /// let mut exchange: Exchange<String, Vec<u8>, i32> = Exchange::new();
    /// println!("Exchange UUID: {}", exchange.uuid());
    /// ```
    ///
    /// # Behavior
    /// This function uses `Uuid::new_v4()` to generate a random UUID for the exchange instance.
    /// All other fields are initialized to empty/default states and can be populated later using
    /// the appropriate setter methods.
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

    /// Creates a new Exchange instance with a specific UUID.
    ///
    /// # Parameters
    /// - `uuid`: A `Uuid` that will be used as the unique identifier for this exchange instance.
    ///   This allows for creating exchanges with predetermined UUIDs, useful for testing or when
    ///   the UUID needs to be coordinated with external systems.
    ///
    /// # Returns
    /// Returns a new `Exchange<'a, I, O, M>` instance with the specified UUID and all other fields
    /// initialized to their default values.
    ///
    /// # Examples
    /// ```rust
    /// use uuid::Uuid;
    /// use idemio::exchange::Exchange;
    ///
    /// let specific_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    /// let mut exchange: Exchange<String, Vec<u8>, i32> = Exchange::new_with_uuid(specific_uuid);
    /// assert_eq!(exchange.uuid(), &specific_uuid);
    /// ```
    ///
    /// # Behavior
    /// Unlike `new()`, this function allows you to specify the exact UUID that will be used for
    /// the exchange. This is particularly useful in scenarios where you need reproducible UUIDs
    /// for testing or when integrating with systems that require specific UUID values.
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
    /// - `metadata`: A value of type `M` that represents the metadata to be associated with this exchange.
    ///   The metadata can be any type that implements `Send + Sync` and is typically used to store
    ///   contextual information about the exchange.
    ///
    /// # Behavior
    /// This method replaces any existing metadata. If metadata was previously set, it will be
    /// overwritten with the new value. The metadata becomes available for retrieval using the
    /// `metadata()` method.
    pub fn set_metadata(&mut self, metadata: M) {
        self.metadata = Some(metadata);
    }

    /// Retrieves a reference to the exchange's metadata.
    ///
    /// # Returns
    /// Returns `Result<&M, ExchangeError>` where:
    /// - `Ok(&M)` contains a reference to the metadata if it has been set
    /// - `Err(ExchangeError)` if no metadata has been set on this exchange
    ///
    /// # Behavior
    /// This method will return an `ExchangeError::metadata_read_error` if the metadata has not
    /// been previously set using `set_metadata()`. The error includes the exchange's UUID and
    /// a descriptive message.
    pub fn metadata(&self) -> Result<&M, ExchangeError> {
        match &self.metadata {
            None => Err(ExchangeError::metadata_read_error(
                &self.uuid,
                "Metadata has not been set",
            )),
            Some(metadata) => Ok(metadata),
        }
    }

    /// Returns an immutable reference to the exchange's attachments.
    ///
    /// # Returns
    /// Returns `&Attachments` - an immutable reference to the attachments collection associated
    /// with this exchange.
    ///
    /// # Behavior
    /// This method provides read-only access to the attachments. To modify attachments, use
    /// `attachments_mut()` instead.
    pub fn attachments(&self) -> &Attachments {
        &self.attachments
    }

    /// Returns a mutable reference to the exchange's attachments.
    ///
    /// # Returns
    /// Returns `&mut Attachments` - a mutable reference to the attachments collection associated
    /// with this exchange.
    ///
    /// # Behavior
    /// This method provides full read-write access to the attachments, allowing you to add,
    /// remove, or modify attachment data.
    pub fn attachments_mut(&mut self) -> &mut Attachments {
        &mut self.attachments
    }

    /// Adds a callback listener that will be invoked when input data is processed.
    ///
    /// # Parameters
    /// - `callback`: A closure that implements `FnMut(&mut I, &mut Attachments) + Send + Sync + 'static`.
    ///   The callback receives mutable references to the input data and attachments, allowing it to
    ///   modify both during input processing.
    ///
    /// # Examples
    /// ```rust
    ///
    /// use idemio::exchange::Exchange;
    /// let mut exchange: Exchange<String, Vec<u8>, i32> = Exchange::new();
    ///
    /// exchange.add_input_listener(|input, attachments| {
    ///     println!("Processing input: {}", input);
    ///     input.push_str(" - processed");
    /// });
    /// ```
    ///
    /// # Behavior
    /// Multiple input listeners can be added and they will be executed in the order they were added
    /// when input data is taken from the exchange. Each listener can modify both the input data
    /// and the attachments.
    pub fn add_input_listener(
        &mut self,
        callback: impl FnMut(&mut I, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.input_listeners.push(Callback::new(callback));
    }

    /// Adds a callback listener that will be invoked when output data is processed.
    ///
    /// # Parameters
    /// - `callback`: A closure that implements `FnMut(&mut O, &mut Attachments) + Send + Sync + 'static`.
    ///   The callback receives mutable references to the output data and attachments, allowing it to
    ///   modify both during output processing.
    ///
    /// # Examples
    /// ```rust
    ///
    /// use idemio::exchange::Exchange;
    ///
    /// let mut exchange: Exchange<String, Vec<u8>, i32> = Exchange::new();
    ///
    /// exchange.add_output_listener(|output, attachments| {
    ///     output.extend_from_slice(b" - output processed");
    /// });
    /// ```
    ///
    /// # Behavior
    /// Multiple output listeners can be added and they will be executed in the order they were added
    /// when output data is processed. Each listener can modify both the output data and the attachments.
    pub fn add_output_listener(
        &mut self,
        callback: impl FnMut(&mut O, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.output_listeners.push(Callback::new(callback));
    }

    /// Returns a reference to the exchange's unique identifier.
    ///
    /// # Returns
    /// Returns `&Uuid` - a reference to the UUID that uniquely identifies this exchange instance.
    ///
    /// # Behavior
    /// The UUID is set when the exchange is created (either randomly via `new()` or specifically
    /// via `new_with_uuid()`) and remains constant throughout the exchange's lifetime.
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Sets the input data as a buffered value.
    ///
    /// # Parameters
    /// - `input`: A value of type `I` that represents the input data for this exchange. The data
    ///   will be stored as a buffered value rather than a stream.
    ///
    /// # Examples
    /// ```rust
    ///
    /// use idemio::exchange::Exchange;
    /// let mut exchange: Exchange<String, Vec<u8>, i32> = Exchange::new();
    /// exchange.set_input("Hello, World!".to_string());
    /// ```
    ///
    /// # Behavior
    /// This method replaces any existing input (whether buffered or streamed) with the provided
    /// buffered value. The input becomes immediately available for retrieval without needing
    /// to await stream collection.
    pub fn set_input(&mut self, input: I) {
        self.input = Some(StreamOrValue::Value(input));
    }

    /// Saves the input data as a buffered value (alias for `set_input`).
    ///
    /// # Parameters
    /// - `input`: A value of type `I` that represents the input data to be saved in this exchange.
    ///
    /// # Examples
    /// ```rust
    ///
    /// use idemio::exchange::Exchange;
    ///
    /// let mut exchange: Exchange<String, Vec<u8>, i32> = Exchange::new();
    /// exchange.save_input("Data to save".to_string());
    /// ```
    ///
    /// # Behavior
    /// This method is functionally identical to `set_input()` and is provided as a semantic alias
    /// for cases where "saving" input data is more descriptive than "setting" it.
    pub fn save_input(&mut self, input: I) {
        self.set_input(input);
    }

    /// Sets the input as a stream of data.
    ///
    /// # Parameters
    /// - `stream`: A stream that implements `Stream<Item = StreamResult<I>> + Send + 'a`. The stream
    ///   will provide input data items asynchronously, each wrapped in a `StreamResult<I>` to handle
    ///   potential errors during streaming.
    ///
    /// # Behavior
    /// This method replaces any existing input with a stream. The stream will be consumed when
    /// the input is accessed, and a default collector will be used to aggregate the stream items
    /// into a single value.
    pub fn set_input_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = StreamResult<I>> + Send + 'a,
    {
        self.input = Some(StreamOrValue::from_stream(stream));
    }

    /// Sets the input as a stream with a custom collector.
    ///
    /// # Parameters
    /// - `stream`: A stream that implements `Stream<Item = StreamResult<I>> + Send + 'a` providing
    ///   the input data items.
    /// - `collector`: A boxed `StreamCollector<I>` that defines how the stream items should be
    ///   collected and aggregated into a single input value.
    ///
    /// # Examples
    /// See [VecCollector](crate::exchange::collector::VecCollector) for an example implementation.
    ///
    /// # Behavior
    /// This method allows for custom collection logic when processing the input stream. The
    /// provided collector will be used instead of the default collection behavior when the
    /// stream is consumed.
    pub fn set_input_stream_with_collector<S>(
        &mut self,
        stream: S,
        collector: Box<dyn StreamCollector<I> + 'a>,
    ) where
        S: Stream<Item = StreamResult<I>> + Send + 'a,
    {
        self.input = Some(StreamOrValue::from_stream_with_collector(stream, collector));
    }

    /// Processes an input stream using a custom processor function.
    ///
    /// # Parameters
    /// - `stream`: A stream that implements `Stream<Item = StreamResult<I>> + Send + 'a` providing
    ///   the stream of input items to be processed.
    /// - `processor`: A closure that takes a `Vec<I>` (all collected stream items) and returns
    ///   a `Result<I, ExchangeError>` representing the processed input value.
    ///
    /// # Returns
    /// Returns `Result<(), ExchangeError>` where:
    /// - `Ok(())` if the stream was successfully processed and the input was set
    /// - `Err(ExchangeError)` if there was an error during stream consumption or processing
    ///
    ///
    /// # Behavior
    /// This method consumes the entire stream, collecting all successful items into a vector.
    /// If any stream item results in an error, the processing stops and returns an error.
    /// The processor function is called with all collected items and its result becomes the
    /// input value for the exchange.
    pub async fn process_input_stream<S, F>(
        &mut self,
        stream: S,
        processor: F,
    ) -> Result<(), ExchangeError>
    where
        S: Stream<Item = StreamResult<I>> + Send + 'a,
        F: FnOnce(Vec<I>) -> Result<I, ExchangeError>,
    {
        let mut items = Vec::new();
        pin_mut!(stream);
        while let Some(result) = stream.next().await {
            match result {
                Ok(item) => items.push(item),
                Err(e) => {
                    return Err(ExchangeError::input_read_error(
                        &self.uuid,
                        &format!("Stream error: {}", e),
                    ));
                }
            }
        }
        let processed = processor(items)?;
        self.set_input(processed);
        Ok(())
    }

    /// Processes an input stream using a custom collector.
    ///
    /// # Parameters
    /// - `stream`: A stream that implements `Stream<Item = StreamResult<I>> + Send + 'a` providing
    ///   the stream of input items.
    /// - `collector`: A `StreamCollector<I>` that defines how to collect and process the stream items.
    ///
    /// # Returns
    /// Returns `Result<(), ExchangeError>` where:
    /// - `Ok(())` if the stream was successfully processed using the collector
    /// - `Err(ExchangeError)` if there was an error during stream processing or collection
    ///
    /// # Examples
    /// See [VecCollector](crate::exchange::collector::VecCollector) for an example implementation.
    ///
    /// # Behavior
    /// This is a convenience method that combines stream processing with collector-based aggregation.
    /// It first collects all stream items and then uses the provided collector to process them
    /// into the final input value.
    pub async fn process_input_stream_with_collector<S, C>(
        &mut self,
        stream: S,
        collector: C,
    ) -> Result<(), ExchangeError>
    where
        S: Stream<Item = StreamResult<I>> + Send + 'a,
        C: StreamCollector<I>,
    {
        let uuid = self.uuid.clone();
        self.process_input_stream(stream, |items| {
            collector
                .collect(items)
                .map_err(|e| ExchangeError::exchange_collector_error(&uuid, e))
        })
            .await
    }

    /// Retrieves a reference to the input data, collecting from stream if necessary.
    ///
    /// # Returns
    /// Returns `Result<&I, ExchangeError>` where:
    /// - `Ok(&I)` contains a reference to the input data
    /// - `Err(ExchangeError)` if no input is available or if stream collection fails
    ///
    /// # Behavior
    /// If the input is stored as a buffered value, it returns immediately. If the input is a stream,
    /// this method will collect the entire stream using the default collector and then return a
    /// reference to the collected value. The input remains available for future calls.
    pub async fn input(&mut self) -> Result<&I, ExchangeError> {
        match &mut self.input {
            Some(stream_or_value) => stream_or_value
                .get_value()
                .await
                .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e)),
            None => Err(ExchangeError::input_read_error(
                &self.uuid,
                "No input available",
            )),
        }
    }

    /// Retrieves a reference to the input data using a custom collector.
    ///
    /// # Parameters
    /// - `collector`: A `StreamCollector<I>` that defines how to collect stream data if the input
    ///   is stored as a stream.
    ///
    /// # Returns
    /// Returns `Result<&I, ExchangeError>` where:
    /// - `Ok(&I)` contains a reference to the input data
    /// - `Err(ExchangeError)` if no input is available or if collection fails
    ///
    /// # Examples
    /// See [VecCollector](crate::exchange::collector::VecCollector) for an example implementation.
    ///
    /// # Behavior
    /// This method is similar to `input()` but allows specifying a custom collector for stream
    /// processing. If the input is already a buffered value, the collector is ignored.
    pub async fn input_with_collector<C>(&mut self, collector: C) -> Result<&I, ExchangeError>
    where
        C: StreamCollector<I>,
    {
        match &mut self.input {
            Some(stream_or_value) => stream_or_value
                .get_value_with_collector(collector)
                .await
                .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e)),
            None => Err(ExchangeError::input_read_error(
                &self.uuid,
                "No input available",
            )),
        }
    }

    /// Takes ownership of the input data, removing it from the exchange.
    ///
    /// # Returns
    /// Returns `Result<I, ExchangeError>` where:
    /// - `Ok(I)` contains the owned input data
    /// - `Err(ExchangeError)` if no input is available to take or if collection fails
    ///
    /// # Examples
    /// ```rust
    ///
    ///  use idemio::exchange::Exchange;
    ///  async move {
    ///     let mut exchange: Exchange<String, Vec<u8>, i32> = Exchange::new();
    ///     exchange.set_input("data to take".to_string());
    ///     let owned_input = exchange.take_input().await?;
    ///     assert_eq!(owned_input, "data to take");
    ///     // Input is no longer available in the exchange
    ///     assert!(exchange.input().await.is_err());
    /// }
    /// ```
    ///
    /// # Behavior
    /// This method removes the input from the exchange and returns ownership to the caller.
    /// If the input is a stream, it will be collected first. All registered input listeners
    /// will be invoked with the input data before it is returned, allowing them to modify
    /// the data and attachments. After this call, no input will be available in the exchange.
    pub async fn take_input(&mut self) -> Result<I, ExchangeError> {
        match self.input.take() {
            Some(stream_or_value) => {
                let mut value = stream_or_value
                    .take_value()
                    .await
                    .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e))?;
                for mut callback in &mut self.input_listeners.drain(..) {
                    callback.invoke(&mut value, &mut self.attachments);
                }
                Ok(value)
            }
            None => Err(ExchangeError::input_take_error(
                &self.uuid,
                "No input available to take",
            )),
        }
    }

    /// Takes ownership of the input data using a custom collector.
    ///
    /// # Parameters
    /// - `collector`: A `StreamCollector<I>` that defines how to collect stream data if the input
    ///   is stored as a stream.
    ///
    /// # Returns
    /// Returns `Result<I, ExchangeError>` where:
    /// - `Ok(I)` contains the owned input data
    /// - `Err(ExchangeError)` if no input is available or if collection fails
    ///
    /// # Examples
    /// See [VecCollector](crate::exchange::collector::VecCollector) for an example of how to implement a custom collector.
    ///
    /// # Behavior
    /// Similar to `take_input()` but allows using a custom collector for stream processing.
    /// The input is removed from the exchange and input listeners are invoked before returning
    /// the owned data.
    pub async fn take_input_with_collector<C>(&mut self, collector: C) -> Result<I, ExchangeError>
    where
        C: StreamCollector<I>,
    {
        match self.input.take() {
            Some(stream_or_value) => {
                let mut value = stream_or_value
                    .take_value_with_collector(collector)
                    .await
                    .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e))?;
                for callback in &mut self.input_listeners {
                    callback.invoke(&mut value, &mut self.attachments);
                }
                Ok(value)
            }
            None => Err(ExchangeError::input_take_error(
                &self.uuid,
                "No input available to take",
            )),
        }
    }

    /// Takes ownership of the input as a stream, removing it from the exchange.
    ///
    /// # Returns
    /// Returns `Result<Pin<Box<dyn Stream<Item = StreamResult<I>> + Send + 'a>>, ExchangeError>` where:
    /// - `Ok(stream)` contains the owned input stream
    /// - `Err(ExchangeError)` if no input stream is available or if the input is not a stream
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::StreamExt;
    /// use idemio::exchange::Exchange;
    /// async move {
    ///     let mut exchange: Exchange<String, Vec<u8>, i32> = Exchange::new();
    ///
    ///     // Assume input was set as a stream
    ///     let mut input_stream = exchange.take_input_stream()?;
    ///     while let Some(item_result) = input_stream.next().await {
    ///         match item_result {
    ///             Ok(item) => println!("Stream item: {}", item),
    ///             Err(e) => eprintln!("Stream error: {}", e),
    ///         }
    ///     }
    /// }
    ///
    /// ```
    ///
    /// # Behavior
    /// This method is only applicable when the input was set as a stream. It removes the stream
    /// from the exchange and returns ownership to the caller, allowing direct stream processing.
    /// If the input was set as a buffered value, this method will return an error.
    pub fn take_input_stream(
        &mut self,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamResult<I>> + Send + 'a>>, ExchangeError> {
        match self.input.take() {
            Some(stream_or_value) => stream_or_value
                .take_stream()
                .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e)),
            None => Err(ExchangeError::input_take_error(
                &self.uuid,
                "No input stream available to take",
            )),
        }
    }


    /// Sets the output value for the exchange.
    ///
    /// # Parameters
    /// - `output`: The output value of type `O` to be stored in the exchange
    ///
    /// # Returns
    /// This function returns `()` (unit type).
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Exchange;
    ///
    /// let mut exchange: Exchange<String, i32, ()> = Exchange::new();
    /// exchange.set_output(42);
    /// ```
    ///
    /// # Behavior
    /// This method stores the provided output value as a buffered value in the exchange.
    /// Any previously set output (whether buffered or streaming) will be replaced.
    pub fn set_output(&mut self, output: O) {
        self.output = Some(StreamOrValue::Value(output));
    }


    /// Sets the output as a stream for the exchange.
    ///
    /// # Parameters
    /// - `stream`: A stream that implements `Stream<Item = StreamResult<O>> + Send + 'a`
    ///
    /// # Returns
    /// This function returns `()` (unit type).
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::Exchange;
    ///
    /// let mut exchange: Exchange<String, i32, ()> = Exchange::new();
    /// let output_stream = stream::iter(vec![Ok(1), Ok(2), Ok(3)]);
    /// exchange.set_output_stream(output_stream);
    /// ```
    ///
    /// # Behavior
    /// This method configures the exchange to use streaming output. The stream will be
    /// consumed when the output is accessed. Any previously set output will be replaced.
    pub fn set_output_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = StreamResult<O>> + Send + 'a,
    {
        self.output = Some(StreamOrValue::from_stream(stream));
    }


    /// Sets the output as a stream with a custom collector for the exchange.
    ///
    /// # Parameters
    /// - `stream`: A stream that implements `Stream<Item = StreamResult<O>> + Send + 'a`
    /// - `collector`: A boxed stream collector that implements `StreamCollector<O> + 'a`
    ///
    /// # Returns
    /// This function returns `()` (unit type).
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::VecCollector;
    /// use idemio::exchange::Exchange;
    ///
    /// let mut exchange: Exchange<String, Vec<i32>, ()> = Exchange::new();
    /// let output_stream = stream::iter(vec![Ok(1), Ok(2), Ok(3)]);
    /// let collector = Box::new(VecCollector::new());
    /// exchange.set_output_stream_with_collector(output_stream, collector);
    /// ```
    ///
    /// # Behavior
    /// This method configures the exchange to use streaming output with a specific collector.
    /// The collector will be used to aggregate stream items when the output is accessed as a value.
    /// Any previously set output will be replaced.
    pub fn set_output_stream_with_collector<S>(
        &mut self,
        stream: S,
        collector: Box<dyn StreamCollector<O> + 'a>,
    ) where
        S: Stream<Item = StreamResult<O>> + Send + 'a,
    {
        self.output = Some(StreamOrValue::from_stream_with_collector(stream, collector));
    }


    /// Processes an output stream using a provided processor function and stores the result.
    ///
    /// # Parameters
    /// - `stream`: A stream that implements `Stream<Item = StreamResult<O>> + Send + 'a`
    /// - `processor`: A function that takes `Vec<O>` and returns `Result<O, ExchangeError>`
    ///
    /// # Returns
    /// Returns `Result<(), ExchangeError>` where:
    /// - `Ok(())` indicates successful processing and storage of the result
    /// - `Err(ExchangeError)` if stream processing fails or the processor function returns an error
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::{Exchange, ExchangeError};
    ///
    /// async {
    ///     let mut exchange: Exchange<String, i32, ()> = Exchange::new();
    ///     let output_stream = stream::iter(vec![Ok(1), Ok(2), Ok(3)]);
    ///     
    ///     let result = exchange.process_output_stream(output_stream, |items| {
    ///         Ok(items.iter().sum())
    ///     }).await;
    ///     
    ///     assert!(result.is_ok());
    /// }
    /// ```
    ///
    /// # Behavior
    /// This method consumes the entire stream, collecting all successful items into a vector.
    /// If any stream item contains an error, the method returns immediately with an error.
    /// The processor function is called with the collected items and its result is stored as the output.
    pub async fn process_output_stream<S, F>(
        &mut self,
        stream: S,
        processor: F,
    ) -> Result<(), ExchangeError>
    where
        S: Stream<Item = StreamResult<O>> + Send + 'a,
        F: FnOnce(Vec<O>) -> Result<O, ExchangeError>,
    {
        let mut items = Vec::new();
        pin_mut!(stream);
        while let Some(result) = stream.next().await {
            match result {
                Ok(item) => items.push(item),
                Err(e) => {
                    return Err(ExchangeError::output_read_error(
                        &self.uuid,
                        &format!("Stream error: {}", e),
                    ));
                }
            }
        }
        let processed = processor(items)?;
        self.set_output(processed);
        Ok(())
    }


    /// Processes an output stream using a provided collector and stores the result.
    ///
    /// # Parameters
    /// - `stream`: A stream that implements `Stream<Item = StreamResult<O>> + Send + 'a`
    /// - `collector`: A collector that implements `StreamCollector<O>`
    ///
    /// # Returns
    /// Returns `Result<(), ExchangeError>` where:
    /// - `Ok(())` indicates successful processing and storage of the result
    /// - `Err(ExchangeError)` if stream processing fails or the collector returns an error
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::VecCollector;
    /// use idemio::exchange::Exchange;
    ///
    /// async {
    ///     let mut exchange: Exchange<String, Vec<i32>, ()> = Exchange::new();
    ///     let output_stream = stream::iter(vec![Ok(1), Ok(2), Ok(3)]);
    ///     let result = exchange.process_output_stream_with_collector(output_stream, VecCollector::new()).await;
    ///     assert!(result.is_ok());
    /// }
    /// ```
    ///
    /// # Behavior
    /// This method is a convenience wrapper around `process_output_stream` that uses a collector
    /// to process the stream items. The collector's `collect` method is called with all
    /// successfully collected items from the stream.
    pub async fn process_output_stream_with_collector<S, C>(
        &mut self,
        stream: S,
        collector: C,
    ) -> Result<(), ExchangeError>
    where
        S: Stream<Item = StreamResult<O>> + Send + 'a,
        C: StreamCollector<O>,
    {
        let uuid = self.uuid.clone();
        self.process_output_stream(stream, |items| {
            collector
                .collect(items)
                .map_err(|e| ExchangeError::exchange_collector_error(&uuid, e))
        })
            .await
    }


    /// Gets a reference to the output value, collecting from stream if necessary.
    ///
    /// # Returns
    /// Returns `Result<&O, ExchangeError>` where:
    /// - `Ok(&O)` contains a reference to the output value
    /// - `Err(ExchangeError)` if no output is available or stream collection fails
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Exchange;
    ///
    /// async {
    ///     let mut exchange: Exchange<String, i32, ()> = Exchange::new();
    ///     exchange.set_output(42);
    ///     
    ///     let output = exchange.output().await?;
    ///     assert_eq!(*output, 42);
    /// }
    /// ```
    ///
    /// # Behavior
    /// If the output is stored as a buffered value, returns a reference immediately.
    /// If the output is a stream, it will be collected using the associated collector (if available).
    /// The method will return an error if no output is set or if stream collection fails without a collector.
    pub async fn output(&mut self) -> Result<&O, ExchangeError> {
        match &mut self.output {
            Some(stream_or_value) => stream_or_value
                .get_value()
                .await
                .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e)),
            None => Err(ExchangeError::output_read_error(
                &self.uuid,
                "No output available",
            )),
        }
    }



    /// Gets a reference to the output value using a specific collector for stream processing.
    ///
    /// # Parameters
    /// - `collector`: A collector that implements `StreamCollector<O>` for processing streams
    ///
    /// # Returns
    /// Returns `Result<&O, ExchangeError>` where:
    /// - `Ok(&O)` contains a reference to the output value
    /// - `Err(ExchangeError)` if no output is available or collection fails
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::VecCollector;
    /// use idemio::exchange::Exchange;
    ///
    /// async {
    ///     let mut exchange: Exchange<String, Vec<i32>, ()> = Exchange::new();
    ///     let output_stream = stream::iter(vec![Ok(1), Ok(2), Ok(3)]);
    ///     exchange.set_output_stream(output_stream);
    ///     
    ///     let collector = VecCollector::new();
    ///     let output = exchange.output_with_collector(collector).await?;
    ///     assert_eq!(*output, vec![1, 2, 3]);
    /// }
    /// ```
    ///
    /// # Behavior
    /// This method allows specifying a custom collector for stream processing, overriding
    /// any collector that may have been set when the stream was created. For buffered outputs,
    /// the collector parameter is ignored and the value is returned directly.
    pub async fn output_with_collector<C>(&mut self, collector: C) -> Result<&O, ExchangeError>
    where
        C: StreamCollector<O>,
    {
        match &mut self.output {
            Some(stream_or_value) => stream_or_value
                .get_value_with_collector(collector)
                .await
                .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e)),
            None => Err(ExchangeError::output_read_error(
                &self.uuid,
                "No output available",
            )),
        }
    }



    /// Takes ownership of the output value, removing it from the exchange and executing callbacks.
    ///
    /// # Returns
    /// Returns `Result<O, ExchangeError>` where:
    /// - `Ok(O)` contains the owned output value
    /// - `Err(ExchangeError)` if no output is available or collection fails
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Exchange;
    ///
    /// async {
    ///     let mut exchange: Exchange<String, i32, ()> = Exchange::new();
    ///     exchange.set_output(42);
    ///     
    ///     let output = exchange.take_output().await?;
    ///     assert_eq!(output, 42);
    ///     
    ///     // Output is no longer available in the exchange
    ///     assert!(exchange.take_output().await.is_err());
    /// }
    /// ```
    ///
    /// # Behavior
    /// This method removes the output from the exchange and returns ownership to the caller.
    /// If the output is a stream, it will be collected first. After taking the value,
    /// all registered output listeners are executed with the value and attachments.
    /// Subsequent calls will return an error until a new output is set.
    pub async fn take_output(&mut self) -> Result<O, ExchangeError> {
        match self.output.take() {
            Some(stream_or_value) => {
                let mut value = stream_or_value
                    .take_value()
                    .await
                    .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e))?;
                // Execute callbacks
                for mut callback in &mut self.output_listeners.drain(..) {
                    callback.invoke(&mut value, &mut self.attachments);
                }
                Ok(value)
            }
            None => Err(ExchangeError::output_take_error(
                &self.uuid,
                "No output available to take",
            )),
        }
    }


    /// Takes ownership of the output value using a specific collector, removing it from the exchange.
    ///
    /// # Parameters
    /// - `collector`: A collector that implements `StreamCollector<O>` for processing streams
    ///
    /// # Returns
    /// Returns `Result<O, ExchangeError>` where:
    /// - `Ok(O)` contains the owned output value
    /// - `Err(ExchangeError)` if no output is available or collection fails
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::VecCollector;
    /// use idemio::exchange::Exchange;
    ///
    /// async {
    ///     let mut exchange: Exchange<String, Vec<i32>, ()> = Exchange::new();
    ///     let output_stream = stream::iter(vec![Ok(1), Ok(2), Ok(3)]);
    ///     exchange.set_output_stream(output_stream);
    ///     
    ///     let collector = VecCollector::new();
    ///     let output = exchange.take_output_with_collector(collector).await?;
    ///     assert_eq!(output, vec![1, 2, 3]);
    /// }
    /// ```
    ///
    /// # Behavior
    /// Similar to `take_output`, but allows specifying a custom collector for stream processing.
    /// The output is removed from the exchange, output listeners are executed, and ownership
    /// is transferred to the caller. For buffered outputs, the collector is ignored.
    pub async fn take_output_with_collector<C>(&mut self, collector: C) -> Result<O, ExchangeError>
    where
        C: StreamCollector<O>,
    {
        match self.output.take() {
            Some(stream_or_value) => {
                let mut value = stream_or_value
                    .take_value_with_collector(collector)
                    .await
                    .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e))?;
                // Execute callbacks
                for mut callback in &mut self.output_listeners.drain(..) {
                    callback.invoke(&mut value, &mut self.attachments);
                }
                Ok(value)
            }
            None => Err(ExchangeError::output_take_error(
                &self.uuid,
                "No output available to take",
            )),
        }
    }


    /// Takes ownership of the output as a stream, removing it from the exchange.
    ///
    /// # Returns
    /// Returns `Result<Pin<Box<dyn Stream<Item = StreamResult<O>> + Send + 'a>>, ExchangeError>` where:
    /// - `Ok(stream)` contains the owned output stream
    /// - `Err(ExchangeError)` if no output stream is available or if the output is not a stream
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::{stream, StreamExt};
    /// use idemio::exchange::Exchange;
    ///
    /// async {
    ///     let mut exchange: Exchange<String, i32, ()> = Exchange::new();
    ///     let output_stream = stream::iter(vec![Ok(1), Ok(2), Ok(3)]);
    ///     exchange.set_output_stream(output_stream);
    ///     
    ///     let mut taken_stream = exchange.take_output_stream()?;
    ///     while let Some(item_result) = taken_stream.next().await {
    ///         match item_result {
    ///             Ok(item) => println!("Stream item: {}", item),
    ///             Err(e) => eprintln!("Stream error: {}", e),
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// # Behavior
    /// This method is only applicable when the output was set as a stream. It removes the stream
    /// from the exchange and returns ownership to the caller, allowing direct stream processing.
    /// If the output was set as a buffered value, this method will return an error.
    pub fn take_output_stream(
        &mut self,
    ) -> Result<
        Pin<
            Box<dyn Stream<Item = StreamResult<O>> + Send + 'a>,
        >,
        ExchangeError,
    > {
        match self.output.take() {
            Some(stream_or_value) => stream_or_value
                .take_stream()
                .map_err(|e| ExchangeError::exchange_collector_error(&self.uuid, e)),
            None => Err(ExchangeError::output_take_error(
                &self.uuid,
                "No output stream available to take",
            )),
        }
    }


    /// Checks if the exchange has input data available.
    ///
    /// # Returns
    /// Returns `bool` where:
    /// - `true` if input data is available (either buffered or streaming)
    /// - `false` if no input data is set
    ///
    /// # Behavior
    /// This is a non-destructive check that returns whether input data has been set.
    /// It does not distinguish between buffered and streaming input types.
    pub fn has_input(&self) -> bool {
        self.input.is_some()
    }


    /// Checks if the exchange has output data available.
    ///
    /// # Returns
    /// Returns `bool` where:
    /// - `true` if output data is available (either buffered or streaming)
    /// - `false` if no output data is set
    ///
    /// # Behavior
    /// This is a non-destructive check that returns whether output data has been set.
    /// It doesn't distinguish between buffered and streaming output types.
    pub fn has_output(&self) -> bool {
        self.output.is_some()
    }

}




pub enum InputConfig<'a, I>
where
    I: Send + Sync,
{
    None,
    Buffered(I),
    Streaming(
        Pin<
            Box<dyn Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>,
        >,
    ),
    StreamingWithCollector {
        stream: Pin<
            Box<dyn Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>,
        >,
        collector: Box<dyn StreamCollector<I> + 'a>,
    },
}

pub enum OutputConfig {
    None,
    Buffered,
    Streaming,
}

pub struct ExchangeBuilder<'a, I, O, M>
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    uuid: Option<Uuid>,
    phantom: PhantomData<O>,
    metadata: Option<M>,
    input_config: InputConfig<'a, I>,
    output_config: OutputConfig,
}

impl<'a, I, O, M> ExchangeBuilder<'a, I, O, M>
where
    I: Send + Sync + 'a,
    O: Send + Sync,
    M: Send + Sync,
{
    /// Creates a new ExchangeBuilder instance.
    ///
    /// # Returns
    /// Returns a new `Self` instance with default configuration.
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let builder: ExchangeBuilder<String, i32, ()> = ExchangeBuilder::new();
    /// ```
    ///
    /// # Behavior
    /// Initializes a builder with no UUID, metadata, or input/output configuration.
    /// All configurations must be set before calling `build()`.
    pub fn new() -> Self {
        Self {
            uuid: None,
            phantom: PhantomData::default(),
            metadata: None,
            input_config: InputConfig::None,
            output_config: OutputConfig::None,
        }
    }

    /// Sets a custom UUID for the exchange being built.
    ///
    /// # Parameters
    /// - `uuid`: A `Uuid` value to be used as the exchange identifier
    ///
    /// # Returns
    /// Returns `Self` with the UUID configuration applied.
    ///
    /// # Behavior
    /// If not set, the exchange will generate a random UUID when built.
    /// This method allows specifying a custom UUID for tracking or correlation purposes.
    pub fn with_uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = Some(uuid);
        self
    }


    /// Sets metadata for the exchange being built.
    ///
    /// # Parameters
    /// - `metadata`: Metadata of type `M` to be associated with the exchange
    ///
    /// # Returns
    /// Returns `Self` with the metadata configuration applied.
    ///
    /// # Behavior
    /// The metadata can be any type that implements `Send + Sync` and will be stored
    /// with the exchange for later retrieval.
    pub fn with_metadata(mut self, metadata: M) -> Self {
        self.metadata = Some(metadata);
        self
    }


    /// Configures the exchange to use buffered input.
    ///
    /// # Parameters
    /// - `input`: The input value of type `I` to be stored as buffered input
    ///
    /// # Returns
    /// Returns `Self` with the buffered input configuration applied.
    ///
    /// # Behavior
    /// The input value will be immediately available for processing without streaming.
    /// This replaces any previously configured input type.
    pub fn with_buffered_input(mut self, input: I) -> Self {
        self.input_config = InputConfig::Buffered(input);
        self
    }


    /// Configures the exchange to use streaming input.
    ///
    /// # Parameters
    /// - `stream`: A stream implementing `Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a`
    ///
    /// # Returns
    /// Returns `Self` with the streaming input configuration applied.
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let input_stream = stream::iter(vec![Ok("item1".to_string()), Ok("item2".to_string())]);
    /// let builder: ExchangeBuilder<String, i32, ()> = ExchangeBuilder::new()
    ///     .with_streaming_input(input_stream);
    /// ```
    ///
    /// # Behavior
    /// The stream will be consumed when input processing occurs. This replaces any
    /// previously configured input type.
    pub fn with_streaming_input<S>(mut self, stream: S) -> Self
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        self.input_config = InputConfig::Streaming(Box::pin(stream));
        self
    }


    /// Configures the exchange to use streaming input with a custom collector.
    ///
    /// # Parameters
    /// - `stream`: A stream implementing `Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a`
    /// - `collector`: A boxed collector implementing `StreamCollector<I> + 'a`
    ///
    /// # Returns
    /// Returns `Self` with the streaming input and collector configuration applied.
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::VecCollector;
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let input_stream = stream::iter(vec![Ok("item1".to_string()), Ok("item2".to_string())]);
    /// let collector = Box::new(VecCollector::new());
    /// let builder: ExchangeBuilder<String, i32, ()> = ExchangeBuilder::new()
    ///     .with_streaming_input_and_collector(input_stream, collector);
    /// ```
    ///
    /// # Behavior
    /// The collector will be used to aggregate stream items when input is accessed as a value.
    /// This replaces any previously configured input type.
    pub fn with_streaming_input_and_collector<S>(
        mut self,
        stream: S,
        collector: Box<dyn StreamCollector<I> + 'a>,
    ) -> Self
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        self.input_config = InputConfig::StreamingWithCollector {
            stream: Box::pin(stream),
            collector,
        };
        self
    }


    /// Configures the exchange to support buffered output.
    ///
    /// # Returns
    /// Returns `Self` with buffered output configuration applied.
    ///
    /// # Behavior
    /// The exchange will be configured to store output as buffered values rather than streams.
    /// This replaces any previously configured output type.
    pub fn with_buffered_output(mut self) -> Self {
        self.output_config = OutputConfig::Buffered;
        self
    }


    /// Configures the exchange to support streaming output.
    ///
    /// # Returns
    /// Returns `Self` with streaming output configuration applied.
    ///
    /// # Behavior
    /// The exchange will be configured to handle output as streams rather than buffered values.
    /// This replaces any previously configured output type.
    pub fn with_streaming_output(mut self) -> Self {
        self.output_config = OutputConfig::Streaming;
        self
    }


    /// Builds the configured exchange instance.
    ///
    /// # Returns
    /// Returns `Result<Exchange<'a, I, O, M>, ExchangeBuilderError>` where:
    /// - `Ok(Exchange)` contains the configured exchange instance
    /// - `Err(ExchangeBuilderError)` if the configuration is invalid
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let exchange = ExchangeBuilder::new()
    ///     .with_buffered_input("test".to_string())
    ///     .with_buffered_output()
    ///     .build()?;
    /// ```
    ///
    /// # Behavior
    /// Validates the configuration and creates an Exchange instance. Returns an error if
    /// no input configuration is provided. The UUID will be auto-generated if not specified.
    pub fn build(self) -> Result<Exchange<'a, I, O, M>, ExchangeBuilderError> {
        let mut exchange = if let Some(uuid) = self.uuid {
            Exchange::new_with_uuid(uuid)
        } else {
            Exchange::new()
        };

        if let Some(metadata) = self.metadata {
            exchange.set_metadata(metadata);
        }

        // Configure input
        match self.input_config {
            InputConfig::None => {
                return Err(ExchangeBuilderError::invalid_input_config(
                    "No input type defined for exchange.",
                ));
            }
            InputConfig::Buffered(input) => {
                exchange.set_input(input);
            }
            InputConfig::Streaming(stream) => {
                exchange.set_input_stream(stream);
            }
            InputConfig::StreamingWithCollector { stream, collector } => {
                exchange.set_input_stream_with_collector(stream, collector);
            }
        }

        Ok(exchange)
    }
}

#[derive(Debug)]
pub enum ExchangeBuilderError {
    InvalidInputConfig(String),
    InvalidOutputConfig(String),
}

impl ExchangeBuilderError {

    /// Creates an error for invalid input configuration.
    pub fn invalid_input_config(msg: impl Into<String>) -> Self {
        ExchangeBuilderError::InvalidInputConfig(msg.into())
    }


    /// Creates an error for invalid output configuration.
    pub fn invalid_output_config(msg: impl Into<String>) -> Self {
        ExchangeBuilderError::InvalidOutputConfig(msg.into())
    }
}

impl Display for ExchangeBuilderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExchangeBuilderError::InvalidInputConfig(msg) => {
                write!(f, "Invalid input configuration: {}", msg)
            }
            ExchangeBuilderError::InvalidOutputConfig(msg) => {
                write!(f, "Invalid output configuration: {}", msg)
            }
        }
    }
}

impl std::error::Error for ExchangeBuilderError {}

impl<'a, I, O, M> ExchangeBuilder<'a, I, O, M>
where
    I: Send + Sync + 'a,
    O: Send + Sync,
    M: Send + Sync,
{


    /// Creates an exchange with buffered input and output.
    ///
    /// # Parameters
    /// - `input`: The input value of type `I` to be stored as buffered input
    ///
    /// # Returns
    /// Returns `Result<Exchange<'a, I, O, M>, ExchangeBuilderError>` where:
    /// - `Ok(Exchange)` contains the configured exchange with buffered I/O
    /// - `Err(ExchangeBuilderError)` if the build process fails
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let exchange = ExchangeBuilder::buffered("test input".to_string())?;
    /// ```
    ///
    /// # Behavior
    /// This is a convenience method that creates an exchange with both input and output
    /// configured for buffered operation. Equivalent to calling `new().with_buffered_input(input).with_buffered_output().build()`.
    pub fn buffered(input: I) -> Result<Exchange<'a, I, O, M>, ExchangeBuilderError> {
        Self::new()
            .with_buffered_input(input)
            .with_buffered_output()
            .build()
    }


    /// Creates an exchange with streaming input and buffered output.
    ///
    /// # Parameters
    /// - `input_stream`: A stream implementing `Stream<Item = StreamResult<I>> + Send + 'a`
    ///
    /// # Returns
    /// Returns `Result<Exchange<'a, I, O, M>, ExchangeBuilderError>` where:
    /// - `Ok(Exchange)` contains the configured exchange
    /// - `Err(ExchangeBuilderError)` if the build process fails
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let input_stream = stream::iter(vec![Ok("item1".to_string()), Ok("item2".to_string())]);
    /// let exchange = ExchangeBuilder::streaming_request_buffered_response(input_stream)?;
    /// ```
    ///
    /// # Behavior
    /// This convenience method creates an exchange that processes streaming input but
    /// produces buffered output. Useful for request-response patterns where input arrives
    /// as a stream but output is a single value.
    pub fn streaming_request_buffered_response<S>(
        input_stream: S,
    ) -> Result<Exchange<'a, I, O, M>, ExchangeBuilderError>
    where
        S: Stream<Item = StreamResult<I>> + Send + 'a,
    {
        Self::new()
            .with_streaming_input(input_stream)
            .with_buffered_output()
            .build()
    }


    /// Creates an exchange with streaming input, collector, and buffered output.
    ///
    /// # Parameters
    /// - `input_stream`: A stream implementing `Stream<Item = StreamResult<I>> + Send + 'a`
    /// - `input_collector`: A boxed collector implementing `StreamCollector<I> + 'a`
    ///
    /// # Returns
    /// Returns `Result<Exchange<'a, I, O, M>, ExchangeBuilderError>` where:
    /// - `Ok(Exchange)` contains the configured exchange
    /// - `Err(ExchangeBuilderError)` if the build process fails
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::VecCollector;
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let input_stream = stream::iter(vec![Ok("item1".to_string()), Ok("item2".to_string())]);
    /// let collector = Box::new(VecCollector::new());
    /// let exchange = ExchangeBuilder::streaming_request_buffered_response_with_collector(input_stream, collector)?;
    /// ```
    ///
    /// # Behavior
    /// Similar to `streaming_request_buffered_response` but allows specifying a custom
    /// collector for aggregating the input stream items.
    pub fn streaming_request_buffered_response_with_collector<S>(
        input_stream: S,
        input_collector: Box<dyn StreamCollector<I> + 'a>,
    ) -> Result<Exchange<'a, I, O, M>, ExchangeBuilderError>
    where
        S: Stream<Item = StreamResult<I>> + Send + 'a,
    {
        Self::new()
            .with_streaming_input_and_collector(input_stream, input_collector)
            .with_buffered_output()
            .build()
    }


    /// Creates an exchange with buffered input and streaming output.
    ///
    /// # Parameters
    /// - `input`: The input value of type `I` to be stored as buffered input
    ///
    /// # Returns
    /// Returns `Result<Exchange<'a, I, O, M>, ExchangeBuilderError>` where:
    /// - `Ok(Exchange)` contains the configured exchange
    /// - `Err(ExchangeBuilderError)` if the build process fails
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let exchange = ExchangeBuilder::buffered_request_streaming_response("test input".to_string())?;
    /// ```
    ///
    /// # Behavior
    /// This convenience method creates an exchange that processes buffered input but
    /// produces streaming output. Useful for patterns where a single request produces
    /// a stream of response items.
    pub fn buffered_request_streaming_response<S>(
        input: I,
    ) -> Result<Exchange<'a, I, O, M>, ExchangeBuilderError> {
        Self::new()
            .with_buffered_input(input)
            .with_streaming_output()
            .build()
    }


    /// Creates an exchange with buffered input and streaming output (with collector support).
    ///
    /// # Parameters
    /// - `input`: The input value of type `I` to be stored as buffered input
    ///
    /// # Returns
    /// Returns `Result<Exchange<'a, I, O, M>, ExchangeBuilderError>` where:
    /// - `Ok(Exchange)` contains the configured exchange
    /// - `Err(ExchangeError)` if the build process fails
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let exchange = ExchangeBuilder::buffered_request_streaming_response_with_collector("test input".to_string())?;
    /// ```
    ///
    /// # Behavior
    /// Similar to `buffered_request_streaming_response` but configured to support
    /// collectors for output stream processing. The generic parameter `S` is not used
    /// in the current implementation.
    pub fn buffered_request_streaming_response_with_collector<S>(
        input: I,
    ) -> Result<Exchange<'a, I, O, M>, ExchangeBuilderError> {
        Self::new()
            .with_buffered_input(input)
            .with_streaming_output()
            .build()
    }


    /// Creates an exchange with streaming input and streaming output.
    ///
    /// # Parameters
    /// - `input_stream`: A stream implementing `Stream<Item = StreamResult<I>> + Send + 'a`
    ///
    /// # Returns
    /// Returns `Result<Exchange<'a, I, O, M>, ExchangeBuilderError>` where:
    /// - `Ok(Exchange)` contains the configured exchange
    /// - `Err(ExchangeBuilderError)` if the build process fails
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let input_stream = stream::iter(vec![Ok("item1".to_string()), Ok("item2".to_string())]);
    /// let exchange = ExchangeBuilder::streaming(input_stream)?;
    /// ```
    ///
    /// # Behavior
    /// This convenience method creates an exchange configured for full streaming operation,
    /// where both input and output are handled as streams.
    pub fn streaming<S>(input_stream: S) -> Result<Exchange<'a, I, O, M>, ExchangeBuilderError>
    where
        S: Stream<Item = StreamResult<I>> + Send + 'a,
    {
        Self::new()
            .with_streaming_input(input_stream)
            .with_streaming_output()
            .build()
    }


    /// Creates an exchange with streaming input/output and input collector.
    ///
    /// # Parameters
    /// - `input_stream`: A stream implementing `Stream<Item = StreamResult<I>> + Send + 'a`
    /// - `input_collector`: A boxed collector implementing `StreamCollector<I> + 'a`
    ///
    /// # Returns
    /// Returns `Result<Exchange<'a, I, O, M>, ExchangeBuilderError>` where:
    /// - `Ok(Exchange)` contains the configured exchange
    /// - `Err(ExchangeBuilderError)` if the build process fails
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::VecCollector;
    /// use idemio::exchange::ExchangeBuilder;
    ///
    /// let input_stream = stream::iter(vec![Ok("item1".to_string()), Ok("item2".to_string())]);
    /// let collector = Box::new(VecCollector::new());
    /// let exchange = ExchangeBuilder::streaming_with_collectors(input_stream, collector)?;
    /// ```
    ///
    /// # Behavior
    /// Creates a fully streaming exchange with a custom collector for input stream processing.
    /// Both input and output are configured as streams.
    pub fn streaming_with_collectors<S>(
        input_stream: S,
        input_collector: Box<dyn StreamCollector<I> + 'a>,
    ) -> Result<Exchange<'a, I, O, M>, ExchangeBuilderError>
    where
        S: Stream<Item = StreamResult<I>> + Send + 'a,
    {
        Self::new()
            .with_streaming_input_and_collector(input_stream, input_collector)
            .with_streaming_output()
            .build()
    }

}

pub struct Attachments {
    attachments: HashMap<AttachmentKey, Box<dyn Any + Send + Sync>, fnv::FnvBuildHasher>,
}

impl Attachments {

    /// Creates a new empty attachments collection.
    ///
    /// # Returns
    /// Returns a new `Self` instance with an empty HashMap using FNV hasher.
    ///
    /// # Behavior
    /// Initializes an empty collection optimized for storing typed key-value pairs
    /// using FNV hashing for performance.
    pub fn new() -> Self {
        Self {
            attachments: HashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    /// Adds a typed value to the attachments collection.
    ///
    /// # Parameters
    /// - `key`: A string-like key that can be converted via `AsRef<str>`
    /// - `value`: A value of type `K` that implements `Send + Sync + 'static`
    ///
    /// # Returns
    /// This function returns `()` (unit type).
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Attachments;
    ///
    /// let mut attachments = Attachments::new();
    /// attachments.add("user_id", 123u32);
    /// attachments.add("username", "alice".to_string());
    /// ```
    ///
    /// # Behavior
    /// The value is stored with a composite key that includes both the string key and
    /// the type ID. This allows storing multiple values with the same string key but
    /// different types. Previous values with the same key and type will be replaced.
    pub fn add<K>(&mut self, key: impl AsRef<str>, value: K)
    where
        K: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<K>();
        self.attachments
            .insert(AttachmentKey::new(key, type_id), Box::new(value));
    }


    /// Retrieves a reference to a typed value from the attachments collection.
    ///
    /// # Parameters
    /// - `key`: A string-like key that can be converted via `AsRef<str>`
    ///
    /// # Returns
    /// Returns `Option<&K>` where:
    /// - `Some(&K)` contains a reference to the stored value of type `K`
    /// - `None` if no value exists for the given key and type combination
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Attachments;
    ///
    /// let mut attachments = Attachments::new();
    /// attachments.add("user_id", 123u32);
    ///
    /// let user_id: Option<&u32> = attachments.get("user_id");
    /// assert_eq!(user_id, Some(&123));
    /// ```
    ///
    /// # Behavior
    /// The lookup is performed using both the string key and the type ID of `K`.
    /// The method will only return a value if both the key exists and the stored
    /// value matches the requested type exactly.
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


    /// Retrieves a mutable reference to a typed value from the attachments collection.
    ///
    /// # Parameters
    /// - `key`: A string-like key that can be converted via `AsRef<str>`
    ///
    /// # Returns
    /// Returns `Option<&mut K>` where:
    /// - `Some(&mut K)` contains a mutable reference to the stored value of type `K`
    /// - `None` if no value exists for the given key and type combination
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::Attachments;
    ///
    /// let mut attachments = Attachments::new();
    /// attachments.add("counter", 0u32);
    ///
    /// if let Some(counter) = attachments.get_mut::<u32>("counter") {
    ///     *counter += 1;
    /// }
    ///
    /// assert_eq!(attachments.get::<u32>("counter"), Some(&1));
    /// ```
    ///
    /// # Behavior
    /// Similar to `get` but returns a mutable reference, allowing modification of the
    /// stored value. The lookup requires both key and type to match exactly.
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

#[derive(Debug)]
pub enum ExchangeError {
    ExchangeCompleted(Uuid),
    ExchangeCollectorError(Uuid, CollectorError),
    MetadataReadError(Uuid, String),
    InputReadError(Uuid, String),
    InputTakeError(Uuid, String),
    OutputReadError(Uuid, String),
    OutputTakeError(Uuid, String),
    InputCallbackError(Uuid, String),
    OutputCallbackError(Uuid, String),
}

impl ExchangeError {
    pub fn exchange_collector_error(uuid: &Uuid, err: CollectorError) -> Self {
        ExchangeError::ExchangeCollectorError(*uuid, err)
    }

    pub fn exchange_completed(uuid: &Uuid) -> Self {
        ExchangeError::ExchangeCompleted(*uuid)
    }

    pub fn metadata_read_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::MetadataReadError(*uuid, msg.into())
    }

    pub fn input_read_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::InputReadError(*uuid, msg.into())
    }

    pub fn input_take_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::InputTakeError(*uuid, msg.into())
    }

    pub fn output_read_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::OutputReadError(*uuid, msg.into())
    }

    pub fn output_take_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::OutputTakeError(*uuid, msg.into())
    }

    pub fn input_callback_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::InputCallbackError(*uuid, msg.into())
    }

    pub fn output_callback_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::OutputCallbackError(*uuid, msg.into())
    }
}

impl Display for ExchangeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExchangeError::ExchangeCompleted(uuid) => {
                write!(f, "{} Exchange has already been completed", uuid)
            }
            ExchangeError::MetadataReadError(uuid, msg) => {
                write!(f, "{} Failed to read metadata: {}", uuid, msg)
            }
            ExchangeError::InputReadError(uuid, msg) => {
                write!(f, "{} Failed to read input: {}", uuid, msg)
            }
            ExchangeError::InputTakeError(uuid, msg) => {
                write!(f, "{} Failed to consume input: {}", uuid, msg)
            }
            ExchangeError::OutputReadError(uuid, msg) => {
                write!(f, "{} Failed to read output: {}", uuid, msg)
            }
            ExchangeError::OutputTakeError(uuid, msg) => {
                write!(f, "{} Failed to consume output: {}", uuid, msg)
            }
            ExchangeError::InputCallbackError(uuid, msg) => {
                write!(f, "{} Failed to invoke input callback: {}", uuid, msg)
            }
            ExchangeError::OutputCallbackError(uuid, msg) => {
                write!(f, "{} Failed to invoke output callback: {}", uuid, msg)
            }
            ExchangeError::ExchangeCollectorError(uuid, err) => {
                write!(f, "{} Exchange collector error: {}", uuid, err)
            }
        }
    }
}

impl std::error::Error for ExchangeError {}

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
    use futures_util::stream;
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
        let mut exchange: Exchange<'_, Bytes, Bytes, ()> = Exchange::new();

        // Test buffered input
        exchange.set_input(Bytes::from("test input"));
        assert!(exchange.has_input());

        // Test buffered output
        exchange.set_output(Bytes::from("test output"));
        assert!(exchange.has_output());

        let output = exchange.take_output().await.unwrap();
        assert_eq!(output, Bytes::from("test output"));
    }

    #[cfg(feature = "hyper")]
    #[tokio::test]
    async fn test_unified_exchange_streaming_with_collector() {
        use crate::exchange::collector::hyper::BytesCollector;

        let mut exchange: Exchange<'_, Bytes, Bytes, ()> = Exchange::new();

        // Test streaming input with collector
        let input_stream = stream::iter(vec![Ok(Bytes::from("chunk1")), Ok(Bytes::from("chunk2"))]);
        exchange.set_input_stream_with_collector(input_stream, Box::new(BytesCollector));

        assert!(exchange.has_input());

        let input = exchange.input().await.unwrap();
        assert_eq!(input, &Bytes::from("chunk1chunk2"));
    }

    #[tokio::test]
    async fn test_unified_exchange_stream_proxy() {
        let mut exchange: Exchange<'_, Bytes, Bytes, ()> = Exchange::new();

        // Test stream proxying
        let input_stream = stream::iter(vec![Ok(Bytes::from("chunk1")), Ok(Bytes::from("chunk2"))]);
        exchange.set_input_stream(input_stream);

        let stream = exchange.take_input_stream().unwrap();

        // Collect from the proxied stream
        let collected: Vec<_> = stream.collect().await;
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0].as_ref().unwrap(), &Bytes::from("chunk1"));
        assert_eq!(collected[1].as_ref().unwrap(), &Bytes::from("chunk2"));
    }

    #[tokio::test]
    async fn test_unified_exchange_callback_processing() {
        let mut exchange: Exchange<'_, Bytes, Bytes, ()> = Exchange::new();

        // Test callback-based stream processing
        let input_stream = stream::iter(vec![Ok(Bytes::from("chunk1")), Ok(Bytes::from("chunk2"))]);

        exchange
            .process_input_stream(input_stream, |chunks| {
                let mut combined = Vec::new();
                for chunk in chunks {
                    combined.extend_from_slice(&chunk);
                }
                Ok(Bytes::from(combined))
            })
            .await
            .unwrap();

        assert!(exchange.has_input());
    }

    #[tokio::test]
    async fn test_unified_exchange_callbacks() {
        let mut exchange: Exchange<'_, Bytes, Bytes, ()> = Exchange::new();

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
        let mut exchange: Exchange<'_, (), (), String> = Exchange::new();

        exchange.set_metadata("test metadata".to_string());
        let metadata = exchange.metadata().unwrap();
        assert_eq!(metadata, "test metadata");
    }

    #[tokio::test]
    async fn test_error_handling() {
        let mut exchange: Exchange<'_, Bytes, Bytes, ()> = Exchange::new();

        // Test error when no collector provided for stream
        let input_stream = stream::iter(vec![Ok(Bytes::from("chunk"))]);
        exchange.set_input_stream(input_stream);

        let result = exchange.input().await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No collector provided")
        );
    }
}
