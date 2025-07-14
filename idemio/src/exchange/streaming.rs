use crate::exchange::buffered::BufferedExchange;
use crate::exchange::{Attachments, Callback, ExchangeError, RootExchange};
use futures_util::{Stream, StreamExt};
use hyper::body::Bytes;
use std::pin::Pin;
use async_trait::async_trait;
use uuid::Uuid;

pub trait Collectible {
    fn collect(items: Vec<Self>) -> Result<Self, ExchangeError>
    where
        Self: Sized;
}

// Implementation for Bytes - concatenate all chunks
impl Collectible for Bytes {
    fn collect(items: Vec<Self>) -> Result<Self, ExchangeError> {
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

impl Collectible for String {
    fn collect(items: Vec<Self>) -> Result<Self, ExchangeError> {
        Ok(items.join(""))
    }
}

impl<T> Collectible for Vec<T> {
    fn collect(items: Vec<Self>) -> Result<Self, ExchangeError> {
        let mut result = Vec::new();
        for item in items {
            result.extend(item);
        }
        Ok(result)
    }
}

pub struct StreamCollector<T> {
    collected: Option<T>,
    stream: Option<
        Pin<
            Box<
                dyn Stream<Item = Result<T, Box<dyn std::error::Error + Send + Sync>>>
                    + Send
                    + Sync,
            >,
        >,
    >,
}

impl<T> StreamCollector<T> {
    pub fn new() -> Self {
        Self {
            collected: None,
            stream: None,
        }
    }

    pub fn is_collected(&self) -> bool {
        self.collected.is_some()
    }

    pub fn set_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = Result<T, Box<dyn std::error::Error + Send + Sync>>> + Send + Sync + 'static,
    {
        self.stream = Some(Box::pin(stream));
    }

    pub async fn collect_stream(&mut self) -> Result<(), ExchangeError> {
        if let Some(stream) = self.stream.as_mut() {
            let mut collected = Vec::new();
            while let Some(item_result) = stream.next().await {
                match item_result {
                    Ok(item) => collected.push(item),
                    Err(e) => {
                        todo!()
                    }
                }
            }
            self.collected = Some(collected);
        }
        Ok(())
    }
}

pub enum StreamingExchange<Input, Output, Metadata> {
    Request(StreamingRequestExchange<Input, Output, Metadata>),
    Response(StreamingResponseExchange<Input, Output, Metadata>),
    BiDirectional(BiDirectionalStreamingExchange<Input, Output, Metadata>),
}

#[async_trait]
impl<I, O, M> RootExchange<I, O, M> for StreamingExchange<I, O, M>
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    fn get_metadata(&self) -> &M {
        match self {
            StreamingExchange::Request(inner) => &inner.exchange.get_metadata(),
            StreamingExchange::Response(inner) => &inner.exchange.get_metadata(),
            StreamingExchange::BiDirectional(inner) => &inner.exchange.get_metadata(),
        }
    }

    fn get_metadata_mut(&mut self) -> &mut M {
        match self {
            StreamingExchange::Request(inner) => inner.exchange.get_metadata_mut(),
            StreamingExchange::Response(inner) => inner.exchange.get_metadata_mut(),
            StreamingExchange::BiDirectional(inner) => inner.exchange.get_metadata_mut(),
        }
    }

    fn get_attachments(&self) -> &Attachments {
        match self {
            StreamingExchange::Request(inner) => &inner.exchange.attachments(),
            StreamingExchange::Response(inner) => &inner.exchange.attachments(),
            StreamingExchange::BiDirectional(inner) => &inner.exchange.attachments(),
        }
    }

    fn get_attachments_mut(&mut self) -> &mut Attachments {
        match self {
            StreamingExchange::Request(inner) => inner.exchange.attachments_mut(),
            StreamingExchange::Response(inner) => inner.exchange.attachments_mut(),
            StreamingExchange::BiDirectional(inner) => inner.exchange.attachments_mut(),
        }
    }

    fn get_uuid(&self) -> &Uuid {
        match self {
            StreamingExchange::Request(inner) => &mut inner.exchange.uuid(),
            StreamingExchange::Response(inner) => &mut inner.exchange.uuid(),
            StreamingExchange::BiDirectional(inner) => &mut inner.exchange.uuid(),
        }
    }

    fn set_input(&mut self, input: I) {
        match self {
            StreamingExchange::Request(inner) => inner.input_stream.set_stream(input),
            StreamingExchange::Response(inner) => inner.exchange.save_input(input),
            StreamingExchange::BiDirectional(inner) => inner.input_stream.set_stream(input),
        }
    }

    async fn get_input(&mut self) -> Result<&I, ExchangeError> {
        match self {
            StreamingExchange::Request(inner) => {
                if !inner.input_stream.is_collected() {
                    Ok(inner.collect_input().await?)
                } else {
                    todo!("Implement 'collectible'")
                }
            }
            StreamingExchange::Response(inner) => inner.exchange.input(),
            StreamingExchange::BiDirectional(inner) => {
                if !inner.input_stream.is_collected() {
                    Ok(inner.collect_input().await?)
                } else {
                    todo!("Implement 'collectible'")
                }
            }
        }
    }

    async fn get_output(&mut self) -> Result<&O, ExchangeError> {
        match self {
            StreamingExchange::Request(inner) => inner.exchange.output(),
            StreamingExchange::Response(inner) => {
                if !inner.output_stream.is_collected() {
                    Ok(inner.collect_output().await?)
                } else {
                    todo!("Implement 'collectible'")
                }
            }
            StreamingExchange::BiDirectional(inner) => {
                if !inner.output_stream.is_collected() {
                    Ok(inner.collect_output().await?)
                } else {
                    todo!("Implement 'collectible'")
                }
            }
        }
    }

    async fn consume_input(&mut self) -> Result<I, ExchangeError> {
        match self {
            StreamingExchange::Request(inner) => {
                if !inner.input_stream.is_collected() {
                    inner.collect_input().await?;
                }
                inner.exchange.take_input()
            }
            StreamingExchange::Response(inner) => inner.exchange.take_input(),
            StreamingExchange::BiDirectional(inner) => {
                if !inner.input_stream.is_collected() {
                    inner.collect_input().await?;
                }
                inner.exchange.take_input()
            }
        }
    }

    fn set_output(&mut self, output: O) {
        match self {
            StreamingExchange::Request(inner) => inner.exchange.save_output(output),
            StreamingExchange::Response(inner) => inner.output_stream.set_stream(output),
            StreamingExchange::BiDirectional(inner) => inner.output_stream.set_stream(output),
        }
    }

    async fn consume_output(&mut self) -> Result<O, ExchangeError> {
        match self {
            StreamingExchange::Request(inner) => inner.exchange.take_output(),
            StreamingExchange::Response(inner) => {
                if !inner.output_stream.is_collected() {
                    inner.collect_output().await?;
                }
                inner.exchange.take_output()
            }
            StreamingExchange::BiDirectional(inner) => {
                if !inner.output_stream.is_collected() {
                    inner.collect_output().await?;
                }
                inner.exchange.take_output()
            }
        }
    }

    fn add_input_callback(&mut self, callback: Callback<I>) {
        todo!()
    }

    fn add_output_callback(&mut self, callback: Callback<O>) {
        todo!()
    }
}

pub struct StreamingRequestExchange<I, O, M> {
    pub(crate) exchange: BufferedExchange<I, O, M>,
    pub(crate) input_stream: StreamCollector<I>,
}

impl<I, O, M> StreamingRequestExchange<I, O, M>
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    pub fn new(exchange: BufferedExchange<I, O, M>) -> Self {
        Self {
            exchange,
            input_stream: StreamCollector::new(),
        }
    }

