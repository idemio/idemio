use uuid::Uuid;
use crate::exchange::{Attachments, Callback, ExchangeError, RootExchange};

pub struct BufferedExchange<I, O, M> {
    pub(crate) uuid: Uuid,
    pub(crate) metadata: Option<M>,
    pub(crate) input: Option<I>,
    pub(crate) output: Option<O>,
    pub(crate) input_listeners: Vec<Callback<I>>,
    pub(crate) output_listeners: Vec<Callback<O>>,
    pub(crate) attachments: Attachments,
}

impl<I, O, M> RootExchange<I, O, M> for BufferedExchange<I, O, M> 
where
    I: Send + Sync,
    O: Send + Sync,
    M: Send + Sync,
{
    fn get_metadata(&self) -> &M {
        todo!()
    }

    fn get_metadata_mut(&mut self) -> &mut M {
        todo!()
    }

    fn get_attachments(&self) -> &Attachments {
        &self.attachments
    }

    fn get_attachments_mut(&mut self) -> &mut Attachments {
        &mut self.attachments
    }

    fn get_uuid(&self) -> &Uuid {
        &self.uuid
    }

    fn set_input(&mut self, input: I) {
        self.save_input(input)
    }

    fn get_input(&mut self) -> Result<&I, ExchangeError> {
        todo!()
    }

    fn get_output(&mut self) -> Result<&O, ExchangeError> {
        todo!()
    }

    fn consume_input(&mut self) -> Result<I, ExchangeError> {
        todo!()
    }

    fn set_output(&mut self, output: O) {
        todo!()
    }

    fn consume_output(&mut self) -> Result<O, ExchangeError> {
        todo!()
    }

    fn add_input_callback(&mut self, callback: Callback<I>) {
        todo!()
    }

    fn add_output_callback(&mut self, callback: Callback<O>) {
        todo!()
    }
}

impl<Input, Output, Metadata> BufferedExchange<Input, Output, Metadata>
where
    Input: Send + Sync,
    Output: Send + Sync,
    Metadata: Send + Sync,
{
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        Self {
            uuid,
            metadata: None,
            input: None,
            output: None,
            input_listeners: vec![],
            output_listeners: vec![],
            attachments: Attachments::new(),
        }
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn add_metadata(&mut self, metadata: Metadata) {
        self.metadata = Some(metadata);
    }

    pub fn attachments(&self) -> &Attachments {
        &self.attachments
    }

    pub fn attachments_mut(&mut self) -> &mut Attachments {
        &mut self.attachments
    }

    pub fn add_input_listener(
        &mut self,
        callback: impl FnMut(&mut Input, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.input_listeners.push(Callback::new(callback));
    }

    pub fn add_output_listener(
        &mut self,
        callback: impl FnMut(&mut Output, &mut Attachments) + Send + Sync + 'static,
    ) {
        self.output_listeners.push(Callback::new(callback));
    }

    fn execute_input_callbacks(&mut self) -> Result<(), ExchangeError> {
        if let Some(input) = &mut self.input {
            for mut callback in &mut self.input_listeners.drain(..) {
                callback.invoke(input, &mut self.attachments);
            }
            Ok(())
        } else {
            Err(ExchangeError::input_callback_error(
                &self.uuid,
                "Exchange does not contain an input to perform callbacks on",
            ))
        }
    }

    fn execute_output_callbacks(&mut self) -> Result<(), ExchangeError> {
        if let Some(output) = &mut self.output {
            for mut callback in &mut self.output_listeners.drain(..) {
                callback.invoke(output, &mut self.attachments);
            }
            Ok(())
        } else {
            Err(ExchangeError::output_callback_error(
                &self.uuid,
                "Exchange does not contain an output to perform callbacks on",
            ))
        }
    }

    pub fn save_input(&mut self, request: Input) {
        self.input = Some(request);
    }

    pub fn input(&self) -> Result<&Input, ExchangeError> {
        match &self.input {
            Some(out) => Ok(out),
            None => Err(ExchangeError::input_read_error(
                &self.uuid,
                "Exchange does not contain any input data to read.",
            )),
        }
    }

    pub fn input_mut(&mut self) -> Result<&mut Input, ExchangeError> {
        match &mut self.input {
            Some(out) => Ok(out),
            None => Err(ExchangeError::input_read_error(
                &self.uuid,
                "Exchange does not contain any input data to modify.",
            )),
        }
    }

    pub fn take_input(&mut self) -> Result<Input, ExchangeError> {
        match self.execute_input_callbacks() {
            Ok(_) => self.input.take().ok_or_else(|| {
                ExchangeError::input_take_error(
                    &self.uuid,
                    "Exchange does not contain any input data to consume.",
                )
            }),
            Err(e) => Err(e),
        }
    }

    pub fn save_output(&mut self, response: Output) {
        self.output = Some(response);
    }

    pub fn output(&self) -> Result<&Output, ExchangeError> {
        match &self.output {
            Some(out) => Ok(out),
            None => Err(ExchangeError::output_read_error(
                &self.uuid,
                "Exchange contains no output data to read.",
            )),
        }
    }

    pub fn output_mut(&mut self) -> Result<&mut Output, ExchangeError> {
        match &mut self.output {
            Some(out) => Ok(out),
            None => Err(ExchangeError::output_read_error(
                &self.uuid,
                "Exchange does not contain any output data to modify.",
            )),
        }
    }

    pub fn take_output(&mut self) -> Result<Output, ExchangeError> {
        match self.execute_output_callbacks() {
            Ok(_) => self.output.take().ok_or_else(|| {
                ExchangeError::output_read_error(
                    &self.uuid,
                    "Exchange does not contain any output data to consume.",
                )
            }),
            Err(e) => Err(e),
        }
    }
}