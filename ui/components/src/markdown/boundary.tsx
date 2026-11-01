import { Component } from "react";
import type { ReactNode } from "react";

type Props = { source: string; children: ReactNode };
type State = { failed: boolean; source: string };

/**
 * One block's error boundary: if rendering the block throws, the block shows its source as plain text and
 * the rest of the answer stays. A new source (the block grew, or changed) tries rendering again.
 */
export class BlockBoundary extends Component<Props, State> {
  override state: State = { failed: false, source: this.props.source };

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.source === state.source ? null : { failed: false, source: props.source };
  }

  static getDerivedStateFromError(): Partial<State> {
    return { failed: true };
  }

  override render() {
    if (!this.state.failed) return this.props.children;
    return <p data-clauseo="markdown-fallback">{this.props.source.trimEnd()}</p>;
  }
}
