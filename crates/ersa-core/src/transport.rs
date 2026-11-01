//! Transport layer: starts processes and carries their stdio on a host, by
//! local spawn or over SSH. Adapters and the workspace and VCS layer run through
//! it without knowing which transport they got.
