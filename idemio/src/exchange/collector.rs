use futures_util::{Stream, StreamExt};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::pin::Pin;

#[cfg(feature = "hyper")]
pub mod hyper {
    use crate::exchange::collector::{CollectorError, StreamCollector};
    use hyper::body::Bytes;

    pub struct BytesCollector;

    impl StreamCollector<Bytes> for BytesCollector {
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

pub struct StringCollector;

impl StreamCollector<String> for StringCollector {
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

pub struct VecCollector<T> {
    phantom: PhantomData<T>,
}

impl<T> VecCollector<T> {
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
    fn collect(&self, items: Vec<Vec<T>>) -> Result<Vec<T>, CollectorError> {
        let total_len: usize = items.iter().map(|v| v.len()).sum();
        let mut result = Vec::with_capacity(total_len);
        for item in items {
            result.extend(item);
        }
        Ok(result)
    }
}

/// Trait for collecting stream items into a single value
pub trait StreamCollector<T>: Send + Sync {
    fn collect(&self, items: Vec<T>) -> Result<T, CollectorError>;
}

pub enum StreamOrValue<'a, T>
where
    T: Send + Sync,
{
    Value(T),
    Stream(
        Pin<Box<dyn Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + 'a>>,
        Option<Box<dyn StreamCollector<T> + 'a>>,
    ),
    Collected(T),
}

impl<'a, T> StreamOrValue<'a, T>
where
    T: Send + Sync,
{
    /// Create from a single value
    pub fn from_value(value: T) -> Self {
        Self::Value(value)
    }

    /// Create from a stream without a collector
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + 'a,
    {
        Self::Stream(Box::pin(stream), None)
    }

    /// Create from a stream with a collector
    pub fn from_stream_with_collector<S>(
        stream: S,
        collector: Box<dyn StreamCollector<T> + 'a>,
    ) -> Self
    where
        S: Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + 'a,
    {
        Self::Stream(Box::pin(stream), Some(collector))
    }

    /// Get value by collecting stream if needed (requires collector for streams)
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
                let collected = collector.collect(items)?;
                *self = StreamOrValue::Collected(collected);
                match self {
                    StreamOrValue::Collected(val) => Ok(val),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// Get value by collecting stream with provided collector
    pub async fn get_value_with_collector<C>(&mut self, collector: C) -> Result<&T, CollectorError>
    where
        C: StreamCollector<T>,
    {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(stream, _) => {
                let mut items = Vec::new();
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(item) => items.push(item),
                        Err(e) => {
                            let msg = format!("Could not collect stream: {}", e.to_string());
                            return Err(CollectorError::stream_collect_error(msg));
                        }
                    }
                }
                let collected = collector.collect(items)?;
                *self = StreamOrValue::Collected(collected);
                match self {
                    StreamOrValue::Collected(val) => Ok(val),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// Take value by collecting stream if needed (requires collector for streams)
    pub async fn take_value(self) -> Result<T, CollectorError> {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(mut stream, collector) => {
                let collector = collector.ok_or_else(|| {
                    CollectorError::invalid_stream_error("No collector present for stream.")
                })?;

                let mut items = Vec::new();
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(item) => items.push(item),
                        Err(e) => {
                            return Err(CollectorError::stream_collect_error(format!(
                                "Could not collect stream: {}",
                                e.to_string()
                            )));
                        }
                    }
                }
                collector.collect(items)
            }
        }
    }

    /// Take value by consuming the stream and collecting it with the provided collector.
    pub async fn take_value_with_collector<C>(self, collector: C) -> Result<T, CollectorError>
    where
        C: StreamCollector<T>,
    {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(mut stream, _) => {
                let mut items = Vec::new();
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(item) => items.push(item),
                        Err(e) => {
                            return Err(CollectorError::stream_collect_error(format!(
                                "Could not collect stream: {}",
                                e.to_string()
                            )));
                        }
                    }
                }
                collector.collect(items)
            }
        }
    }

    /// Takes the stream without consuming it.
    pub fn take_stream(
        self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + 'a>>,
        CollectorError,
    > {
        match self {
            StreamOrValue::Stream(stream, _) => Ok(stream),
            _ => Err(CollectorError::invalid_stream_error("No stream to take")),
        }
    }
}

#[derive(Debug)]
pub enum CollectorError {
    InvalidStreamError(String),
    StreamCollectError(String),
}

impl CollectorError {
    pub fn invalid_stream_error(msg: impl Into<String>) -> Self {
        Self::InvalidStreamError(msg.into())
    }

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
