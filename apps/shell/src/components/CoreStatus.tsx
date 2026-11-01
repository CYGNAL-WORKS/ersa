import type { CoreView } from "../core/connection";
import { Button } from "./Button";
import { StatusTile } from "./StatusTile";

interface CoreStatusProps {
  view: CoreView;
  /** Attach again, after the connection was lost or could not be made. */
  onRetry: () => void;
}

/** Core's state in the rail: connecting, connected (with its readouts), lost, or unavailable. */
export function CoreStatus({ view, onRetry }: CoreStatusProps) {
  switch (view.phase) {
    case "connecting":
      return <StatusTile title="Core" tone="pending" summary="Connecting…" />;
    case "connected": {
      const { hello, status, how } = view;
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
          summary={`${how === "launched" ? "Started" : "Attached"} on ${hello.host.name}`}
          readouts={readouts}
        />
      );
    }
    case "lost":
      return (
        <StatusTile
          title="Core"
          tone="danger"
          summary="Connection lost"
          detail={view.reason}
          action={<Button onClick={onRetry}>Reconnect</Button>}
        />
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
