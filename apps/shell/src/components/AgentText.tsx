import { Check, Copy, Square, SquareCheck } from "lucide-react";
import { type ReactNode, memo, useMemo, useState } from "react";
import Markdown, { type Components, type ExtraProps } from "react-markdown";
import remarkGfm from "remark-gfm";
import { t } from "../catalog";
import { Icon } from "./Icon";
import "./AgentText.css";

/** Opens an address in the system's default handler; only ever given one `isOpenable` accepts. */
export type OpenLink = (url: string) => void;

// GitHub's extensions: tables, strikethrough, task lists and autolinks (FLOW-READ-TRANSCRIPT-R01). No plugin that
// passes raw HTML through is ever added (ADR 0034): without one, react-markdown turns raw HTML into text.
const PLUGINS = [remarkGfm];

/** The schemes a link may open (FLOW-READ-TRANSCRIPT-R03): a web address in the browser, an email address in the mail app. */
const OPENABLE = new Set(["http:", "https:", "mailto:"]);

/** Whether a link's address may be opened: an absolute address with an openable scheme. A relative one never is. */
export function isOpenable(href: string): boolean {
  try {
    return OPENABLE.has(new URL(href).protocol);
  } catch {
    return false;
  }
}

/**
 * The agent's words as formatted text (FLOW-READ-TRANSCRIPT-R01; docs/design/TRANSCRIPT.md, Formatted text): Markdown
 * with GitHub's extensions, built into React elements and never inserted as HTML (ADR 0034). Memoized on its text, so
 * while the agent streams only the growing message is parsed again; React keeps the elements of what came before, so
 * earlier text does not change as the rest arrives (R05).
 */
export const AgentText = memo(function AgentText({ text, onOpenLink }: { text: string; onOpenLink?: OpenLink }) {
  const components = useMemo(() => componentsFor(onOpenLink), [onOpenLink]);
  return (
    <div className="agent-text">
      <Markdown remarkPlugins={PLUGINS} components={components}>
        {text}
      </Markdown>
    </div>
  );
});

/** The elements Ersa draws its own way: links, images, code blocks, tables and task boxes. */
function componentsFor(onOpenLink?: OpenLink): Components {
  return {
    a: ({ href, children }) => (
      <AgentLink href={href} onOpenLink={onOpenLink}>
        {children}
      </AgentLink>
    ),
    // An image is never loaded, so the agent's words can't make the window fetch anything (R02, ADR 0034): it shows
    // as its alternative text linked to its address, or as the address when it has no alternative text.
    img: ({ src, alt }) => {
      const address = typeof src === "string" ? src : undefined;
      return (
        <AgentLink href={address} onOpenLink={onOpenLink}>
          {alt || address}
        </AgentLink>
      );
    },
    pre: ({ node }) => {
      const { language, text } = codeOf(node);
      return <CodeBlock language={language} text={text} />;
    },
    // A wide table scrolls sideways inside the column rather than widening it.
    table: ({ node: _node, ...props }) => (
      <div className="agent-table">
        <table {...props} />
      </div>
    ),
    // A task item's box, drawn and unchangeable (TRANSCRIPT.md, Lists). Other inputs never come from Markdown.
    input: ({ type, checked }) =>
      type === "checkbox" ? (
        <span className="agent-task-box" role="checkbox" aria-checked={checked === true} aria-disabled="true">
          <Icon icon={checked ? SquareCheck : Square} size="icon-sm" />
        </span>
      ) : null,
  };
}

/**
 * A link in the agent's words. One with an openable scheme opens through the Shell, never in the window (R03); any
 * other shows as its text and opens nothing.
 */
export function AgentLink({
  href,
  onOpenLink,
  force,
  children,
}: {
  href?: string;
  onOpenLink?: OpenLink;
  /** For the gallery: show a pointer or keyboard state without one. */
  force?: "hover" | "focus";
  children: ReactNode;
}) {
  if (href === undefined || !isOpenable(href)) return <>{children}</>;
  return (
    <a
      className="agent-link"
      href={href}
      title={href}
      data-force={force}
      onClick={(event) => {
        // The window never follows the link itself; the Shell hands it to the system.
        event.preventDefault();
        onOpenLink?.(href);
      }}
    >
      {children}
    </a>
  );
}

/** A minimal view of the syntax tree's nodes, enough to read a code block's text. */
type TreeNode = { type: string; value?: string; tagName?: string; properties?: Record<string, unknown>; children?: TreeNode[] };

/** The text under a node, as it was written. */
function textOf(node: TreeNode | undefined): string {
  if (!node) return "";
  if (node.type === "text") return node.value ?? "";
  return (node.children ?? []).map(textOf).join("");
}

/**
 * A code block's language (from its `language-…` class, which a fence's info string gives) and its text without its
 * fences. The tree ends a block's text with one newline the Markdown did not have, which is dropped (R04).
 */
function codeOf(pre: ExtraProps["node"]): { language?: string; text: string } {
  const tree = pre as TreeNode | undefined;
  const code = tree?.children?.find((child) => child.type === "element" && child.tagName === "code");
  const classes = code?.properties?.className;
  const language = Array.isArray(classes)
    ? classes.map(String).find((name) => name.startsWith("language-"))?.slice("language-".length)
    : undefined;
  return { language, text: textOf(code ?? tree).replace(/\n$/, "") };
}

/**
 * A code block (TRANSCRIPT.md, Formatted text): a head row with its language and Copy, then its code, unwrapped and
 * without syntax colors. Copy puts the code on the clipboard exactly (R04), then shows a check and says "Copied" until
 * the pointer leaves it or focus moves.
 */
export function CodeBlock({
  language,
  text,
  force,
  copied: forceCopied,
}: {
  language?: string;
  text: string;
  /** For the gallery: show Copy under the pointer, pressed, or with keyboard focus, without one. */
  force?: "hover" | "focus" | "pressed";
  /** For the gallery: show Copy as it is just after copying. */
  copied?: boolean;
}) {
  const [copiedNow, setCopied] = useState(false);
  const copied = copiedNow || forceCopied === true;
  const label = copied ? t("transcript.markdown.copiedLabel") : t("transcript.markdown.copyCodeButton");
  const copy = () => {
    // A refused write leaves Copy as it was: it says "Copied" only once the code is on the clipboard.
    navigator.clipboard.writeText(text).then(
      () => setCopied(true),
      () => {},
    );
  };
  const reset = () => setCopied(false);
  return (
    <div className="code-block">
      <div className="code-block-head">
        <span className="code-block-language">{language}</span>
        <button
          type="button"
          className="icon-button code-block-copy"
          aria-label={label}
          title={label}
          data-force={force}
          data-copied={copied || undefined}
          onClick={copy}
          onPointerLeave={reset}
          onBlur={reset}
        >
          {/* Both icons are drawn, one over the other, so the change between them can crossfade. */}
          <span className="code-block-copy-icons">
            <Icon icon={Copy} size="icon-sm" />
            <Icon icon={Check} size="icon-sm" />
          </span>
        </button>
      </div>
      <pre className="code-block-code">
        <code>{text}</code>
      </pre>
    </div>
  );
}
