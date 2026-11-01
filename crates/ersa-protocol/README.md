# ersa-protocol

The Rust types for the two protocols between the Interface and Core: the Core API and the agent event stream. JSON Schema is the source of truth for both; these types implement the schemas, and the Interface's TypeScript types are generated from the same schemas. For now the crate holds only the protocol version and a placeholder type. It must depend on nothing but serde: no Core internals, no async runtime, no Tauri, and nothing platform-specific.
