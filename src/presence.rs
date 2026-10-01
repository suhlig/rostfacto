//! Presence: who currently has a retro board open.
//!
//! Multi-process by design. Several app processes share one PostgreSQL
//! database, so the roster's source of truth is the ephemeral `presence`
//! table (one row per retro, participant and process instance), not memory.
//! A row is *present* while its `last_seen_at` is fresher than the grace
//! period; each process heartbeats the rows of its own open connections, so a
//! closed connection (or a crashed process) simply ages out.
//!
//! The hub itself only holds this process's local state: open connection
//! counts (what the heartbeat refreshes) and roster subscribers (the SSE
//! streams). A 1 s poll loop detects roster changes made by *any* process
//! and fans them out to local subscribers. Nothing here touches the durable
//! `events` table; clients learn about roster changes through `PARTICIPANTS`
//! SSE frames that carry no `id:` line.

use crate::models::initials;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

/// How often every process looks for roster changes and expires stale rows.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Lower bound for the heartbeat interval, so a tiny grace period (tests)
/// does not turn into a busy loop.
const MIN_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(250);

/// Identity under which a participant is deduplicated across connections.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParticipantKey {
    /// Auth mode: `users.id`, so all devices/tabs of one user collapse.
    User(i32),
    /// Demo mode: client-generated id (one per browser).
    Guest(Uuid),
}

impl ParticipantKey {
    /// Value of the `presence.participant_key` column.
    pub fn as_db_key(&self) -> String {
        match self {
            ParticipantKey::User(id) => format!("user:{id}"),
            ParticipantKey::Guest(id) => format!("guest:{id}"),
        }
    }

    fn is_guest(&self) -> bool {
        matches!(self, ParticipantKey::Guest(_))
    }
}

/// What clients get to see about a participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParticipantView {
    /// `presence.participant_key` (`user:{id}` / `guest:{uuid}`), so a client
    /// can recognize its own entry in the roster.
    pub key: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub initials: String,
    pub guest: bool,
    /// Whether the participant has indicated they are done writing cards.
    pub ready: bool,
}

/// Full roster snapshot, ordered by join time (first joiner first).
pub type Roster = Vec<ParticipantView>;

/// This process's local state; the database holds the shared truth.
#[derive(Default)]
struct PresenceState {
    /// Open SSE connections per `(retro_id, participant_key)` on *this*
    /// process. Only these rows are heartbeated.
    connections: HashMap<(i32, String), usize>,
    /// Local roster subscribers (SSE streams) per retro.
    subscribers: HashMap<i32, Vec<mpsc::UnboundedSender<Roster>>>,
    /// Last roster handed to the subscribers of a retro, to detect changes
    /// in the poll loop.
    last_roster: HashMap<i32, Roster>,
}

struct PresenceHubInner {
    /// Regenerated on every process start; scopes heartbeats and shutdown
    /// cleanup to this process's rows.
    instance_id: Uuid,
    pool: PgPool,
    grace: Duration,
    state: Mutex<PresenceState>,
}

/// Tracks connected participants per retro in PostgreSQL and fans roster
/// snapshots out to this process's SSE subscribers.
#[derive(Clone)]
pub struct PresenceHub {
    inner: Arc<PresenceHubInner>,
}

