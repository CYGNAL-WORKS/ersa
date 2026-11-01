//! Raw log and projection: every line a backend emits is stored verbatim per
//! session (ADR 0010). Ersa events are a versioned projection derived from the
//! raw log, cached, and re-derived when the projection version changes.
//! Transcript, rail state, and resume are all projections.
