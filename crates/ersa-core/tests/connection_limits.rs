//! The limits a connection lives under (`PROTO-CORE-API`, Transport and
//! framing): the line cap, UTF-8 only, and the handshake deadline. The
//! checks are in the shared suite, which `replay.rs` also runs.

mod support;

use support::suite::{self, Target};

#[tokio::test(flavor = "multi_thread")]
async fn a_line_over_the_cap_gets_a_parse_error_and_closes() {
    suite::a_line_over_the_cap_gets_a_parse_error_and_closes(Target::Core).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_that_is_not_utf8_gets_a_parse_error_and_closes() {
    suite::a_line_that_is_not_utf8_gets_a_parse_error_and_closes(Target::Core).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_without_hello_is_closed_after_the_deadline() {
    suite::a_connection_without_hello_is_closed_after_the_deadline(Target::Core).await;
}
