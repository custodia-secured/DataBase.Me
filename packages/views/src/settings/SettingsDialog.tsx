// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Settings and Help (M7, ADR-0027): the vault and how to open or make another, the files folder
 * (its own dialog), the mode, the theme, update checks, "Set up again", and Help: the version,
 * where the docs and the issue tracker are as text to copy (the app opens no browser and makes
 * no request), and the diagnostic report.
 */
import {
  commands,
  setSettings,
  unwrap,
  useSettings,
  useVaultInfo,
  type Mode,
  type Theme,
} from "@databaseme/client";
import { applyTheme, Button, copyText, Dialog, StatusMessage } from "@databaseme/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { vaultIn } from "../onboarding/paths";

export interface SettingsDialogProps {
  open: boolean;
  onClose: () => void;
  /** The app's version and the format version it writes, for Help; `null` until known. */
  about: { version: string; formatVersion: number } | null;
  /** Where the docs and the issue tracker are. */
  links: { docs: string; issues: string };
  /** Opens the files folder dialog. */
  onOpenFiles: () => void;
  /** Runs onboarding again. */
  onSetupAgain: () => void;
  /** Another vault is open now: the shell forgets what it showed. */
  onVaultChanged: () => void;
  /** The OS's folder picker, where there is one. */
  pickFolder?: (() => Promise<string | null>) | undefined;
  /** Builds the diagnostic report (Task 7); absent until then. */
  report?: (() => Promise<string>) | undefined;
}

