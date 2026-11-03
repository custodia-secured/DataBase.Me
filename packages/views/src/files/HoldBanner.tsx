// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The bulk-change hold (ADR-0021, Decision 3): many files were removed or rewritten at once, so
 * nothing was applied until the person says. Shown under the header while a hold is open.
 */
import { commands, unwrap, useMirrorStatus, type HoldChoice } from "@databaseme/client";
import { Button } from "@databaseme/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { attempt } from "../actions";

export interface HoldBannerProps {
  /** Opens the files folder dialog, where the hold's details are. */
  onDetails: () => void;
}

export function HoldBanner({ onDetails }: HoldBannerProps) {
  const client = useQueryClient();
  const status = useMirrorStatus();
  const [busy, setBusy] = useState(false);
  const hold = status.data?.hold ?? null;
  if (hold === null) return null;

  const decide = (choice: HoldChoice) => {
    setBusy(true);
    void attempt(() => unwrap(commands.mirrorHoldDecide(hold, choice))).finally(() => {
      setBusy(false);
      void client.invalidateQueries({ queryKey: ["vault"] });
    });
  };

  return (
    <div className="dbme-hold" role="region" aria-label="Files folder hold">
      <p className="dbme-hold__text" role="status">
        Many files in the files folder changed at once. Nothing was applied yet.
      </p>
      <div className="dbme-hold__actions">
        <Button
          variant="primary"
          disabled={busy}
          onClick={() => {
            decide("proceed");
          }}
        >
          Apply the changes
        </Button>
        <Button
          disabled={busy}
          onClick={() => {
            decide("restore");
          }}
        >
          Restore the folder
        </Button>
        <Button variant="ghost" onClick={onDetails}>
          Details
        </Button>
      </div>
    </div>
  );
}
