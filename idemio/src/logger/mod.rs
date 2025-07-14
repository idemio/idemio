use std::panic::Location;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LogCode {}

#[derive(Debug, Serialize, Deserialize)]
pub enum LogLevel {
    ERROR,
    WARN,
    INFO,
    DEBUG,
    TRACE,
}

#[derive(Debug, Serialize, Deserialize)]
enum Component {
    Router,
    Handler,
    Cache,
    Config,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry<'a> {
    timestamp: String,
    level: LogLevel,
    code: LogCode,
    thread_id: Option<&'a str>,
    file: Option<&'a str>,
    line: Option<u32>,
    column: Option<u32>,
    exchange_id: Uuid,
    component: Component,
    message: &'a str,
}

impl<'a> LogEntry<'a> {
    fn new(log_level: LogLevel, code: LogCode, thread_id: Option<&'a str>, message: &'a impl AsRef<str>, component: Component, uuid: &Uuid, location: Option<&'a Location>) -> Self {
        match location {
            None => {
                Self {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    level: log_level,
                    thread_id,
                    code,
                    file: None,
                    line: None,
                    column: None,
                    exchange_id: *uuid,
                    component,
                    message: message.as_ref(),
                }
            }
            Some(location) => {
                Self {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    level: log_level,
                    thread_id,
                    code,
                    file: Some(location.file()),
                    line: Some(location.line()),
                    column: Some(location.column()),
                    exchange_id: *uuid,
                    component,
                    message: message.as_ref(),
                }
            }
        }
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or(String::from("Error serializing log entry"))
    }
}

struct IdemioLogger;
impl IdemioLogger {
    pub fn log() {
        let loc = Location::caller();
    }
}