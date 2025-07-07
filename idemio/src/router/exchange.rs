use fnv::FnvHasher;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use uuid::Uuid;

pub struct Exchange<I, O, M> {
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
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        Self {
            uuid,
            metadata: None,
            input: None,
            output: None,
            input_listeners: vec![],
            output_listeners: vec![],
            attachments: Attachments::new(),
        }
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn add_metadata(&mut self, metadata: M) {
        self.metadata = Some(metadata);
    }

    pub fn attachments(&self) -> &Attachments {
        &self.attachments
    }

    pub fn attachments_mut(&mut self) -> &mut Attachments {
        &mut self.attachments
    }

    pub fn add_input_listener(
        &mut self,
        callback: impl FnMut(&mut I, &mut Attachments) + Send + 'static,
    ) {
        self.input_listeners.push(Callback::new(callback));
    }

    pub fn add_output_listener(
        &mut self,
        callback: impl FnMut(&mut O, &mut Attachments) + Send + 'static,
    ) {
        self.output_listeners.push(Callback::new(callback));
    }

    fn execute_input_callbacks(&mut self) -> Result<(), ExchangeError> {
        if let Some(input) = &mut self.input {
            for mut callback in &mut self.input_listeners.drain(..) {
                callback.invoke(input, &mut self.attachments);
            }
            Ok(())
        } else {
            Err(ExchangeError::input_callback_error(
                &self.uuid,
                "Exchange does not contain an input to perform callbacks on",
            ))
        }
    }

    fn execute_output_callbacks(&mut self) -> Result<(), ExchangeError> {
        if let Some(output) = &mut self.output {
            for mut callback in &mut self.output_listeners.drain(..) {
                callback.invoke(output, &mut self.attachments);
            }
            Ok(())
        } else {
            Err(ExchangeError::output_callback_error(
                &self.uuid,
                "Exchange does not contain an output to perform callbacks on",
            ))
        }
    }

    pub fn save_input(&mut self, request: I) {
        self.input = Some(request);
    }

    pub fn input(&self) -> Result<&I, ExchangeError> {
        match &self.input {
            Some(out) => Ok(out),
            None => Err(ExchangeError::input_read_error(
                &self.uuid,
                "Exchange does not contain any input data to read.",
            )),
        }
    }

    pub fn input_mut(&mut self) -> Result<&mut I, ExchangeError> {
        match &mut self.input {
            Some(out) => Ok(out),
            None => Err(ExchangeError::input_read_error(
                &self.uuid,
                "Exchange does not contain any input data to modify.",
            )),
        }
    }

    pub fn take_input(&mut self) -> Result<I, ExchangeError> {
        match self.execute_input_callbacks() {
            Ok(_) => self.input.take().ok_or_else(|| {
                ExchangeError::input_take_error(
                    &self.uuid,
                    "Exchange does not contain any input data to consume.",
                )
            }),
            Err(e) => Err(e),
        }
    }

    pub fn save_output(&mut self, response: O) {
        self.output = Some(response);
    }

    pub fn output(&self) -> Result<&O, ExchangeError> {
        match &self.output {
            Some(out) => Ok(out),
            None => Err(ExchangeError::output_read_error(
                &self.uuid,
                "Exchange contains no output data to read.",
            )),
        }
    }

    pub fn output_mut(&mut self) -> Result<&mut O, ExchangeError> {
        match &mut self.output {
            Some(out) => Ok(out),
            None => Err(ExchangeError::output_read_error(
                &self.uuid,
                "Exchange does not contain any output data to modify.",
            )),
        }
    }

    pub fn take_output(&mut self) -> Result<O, ExchangeError> {
        match self.execute_output_callbacks() {
            Ok(_) => self.output.take().ok_or_else(|| {
                ExchangeError::output_read_error(
                    &self.uuid,
                    "Exchange does not contain any output data to consume.",
                )
            }),
            Err(e) => Err(e),
        }
    }
}

pub struct Attachments {
    attachments: HashMap<AttachmentKey, Box<dyn Any + Send + Sync>, fnv::FnvBuildHasher>,
}

impl Attachments {
    pub fn new() -> Self {
        Self {
            attachments: HashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    pub fn add<K>(&mut self, key: impl AsRef<str>, value: K)
    where
        K: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<K>();
        self.attachments
            .insert(AttachmentKey::new(key, type_id), Box::new(value));
    }

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
    InputReadError(Uuid, String),
    InputTakeError(Uuid, String),
    OutputReadError(Uuid, String),
    OutputTakeError(Uuid, String),
    InputCallbackError(Uuid, String),
    OutputCallbackError(Uuid, String),
}

impl ExchangeError {
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
    callback: Box<dyn FnMut(&mut T, &mut Attachments) + Send>,
}

impl<T> Callback<T>
where
    T: Send,
{
    pub fn new(callback: impl FnMut(&mut T, &mut Attachments) + Send + 'static) -> Self {
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
    use crate::router::exchange::Exchange;

    struct TestStruct;

    #[test]
    fn test_attachments() {
        let mut exchange: Exchange<(), (), ()> = Exchange::new();
        let key1 = "test_key1";
        let key2 = "test_key2";
        let key3 = "test_key3";
        let key4 = "test_key4";
        {
            exchange.attachments_mut().add::<u64>(key1, 1);
            exchange
                .attachments_mut()
                .add::<String>(key2, String::from("test"));
            exchange.attachments_mut().add::<bool>(key3, true);
            let test_struct = TestStruct;
            exchange
                .attachments_mut()
                .add::<TestStruct>(key4, test_struct);
        }

        {
            assert!(exchange.attachments().get::<u64>(key1).is_some());
            assert!(exchange.attachments().get::<String>(key2).is_some());
            assert!(exchange.attachments().get::<bool>(key3).is_some());
            assert!(exchange.attachments().get::<TestStruct>(key4).is_some());
        }
    }

    #[test]
    fn test_callbacks() {
        let mut exchange: Exchange<String, (), ()> = Exchange::new();
        exchange.save_input(String::from("hello"));
        exchange.add_input_listener(|input, _| {
            input.push_str(" world!");
        });
        exchange.add_input_listener(|input, _| {
            input.push_str(" world!");
        });
        exchange.add_input_listener(|input, _| {
            input.push_str(" world!");
        });
        let request = exchange.take_input().unwrap();
        assert_eq!(request, "hello world! world! world!");
    }

    #[test]
    fn test_consume() {
        let mut exchange: Exchange<String, (), ()> = Exchange::new();

        // consume before adding data
        let invalid_consume = exchange.take_input();
        assert!(invalid_consume.is_err());

        // consume after adding data
        exchange.save_input(String::from("hello"));
        let request = exchange.take_input().unwrap();
        assert_eq!(request, "hello");

        // consume again
        let invalid_consume = exchange.take_input();
        assert!(invalid_consume.is_err());
    }
}
