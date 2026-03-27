use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::File;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Default)]
pub struct Config<C> {
    config: C,
}

impl<C> Config<C>
where
    C: Default + DeserializeOwned,
{
    pub fn new(provider: impl ConfigProvider<C>) -> Result<Self, ()> {
        provider
            .load()
            .map(|config| Config { config })
            .map_err(|_| ())
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
    C: Default + DeserializeOwned,
{
    fn load(&self) -> Result<C, ConfigProviderError>;
}

pub struct DefaultConfigProvider;
impl<C> ConfigProvider<C> for DefaultConfigProvider
where
    C: Default + DeserializeOwned,
{
    fn load(&self) -> Result<C, ConfigProviderError> {
        Ok(C::default())
    }
}

pub struct FileConfigProvider {
    pub base_path: String,
    pub config_name: String,
}

impl<C> ConfigProvider<C> for FileConfigProvider
where
    C: Default + DeserializeOwned,
{
    fn load(&self) -> Result<C, ConfigProviderError> {
        let config_path = Path::new(&self.base_path).join(&self.config_name);
        let file = File::open(config_path).map_err(|e| {
            let msg = format!("Could not open config file: {}", e);
            ConfigProviderError::load_error(msg)
        })?;
        serde_json::from_reader(file).map_err(|e| {
            let msg = format!("Could not load config file from reader: {}", e);
            ConfigProviderError::load_error(msg)
        })
    }
}

pub struct ProgrammaticConfigProvider<C> {
    pub config: C,
}

impl<C> ConfigProvider<C> for ProgrammaticConfigProvider<C>
where
    C: Default + DeserializeOwned + Clone + Serialize,
{
    fn load(&self) -> Result<C, ConfigProviderError> {
        Ok(self.config.clone())
    }
}

#[derive(Error, Debug)]
pub enum ConfigProviderError {
    #[error("Could not load config file. {message}")]
    Load { message: String },
}

impl ConfigProviderError {
    #[inline]
    pub(crate) fn load_error(msg: impl Into<String>) -> Self {
        Self::Load {
            message: msg.into(),
        }
    }
}
