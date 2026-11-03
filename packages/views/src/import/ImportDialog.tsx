// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The Import dialog (ADR-0026, M6): the paths to import (dropped on the window, or typed), where
 * they land, the plan as a dry run that writes nothing, Apply, and the receipt with Undo. The
 * plan is the one the apply takes: changing the paths or the parent drops it.
 */
import {
  commands,
  unwrap,
  useObject,
  type Id,
  type ImportPlan_Serialize,
  type ImportRun_Serialize,
} from "@databaseme/client";
import { Button, Dialog, StatusMessage } from "@databaseme/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { actions } from "../actions";
import { importerLabel, sourceName } from "./origin";
import { becomes, countOf, pathsOf } from "./plan";

export interface ImportDialogProps {
  open: boolean;
  /** The paths to start from: what was dropped on the window, or none. */
  paths: readonly string[];
  /** The open page, offered as the place to land under. */
  parent: Id | null;
  onClose: () => void;
  /** Opens a page: the receipt, or an imported object. */
  onOpen: (id: Id) => void;
}

export function ImportDialog({ open, paths, parent, onClose, onOpen }: ImportDialogProps) {
  return (
    <Dialog open={open} onClose={onClose} title="Import" className="dbme-import">
      {/* The body mounts with each opening, so every opening starts from the paths it was given. */}
      {open ? <Body paths={paths} parent={parent} onClose={onClose} onOpen={onOpen} /> : null}
    </Dialog>
  );
}

function Body({ paths, parent, onClose, onOpen }: Omit<ImportDialogProps, "open">) {
  const client = useQueryClient();
  const [text, setText] = useState(() => paths.join("\n"));
  const [under, setUnder] = useState(false);
  const [plan, setPlan] = useState<ImportPlan_Serialize | null>(null);
  const [run, setRun] = useState<ImportRun_Serialize | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const page = useObject(parent);
  const baseId = useId();

  const sources = pathsOf(text);
  const options = { parent: under ? parent : null };

  const attempt = async (step: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await step();
    } catch (failure: unknown) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  };

  const preview = () =>
    attempt(async () => {
      setRun(null);
      setPlan(await unwrap(commands.importPlan(sources, options)));
    });

  const apply = () =>
    attempt(async () => {
      if (plan === null) return;
      const done = await unwrap(commands.importApply(plan));
      setRun(done);
      setPlan(null);
      await client.invalidateQueries({ queryKey: ["vault"] });
    });

  const undo = () =>
    attempt(async () => {
      if (run === null) return;
      // A run is one undo step per transaction it took.
      for (let step = 0; step < run.transactions; step++) await actions.undo();
      setRun(null);
    });

  const invalidate = () => {
    setPlan(null);
    setRun(null);
  };

  const pageTitle =
    page.data === undefined ? null : page.data.title === "" ? "Untitled" : page.data.title;

  return (
    <>
      <p className="dbme-import__intro">
        Brings files into the vault as pages: Markdown and Obsidian folders, CSV and Excel tables,
        PDFs, pictures, and any other file as an attachment. Preview first; nothing is written
        until you import.
      </p>
      <label className="dbme-import__paths" htmlFor={`${baseId}-paths`}>
        <span>Files or folders to import, one per line</span>
        <textarea
          id={`${baseId}-paths`}
          className="dbme-input dbme-import__textarea"
          rows={4}
          value={text}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => {
            setText(event.target.value);
            invalidate();
          }}
        />
      </label>
      {parent !== null && pageTitle !== null ? (
        <label className="dbme-import__under">
          <input
            type="checkbox"
            checked={under}
            onChange={(event) => {
              setUnder(event.target.checked);
              invalidate();
            }}
          />
          <span>
            Put them under <strong>{pageTitle}</strong> (otherwise under a new "Import" page)
          </span>
        </label>
      ) : null}
      {error !== null ? <StatusMessage tone="error">{error}</StatusMessage> : null}
      {plan !== null ? <Plan plan={plan} baseId={baseId} /> : null}
      {run !== null ? (
        <section className="dbme-import__receipt" aria-labelledby={`${baseId}-receipt`}>
          <h3 id={`${baseId}-receipt`}>Done</h3>
          <StatusMessage>
            Imported {countOf(run.objects, "object")}
            {run.attached.length > 0 ? `, ${countOf(run.attached.length, "duplicate")} linked` : ""}
            {run.transactions > 1 ? `, in ${countOf(run.transactions, "undo step")}` : ""}.
          </StatusMessage>
          {run.warnings.length > 0 ? <Warnings warnings={run.warnings} /> : null}
          <div className="dbme-import__controls">
            <Button
              variant="primary"
              onClick={() => {
                onClose();
                onOpen(run.receipt ?? run.parent);
              }}
            >
              {run.receipt !== null ? "Open the receipt" : "Open the page"}
            </Button>
            <Button disabled={busy} onClick={() => void undo()}>
              Undo the import
            </Button>
          </div>
        </section>
      ) : null}
      <div className="dbme-import__controls dbme-import__footer">
        {plan === null ? (
          <Button
            variant="primary"
            disabled={busy || sources.length === 0}
            onClick={() => void preview()}
          >
            Preview
          </Button>
        ) : (
          <Button
            variant="primary"
            disabled={busy || plan.entries.length === 0}
            onClick={() => void apply()}
          >
            Import {countOf(plan.objects, "object")}
          </Button>
        )}
        <Button onClick={onClose}>Close</Button>
      </div>
    </>
  );
}

