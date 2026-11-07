import { File, Folder, Image, Plus } from "lucide-react";
import { type CSSProperties, type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { t } from "../catalog";
import { Icon, IconButton } from "./Icon";
import "./Chip.css";
import "./AttachMenu.css";

/** What the user adds through the system's dialog (FLOW-SEND-MESSAGE-R07). */
export type AttachWhat = "files" | "images" | "folder";

const CHOICES = [
  { what: "files", icon: File, label: "composer.attach.filesButton" },
  { what: "images", icon: Image, label: "composer.attach.imagesButton" },
  { what: "folder", icon: Folder, label: "composer.attach.folderButton" },
] as const;

interface AttachMenuProps {
  /** Opens the dialog for the choice; absent while nothing can be attached, and attach is unavailable. */
  onChoose?: (what: AttachWhat) => void;
  disabled?: boolean;
  /** For the gallery: the menu shown open, and one choice shown under the pointer or with keyboard focus. */
  initialOpen?: boolean;
  forceChoice?: { what: AttachWhat; state: "hover" | "pressed" | "focus" };
}

/**
 * Attach and its menu (docs/design/COMPOSER.md, The attach menu): a chip's list that opens upward from attach, offering
 * files, images and a folder, with a note that attachments can also be pasted or dropped. The arrows move among the
 * choices, Enter or Space chooses one, and choosing or Escape closes the menu with focus back on attach.
 */
export function AttachMenu({ onChoose, disabled, initialOpen = false, forceChoice }: AttachMenuProps) {
  const [open, setOpen] = useState(initialOpen);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();
  const anchor = `--attach-${id.replace(/[^a-zA-Z0-9]/g, "")}`;
  const focusOnOpen = useRef(false);
  const off = disabled || onChoose === undefined;

  useEffect(() => {
    if (!open || !focusOnOpen.current) return;
    focusOnOpen.current = false;
    items(menu.current)[0]?.focus({ preventScroll: true });
  }, [open]);

  // A press anywhere outside closes the menu, leaving focus where the press put it.
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);

  useEffect(() => {
    if (off) setOpen(false);
  }, [off]);

  const show = () => {
    focusOnOpen.current = true;
    setOpen(true);
  };
  const close = () => {
    setOpen(false);
    button.current?.focus({ preventScroll: true });
  };
  const choose = (what: AttachWhat) => {
    close();
    onChoose?.(what);
  };

  const onMenuKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const all = items(menu.current);
    const at = all.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => {
      event.preventDefault();
      all[(to + all.length) % all.length]?.focus({ preventScroll: true });
    };
    switch (event.key) {
      case "ArrowDown":
        return move(at + 1);
      case "ArrowUp":
        return move(at - 1);
      case "Home":
        return move(0);
      case "End":
        return move(all.length - 1);
      case "Escape":
        event.preventDefault();
        // The menu is attach's; Escape here is not also a request to leave the screen.
        event.stopPropagation();
        return close();
      case "Tab":
        setOpen(false);
        return;
      case "Enter":
      case " ": {
        event.preventDefault();
        const what = all[at]?.dataset.what as AttachWhat | undefined;
        if (what) choose(what);
      }
    }
  };

  return (
    <div className="attach-root" ref={root}>
      <IconButton
        ref={button}
        icon={Plus}
        label={t("composer.attachButton")}
        className="composer-attach"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        style={{ anchorName: anchor } as CSSProperties}
        disabled={off}
        onClick={() => (open ? setOpen(false) : show())}
        onKeyDown={(event) => {
          if (event.key === "ArrowUp" || event.key === "ArrowDown") {
            event.preventDefault();
            if (open) items(menu.current)[0]?.focus({ preventScroll: true });
            else show();
          } else if (event.key === "Escape" && open) {
            event.preventDefault();
            event.stopPropagation();
            setOpen(false);
          }
        }}
      />
      {open && (
        <div ref={menu} id={id} className="chip-list attach-menu" role="menu" aria-label={t("composer.attach.menuLabel")} style={{ positionAnchor: anchor } as CSSProperties} onKeyDown={onMenuKey}>
          {CHOICES.map((choice) => (
            <div
              key={choice.what}
              role="menuitem"
              tabIndex={-1}
              className="chip-option"
              data-what={choice.what}
              data-force={forceChoice?.what === choice.what ? forceChoice.state : undefined}
              onClick={() => choose(choice.what)}
            >
              <span className="chip-action-icon">
                <Icon icon={choice.icon} size="icon-sm" />
              </span>
              <span className="chip-option-label">{t(choice.label)}</span>
            </div>
          ))}
          <p className="attach-menu-note">{t("composer.attach.pasteNote")}</p>
        </div>
      )}
    </div>
  );
}

function items(menu: HTMLElement | null): HTMLElement[] {
  return menu ? Array.from(menu.querySelectorAll<HTMLElement>("[role=menuitem]")) : [];
}
