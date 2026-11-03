// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Onboarding (M7, ADR-0028; brief §2.2, §2.9 and §8): six short steps on first launch, or again
 * from Settings. Where the vault goes, whether and where the files folder is, the mode, the
 * theme, and whether update checks are wanted (off, and 0.1 has no updater either way). Nothing
 * here touches the network. The AI question is M11's (brief §2.5).
 */
import {
  commands,
  setSettings,
  unwrap,
  type Mode,
  type Theme,
  type VaultInfo,
} from "@databaseme/client";
import { applyTheme, Button, StatusMessage } from "@databaseme/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { vaultIn } from "./paths";

export interface OnboardingProps {
  /** The vault already open, when onboarding runs again; `null` on first launch. */
  vaultPath: string | null;
  /** Where the default vault would go, for the first choice. */
  defaultVault: string | null;
  /** Opens the OS's folder picker and resolves to the folder, or `null` when cancelled; absent
   *  where there is none (the e2e build), and the folder is typed. */
  pickFolder?: (() => Promise<string | null>) | undefined;
  /** Called once everything is saved and the vault is open. */
  onDone: () => void;
}

type VaultChoice = "default" | "custom";
type FilesChoice = "inside" | "folder" | "off";

const STEPS = ["Welcome", "Your vault", "Files folder", "Mode", "Theme", "Updates"] as const;

