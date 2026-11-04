# 0023: The repository is public under an organization, CI runs on hosted runners, and the merge review runs on the owner's machine outside Actions

- Status: accepted
- Date: 2026-11-04
- Related briefs: none

## Context

ADR 0022 made the merge review a GitHub Actions workflow on the owner's self-hosted Windows runner, where Claude Code is installed and logged in, because the repository was private on GitHub Free: no rulesets, no merge queue, and every CI job already ran on the owner's machine. The repository now moves to the CYGNAL-WORKS organization and becomes public, for GitHub's hosted runners, which are free for a public repository, and for a merge queue, which GitHub gives a public repository owned by an organization on any plan and no repository owned by a person. GitHub advises against self-hosted runners on a public repository, since a fork's pull request can run code on them, and the review workflow guards itself off once the repository is public. Running Claude Code inside Actions needs a credential in a repository secret: an API key billed apart from the owner's subscription, since the subscription's own token may not be used that way. The review script runs by hand today with the same command the workflow used.

## Options considered

1. **Review on a hosted runner with an API key in a secret.** No machine to keep online. Costs API billing on top of the subscription, puts the key within reach of any same-repository pull request's tests, and adds an account to manage.
2. **Keep the review workflow on the owner's runner, same-repository pull requests only.** Nothing to build. Keeps a self-hosted runner registered to a public repository, which GitHub discourages, and the review still waits while the machine is off.
3. **Run the review on the owner's machine outside Actions, and merge through the merge queue.** A poller, started by a scheduled task, runs the existing review script on each pull request whose CI passed and whose head is not yet reviewed, one at a time; a clean head is added to the merge queue, where CI runs on the actual merge before main moves. The owner's own login and `gh` token, as the shepherd once used and ADR 0022 accepted; no secret, no runner, GitHub still the only record. Reviews post under the owner's account, and a superseded head finishes its review before the next starts, which the gate's head match makes harmless.
4. **No merge-time review.** The author's review, the queue and the owner's merge. Loses the fresh-context review every pull request gets, which caught three defects in a 60-line pull request (#68).

## Decision

Option 3. CI and the daily dependency scan run on GitHub-hosted runners, with a `merge_group` trigger. A ruleset on `main` requires a pull request, the CI checks and the Merge review status, and the merge queue. The review script posts its verdict as a commit status named "Merge review", since a person's token may not create check runs, and adds a clean head to the queue instead of merging; the gate's rule that the head contains main's tip retires, because the queue tests the merge itself. Only pull requests from this repository are reviewed, never a fork's; `needs-owner` and `hold` keep their meaning from ADR 0022. The review workflow and the runner registrations are retired. Until the poller lands, the owner runs the review script by hand.

## Consequences

Reviews appear as the owner's, not a bot's, and the poller runs only while the owner's machine is on, as the workflow did. Fork pull requests get CI with a read-only token and nothing else, so the owner decides each one. The poller is a short script and a scheduled task, cut as a ticket; a merge queue entry that fails CI is removed by GitHub and needs a push. Rewriting the review's check run as a commit status and the gate's trimming are part of that ticket. Revisit when Ersa can run the review as one of its own sessions, which is the product's own job; if the machine's availability holds pull requests up; or if API billing becomes acceptable, when option 1 is the smaller change.
