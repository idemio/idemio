use std::fmt::Formatter;

#[derive(Debug)]
pub struct HandlerStatus {
    pub(crate) state: ExchangeState,
    pub(crate) message: Option<String>,
    pub(crate) details: Option<String>,
}

impl HandlerStatus {
    pub fn state(&self) -> ExchangeState {
        self.state
    }
    
    #[inline]
    pub fn new(state: ExchangeState) -> HandlerStatus {
        Self {
            state,
            message: None,
            details: None,
        }
    }

    #[inline]
    pub fn message(mut self, message: impl Into<String>) -> HandlerStatus {
        self.message = Some(message.into());
        self
    }

    #[inline]
    pub fn details(mut self, details: impl Into<String>) -> HandlerStatus {
        self.details = Some(details.into());
        self
    }

}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExchangeState {
    LIVE,
    COMPLETED,
    ERROR
}

impl std::fmt::Display for ExchangeState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExchangeState::LIVE => write!(f, "LIVE"),
            ExchangeState::COMPLETED => write!(f, "COMPLETED"),
            ExchangeState::ERROR => write!(f, "ERROR"),
        }
    }
}