export function SettingsDialog({
  open,
  onClose,
  about,
  links,
  onOpenFiles,
  onSetupAgain,
  onVaultChanged,
  pickFolder,
  report,
}: SettingsDialogProps) {
  const client = useQueryClient();
  const settings = useSettings();
  const vault = useVaultInfo();
  const [otherVault, setOtherVault] = useState("");
  const [newFolder, setNewFolder] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [shown, setShown] = useState<string | null>(null);
  const base = useId();

  const attempt = (step: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    step()
      .catch((failure: unknown) => {
        setError(failure instanceof Error ? failure.message : String(failure));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  // The shell drops its selection and forgets what it read before the settings move on.
  const switchTo = (opened: { path: string }) => {
    onVaultChanged();
    return setSettings(client, { last_vault: opened.path });
  };

  const copy = async (what: string, text: string) => {
    const ok = await copyText(text);
    setCopied(ok ? what : null);
    setShown(ok ? null : text);
  };

  const data = settings.data;
  const defaultPath = vault.data?.path ?? null;

  const folderField = (
    id: string,
    label: string,
    value: string,
    onChange: (next: string) => void,
    action: { label: string; run: () => void },
  ) => (
    <div className="dbme-settings__field">
      <label htmlFor={id}>{label}</label>
      <div className="dbme-settings__row">
        <input
          id={id}
          className="dbme-input"
          value={value}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
        {pickFolder ? (
          <Button
            onClick={() => {
              void pickFolder().then((picked) => {
                if (picked !== null) onChange(picked);
              });
            }}
          >
            Browse…
          </Button>
        ) : null}
        <Button disabled={busy || value.trim() === ""} onClick={action.run}>
          {action.label}
        </Button>
      </div>
    </div>
  );

  const radios = <T extends string>(
    name: string,
    legend: string,
    value: T | undefined,
    options: readonly { value: T; label: string }[],
    onChange: (next: T) => void,
  ) => (
    <fieldset className="dbme-settings__choices">
      <legend>{legend}</legend>
      {options.map((option) => (
        <label key={option.value} className="dbme-settings__choice">
          <input
            type="radio"
            name={`${base}-${name}`}
            value={option.value}
            checked={value === option.value}
            disabled={value === undefined}
            onChange={() => {
              onChange(option.value);
            }}
          />
          <span>{option.label}</span>
        </label>
      ))}
    </fieldset>
  );

  return (
    <Dialog open={open} onClose={onClose} title="Settings" className="dbme-settings">
      {error !== null ? <StatusMessage tone="error">{error}</StatusMessage> : null}

      <section aria-labelledby={`${base}-vault`}>
        <h3 id={`${base}-vault`}>Vault</h3>
        <p className="dbme-settings__path">
          <span className="dbme-muted">This vault: </span>
          <code>{defaultPath ?? "none open"}</code>
        </p>
        {folderField(
          `${base}-other`,
          "Open another vault (its folder)",
          otherVault,
          setOtherVault,
          {
            label: "Open",
            run: () => {
              attempt(async () => {
                const opened = await unwrap(commands.vaultOpen(otherVault.trim()));
                await switchTo(opened);
                setOtherVault("");
              });
            },
          },
        )}
        {folderField(`${base}-new`, "Make a new vault in a folder", newFolder, setNewFolder, {
          label: "Make",
          run: () => {
            attempt(async () => {
              const opened = await unwrap(
                commands.vaultCreate(vaultIn(newFolder.trim(), defaultPath)),
              );
              await switchTo(opened);
              setNewFolder("");
            });
          },
        })}
        <div className="dbme-settings__row">
          <Button
            onClick={() => {
              onClose();
              onOpenFiles();
            }}
          >
            Files folder…
          </Button>
          <Button
            onClick={() => {
              onClose();
              onSetupAgain();
            }}
          >
            Set up again
          </Button>
        </div>
      </section>

      <section aria-labelledby={`${base}-looks`}>
        <h3 id={`${base}-looks`}>Mode and theme</h3>
        {radios<Mode>(
          "mode",
          "Mode",
          data?.mode,
          [
            { value: "simple", label: "Simple: a plain notes app" },
            { value: "organizer", label: "Organizer: types, fields and tables too" },
          ],
          (mode) => {
            attempt(async () => {
              await setSettings(client, { mode });
            });
          },
        )}
        {radios<Theme>(
          "theme",
          "Theme",
          data?.theme,
          [
            { value: "system", label: "Follow the system" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ],
          (theme) => {
            applyTheme(theme);
            attempt(async () => {
              await setSettings(client, { theme });
            });
          },
        )}
        <label className="dbme-settings__choice">
          <input
            type="checkbox"
            checked={data?.update_checks ?? false}
            disabled={data === undefined}
            onChange={(event) => {
              const update_checks = event.target.checked;
              attempt(async () => {
                await setSettings(client, { update_checks });
              });
            }}
          />
          <span>
            Check for updates
            <span className="dbme-muted">
              {" "}
              (this version has no updater; the choice is kept for the first one that does)
            </span>
          </span>
        </label>
      </section>

      <section aria-labelledby={`${base}-help`}>
        <h3 id={`${base}-help`}>Help</h3>
        <p className="dbme-settings__about">
          {about
            ? `DataBase.Me ${about.version}, vault format ${String(about.formatVersion)}.`
            : "DataBase.Me."}{" "}
          No account, no telemetry: problems go to the issue tracker, with the diagnostic report.
        </p>
        <dl className="dbme-settings__links">
          <div>
            <dt>Documentation</dt>
            <dd>
              <code>{links.docs}</code>
              <Button variant="ghost" onClick={() => void copy("docs", links.docs)}>
                {copied === "docs" ? "Copied" : "Copy"}
              </Button>
            </dd>
          </div>
          <div>
            <dt>Issues</dt>
            <dd>
              <code>{links.issues}</code>
              <Button variant="ghost" onClick={() => void copy("issues", links.issues)}>
                {copied === "issues" ? "Copied" : "Copy"}
              </Button>
            </dd>
          </div>
        </dl>
        {report ? (
          <Button
            disabled={busy}
            onClick={() => {
              attempt(async () => {
                await copy("report", await report());
              });
            }}
          >
            {copied === "report" ? "Diagnostic report copied" : "Copy diagnostic report"}
          </Button>
        ) : null}
        {shown !== null ? (
          <label className="dbme-settings__field">
            <span>The clipboard is not available here; copy this by hand.</span>
            <textarea className="dbme-input dbme-settings__shown" readOnly rows={6} value={shown} />
          </label>
        ) : null}
      </section>

      <div className="dbme-settings__row dbme-settings__footer">
        <Button onClick={onClose}>Close</Button>
      </div>
    </Dialog>
  );
}
