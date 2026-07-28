use sqlx::PgPool;
use std::env;
use url::Url;

async fn with_fresh_migrated_database<F, Fut>(name: &str, test: F)
where
    F: FnOnce(PgPool) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");

    let url = Url::parse(&database_url).expect("DATABASE_URL must be a valid URL");
    let db_name = url.path().trim_start_matches('/');
    let test_db_name = format!("{}_test_migrations_{}", db_name, name);

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

    // Run the actual test.
    test(test_pool.clone()).await;

    // Cleanup.
    drop(test_pool);
    let drop_sql = format!("DROP DATABASE \"{}\"", test_db_name);
    sqlx::raw_sql(sqlx::AssertSqlSafe(drop_sql))
        .execute(&admin_pool)
        .await
        .expect("Failed to drop test database");
}

#[tokio::test]
async fn status_enum_includes_archived_after_migrations() {
    with_fresh_migrated_database("archived", |pool| async move {
        let values: Vec<String> = sqlx::query_scalar!(
            "SELECT enumlabel FROM pg_enum WHERE enumtypid = 'status'::regtype ORDER BY enumlabel"
        )
        .fetch_all(&pool)
        .await
        .expect("Failed to query status enum values");

        assert!(
            values.iter().any(|v| v == "ARCHIVED"),
            "status enum should contain ARCHIVED, got: {:?}",
            values
        );
    })
    .await;
}

#[tokio::test]
async fn items_retro_category_status_index_exists() {
    with_fresh_migrated_database("index", |pool| async move {
        let row = sqlx::query!(
            r#"
            SELECT indexdef
            FROM pg_indexes
            WHERE schemaname = 'public'
              AND tablename = 'items'
              AND indexname = 'items_retro_category_status_idx'
            "#
        )
        .fetch_one(&pool)
        .await
        .expect("items_retro_category_status_idx should exist");

        let indexdef = row.indexdef.expect("indexdef should be present");
        assert!(
            indexdef.contains("retro_id"),
            "index should include retro_id: {}",
            indexdef
        );
        assert!(
            indexdef.contains("category"),
            "index should include category: {}",
            indexdef
        );
        assert!(
            indexdef.contains("status"),
            "index should include status: {}",
            indexdef
        );
    })
    .await;
}

#[tokio::test]
async fn sessions_table_has_cached_team_columns() {
    with_fresh_migrated_database("session_cache", |pool| async move {
        let columns: Vec<Option<String>> = sqlx::query_scalar!(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = 'sessions' AND column_name IN ('is_admin', 'teams') \
             ORDER BY column_name"
        )
        .fetch_all(&pool)
        .await
        .expect("Failed to query sessions columns");

        let columns: Vec<String> = columns.into_iter().flatten().collect();
        assert!(
            columns.iter().any(|c| c == "is_admin"),
            "sessions should have is_admin column, got: {:?}",
            columns
        );
        assert!(
            columns.iter().any(|c| c == "teams"),
            "sessions should have teams column, got: {:?}",
            columns
        );
    })
    .await;
}

#[tokio::test]
async fn users_table_does_not_store_access_token() {
    with_fresh_migrated_database("no_access_token", |pool| async move {
        let columns: Vec<Option<String>> = sqlx::query_scalar!(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = 'users' AND column_name = 'access_token'"
        )
        .fetch_all(&pool)
        .await
        .expect("Failed to query users columns");

        assert!(
            columns.into_iter().flatten().next().is_none(),
            "users table should not contain access_token column"
        );
    })
    .await;
}

#[tokio::test]
async fn updated_at_columns_and_triggers() {
    with_fresh_migrated_database("updated_at", |pool| async move {
        for table in ["retrospectives", "items", "users", "sessions"] {
            let column: Option<Option<String>> = sqlx::query_scalar!(
                "SELECT column_name FROM information_schema.columns \
                 WHERE table_name = $1 AND column_name = 'updated_at'",
                table
            )
            .fetch_optional(&pool)
            .await
            .expect("Failed to query columns");

            assert!(
                column.flatten().is_some(),
                "{} should have updated_at column",
                table
            );
        }

        // Verify the trigger fires for users.
        let user_id: i32 = sqlx::query_scalar!(
            "INSERT INTO users (github_id, username, full_name) VALUES ($1, $2, $3) RETURNING id",
            789_i64,
            "updater",
            "Original Name"
        )
        .fetch_one(&pool)
        .await
        .expect("Failed to insert user");

        let before: chrono::DateTime<chrono::Utc> =
            sqlx::query_scalar!("SELECT updated_at FROM users WHERE id = $1", user_id)
                .fetch_one(&pool)
                .await
                .expect("Failed to read updated_at");

        // Sleep briefly to ensure updated_at changes.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        sqlx::query!(
            "UPDATE users SET full_name = $1 WHERE id = $2",
            "Updated Name",
            user_id
        )
        .execute(&pool)
        .await
        .expect("Failed to update user");

        let after: chrono::DateTime<chrono::Utc> =
            sqlx::query_scalar!("SELECT updated_at FROM users WHERE id = $1", user_id)
                .fetch_one(&pool)
                .await
                .expect("Failed to read updated_at");

        assert!(
            after > before,
            "updated_at should advance after UPDATE: before={:?}, after={:?}",
            before,
            after
        );
    })
    .await;
}