impl PresenceHub {
    pub fn new(pool: PgPool, grace: Duration) -> Self {
        Self {
            inner: Arc::new(PresenceHubInner {
                instance_id: Uuid::new_v4(),
                pool,
                grace,
                state: Mutex::new(PresenceState::default()),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, PresenceState> {
        // Nothing in here can leave the maps half-updated, so a poisoned lock
        // is safe to keep using.
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn grace_secs(&self) -> f64 {
        self.inner.grace.as_secs_f64()
    }

    /// Register one more open connection for `key` on this process.
    ///
    /// Upserts this instance's `presence` row. `joined_at` (and a guest's
    /// number) is taken from the participant's earliest still-fresh row on
    /// *any* instance, so reconnecting within the grace period keeps the
    /// list position even when the client lands on a different process.
    /// A participant whose rows have all expired starts over at the bottom.
    ///
    /// For `ParticipantKey::Guest` the `name` and `avatar_url` arguments are
    /// ignored: the name is "Guest N" from a global sequence (unique across
    /// processes, not contiguous per retro).
    ///
    /// Does not broadcast; the poll loop picks the change up.
    pub async fn join(
        &self,
        retro_id: i32,
        key: ParticipantKey,
        name: String,
        avatar_url: Option<String>,
    ) -> Result<(), sqlx::Error> {
        let db_key = key.as_db_key();
        let initials = initials(&name, false);

        sqlx::query!(
            r#"
            WITH prior AS (
                SELECT name, joined_at, ready
                FROM presence
                WHERE retro_id = $1::int4
                  AND participant_key = $2::text
                  AND last_seen_at > NOW() - make_interval(secs => $8::float8)
                ORDER BY joined_at
                LIMIT 1
            ),
            guest_name AS (
                -- CASE/COALESCE short-circuit: nextval() is only consumed
                -- when a brand-new guest number is actually needed.
                SELECT CASE
                    WHEN $4::bool THEN COALESCE(
                        (SELECT name FROM prior),
                        'Guest ' || nextval('presence_guest_number_seq')
                    )
                END AS name
            )
            INSERT INTO presence
                (retro_id, participant_key, instance_id, name, avatar_url,
                 initials, guest, joined_at, ready)
            SELECT $1::int4,
                   $2::text,
                   $3::uuid,
                   COALESCE(g.name, $5::text),
                   CASE WHEN $4::bool THEN NULL ELSE $6::text END,
                   CASE WHEN g.name IS NULL
                        THEN $7::text
                        ELSE 'G' || substring(g.name from '[0-9]+$')
                   END,
                   $4::bool,
                   COALESCE((SELECT joined_at FROM prior), NOW()),
                   COALESCE((SELECT ready FROM prior), FALSE)
            FROM guest_name g
            ON CONFLICT (retro_id, participant_key, instance_id) DO UPDATE SET
                name = EXCLUDED.name,
                avatar_url = EXCLUDED.avatar_url,
                initials = EXCLUDED.initials,
                joined_at = EXCLUDED.joined_at,
                last_seen_at = NOW()
                -- `ready` is intentionally not touched: a rejoin (e.g. an SSE
                -- reconnect) keeps the participant's state.
            "#,
            retro_id,
            db_key,
            self.inner.instance_id,
            key.is_guest(),
            name,
            avatar_url,
            initials,
            self.grace_secs(),
        )
        .execute(&self.inner.pool)
        .await?;

        *self
            .state()
            .connections
            .entry((retro_id, db_key))
            .or_default() += 1;
        Ok(())
    }

    /// Mark a participant as done writing cards (or writing again).
    ///
    /// Updates every row of the participant in the retro, not just this
    /// instance's: the roster collapses a participant's rows with
    /// `DISTINCT ON`, so a single stale row could otherwise win. Does not
    /// broadcast; the poll loop picks the change up.
    pub async fn set_ready(
        &self,
        retro_id: i32,
        key: &ParticipantKey,
        ready: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE presence SET ready = $3 WHERE retro_id = $1 AND participant_key = $2",
            retro_id,
            key.as_db_key(),
            ready,
        )
        .execute(&self.inner.pool)
        .await?;
        Ok(())
    }

    /// Release one local connection of `key`. At zero the heartbeat stops
    /// refreshing the row, which then ages out after the grace period. No
    /// database write, so this is safe to call from `Drop`.
    pub fn leave(&self, retro_id: i32, key: ParticipantKey) {
        let mut state = self.state();
        let entry = (retro_id, key.as_db_key());
        if let Some(count) = state.connections.get_mut(&entry) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                state.connections.remove(&entry);
            }
        }
    }

    /// Subscribe to roster snapshots for one retro. The current roster is
    /// queued immediately (it includes the caller if it joined first) so a
    /// fresh client can render without waiting for the next change.
    pub async fn subscribe(
        &self,
        retro_id: i32,
    ) -> Result<mpsc::UnboundedReceiver<Roster>, sqlx::Error> {
        let roster = load_roster(&self.inner.pool, retro_id, self.grace_secs()).await?;

        let (sender, receiver) = mpsc::unbounded_channel();
        // The receiver is alive in this scope, so the send cannot fail.
        let _ = sender.send(roster.clone());

        let mut state = self.state();
        let is_first = !state.subscribers.contains_key(&retro_id);
        state.subscribers.entry(retro_id).or_default().push(sender);
        // Only the first subscriber's snapshot may define `last_roster`: it is
        // shared across the retro's subscribers, so once others are present it
        // must keep reflecting what *they* were last sent. Otherwise a roster
        // change that happened since the last poll (e.g. another client just
        // joining) would be swallowed for them, because the poll loop would see
        // `last_roster` already equal to the new roster and skip the broadcast.
        if is_first {
            state.last_roster.insert(retro_id, roster);
        }
        Ok(receiver)
    }

    /// Refresh `last_seen_at` for every row backing a connection that is open
    /// on this process, in one batched statement.
    pub async fn heartbeat(&self) -> Result<(), sqlx::Error> {
        let (retro_ids, keys): (Vec<i32>, Vec<String>) =
            self.state().connections.keys().cloned().unzip();
        if retro_ids.is_empty() {
            return Ok(());
        }

        sqlx::query!(
            r#"UPDATE presence
               SET last_seen_at = NOW()
               WHERE instance_id = $1
                 AND (retro_id, participant_key) IN
                     (SELECT * FROM unnest($2::int4[], $3::text[]))"#,
            self.inner.instance_id,
            &retro_ids,
            &keys,
        )
        .execute(&self.inner.pool)
        .await?;
        Ok(())
    }

    /// Expire stale rows and broadcast roster changes to local subscribers.
    ///
    /// Idempotent and safe to run on every process at once: the `DELETE`
    /// matches nothing the second time, and each process only compares and
    /// broadcasts for its own subscribers.
    pub async fn poll(&self) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "DELETE FROM presence WHERE last_seen_at < NOW() - make_interval(secs => $1::float8)",
            self.grace_secs(),
        )
        .execute(&self.inner.pool)
        .await?;

        let retro_ids: Vec<i32> = {
            let mut state = self.state();
            state.subscribers.retain(|_, senders| {
                senders.retain(|sender| !sender.is_closed());
                !senders.is_empty()
            });
            let PresenceState {
                subscribers,
                last_roster,
                ..
            } = &mut *state;
            last_roster.retain(|retro_id, _| subscribers.contains_key(retro_id));
            subscribers.keys().copied().collect()
        };

        for retro_id in retro_ids {
            let roster = load_roster(&self.inner.pool, retro_id, self.grace_secs()).await?;
            let mut state = self.state();
            if state.last_roster.get(&retro_id) == Some(&roster) {
                continue;
            }
            if let Some(senders) = state.subscribers.get_mut(&retro_id) {
                senders.retain(|sender| sender.send(roster.clone()).is_ok());
            }
            state.last_roster.insert(retro_id, roster);
        }
        Ok(())
    }