export function Onboarding({ vaultPath, defaultVault, pickFolder, onDone }: OnboardingProps) {
  const client = useQueryClient();
  const [step, setStep] = useState(0);
  const [vaultChoice, setVaultChoice] = useState<VaultChoice>("default");
  const [vaultFolder, setVaultFolder] = useState("");
  const [files, setFiles] = useState<FilesChoice>("inside");
  const [filesFolder, setFilesFolder] = useState("");
  const [mode, setMode] = useState<Mode>("simple");
  const [theme, setTheme] = useState<Theme>("system");
  const [updateChecks, setUpdateChecks] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const base = useId();

  const last = STEPS.length - 1;
  const vaultOk = vaultPath !== null || vaultChoice === "default" || vaultFolder.trim() !== "";
  const filesOk = files !== "folder" || filesFolder.trim() !== "";
  const canGoOn = step === 1 ? vaultOk : step === 2 ? filesOk : true;

  const finish = async () => {
    setBusy(true);
    setError(null);
    try {
      let vault: VaultInfo;
      if (vaultPath !== null) {
        vault = (await unwrap(commands.vaultInfo())) ?? (await unwrap(commands.vaultOpenDefault()));
      } else if (vaultChoice === "default") {
        vault = await unwrap(commands.vaultOpenDefault());
      } else {
        const path = vaultIn(vaultFolder.trim(), defaultVault);
        try {
          vault = await unwrap(commands.vaultCreate(path));
        } catch (failure: unknown) {
          // A vault already there is the one to open.
          if ((failure as { kind?: string }).kind !== "bad_path") throw failure;
          vault = await unwrap(commands.vaultOpen(path));
        }
      }
      if (files === "inside") await unwrap(commands.mirrorEnable(null));
      else if (files === "folder") await unwrap(commands.mirrorEnable(filesFolder.trim()));
      else if (vaultPath !== null) await unwrap(commands.mirrorDisable());
      await setSettings(client, {
        mode,
        theme,
        update_checks: updateChecks,
        onboarded: true,
        last_vault: vault.path,
      });
      applyTheme(theme);
      onDone();
    } catch (failure: unknown) {
      const message =
        failure instanceof Error
          ? failure.message
          : ((failure as { message?: string }).message ?? String(failure));
      setError(message);
      setBusy(false);
    }
  };

  const folderField = (
    id: string,
    label: string,
    value: string,
    onChange: (next: string) => void,
  ) => (
    <div className="dbme-onboarding__folder">
      <label htmlFor={id}>{label}</label>
      <div className="dbme-onboarding__folder-row">
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
      </div>
    </div>
  );

  const choice = <T extends string>(
    name: string,
    legend: string,
    value: T,
    onChange: (next: T) => void,
    options: readonly { value: T; label: string; help: string }[],
  ) => (
    <fieldset className="dbme-onboarding__choices">
      <legend>{legend}</legend>
      {options.map((option) => (
        <label key={option.value} className="dbme-onboarding__choice">
          <input
            type="radio"
            name={`${base}-${name}`}
            value={option.value}
            checked={value === option.value}
            onChange={() => {
              onChange(option.value);
            }}
          />
          <span>
            <strong>{option.label}</strong>
            <span className="dbme-muted"> {option.help}</span>
          </span>
        </label>
      ))}
    </fieldset>
  );

  return (
    <main className="dbme-onboarding" aria-labelledby={`${base}-title`}>
      <h1 id={`${base}-title`}>{step === 0 ? "Welcome to DataBase.Me" : STEPS[step]}</h1>
      <p className="dbme-muted" role="status">
        Step {step + 1} of {STEPS.length}
      </p>

      {step === 0 ? (
        <div className="dbme-onboarding__body">
          <p>
            A place for everything you know: notes, files, links and data, shown as documents,
            outlines, mind maps, tables and more. It lives on this computer, in files you own.
          </p>
          <p>
            No account, no telemetry, and no network requests unless you turn on a feature that
            needs one. The next five steps set it up; every choice can be changed later in Settings.
          </p>
        </div>
      ) : null}

      {step === 1 ? (
        <div className="dbme-onboarding__body">
          {vaultPath !== null ? (
            <p>
              Your vault is at <code>{vaultPath}</code>. Opening another one comes with Settings.
            </p>
          ) : (
            <>
              {choice<VaultChoice>("vault", "Where the vault goes", vaultChoice, setVaultChoice, [
                {
                  value: "default",
                  label: "In the app's own folder",
                  help: defaultVault ?? "",
                },
                {
                  value: "custom",
                  label: "In a folder I choose",
                  help: "a vault folder named Main.dbme is made inside it",
                },
              ])}
              {vaultChoice === "custom"
                ? folderField(`${base}-vault-folder`, "Folder", vaultFolder, setVaultFolder)
                : null}
            </>
          )}
        </div>
      ) : null}

      {step === 2 ? (
        <div className="dbme-onboarding__body">
          <p>
            The files folder keeps every page as a Markdown file you can open in any editor, and
            edits made there come back into the app.
          </p>
          {choice<FilesChoice>("files", "The files folder", files, setFiles, [
            { value: "inside", label: "Inside the vault", help: "in its files folder" },
            { value: "folder", label: "In a folder I choose", help: "an Obsidian vault, say" },
            { value: "off", label: "Off for now", help: "it can be turned on in Settings" },
          ])}
          {files === "folder"
            ? folderField(`${base}-files-folder`, "Folder", filesFolder, setFilesFolder)
            : null}
        </div>
      ) : null}

      {step === 3 ? (
        <div className="dbme-onboarding__body">
          {choice<Mode>("mode", "How much to show", mode, setMode, [
            {
              value: "simple",
              label: "Simple",
              help: "a plain notes app: pages, search, the mind map, the gallery, a daily journal",
            },
            {
              value: "organizer",
              label: "Organizer",
              help: "adds types, fields and tables",
            },
          ])}
          <p className="dbme-muted">Researcher and Data modes come with later versions.</p>
        </div>
      ) : null}

      {step === 4 ? (
        <div className="dbme-onboarding__body">
          {choice<Theme>(
            "theme",
            "Colours",
            theme,
            (next) => {
              setTheme(next);
              applyTheme(next);
            },
            [
              { value: "system", label: "Follow the system", help: "" },
              { value: "light", label: "Light", help: "" },
              { value: "dark", label: "Dark", help: "" },
            ],
          )}
        </div>
      ) : null}

      {step === 5 ? (
        <div className="dbme-onboarding__body">
          <label className="dbme-onboarding__choice">
            <input
              type="checkbox"
              checked={updateChecks}
              onChange={(event) => {
                setUpdateChecks(event.target.checked);
              }}
            />
            <span>
              <strong>Check for updates</strong>
              <span className="dbme-muted">
                {" "}
                Off, a check would send the app's version and your address to the download host.
                This version has no updater yet: your choice is kept for the first one that does.
              </span>
            </span>
          </label>
          {error !== null ? <StatusMessage tone="error">{error}</StatusMessage> : null}
        </div>
      ) : null}

      <div className="dbme-onboarding__controls">
        {step > 0 ? (
          <Button
            disabled={busy}
            onClick={() => {
              setStep(step - 1);
            }}
          >
            Back
          </Button>
        ) : null}
        {step < last ? (
          <Button
            variant="primary"
            disabled={!canGoOn}
            onClick={() => {
              setStep(step + 1);
            }}
          >
            Next
          </Button>
        ) : (
          <Button variant="primary" disabled={busy} onClick={() => void finish()}>
            Finish
          </Button>
        )}
      </div>
    </main>
  );
}
