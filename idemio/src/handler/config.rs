use std::borrow::Cow;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChainId {
    chain_hash: u64,
}

impl ChainId {
    pub fn new(id: impl Into<String>) -> Self {
        let mut hasher = fnv::FnvHasher::default();
        id.into().hash(&mut hasher);
        let hash = hasher.finish();
        Self { chain_hash: hash }
    }
}

impl Display for ChainId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChainId({})", self.chain_hash)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandlerId {
    handler_hash: u64,
}

impl<'a> From<&'a HandlerId> for Cow<'a, HandlerId> {
    fn from(value: &'a HandlerId) -> Self {
        Cow::Borrowed(value)
    }
}

impl HandlerId {
    pub fn new(id: impl Into<String>) -> Self {
        let mut hasher = fnv::FnvHasher::default();
        id.into().hash(&mut hasher);
        let hash = hasher.finish();
        Self { handler_hash: hash }
    }
}

impl Display for HandlerId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "HandlerId({})", self.handler_hash)
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum Executable {
    Handler(HandlerId),
    Chain(ChainId),
}

impl Display for Executable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Executable::Handler(id) => write!(f, "Handler({})", id),
            Executable::Chain(id) => write!(f, "Chain({})", id),
        }
    }
}

impl FromStr for Executable {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("@") {
            Ok(Executable::Chain(ChainId::new(s)))
        } else {
            Ok(Executable::Handler(HandlerId::new(s)))
        }
    }
}