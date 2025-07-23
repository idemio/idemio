use std::fmt::{Binary, Display, Formatter};
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};


#[derive(Debug)]
pub struct HandlerStatus {
    pub(crate) code: ExchangeState,
    pub(crate) message: Option<String>,
    pub(crate) details: Option<String>,
}

impl HandlerStatus {
    pub fn code(&self) -> ExchangeState {
        self.code
    }
    
    pub fn new(code: ExchangeState) -> HandlerStatus {
        Self {
            code,
            message: None,
            details: None,
        }
    }

    pub fn message(mut self, message: impl Into<String>) -> HandlerStatus {
        self.message = Some(message.into());
        self
    }

    pub fn details(mut self, details: impl Into<String>) -> HandlerStatus {
        self.details = Some(details.into());
        self
    }

}

#[derive(Clone, Copy, Debug)]
pub struct ExchangeState(pub i32);

impl Binary for ExchangeState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let val = self.0;
        Binary::fmt(&val, f)
    }
}

impl ExchangeState {
    pub const OK: Self = Self(1);
    pub const EXCHANGE_COMPLETED: Self = Self(1 << 1);
    pub const SERVER_ERROR: Self = Self(1 << 2);
    pub const CLIENT_ERROR: Self = Self(1 << 3);
    pub const DISABLED: Self = Self(1 << 4);
    pub const TIMEOUT: Self = Self(1 << 5);
    pub const CONTINUE: Self = Self(1 << 6);

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
    
    pub fn is_completed(&self) -> bool {
        self.any_flags(ExchangeState::EXCHANGE_COMPLETED)
    }
    
    pub fn is_in_flight(&self) -> bool {
        self.any_flags(ExchangeState::OK) && !self.is_error() && !self.is_completed()
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
        test_code |= ExchangeState::EXCHANGE_COMPLETED;
        assert!(test_code.all_flags(ExchangeState::OK | ExchangeState::EXCHANGE_COMPLETED));
        assert!(test_code.all_flags_clear(ExchangeState::DISABLED | ExchangeState::TIMEOUT | ExchangeState::CONTINUE | ExchangeState::CLIENT_ERROR | ExchangeState::SERVER_ERROR));
    }

    #[test]
    fn test_status_constructs() {
        let test_code1 = ExchangeState::OK | ExchangeState::SERVER_ERROR;
        let test_code2 = ExchangeState::from(test_code1 | ExchangeState::EXCHANGE_COMPLETED);
        assert!(test_code2.all_flags(ExchangeState::OK | ExchangeState::SERVER_ERROR | ExchangeState::EXCHANGE_COMPLETED));
    }
}
