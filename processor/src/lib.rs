#[derive(Debug, serde::Deserialize)]
pub struct Config {}

pub async fn run(config: Config) -> anyhow::Result<()> {
    Ok(())
}
