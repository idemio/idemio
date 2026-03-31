use crate::attachments::Attachments;
use std::any::Any;
use std::hash::{Hash, Hasher};
use thiserror::Error;
use uuid::Uuid;

/// A generic container that manages data flow and attachments.
pub struct Exchange<T>
where
    T: Send + Sync,
{
    uuid: Uuid,
    inner: InnerData<T>,
    consume_listeners: Vec<Callback<T>>,
}

pub struct InnerData<T> {
    pub data: T,
    pub attachments: Attachments
}

impl<T> InnerData<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            attachments: Attachments::new()
        }
    }
}

impl<T: Send + Sync> From<(Uuid, InnerData<T>)> for Exchange<T> {
    fn from(value: (Uuid, InnerData<T>)) -> Self {
        Self {
            uuid: value.0,
            consume_listeners: Vec::new(),
            inner: value.1
        }
    }
}

impl<T: Send + Sync> Exchange<T> {
    /// Creates a new exchange instance with a randomly generated UUID.
    pub fn new(data: T) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            consume_listeners: Vec::new(),
            inner: InnerData {
                data,
                attachments: Attachments::new()
            }
        }
    }

    /// Returns a reference to the exchange's unique identifier.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns a reference to the attachments' collection.
    pub fn attachments(&self) -> &Attachments {
        &self.inner.attachments
    }

    /// Returns a mutable reference to the attachments' collection.
    pub fn attachments_mut(&mut self) -> &mut Attachments {
        &mut self.inner.attachments
    }

    /// Retrieves a reference to the stored input data.
    pub fn data(&self) -> &T {
        &self.inner.data
    }

    /// Returns a mutable reference to the stored input data.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.inner.data
    }

    /// Adds a callback listener for data processing.
    ///
    /// # Parameters
    /// - `callback`: A closure that takes `&mut I` and `&mut Attachments` and implements
    ///   `FnMut + Send + Sync + 'static`
    pub fn add_consume_listener(
        &mut self,
        callback: impl FnMut(&mut T, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.consume_listeners.push(Callback::new(callback));
    }

    /// Consumes and returns the stored data, executing all listeners for this exchange.
    ///
    pub fn take_data(mut self) -> (Uuid, InnerData<T>) {
        let uuid = self.uuid;
        let InnerData {
            data: val,
            attachments
        } = &mut self.inner;
        self.consume_listeners
            .drain(..)
            .for_each(|mut listener| listener.invoke(val, attachments));
        (uuid, self.inner)
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
    pub fn take_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::Take {
            uuid: *uuid,
            message: msg.into(),
        }
    }

    #[inline]
    pub fn callback_error(uuid: &Uuid, msg: impl Into<String>) -> Self {
        ExchangeError::Callback {
            uuid: *uuid,
            message: msg.into(),
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


