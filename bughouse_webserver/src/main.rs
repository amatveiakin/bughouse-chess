use bughouse_console::database::{self, SqlxDatabase};
use bughouse_console::stats_handlers_tide::{Handlers, SuitableServerState};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "0.0.0.0:14362")]
    bind_address: String,

    #[arg(long)]
    sqlite_db: Option<String>,

    #[arg(long)]
    postgres_db: Option<String>,

    #[arg(long, default_value = "")]
    static_content_url_prefix: String,
}

pub struct DatabaseApp<DB> {
    pub db: DB,
    pub static_content_url_prefix: String,
}

impl<DB: Clone> Clone for DatabaseApp<DB> {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            static_content_url_prefix: self.static_content_url_prefix.clone(),
        }
    }
}

impl<DB: Sync + Send + Clone + 'static + database::DatabaseReader> SuitableServerState
    for DatabaseApp<DB>
{
    type DB = DB;
    fn db(&self) -> &Self::DB {
        &self.db
    }

    fn static_content_url_prefix(&self) -> &str {
        self.static_content_url_prefix.as_str()
    }
}

async fn run_app<DB>(db: DB, args: Args) -> anyhow::Result<()>
where
    DB: Sync + Send + Clone + database::DatabaseReader + 'static,
{
    let mut app = tide::with_state(DatabaseApp {
        db,
        static_content_url_prefix: args.static_content_url_prefix,
    });
    Handlers::register_handlers(&mut app);
    app.listen(args.bind_address)
        .await
        .map_err(anyhow::Error::from)
}

#[async_std::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    match (&args.sqlite_db, &args.postgres_db) {
        (None, None) => return Err(anyhow::Error::msg("Database address was not specified.")),
        (Some(_), Some(_)) => {
            return Err(anyhow::Error::msg(
                "Both sqlite-db and postgres-db were specified.",
            ))
        }
        (Some(db), _) => run_app(SqlxDatabase::<sqlx::Sqlite>::new(&db)?, args).await?,
        (_, Some(db)) => run_app(SqlxDatabase::<sqlx::Postgres>::new(&db)?, args).await?,
    }
    Ok(())
}
