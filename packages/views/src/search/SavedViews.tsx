// SPDX-License-Identifier: AGPL-3.0-or-later
/** The saved views, under the Pages tree in the sidebar (ADR-0020). Nothing shows until one exists. */
import { useViews, type Id } from "@databaseme/client";
import { Search } from "@databaseme/ui";

export function SavedViews({
  selected,
  onOpen,
}: {
  selected: Id | null;
  onOpen: (id: Id) => void;
}) {
  const views = useViews();
  if (!views.data || views.data.length === 0) return null;
  return (
    <section className="dbme-views" aria-label="Saved views">
      <h2 className="dbme-sidebar__heading">Saved views</h2>
      <ul className="dbme-views__list">
        {views.data.map((view) => (
          <li key={view.id}>
            <button
              type="button"
              className={`dbme-views__item${view.id === selected ? " is-selected" : ""}`}
              aria-current={view.id === selected ? "page" : undefined}
              onClick={() => {
                onOpen(view.id);
              }}
            >
              <Search aria-hidden="true" size={16} className="dbme-tree__icon" />
              <span className="dbme-tree__title">{view.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
