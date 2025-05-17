use crate::exchange::Exchange;
use crate::handler::Handler;
use crate::status::Code;
use fnv::FnvHashMap;
use std::sync::Arc;

pub struct HandlerExecutor<Input, Output, Metadata> {
    pub handlers: FnvHashMap<u64, Arc<dyn Handler<Input, Output, Metadata> + Send>>,
}

impl<Input, Output, Metadata> HandlerExecutor<Input, Output, Metadata>
where
    Input: Default + Send + 'static,
    Output: Default + Send + 'static,
    Metadata: Send,
{
    pub fn new() -> HandlerExecutor<Input, Output, Metadata> {
        HandlerExecutor {
            handlers: FnvHashMap::default(),
        }
    }

    pub fn add_handler(
        &mut self,
        handler_id: u64,
        handler: Arc<dyn Handler<Input, Output, Metadata> + Send>,
    ) -> &mut Self {
        self.handlers.insert(handler_id, handler);
        self
    }

    pub async fn handle(
        &self,
        handler_chain: &Vec<u64>,
        input: Input,
        metadata: Option<Metadata>,
    ) -> Result<Output, ()> {
        let mut exchange: Exchange<Input, Output, Metadata> = Exchange::new();
        exchange.save_input(input);
        if let Some(metadata) = metadata {
            exchange.add_metadata(metadata);
        }

        for handler_id in handler_chain {
            if let Some(handler) = self.handlers.get(&handler_id) {
                match handler.exec(&mut exchange).await {
                    Ok(status) => {
                        if status.code.all_flags(Code::REQUEST_COMPLETED) {
                           if let Ok(output) = exchange.take_output() {
                               return Ok(output);
                           }
                        } else if status.code.any_flags(
                            Code::CLIENT_ERROR
                                | Code::SERVER_ERROR
                                | Code::TIMEOUT
                                | Code::REQUEST_COMPLETED,
                        ) {
                            todo!("Handle error in chain")
                        }
                        // continue handler execution
                    }
                    Err(_e) => {
                        todo!("Handle handler failed execution")
                    },
                }
            }
        }

        todo!("Handle handler chain not completing request")
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;
    use async_trait::async_trait;
    use crate::exchange::Exchange;
    use crate::executor::HandlerExecutor;
    use crate::handler::Handler;
    use crate::status::{Code, HandlerExecutionError, HandlerStatus};

    struct ModifyBodyHandler;

    #[async_trait]
    impl Handler<String, String, ()> for ModifyBodyHandler {
        async fn exec(&self, exchange: &mut Exchange<String, String, ()>) -> Result<HandlerStatus, HandlerExecutionError> {
            let request_body = exchange.input_mut().unwrap();
            let new_body = request_body.replace("Hello", "Goodbye");
            *request_body = new_body;
            Ok(HandlerStatus::new(Code::OK))
        }
    }
    struct IdempotentHandler1;

    #[async_trait]
    impl Handler<String, String, ()> for IdempotentHandler1 {
        async fn exec(&self, exchange: &mut Exchange<String, String, ()>) -> Result<HandlerStatus, HandlerExecutionError> {
            println!("Some logging or something");
            Ok(HandlerStatus::new(Code::OK))
        }
    }

    struct IdempotentHandler2;

    #[async_trait]
    impl Handler<String, String, ()> for IdempotentHandler2 {
        async fn exec(&self, exchange: &mut Exchange<String, String, ()>) -> Result<HandlerStatus, HandlerExecutionError> {
            println!("Some other idempotent operation");
            Ok(HandlerStatus::new(Code::OK))
        }
    }

    struct RequestCompletingHandler;

    #[async_trait]
    impl Handler<String, String, ()> for RequestCompletingHandler {
        async fn exec(&self, exchange: &mut Exchange<String, String, ()>) -> Result<HandlerStatus, HandlerExecutionError> {
            let response = exchange.take_request().unwrap();
            exchange.save_output(response);
            Ok(HandlerStatus::new(Code::REQUEST_COMPLETED))
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_valid_handler_chain() {
        let mut handler_executor: HandlerExecutor<String, String, ()> = HandlerExecutor::new();

        // load handlers in any order.
        handler_executor.add_handler(1, Arc::new(IdempotentHandler1));
        handler_executor.add_handler(2, Arc::new(IdempotentHandler2));
        handler_executor.add_handler(3, Arc::new(RequestCompletingHandler));
        handler_executor.add_handler(4, Arc::new(ModifyBodyHandler));

        // choose the handlers to be executed in order.
        let chain_for_request: Vec<u64> = vec![1, 2, 4, 3];
        let my_input = String::from("Hello Handler!");

        let result = handler_executor.handle(&chain_for_request, my_input, None).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!("Goodbye Handler!", result);
    }

    #[test]
    fn test_invalid_handler_chain() {

    }


}
