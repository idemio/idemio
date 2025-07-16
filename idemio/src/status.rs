use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

#[derive(Debug)]
pub struct HandlerExecutionError<'a> {
    pub message: Cow<'a, str>,
}

impl Display for HandlerExecutionError<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fatal error: {}", self.message)
    }
}

#[derive(Debug)]
pub struct HandlerStatus<'a> {
    pub(crate) code: ExchangeState,
    message: Option<Cow<'a, str>>,
    details: Option<Cow<'a, str>>,
}

impl<'a> HandlerStatus<'a> {
    pub fn code(&self) -> ExchangeState {
        self.code
    }
    pub fn message(&self) -> &str {
        match &self.message {
            Some(cow) => cow.as_ref(),
            None => "",
        }
    }


    pub fn details(&self) -> &str {
        match &self.details {
            Some(cow) => cow.as_ref(),
            None => "",
        }
    }


    pub fn new(code: ExchangeState) -> HandlerStatus<'a> {
        Self {
            code,
            message: None,
            details: None,
        }
    }

    pub fn set_message(mut self, message: &'static str) -> HandlerStatus<'a> {
        self.message = Some(Cow::Borrowed(message));
        self
    }

    pub fn set_details(mut self, description: &'static str) -> HandlerStatus<'a> {
        self.details = Some(Cow::Borrowed(description));
        self
    }

}

#[derive(Clone, Copy)]
#[derive(Debug)]
pub struct ExchangeState(pub i32);

impl ExchangeState {
    pub const OK: Self = Self(1);
    pub const REQUEST_COMPLETED: Self = Self(1 << 1);
    pub const RESPONSE_COMPLETED: Self = Self(1 << 2);
    pub const SERVER_ERROR: Self = Self(1 << 3);
    pub const CLIENT_ERROR: Self = Self(1 << 4);
    pub const DISABLED: Self = Self(1 << 5);
    pub const TIMEOUT: Self = Self(1 << 6);
    pub const CONTINUE: Self = Self(1 << 7);

    pub fn any_flags(&self, flags: ExchangeState) -> bool {
        self.0 & flags.0 != 0
    }

    pub fn any_flags_clear(&self, flags: ExchangeState) -> bool {
        self.0 & flags.0 != flags.0
    }

    pub fn all_flags(&self, flags: ExchangeState) -> bool {
        self.0 & flags.0 == flags.0
    }

    pub fn all_flags_clear(&self, flags: ExchangeState) -> bool {
        self.0 & flags.0 == 0
    }
    
    pub fn is_complete(&self) -> bool {
        self.all_flags(ExchangeState::REQUEST_COMPLETED | ExchangeState::RESPONSE_COMPLETED)
    }
    
    pub fn is_error(&self) -> bool {
        self.any_flags(
            ExchangeState::CLIENT_ERROR
                | ExchangeState::SERVER_ERROR
                | ExchangeState::TIMEOUT
        )
    }
}

impl Display for ExchangeState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:b}", self.0)
    }
}

impl PartialEq for ExchangeState {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl BitOrAssign for ExchangeState {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0
    }
}

impl BitAndAssign for ExchangeState {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0
    }
}

impl Not for ExchangeState {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

impl BitAnd for ExchangeState {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitOr for ExchangeState {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::status::ExchangeState;

    #[test]
    fn test_status_flags() {
        let mut test_code = ExchangeState::OK;
        test_code |= ExchangeState::REQUEST_COMPLETED;
        assert!(test_code.all_flags(ExchangeState::OK | ExchangeState::REQUEST_COMPLETED));
        assert!(test_code.all_flags_clear(ExchangeState::DISABLED | ExchangeState::TIMEOUT | ExchangeState::CONTINUE | ExchangeState::CLIENT_ERROR | ExchangeState::SERVER_ERROR));
    }

    #[test]
    fn test_status_constructs() {
        let test_code1 = ExchangeState::OK | ExchangeState::SERVER_ERROR;
        let test_code2 = ExchangeState::from(test_code1 | ExchangeState::REQUEST_COMPLETED);
        assert!(test_code2.all_flags(ExchangeState::OK | ExchangeState::SERVER_ERROR | ExchangeState::REQUEST_COMPLETED));
    }
}
