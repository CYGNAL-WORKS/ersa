# 0024: Ersa is licensed under the MIT license

- Status: accepted
- Date: 2026-11-04
- Related briefs: none

## Context

The repository became public (ADR 0023), and a public repository without a license file is all rights reserved: visitors may read and fork under GitHub's terms and do nothing else. The owner is the sole author and takes no outside code for now, so the choice can change later without anyone's consent. The AGPL-3.0 was committed first and reconsidered the same day as more restrictive than intended. The vendored skills are MIT (`.claude/skills/THIRD-PARTY.md`) and keep their license whatever Ersa's is.

## Options considered

1. **MIT.** Anyone may use, change and distribute Ersa, in closed products too, keeping the copyright notice. The shortest license people know, and the one most of Ersa's own dependencies use.
2. **MIT or Apache-2.0 at the user's option**, the Rust convention. Adds Apache's patent grant and contribution terms. Two files and a longer notice for a benefit that matters once there are outside contributors and patents to speak of.
3. **MPL-2.0.** Changes to Ersa's own files must be published; the rest of a product may stay closed. Keeps improvements flowing back at the cost of a license fewer people know.
4. **AGPL-3.0.** A modified Ersa offered to others, even over a network, must publish its source. Keeps forks open and deters repackaging, but some organizations refuse AGPL software outright.
5. **No license.** Public for the free runners and merge queue only. Misleads visitors, who assume a public repository may be used.

## Decision

Option 1. The copyright holder is the owner.

## Consequences

Anyone may build on Ersa, including a closed fork that competes with it; the owner accepts that for adoption's sake. Relicensing stays possible while every line is the owner's or carries an agreement to it, which is why outside code is not taken without one. Revisit if Ersa gains a commercial edition, when option 4 or a source-available license would be the change, or if outside contributions begin, when option 2's contribution terms become worth their notice.
