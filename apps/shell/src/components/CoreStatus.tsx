import type { CoreView } from "../core/connection";
import { Button } from "./Button";
import { StatusTile } from "./StatusTile";

interface CoreStatusProps {
  view: CoreView;
  /** Attach again, after the first attach failed. A lost connection reattaches by itself. */
  onRetry: () => void;
}

/** Core's state in the rail: connecting, connected (with its readouts), reconnecting, or unavailable. */
export function CoreStatus({ view, onRetry }: CoreStatusProps) {
  switch (view.phase) {
    case "connecting":
      return <StatusTile title="Core" tone="pending" summary="Connecting…" />;
    case "connected": {
      const { hello, status, how, label } = view;
      const readouts = [
        { label: "Version", value: hello.core_version },
        { label: "API", value: hello.api_version },
        ...(status
          ? [
              { label: "Running", value: status.sessions_running },
              { label: "Waiting", value: status.sessions_waiting },
            ]
          : []),
      ];
      return (
        <StatusTile
          title="Core"
          tone="positive"
          summary={
            // Name a labelled endpoint, so a window attached to a replay or
            // a second Core never passes for the user's own.
            label === undefined
              ? `${how === "launched" ? "Started" : "Attached"} on ${hello.host.name}`
              : `Attached to ${label} on ${hello.host.name}`
          }
          readouts={readouts}
        />
      );
    }
    case "lost":
      return (
        <StatusTile title="Core" tone="danger" summary="Reconnecting…" detail={view.reason} />
      );
    case "failed":
      return (
        <StatusTile
          title="Core"
          tone="danger"
          summary="Unavailable"
          detail={view.message}
          action={<Button onClick={onRetry}>Retry</Button>}
        />
      );
  }
}
