// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The files folder (ADR-0021, M5): turning it on and off, where it is, how it stands, the first
 * write's progress, and the Conflicts inbox. One dialog, reached from the header's Files button.
 */
import {
  commands,
  unwrap,
  useConflicts,
  useMirrorStatus,
  type Conflict_Serialize,
  type HoldChoice,
  type Id,
} from "@databaseme/client";
import { Button, Dialog, StatusMessage } from "@databaseme/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { attempt } from "../actions";
import { detailLine, KIND_HELP, KIND_LABELS } from "./conflicts";
import { statusLine } from "./status";

export interface FilesDialogProps {
  open: boolean;
  onClose: () => void;
  /** Opens a page: an entry's object, or the page that keeps a file's version. */
  onOpen: (id: Id) => void;
}

export function FilesDialog({ open, onClose, onOpen }: FilesDialogProps) {
  const client = useQueryClient();
  const status = useMirrorStatus();
  const conflicts = useConflicts(true);
  const [folder, setFolder] = useState("");
  const [busy, setBusy] = useState(false);
  const folderId = useId();
  const refresh = () => client.invalidateQueries({ queryKey: ["vault"] });

  const run = (change: () => Promise<unknown>) => {
    setBusy(true);
    void attempt(change).finally(() => {
      setBusy(false);
      void refresh();
    });
  };

  const data = status.data;
  const writing = data?.enabled === true && (data.dirty > 0 || data.full_rewrite_needed);

  return (
    <Dialog open={open} onClose={onClose} title="Files folder" className="dbme-files">
      <p className="dbme-files__intro">
        Keeps every page as a Markdown file you can open in any editor. Edits made to the files come
        back into the app.
      </p>
      {data === undefined ? (
        <StatusMessage>Reading the folder's status…</StatusMessage>
      ) : (
        <>
          <StatusMessage>{statusLine(data)}</StatusMessage>
          {data.enabled && data.folder !== null ? (
            <p className="dbme-files__path">
              <span className="dbme-muted">Folder: </span>
              <code>{data.folder}</code>
            </p>
          ) : null}
          {writing ? (
            <p className="dbme-files__progress" role="status">
              <progress aria-label="Writing files" />
              {data.full_rewrite_needed
                ? "Writing every page to its file…"
                : `Writing ${String(data.dirty)} ${data.dirty === 1 ? "page" : "pages"}…`}
            </p>
          ) : null}
          <div className="dbme-files__controls">
            {data.enabled ? (
              <>
                <Button
                  variant="primary"
                  disabled={busy}
                  onClick={() => {
                    run(() => unwrap(commands.mirrorSyncNow()));
                  }}
                >
                  Sync now
                </Button>
                <Button
                  disabled={busy}
                  onClick={() => {
                    run(() => unwrap(commands.mirrorDisable()));
                  }}
                >
                  Turn off
                </Button>
              </>
            ) : (
              <>
                <label className="dbme-files__folder" htmlFor={folderId}>
                  <span>Folder (leave empty for the vault's own files folder)</span>
                  <input
                    id={folderId}
                    className="dbme-input"
                    value={folder}
                    placeholder="files"
                    autoComplete="off"
                    onChange={(event) => {
                      setFolder(event.target.value);
                    }}
                  />
                </label>
                <Button
                  variant="primary"
                  disabled={busy}
                  onClick={() => {
                    const chosen = folder.trim();
                    run(() => unwrap(commands.mirrorEnable(chosen === "" ? null : chosen)));
                  }}
                >
                  Turn on
                </Button>
              </>
            )}
          </div>
        </>
      )}
      <section className="dbme-files__inbox" aria-labelledby={`${folderId}-inbox`}>
        <h3 id={`${folderId}-inbox`}>Conflicts</h3>
        {conflicts.data === undefined ? (
          <StatusMessage>Reading the inbox…</StatusMessage>
        ) : conflicts.data.length === 0 ? (
          <p className="dbme-muted">Nothing to resolve.</p>
        ) : (
          <ul className="dbme-conflicts" aria-label="Conflicts">
            {conflicts.data.map((conflict) => (
              <ConflictItem
                key={conflict.id}
                conflict={conflict}
                busy={busy}
                onOpen={(id) => {
                  onClose();
                  onOpen(id);
                }}
                onDecide={(choice) => {
                  run(() => unwrap(commands.mirrorHoldDecide(conflict.id, choice)));
                }}
                onResolve={() => {
                  run(() => unwrap(commands.conflictResolve(conflict.id, "dismissed")));
                }}
              />
            ))}
          </ul>
        )}
      </section>
      <div className="dbme-files__controls dbme-files__footer">
        <Button onClick={onClose}>Close</Button>
      </div>
    </Dialog>
  );
}

function ConflictItem({
  conflict,
  busy,
  onOpen,
  onDecide,
  onResolve,
}: {
  conflict: Conflict_Serialize;
  busy: boolean;
  onOpen: (id: Id) => void;
  onDecide: (choice: HoldChoice) => void;
  onResolve: () => void;
}) {
  const kind = conflict.kind;
  const detail = detailLine(conflict);
  const hold = kind === "bulk_change_hold";
  return (
    <li className="dbme-conflict">
      <div className="dbme-conflict__head">
        <strong>{KIND_LABELS[kind]}</strong>
        {conflict.rel_path !== null ? <code>{conflict.rel_path}</code> : null}
      </div>
      <p className="dbme-conflict__help">{KIND_HELP[kind]}</p>
      {detail !== null ? <p className="dbme-conflict__detail">{detail}</p> : null}
      <div className="dbme-conflict__actions">
        {hold ? (
          <>
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => {
                onDecide("proceed");
              }}
            >
              Apply the changes
            </Button>
            <Button
              disabled={busy}
              onClick={() => {
                onDecide("restore");
              }}
            >
              Restore the folder
            </Button>
          </>
        ) : (
          <>
            {conflict.object_id !== null ? (
              <Button
                variant="ghost"
                onClick={() => {
                  onOpen(conflict.object_id ?? "");
                }}
              >
                Open the page
              </Button>
            ) : null}
            {conflict.conflict_copy_id !== null ? (
              <Button
                variant="ghost"
                onClick={() => {
                  onOpen(conflict.conflict_copy_id ?? "");
                }}
              >
                Open the file's version
              </Button>
            ) : null}
            <Button disabled={busy} onClick={onResolve}>
              Done
            </Button>
          </>
        )}
      </div>
    </li>
  );
}
