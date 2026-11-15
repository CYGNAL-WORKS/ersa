//! What the adapter reports of a session's model, effort and permission
//! mode: a live session's from its stream (PROTO-CLAUDE-CODE-STREAM-R40),
//! any session's from its record (R57, R58, R59), and before a relaunch what
//! the first turn after each of Core's requests ran with (R60).

mod common;

use common::{fixture, log};
use ersa_adapter::Direction::{Backend, Host};
use ersa_adapter::{Projector, RawLine, RecordedSettings, RelaunchSettings, Requested};
use ersa_adapter_claude::ClaudeProjector;
use ersa_adapter_claude::record::{encoded_cwd, record_settings, summarize_session};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The live report after each line that changed it: the line's kind, then
/// the model and permission mode the projector reports.
fn live_changes(lines: &[RawLine]) -> Vec<(String, Option<String>, Option<String>)> {
    let mut projector = ClaudeProjector::new();
    let mut changes = Vec::new();
    let mut last = (None, None);
    for line in lines {
        projector.project(line);
        let now = (
            projector.reported_model().map(String::from),
            projector.reported_permission_mode().map(String::from),
        );
        if now != last {
            changes.push((kind(line), now.0.clone(), now.1.clone()));
            last = now;
        }
    }
    changes
}

/// `type/subtype` of a raw line, or `type` alone.
fn kind(line: &RawLine) -> String {
    let value: Value = serde_json::from_str(&line.line).unwrap();
    match (value["type"].as_str(), value["subtype"].as_str()) {
        (Some(t), Some(s)) => format!("{t}/{s}"),
        (Some(t), None) => t.to_string(),
        _ => String::new(),
    }
}

fn change(kind: &str, model: Option<&str>, mode: &str) -> (String, Option<String>, Option<String>) {
    (
        kind.to_string(),
        model.map(String::from),
        Some(mode.to_string()),
    )
}

const OPUS: &str = "claude-opus-5-5";
const HAIKU: &str = "claude-haiku-5-5";

/// Between turns (fixture 25): the mode from `initialize` before the first
/// turn, then the first `system/init`; the answers to `set_model` and
/// `set_permission_mode` change them at once, before the `system/status`
/// after the mode's answer and the next turn's `system/init` repeat them.
// verifies: PROTO-CLAUDE-CODE-STREAM-R40
#[test]
fn a_live_session_reports_each_new_model_and_mode_as_the_stream_gives_it() {
    let lines = fixture("2.1.316/25-controls-between-turns").raw_log();
    assert_eq!(
        live_changes(&lines),
        [
            change("control_response", None, "default"),
            change("system/init", Some(OPUS), "default"),
            change("control_response", Some(HAIKU), "default"),
            change("control_response", Some(HAIKU), "acceptEdits"),
        ]
    );
}

/// During a turn (fixture 26) the same answers change the report at once,
/// long before the next `system/init`.
// verifies: PROTO-CLAUDE-CODE-STREAM-R40
#[test]
fn controls_sent_during_a_turn_are_reported_from_their_answers() {
    let lines = fixture("2.1.316/26-controls-during-turn").raw_log();
    let changes = live_changes(&lines);
    assert_eq!(
        changes,
        [
            change("control_response", None, "default"),
            change("system/init", Some(OPUS), "default"),
            change("control_response", Some(HAIKU), "default"),
            change("control_response", Some(HAIKU), "acceptEdits"),
        ]
    );
}

/// A later `system/init` that reports something else wins over an earlier
/// answer: the stream's newest report is the one reported.
// verifies: PROTO-CLAUDE-CODE-STREAM-R40
#[test]
fn each_system_init_reports_the_model_and_mode_again() {
    let lines = log(&[
        (
            Backend,
            r#"{"type":"system","subtype":"init","model":"claude-opus-5-5","permissionMode":"plan","session_id":"s"}"#,
        ),
        (
            Backend,
            r#"{"type":"result","subtype":"success","is_error":false,"session_id":"s"}"#,
        ),
        (
            Backend,
            r#"{"type":"system","subtype":"init","model":"claude-sonnet-5-5","permissionMode":"auto","session_id":"s"}"#,
        ),
    ]);
    assert_eq!(
        live_changes(&lines),
        [
            change("system/init", Some(OPUS), "plan"),
            change("system/init", Some("claude-sonnet-5-5"), "auto"),
        ]
    );
}

