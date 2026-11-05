// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Holds a view that loads on demand (M8, ADR-0031): the libraries behind Charts, Map, Graph and
 * Canvas stay out of the first bundle, and one that cannot load or start leaves the rest of the app
 * working and says so. The shell keys it by view, so choosing another view starts afresh.
 */
import { StatusMessage } from "@databaseme/ui";
import { Component, Suspense, type ErrorInfo, type ReactNode } from "react";
import { reportError } from "../actions";

class Boundary extends Component<{ quiet: boolean; children: ReactNode }, { failed: boolean }> {
  override state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  override componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error(error, info.componentStack);
    if (!this.props.quiet) reportError(error);
  }

  override render() {
    if (this.state.failed) {
      return <StatusMessage tone="error">This view could not start.</StatusMessage>;
    }
    return this.props.children;
  }
}

export function ViewBoundary({
  quiet = false,
  children,
}: {
  /** Says so here and nowhere else: a dashboard's tile, where the rest of the page is still fine. */
  quiet?: boolean;
  children: ReactNode;
}) {
  return (
    <Boundary quiet={quiet}>
      <Suspense fallback={<StatusMessage>Loading…</StatusMessage>}>{children}</Suspense>
    </Boundary>
  );
}
