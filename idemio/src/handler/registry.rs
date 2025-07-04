use dashmap::{DashMap, Entry};
use crate::handler::{SharedHandler};
use crate::handler::config::HandlerId;

pub struct HandlerRegistry<I, O, M> {
    handlers: DashMap<HandlerId, SharedHandler<I, O, M>, fnv::FnvBuildHasher>
}

impl<I, O, M> HandlerRegistry<I, O, M> {
    pub fn new() -> Self {
        Self {
            handlers: DashMap::with_hasher(fnv::FnvBuildHasher::default())
        }
    }
    
    pub fn find(&self, id: impl Into<String>) -> Result<SharedHandler<I, O, M>, ()> {
        let id = HandlerId::new(id);
        self.find_with_id(&id)
    }
    
    pub fn find_with_id(&self, id: &HandlerId) -> Result<SharedHandler<I, O, M>, ()> {
        match self.handlers.get(id) {
            None => todo!("Handle missing handler"),
            Some(handler) => {
                Ok(handler.value().clone())
            }
        }
    }
    
    pub fn register_handler(&mut self, handler_id: impl Into<String>, handler: SharedHandler<I, O, M>) -> Result<(), ()> {
        let handler_id = HandlerId::new(handler_id);
        match self.handlers.entry(handler_id) {
            Entry::Occupied(_) => todo!("Implement conflicting id error"),
            Entry::Vacant(entry) => {
                entry.insert(handler);
            }
        }
        Ok(())
    }
}