use std::marker::PhantomData;
use crate::exchange::{Attachments, Callback, ExchangeError};
use futures_util::{Stream, StreamExt};
use std::pin::Pin;
use uuid::Uuid;
use crate::exchange::collector::{StreamCollector, StreamOrValue};

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

    // Metadata operations
    pub fn set_metadata(&mut self, metadata: M) {
        self.metadata = Some(metadata);
    }

    pub fn metadata(&self) -> Result<&M, ExchangeError> {
        match &self.metadata {
            None => Err(ExchangeError::metadata_read_error(
                &self.uuid,
                "Metadata has not been set",
            )),
            Some(metadata) => Ok(metadata),
        }
    }

    pub fn attachments(&self) -> &Attachments {
        &self.attachments
    }

    pub fn attachments_mut(&mut self) -> &mut Attachments {
        &mut self.attachments
    }

    pub fn add_input_listener(
        &mut self,
        callback: impl FnMut(&mut I, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.input_listeners.push(Callback::new(callback));
    }

    pub fn add_output_listener(
        &mut self,
        callback: impl FnMut(&mut O, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.output_listeners.push(Callback::new(callback));
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    // Input operations - buffered
    pub fn set_input(&mut self, input: I) {
        self.input = Some(StreamOrValue::Value(input));
    }

    pub fn save_input(&mut self, input: I) {
        self.set_input(input);
    }

    // Input operations - streaming
    pub fn set_input_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        self.input = Some(StreamOrValue::from_stream(stream));
    }

    pub fn set_input_stream_with_collector<S>(
        &mut self,
        stream: S,
        collector: Box<dyn StreamCollector<I> + 'a>,
    )
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        self.input = Some(StreamOrValue::from_stream_with_collector(stream, collector));
    }

    pub async fn process_input_stream<S, F>(&mut self, stream: S, processor: F) -> Result<(), ExchangeError>
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
        F: FnOnce(Vec<I>) -> Result<I, ExchangeError>,
    {
        let mut items = Vec::new();
        futures_util::pin_mut!(stream);

        while let Some(result) = stream.next().await {
            match result {
                Ok(item) => items.push(item),
                Err(e) => return Err(ExchangeError::input_read_error(
                    &self.uuid,
                    &format!("Stream error: {}", e)
                )),
            }
        }

        let processed = processor(items)?;
        self.set_input(processed);
        Ok(())
    }

    pub async fn process_input_stream_with_collector<S, C>(&mut self, stream: S, collector: C) -> Result<(), ExchangeError>
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
        C: StreamCollector<I>,
    {
        self.process_input_stream(stream, |items| collector.collect(items)).await
    }

    // Get input (collects stream if needed and collector is available)
    pub async fn input(&mut self) -> Result<&I, ExchangeError> {
        match &mut self.input {
            Some(stream_or_value) => stream_or_value.get_value().await,
            None => Err(ExchangeError::input_read_error(
                &self.uuid,
                "No input available"
            )),
        }
    }

    // Get input with provided collector
    pub async fn input_with_collector<C>(&mut self, collector: C) -> Result<&I, ExchangeError>
    where
        C: StreamCollector<I>,
    {
        match &mut self.input {
            Some(stream_or_value) => stream_or_value.get_value_with_collector(collector, &self.uuid).await,
            None => Err(ExchangeError::input_read_error(
                &self.uuid,
                "No input available"
            )),
        }
    }

    // Take input (collects stream if needed and executes callbacks)
    pub async fn take_input(&mut self) -> Result<I, ExchangeError> {
        match self.input.take() {
            Some(stream_or_value) => {
                let mut value = stream_or_value.take_value().await?;
                for mut callback in &mut self.input_listeners.drain(..) {
                    callback.invoke(&mut value, &mut self.attachments);
                }
                Ok(value)
            }
            None => Err(ExchangeError::input_take_error(
                &self.uuid,
                "No input available to take"
            )),
        }
    }

    // Take input with provided collector
    pub async fn take_input_with_collector<C>(&mut self, collector: C) -> Result<I, ExchangeError>
    where
        C: StreamCollector<I>,
    {
        match self.input.take() {
            Some(stream_or_value) => {
                let mut value = stream_or_value.take_value_with_collector(collector).await?;
                // Execute callbacks
                for callback in &mut self.input_listeners {
                    callback.invoke(&mut value, &mut self.attachments);
                }
                Ok(value)
            }
            None => Err(ExchangeError::input_take_error(
                &self.uuid,
                "No input available to take"
            )),
        }
    }

    // Take input stream for proxying
    pub fn take_input_stream(
        &mut self,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>, ExchangeError> {
        match self.input.take() {
            Some(stream_or_value) => stream_or_value.take_stream(),
            None => Err(ExchangeError::input_take_error(
                &self.uuid,
                "No input stream available to take"
            )),
        }
    }

    // Output operations - buffered
    pub fn set_output(&mut self, output: O) {
        self.output = Some(StreamOrValue::Value(output));
    }

    pub fn save_output(&mut self, output: O) {
        self.set_output(output);
    }

    // Output operations - streaming
    pub fn set_output_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = Result<O, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        self.output = Some(StreamOrValue::from_stream(stream));
    }

    pub fn set_output_stream_with_collector<S>(
        &mut self,
        stream: S,
        collector: Box<dyn StreamCollector<O> + 'a>,
    )
    where
        S: Stream<Item = Result<O, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        self.output = Some(StreamOrValue::from_stream_with_collector(stream, collector));
    }

    pub async fn process_output_stream<S, F>(&mut self, stream: S, processor: F) -> Result<(), ExchangeError>
    where
        S: Stream<Item = Result<O, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
        F: FnOnce(Vec<O>) -> Result<O, ExchangeError>,
    {

        let mut items = Vec::new();
        futures_util::pin_mut!(stream);

        while let Some(result) = stream.next().await {
            match result {
                Ok(item) => items.push(item),
                Err(e) => return Err(ExchangeError::output_read_error(
                    &self.uuid,
                    &format!("Stream error: {}", e)
                )),
            }
        }

        let processed = processor(items)?;
        self.set_output(processed);
        Ok(())
    }

    pub async fn process_output_stream_with_collector<S, C>(&mut self, stream: S, collector: C) -> Result<(), ExchangeError>
    where
        S: Stream<Item = Result<O, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
        C: StreamCollector<O>,
    {
        self.process_output_stream(stream, |items| collector.collect(items)).await
    }

    // Get output (collects stream if needed and collector is available)
    pub async fn output(&mut self) -> Result<&O, ExchangeError> {
        match &mut self.output {
            Some(stream_or_value) => stream_or_value.get_value().await,
            None => Err(ExchangeError::output_read_error(
                &self.uuid,
                "No output available"
            )),
        }
    }

    // Get output with provided collector
    pub async fn output_with_collector<C>(&mut self, collector: C) -> Result<&O, ExchangeError>
    where
        C: StreamCollector<O>,
    {
        match &mut self.output {
            Some(stream_or_value) => stream_or_value.get_value_with_collector(collector, &self.uuid).await,
            None => Err(ExchangeError::output_read_error(
                &self.uuid,
                "No output available"
            )),
        }
    }

    // Take output (collects stream if needed and executes callbacks)
    pub async fn take_output(&mut self) -> Result<O, ExchangeError> {
        match self.output.take() {
            Some(stream_or_value) => {
                let mut value = stream_or_value.take_value().await?;
                // Execute callbacks
                for callback in &mut self.output_listeners {
                    callback.invoke(&mut value, &mut self.attachments);
                }
                Ok(value)
            }
            None => Err(ExchangeError::output_take_error(
                &self.uuid,
                "No output available to take"
            )),
        }
    }

    pub async fn take_output_with_collector<C>(&mut self, collector: C) -> Result<O, ExchangeError>
    where
        C: StreamCollector<O>,
    {
        match self.output.take() {
            Some(stream_or_value) => {
                let mut value = stream_or_value.take_value_with_collector(collector).await?;
                // Execute callbacks
                for mut callback in &mut self.output_listeners.drain(..) {
                    callback.invoke(&mut value, &mut self.attachments);
                }
                Ok(value)
            }
            None => Err(ExchangeError::output_take_error(
                &self.uuid,
                "No output available to take"
            )),
        }
    }

    // Take output stream for proxying
    pub fn take_output_stream(
        &mut self,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<O, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>, ExchangeError> {
        match self.output.take() {
            Some(stream_or_value) => {
                stream_or_value.take_stream()
            }
            None => Err(ExchangeError::output_take_error(
                &self.uuid,
                "No output stream available to take"
            )),
        }
    }

    pub fn has_input(&self) -> bool {
        self.input.is_some()
    }

    pub fn has_output(&self) -> bool {
        self.output.is_some()
    }

}

