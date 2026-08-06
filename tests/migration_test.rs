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
async fn session_ids_use_uuidv7() {
    with_fresh_migrated_database("uuidv7", |pool| async move {
        let user_id: i32 = sqlx::query_scalar!(
            "INSERT INTO users (github_id, username, full_name) VALUES ($1, $2, $3) RETURNING id",
            1000_i64,
            "uuid-user",
            "UUID User"
        )
        .fetch_one(&pool)
        .await
        .expect("Failed to insert user");

        let session_id: String = sqlx::query_scalar!(
            "INSERT INTO sessions (user_id, expires_at, is_admin, teams) \
             VALUES ($1, NOW() + interval '1 hour', false, '[]'::jsonb) RETURNING id",
            user_id
        )
        .fetch_one(&pool)
        .await
        .expect("Failed to insert session");

        assert!(
            session_id.len() == 36 && session_id.chars().filter(|c| *c == '-').count() == 4,
            "session id should be a UUID, got: {}",
            session_id
        );
    })
    .await;
}

#[tokio::test]
async fn identity_columns_use_generated_always() {
    with_fresh_migrated_database("identity", |pool| async move {
        for (table, column) in [("retrospectives", "id"), ("items", "id"), ("users", "id")] {
            let sql = format!(
                "SELECT attidentity::text FROM pg_attribute \
                 WHERE attrelid = '{}'::regclass AND attname = '{}'",
                table, column
            );
            let identity: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .fetch_one(&pool)
                .await
                .expect("Failed to query identity attribute");

            assert_eq!(
                identity,
                Some("a".to_string()),
                "{}.{} should use GENERATED ALWAYS AS IDENTITY",
                table,
                column
            );
        }
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

// ---------------------------------------------------------------------------
// SSE event log (migration 021)
// ---------------------------------------------------------------------------

async fn insert_test_user(pool: &PgPool, github_id: i64, username: &str) -> i32 {
    sqlx::query_scalar!(
        "INSERT INTO users (github_id, username, full_name) VALUES ($1, $2, $3) RETURNING id",
        github_id,
        username,
        "Test User"
    )
    .fetch_one(pool)
    .await
    .expect("Failed to insert user")
}

async fn insert_test_retro(pool: &PgPool, created_by: i32, slug: &str) -> i32 {
    sqlx::query_scalar!(
        "INSERT INTO retrospectives (title, slug, team_slug, created_by) \
         VALUES ($1, $2, $3, $4) RETURNING id",
        "Test Retro",
        slug,
        "team-a",
        created_by
    )
    .fetch_one(pool)
    .await
    .expect("Failed to insert retro")
}

async fn insert_test_item(pool: &PgPool, retro_id: i32, created_by: i32, text: &str) -> i32 {
    sqlx::query_scalar!(
        "INSERT INTO items (retro_id, text, category, status, created_by) \
         VALUES ($1, $2, 'GOOD'::category, 'CREATED'::status, $3) RETURNING id",
        retro_id,
        text,
        created_by
    )
    .fetch_one(pool)
    .await
    .expect("Failed to insert item")
}

#[derive(Debug)]
struct EventRow {
    id: i64,
    event_type: String,
    item_id: Option<i32>,
    payload: sqlx::types::Json<serde_json::Value>,
}

async fn events_for(pool: &PgPool, retro_id: i32) -> Vec<EventRow> {
    sqlx::query_as!(
        EventRow,
        r#"SELECT id as "id!", event_type::text as "event_type!", item_id as "item_id: _",
                  payload as "payload: sqlx::types::Json<serde_json::Value>"
           FROM events WHERE retro_id = $1 ORDER BY id"#,
        retro_id
    )
    .fetch_all(pool)
    .await
    .expect("Failed to query events")
}

#[tokio::test]
async fn item_insert_emits_item_created_event() {
    with_fresh_migrated_database("item_created", |pool| async move {
        let user_id = insert_test_user(&pool, 1101, "creator").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-created").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Ship it").await;

        let events = events_for(&pool, retro_id).await;
        assert_eq!(
            events.len(),
            1,
            "inserting an item should emit exactly one event, got: {:?}",
            events
        );

        let event = &events[0];
        assert!(event.id > 0, "event id should be a positive identity value");
        assert_eq!(event.event_type, "ITEM_CREATED");
        assert_eq!(event.item_id, Some(item_id));
        let payload = &event.payload.0;
        assert_eq!(payload["item_id"].as_i64(), Some(item_id as i64));
        assert_eq!(payload["retro_id"].as_i64(), Some(retro_id as i64));
        assert_eq!(payload["category"].as_str(), Some("GOOD"));
        assert_eq!(payload["text"].as_str(), Some("Ship it"));
        assert_eq!(payload["status"].as_str(), Some("CREATED"));
        assert_eq!(payload["likes_count"].as_i64(), Some(0));
        assert_eq!(payload["author_name"].as_str(), Some("Test User"));
    })
    .await;
}

#[tokio::test]
async fn text_update_emits_item_updated_event() {
    with_fresh_migrated_database("item_updated", |pool| async move {
        let user_id = insert_test_user(&pool, 1102, "editor").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-updated").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Old text").await;

        sqlx::query!(
            "UPDATE items SET text = $1 WHERE id = $2",
            "New text",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to update item text");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(events.len(), 2, "expected ITEM_CREATED + ITEM_UPDATED");
        let event = &events[1];
        assert_eq!(event.event_type, "ITEM_UPDATED");
        assert_eq!(event.item_id, Some(item_id));
        assert_eq!(event.payload.0["text"].as_str(), Some("New text"));
    })
    .await;
}

#[tokio::test]
async fn status_change_emits_item_status_changed_event() {
    with_fresh_migrated_database("status_changed", |pool| async move {
        let user_id = insert_test_user(&pool, 1103, "status-changer").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-status").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Discuss me").await;

        sqlx::query!(
            "UPDATE items SET status = 'HIGHLIGHTED'::status WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to highlight item");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(
            events.len(),
            2,
            "expected ITEM_CREATED + ITEM_STATUS_CHANGED"
        );
        let event = &events[1];
        assert_eq!(event.event_type, "ITEM_STATUS_CHANGED");
        assert_eq!(event.item_id, Some(item_id));
        assert_eq!(event.payload.0["old_status"].as_str(), Some("CREATED"));
        assert_eq!(event.payload.0["new_status"].as_str(), Some("HIGHLIGHTED"));

        sqlx::query!(
            "UPDATE items SET status = 'COMPLETED'::status WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to complete item");

        let events = events_for(&pool, retro_id).await;
        let event = &events[2];
        assert_eq!(event.event_type, "ITEM_STATUS_CHANGED");
        assert_eq!(event.payload.0["old_status"].as_str(), Some("HIGHLIGHTED"));
        assert_eq!(event.payload.0["new_status"].as_str(), Some("COMPLETED"));
    })
    .await;
}

#[tokio::test]
async fn noop_status_update_emits_no_event() {
    with_fresh_migrated_database("status_noop", |pool| async move {
        let user_id = insert_test_user(&pool, 1104, "noop").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-noop").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Stable").await;

        sqlx::query!(
            "UPDATE items SET status = 'CREATED'::status WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to run noop status update");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(
            events.len(),
            1,
            "a no-op status update should not emit an event, got: {:?}",
            events
        );
    })
    .await;
}

