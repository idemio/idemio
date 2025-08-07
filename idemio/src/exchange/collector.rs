use futures_util::{Stream, StreamExt};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::pin::Pin;

#[cfg(feature = "hyper")]
pub mod hyper {
    use crate::exchange::collector::{CollectorError, StreamCollector};
    use hyper::body::Bytes;

    /// A collector that combines multiple `Bytes` chunks into a single `Bytes` instance.
    pub struct HyperBytesCollector;

    impl StreamCollector<Bytes> for HyperBytesCollector {
        /// Collects a vector of `Bytes` chunks into a single `Bytes` instance.
        ///
        /// # Parameters
        /// - `items`: A vector of `Bytes` chunks to be combined
        ///
        /// # Returns
        /// `Ok(Bytes)` containing all input bytes concatenated, or `Err(CollectorError)` if collection fails.
        ///
        /// # Behavior
        /// If the input vector is empty, returns an empty `Bytes` instance. Otherwise, calculates the total
        /// length and pre-allocates a vector with that capacity for efficient concatenation.
        fn collect(&self, items: Vec<Bytes>) -> Result<Bytes, CollectorError> {
            if items.is_empty() {
                return Ok(Bytes::new());
            }
            let total_len = items.iter().map(|b| b.len()).sum();
            let mut combined = Vec::with_capacity(total_len);
            for chunk in items {
                combined.extend_from_slice(&chunk);
            }
            Ok(Bytes::from(combined))
        }
    }
}

/// A collector that concatenates multiple strings into a single string.
pub struct StringCollector;

impl StreamCollector<String> for StringCollector {
    /// Collects a vector of strings into a single concatenated string.
    ///
    /// # Parameters
    /// - `items`: A vector of strings to be concatenated
    ///
    /// # Returns
    /// `Ok(String)` containing all input strings concatenated, or `Err(CollectorError)` if collection fails.
    ///
    /// # Behavior
    /// If the input vector is empty, returns an empty string. Otherwise, calculates the total
    /// length and pre-allocates a string with that capacity for efficient concatenation.
    fn collect(&self, items: Vec<String>) -> Result<String, CollectorError> {
        if items.is_empty() {
            return Ok(String::new());
        }
        let total_len: usize = items.iter().map(|s| s.len()).sum();
        let mut result = String::with_capacity(total_len);
        for item in items {
            result.push_str(&item);
        }
        Ok(result)
    }
}

/// A collector that flattens multiple vectors into a single vector.
pub struct VecCollector<T> {
    phantom: PhantomData<T>,
}

impl<T> VecCollector<T> {
    /// Creates a new `VecCollector` instance.
    pub fn new() -> Self {
        Self {
            phantom: PhantomData,
        }
    }
}

impl<T> StreamCollector<Vec<T>> for VecCollector<T>
where
    T: Send + Sync,
{
    /// Collects multiple vectors into a single flattened vector.
    ///
    /// # Parameters
    /// - `items`: A vector of vectors to be flattened
    ///
    /// # Returns
    /// `Ok(Vec<T>)` containing all elements from input vectors, or `Err(CollectorError)` if collection fails.
    ///
    /// # Behavior
    /// Calculates the total length of all input vectors and pre-allocates a result vector
    /// with that capacity for efficient flattening.
    fn collect(&self, items: Vec<Vec<T>>) -> Result<Vec<T>, CollectorError> {
        let total_len: usize = items.iter().map(|v| v.len()).sum();
        let mut result = Vec::with_capacity(total_len);
        for item in items {
            result.extend(item);
        }
        Ok(result)
    }
}

/// Trait for collecting stream items into a single value.
///
/// This trait defines the interface for collectors that can combine multiple items
/// of type `T` into a single item of the same type.
pub trait StreamCollector<T>: Send + Sync {
    /// Collects a vector of items into a single item.
    ///
    /// # Parameters
    /// - `items`: A vector of items to be collected/combined
    ///
    /// # Returns
    /// `Ok(T)` containing the collected result, or `Err(CollectorError)` if collection fails.
    fn collect(&self, items: Vec<T>) -> Result<T, CollectorError>;
}

/// An enum that can hold either a direct value, a stream of values, or a collected value.
///
/// This type allows for flexible handling of data that might be immediately available
/// as a value or streamed asynchronously.
pub enum StreamOrValue<'a, T>
where
    Self: Send + Sync,
    T: Send + Sync,
{
    /// A direct value that's immediately available.
    Value(T),
    /// A stream of values with an optional collector for combining them.
    Stream(
        Pin<Box<dyn Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send  + Sync + 'a>>,
        Option<Box<dyn StreamCollector<T> + 'a>>,
    ),
    /// A value that has been collected from a stream.
    Collected(T),
}