    pub fn set_input_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'static,
    {
        self.input_stream.set_stream(stream)
    }

    pub async fn collect_input(&mut self) -> Result<&I, ExchangeError> {
        self.input_stream.collect_stream().await?;
        let input = match self.input_stream.collected.take() {
            None => todo!(),
            Some(collected) => collected,
        };
        self.exchange.save_input(input);
        self.exchange.input()
    }
}

pub struct StreamingResponseExchange<Input, Output, Metadata> {
    pub(crate) exchange: BufferedExchange<Input, Output, Metadata>,
    pub(crate) output_stream: StreamCollector<Output>,
}

impl<Input, Output, Metadata> StreamingResponseExchange<Input, Output, Metadata>
where
    Input: Send + Sync,
    Output: Send + Sync,
    Metadata: Send + Sync,
{
    pub fn set_output_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = Result<Output, Box<dyn std::error::Error + Send + Sync>>> + Send + 'static,
    {
        self.output_stream.set_stream(stream)
    }

    pub async fn collect_output(&mut self) -> Result<&Output, ExchangeError> {
        self.output_stream.collect_stream().await?;
        let input = match self.output_stream.collected.take() {
            None => todo!(),
            Some(collected) => collected,
        };
        self.exchange.save_output(input);
        self.exchange.output()
    }
}

pub struct BiDirectionalStreamingExchange<I, O, M> {
    pub(crate) exchange: BufferedExchange<I, O, M>,
    pub(crate) input_stream: StreamCollector<I>,
    pub(crate) output_stream: StreamCollector<O>,
}

impl<I, O, M> BiDirectionalStreamingExchange<I, O, M>
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    pub fn set_output_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = Result<O, Box<dyn std::error::Error + Send + Sync>>> + Send + 'static,
    {
        self.output_stream.set_stream(stream)
    }

    pub async fn collect_output(&mut self) -> Result<&O, ExchangeError> {
        self.output_stream.collect_stream().await?;
        let input = match self.output_stream.collected.take() {
            None => todo!(),
            Some(collected) => collected,
        };
        self.exchange.save_output(input);
        self.exchange.output()
    }

    pub fn set_input_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = Result<I, Box<dyn std::error::Error + Send + Sync>>> + Send + 'static,
    {
        self.input_stream.set_stream(stream)
    }

    pub async fn collect_input(&mut self) -> Result<&I, ExchangeError> {
        self.input_stream.collect_stream().await?;
        let input = match self.input_stream.collected.take() {
            None => todo!(),
            Some(collected) => collected,
        };
        self.exchange.save_input(input);
        self.exchange.input()
    }
}
