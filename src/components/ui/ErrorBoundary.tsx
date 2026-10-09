import React, { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "./Button";
import { AlertTriangle, RefreshCw } from "lucide-react";

interface Props {
  children: ReactNode;
  fallbackTitle?: string;
  onReset?: () => void;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  public override state: State = {
    hasError: false,
    error: null,
  };

  public static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  public override componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("Uncaught error caught by boundary:", error, errorInfo);
  }

  private handleReset = () => {
    this.setState({ hasError: false, error: null });
    if (this.props.onReset) {
      this.props.onReset();
    }
  };

  public override render() {
    if (this.state.hasError) {
      return (
        <div
          role="alert"
          aria-live="assertive"
          className="p-6 m-4 max-w-lg mx-auto bg-red-50/60 dark:bg-red-950/30 rounded-2xl text-left"
        >
          <div className="flex items-center gap-3 mb-3 text-status-error font-semibold">
            <AlertTriangle className="w-5 h-5 shrink-0" />
            <h2 className="text-base">{this.props.fallbackTitle || "Something went wrong in this view"}</h2>
          </div>
          <p className="text-xs text-ink-muted mb-4 font-mono bg-surface-sunken p-2.5 rounded-lg overflow-x-auto">
            {this.state.error?.message || "An unexpected error occurred."}
          </p>
          <div className="flex items-center gap-3">
            <Button variant="secondary" size="sm" onClick={this.handleReset}>
              <RefreshCw className="w-3.5 h-3.5 mr-1.5" />
              Retry View
            </Button>
            <Button variant="ghost" size="sm" onClick={() => window.location.reload()}>
              Reload Window
            </Button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
