use crate::handler::SharedHandler;
use crate::handler::config::HandlerId;
use dashmap::{DashMap, Entry};
use std::borrow::Cow;
use std::fmt::{Debug, Display, Formatter};

#[derive(Debug)]
pub enum HandlerRegistryError<'a> {
    MissingHandler(Cow<'a, HandlerId>),
    ConflictingHandlerId(Cow<'a, HandlerId>),
}

impl<'a> HandlerRegistryError<'a> {
    pub fn missing_handler(id: &'a HandlerId) -> Self {
        Self::MissingHandler(id.into())
    }
    
    pub fn conflicting_handler_id(id: &'a HandlerId) -> Self {
        Self::ConflictingHandlerId(id.into())
    }
}

impl Display for HandlerRegistryError<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HandlerRegistryError::MissingHandler(id) => {
                write!(f, "Could not find a handler with the id '{}'.", id)
            }
            HandlerRegistryError::ConflictingHandlerId(id) => {
                write!(f, "A handler with the id '{}' already exists.", id)
            }
        }
    }
}

impl std::error::Error for HandlerRegistryError<'_> {}

type RegistrationResult<'a> = Result<(), HandlerRegistryError<'a>>;
type FindResult<'a, I, O, M> = Result<SharedHandler<I, O, M>, HandlerRegistryError<'a>>;

pub struct HandlerRegistry<I, O, M> {
    handlers: DashMap<HandlerId, SharedHandler<I, O, M>, fnv::FnvBuildHasher>,
}

impl<I, O, M> HandlerRegistry<I, O, M> {
    pub fn new() -> Self {
        Self {
            handlers: DashMap::with_hasher(fnv::FnvBuildHasher::default()),
        }
    }

    pub fn find_with_id<'a>(
        &self,
        id: &'a HandlerId,
    ) -> FindResult<'a, I, O, M> {
        match self.handlers.get(id) {
            None => Err(HandlerRegistryError::missing_handler(id)),
            Some(handler) => Ok(handler.value().clone()),
        }
    }

    pub fn register_handler<'a>(
        &mut self,
        handler_id: &'a HandlerId,
        handler: SharedHandler<I, O, M>,
    ) -> RegistrationResult<'a> {
        match self.handlers.entry(handler_id.clone()) {
            Entry::Occupied(_) => Err(HandlerRegistryError::conflicting_handler_id(handler_id)),
            Entry::Vacant(entry) => {
                entry.insert(handler);
                Ok(())
            }
        }
    }
}
