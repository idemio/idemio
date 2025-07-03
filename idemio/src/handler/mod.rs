pub mod registry;
pub mod config;

use std::fmt::Debug;
use crate::config::config::HandlerConfig;
use crate::router::exchange::Exchange;
use crate::status::{HandlerExecutionError, HandlerStatus};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
pub type SharedHandler<I, O, M> = Arc<dyn Handler<I, O, M> + Send + Sync>;

#[async_trait]
pub trait Handler<I, O, M>: Send
where
    Self: Debug,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    async fn exec(
        &self,
        exchange: &mut Exchange<I, O, M>,
    ) -> Result<HandlerStatus, HandlerExecutionError>;
}

#[derive(Debug)]
pub struct IdemioHandler<I, O, M, C>
where
    C: Send + Sync + Default + DeserializeOwned + Debug,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    inner: SharedHandler<I, O, M>,
    config: HandlerConfig<C>,
}

impl<I, O, M, C> IdemioHandler<I, O, M, C>
where
    C: Send + Sync + Default + DeserializeOwned + Debug,
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    pub fn new(inner: SharedHandler<I, O, M>, config: HandlerConfig<C>) -> Self {
        Self { inner, config }
    }

    pub fn config(&self) -> &HandlerConfig<C> {
        &self.config
    }

    pub fn name(&self) -> &str {
        &self.config.id()
    }

    async fn exec_with_timeout(
        &self,
        exchange: &mut Exchange<I, O, M>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        match self.config.timeout() {
            None => self.inner.exec(exchange).await,
            Some(duration) => {
                let duration = Duration::from_millis(duration);
                match tokio::time::timeout(duration, self.inner.exec(exchange)).await {
                    Ok(result) => result,
                    Err(_) => {
                        // Create a timeout error
                        let error_msg = format!(
                            "Handler '{}' timed out after {}ms",
                            self.config.id(),
                            self.config.timeout().unwrap_or(0)
                        );
                        Err(HandlerExecutionError {
                            message: error_msg.into(),
                        })
                    }
                }
            }
        }
    }

    async fn exec_with_retry(
        &self,
        exchange: &mut Exchange<I, O, M>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        let max_attempts = self
            .config
            .retry_count()
            .map(|attempt_count| attempt_count + 1)
            .unwrap_or(1);

        let mut last_error = None;
        for attempt in 1..=max_attempts {
            let retry_delay = Duration::from_millis(self.config.retry_delay().unwrap_or(0));
            match self.exec_with_timeout(exchange).await {
                Ok(status) => {
                    if status.code.is_error() && attempt < max_attempts {
                        if !retry_delay.is_zero() {
                            sleep(retry_delay).await;
                        }
                        continue;
                    }
                    return Ok(status);
                }
                Err(error) => {
                    last_error = Some(error);
                    if attempt < max_attempts && !retry_delay.is_zero() {
                        sleep(retry_delay).await;
                    }
                }
            }
        }

        // Return the last error if all retries failed
        Err(last_error.unwrap_or_else(|| HandlerExecutionError {
            message: format!(
                "Handler '{}' failed after {} attempts",
                self.config.id(),
                max_attempts
            )
            .into(),
        }))
    }
}

#[async_trait]
impl<I, O, M, C> Handler<I, O, M> for IdemioHandler<I, O, M, C>
where
    C: Send + Sync + Default + DeserializeOwned + Debug,
    I: Send + Sync + Debug,
    O: Send + Sync + Debug,
    M: Send + Sync + Debug,
{
    async fn exec(
        &self,
        exchange: &mut Exchange<I, O, M>,
    ) -> Result<HandlerStatus, HandlerExecutionError> {
        if !self.config.enabled() {
            return Ok(HandlerStatus::new(crate::status::Code::DISABLED));
        }

        self.exec_with_retry(exchange).await
    }
}

#[cfg(test)]
mod test {
    use crate::config::config::{HandlerConfig, HandlerConfigBuilder};
    use crate::router::exchange::Exchange;
    use crate::handler::{Handler, IdemioHandler};
    use crate::status::{Code, HandlerExecutionError, HandlerStatus};
    use async_trait::async_trait;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    use tokio::time::Instant;

    // Test configuration struct
    #[derive(Debug, Default, Serialize, Deserialize)]
    struct TestConfig {
        test_value: u32,
    }

    #[derive(Debug)]
    // Mock handler that always succeeds
    struct SuccessHandler;

