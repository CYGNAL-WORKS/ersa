# Drive the pull request through the merge review

Step 5 of `implement-ticket`. Follow it from the moment the pull request is open until a row or a bullet below says to stop.

## Ask Docket for the state

Ask Docket for your pull request's state, never GitHub, with a 10-minute tool timeout:

```bash
curl -sS --max-time 600 "http://127.0.0.1:4477/pulls/<n>?head=<sha of your last push>&wait=540"
```

Docket holds the request until your head reaches a state in the table below, or for 9 minutes, and answers with the pull request's state as JSON. Act only on an answer whose `head` starts with your sha: one that carries a `note` still describes your previous head. Any other answer, including a 404 for a pull request Docket has not synced yet, means ask again. When curl cannot connect, Docket is not running: report that to the owner and stop; never guess the state from GitHub.

| State | Do |
| --- | --- |
| `ci-failed` | Fix the checks it lists and push. CI runs on GitHub-hosted Linux and Windows runners: a test that assumes a platform's path form or locale fails on the other one. When the only failure is a test your change does not reach and it passes here, it failed by chance: add the run's link to that test's issue (`gh issue list --search "<test name>"`; file one labeled `needs-triage` when none exists), rerun the failed jobs once (`gh run list --branch <branch>` gives the run's id; `gh run rerun <id> --failed`, then `gh run watch <id>`), and ask Docket again. |
| `conflicts` | `git merge origin/main`, resolve, run the checks again and push. When the manifest check then reports a brief line the merge kept twice, only a planning pass can remove it: message the session that started you as the `waits-on-planning` row says. |
| `dequeued` | The merge queue's own run on the merge with `main` failed twice. Read that run on the pull request, fix what is yours and push. When it is a test your change does not reach, add the run's link to that test's issue, report to the owner and stop. |
| `findings` | Fix every high and medium finding, push, and wait again (below). |
| `merged` | Done. |
| `waits-on-owner` | When the `reason` is review rounds spent and the state lists findings, treat it as `findings` (see the last bullet of Fixing findings). Otherwise report the `reason` and stop. |
| `waits-on-planning` | Send one message with `SendMessage` to the session that started you (`get_session` on `self` gives its id as `parentSessionId`): the pull request, the `reason`, and your inbox lines verbatim. It runs the planning pass. Then report and stop. When no session started you, report and stop. |
| `error`, twice in a row on the same head | Report the `reason` to the owner and stop. |

## Fixing findings

- Fix each with the smallest change that removes the defect and a test that fails without the fix, proven as step 3 says: the review blocks a fix no test reaches, which costs a round. When no test can reach it, say why in the pull request. Fix low ones when trivial and list the rest as follow-ups in the pull request.
- A fix that needs a new mechanism (a cache, a background task, a protocol field, a state machine or reducer) becomes its own ticket unless the branch is unsafe to merge without it: new mechanisms bring their own defects. So does a reshape the review names that is larger than the ticket. A gap the brief does not cover gets an inbox line when your code depends on it, and is a follow-up otherwise.
- State you are already patching is the exception. When a round finds a defect in the last round's fix, or a second order of the same two events, another flag opens a third: list every state and every event that can arrive in it as a table in the plan, test each row, and rebuild the fix from the table, removing the flags it replaces. Between two of Core's operations the table exists: add an input and its rows to admission, never a lock, flag or guard (ADR 0037).
- A finding on a planning-only path, or one that contradicts a harness rule, is the owner's: report it and stop rather than spend a round on it.
- A finding marked as a follow-up goes in the pull request's follow-ups; it takes no round.
- After the fifth round (`round` in the state), keep fixing while each round finds new defects you can fix within the ticket. Stop and report to the owner in plain English when a finding needs a decision `CLAUDE.md` leaves to the owner, or a round finds again a defect you already fixed.

## Pushing

- Never force-push: a re-review reads the commits since the last reviewed head. Bring `main` in with `git merge origin/main`.
- While your head is `queued` or `reviewing`, push nothing: a new head cancels the running review, and the next one restates the same findings. Hold the commit (an inbox line closed, baselines, a merge of main) and push it with the fixes. A push that is not meant to be reviewed goes behind `gh pr ready --undo` first, since Docket skips a draft; `gh pr ready` does not rerun CI. A turn never ends on a draft.
- Always put the head you just pushed in the request's `head=`: Docket syncs about once a minute, and until then it still describes the old head.
