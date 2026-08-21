use sqlx::{Executor, PgPool};

const MIGRATIONS: &[(&str, &str)] = &[(
    "0001_rust_baseline.sql",
    include_str!("../migrations/0001_rust_baseline.sql"),
)];

pub async fn run(pool: &PgPool) -> Result<(), sqlx::Error> {
    pool.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
         filename text PRIMARY KEY, \
         applied_at timestamptz NOT NULL DEFAULT now()\
         )",
    )
    .await?;

    for (filename, sql) in MIGRATIONS {
        let applied: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM schema_migrations WHERE filename = $1)",
        )
        .bind(filename)
        .fetch_one(pool)
        .await?;
        if applied {
            continue;
        }

        let mut transaction = pool.begin().await?;
        // The SQL is compiled into the binary with include_str!, never supplied by a request.
        sqlx::raw_sql(*sql).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO schema_migrations (filename) VALUES ($1)")
            .bind(filename)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
    }

    Ok(())
}
