# 0040: A person signs in to a backend from inside Ersa, through the backend's own sign-in

- Status: accepted
- Date: 2026-11-25
- Related briefs: PRD-ERSA, FLOW-SIGN-IN, PROTO-CORE-API, PROTO-CLAUDE-CODE-STREAM

## Context

Ersa was planned for people who arrive with a backend installed and signed in: ADR 0004's context says so, ADR 0014 speaks of "the user's own login", and the glossary said the user "installs and authenticates it themselves". A backend that is installed and not signed in passed detection, and its session failed at the first message with the CLI's own words, "Please run /login", which nothing in Ersa can do (#622). The only way on was a terminal, which the non-technical people `PRD-ERSA` names do not have. The owner's decision of 2026-11-25, on a machine with the CLI installed and not signed in: "It should auth through Ersa."

## Options considered

1. **Send the person to a terminal**, as today. Nothing to build. It fails everyone who does not use one, and the owner refused it.
2. **Ersa takes a token or key and hands it to the backend**, as a CLI's long-lived token command or an environment variable allows. Works without a browser. Ersa would then see and hold a credential, which `PRD-ERSA-R01` rules out, and it would be a second way to sign in beside the backend's own.
3. **Ersa shows the vendor's sign-in page in a window of its own.** The person never leaves Ersa. The page's address is hidden from them, their password manager and existing browser login are not there, and what they type passes through Ersa's window.
4. **Ersa starts the backend's own sign-in and waits to learn how it ended**, the vendor's page opening in the person's own browser.

## Decision

Option 4. A person signs in to a backend from inside Ersa, with no terminal. The backend owns the sign-in: Ersa starts it through the backend's adapter, shows that it is under way, and learns that it ended and how. Ersa never sees, stores or relays a credential, a code or a token, offers no field that takes one, and adds no sign-in concept of its own: whether a backend is signed in is the backend's report each time Ersa asks, never a state Ersa keeps.

ADR 0004 stands: Ersa drives the vendor CLIs and holds no key. This record changes what its context assumed, and the glossary's sentence that the user authenticates the backend themselves. ADR 0014's reading stands too: the sign-in is the person's own, made by them at their own machine, and a session where nobody is signed in is still the case that record leaves for a decision of its own.

## Consequences

The first run needs no terminal once a backend is installed (#580 plans installing one). Each adapter must say how its backend reports that it is signed out and how its sign-in is started, from a recording of the real thing, and recording a sign-in needs a person to sign in with an account, which is the owner's to do. A backend with no sign-in that a host can start is one Ersa cannot sign in to, and its person is sent to the backend's own tools; that would be a reason to revisit option 2 for that backend alone. A sign-in on a remote host (M4) opens a page in the person's browser while the backend runs elsewhere, which this record does not settle.
