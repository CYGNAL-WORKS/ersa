//! Archive inactive sessions (FLOW-CLEAN-UP Step 4, FLOW-CLEAN-UP-R10):
//! with `sessions.archive_inactive_days` set, the sweep archives each
//! session with no activity for that many days, as `session.set_archived`
//! does, and the same pass puts its folder away. Against a real git
//! repository and the fake backend, with the sweep's clock in the test's
//! hands: days pass when the test says so.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ersa_adapter::OpenElsewhere;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Records};
use support::put_away::*;
use support::sweep::*;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Ten minutes: more than the test itself takes, so a session is still
/// short of its days by this much however slow the machine.
const MARGIN: Duration = Duration::from_secs(10 * 60);

/// Sets Archive inactive sessions to `days`; null is Never.
async fn archive_after(s: &mut Setup, days: Value) {
    let patch = json!({ "patch": { "sessions": { "archive_inactive_days": days } } });
    s.client.result("settings.set", patch).await;
}

/// A session in a workspace of its own that started, went idle and was
/// stopped, which no connection is subscribed to, and its backend's id.
/// Its record holds nothing yet: it has no last activity, and it was made
/// when Core first started it, just now.
async fn left_alone(s: &mut Setup) -> (Value, String, String) {
    let workspace = s.workspace().await;
    let params = json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND });
    let session = s.client.result("session.create", params).await;
    let session = session["id"].as_str().unwrap().to_string();
    let subscribe = json!({ "id": session, "from_seq": 0 });
    s.client.result("session.subscribe", subscribe).await;
    let started = s.client.events_until("session.started").await;
    let backend_id = started.last().unwrap()["event"]["backend_session_id"]
        .as_str()
        .expect("the fake names its session")
        .to_string();
    s.client
        .result("session.stop", json!({ "id": session }))
        .await;
    s.client.events_until("session.ended").await;
    unsubscribe(s, &session).await;
    (workspace, session, backend_id)
}

async fn unsubscribe(s: &mut Setup, session: &str) {
    s.client
        .result("session.unsubscribe", json!({ "id": session }))
        .await;
}

/// The session as `session.list` lists it now.
async fn row(s: &mut Setup, session: &str) -> Value {
    support::listed(&mut s.client, session)
        .await
        .expect("the session is listed")
}

async fn archived(s: &mut Setup, session: &str) -> bool {
    row(s, session).await["archived"] == json!(true)
}

/// `workspace` was put away: its folder gone, its branch kept.
async fn put_away(s: &mut Setup, workspace: &Value) {
    assert_eq!(s.listed(workspace).await["state"], "put_away");
    assert!(!folder(workspace).exists(), "the folder is gone");
    assert!(has_branch(&s.repo, branch(workspace)), "the branch stays");
}

/// `at`, moved by `by`, as Core writes times: RFC 3339 UTC, to the
/// millisecond.
fn rfc3339(at: time::OffsetDateTime) -> String {
    let at = at.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.millisecond()
    )
}

/// Gives `backend_id`'s record a conversation begun at `made` whose last
/// entry is at `last`, as the backend would have written it.
fn recorded(
    records: &Records,
    backend_id: &str,
    made: time::OffsetDateTime,
    last: time::OffsetDateTime,
) {
    let mut first = fake::recorded_user("u-1", "hello");
    first.ts = Some(rfc3339(made));
    let mut reply = fake::recorded_reply("r-1", "echo: hello");
    reply.ts = Some(rfc3339(last));
    records
        .lock()
        .unwrap()
        .insert(backend_id.to_string(), vec![first, reply]);
}

/// Unset, the sweep archives nothing, however long a session has been
/// left; set and cleared again, it is unset. Set once more, the same
/// session is archived, so it was one the setting would archive.
// verifies: FLOW-CLEAN-UP-R10
// verifies: PROTO-CORE-API-R126
#[tokio::test(flavor = "multi_thread")]
async fn with_the_setting_unset_no_session_is_archived() {
    let (mut s, clock) = swept().await;
    let (workspace, session, _) = left_alone(&mut s).await;

    later(&clock, 31 * DAY).await;
    assert!(!archived(&mut s, &session).await);

    archive_after(&mut s, json!(1)).await;
    archive_after(&mut s, Value::Null).await;
    later(&clock, DAY).await;
    assert!(!archived(&mut s, &session).await);
    assert_eq!(s.listed(&workspace).await["state"], "ready");

    archive_after(&mut s, json!(30)).await;
    later(&clock, Duration::ZERO).await;
    assert!(archived(&mut s, &session).await);
}

