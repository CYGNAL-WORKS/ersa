// Core keeps one subscription per session on a connection (PROTO-CORE-API, `session.subscribe`): a second subscribe
// replaces the first, and one unsubscribe ends it for everyone. The window can follow a session twice at once (the
// transcript, and the shelf while the new-session screen hides the transcript's session), so each follower gets its
// own view of the client, and a session is unsubscribed only when the last follower holding it lets go.

import type { CoreClient } from "../core/client";

/** Makes clients that share Core's subscriptions: call it once per follower. */
export type SharedSubscriptions = () => CoreClient;

export function sharedSubscriptions(client: CoreClient): SharedSubscriptions {
  // The followers holding each session's subscription.
  const holders = new Map<string, Set<object>>();
  return () => {
    const me = {};
    const call: CoreClient["call"] = (method, params) => {
      const id = (params as { id?: unknown } | undefined)?.id;
      if (typeof id === "string" && method === "session.subscribe") {
        holders.set(id, (holders.get(id) ?? new Set()).add(me));
      }
      if (typeof id === "string" && method === "session.unsubscribe") {
        const held = holders.get(id);
        held?.delete(me);
        // Another follower still reads this session: its subscription stays.
        if (held && held.size > 0) return Promise.resolve({}) as ReturnType<CoreClient["call"]>;
        holders.delete(id);
      }
      return client.call(method, params);
    };
    return { attach: (onEvent) => client.attach(onEvent), call };
  };
}