    /// Delete exactly this process's rows, so the roster clears immediately
    /// on graceful shutdown instead of waiting out the grace period. Rows of
    /// the same participants on other processes are untouched.
    // Wired into graceful shutdown in a later step.
    #[allow(dead_code)]
    pub async fn shutdown(&self) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "DELETE FROM presence WHERE instance_id = $1",
            self.inner.instance_id
        )
        .execute(&self.inner.pool)
        .await?;
        self.state().connections.clear();
        Ok(())
    }

    /// Interval between heartbeats: a third of the grace period, so a live
    /// row is refreshed several times before it could expire.
    fn heartbeat_interval(&self) -> Duration {
        (self.inner.grace / 3).max(MIN_HEARTBEAT_INTERVAL)
    }
}

/// Current roster of one retro: one entry per participant (rows from several
/// instances collapse onto the earliest), ordered by join time.
async fn load_roster(pool: &PgPool, retro_id: i32, grace_secs: f64) -> Result<Roster, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT participant_key as "key!", name as "name!", avatar_url,
                  initials as "initials!", guest as "guest!", ready as "ready!"
           FROM (
               SELECT DISTINCT ON (participant_key)
                      participant_key, name, avatar_url, initials, guest, ready, joined_at
               FROM presence
               WHERE retro_id = $1
                 AND last_seen_at > NOW() - make_interval(secs => $2::float8)
               ORDER BY participant_key, joined_at
           ) AS p
           ORDER BY joined_at, participant_key"#,
        retro_id,
        grace_secs,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ParticipantView {
            key: row.key,
            name: row.name,
            avatar_url: row.avatar_url,
            initials: row.initials,
            guest: row.guest,
            ready: row.ready,
        })
        .collect())
}

