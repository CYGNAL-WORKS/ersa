//! Core API: the request/response surface the Interface uses, for projects,
//! hosts, workspaces, sessions, settings, files, and diffs. Schema-defined; the
//! Interface's TypeScript types are generated from the same schema
//! (`PROTO-CORE-API`). The wire types live in the `ersa-protocol` crate; this
//! module will answer requests with them.
