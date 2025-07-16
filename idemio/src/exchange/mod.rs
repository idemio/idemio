pub mod unified;
pub mod collector;

use fnv::FnvHasher;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use uuid::Uuid;

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
    ExchangeCompleted(Uuid),
    MetadataReadError(Uuid, String),
    InputReadError(Uuid, String),
    InputTakeError(Uuid, String),
    OutputReadError(Uuid, String),
    OutputTakeError(Uuid, String),
    InputCallbackError(Uuid, String),
    OutputCallbackError(Uuid, String),
}

impl ExchangeError {

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
    use crate::exchange::Attachments;

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
            attachments
                .add::<String>(key2, String::from("test"));
            attachments.add::<bool>(key3, true);
            let test_struct = TestStruct;
            attachments
                .add::<TestStruct>(key4, test_struct);
        }

        {
            assert!(attachments.get::<u64>(key1).is_some());
            assert!(attachments.get::<String>(key2).is_some());
            assert!(attachments.get::<bool>(key3).is_some());
            assert!(attachments.get::<TestStruct>(key4).is_some());
        }
    }
}