/// Builder for creating different types of exchanges with various input/output configurations
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
    output_config: OutputConfig
}

/// Configuration for input handling
pub enum InputConfig<'a, I>
where
    I: Send + Sync,
{
    /// No input configured
    None,
    /// Buffered input with a concrete value
    Buffered(I),
    /// Streaming input without a collector
    Streaming(Pin<Box<dyn Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>),
    /// Streaming input with a collector
    StreamingWithCollector {
        stream: Pin<Box<dyn Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>,
        collector: Box<dyn StreamCollector<I> + 'a>,
    },
}

/// Configuration for output handling
pub enum OutputConfig
{
    /// No output configured
    None,
    /// Buffered output with a concrete value
    Buffered,
    /// Streaming output without a collector
    Streaming,
}

impl<'a, I, O, M> ExchangeBuilder<'a, I, O, M>
where
    I: Send + Sync + 'a,
    O: Send + Sync,
    M: Send + Sync,
{
    /// Create a new exchange builder
    pub fn new() -> Self {
        Self {
            uuid: None,
            phantom: PhantomData::default(),
            metadata: None,
            input_config: InputConfig::None,
            output_config: OutputConfig::None
        }
    }

    /// Set a custom UUID for the exchange
    pub fn with_uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = Some(uuid);
        self
    }

    /// Set metadata for the exchange
    pub fn with_metadata(mut self, metadata: M) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Configure buffered input
    pub fn with_buffered_input(mut self, input: I) -> Self {
        self.input_config = InputConfig::Buffered(input);
        self
    }

    /// Configure streaming input without a collector
    pub fn with_streaming_input<S>(mut self, stream: S) -> Self
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        self.input_config = InputConfig::Streaming(Box::pin(stream));
        self
    }

    /// Configure streaming input with a collector
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

    /// Configure buffered output
    pub fn with_buffered_output(mut self) -> Self {
        self.output_config = OutputConfig::Buffered;
        self
    }

    /// Configure streaming output without a collector
    pub fn with_streaming_output(mut self) -> Self
    {
        self.output_config = OutputConfig::Streaming;
        self
    }

    /// Build the exchange
    pub fn build(self) -> Exchange<'a, I, O, M> {
        let mut exchange = Exchange::new();
        // Set metadata if provided
        if let Some(metadata) = self.metadata {
            exchange.set_metadata(metadata);
        }

        // Configure input
        match self.input_config {
            InputConfig::None => {
                // No input to configure
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

        exchange
    }
}