#[tokio::test]
async fn likes_emit_like_events_with_recomputed_counts() {
    with_fresh_migrated_database("likes", |pool| async move {
        let user_id = insert_test_user(&pool, 1105, "liker").await;
        let other_user_id = insert_test_user(&pool, 1106, "other-liker").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-likes").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Liked").await;

        sqlx::query!(
            "INSERT INTO likes (item_id, user_id) VALUES ($1, $2)",
            item_id,
            other_user_id
        )
        .execute(&pool)
        .await
        .expect("Failed to insert like");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(events.len(), 2, "expected ITEM_CREATED + ITEM_LIKED");
        let event = &events[1];
        assert_eq!(event.event_type, "ITEM_LIKED");
        assert_eq!(event.item_id, Some(item_id));
        assert_eq!(event.payload.0["likes_count"].as_i64(), Some(1));

        sqlx::query!(
            "DELETE FROM likes WHERE item_id = $1 AND user_id = $2",
            item_id,
            other_user_id
        )
        .execute(&pool)
        .await
        .expect("Failed to delete like");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(
            events.len(),
            3,
            "expected ITEM_CREATED + ITEM_LIKED + ITEM_UNLIKED"
        );
        let event = &events[2];
        assert_eq!(event.event_type, "ITEM_UNLIKED");
        assert_eq!(event.item_id, Some(item_id));
        assert_eq!(event.payload.0["likes_count"].as_i64(), Some(0));
    })
    .await;
}

