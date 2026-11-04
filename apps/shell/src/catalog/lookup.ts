// The typed lookup components call to read the string catalog
// (docs/design/CONTENT.md, "The string catalog"). A key is the dotted path to a
// message, such as `quit.quitButton`. The types read each message's values out
// of its text, so a missing value or a misspelled one fails the typecheck
// rather than reaching the screen as `{count}`.

import { Fragment, createElement, type ReactElement, type ReactNode } from "react";

/**
 * A message with a count: whole messages keyed by `Intl.PluralRules` category
 * and chosen by `{count}`. `other` is the one category every locale has.
 */
export type Plural = { readonly other: string } & { readonly [C in Intl.LDMLPluralRule]?: string };

/** What a key leads to: one message, or a message with plural forms. */
export type Message = string | Plural;

/** An area of the catalog, or a sub-area: keys to messages and further sections. */
export interface Section {
  readonly [key: string]: Message | Section;
}

/** What may fill a value. An element, such as a readout, keeps its place in the sentence. */
export type Value = string | number | ReactElement;

/** Every dotted key in a catalog that leads to a message. */
export type Key<S> = {
  [K in keyof S & string]: S[K] extends Message ? K : `${K}.${Key<S[K]>}`;
}[keyof S & string];

/** The message a dotted key leads to. */
export type At<S, K extends string> = K extends `${infer Head}.${infer Rest}`
  ? Head extends keyof S
    ? At<S[Head], Rest>
    : never
  : K extends keyof S
    ? S[K]
    : never;

// The names in braces in a text: "{count} of {total}" gives "count" | "total".
// Distributes over a union, so a plural's forms give the names any form uses.
type Names<T> = T extends `${string}{${infer Name}}${infer Rest}` ? Name | Names<Rest> : never;

// The text of every form of a message.
type Texts<M> = M extends string ? M : M[keyof M];

// The values a message needs, each of type V. A plural always needs `count`,
// which chooses the form, even when no form prints it.
type Values<M, V> = M extends Plural
  ? { readonly count: number } & { readonly [N in Exclude<Names<Texts<M>>, "count">]: V }
  : { readonly [N in Names<Texts<M>>]: V };

// The lookup's arguments after the key: nothing for a message without values.
type Args<M, V> = M extends Plural ? [values: Values<M, V>] : [Names<M>] extends [never] ? [] : [values: Values<M, V>];

/**
 * Looks a key up and fills its values. With text and numbers only, the result
 * is a string, which suits an `aria-label` or a `title`; with an element among
 * the values it is a fragment that keeps the sentence's word order.
 */
export interface Lookup<C> {
  <K extends Key<C>>(key: K, ...values: Args<At<C, K>, string | number>): string;
  <K extends Key<C>>(key: K, ...values: Args<At<C, K>, Value>): ReactNode;
}

const isPlural = (entry: Message | Section | undefined): entry is Plural =>
  typeof entry === "object" && typeof entry.other === "string";

// The message at a dotted key. Unreachable through the types; it throws so a
// key built at runtime and wrong is found, not shown blank.
function messageAt(catalog: Section, key: string): Message {
  let entry: Message | Section | undefined = catalog;
  for (const part of key.split(".")) {
    entry = typeof entry === "object" && !isPlural(entry) ? entry[part] : undefined;
  }
  if (typeof entry === "string" || isPlural(entry)) return entry;
  throw new Error(`The string catalog has no message at "${key}"`);
}

/** A lookup into one locale's catalog; plural forms follow that locale's rules. */
export function lookupIn<C extends Section>(catalog: C, locale: string): Lookup<C> {
  const plurals = new Intl.PluralRules(locale);
  const numbers = new Intl.NumberFormat(locale);
  const lookup = (key: string, values: Readonly<Record<string, Value>> = {}): ReactNode => {
    const message = messageAt(catalog, key);
    let text: string;
    if (typeof message === "string") {
      text = message;
    } else {
      const count = values.count;
      if (typeof count !== "number") throw new Error(`"${key}" has plural forms and needs a count`);
      text = message[plurals.select(count)] ?? message.other;
    }
    // split() with a capture group alternates text and value names: even
    // indexes are text, odd ones name a value.
    const parts = text.split(/\{(\w+)\}/).map((part, index) => {
      if (index % 2 === 0) return part;
      const value = values[part];
      if (value === undefined) throw new Error(`"${key}" needs a value for {${part}}`);
      return typeof value === "number" ? numbers.format(value) : value;
    });
    if (parts.every((part) => typeof part === "string")) return parts.join("");
    // Passed as separate children, so React needs no keys for them.
    return createElement(Fragment, null, ...parts.filter((part) => part !== ""));
  };
  return lookup as Lookup<C>;
}
