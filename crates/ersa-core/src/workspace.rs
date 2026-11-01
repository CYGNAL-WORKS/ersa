//! Workspace and VCS: creates and destroys isolated workspaces for a project
//! behind a VCS interface. Git worktrees first, Perforce second. Executes through
//! the transport so remote hosts work the same way as the local one.
