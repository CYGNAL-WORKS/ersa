#!/usr/bin/env node
// A stand-in for the Claude Code CLI, for the sign-in recorder's tests
// (scripts/record-claude-sign-in.test.mjs). It plays the sign-in the way the
// recorder guesses the real CLI does, and nothing else is evidence of the
// real CLI. Every value it prints is invented.
//
//   --version                       prints a version
//   auth login ...                  prints a sign-in address, then waits for the person
//   auth logout                     removes the credential
//   (anything else)                 a stream-json session: initialize, get_settings,
//                                   claude_authenticate, claude_oauth_wait_for_completion,
//                                   control_cancel_request and user messages
//
// The person signs in when the file FAKE_SIGN_IN_TRIGGER appears (the tests'
// stand-in browser writes it): the credential is then written to
// CLAUDE_CONFIG_DIR, and a later process there starts signed in.
// FAKE_SIGN_IN_PLAY, a comma-separated list, changes what it plays:
//   refuse-control   claude_authenticate answers an error
//   silent-control   claude_authenticate is never answered
//   no-address       auth login prints no address and exits 1
//   leak-token       a line carrying a token is written when the sign-in finishes
//   leak-name        the email's local part and the machine's user name are written
//                    to stderr when the sign-in finishes
// A key in the environment (ANTHROPIC_API_KEY) shows in `account` as the real
// CLI shows one (2.1.330/01-probe-key-in-environment).

import { existsSync, rmSync, writeFileSync } from "node:fs";
import { userInfo } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";

const PLAY = new Set((process.env.FAKE_SIGN_IN_PLAY ?? "").split(",").filter(Boolean));
const CONFIG = process.env.CLAUDE_CONFIG_DIR;
const CREDENTIAL = CONFIG && join(CONFIG, ".credentials.json");
const TRIGGER = process.env.FAKE_SIGN_IN_TRIGGER;
const EMAIL = "quillon.doe@mail.example";
const ACCOUNT = { email: EMAIL, organization: `${EMAIL}'s Organization`, subscriptionType: "max", apiProvider: "firstParty" };
const SIGNED_OUT = process.env.ANTHROPIC_API_KEY
  ? { tokenSource: "none", apiKeySource: "ANTHROPIC_API_KEY", apiProvider: "firstParty" }
  : { tokenSource: "none", apiProvider: "firstParty" };
const FAKE_TOKEN = "sk-ant-oat01-" + "Zq3vXk9TmB2wLp7RcY4nHs8dJf6GaE1u".repeat(3);

const signedIn = () => !!CREDENTIAL && existsSync(CREDENTIAL);
const say = (obj) => process.stdout.write(JSON.stringify(obj) + "\n");

// A sign-in address as the real one is shaped, with fresh one-time values.
function address(redirect) {
  const random = () => Array.from({ length: 43 }, () => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"[Math.floor(Math.random() * 64)]).join("");
  return `https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e&response_type=code&redirect_uri=${encodeURIComponent(redirect)}&scope=user%3Aprofile&code_challenge=${random()}&code_challenge_method=S256&state=${random()}`;
}

// Resolves once the person has signed in in the stand-in browser.
function personSignsIn() {
  return new Promise((resolve) => {
    const timer = setInterval(() => {
      if (!TRIGGER || !existsSync(TRIGGER)) return;
      clearInterval(timer);
      rmSync(TRIGGER, { force: true });
      writeFileSync(CREDENTIAL, JSON.stringify({ claudeAiOauth: { accessToken: FAKE_TOKEN } }));
      if (PLAY.has("leak-token")) say({ type: "system", subtype: "debug", accessToken: FAKE_TOKEN });
      if (PLAY.has("leak-name")) process.stderr.write(`welcome back, ${EMAIL.split("@")[0]}; cache in D:\\builds\\${userInfo().username}\\cache\n`);
      resolve();
    }, 20);
  });
}

const args = process.argv.slice(2);
if (args[0] === "--version") {
  console.log("0.0.0-fake (Claude Code)");
} else if (args[0] === "auth" && args[1] === "login") {
  console.log("Opening browser to sign in…");
  if (PLAY.has("no-address")) process.exit(1);
  console.log(`If the browser didn't open, visit: ${address("https://console.anthropic.com/oauth/code/callback")}`);
  // Stdin stays open and unread, as for a host that relays no code.
  await personSignsIn();
  console.log("Login successful.");
  process.exit(0);
} else if (args[0] === "auth" && args[1] === "logout") {
  rmSync(CREDENTIAL, { force: true });
  console.log("Successfully logged out from your Anthropic account.");
} else {
  const lines = createInterface({ input: process.stdin });
  lines.on("line", (line) => {
    const msg = JSON.parse(line);
    const answer = (response) => say({ type: "control_response", response: { subtype: "success", request_id: msg.request_id, response } });
    const fail = (error) => say({ type: "control_response", response: { subtype: "error", request_id: msg.request_id, error } });
    const request = msg.request ?? {};
    if (msg.type === "control_request" && request.subtype === "initialize") {
      answer({ commands: [], models: [{ value: "default", displayName: "Default" }], account: signedIn() ? ACCOUNT : SIGNED_OUT });
    } else if (msg.type === "control_request" && request.subtype === "get_settings") {
      answer({ applied: { effort: "medium" } });
    } else if (msg.type === "control_request" && request.subtype === "claude_authenticate") {
      if (PLAY.has("silent-control")) return;
      if (PLAY.has("refuse-control")) fail("Signing in during a session isn't available in this host yet. Sign in first, then start the session.");
      else answer({ manualUrl: address("https://console.anthropic.com/oauth/code/callback"), automaticUrl: address("http://localhost:54545/callback") });
    } else if (msg.type === "control_request" && request.subtype === "claude_oauth_wait_for_completion") {
      personSignsIn().then(() => answer({ account: ACCOUNT }));
    } else if (msg.type === "user") {
      if (signedIn()) {
        say({ type: "assistant", message: { model: "claude-fake", content: [{ type: "text", text: "ok" }] } });
        say({ type: "result", subtype: "success", is_error: false, result: "ok" });
      } else {
        say({ type: "assistant", message: { model: "<synthetic>", content: [{ type: "text", text: "Not logged in · Please run /login" }] }, error: "authentication_failed" });
        say({ type: "result", subtype: "success", is_error: true, result: "Not logged in · Please run /login" });
      }
    }
    // control_cancel_request: withdrawn, nothing to answer.
  });
  lines.on("close", () => process.exit(0));
}
