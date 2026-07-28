use sqlx::PgPool;
use std::env;
use url::Url;

#[tokio::test]
async fn status_enum_includes_archived_after_migrations() {
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

    let url = Url::parse(&database_url).expect("DATABASE_URL must be a valid URL");
    let db_name = url.path().trim_start_matches('/');
    let test_db_name = format!("{}_test_migrations", db_name);

    // Connect to the default postgres database to create/drop the test database.
    let mut admin_url = url.clone();
    admin_url.set_path("postgres");
    let admin_pool = PgPool::connect(admin_url.as_str())
        .await
        .expect("Failed to connect to postgres database");

    // Clean up any leftover database from a previous interrupted run.
    let drop_sql = format!("DROP DATABASE IF EXISTS \"{}\"", test_db_name);
    sqlx::raw_sql(sqlx::AssertSqlSafe(drop_sql))
        .execute(&admin_pool)
        .await
        .expect("Failed to drop test database");

    let create_sql = format!("CREATE DATABASE \"{}\"", test_db_name);
    sqlx::raw_sql(sqlx::AssertSqlSafe(create_sql))
        .execute(&admin_pool)
        .await
        .expect("Failed to create test database");

    // Connect to the new test database and run all migrations.
    let mut test_url = url.clone();
    test_url.set_path(&format!("/{}", test_db_name));
    let test_pool = PgPool::connect(test_url.as_str())
        .await
        .expect("Failed to connect to test database");

    sqlx::migrate!("./migrations")
        .run(&test_pool)
        .await
        .expect("Failed to run migrations");

    // Verify ARCHIVED is present in the status enum.
    let values: Vec<String> = sqlx::query_scalar!(
        "SELECT enumlabel FROM pg_enum WHERE enumtypid = 'status'::regtype ORDER BY enumlabel"
    )
    .fetch_all(&test_pool)
    .await
    .expect("Failed to query status enum values");

    assert!(
        values.iter().any(|v| v == "ARCHIVED"),
        "status enum should contain ARCHIVED, got: {:?}",
        values
    );

    // Cleanup.
    drop(test_pool);
    let drop_sql = format!("DROP DATABASE \"{}\"", test_db_name);
    sqlx::raw_sql(sqlx::AssertSqlSafe(drop_sql))
        .execute(&admin_pool)
        .await
        .expect("Failed to drop test database");
}
