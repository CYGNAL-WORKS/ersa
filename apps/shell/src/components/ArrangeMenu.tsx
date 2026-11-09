import { SlidersHorizontal } from "lucide-react";
import { type CSSProperties, useCallback, useId, useRef, useState } from "react";
import { t } from "../catalog";
import { GROUP_BYS, type GroupBy, type RailArrangement, SHOWS, type Show } from "../session/arrangement";
import { IconButton } from "./Icon";
import { Menu, type MenuClose, type MenuEntry } from "./Menu";

const GROUP_BY_NAMES = {
  project: "rail.arrange.groupByProjectButton",
  date: "rail.arrange.groupByDateButton",
  none: "rail.arrange.groupByNoneButton",
} as const satisfies Record<GroupBy, string>;
const SHOW_NAMES = {
  active: "rail.arrange.showActiveButton",
  archived: "rail.arrange.showArchivedButton",
  all: "rail.arrange.showAllButton",
} as const satisfies Record<Show, string>;

/** What the arrange menu's items do: each sends its change to Core (FLOW-ORGANIZE-SESSIONS, steps 4 and 7). */
export interface ArrangeActions {
  onGroupBy: (groupBy: GroupBy) => void;
  onShow: (show: Show) => void;
  onReset: () => void;
  /** Shows the sessions page (FLOW-ORGANIZE-SESSIONS, step 9). */
  onAllSessions: () => void;
}

/**
 * The arrange menu's items (LAY-SHELL, The arrange button): Group by ▸ and Show ▸, each naming its current choice and
 * its submenu marking it, then Reset to defaults and All sessions apart. Sort by ▸ goes after Group by, and New section
 * in the group with Reset, as they are built.
 */
export function arrangeEntries(arrangement: Pick<RailArrangement, "groupBy" | "show">, actions: ArrangeActions): MenuEntry[] {
  return [
    {
      kind: "submenu",
      label: t("rail.arrange.groupByButton"),
      current: t(GROUP_BY_NAMES[arrangement.groupBy]),
      entries: GROUP_BYS.map((g) => ({ kind: "item", label: t(GROUP_BY_NAMES[g]), checked: g === arrangement.groupBy, onChoose: () => actions.onGroupBy(g) })),
    },
    {
      kind: "submenu",
      label: t("rail.arrange.showButton"),
      current: t(SHOW_NAMES[arrangement.show]),
      entries: SHOWS.map((s) => ({ kind: "item", label: t(SHOW_NAMES[s]), checked: s === arrangement.show, onChoose: () => actions.onShow(s) })),
    },
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
  arrangement: Pick<RailArrangement, "groupBy" | "show">;
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