/// Only the answers to the host's own `set_model` and
/// `set_permission_mode` name the model and mode: another request's answer
/// that happens to carry `model` or `mode` does not, a refused
/// `set_permission_mode` leaves the mode, and so does a status line without
/// `permissionMode`.
// verifies: PROTO-CLAUDE-CODE-STREAM-R40
#[test]
fn only_the_answers_to_the_hosts_changes_and_status_lines_with_a_mode_change_the_report() {
    let lines = log(&[
        (
            Backend,
            r#"{"type":"system","subtype":"init","model":"claude-opus-5-5","permissionMode":"default","session_id":"s"}"#,
        ),
        (
            Host,
            r#"{"type":"control_request","request_id":"other-1","request":{"subtype":"get_context_usage"}}"#,
        ),
        (
            Backend,
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"other-1","response":{"model":"claude-haiku-5-5","mode":"plan"}}}"#,
        ),
        (
            Backend,
            r#"{"type":"system","subtype":"status","status":"requesting","session_id":"s"}"#,
        ),
        (
            Host,
            r#"{"type":"control_request","request_id":"m-2","request":{"subtype":"set_model","model":"sonnet"}}"#,
        ),
        (
            Backend,
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"m-2","response":{"model":"claude-sonnet-5-5"}}}"#,
        ),
        (
            Host,
            r#"{"type":"control_request","request_id":"p-3","request":{"subtype":"set_permission_mode","mode":"plan"}}"#,
        ),
        (
            Backend,
            r#"{"type":"control_response","response":{"subtype":"error","request_id":"p-3","error":"not now"}}"#,
        ),
        (
            Host,
            r#"{"type":"control_request","request_id":"p-4","request":{"subtype":"set_permission_mode","mode":"auto"}}"#,
        ),
        (
            Backend,
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"p-4","response":{"mode":"auto"}}}"#,
        ),
    ]);
    assert_eq!(
        live_changes(&lines),
        [
            change("system/init", Some(OPUS), "default"),
            change("control_response", Some("claude-sonnet-5-5"), "default"),
            change("control_response", Some("claude-sonnet-5-5"), "auto"),
        ]
    );
}

// ---------------------------------------------------------------------------
// The record (PROTO-CLAUDE-CODE-STREAM-R57 to R59)

const SESSION: &str = "0b4ac7de-1f6e-4a39-9d52-5f1e0c3a7b11";
const CWD: &str = r"C:\work\repo";

/// A scratch Claude home holding one record of `lines`.
fn home_with(lines: &[String]) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("projects").join(encoded_cwd(CWD));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{SESSION}.jsonl")),
        lines.join("\n") + "\n",
    )
    .unwrap();
    home
}

/// The lines of a 2.1.316 record fixture.
fn record_lines(name: &str) -> Vec<String> {
    let path = common::fixtures_root().join(format!("2.1.316/record/{name}.jsonl"));
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect()
}

/// What a listing reports of the record of `lines`.
fn listed(lines: &[String]) -> RecordedSettings {
    let home = home_with(lines);
    summarize_session(home.path(), CWD, SESSION)
        .unwrap()
        .unwrap()
        .settings
}

/// What a relaunch reads back of the record of `lines`.
fn read_back(lines: &[String], requested: Requested) -> RelaunchSettings {
    let home = home_with(lines);
    record_settings(home.path(), CWD, SESSION, requested).unwrap()
}