/// Convenience methods for common exchange patterns
impl<'a, I, O, M> ExchangeBuilder<'a, I, O, M>
where
    I: Send + Sync + 'a,
    O: Send + Sync,
    M: Send + Sync,
{
    /// Create a fully buffered exchange (buffered request + buffered response)
    pub fn buffered(input: I) -> Exchange<'a, I, O, M> {
        Self::new()
            .with_buffered_input(input)
            .with_buffered_output()
            .build()
    }

    /// Create a streaming request with buffered response exchange
    pub fn streaming_request_buffered_response<S>(
        input_stream: S,
    ) -> Exchange<'a, I, O, M>
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        Self::new()
            .with_streaming_input(input_stream)
            .with_buffered_output()
            .build()
    }

    /// Create a streaming request with buffered response exchange (with collector)
    pub fn streaming_request_buffered_response_with_collector<S>(
        input_stream: S,
        input_collector: Box<dyn StreamCollector<I> + 'a>,
    ) -> Exchange<'a, I, O, M>
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        Self::new()
            .with_streaming_input_and_collector(input_stream, input_collector)
            .with_buffered_output()
            .build()
    }

    /// Create a buffered request with streaming response exchange
    pub fn buffered_request_streaming_response<S>(
        input: I,
    ) -> Exchange<'a, I, O, M>
    {
        Self::new()
            .with_buffered_input(input)
            .with_streaming_output()
            .build()
    }

    /// Create a buffered request with streaming response exchange (with collector)
    pub fn buffered_request_streaming_response_with_collector<S>(
        input: I,
    ) -> Exchange<'a, I, O, M>
    {
        Self::new()
            .with_buffered_input(input)
            .with_streaming_output()
            .build()
    }

    /// Create a fully streaming exchange (streaming request + streaming response)
    pub fn streaming<S>(
        input_stream: S,
    ) -> Exchange<'a, I, O, M>
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a,
    {
        Self::new()
            .with_streaming_input(input_stream)
            .with_streaming_output()
            .build()
    }

    /// Create a fully streaming exchange with collectors
    pub fn streaming_with_collectors<S>(
        input_stream: S,
        input_collector: Box<dyn StreamCollector<I> + 'a>,
    ) -> Exchange<'a, I, O, M>
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a
    {
        Self::new()
            .with_streaming_input_and_collector(input_stream, input_collector)
            .with_streaming_output()
            .build()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use hyper::body::Bytes;


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
        use crate::exchange::collector::BytesCollector;

        let mut exchange: Exchange<'_, Bytes, Bytes, ()> = Exchange::new();

        // Test streaming input with collector
        let input_stream = stream::iter(vec![
            Ok(Bytes::from("chunk1")),
            Ok(Bytes::from("chunk2")),
        ]);
        exchange.set_input_stream_with_collector(input_stream, Box::new(BytesCollector));

        assert!(exchange.has_input());

        let input = exchange.input().await.unwrap();
        assert_eq!(input, &Bytes::from("chunk1chunk2"));
    }

    #[tokio::test]
    async fn test_unified_exchange_stream_proxy() {
        let mut exchange: Exchange<'_, Bytes, Bytes, ()> = Exchange::new();

        // Test stream proxying
        let input_stream = stream::iter(vec![
            Ok(Bytes::from("chunk1")),
            Ok(Bytes::from("chunk2")),
        ]);
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
        let input_stream = stream::iter(vec![
            Ok(Bytes::from("chunk1")),
            Ok(Bytes::from("chunk2")),
        ]);

        exchange.process_input_stream(input_stream, |chunks| {
            let mut combined = Vec::new();
            for chunk in chunks {
                combined.extend_from_slice(&chunk);
            }
            Ok(Bytes::from(combined))
        }).await.unwrap();

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
        assert!(result.unwrap_err().to_string().contains("No collector provided"));
    }
}