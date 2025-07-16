use std::error::Error;
use std::pin::Pin;
use futures_util::{Stream, StreamExt};
use uuid::Uuid;
use crate::exchange::ExchangeError;

#[cfg(feature = "hyper")]
use hyper::body::Bytes;

#[cfg(feature = "hyper")]
pub struct BytesCollector;

#[cfg(feature = "hyper")]
impl StreamCollector<Bytes> for BytesCollector {
    fn collect(&self, items: Vec<Bytes>) -> Result<Bytes, ExchangeError> {
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

pub struct StringCollector;

impl StreamCollector<String> for StringCollector {
    fn collect(&self, items: Vec<String>) -> Result<String, ExchangeError> {
        Ok(items.join(""))
    }
}

pub struct VecCollector<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> VecCollector<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> Default for VecCollector<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> StreamCollector<Vec<T>> for VecCollector<T>
where
    T: Send + Sync
{
    fn collect(&self, items: Vec<Vec<T>>) -> Result<Vec<T>, ExchangeError> {
        let mut result = Vec::new();
        for item in items {
            result.extend(item);
        }
        Ok(result)
    }
}

/// Trait for collecting stream items into a single value
pub trait StreamCollector<T>: Send + Sync {
    fn collect(&self, items: Vec<T>) -> Result<T, ExchangeError>;
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
        S: Stream<Item = Result<T, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        Self::Stream(Box::pin(stream), None)
    }

    /// Create from a stream with a collector
    pub fn from_stream_with_collector<S>(
        stream: S,
        collector: Box<dyn StreamCollector<T> + 'a>,
    ) -> Self
    where
        S: Stream<Item = Result<T, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        Self::Stream(Box::pin(stream), Some(collector))
    }

    /// Get value by collecting stream if needed (requires collector for streams)
    pub async fn get_value(&mut self) -> Result<&T, ExchangeError> {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(stream, collector) => {
                let collector = collector.take().ok_or_else(|| {
                    todo!("Handle stream error")
                })?;

                let mut items = Vec::new();
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(item) => items.push(item),
                        Err(e) => todo!("Handle stream error"),
                    }
                }
                let collected = collector.collect(items)?;
                *self = StreamOrValue::Collected(collected);
                if let StreamOrValue::Collected(val) = self {
                    Ok(val)
                } else {
                    unreachable!()
                }
            }
        }
    }

    /// Get value by collecting stream with provided collector
    pub async fn get_value_with_collector<C>(
        &mut self,
        collector: C,
        uuid: &Uuid,
    ) -> Result<&T, ExchangeError>
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
                        Err(e) => return Err(ExchangeError::input_read_error(
                            uuid,
                            &format!("Stream error: {}", e)
                        )),
                    }
                }
                let collected = collector.collect(items)?;
                *self = StreamOrValue::Collected(collected);
                if let StreamOrValue::Collected(val) = self {
                    Ok(val)
                } else {
                    unreachable!()
                }
            }
        }
    }

    /// Take value by collecting stream if needed (requires collector for streams)
    pub async fn take_value(self) -> Result<T, ExchangeError> {
        match self {
            StreamOrValue::Value(val) => Ok(val),
            StreamOrValue::Collected(val) => Ok(val),
            StreamOrValue::Stream(mut stream, collector) => {
                let collector = collector.ok_or_else(|| {
                    todo!("Implement stream error handling")
                })?;

                let mut items = Vec::new();
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(item) => items.push(item),
                        Err(e) => todo!("Handle stream error"),
                    }
                }
                collector.collect(items)
            }
        }
    }

    /// Take value by collecting stream with provided collector
    pub async fn take_value_with_collector<C>(
        self,
        collector: C
    ) -> Result<T, ExchangeError>
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
                        Err(e) => todo!("Handle stream error"),
                    }
                }
                collector.collect(items)
            }
        }
    }
    
    pub fn take_stream(self) -> Result<Pin<Box<dyn Stream<Item = Result<T, Box<dyn Error + Send + Sync>>> + Send + 'a>>, ExchangeError> {
        match self {
            StreamOrValue::Stream(stream, _) => Ok(stream),
            _ => todo!("Handle stream error"),
        }
    }
}