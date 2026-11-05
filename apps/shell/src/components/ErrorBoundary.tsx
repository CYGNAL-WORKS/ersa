import { Component, type ReactNode } from "react";
import "./ErrorBoundary.css";

interface ErrorBoundaryProps {
  /** What failed to show, for the message: "This session", "The rail's lists". */
  what: string;
  /** A change of key (another session chosen) tries again. */
  resetKey?: string;
  /** Shown with the message, so a way forward survives the part that failed. */
  action?: ReactNode;
  children: ReactNode;
}

/**
 * Keeps a part of the window that failed to render from taking the rest
 * down: the rail and its foot stay usable when a session cannot be shown.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, { error?: Error; key?: string }> {
  state: { error?: Error; key?: string } = {};

  static getDerivedStateFromError(error: unknown) {
    // Whatever was thrown, even `null`, is an error to show.
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  static getDerivedStateFromProps(props: ErrorBoundaryProps, state: { error?: Error; key?: string }) {
    return props.resetKey === state.key ? null : { error: undefined, key: props.resetKey };
  }

  render() {
    if (this.state.error) {
      return (
        <div className="error-boundary">
          <p className="problem" role="alert">
            {this.props.what} could not be shown: {this.state.error.message}
          </p>
          {this.props.action}
        </div>
      );
    }
    return this.props.children;
  }
}
