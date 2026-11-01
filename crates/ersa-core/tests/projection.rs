//! A session's projection as Core makes it when the session is opened: the
//! record's events, then the live stream's, merged on the backend's message
//! ids (PROTO-AGENT-EVENTS, Projection rules).

use ersa_adapter::{Direction, Projected, Projector, RawLine, RecordEvent};
use ersa_core::projection::View;
use ersa_protocol::events::{AssistantText, BackendPassthrough, Event, Input};
use serde_json::Value;

/// Projects a live line `msg:<id>` to an assistant text of message `<id>`,
/// and anything else to a passthrough with no message behind it.
struct Messages;

impl Projector for Messages {
    fn version(&self) -> u32 {
        1
    }

    fn project(&mut self, line: &RawLine) -> Vec<Projected> {
        match line.line.strip_prefix("msg:") {
            Some(id) => vec![Projected::of_message(Some(id.into()), text(id))],
            None => vec![Projected::from_line(
                line,
                Event::BackendPassthrough(BackendPassthrough {
                    backend: "test".into(),
                    kind: "line".into(),
                    payload: Value::String(line.line.clone()),
                }),
            )],
        }
    }
}

fn text(id: &str) -> Event {
    Event::AssistantText(AssistantText {
        block_id: format!("{id}/0"),
        text: id.into(),
        is_final: true,
    })
}

fn live(index: u64, line: &str) -> RawLine {
    RawLine {
        index,
        ts: format!("2026-11-01T00:00:0{index}.000Z"),
        dir: Direction::Backend,
        line: line.into(),
    }
}

// verifies: PROTO-AGENT-EVENTS-R12
#[test]
fn a_message_in_both_the_record_and_the_live_stream_yields_its_events_once() {
    let record = vec![RecordEvent {
        message_id: Some("m1".into()),
        ts: Some("2026-10-31T00:00:00.000Z".into()),
        backend_version: Some("1.0".into()),
        event: text("m1"),
    }];
    let (mut view, opened) = View::open("s-1", "test", Some(Box::new(Messages)), record);
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].source.input, Input::Record);

    // The live stream carries m1 again, then m2 and a line with no message.
    let mut events = Vec::new();
    for (index, line) in ["msg:m1", "msg:m2", "status"].into_iter().enumerate() {
        events.extend(view.live(&live(index as u64, line)));
    }
    let texts: Vec<(u64, Option<&str>)> = events
        .iter()
        .map(|e| (e.seq, e.source.message_id.as_deref()))
        .collect();
    assert_eq!(texts, [(1, Some("m2")), (2, None)]);
    assert!(events.iter().all(|e| e.source.input == Input::Live));
}
