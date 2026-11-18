import type { LucideIcon } from "lucide-react";
import { type CSSProperties, type Ref, useCallback, useEffect, useId, useImperativeHandle, useRef, useState } from "react";
import { IconButton } from "./Icon";
import { Menu, type MenuClose, type MenuEntry } from "./Menu";

/**
 * An icon button and the menu it opens below it (docs/design/RAIL.md, Menus), as the arrange button and a heading's
 * More are: it reads pressed while its menu is open; opened from the keyboard, focus goes to the first item, from the
 * pointer, to the menu; and closing the menu returns focus to the button, unless a press elsewhere put it there.
 *
 * Unavailable, as when Ersa loses Core, the button keeps keyboard focus and opens nothing, so a keyboard user keeps
 * their place; a menu open at that moment closes, and focus that was in it comes back to the button.
 */
export function MenuButton({
  ref,
  icon,
  label,
  entries,
  className,
  unavailable = false,
  force,
  initialOpen,
  initialSubmenu,
}: {
  /** The button, for an owner that gives focus back to it after something its menu opened, such as a question. */
  ref?: Ref<HTMLButtonElement | null>;
  icon: LucideIcon;
  /** The button's and its menu's name, for assistive technology and as its tooltip. */
  label: string;
  entries: MenuEntry[];
  className?: string;
  unavailable?: boolean;
  /** For the gallery: a pointer or keyboard state without one. */
  force?: "hover" | "focus" | "pressed";
  /** For the gallery: the menu open, taking no focus, and one of its submenus open by index. */
  initialOpen?: boolean;
  initialSubmenu?: number;
}) {
  // The menu, while open: where focus went as it opened. The gallery's open menu takes none.
  const [menu, setMenu] = useState<"item" | "menu" | "none" | undefined>(initialOpen ? "none" : undefined);
  const button = useRef<HTMLButtonElement>(null);
  // Set once the button is drawn, which is before an owner can use it.
  useImperativeHandle(ref, () => button.current as HTMLButtonElement, []);
  // The menu floats above the rail in the top layer, anchored to the button by this name, which CSS needs bare.
  const anchor = `--menu-button-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const close = useCallback((how: MenuClose) => {
    setMenu(undefined);
    if (how !== "away") button.current?.focus({ preventScroll: true });
  }, []);
  const inside = useCallback(() => [button.current], []);
  // Becoming unavailable while open closes the menu for good rather than leave it to act on a Core that is gone.
  // Focus in the menu would be lost as it goes, so it comes back to the button, which still takes it.
  const open = menu !== undefined && !(unavailable && !initialOpen);
  useEffect(() => {
    if (!unavailable || menu === undefined || initialOpen) return;
    setMenu(undefined);
    const focus = document.activeElement;
    if (focus === null || focus === document.body || focus.closest("[role=menu]")) button.current?.focus({ preventScroll: true });
  }, [unavailable, menu, initialOpen]);
  return (
    <>
      <IconButton
        ref={button}
        className={className}
        icon={icon}
        size="icon-sm"
        label={label}
        force={force}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-disabled={unavailable || undefined}
        style={{ anchorName: anchor } as CSSProperties}
        // A click the keyboard made (Enter or Space) has no pointer presses to count.
        onClick={(event) => {
          if (unavailable) return;
          setMenu(menu ? undefined : event.detail === 0 ? "item" : "menu");
        }}
      />
      <Menu label={label} entries={entries} open={open} place={{ anchor }} focus={menu} inside={inside} onClose={close} initialSubmenu={initialSubmenu} />
    </>
  );
}
