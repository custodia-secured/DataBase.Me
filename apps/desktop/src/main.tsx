// SPDX-License-Identifier: AGPL-3.0-or-later
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "@databaseme/ui/tokens.css";
import "@databaseme/ui/ui.css";
import "@databaseme/views/views.css";
import "./styles.css";

// The e2e build (ADR-0018) talks to the test server instead of the Rust shell. In any other build
// this branch is dead code and the bundler drops it with the transport.
if (import.meta.env.MODE === "e2e") {
  const { installE2eTransport } = await import("@databaseme/client/e2e");
  installE2eTransport();
  // Tauri freezes Object.prototype in the app (tauri.conf.json, freezePrototype); so do the tests.
  Object.freeze(Object.prototype);
}

const container = document.getElementById("root");
if (!container) {
  throw new Error("index.html is missing the #root element");
}

const client = new QueryClient({
  defaultOptions: {
    queries: {
      // Reads are local and every change announces itself, so data is never stale on a timer.
      staleTime: Infinity,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});

createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
);