    #[async_trait]
    impl Handler<String, String, ()> for SuccessHandler {
        async fn exec(
            &self,
            exchange: &mut Exchange<String, String, ()>,
        ) -> Result<HandlerStatus, HandlerExecutionError> {
            let input = exchange.take_request().unwrap();
            exchange.save_output(format!("processed: {}", input));
            Ok(HandlerStatus::new(Code::OK))
        }
    }

    #[derive(Debug)]
    // Mock handler that always fails
    struct FailureHandler;

    #[async_trait]
    impl Handler<String, String, ()> for FailureHandler {
        async fn exec(
            &self,
            _exchange: &mut Exchange<String, String, ()>,
        ) -> Result<HandlerStatus, HandlerExecutionError> {
            Err(HandlerExecutionError {
                message: "Test failure".into(),
            })
        }
    }

    #[derive(Debug)]
    // Mock handler that fails a certain number of times then succeeds
    struct RetryableHandler {
        call_count: AtomicU32,
        fail_count: u32,
    }

    impl RetryableHandler {
        fn new(fail_count: u32) -> Self {
            Self {
                call_count: AtomicU32::new(0),
                fail_count,
            }
        }
    }

    #[async_trait]
    impl Handler<String, String, ()> for RetryableHandler {
        async fn exec(
            &self,
            exchange: &mut Exchange<String, String, ()>,
        ) -> Result<HandlerStatus, HandlerExecutionError> {
            let current_call = self.call_count.fetch_add(1, Ordering::SeqCst);

            if current_call < self.fail_count {
                Err(HandlerExecutionError {
                    message: format!("Failure #{}", current_call + 1).into(),
                })
            } else {
                let input = exchange.take_request().unwrap();
                exchange.save_output(format!("retry success: {}", input));
                Ok(HandlerStatus::new(Code::OK))
            }
        }
    }

    #[derive(Debug)]
    // Mock handler that returns error status codes
    struct ErrorStatusHandler {
        status_code: Code,
    }


    impl ErrorStatusHandler {
        fn new(status_code: Code) -> Self {
            Self { status_code }
        }
    }

    #[async_trait]
    impl Handler<String, String, ()> for ErrorStatusHandler {
        async fn exec(
            &self,
            _exchange: &mut Exchange<String, String, ()>,
        ) -> Result<HandlerStatus, HandlerExecutionError> {
            Ok(HandlerStatus::new(self.status_code))
        }
    }

    #[derive(Debug)]
    // Mock handler that takes time to execute
    struct SlowHandler {
        delay_ms: u64,
    }
    
    impl SlowHandler {
        fn new(delay_ms: u64) -> Self {
            Self { delay_ms }
        }
    }

    #[async_trait]
    impl Handler<String, String, ()> for SlowHandler {
        async fn exec(
            &self,
            exchange: &mut Exchange<String, String, ()>,
        ) -> Result<HandlerStatus, HandlerExecutionError> {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            let input = exchange.take_request().unwrap();
            exchange.save_output(format!("slow processed: {}", input));
            Ok(HandlerStatus::new(Code::OK))
        }
    }

    fn create_test_exchange() -> Exchange<String, String, ()> {
        let mut exchange = Exchange::new();
        exchange.save_input("test_input".to_string());
        exchange
    }

    fn create_enabled_config() -> HandlerConfig<TestConfig> {
        let mut binding = HandlerConfigBuilder::<TestConfig>::new();
        binding.id("enabled_handler".to_string()).enabled(true);
        binding.build()
    }

    fn create_disabled_config() -> HandlerConfig<TestConfig> {
        let mut binding = HandlerConfigBuilder::<TestConfig>::new();
        binding.id("disabled".to_string()).enabled(false);
        binding.build()
    }

    #[tokio::test]
    async fn test_exec_with_enabled_handler_success() {
        // Arrange
        let handler = Arc::new(SuccessHandler);
        let config = create_enabled_config();
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::OK);