impl<'a, T> StreamOrValue<'a, T>
where
    Self: Send + Sync,
    T: Send + Sync,
{
    /// Creates a `StreamOrValue` from a direct value.
    ///
    /// # Parameters
    /// - `value`: The value to wrap
    ///
    /// # Returns
    /// `StreamOrValue::Value` containing the provided value.
    ///
    /// # Examples
    /// ```rust
    /// use idemio::exchange::collector::StreamOrValue;
    /// let stream_or_value = StreamOrValue::from_value("hello".to_string());
    /// ```
    ///
    /// # Behavior
    /// This creates a variant that contains an immediately available value.
    pub fn from_value(value: T) -> Self {
        Self::Value(value)
    }

    /// Creates a `StreamOrValue` from a stream without a collector.
    ///
    /// # Parameters
    /// - `stream`: A stream that yields `Result<T, Box<dyn Error + Send + Sync>>`
    ///
    /// # Returns
    /// `StreamOrValue::Stream` containing the provided stream.
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::StreamOrValue;
    ///
    /// let data_stream = stream::iter(vec![Ok("chunk1".to_string()), Ok("chunk2".to_string())]);
    /// let stream_or_value = StreamOrValue::from_stream(data_stream);
    /// ```
    ///
    /// # Behavior
    /// The stream will need a collector to be provided later when collecting values.
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + Sync + 'a,
    {
        Self::Stream(Box::pin(stream), None)
    }

    /// Creates a `StreamOrValue` from a stream with a pre-configured collector.
    ///
    /// # Parameters
    /// - `stream`: A stream that yields `Result<T, Box<dyn Error + Send + Sync>>`
    /// - `collector`: A boxed collector for combining stream items
    ///
    /// # Returns
    /// `StreamOrValue::Stream` containing the provided stream and collector.
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::{StreamOrValue, StringCollector};
    ///
    /// async {
    ///     let data_stream = stream::iter(vec![Ok("chunk1".to_string()), Ok("chunk2".to_string())]);
    ///     let collector = Box::new(StringCollector);
    ///     let stream_or_value = StreamOrValue::from_stream_with_collector(data_stream, collector);
    /// }
    /// ```
    ///
    /// # Behavior
    /// The stream can be immediately collected using the provided collector.
    pub fn from_stream_with_collector<S>(
        stream: S,
        collector: Box<dyn StreamCollector<T> + 'a>,
    ) -> Self
    where
        S: Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + Sync + 'a,
    {
        Self::Stream(Box::pin(stream), Some(collector))
    }

    /// Gets a reference to the value, collecting from the stream if needed.
    ///
    /// # Returns
    /// `Ok(&T)` containing a reference to the value, or
    /// `Err(CollectorError)` if a collection fails or no collector is available for a stream.
    ///
    /// # Examples
    /// ```rust
    ///
    /// use idemio::exchange::collector::StreamOrValue;
    /// async {
    ///     let mut stream_or_value = StreamOrValue::from_value("hello".to_string());
    ///     let value_ref = stream_or_value.get_value().await?;
    ///     println!("{}", value_ref);
    /// }
    /// ```
    ///
    /// # Behavior
    /// - For `Value` and `Collected` variants, returns the contained value immediately
    /// - For `Stream` variant, requires a collector to be present, consumes the stream,
    ///   and converts to `Collected` variant
    /// - May panic if called on a stream without a collector
    pub async fn get_value(&mut self) -> Result<&T, CollectorError> {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(stream, collector) => {
                let collector = match collector.take() {
                    None => {
                        return Err(CollectorError::invalid_stream_error(
                            "No collector present for stream.",
                        ));
                    }
                    Some(collector) => collector,
                };
                let items = Self::collect_stream_items(stream).await?;
                let collected = collector.collect(items)?;
                *self = StreamOrValue::Collected(collected);
                match self {
                    StreamOrValue::Collected(val) => Ok(val),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// Helper function to collect all items from a stream into a vector.
    ///
    /// # Parameters
    /// - `stream`: A mutable reference to the stream to collect from
    ///
    /// # Returns
    /// Returns `Ok(Vec<T>)` containing all collected items, or `Err(CollectorError)` if any stream item fails.
    ///
    /// # Behavior
    /// Iterates through the stream, collecting successful items and returning an error
    /// immediately if any item fails.
    #[inline]
    async fn collect_stream_items(
        stream: &mut Pin<Box<dyn Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + Sync + 'a>>,
    ) -> Result<Vec<T>, CollectorError> {
        let mut items = Vec::new();
        while let Some(result) = stream.next().await {
            match result {
                Ok(item) => items.push(item),
                Err(e) => {
                    return Err(CollectorError::stream_collect_error(format!(
                        "Could not collect stream: {}",
                        e
                    )));
                }
            }
        }
        Ok(items)
    }


    /// Gets a reference to the value using a provided collector.
    ///
    /// # Parameters
    /// - `collector`: A collector implementing `StreamCollector<T>` for combining stream items
    ///
    /// # Returns
    /// `Ok(&T)` containing a reference to the value, or `Err(CollectorError)` if collection fails.
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::{StreamOrValue, StringCollector};
    ///
    /// async {
    ///     let data_stream = stream::iter(vec![Ok("chunk1".to_string()), Ok("chunk2".to_string())]);
    ///     let mut stream_or_value = StreamOrValue::from_stream(data_stream);
    ///     let collector = StringCollector;
    ///     let value_ref = stream_or_value.get_value_with_collector(collector).await?;
    /// }
    /// ```
    ///
    /// # Behavior
    /// - For `Value` and `Collected` variants, returns the contained value immediately
    /// - For `Stream` variant, uses the provided collector to collect stream items
    ///   and converts to `Collected` variant
    pub async fn get_value_with_collector<C>(&mut self, collector: C) -> Result<&T, CollectorError>
    where
        C: StreamCollector<T>,
    {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(stream, _) => {
                let items = Self::collect_stream_items(stream).await?;
                let collected = collector.collect(items)?;
                *self = StreamOrValue::Collected(collected);
                match self {
                    StreamOrValue::Collected(val) => Ok(val),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// Takes ownership of the value, collecting from stream if needed.
    ///
    /// # Returns
    /// `Ok(T)` containing the owned value, or `Err(CollectorError)` if
    /// collection fails or no collector is available for a stream.
    ///
    /// # Examples
    /// ```rust
    ///
    /// use idemio::exchange::collector::StreamOrValue;
    /// async {
    ///     let stream_or_value = StreamOrValue::from_value("hello".to_string());
    ///     let owned_value = stream_or_value.take_value().await?;
    ///     println!("{}", owned_value);
    /// }
    /// ```
    ///
    /// # Behavior
    /// - Consumes the `StreamOrValue` instance
    /// - For `Value` and `Collected` variants, returns the contained value
    /// - For `Stream` variant, requires a collector to be present and collects all stream items
    pub async fn take_value(self) -> Result<T, CollectorError> {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(mut stream, collector) => {
                let collector = collector.ok_or_else(|| {
                    CollectorError::invalid_stream_error("No collector present for stream.")
                })?;
                let items = Self::collect_stream_items(&mut stream).await?;
                collector.collect(items)
            }
        }
    }

    /// Takes ownership of the value using a provided collector.
    ///
    /// # Parameters
    /// - `collector`: A collector implementing `StreamCollector<T>` for combining stream items
    ///
    /// # Returns
    /// `Ok(T)` containing the owned value, or `Err(CollectorError)` if collection fails.
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::{StreamOrValue, StringCollector};
    /// async {
    ///     let data_stream = stream::iter(vec![Ok("chunk1".to_string()), Ok("chunk2".to_string())]);
    ///     let stream_or_value = StreamOrValue::from_stream(data_stream);
    ///     let collector = StringCollector;
    ///     let owned_value = stream_or_value.take_value_with_collector(collector).await?;
    /// }
    /// ```
    ///
    /// # Behavior
    /// - Consumes the `StreamOrValue` instance
    /// - For `Value` and `Collected` variants, returns the contained value
    /// - For `Stream` variant, uses the provided collector to collect all stream items
    pub async fn take_value_with_collector<C>(self, collector: C) -> Result<T, CollectorError>
    where
        C: StreamCollector<T>,
    {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(mut stream, _) => {
                let items = Self::collect_stream_items(&mut stream).await?;
                collector.collect(items)
            }
        }
    }

    /// Extracts the underlying stream if present.
    ///
    /// # Returns
    /// `Ok(Pin<Box<dyn Stream<...>>>)` containing the stream, or `Err(CollectorError)`
    /// if this instance doesn't contain a stream.
    ///
    /// # Examples
    /// ```rust
    /// use futures_util::stream;
    /// use idemio::exchange::collector::StreamOrValue;
    ///
    /// let data_stream = stream::iter(vec![Ok("chunk1".to_string()), Ok("chunk2".to_string())]);
    /// let stream_or_value = StreamOrValue::from_stream(data_stream);
    /// let extracted_stream = stream_or_value.take_stream()?;
    /// ```
    ///
    /// # Behavior
    /// - Consumes the `StreamOrValue` instance
    /// - Only succeeds for `Stream` variant
    /// - Returns an error for `Value` and `Collected` variants
    pub fn take_stream(
        self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + Sync + 'a>>,
        CollectorError,
    > {
        match self {
            StreamOrValue::Stream(stream, _) => Ok(stream),
            _ => Err(CollectorError::invalid_stream_error("No stream to take")),
        }
    }
}

/// Error type for stream collection operations.
///
/// This enum represents various error conditions that can occur during
/// stream collection and processing.
#[derive(Debug)]
pub enum CollectorError {
    /// Error indicating an invalid stream operation or state.
    InvalidStreamError(String),
    /// Error that occurred during stream collection.
    StreamCollectError(String),
}

impl CollectorError {
    /// Creates a new `InvalidStreamError` with the provided message.
    pub fn invalid_stream_error(msg: impl Into<String>) -> Self {
        Self::InvalidStreamError(msg.into())
    }

    /// Creates a new `StreamCollectError` with the provided message.
    pub fn stream_collect_error(msg: impl Into<String>) -> Self {
        Self::StreamCollectError(msg.into())
    }
}

impl Display for CollectorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CollectorError::InvalidStreamError(msg) => {
                write!(f, "Invalid Stream Error: {}", msg)
            }
            CollectorError::StreamCollectError(msg) => {
                write!(f, "Stream Collect Error: {}", msg)
            }
        }
    }
}

impl Error for CollectorError {}