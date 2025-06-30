use std::fs::File;
use serde::de::DeserializeOwned;
use std::path::MAIN_SEPARATOR_STR;

pub struct HandlerConfig<C> 
where
    C: Default + DeserializeOwned
{
    id: String,
    enabled: bool,
    timeout: Option<u64>,
    retry_count: Option<u32>,
    retry_delay: Option<u64>,
    config: Config<C>
}


impl<C> Default for HandlerConfig<C> 
where
    C: Default + DeserializeOwned
{
    fn default() -> Self {
        Self {
            id: "unknown".to_string(),
            enabled: true,
            timeout: Some(30000),
            retry_count: None,
            retry_delay: None,
            config: Config::default()
        }
    }
}

impl<C> HandlerConfig<C> 
where
    C: Default + DeserializeOwned
{
    pub fn new(id: String, config: Config<C>) -> Self {
        Self {
            id,
            config,
            ..Default::default()
        }
    }
    
    pub fn id(&self) -> &String {
        &self.id
    }
    
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    
    pub fn timeout(&self) -> Option<u64> {
        self.timeout
    }
    
    pub fn retry_count(&self) -> Option<u32> {
        self.retry_count
    }
    
    pub fn retry_delay(&self) -> Option<u64> {
        self.retry_delay
    }
    
    pub fn config(&self) -> &Config<C> {
        &self.config
    }
    
    pub fn set_config(&mut self, config: Config<C>) {
        self.config = config;
    }
    
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    pub fn set_timeout(&mut self, timeout: Option<u64>) {
        self.timeout = timeout;
    }
    
    pub fn set_retry_count(&mut self, retry_count: Option<u32>) {
        self.retry_count = retry_count;
    }
    
    pub fn set_retry_delay(&mut self, retry_delay: Option<u64>) {
        self.retry_delay = retry_delay;
    }
    
    pub fn set_id(&mut self, id: String) {
        self.id = id;
    }
    
    pub fn builder() -> HandlerConfigBuilder<C> {
        HandlerConfigBuilder::new()
    }
}

pub struct HandlerConfigBuilder<C>
where
    C: Default + DeserializeOwned
{
    config: HandlerConfig<C>
}

impl<C> HandlerConfigBuilder<C>
where
    C: Default + DeserializeOwned
{
    pub fn new() -> Self {
        Self {
            config: HandlerConfig::<C>::default()
        }
    }
    
    pub fn id(&mut self, id: String) -> &mut Self {
        self.config.id = id;
        self
    }
    
    pub fn enabled(&mut self, enabled: bool) -> &mut Self {
        self.config.enabled = enabled;
        self
    }
    
    pub fn timeout(&mut self, timeout: Option<u64>) -> &mut Self {
        self.config.timeout = timeout;
        self
    }
    
    pub fn retry_count(&mut self, retry_count: Option<u32>) -> &mut Self {
        self.config.retry_count = retry_count;
        self
    }
    
    pub fn retry_delay(&mut self, retry_delay: Option<u64>) -> &mut Self {
        self.config.retry_delay = retry_delay;
        self
    }
    
    pub fn config(&mut self, config: Config<C>) -> &mut Self {
        self.config.config = config;
        self
    }
    
    pub fn build(self) -> HandlerConfig<C> {
        self.config
    }
}

#[derive(Default)]
pub struct Config<C> {
    config: C,
}

impl<C> Config<C>
where
    C: Default + DeserializeOwned
{
    pub fn new(provider: impl ConfigProvider<C>) -> Result<Self, ()> {
        match provider.load() {
            Ok(config) => Ok(Self{config}),
            Err(_) => Err(())
        }
    }

    pub fn get(&self) -> &C {
        &self.config
    }

    pub fn get_mut(&mut self) -> &mut C {
        &mut self.config
    }
}

pub trait ConfigProvider<C>
where
    C: Default + DeserializeOwned
{
    fn load(&self) -> Result<C, ()>;
}

pub struct DefaultConfigProvider;

impl<C> ConfigProvider<C> for DefaultConfigProvider
where
    C: Default + DeserializeOwned
{
    fn load(&self) -> Result<C, ()> {
        Ok(C::default())
    }
}

pub struct FileConfigProvider {
    pub base_path: String,
    pub config_name: String
}

impl<C> ConfigProvider<C> for FileConfigProvider
where
    C: Default + DeserializeOwned
{
    fn load(&self) -> Result<C, ()> {
        let file = match File::open(format!("{}{}{}", self.base_path, MAIN_SEPARATOR_STR, self.config_name)) {
            Ok(file) => file,
            Err(_) => return Err(()),
        };
        match serde_json::from_reader(file) {
            Ok(config) => Ok(config),
            Err(_) => Err(())
        }
    }
}

pub enum ProviderType {
    File,
    Default
}