        let output = exchange.output().unwrap();
        assert_eq!(output, "processed: test_input");
    }

    #[tokio::test]
    async fn test_exec_with_disabled_handler() {
        // Arrange
        let handler = Arc::new(SuccessHandler);
        let config = create_disabled_config();
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::DISABLED);

        // Verify the inner handler was not called (exchange still has input)
        assert!(exchange.input().is_ok());
    }

    #[tokio::test]
    async fn test_exec_with_handler_failure() {
        // Arrange
        let handler = Arc::new(FailureHandler);
        let config = create_enabled_config();
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.message.contains("Test failure"));
    }

    #[tokio::test]
    async fn test_exec_with_retry_success() {
        // Arrange
        let handler = Arc::new(RetryableHandler::new(2)); // Fail 2 times, then succeed
        let mut config = create_enabled_config();
        config.set_retry_count(Some(3));
        config.set_retry_delay(Some(10)); // Short delay for testing
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::OK);

        let output = exchange.output().unwrap();
        assert_eq!(output, "retry success: test_input");
    }

    #[tokio::test]
    async fn test_exec_with_retry_exhausted() {
        // Arrange
        let handler = Arc::new(RetryableHandler::new(5)); // Fail 5 times
        let mut config = create_enabled_config();
        config.set_retry_count(Some(2)); // Only allow 2 retries (3 total attempts)
        config.set_retry_delay(Some(1)); // Very short delay
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.message.contains("Failure #3"));
    }

    #[tokio::test]
    async fn test_exec_with_timeout_success() {
        // Arrange
        let handler = Arc::new(SlowHandler::new(50)); // 50ms delay
        let mut config = create_enabled_config();
        config.set_timeout(Some(200)); // 200ms timeout
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::OK);
    }

    #[tokio::test]
    async fn test_exec_with_timeout_exceeded() {
        // Arrange
        let handler = Arc::new(SlowHandler::new(200)); // 200ms delay
        let mut config = create_enabled_config();
        config.set_id("test_handler".to_string());
        config.set_timeout(Some(50)); // 50ms timeout
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let start = Instant::now();
        let result = configurable_handler.exec(&mut exchange).await;
        let duration = start.elapsed();

        // Assert
        assert!(result.is_err());
        assert!(duration.as_millis() < 100); // Should timeout quickly

        let error = result.unwrap_err();
        assert!(error.message.contains("timed out"));
        assert!(error.message.contains("test_handler"));
    }

    #[tokio::test]
    async fn test_exec_with_error_status_retry() {
        // Arrange
        let handler = Arc::new(ErrorStatusHandler::new(Code::SERVER_ERROR));
        let mut config = create_enabled_config();
        config.set_retry_count(Some(2));
        config.set_retry_delay(Some(10));
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::SERVER_ERROR);
        // Should have retried but eventually returned the error status
    }

    #[tokio::test]
    async fn test_exec_with_non_error_status_no_retry() {
        // Arrange
        let handler = Arc::new(ErrorStatusHandler::new(Code::CONTINUE));
        let mut config = create_enabled_config();
        config.set_retry_count(Some(2));
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let start = Instant::now();
        let result = configurable_handler.exec(&mut exchange).await;
        let duration = start.elapsed();

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::CONTINUE);
        // Should not have retried since CONTINUE is not an error
        assert!(duration.as_millis() < 50); // Should complete quickly
    }

    #[tokio::test]
    async fn test_exec_without_retry_config() {
        // Arrange
        let handler = Arc::new(FailureHandler);
        let config = create_enabled_config(); // No retry count set
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_err());
        // Should fail immediately without retries
    }

    #[tokio::test]
    async fn test_exec_without_timeout_config() {
        // Arrange
        let handler = Arc::new(SlowHandler::new(100)); // 100ms delay
        let config = create_enabled_config(); // No timeout set
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::OK);
        // Should complete successfully without timeout
    }

    #[tokio::test]
    async fn test_exec_with_zero_retry_delay() {
        // Arrange
        let handler = Arc::new(RetryableHandler::new(1)); // Fail once, then succeed
        let mut config = create_enabled_config();
        config.set_retry_count(Some(2));
        config.set_retry_delay(Some(0)); // No delay between retries
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let start = Instant::now();
        let result = configurable_handler.exec(&mut exchange).await;
        let duration = start.elapsed();

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::OK);
        // Should complete quickly without retry delays
        assert!(duration.as_millis() < 50);
    }

    #[tokio::test]
    async fn test_exec_complex_scenario() {
        // Arrange: Handler that fails twice with timeout and retry
        let handler = Arc::new(RetryableHandler::new(2));
        let mut config = create_enabled_config();
        config.set_timeout(Some(1000)); // 1 second timeout
        config.set_retry_count(Some(3)); // 3 retries
        config.set_retry_delay(Some(20)); // 20ms delay
        let configurable_handler = IdemioHandler::new(handler, config);
        let mut exchange = create_test_exchange();

        // Act
        let result = configurable_handler.exec(&mut exchange).await;

        // Assert
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.code(), Code::OK);

        let output = exchange.output().unwrap();
        assert_eq!(output, "retry success: test_input");
    }
}