/** The plan: one row per source, then what was left out and what to know. */
function Plan({ plan, baseId }: { plan: ImportPlan_Serialize; baseId: string }) {
  return (
    <section className="dbme-import__plan" aria-labelledby={`${baseId}-plan`}>
      <h3 id={`${baseId}-plan`}>Preview</h3>
      <p className="dbme-import__totals" role="status">
        {plan.entries.length === 0
          ? "Nothing to import."
          : `${countOf(plan.objects, "object")} from ${countOf(plan.entries.length, "source")}${
              plan.transactions > 1 ? `, in ${countOf(plan.transactions, "undo step")}` : ""
            }.`}
      </p>
      {plan.entries.length > 0 ? (
        <ul className="dbme-import__sources" aria-label="Sources">
          {plan.entries.map((entry) => (
            <li key={entry.source.path} className="dbme-import__source">
              <strong>{sourceName(entry.source.path)}</strong>
              <span className="dbme-muted">
                {" "}
                as {importerLabel(entry.importer)}: {becomes(entry)}
              </span>
              {entry.extract.types.length > 0 ? (
                <span className="dbme-import__types">
                  {" "}
                  Type {entry.extract.types.map((type) => type.name).join(", ")}
                </span>
              ) : null}
              {entry.extract.warnings.length > 0 ? (
                <Warnings warnings={entry.extract.warnings} />
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {plan.duplicates.length > 0 ? (
        <ul className="dbme-import__left-out" aria-label="Already in the vault">
          {plan.duplicates.map((duplicate) => (
            <li key={duplicate.path}>
              <strong>{sourceName(duplicate.path)}</strong>
              <span className="dbme-muted"> is already here as {duplicate.of}; left out.</span>
            </li>
          ))}
        </ul>
      ) : null}
      {plan.refused.length > 0 ? (
        <ul className="dbme-import__left-out" aria-label="Could not be read">
          {plan.refused.map((refused) => (
            <li key={refused.path}>
              <strong>{sourceName(refused.path)}</strong>
              <span className="dbme-muted"> could not be read: {refused.reason}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

function Warnings({ warnings }: { warnings: readonly string[] }) {
  return (
    <ul className="dbme-import__warnings" aria-label="Warnings">
      {warnings.map((warning, index) => (
        <li key={`${String(index)}:${warning}`}>{warning}</li>
      ))}
    </ul>
  );
}
