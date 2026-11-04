# Copilot: lessons for later

These were seen through the Node SDK (`@github/copilot-sdk` 1.0.13) and, earlier, the ACP interface. Ersa uses the Rust SDK (ADR 0009), which wasn't in Ersa's repository when these were checked; Rust names here come from docs.rs `github-copilot-sdk` 1.0.15.

## Starting

- **Versions.** A negotiated protocol version doesn't prove that methods exist: `protocolVersion` stayed 3 while `session.permissions.setAllowAll` was removed, and `getStatus().version` returned `"0.0.1"` on runtime 1.0.83. Check the runtime's own `--version` against a floor, and report a -32601 error at start as "runtime too old". Bears on PROTO-COPILOT-SDK's open question on version mismatches and on ADR 0009's "the SDK's minimum CLI version becomes Ersa's".
- **Authentication.** An unauthenticated runtime starts and connects normally, so only an auth query tells (`getAuthStatus().isAuthenticated`). The credential chain falls back to the `gh` CLI's login, which can be another account with a smaller entitlement; the earlier failure shows only in the last-auth-errors fields. `copilot login` blocks and returns no URL, while the SDK's experimental `auth.login` steps return an `OpenUrl` step. Seen on runtime 1.0.83 and 1.0.88. Bears on FLOW-REMOTE-SESSION's open question on detecting authentication.
- **Models.** `listModels` follows the account's entitlement (it shrank to `auto` for a limited account on 1.0.88), and `supportedReasoningEfforts` differs per model, including a `none` level that the Node SDK's enum lacks and the Rust SDK types as a string. PROTO-CORE-API's "not detected on the host" doesn't fit.

## Turns

- **Sending during a turn.** `session.send` takes `mode: enqueue | immediate`; the resulting `user.message` carries the `messageId` the send returned and `delivery: idle | steering | queued`, and stopping the turn discards a held `immediate` message (runtime 1.0.83 and 1.0.95). The `messageId` answers PROTO-COPILOT-SDK's merge-id question for user messages.
- **Idle.** An attached background shell holds `session.idle` until the shell exits, so the tile's idle state needs a rule. `session.tasks.cancel` stops it; `session.shell.kill` knows only processes the host started.
- **Cost.** Copilot reports `totalNanoAiu`, which converts to dollars at nano-AIU / 1e11, and premium requests, for which `usage.updated` has no field.

## Permissions

For PROTO-COPILOT-SDK's open question on mapping: Copilot has two axes, the session mode (`interactive`, `plan`, `autopilot`) and the permission mode (`manual`, `assisted`, `allow-all`). `assisted` reliably needs `COPILOT_CLI_ENABLED_FEATURE_FLAGS=AUTO_APPROVAL`, because a per-session feature flag was dropped after the first turn on runtime 1.0.95. The judge's verdict rides on the `permission.requested` event, not the Node handler's argument; the Rust handler is unchecked.

## Session lists

Copilot's session list matched exact native paths: backslashes found 17 sessions, forward slashes 0 (ACP on 1.0.65; unchecked on the SDK). ARCH-CORE's "list the backend's sessions for a directory" states no path form.
