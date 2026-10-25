// SPDX-License-Identifier: AGPL-3.0-or-later
import { useEffect } from "react";
import { notifyReady } from "./ipc";

/** The M0 shell: proves the window, the web UI and the IPC bridge work. Real views arrive in M2. */
export function App() {
  useEffect(() => {
    notifyReady().catch((error: unknown) => {
      console.error("Could not tell the shell the UI is ready", error);
    });
  }, []);

  return (
    <main className="shell">
      <h1>DataBase.Me</h1>
      <p>
        Your lifetime information repository. There is nothing to use yet: this is the M0 shell.
      </p>
      <p className="shell-meta">Version {__APP_VERSION__}</p>
    </main>
  );
}
