// SPDX-License-Identifier: AGPL-3.0-or-later
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";

afterEach(cleanup);

describe("App shell", () => {
  it("renders the product name as the page heading inside the main landmark", () => {
    render(<App />);
    const main = screen.getByRole("main");
    expect(main).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("DataBase.Me");
  });

  it("shows the build version", () => {
    render(<App />);
    expect(screen.getByText(`Version ${__APP_VERSION__}`)).toBeTruthy();
  });
});