#[tokio::test]
async fn archive_emits_single_retro_archived_event_without_per_item_events() {
    with_fresh_migrated_database("archive_event", |pool| async move {
        let user_id = insert_test_user(&pool, 1107, "archiver").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-archive").await;
        insert_test_item(&pool, retro_id, user_id, "First").await;
        insert_test_item(&pool, retro_id, user_id, "Second").await;

        let archive_id: i32 = sqlx::query_scalar!(
            "INSERT INTO archives (retro_id) VALUES ($1) RETURNING id",
            retro_id
        )
        .fetch_one(&pool)
        .await
        .expect("Failed to insert archive");

        sqlx::query!(
            "UPDATE items SET status = 'ARCHIVED'::status, archive_id = $1, archived_at = NOW() \
             WHERE retro_id = $2 AND archive_id IS NULL",
            archive_id,
            retro_id
        )
        .execute(&pool)
        .await
        .expect("Failed to archive items");

        let events = events_for(&pool, retro_id).await;
        let archived = events
            .iter()
            .filter(|e| e.event_type == "RETRO_ARCHIVED")
            .collect::<Vec<_>>();
        assert_eq!(
            archived.len(),
            1,
            "archiving should emit exactly one RETRO_ARCHIVED event, got: {:?}",
            archived
        );
        assert_eq!(
            archived[0].payload.0["retro_id"].as_i64(),
            Some(retro_id as i64)
        );

        let per_item = events
            .iter()
            .filter(|e| e.event_type == "ITEM_STATUS_CHANGED" || e.event_type == "ITEM_UPDATED")
            .count();
        assert_eq!(
            per_item, 0,
            "bulk archive should not emit per-item events, got: {:?}",
            events
        );
    })
    .await;
}

#[tokio::test]
async fn item_mutations_notify_listeners_on_rostfacto_events_channel() {
    with_fresh_migrated_database("notify", |pool| async move {
        let user_id = insert_test_user(&pool, 1108, "notifier").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-notify").await;

        let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
            .await
            .expect("Failed to connect PgListener");
        listener
            .listen("rostfacto_events")
            .await
            .expect("Failed to LISTEN");

        let item_id = insert_test_item(&pool, retro_id, user_id, "Notify me").await;

        let notification = tokio::time::timeout(std::time::Duration::from_secs(3), listener.recv())
            .await
            .expect("timed out waiting for item insert notification")
            .expect("failed to receive item insert notification");
        assert_eq!(notification.channel(), "rostfacto_events");
        assert_eq!(notification.payload(), retro_id.to_string());

        sqlx::query!(
            "UPDATE items SET status = 'HIGHLIGHTED'::status WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to highlight item");

        let notification = tokio::time::timeout(std::time::Duration::from_secs(3), listener.recv())
            .await
            .expect("timed out waiting for status change notification")
            .expect("failed to receive status change notification");
        assert_eq!(notification.channel(), "rostfacto_events");
        assert_eq!(notification.payload(), retro_id.to_string());
    })
    .await;
}

// ---------------------------------------------------------------------------
// Server-authoritative timers (migration 022)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn timer_ends_at_is_a_virtual_generated_column() {
    with_fresh_migrated_database("timer_ends_at", |pool| async move {
        let generated: String = sqlx::query_scalar!(
            "SELECT attgenerated::text FROM pg_attribute \
             WHERE attrelid = 'items'::regclass AND attname = 'timer_ends_at'"
        )
        .fetch_one(&pool)
        .await
        .expect("timer_ends_at should be a generated column")
        .expect("attgenerated should not be NULL");
        assert_eq!(
            generated, "v",
            "timer_ends_at should be VIRTUAL, got: {}",
            generated
        );

        let user_id = insert_test_user(&pool, 1201, "timer-user").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-timer-ends").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Timed").await;

        sqlx::query!(
            "UPDATE items SET timer_started_at = $1, timer_duration_seconds = 300 WHERE id = $2",
            chrono::Utc::now(),
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to start timer");

        let row = sqlx::query!(
            "SELECT timer_started_at as \"started_at\", timer_duration_seconds as \"duration\", \
             timer_ends_at as \"ends_at\" FROM items WHERE id = $1",
            item_id
        )
        .fetch_one(&pool)
        .await
        .expect("Failed to read timer columns");

        let started_at = row.started_at.expect("timer_started_at should be set");
        let duration = row.duration.expect("timer_duration_seconds should be set");
        let ends_at = row.ends_at.expect("timer_ends_at should be computed");
        assert_eq!(
            ends_at,
            started_at + chrono::Duration::seconds(duration as i64),
            "timer_ends_at should be timer_started_at + timer_duration_seconds"
        );
    })
    .await;
}

