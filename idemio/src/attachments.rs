use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use fnv::FnvHasher;

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

    /// Retrieves a reference to a typed attachment.
    ///
    /// # Examples
    /// ```rust
    /// use idemio::Attachments;
    ///
    /// let mut attachments = Attachments::new();
    /// attachments.add::<u32>("user_id", 123);
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

#[derive(PartialOrd, PartialEq, Hash, Eq)]
pub struct AttachmentKey {
    key_hash: u64,
    type_hash: u64,
}

impl AttachmentKey {
    pub fn new(key: impl AsRef<str>, type_id: TypeId) -> Self {
        let key_hash = Self::hash(key.as_ref());
        let type_hash = Self::hash(type_id);
        Self {
            key_hash,
            type_hash,
        }
    }
    fn hash(in_string: impl Hash) -> u64 {
        let mut hasher = FnvHasher::default();
        in_string.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::{json, Value};

    struct TestStruct;

    #[test]
    fn test_attachments() {
        let mut attachments = Attachments::new();
        let key1 = "test_key1";
        let key2 = "test_key2";
        let key3 = "test_key3";
        let key4 = "test_key4";
        let key5 = "test_key5";
        {
            attachments.add::<u64>(key1, 1);
            attachments.add::<String>(key2, String::from("test"));
            attachments.add::<bool>(key3, true);
            let test_struct = TestStruct;
            attachments.add::<TestStruct>(key4, test_struct);
            let map = json!({
                "test": "another_value",
                "some": "value"
            });
            attachments.add::<Value>(key5, map);
        }

        {
            let msg = attachments.get_mut::<String>(key2).expect("Should exist");
            *msg = String::from("Some other value...");
        }

        {
            assert!(attachments.get::<u64>(key1).is_some());
            assert!(attachments.get::<String>(key2).is_some_and(|string| string.eq("Some other value...")));
            assert!(attachments.get::<bool>(key3).is_some());
            assert!(attachments.get::<TestStruct>(key4).is_some());
            assert!(attachments.get::<Value>(key5).is_some());
        }
    }
}