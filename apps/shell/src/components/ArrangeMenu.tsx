import { SlidersHorizontal } from "lucide-react";
import { t } from "../catalog";
import { type ArrangeChoices, GROUP_BYS, type GroupBy, SHOWS, SORT_BYS, type Show, type SortBy } from "../session/arrangement";
import type { MenuEntry } from "./Menu";
import { MenuButton } from "./MenuButton";

const GROUP_BY_NAMES = {
  project: "rail.arrange.groupByProjectButton",
  section: "rail.arrange.groupBySectionButton",
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
  /** Shows a new section's heading with its name field, grouping by section first (FLOW-ORGANIZE-SESSIONS, step 5; R36). */
  onNewSection: () => void;
  /** Why New section can't be used now, such as the sections not being read; given, it shows unavailable with this. */
  newSectionUnavailable?: string;
  onReset: () => void;
  /** Shows the sessions page (FLOW-ORGANIZE-SESSIONS, step 9). */
  onAllSessions: () => void;
  /** Whether the workbench is open, which marks Developer pane, and what choosing it does (FLOW-READ-RAW-LOG-R01). */
  workbenchOpen: boolean;
  onDeveloperPane: () => void;
  /** Shows the settings page (LAY-SHELL, The settings page), until the title bar's menu does. */
  onSettings: () => void;
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
 * choice and its submenu marking it, then New section, Reset to defaults and View all apart, and last, apart from the
 * rest until the title bar is built, Developer pane, checked while the workbench is open (LAY-SHELL, The workbench),
 * and Settings, which opens the settings page (LAY-SHELL, The settings page).
 */
export function arrangeEntries(arrangement: ArrangeChoices, actions: ArrangeActions): MenuEntry[] {
  return [
    choice(t("rail.arrange.groupByButton"), GROUP_BYS, arrangement.groupBy, (g) => t(GROUP_BY_NAMES[g]), actions.onGroupBy),
    choice(t("rail.arrange.sortByButton"), SORT_BYS, arrangement.sortBy, (s) => t(SORT_BY_NAMES[s]), actions.onSortBy),
    choice(t("rail.arrange.showButton"), SHOWS, arrangement.show, (s) => t(SHOW_NAMES[s]), actions.onShow),
    { kind: "rule" },
    { kind: "item", label: t("rail.arrange.newSectionButton"), unavailable: actions.newSectionUnavailable, onChoose: actions.onNewSection },
    { kind: "item", label: t("rail.arrange.resetButton"), onChoose: actions.onReset },
    { kind: "item", label: t("rail.arrange.viewAllButton"), onChoose: actions.onAllSessions },
    { kind: "rule" },
    { kind: "item", label: t("workbench.developerPaneButton"), checked: actions.workbenchOpen, onChoose: actions.onDeveloperPane },
    { kind: "item", label: t("rail.arrange.settingsButton"), onChoose: actions.onSettings },
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
  return (
    <MenuButton
      className="rail-arrange"
      icon={SlidersHorizontal}
      label={t("rail.arrange.label")}
      entries={arrangeEntries(arrangement, actions)}
      unavailable={disabled}
      initialOpen={initialOpen}
      initialSubmenu={initialSubmenu}
    />
  );
}
