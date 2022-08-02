pub mod mqtt;

use crate::app::Spawner;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use std::fmt::Debug;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub application: String,
    pub device: String,
    pub channel: String,
    pub payload: Vec<u8>,
}

#[async_trait]
pub trait CommandSink: Sized + Send + Sync + 'static {
    type Error: std::error::Error;
    type Config: Clone + Debug + DeserializeOwned;

    fn from_config(spawner: &mut dyn Spawner, config: Self::Config) -> anyhow::Result<Self>;

    async fn send_command(&self, command: Command) -> Result<(), Self::Error>;

    async fn send_commands(&self, commands: Vec<Command>) -> Result<(), Self::Error> {
        for command in commands {
            self.send_command(command).await?;
        }
        Ok(())
    }
}
