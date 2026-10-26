// SPDX-License-Identifier: AGPL-3.0-or-later
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";

afterEach(() => {
  cleanup();
  clearMocks();
});

function renderApp() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>,
  );
}

describe("App shell", () => {
  it("names the product and shows the build version while the vault opens", () => {
    mockIPC(() => new Promise(() => undefined), { shouldMockEvents: true });
    renderApp();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("DataBase.Me");
    expect(screen.getByText(`Version ${__APP_VERSION__}`)).toBeTruthy();
  });

  it("opens the default vault when none is open, then offers to make the first page", async () => {
    const calls: string[] = [];
    mockIPC(
      (command) => {
        calls.push(command);
        switch (command) {
          case "vault_info":
            return null;
          case "vault_open_default":
            return { id: "VAULT", path: "Main.dbme", can_undo: false, can_redo: false };
          case "objects_page":
            return [];
          case "objects_count":
            return 0;
          case "types_list":
            return [];
          default:
            return null;
        }
      },
      { shouldMockEvents: true },
    );
    renderApp();
    const buttons = await screen.findAllByRole("button", { name: "New page" }, { timeout: 3000 });
    expect(buttons.length).toBeGreaterThan(0);
    expect(calls).toContain("vault_open_default");
  });
});
