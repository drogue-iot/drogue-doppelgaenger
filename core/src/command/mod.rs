pub mod mqtt;

use async_trait::async_trait;
use drogue_bazaar::app::Startup;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use tracing::instrument;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub application: String,
    pub device: String,
    pub channel: String,
    pub payload: Vec<u8>,
}

#[async_trait]
pub trait CommandSink: Sized + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync;
    type Config: Clone + Debug + DeserializeOwned;

    fn from_config(startup: &mut dyn Startup, config: Self::Config) -> anyhow::Result<Self>;

    async fn send_command(&self, command: Command) -> Result<(), Self::Error>;

    #[instrument(skip_all, err)]
    async fn send_commands(&self, commands: Vec<Command>) -> Result<(), Self::Error> {
        for command in commands {
            self.send_command(command).await?;
        }
        Ok(())
    }
}
