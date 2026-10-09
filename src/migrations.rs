use sqlx::{Executor, PgPool};

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_rust_baseline.sql",
        include_str!("../migrations/0001_rust_baseline.sql"),
    ),
    (
        "0002_goal_branch_core.sql",
        include_str!("../migrations/0002_goal_branch_core.sql"),
    ),
    (
        "0003_tooling_core.sql",
        include_str!("../migrations/0003_tooling_core.sql"),
    ),
    (
        "0004_input_artifacts.sql",
        include_str!("../migrations/0004_input_artifacts.sql"),
    ),
    (
        "0005_ideas_and_project_proposals.sql",
        include_str!("../migrations/0005_ideas_and_project_proposals.sql"),
    ),
    (
        "0006_idea_sources.sql",
        include_str!("../migrations/0006_idea_sources.sql"),
    ),
    (
        "0007_goal_domain_v2.sql",
        include_str!("../migrations/0007_goal_domain_v2.sql"),
    ),
    (
        "0008_context_memory.sql",
        include_str!("../migrations/0008_context_memory.sql"),
    ),
    (
        "0009_workspace_runner.sql",
        include_str!("../migrations/0009_workspace_runner.sql"),
    ),
    (
        "0010_signed_real_plugins.sql",
        include_str!("../migrations/0010_signed_real_plugins.sql"),
    ),
    (
        "0011_action_scheduler.sql",
        include_str!("../migrations/0011_action_scheduler.sql"),
    ),
    (
        "0012_review_integration.sql",
        include_str!("../migrations/0012_review_integration.sql"),
    ),
    (
        "0013_private_security_recovery.sql",
        include_str!("../migrations/0013_private_security_recovery.sql"),
    ),
    (
        "0014_plugin_resources.sql",
        include_str!("../migrations/0014_plugin_resources.sql"),
    ),
    (
        "0015_maitu_file_workflows.sql",
        include_str!("../migrations/0015_maitu_file_workflows.sql"),
    ),
    (
        "0016_maitu_project_execution.sql",
        include_str!("../migrations/0016_maitu_project_execution.sql"),
    ),
    (
        "0017_maitu_connections.sql",
        include_str!("../migrations/0017_maitu_connections.sql"),
    ),
    (
        "0018_maitu_daily_history.sql",
        include_str!("../migrations/0018_maitu_daily_history.sql"),
    ),
    (
        "0019_maitu_retry_hold.sql",
        include_str!("../migrations/0019_maitu_retry_hold.sql"),
    ),
];

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