#[tokio::test]
async fn creator_foreign_keys_use_on_delete_restrict() {
    with_fresh_migrated_database("fk_restrict", |pool| async move {
        let constraints: Vec<Option<String>> = sqlx::query_scalar!(
            "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
             WHERE conrelid = 'items'::regclass AND conname = 'items_created_by_fkey'"
        )
        .fetch_all(&pool)
        .await
        .expect("Failed to query items foreign keys");

        let def = constraints
            .into_iter()
            .flatten()
            .next()
            .expect("items_created_by_fkey should exist");
        assert!(
            def.contains("ON DELETE RESTRICT"),
            "items.created_by FK should use ON DELETE RESTRICT, got: {}",
            def
        );

        let constraints: Vec<Option<String>> = sqlx::query_scalar!(
            "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
             WHERE conrelid = 'retrospectives'::regclass AND conname = 'retrospectives_created_by_fkey'"
        )
        .fetch_all(&pool)
        .await
        .expect("Failed to query retrospectives foreign keys");

        let def = constraints
            .into_iter()
            .flatten()
            .next()
            .expect("retrospectives_created_by_fkey should exist");
        assert!(
            def.contains("ON DELETE RESTRICT"),
            "retrospectives.created_by FK should use ON DELETE RESTRICT, got: {}",
            def
        );
    })
    .await;
}

#[tokio::test]
async fn check_constraints_exist() {
    with_fresh_migrated_database("checks", |pool| async move {
        let constraints: Vec<String> = sqlx::query_scalar!(
            "SELECT conname FROM pg_constraint WHERE conrelid = 'retrospectives'::regclass AND contype = 'c'"
        )
        .fetch_all(&pool)
        .await
        .expect("Failed to query retrospectives constraints");

        assert!(
            constraints
                .iter()
                .any(|c| c == "retrospectives_slug_format_check"),
            "retrospectives should have slug format CHECK constraint, got: {:?}",
            constraints
        );

        let constraints: Vec<String> = sqlx::query_scalar!(
            "SELECT conname FROM pg_constraint WHERE conrelid = 'items'::regclass AND contype = 'c'"
        )
        .fetch_all(&pool)
        .await
        .expect("Failed to query items constraints");

        assert!(
            constraints.iter().any(|c| c == "items_text_not_empty_check"),
            "items should have text not-empty CHECK constraint, got: {:?}",
            constraints
        );

        let constraints: Vec<String> = sqlx::query_scalar!(
            "SELECT conname FROM pg_constraint WHERE conrelid = 'users'::regclass AND contype = 'c'"
        )
        .fetch_all(&pool)
        .await
        .expect("Failed to query users constraints");

        assert!(
            constraints
                .iter()
                .any(|c| c == "users_username_not_empty_check"),
            "users should have username not-empty CHECK constraint, got: {:?}",
            constraints
        );
    })
    .await;
}

#[tokio::test]
async fn users_display_name_virtual_column() {
    with_fresh_migrated_database("display_name", |pool| async move {
        sqlx::query!(
            "INSERT INTO users (github_id, username, full_name) VALUES ($1, $2, $3)",
            123_i64,
            "jdoe",
            "John Doe"
        )
        .execute(&pool)
        .await
        .expect("Failed to insert user with full name");

        let with_full_name: Option<String> =
            sqlx::query_scalar!("SELECT display_name FROM users WHERE username = $1", "jdoe")
                .fetch_one(&pool)
                .await
                .expect("Failed to query display_name");

        assert_eq!(
            with_full_name.expect("display_name should be set"),
            "John Doe"
        );

        sqlx::query!(
            "INSERT INTO users (github_id, username, full_name) VALUES ($1, $2, NULL)",
            456_i64,
            "msmith"
        )
        .execute(&pool)
        .await
        .expect("Failed to insert user without full name");

        let without_full_name: Option<String> = sqlx::query_scalar!(
            "SELECT display_name FROM users WHERE username = $1",
            "msmith"
        )
        .fetch_one(&pool)
        .await
        .expect("Failed to query display_name");

        assert_eq!(
            without_full_name.expect("display_name should be set"),
            "msmith"
        );
    })
    .await;
}