/// A session whose record was last written at T, and begun a month before
/// that, is left until T plus the days chosen, and archived from then on
/// by the next pass, which puts its folder away too and writes the archive
/// to the diagnostic log. Its creation would have archived it at once.
// verifies: FLOW-CLEAN-UP-R10
#[tokio::test(flavor = "multi_thread")]
async fn a_session_is_archived_once_its_last_activity_is_that_many_days_ago() {
    let log = support::log::Log::installed();
    let records = Records::default();
    let (mut s, clock) = swept_with(FakeAdapter::with_records(Arc::clone(&records)), |_| {}).await;
    let (workspace, session, backend_id) = left_alone(&mut s).await;
    let now = time::OffsetDateTime::now_utc();
    recorded(&records, &backend_id, now - 30 * DAY, now);
    archive_after(&mut s, json!(7)).await;

    later(&clock, Duration::ZERO).await;
    assert_eq!(
        row(&mut s, &session).await["last_activity_at"],
        json!(rfc3339(now))
    );
    assert!(
        !archived(&mut s, &session).await,
        "its creation is not its last activity"
    );

    later(&clock, 7 * DAY - MARGIN).await;
    assert!(!archived(&mut s, &session).await, "archived early");

    later(&clock, 2 * MARGIN).await;
    assert!(archived(&mut s, &session).await);
    put_away(&mut s, &workspace).await;
    let field = format!("session={session}");
    assert!(
        log.logged("INFO", "the sweep archived the session", &field),
        "{field} not in the log"
    );
}

/// A session with no last activity is archived by when it was made.
// verifies: FLOW-CLEAN-UP-R10
#[tokio::test(flavor = "multi_thread")]
async fn a_session_with_no_activity_is_archived_by_its_creation() {
    let (mut s, clock) = swept().await;
    let (workspace, session, _) = left_alone(&mut s).await;
    assert_eq!(row(&mut s, &session).await.get("last_activity_at"), None);
    archive_after(&mut s, json!(2)).await;

    later(&clock, 2 * DAY - MARGIN).await;
    assert!(!archived(&mut s, &session).await, "archived early");

    later(&clock, 2 * MARGIN).await;
    assert!(archived(&mut s, &session).await);
    put_away(&mut s, &workspace).await;
}

