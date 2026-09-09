import { Component, type ReactNode } from "react";

type Props = Readonly<{ children: ReactNode }>;
type State = Readonly<{ failed: boolean }>;

export class ErrorBoundary extends Component<Props, State> {
  public state: State = { failed: false };

  public static getDerivedStateFromError(): State {
    return { failed: true };
  }

  public componentDidCatch(): void {
    // Never send exception values to logs: future editor errors may contain user text.
    console.error("Cairn.md view failed");
  }

  public render(): ReactNode {
    if (this.state.failed) {
      return (
        <main className="fatal-error" role="alert">
          <h1>Cairn.md could not display this view</h1>
          <p>Your Markdown files were not changed. Restart Cairn.md to try again.</p>
        </main>
      );
    }

    return this.props.children;
  }
}
