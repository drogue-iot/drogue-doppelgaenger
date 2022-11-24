use diesel_migrations::{
    embed_migrations, EmbeddedMigrations, HarnessWithOutput, MigrationHarness,
};
use tokio::runtime::Handle;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("../database-migration/migrations");

pub async fn run_migrations(config: &drogue_bazaar::db::postgres::Config) -> anyhow::Result<()> {
    use diesel::Connection;
    println!("Migrating database schema...");
    let database_url = format!(
        "postgres://{}:{}@{}:{}/{}",
        config.db.user.as_deref().unwrap_or_default(),
        config.db.password.as_deref().unwrap_or_default(),
        config.db.host.as_deref().unwrap_or_default(),
        config.db.port.unwrap_or_default(),
        config.db.dbname.as_deref().unwrap_or_default()
    );

    Handle::current()
        .spawn_blocking(move || {
            let mut connection = diesel::PgConnection::establish(&database_url)
                .unwrap_or_else(|_| panic!("Error connecting to {}", database_url));

            HarnessWithOutput::new(&mut connection, std::io::stdout())
                .run_pending_migrations(MIGRATIONS)
                .unwrap();
            println!("Migrating database schema... done!");
        })
        .await?;

    Ok(())
}