/// Pinned, unseen, subscribed to, marked deleting or in a turn, a session
/// stays however long it was left; one beside them with none of those is
/// archived by the same pass.
// verifies: FLOW-CLEAN-UP-R10
#[tokio::test(flavor = "multi_thread")]
async fn attention_and_work_keep_a_session_unarchived() {
    let fake = FakeAdapter::new(1);
    let refusal = Arc::clone(&fake.delete_refusal);
    let (mut s, clock) = swept_with(fake, |_| {}).await;

    let (_, pinned, _) = left_alone(&mut s).await;
    let arrange = json!({ "id": pinned, "pinned": true });
    s.client.result("session.arrange", arrange).await;

    let (_, unseen, _) = left_alone(&mut s).await;
    let arrange = json!({ "id": unseen, "unseen": true });
    s.client.result("session.arrange", arrange).await;

    let (_, watched, _) = left_alone(&mut s).await;
    let mut window = s.core.client().await;
    let subscribe = json!({ "id": watched, "from_seq": 0 });
    window.result("session.subscribe", subscribe).await;

    // Its record's deletion is refused: marked deleting, `failed`. A
    // second session keeps the folder, so the delete takes no files.
    let (workspace, deleting, _) = left_alone(&mut s).await;
    s.stopped_session(&workspace).await;
    *refusal.lock().unwrap() = Some(OpenElsewhere::Yes { pids: vec![4242] });
    let changes = json!({ "id": workspace["id"] });
    let token = s.client.result("workspace.changes", changes).await["token"].clone();
    let delete = json!({ "id": deleting, "token": token });
    s.client.error("session.delete", delete).await;
    assert_eq!(row(&mut s, &deleting).await["deleting"], "failed");

    // A turn that keeps working, nobody watching it.
    let busy_workspace = s.workspace().await;
    let params = json!({ "workspace_id": busy_workspace["id"], "backend": fake::BACKEND });
    let busy = s.client.result("session.create", params).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let subscribe = json!({ "id": busy, "from_seq": 0 });
    s.client.result("session.subscribe", subscribe).await;
    s.client.events_until("session.started").await;
    let send = json!({ "id": busy, "text": "work" });
    s.client.result("session.send", send).await;
    s.client.events_until("turn.started").await;
    unsubscribe(&mut s, &busy).await;

    let (_, alone, _) = left_alone(&mut s).await;
    archive_after(&mut s, json!(1)).await;
    later(&clock, 31 * DAY).await;

    assert!(
        archived(&mut s, &alone).await,
        "the one left alone is archived"
    );
    for kept in [&pinned, &unseen, &watched, &deleting, &busy] {
        assert!(!archived(&mut s, kept).await, "{kept} was archived");
    }
    assert_eq!(
        row(&mut s, &busy).await["state"],
        "running",
        "its turn runs on"
    );

    // Once the window lets it go, the next pass archives it.
    window
        .result("session.unsubscribe", json!({ "id": watched }))
        .await;
    later(&clock, Duration::ZERO).await;
    assert!(archived(&mut s, &watched).await);
}

/// A session another process has open stays, and so does one whose
/// backend can't say; once no other process has it, it is archived.
// verifies: FLOW-CLEAN-UP-R10
#[tokio::test(flavor = "multi_thread")]
async fn a_session_another_process_has_open_stays() {
    let fake = FakeAdapter::new(1);
    let open = Arc::clone(&fake.open_elsewhere);
    let (mut s, clock) = swept_with(fake, |_| {}).await;
    let (_, session, _) = left_alone(&mut s).await;
    archive_after(&mut s, json!(1)).await;

    *open.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    later(&clock, 31 * DAY).await;
    assert!(!archived(&mut s, &session).await);

    *open.lock().unwrap() = OpenElsewhere::Unknown;
    later(&clock, Duration::ZERO).await;
    assert!(!archived(&mut s, &session).await);

    *open.lock().unwrap() = OpenElsewhere::No;
    later(&clock, Duration::ZERO).await;
    assert!(archived(&mut s, &session).await);
}

/// A live session nobody is subscribed to is stopped, as Archive stops
/// one, and archived, and the same pass puts its folder away.
// verifies: FLOW-CLEAN-UP-R10
#[tokio::test(flavor = "multi_thread")]
async fn a_live_session_nobody_watches_is_stopped_then_archived() {
    let (mut s, clock) = swept().await;
    let workspace = s.workspace().await;
    let params = json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND });
    let session = s.client.result("session.create", params).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let subscribe = json!({ "id": session, "from_seq": 0 });
    s.client.result("session.subscribe", subscribe).await;
    s.client.events_until("session.started").await;
    // The fake backend finishes a turn whose message is `idle`.
    let send = json!({ "id": session, "text": "idle" });
    s.client.result("session.send", send).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while row(&mut s, &session).await["state"] != "idle" {
        assert!(tokio::time::Instant::now() < deadline, "never idle");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    unsubscribe(&mut s, &session).await;
    let stops = s.stats.stops.load(Ordering::SeqCst);
    archive_after(&mut s, json!(14)).await;

    later(&clock, 14 * DAY + MARGIN).await;

    let after = row(&mut s, &session).await;
    assert_eq!(after["archived"], true);
    assert_eq!(after["state"], "ended", "{after}");
    assert_eq!(s.stats.stops.load(Ordering::SeqCst), stops + 1);
    put_away(&mut s, &workspace).await;
}
