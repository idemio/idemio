use async_trait::async_trait;
use crate::exchange::Exchange;
use crate::status::{HandlerExecutionError, HandlerStatus};

#[async_trait]
pub trait Handler<Input, Output, Metadata>: Send
where
    Input: Send,
    Output: Send,
    Metadata: Send,
{
    async fn exec(
        &self,
        exchange: & mut Exchange<Input, Output, Metadata>,
    ) -> Result<HandlerStatus, HandlerExecutionError>;
}

#[cfg(test)]
mod test {
    use async_trait::async_trait;
    use crate::exchange::Exchange;
    use crate::handler::Handler;
    use crate::status::{Code, HandlerExecutionError, HandlerStatus};

    pub struct TestHandler;

    #[derive(Default)]
    pub struct TestInput
    where
        Self: Send,
    {
        pub test_input: String,
    }

    #[derive(Default)]
    pub struct TestOutput
    where
        Self: Send,
    {
        pub test_output: String,
    }

    #[async_trait]
    impl Handler<TestInput, TestOutput, ()> for TestHandler {
        async fn exec(
            &self,
            exchange: &mut Exchange<TestInput, TestOutput, ()>,
        ) -> Result<HandlerStatus, HandlerExecutionError>
        {
            // Simple exchange of input and output values
            let consumed_input = exchange.take_request().unwrap();
            let mut output = TestOutput::default();
            output.test_output = consumed_input.test_input;
            exchange.save_output(output);
            Ok(HandlerStatus::new(Code::OK))
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_handler() {
        let handler = TestHandler;
        let mut exchange: Exchange<TestInput, TestOutput, ()> = Exchange::new();
        let mut input = TestInput::default();
        input.test_input = "test".to_string();
        exchange.save_input(input);
        handler.exec(&mut exchange).await.expect("Failed to execute test");
        let output = exchange.take_output().unwrap();
        assert_eq!(output.test_output, "test");
    }
}