#[tokio::test]
async fn timer_updates_emit_timer_events() {
    with_fresh_migrated_database("timer_events", |pool| async move {
        let user_id = insert_test_user(&pool, 1202, "timer-events").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-timer-updates").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Timed").await;

        sqlx::query!(
            "UPDATE items SET timer_started_at = NOW(), timer_duration_seconds = 300 WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to start timer");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(events.len(), 2, "expected ITEM_CREATED + TIMER_STARTED");
        let event = &events[1];
        assert_eq!(event.event_type, "TIMER_STARTED");
        assert_eq!(event.payload.0["item_id"].as_i64(), Some(item_id as i64));
        assert_eq!(event.payload.0["duration_seconds"].as_i64(), Some(300));
        assert!(
            event.payload.0["started_at"].is_string(),
            "TIMER_STARTED should carry started_at"
        );
        assert!(
            event.payload.0["ends_at"].is_string(),
            "TIMER_STARTED should carry ends_at"
        );

        sqlx::query!(
            "UPDATE items SET timer_duration_seconds = 420 WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to extend timer");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(events.len(), 3, "expected + TIMER_EXTENDED");
        let event = &events[2];
        assert_eq!(event.event_type, "TIMER_EXTENDED");
        assert_eq!(event.payload.0["duration_seconds"].as_i64(), Some(420));

        sqlx::query!(
            "UPDATE items SET timer_elapsed_at = NOW() WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to mark timer elapsed");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(events.len(), 4, "expected + TIMER_ELAPSED");
        let event = &events[3];
        assert_eq!(event.event_type, "TIMER_ELAPSED");
        assert_eq!(event.payload.0["item_id"].as_i64(), Some(item_id as i64));
    })
    .await;
}

#[tokio::test]
async fn status_change_wins_over_timer_changes_and_cancel_emits_no_timer_cancelled() {
    with_fresh_migrated_database("timer_precedence", |pool| async move {
        let user_id = insert_test_user(&pool, 1203, "timer-precedence").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-timer-precedence").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Timed").await;

        // Highlighting plus starting a timer in one UPDATE: the status change
        // wins, so only ITEM_STATUS_CHANGED is emitted.
        sqlx::query!(
            "UPDATE items SET status = 'HIGHLIGHTED'::status, \
             timer_started_at = NOW(), timer_duration_seconds = 300 WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to highlight and start timer");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(
            events.len(),
            2,
            "expected ITEM_CREATED + ITEM_STATUS_CHANGED"
        );
        assert_eq!(events[1].event_type, "ITEM_STATUS_CHANGED");
        assert_eq!(
            events[1].payload.0["new_status"].as_str(),
            Some("HIGHLIGHTED")
        );

        // Cancelling resets the timer columns and the status together: still
        // exactly one event, and never a TIMER_CANCELLED.
        sqlx::query!(
            "UPDATE items SET status = 'CREATED'::status, \
             timer_started_at = NULL, timer_duration_seconds = NULL, timer_elapsed_at = NULL \
             WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to cancel");

        let events = events_for(&pool, retro_id).await;
        assert_eq!(events.len(), 3, "expected + ITEM_STATUS_CHANGED");
        assert_eq!(events[2].event_type, "ITEM_STATUS_CHANGED");
        assert_eq!(events[2].payload.0["new_status"].as_str(), Some("CREATED"));
        assert!(
            events.iter().all(|e| e.event_type != "TIMER_CANCELLED"),
            "cancelling should never emit TIMER_CANCELLED, got: {:?}",
            events
        );
    })
    .await;
}

#[tokio::test]
async fn elapsed_sweep_update_is_idempotent() {
    with_fresh_migrated_database("timer_sweep", |pool| async move {
        let user_id = insert_test_user(&pool, 1204, "timer-sweep").await;
        let retro_id = insert_test_retro(&pool, user_id, "events-timer-sweep").await;
        let item_id = insert_test_item(&pool, retro_id, user_id, "Timed").await;

        sqlx::query!(
            "UPDATE items SET status = 'HIGHLIGHTED'::status, \
             timer_started_at = NOW() - interval '10 minutes', timer_duration_seconds = 300 \
             WHERE id = $1",
            item_id
        )
        .execute(&pool)
        .await
        .expect("Failed to set up overdue timer");

        let sweep = r#"
            UPDATE items SET timer_elapsed_at = NOW()
            WHERE status = 'HIGHLIGHTED'::status
              AND timer_ends_at <= NOW()
              AND timer_elapsed_at IS NULL
        "#;
        let first = sqlx::query(sweep)
            .execute(&pool)
            .await
            .expect("First sweep failed");
        assert_eq!(
            first.rows_affected(),
            1,
            "first sweep should mark the timer elapsed"
        );
        let second = sqlx::query(sweep)
            .execute(&pool)
            .await
            .expect("Second sweep failed");
        assert_eq!(second.rows_affected(), 0, "second sweep should be a no-op");

        let events = events_for(&pool, retro_id).await;
        let elapsed = events
            .iter()
            .filter(|e| e.event_type == "TIMER_ELAPSED")
            .count();
        assert_eq!(
            elapsed, 1,
            "the sweep should emit exactly one TIMER_ELAPSED"
        );
    })
    .await;
}
