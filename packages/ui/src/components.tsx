// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Shared components. They style themselves with the classes in `ui.css` and never inject a
 * `<style>` element: the app's Content-Security-Policy refuses inline styles (ADR-0017).
 */
import { DropdownMenu } from "radix-ui";
import {
  createElement,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type ReactNode,
  type Ref,
} from "react";
import { typeIcon, type LucideIcon } from "./icons";

function classes(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(" ");
}

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "primary" | "ghost" | "danger";
  ref?: Ref<HTMLButtonElement>;
}

/** A button. It is `type="button"` unless told otherwise, so it never submits a form by accident. */
export function Button({ variant = "default", className, type = "button", ...props }: ButtonProps) {
  return (
    <button
      type={type}
      className={classes("dbme-button", `dbme-button--${variant}`, className)}
      {...props}
    />
  );
}

export interface IconButtonProps extends Omit<ButtonProps, "children"> {
  /** What the button does, read by screen readers and shown as a tooltip. */
  label: string;
  icon: LucideIcon;
}

/** A button showing only an icon. Its label is its accessible name and its tooltip. */
export function IconButton({
  label,
  icon: Icon,
  variant = "ghost",
  className,
  ...props
}: IconButtonProps) {
  return (
    <Button
      variant={variant}
      aria-label={label}
      title={label}
      className={classes("dbme-icon-button", className)}
      {...props}
    >
      <Icon aria-hidden="true" focusable="false" size={18} />
    </Button>
  );
}

/** Text only screen readers see. */
export function VisuallyHidden({ children }: { children: ReactNode }) {
  return <span className="dbme-visually-hidden">{children}</span>;
}

export interface DialogProps {
  open: boolean;
  /** Called when the dialog asks to close: Escape, the close button or the backdrop. */
  onClose: () => void;
  title: string;
  children: ReactNode;
  className?: string;
}

/**
 * A modal dialog on the native `<dialog>` element. The browser traps focus, makes the rest of the
 * page inert and closes on Escape, with no scroll-lock styles injected (ADR-0017).
 */
export function Dialog({ open, onClose, title, children, className }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      className={classes("dbme-dialog", className)}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        // A click on the dialog element itself, not its content, is a click on the backdrop.
        if (event.target === ref.current) onClose();
      }}
    >
      <div className="dbme-dialog__content">
        <header className="dbme-dialog__header">
          <h2 id={titleId} className="dbme-dialog__title">
            {title}
          </h2>
        </header>
        {children}
      </div>
    </dialog>
  );
}

export interface MenuItem {
  label: string;
  icon?: LucideIcon;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
}

export interface MenuProps {
  /** The trigger's accessible name. */
  label: string;
  icon: LucideIcon;
  items: readonly MenuItem[];
  className?: string;
}

/**
 * A menu of actions behind an icon button. It is not modal: Radix's modal menus lock scrolling by
 * injecting styles, which the CSP refuses (ADR-0017). Inside a modal `<dialog>` (the phone's
 * sidebar drawer) it opens within the dialog, since the rest of the page is inert there.
 */
export function Menu({ label, icon, items, className }: MenuProps) {
  const [container, setContainer] = useState<HTMLElement | null>(null);
  const findContainer = useCallback((trigger: HTMLButtonElement | null) => {
    setContainer(trigger?.closest("dialog") ?? null);
  }, []);
  return (
    <DropdownMenu.Root modal={false}>
      <DropdownMenu.Trigger asChild>
        <IconButton ref={findContainer} label={label} icon={icon} className={className} />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal container={container}>
        <DropdownMenu.Content className="dbme-menu" align="end" sideOffset={4}>
          {items.map((item) => {
            const ItemIcon = item.icon;
            return (
              <DropdownMenu.Item
                key={item.label}
                className={classes("dbme-menu__item", item.danger && "dbme-menu__item--danger")}
                disabled={item.disabled ?? false}
                onSelect={item.onSelect}
              >
                {ItemIcon ? <ItemIcon aria-hidden="true" size={16} /> : null}
                <span>{item.label}</span>
              </DropdownMenu.Item>
            );
          })}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** A message for screen readers and sighted users alike, announced when it changes. */
export function StatusMessage({
  children,
  tone = "info",
}: {
  children: ReactNode;
  tone?: "info" | "error";
}) {
  return (
    <p
      className={classes("dbme-status", tone === "error" && "dbme-status--error")}
      role={tone === "error" ? "alert" : "status"}
    >
      {children}
    </p>
  );
}

/** The icon of a type, by its Lucide name. Decorative: the type's name says what it is. */
export function TypeIcon({
  name,
  size = 16,
  className,
}: {
  name: string | null | undefined;
  size?: number;
  className?: string;
}) {
  return createElement(typeIcon(name), {
    "aria-hidden": true,
    focusable: "false",
    size,
    ...(className === undefined ? {} : { className }),
  });
}
