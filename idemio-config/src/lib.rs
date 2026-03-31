use std::collections::HashMap;
use std::fmt::Debug;

/* ------------------< TRAITS >------------------ */

pub trait Config: Send + Sync + Debug {}

pub trait Handler<I>: Send + Sync
where
    I: Send + Sync
{
    fn execute(&self, input: &mut I);
}

/* ---------------------------------------------- */

/* -----------------< HANDLER1 >----------------- */
#[derive(Debug)]
pub struct HandlerConfig1 {
    pub prop1: String,
    pub prop2: bool,
}
impl Config for HandlerConfig1 {}
pub struct Handler1 {
    config: HandlerConfig1
}
impl<I> Handler<I> for Handler1
where
    I: Send + Sync
{
    fn execute(&self, _input: &mut I) {
        println!("Handler1 executed with config: {:?}", self.config);
    }
}
impl Handler1 {
    pub fn handler_id() -> &'static str {
        "Handler1"
    }
}

/* ---------------------------------------------- */

/* -----------------< HANDLER2 >----------------- */
#[derive(Debug)]
pub struct HandlerConfig2 {
    pub some_prop: u64,
    pub some_other_prop: String,
}
impl Config for HandlerConfig2 {}
pub struct Handler2 {
    config: HandlerConfig2
}
impl<I> Handler<I> for Handler2
where
    I: Send + Sync
{
    fn execute(&self, _input: &mut I) {
        println!("Handler2 executed with config: {:?}", self.config);
    }
}
impl Handler2 {
    pub fn handler_id() -> &'static str {
        "Handler2"
    }
}

/* ---------------------------------------------- */

#[derive(Default)]
pub struct HandlerRegistry<I> {
    handlers: HashMap<String, Box<dyn Handler<I>>>
}

impl<I> HandlerRegistry<I> {
    pub fn register_handler(&mut self, id: String, handler: Box<dyn Handler<I>>) {
        self.handlers.insert(id, handler);
    }

    pub fn get_handler(&self, id: &str) -> Option<&Box<dyn Handler<I>>> {
        self.handlers.get(id)
    }
}


#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};
    use arc_swap::{ArcSwap, Cache};
    use super::*;

    fn create_basic_registry() -> HandlerRegistry<()> {
        let mut registry = HandlerRegistry::default();

        // Create and register handler 1
        let handler_1_key = Handler1::handler_id();
        let handler_1 = Box::new(Handler1 { config: HandlerConfig1 {
            prop1: "Some String Value".to_string(),
            prop2: false,
        }});
        registry.register_handler(handler_1_key.into(), handler_1);

        // Create and register handler 2
        let handler_2_key = Handler2::handler_id();
        let handler_2 = Box::new(Handler2 {
            config: HandlerConfig2 {
                some_prop: 0,
                some_other_prop: "First value".to_string(),
            }
        });
        registry.register_handler(handler_2_key.into(), handler_2);
        registry
    }

    // This is a temp test. I am using it to find the best way to integrate arc_swap into the idemio framework.
    #[test]
    fn handler_config_test() {
        let handler_configs = Arc::new(ArcSwap::from_pointee(create_basic_registry()));
        let terminate = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        threads.push(thread::spawn({
            let handler_configs = Arc::clone(&handler_configs);
            let terminate = Arc::clone(&terminate);
            move || {
                let mut count = 0;
                while !terminate.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(2));
                    println!("Creating new values for handler configs");
                    let mut registry = HandlerRegistry::default();
                    // Create and register handler 1
                    let handler_1_key = Handler1::handler_id();
                    let handler_1 = Box::new(Handler1 { config: HandlerConfig1 {
                        prop1: format!("Changed {} times.", count),
                        prop2: true,
                    }});
                    registry.register_handler(handler_1_key.into(), handler_1);

                    // Handler 2 is removed completely in this reload
                    let new_config = Arc::new(registry);
                    handler_configs.store(new_config);
                    count += 1;
                }
            }
        }));

        for x in 0..10 {
            let t = thread::spawn({
                let handler_configs = Arc::clone(&handler_configs);
                let terminate = Arc::clone(&terminate);
                move || {
                    while !terminate.load(Ordering::Relaxed) {
                        println!("Executing handlers from thread: {}", x);
                        let start = Instant::now();
                        let mock_chain: Vec<String> = vec!["Handler1".into(), "Handler2".into()];
                        let handler_configs = handler_configs.load();
                        for handler_id in mock_chain {
                            match handler_configs.get_handler(&handler_id) {
                                Some(handler) => {
                                    println!("Executing handler {}", handler_id);
                                    handler.execute(&mut ())
                                },
                                _ => println!("Could not find handler: {}", handler_id)
                            }
                        }
                        println!("Thread {} took {} micro-seconds to execute handlers.", x, start.elapsed().as_micros());
                        thread::sleep(Duration::from_secs(1));
                    }


                }
            });
            threads.push(t);
        }

        thread::sleep(Duration::from_secs(9));

        // Terminate gracefully
        terminate.store(true, Ordering::Relaxed);
        for thread in threads {
            thread.join().unwrap();
        }
    }
}
