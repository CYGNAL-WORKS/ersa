import { SlidersHorizontal } from "lucide-react";
import { type CSSProperties, useCallback, useId, useRef, useState } from "react";
import { t } from "../catalog";
import { type ArrangeChoices, GROUP_BYS, type GroupBy, SHOWS, SORT_BYS, type Show, type SortBy } from "../session/arrangement";
import { IconButton } from "./Icon";
import { Menu, type MenuClose, type MenuEntry } from "./Menu";

const GROUP_BY_NAMES = {
  project: "rail.arrange.groupByProjectButton",
  date: "rail.arrange.groupByDateButton",
  none: "rail.arrange.groupByNoneButton",
} as const satisfies Record<GroupBy, string>;
const SORT_BY_NAMES = {
  created: "rail.arrange.sortByCreatedButton",
  last_activity: "rail.arrange.sortByLastActivityButton",
  name: "rail.arrange.sortByNameButton",
} as const satisfies Record<SortBy, string>;
const SHOW_NAMES = {
  active: "rail.arrange.showActiveButton",
  archived: "rail.arrange.showArchivedButton",
  all: "rail.arrange.showAllButton",
} as const satisfies Record<Show, string>;

/** What the arrange menu's items do: each sends its change to Core (FLOW-ORGANIZE-SESSIONS, steps 4 and 7). */
export interface ArrangeActions {
  onGroupBy: (groupBy: GroupBy) => void;
  onSortBy: (sortBy: SortBy) => void;
  onShow: (show: Show) => void;
  onReset: () => void;
  /** Shows the sessions page (FLOW-ORGANIZE-SESSIONS, step 9). */
  onAllSessions: () => void;
}

/** A choice's submenu entry: it names the current choice, and its submenu marks it. */
function choice<C>(label: string, values: readonly C[], chosen: C, name: (value: C) => string, choose: (value: C) => void): MenuEntry {
  return {
    kind: "submenu",
    label,
    current: name(chosen),
    entries: values.map((value) => ({ kind: "item", label: name(value), checked: value === chosen, onChoose: () => choose(value) })),
  };
}

/**
 * The arrange menu's items (LAY-SHELL, The arrange button): Group by ▸, Sort by ▸ and Show ▸, each naming its current
 * choice and its submenu marking it, then Reset to defaults and All sessions apart. New section goes in the group with
 * Reset, as it is built.
 */
export function arrangeEntries(arrangement: ArrangeChoices, actions: ArrangeActions): MenuEntry[] {
  return [
    choice(t("rail.arrange.groupByButton"), GROUP_BYS, arrangement.groupBy, (g) => t(GROUP_BY_NAMES[g]), actions.onGroupBy),
    choice(t("rail.arrange.sortByButton"), SORT_BYS, arrangement.sortBy, (s) => t(SORT_BY_NAMES[s]), actions.onSortBy),
    choice(t("rail.arrange.showButton"), SHOWS, arrangement.show, (s) => t(SHOW_NAMES[s]), actions.onShow),
    { kind: "rule" },
    { kind: "item", label: t("rail.arrange.resetButton"), onChoose: actions.onReset },
    { kind: "item", label: t("rail.arrange.allSessionsButton"), onChoose: actions.onAllSessions },
  ];
}

/**
 * The arrange button beside New session and its menu (docs/design/RAIL.md, The rail and Menus): a sliders icon
 * button that reads pressed while its menu is open, which opens below it.
 */
export function ArrangeButton({
  arrangement,
  disabled,
  initialOpen,
  initialSubmenu,
  ...actions
}: ArrangeActions & {
  arrangement: ArrangeChoices;
  /** Ersa has not connected to Core, or has not read the arrangement yet. */
  disabled?: boolean;
  /** For the gallery: the menu open, taking no focus, and one of its submenus open by index. */
  initialOpen?: boolean;
  initialSubmenu?: number;
}) {
  // The menu, while open: where focus went as it opened (RAIL.md, Menus). The gallery's open menu takes none.
  const [menu, setMenu] = useState<"item" | "menu" | "none" | undefined>(initialOpen ? "none" : undefined);
  // Unavailable while open, as when Ersa loses Core, the menu closes for good rather than open again with it.
  if (disabled && menu && !initialOpen) setMenu(undefined);
  const button = useRef<HTMLButtonElement>(null);
  // The menu floats above the rail in the top layer, anchored to the button by this name.
  const anchor = `--rail-arrange-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  // Closing the menu returns focus to the button, unless a press elsewhere put it there.
  const close = useCallback((how: MenuClose) => {
    setMenu(undefined);
    if (how !== "away") button.current?.focus({ preventScroll: true });
  }, []);
  const inside = useCallback(() => [button.current], []);
  return (
    <>
      <IconButton
        ref={button}
        className="rail-arrange"
        icon={SlidersHorizontal}
        size="icon-sm"
        label={t("rail.arrange.label")}
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={!!menu}
        style={{ anchorName: anchor } as CSSProperties}
        // A click the keyboard made (Enter or Space) has no pointer presses to count.
        onClick={(event) => setMenu(menu ? undefined : event.detail === 0 ? "item" : "menu")}
      />
      <Menu
        label={t("rail.arrange.label")}
        entries={arrangeEntries(arrangement, actions)}
        open={menu !== undefined}
        place={{ anchor }}
        focus={menu}
        inside={inside}
        onClose={close}
        initialSubmenu={initialSubmenu}
      />
    </>
  );
}
