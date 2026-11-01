//! Persistence: one local database per Core process holding projects, hosts,
//! workspaces, sessions, raw logs, the projection cache, pack cache metadata,
//! and settings. SQLite in the user's application data directory, with schema
//! migrations run at startup.