fn at(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

/// The values alone, as Core would list them: model, effort, mode.
fn values(settings: &RecordedSettings) -> [Option<&str>; 3] {
    [&settings.model, &settings.effort, &settings.permission_mode]
        .map(|v| v.as_ref().map(|r| r.value.as_str()))
}

/// Core requested every value at `time`.
fn every(time: &str) -> Requested {
    Requested {
        model: Some(at(time)),
        effort: Some(at(time)),
        permission_mode: Some(at(time)),
    }
}

/// Fixture 25: controls sent between turns; the second turn ran with them,
/// and every value carries when that turn began (its prompt's time).
// verifies: PROTO-CLAUDE-CODE-STREAM-R57
// verifies: PROTO-CLAUDE-CODE-STREAM-R58
#[test]
fn the_record_reports_the_latest_turns_model_effort_and_mode() {
    let settings = listed(&record_lines("25-controls-between-turns"));
    assert_eq!(
        values(&settings),
        [Some(HAIKU), Some("low"), Some("acceptEdits")]
    );
    let began = Some(at("2026-11-14T01:07:03.322Z"));
    assert_eq!(settings.model.as_ref().unwrap().turn_began, began);
    assert_eq!(settings.effort.as_ref().unwrap().turn_began, began);
    assert_eq!(settings.permission_mode.as_ref().unwrap().turn_began, began);

    // Core's request just before the second turn began: that turn counts.
    let since = settings.clone().since(every("2026-11-14T01:07:03.300Z"));
    assert_eq!(
        values(&since),
        [Some(HAIKU), Some("low"), Some("acceptEdits")]
    );
    // After it began: no turn has begun since, so Core's request stands.
    let since = settings.since(every("2026-11-14T01:07:03.400Z"));
    assert_eq!(values(&since), [None, None, None]);
}

/// Fixture 26: `set_model` during the first turn applied at once, so that
/// turn's later replies are haiku and stamped after the request; but the
/// turn began before it, so nothing counts until the next turn begins.
// verifies: PROTO-CLAUDE-CODE-STREAM-R58
#[test]
fn a_reply_after_the_request_in_a_turn_that_began_before_it_does_not_count() {
    let lines = record_lines("26-controls-during-turn");
    let request = every("2026-11-14T01:08:01.397Z");

    // The record as it was when the first turn ended: its last reply
    // ("Done.") is haiku, from a turn that began at 01:08:00.326.
    let done = lines.iter().position(|l| l.contains("\"Done.\"")).unwrap();
    let settings = listed(&lines[..=done]);
    assert_eq!(
        values(&settings),
        [Some(HAIKU), Some("low"), Some("default")]
    );
    assert_eq!(values(&settings.since(request)), [None, None, None]);

    // The whole record: the second turn began after the request.
    let settings = listed(&lines);
    assert_eq!(
        values(&settings.since(request)),
        [Some(HAIKU), Some("low"), Some("acceptEdits")]
    );
}

/// Each value is weighed against Core's request for it alone; one Core
/// never requested counts from any turn.
// verifies: PROTO-CLAUDE-CODE-STREAM-R58
#[test]
fn each_value_is_weighed_against_its_own_request() {
    let settings = listed(&record_lines("25-controls-between-turns"));
    let requested = Requested {
        model: Some(at("2026-11-14T01:07:03.400Z")),
        effort: None,
        permission_mode: Some(at("2026-11-14T01:07:03.300Z")),
    };
    assert_eq!(
        values(&settings.since(requested)),
        [None, Some("low"), Some("acceptEdits")]
    );
}

fn prompt(ts: &str, mode: &str) -> String {
    json!({"type": "user", "uuid": format!("p{ts}"), "timestamp": ts, "permissionMode": mode,
        "message": {"role": "user", "content": "Go"}})
    .to_string()
}

fn reply(ts: &str, model: &str, effort: &str) -> String {
    json!({"type": "assistant", "uuid": format!("a{ts}"), "timestamp": ts, "effort": effort,
        "message": {"id": "m", "type": "message", "role": "assistant", "model": model,
            "content": [{"type": "text", "text": "Ok"}]}})
    .to_string()
}

/// Latest by `timestamp`, not by place in the file; a synthetic reply (a
/// local command's output, a failed sign-in) and a subagent's reply are not
/// the session's model.
// verifies: PROTO-CLAUDE-CODE-STREAM-R57
#[test]
fn the_latest_by_time_of_the_sessions_own_replies_counts() {
    let mut sidechain: Value =
        serde_json::from_str(&reply("2026-11-14T10:00:09.000Z", HAIKU, "low")).unwrap();
    sidechain["isSidechain"] = json!(true);
    let lines = [
        prompt("2026-11-14T10:00:05.000Z", "plan"),
        reply("2026-11-14T10:00:06.000Z", OPUS, "high"),
        // Written later, stamped earlier.
        prompt("2026-11-14T10:00:01.000Z", "auto"),
        reply("2026-11-14T10:00:02.000Z", "claude-sonnet-5-5", "low"),
        reply("2026-11-14T10:00:08.000Z", "<synthetic>", "max"),
        sidechain.to_string(),
    ];
    let settings = listed(&lines);
    assert_eq!(values(&settings), [Some(OPUS), Some("high"), Some("plan")]);
    assert_eq!(
        settings.model.unwrap().turn_began,
        Some(at("2026-11-14T10:00:05.000Z"))
    );
}

/// About 1 KiB each, holding no setting.
fn filler(count: usize) -> Vec<String> {
    let body = "x".repeat(1000);
    (0..count)
        .map(|_| json!({"type": "attachment", "attachment": {"body": body}}).to_string())
        .collect()
}

/// A listing reads only the tail: what only the rest of the record holds is
/// none there, and a relaunch reads back for it.
// verifies: PROTO-CLAUDE-CODE-STREAM-R59
#[test]
fn a_listing_reports_none_beyond_the_tail_and_a_relaunch_reads_back() {
    let mut lines = vec![
        prompt("2026-11-14T10:00:01.000Z", "plan"),
        reply("2026-11-14T10:00:02.000Z", OPUS, "high"),
    ];
    lines.extend(filler(300));
    assert_eq!(values(&listed(&lines)), [None, None, None]);
    let back = read_back(&lines, Requested::default()).latest;
    assert_eq!(values(&back), [Some(OPUS), Some("high"), Some("plan")]);
}

/// Before a relaunch, a value Core never requested is read back past the
/// turns after Core's other requests, as far as the record holds it.
// verifies: PROTO-CLAUDE-CODE-STREAM-R59
#[test]
fn a_relaunch_reads_back_as_far_as_each_request() {
    let mut lines = vec![
        prompt("2026-11-14T10:00:01.000Z", "plan"),
        reply("2026-11-14T10:00:02.000Z", OPUS, "high"),
    ];
    lines.extend(filler(300));
    // A turn that began before Core's requests and was interrupted before
    // any reply, which the tail holds: it settles the model, never asked
    // for after it, but not the effort, which Core never requested.
    lines.push(prompt("2026-11-14T10:00:30.000Z", "plan"));
    // A turn that began after Core's requests, with no reply yet.
    lines.push(prompt("2026-11-14T10:05:00.000Z", "acceptEdits"));
    let requested = Requested {
        model: Some(at("2026-11-14T10:01:00.000Z")),
        effort: None,
        permission_mode: Some(at("2026-11-14T10:01:00.000Z")),
    };
    let back = read_back(&lines, requested).latest.since(requested);
    assert_eq!(values(&back), [None, Some("high"), Some("acceptEdits")]);
}

/// Before a relaunch, the read stops once the record reaches Core's time
/// for every value: the old reply before that time is never read.
// verifies: PROTO-CLAUDE-CODE-STREAM-R59
#[test]
fn a_relaunch_stops_reading_at_cores_time() {
    let mut lines = vec![
        prompt("2026-11-14T10:00:01.000Z", "plan"),
        reply("2026-11-14T10:00:02.000Z", OPUS, "high"),
    ];
    lines.extend(filler(300));
    lines.push(prompt("2026-11-14T10:00:30.000Z", "plan"));
    lines.push(prompt("2026-11-14T10:05:00.000Z", "acceptEdits"));
    // Read as it is, before Core picks: the reply was never reached.
    let back = read_back(&lines, every("2026-11-14T10:01:00.000Z")).latest;
    assert_eq!(values(&back), [None, None, Some("acceptEdits")]);
}

/// Times are compared to the millisecond: a turn that began at Core's very
/// time did not begin after it, and a reply stamped with its prompt belongs
/// to that prompt's turn.
// verifies: PROTO-CLAUDE-CODE-STREAM-R58
#[test]
fn a_turn_that_began_at_cores_time_is_not_after_it() {
    let lines = [
        prompt("2026-11-14T10:00:05.000Z", "plan"),
        reply("2026-11-14T10:00:05.000Z", OPUS, "high"),
    ];
    let settings = listed(&lines);
    let began = Some(at("2026-11-14T10:00:05.000Z"));
    assert_eq!(settings.model.as_ref().unwrap().turn_began, began);
    assert_eq!(
        values(&settings.clone().since(every("2026-11-14T10:00:05.000Z"))),
        [None, None, None]
    );
    assert_eq!(
        values(&settings.since(every("2026-11-14T10:00:04.999Z"))),
        [Some(OPUS), Some("high"), Some("plan")]
    );
}

/// A tail that holds a reply but not the prompt its turn began with: the
/// value counts only when Core gave no time for it, since its turn may have
/// begun before that time.
// verifies: PROTO-CLAUDE-CODE-STREAM-R58
#[test]
fn a_value_whose_turns_start_was_not_read_counts_only_without_a_time() {
    let mut lines = vec![prompt("2026-11-14T10:00:01.000Z", "plan")];
    lines.extend(filler(300));
    lines.push(reply("2026-11-14T10:09:00.000Z", OPUS, "high"));
    let settings = listed(&lines);
    assert_eq!(settings.model.as_ref().unwrap().turn_began, None);
    assert_eq!(
        values(&settings.clone().since(Requested::default())),
        [Some(OPUS), Some("high"), None]
    );
    // Even a time long before the reply: its turn's start is unknown.
    assert_eq!(
        values(&settings.since(every("2026-11-14T10:00:00.000Z"))),
        [None, None, None]
    );
}

/// A session with no record has none.
// verifies: PROTO-CLAUDE-CODE-STREAM-R57
#[test]
fn a_session_with_no_record_reports_none() {
    let home = tempfile::tempdir().unwrap();
    let settings = record_settings(home.path(), CWD, SESSION, Requested::default()).unwrap();
    assert_eq!(settings, RelaunchSettings::default());
}

// ---------------------------------------------------------------------------
// The first turn after each request, before a relaunch (R60)

/// [`reply`], written by a subagent.
fn sidechain_reply(ts: &str, model: &str, effort: &str) -> String {
    let mut entry: Value = serde_json::from_str(&reply(ts, model, effort)).unwrap();
    entry["isSidechain"] = json!(true);
    entry.to_string()
}

/// [`prompt`], written to a subagent.
fn sidechain_prompt(ts: &str, mode: &str) -> String {
    let mut entry: Value = serde_json::from_str(&prompt(ts, mode)).unwrap();
    entry["isSidechain"] = json!(true);
    entry.to_string()
}

/// Two turns after Core's requests ran with different values: the first
/// reports the first's, the latest the second's, each with its turn's start.
// verifies: PROTO-CLAUDE-CODE-STREAM-R60
#[test]
fn a_relaunch_reports_the_first_turn_after_each_request_and_the_latest() {
    let lines = [
        prompt("2026-11-14T10:00:01.000Z", "default"),
        reply("2026-11-14T10:00:02.000Z", "claude-sonnet-5-5", "low"),
        // Core's requests at 10:01.
        prompt("2026-11-14T10:02:00.000Z", "plan"),
        reply("2026-11-14T10:02:01.000Z", OPUS, "high"),
        reply("2026-11-14T10:02:02.000Z", OPUS, "high"),
        prompt("2026-11-14T10:03:00.000Z", "acceptEdits"),
        reply("2026-11-14T10:03:01.000Z", HAIKU, "medium"),
    ];
    let back = read_back(&lines, every("2026-11-14T10:01:00.000Z"));
    assert_eq!(
        values(&back.first),
        [Some(OPUS), Some("high"), Some("plan")]
    );
    let first_began = Some(at("2026-11-14T10:02:00.000Z"));
    assert_eq!(back.first.model.as_ref().unwrap().turn_began, first_began);
    assert_eq!(back.first.effort.as_ref().unwrap().turn_began, first_began);
    assert_eq!(
        back.first.permission_mode.as_ref().unwrap().turn_began,
        first_began
    );
    assert_eq!(
        values(&back.latest),
        [Some(HAIKU), Some("medium"), Some("acceptEdits")]
    );
}

/// Each value's first turn is weighed against Core's time for it alone, and
/// a value Core gave no time for has no first turn.
// verifies: PROTO-CLAUDE-CODE-STREAM-R60
#[test]
fn each_values_first_turn_is_after_its_own_request() {
    let lines = [
        prompt("2026-11-14T10:01:00.000Z", "plan"),
        reply("2026-11-14T10:01:01.000Z", OPUS, "high"),
        prompt("2026-11-14T10:03:00.000Z", "acceptEdits"),
        reply("2026-11-14T10:03:01.000Z", HAIKU, "medium"),
    ];
    let requested = Requested {
        model: Some(at("2026-11-14T10:00:00.000Z")),
        effort: None,
        permission_mode: Some(at("2026-11-14T10:02:00.000Z")),
    };
    let back = read_back(&lines, requested);
    assert_eq!(values(&back.first), [Some(OPUS), None, Some("acceptEdits")]);
}

/// Fixture 26: the request came during a turn, whose later replies already
/// ran with it; that turn began before the request, so it is not the first
/// after it, which is the next turn.
// verifies: PROTO-CLAUDE-CODE-STREAM-R60
#[test]
fn a_turn_that_began_before_the_request_is_not_the_first_after_it() {
    let lines = record_lines("26-controls-during-turn");
    let request = every("2026-11-14T01:08:01.397Z");
    let done = lines.iter().position(|l| l.contains("\"Done.\"")).unwrap();
    // Only the turn that began before the request: no first turn yet.
    let back = read_back(&lines[..=done], request);
    assert_eq!(values(&back.first), [None, None, None]);
    // The next turn is the first after it.
    let back = read_back(&lines, request);
    assert_eq!(
        values(&back.first),
        [Some(HAIKU), Some("low"), Some("acceptEdits")]
    );
}

/// A subagent's prompts and replies are not the session's turns, even when
/// stamped before the session's own first turn after the request.
// verifies: PROTO-CLAUDE-CODE-STREAM-R60
#[test]
fn a_subagents_entries_are_not_the_first_turn() {
    let lines = [
        prompt("2026-11-14T10:00:01.000Z", "default"),
        sidechain_prompt("2026-11-14T10:01:30.000Z", "bypassPermissions"),
        sidechain_reply("2026-11-14T10:01:31.000Z", HAIKU, "low"),
        prompt("2026-11-14T10:02:00.000Z", "plan"),
        reply("2026-11-14T10:02:01.000Z", OPUS, "high"),
    ];
    let back = read_back(&lines, every("2026-11-14T10:01:00.000Z"));
    assert_eq!(
        values(&back.first),
        [Some(OPUS), Some("high"), Some("plan")]
    );
}

/// A turn interrupted before any reply ran with no model: the first turn
/// after the request that ran with one is the next.
// verifies: PROTO-CLAUDE-CODE-STREAM-R60
#[test]
fn a_turn_with_no_reply_gives_its_mode_but_no_model() {
    let lines = [
        prompt("2026-11-14T10:02:00.000Z", "plan"),
        prompt("2026-11-14T10:03:00.000Z", "acceptEdits"),
        reply("2026-11-14T10:03:01.000Z", OPUS, "high"),
    ];
    let back = read_back(&lines, every("2026-11-14T10:01:00.000Z"));
    assert_eq!(
        values(&back.first),
        [Some(OPUS), Some("high"), Some("plan")]
    );
}

/// The latest value lies in the tail, but the first turn after the request
/// lies further back: the read goes on to it.
// verifies: PROTO-CLAUDE-CODE-STREAM-R60
#[test]
fn a_relaunch_reads_back_to_the_first_turn_after_the_request() {
    let mut lines = vec![
        prompt("2026-11-14T10:00:01.000Z", "default"),
        reply("2026-11-14T10:00:02.000Z", "claude-sonnet-5-5", "low"),
        prompt("2026-11-14T10:02:00.000Z", "plan"),
        reply("2026-11-14T10:02:01.000Z", OPUS, "high"),
    ];
    lines.extend(filler(300));
    lines.push(prompt("2026-11-14T10:09:00.000Z", "acceptEdits"));
    lines.push(reply("2026-11-14T10:09:01.000Z", HAIKU, "medium"));
    let back = read_back(&lines, every("2026-11-14T10:01:00.000Z"));
    assert_eq!(
        values(&back.first),
        [Some(OPUS), Some("high"), Some("plan")]
    );
    assert_eq!(
        values(&back.latest),
        [Some(HAIKU), Some("medium"), Some("acceptEdits")]
    );
}