/// Background task: keeps the rows of this process's open connections fresh.
/// Spawned once per app process; a crashed process stops heartbeating, so its
/// rows expire everywhere after the grace period.
pub async fn heartbeat_loop(hub: PresenceHub) {
    let mut interval = tokio::time::interval(hub.heartbeat_interval());
    loop {
        interval.tick().await;
        if let Err(error) = hub.heartbeat().await {
            crate::handlers::log_database_error("presence_heartbeat", &error);
        }
    }
}

/// Background task: propagates roster changes (joins, leaves, expiries made
/// on *any* process) to this process's SSE subscribers within ~1 s. Spawned
/// once per app process.
pub async fn poll_loop(hub: PresenceHub) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    loop {
        interval.tick().await;
        if let Err(error) = hub.poll().await {
            crate::handlers::log_database_error("presence_poll", &error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ensure_demo_user;
    use chrono::{DateTime, Utc};
    use tokio::sync::mpsc::error::TryRecvError;
    use tokio::time::{sleep, timeout};

    const GRACE: Duration = Duration::from_secs(1);
    const WAIT: Duration = Duration::from_secs(5);

    /// A throwaway retro (presence rows need a real `retro_id`).
    struct TestRetro {
        pool: PgPool,
        id: i32,
    }

    impl TestRetro {
        async fn new() -> Self {
            let database_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL environment variable must be set");
            let pool = PgPool::connect(&database_url)
                .await
                .expect("Failed to connect to database");
            let user_id = ensure_demo_user(&pool)
                .await
                .expect("Failed to ensure demo user");
            let slug = format!("presence-test-{}", Uuid::new_v4().simple());
            let id = sqlx::query_scalar::<_, i32>(
                "INSERT INTO retrospectives (title, slug, team_slug, created_by)
                 VALUES ('Presence test', $1, 'presence-test', $2)
                 RETURNING id",
            )
            .bind(slug)
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("Failed to create test retro");
            Self { pool, id }
        }

        fn hub(&self) -> PresenceHub {
            PresenceHub::new(self.pool.clone(), GRACE)
        }

        async fn roster(&self) -> Roster {
            load_roster(&self.pool, self.id, GRACE.as_secs_f64())
                .await
                .expect("roster query failed")
        }

        async fn rows(&self) -> i64 {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM presence WHERE retro_id = $1")
                .bind(self.id)
                .fetch_one(&self.pool)
                .await
                .unwrap()
        }

        async fn joined_ats(&self) -> Vec<DateTime<Utc>> {
            sqlx::query_scalar::<_, DateTime<Utc>>(
                "SELECT joined_at FROM presence WHERE retro_id = $1 ORDER BY joined_at",
            )
            .bind(self.id)
            .fetch_all(&self.pool)
            .await
            .unwrap()
        }

        /// Deleting the retro cascades to its presence rows.
        async fn cleanup(self) {
            sqlx::query("DELETE FROM retrospectives WHERE id = $1")
                .bind(self.id)
                .execute(&self.pool)
                .await
                .expect("Failed to delete test retro");
        }
    }

    fn user(id: i32) -> ParticipantKey {
        ParticipantKey::User(id)
    }

    async fn join_user(hub: &PresenceHub, retro_id: i32, id: i32, name: &str) {
        hub.join(retro_id, user(id), name.to_string(), None)
            .await
            .expect("join failed");
    }

    fn names(roster: &Roster) -> Vec<&str> {
        roster.iter().map(|p| p.name.as_str()).collect()
    }

    async fn next(receiver: &mut mpsc::UnboundedReceiver<Roster>) -> Roster {
        timeout(WAIT, receiver.recv())
            .await
            .expect("timed out waiting for a roster")
            .expect("roster channel closed")
    }

    #[test]
    fn db_keys_are_namespaced() {
        let guest = Uuid::nil();
        assert_eq!(user(42).as_db_key(), "user:42");
        assert_eq!(
            ParticipantKey::Guest(guest).as_db_key(),
            "guest:00000000-0000-0000-0000-000000000000"
        );
    }

    #[tokio::test]
    async fn heartbeat_interval_is_a_third_of_the_grace_with_a_floor() {
        let pool = PgPool::connect_lazy("postgres://localhost/unused").unwrap();
        let interval = |secs| PresenceHub::new(pool.clone(), secs).heartbeat_interval();

        assert_eq!(interval(Duration::from_secs(15)), Duration::from_secs(5));
        assert_eq!(interval(Duration::from_millis(300)), MIN_HEARTBEAT_INTERVAL);
    }

    #[tokio::test]
    async fn join_inserts_a_row_and_counts_the_connection() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();

        hub.join(
            retro.id,
            user(1),
            "Ada Lovelace".into(),
            Some("https://example.test/a.png".into()),
        )
        .await
        .unwrap();

        assert_eq!(
            retro.roster().await,
            vec![ParticipantView {
                key: "user:1".into(),
                name: "Ada Lovelace".into(),
                avatar_url: Some("https://example.test/a.png".into()),
                initials: "AL".into(),
                guest: false,
                ready: false,
            }]
        );
        assert_eq!(
            hub.state()
                .connections
                .get(&(retro.id, "user:1".to_string())),
            Some(&1)
        );
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn second_join_on_the_same_instance_preserves_joined_at() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;
        let first = retro.joined_ats().await;

        sleep(Duration::from_millis(50)).await;
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;

        assert_eq!(retro.joined_ats().await, first);
        assert_eq!(retro.rows().await, 1);
        assert_eq!(
            hub.state()
                .connections
                .get(&(retro.id, "user:1".to_string())),
            Some(&2)
        );
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn joins_on_two_instances_collapse_to_one_participant() {
        let retro = TestRetro::new().await;
        let (a, b) = (retro.hub(), retro.hub());
        join_user(&a, retro.id, 1, "Ada Lovelace").await;
        let first = retro.joined_ats().await;

        sleep(Duration::from_millis(50)).await;
        join_user(&b, retro.id, 1, "Ada Lovelace").await;

        assert_eq!(retro.rows().await, 2, "one row per instance");
        assert_eq!(names(&retro.roster().await), ["Ada Lovelace"]);
        // The second instance inherits the earliest fresh join time.
        assert_eq!(retro.joined_ats().await, vec![first[0], first[0]]);
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn roster_is_ordered_by_join_time() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        for (id, name) in [(3, "Third"), (1, "First"), (2, "Second")] {
            join_user(&hub, retro.id, id, name).await;
            sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(names(&retro.roster().await), ["Third", "First", "Second"]);
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn leaving_to_zero_stops_the_heartbeat_and_the_row_expires() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;

        // While connected, heartbeats keep the row fresh beyond the grace.
        sleep(GRACE * 6 / 10).await;
        hub.heartbeat().await.unwrap();
        sleep(GRACE * 6 / 10).await;
        assert_eq!(retro.roster().await.len(), 1);

        hub.leave(retro.id, user(1));
        assert!(hub.state().connections.is_empty());
        hub.heartbeat().await.unwrap();
        sleep(GRACE + Duration::from_millis(200)).await;

        assert!(retro.roster().await.is_empty());
        hub.poll().await.unwrap();
        assert_eq!(retro.rows().await, 0, "poll deletes the expired row");
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn closing_one_of_several_connections_keeps_the_heartbeat() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;

        hub.leave(retro.id, user(1));

        assert_eq!(hub.state().connections.len(), 1);
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn rejoin_within_the_grace_period_keeps_the_position() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;
        sleep(Duration::from_millis(10)).await;
        join_user(&hub, retro.id, 2, "Grace Hopper").await;

        hub.leave(retro.id, user(1));
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;

        assert_eq!(
            names(&retro.roster().await),
            ["Ada Lovelace", "Grace Hopper"]
        );
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn rejoin_after_the_grace_period_goes_to_the_bottom() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;
        sleep(Duration::from_millis(10)).await;
        join_user(&hub, retro.id, 2, "Grace Hopper").await;

        // Only Grace keeps heartbeating.
        hub.leave(retro.id, user(1));
        for _ in 0..3 {
            sleep(GRACE * 5 / 10).await;
            hub.heartbeat().await.unwrap();
        }
        assert_eq!(names(&retro.roster().await), ["Grace Hopper"]);
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;

        assert_eq!(
            names(&retro.roster().await),
            ["Grace Hopper", "Ada Lovelace"]
        );
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn subscribe_queues_the_current_roster() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;

        let mut receiver = hub.subscribe(retro.id).await.unwrap();

        assert_eq!(names(&next(&mut receiver).await), ["Ada Lovelace"]);
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn poll_broadcasts_only_on_change() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        let mut receiver = hub.subscribe(retro.id).await.unwrap();
        assert!(next(&mut receiver).await.is_empty());

        hub.poll().await.unwrap();
        assert_eq!(receiver.try_recv().unwrap_err(), TryRecvError::Empty);

        join_user(&hub, retro.id, 1, "Ada Lovelace").await;
        hub.poll().await.unwrap();
        assert_eq!(names(&next(&mut receiver).await), ["Ada Lovelace"]);

        hub.poll().await.unwrap();
        assert_eq!(receiver.try_recv().unwrap_err(), TryRecvError::Empty);
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn poll_propagates_changes_made_by_another_instance() {
        let retro = TestRetro::new().await;
        let (a, b) = (retro.hub(), retro.hub());
        let mut receiver = a.subscribe(retro.id).await.unwrap();
        assert!(next(&mut receiver).await.is_empty());

        join_user(&b, retro.id, 7, "Grace Hopper").await;
        a.poll().await.unwrap();

        assert_eq!(names(&next(&mut receiver).await), ["Grace Hopper"]);
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn poll_expiry_is_broadcast_and_pruned_subscribers_are_dropped() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;
        let mut receiver = hub.subscribe(retro.id).await.unwrap();
        assert_eq!(next(&mut receiver).await.len(), 1);

        hub.leave(retro.id, user(1));
        sleep(GRACE + Duration::from_millis(200)).await;
        hub.poll().await.unwrap();
        assert!(next(&mut receiver).await.is_empty());

        drop(receiver);
        hub.poll().await.unwrap();
        assert!(hub.state().subscribers.is_empty());
        assert!(hub.state().last_roster.is_empty());
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn retros_are_isolated() {
        let retro = TestRetro::new().await;
        let other = TestRetro::new().await;
        let hub = retro.hub();
        let mut receiver = hub.subscribe(other.id).await.unwrap();
        next(&mut receiver).await;

        join_user(&hub, retro.id, 1, "Ada Lovelace").await;
        hub.poll().await.unwrap();

        assert_eq!(receiver.try_recv().unwrap_err(), TryRecvError::Empty);
        assert!(other.roster().await.is_empty());
        retro.cleanup().await;
        other.cleanup().await;
    }

    #[tokio::test]
    async fn shutdown_deletes_only_this_instances_rows() {
        let retro = TestRetro::new().await;
        let (a, b) = (retro.hub(), retro.hub());
        join_user(&a, retro.id, 1, "Ada Lovelace").await;
        join_user(&b, retro.id, 1, "Ada Lovelace").await;
        join_user(&a, retro.id, 2, "Grace Hopper").await;
        assert_eq!(retro.rows().await, 3);

        a.shutdown().await.unwrap();

        assert_eq!(retro.rows().await, 1);
        assert_eq!(names(&retro.roster().await), ["Ada Lovelace"]);
        assert!(a.state().connections.is_empty());
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn guests_get_unique_stable_names_across_instances() {
        let retro = TestRetro::new().await;
        let (a, b) = (retro.hub(), retro.hub());
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());

        a.join(
            retro.id,
            ParticipantKey::Guest(first),
            "ignored".into(),
            Some("ignored".into()),
        )
        .await
        .unwrap();
        a.join(retro.id, ParticipantKey::Guest(second), String::new(), None)
            .await
            .unwrap();
        // The first guest reconnects to another instance and keeps its name.
        b.join(retro.id, ParticipantKey::Guest(first), String::new(), None)
            .await
            .unwrap();

        let roster = retro.roster().await;
        assert_eq!(roster.len(), 2);
        assert_ne!(roster[0].name, roster[1].name);
        for participant in &roster {
            assert!(participant.name.starts_with("Guest "));
            assert!(participant.guest);
            assert_eq!(participant.avatar_url, None);
            assert_eq!(
                participant.initials,
                format!("G{}", participant.name.trim_start_matches("Guest "))
            );
        }

        let names_of_first: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT name FROM presence WHERE retro_id = $1 AND participant_key = $2",
        )
        .bind(retro.id)
        .bind(ParticipantKey::Guest(first).as_db_key())
        .fetch_all(&retro.pool)
        .await
        .unwrap();
        assert_eq!(names_of_first.len(), 1, "both instances agree on the name");
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn set_ready_marks_the_participant_and_survives_a_rejoin() {
        let retro = TestRetro::new().await;
        let hub = retro.hub();
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;

        hub.set_ready(retro.id, &user(1), true).await.unwrap();
        assert!(retro.roster().await[0].ready);

        // A rejoin (e.g. an SSE reconnect) keeps the state.
        join_user(&hub, retro.id, 1, "Ada Lovelace").await;
        assert!(retro.roster().await[0].ready);

        hub.set_ready(retro.id, &user(1), false).await.unwrap();
        assert!(!retro.roster().await[0].ready);
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn set_ready_updates_every_row_of_the_participant() {
        let retro = TestRetro::new().await;
        let (a, b) = (retro.hub(), retro.hub());
        join_user(&a, retro.id, 1, "Ada Lovelace").await;
        join_user(&b, retro.id, 1, "Ada Lovelace").await;

        // Mark ready through one instance; both rows must reflect it, otherwise
        // the collapsed roster could pick the stale row.
        a.set_ready(retro.id, &user(1), true).await.unwrap();

        let ready_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM presence WHERE retro_id = $1 AND participant_key = $2 AND ready",
        )
        .bind(retro.id)
        .bind("user:1")
        .fetch_one(&retro.pool)
        .await
        .unwrap();
        assert_eq!(ready_rows, 2);
        assert!(retro.roster().await[0].ready);
        retro.cleanup().await;
    }

    #[tokio::test]
    async fn a_new_instance_inherits_ready_from_a_fresh_row() {
        let retro = TestRetro::new().await;
        let (a, b) = (retro.hub(), retro.hub());
        join_user(&a, retro.id, 1, "Ada Lovelace").await;
        a.set_ready(retro.id, &user(1), true).await.unwrap();

        // A second connection on another process starts a new row; it must
        // carry the participant's ready state over.
        join_user(&b, retro.id, 1, "Ada Lovelace").await;

        let ready_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM presence WHERE retro_id = $1 AND participant_key = $2 AND ready",
        )
        .bind(retro.id)
        .bind("user:1")
        .fetch_one(&retro.pool)
        .await
        .unwrap();
        assert_eq!(ready_rows, 2);
        retro.cleanup().await;
    }

    #[test]
    fn participant_view_serializes_to_the_wire_format() {
        let view = ParticipantView {
            key: "guest:00000000-0000-0000-0000-000000000000".into(),
            name: "Guest 2".into(),
            avatar_url: None,
            initials: "G2".into(),
            guest: true,
            ready: true,
        };

        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            r#"{"key":"guest:00000000-0000-0000-0000-000000000000","name":"Guest 2","avatar_url":null,"initials":"G2","guest":true,"ready":true}"#
        );
    }
}
