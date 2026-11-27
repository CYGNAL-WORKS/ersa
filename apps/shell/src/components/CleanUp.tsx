import { Square, SquareCheck } from "lucide-react";
import { type ReactNode, useEffect, useId, useLayoutEffect, useRef } from "react";
import { t } from "../catalog";
import { size } from "../session/attachments";
import type { CleanupInspectResult, Project, Session } from "../protocol/core-api.gen";
import { CLEAN_UP_DAYS, GROUPS, type ItemView, itemName, losesWork, lostCounts, lostParts, unfinished } from "../session/cleanup";
import type { CleanUpActions, CleanUpView } from "../session/cleanupList";
import { lastActive } from "../session/sessionsPage";
import { HeightEase } from "../theme/motion";
import { Button } from "./Button";
import { Chip } from "./Chip";
import { ContextRing } from "./ContextRing";
import { Icon } from "./Icon";
import { Problem } from "./RailTree";
import "./CleanUp.css";

/** The names the list shows: sessions' titles and projects' names, as the window holds them. */
export interface CleanUpNames {
  sessions: readonly Pick<Session, "id" | "title">[];
  projects: readonly Pick<Project, "id" | "name">[];
}

const DAYS_OPTIONS = CLEAN_UP_DAYS.map((days) => ({ id: String(days), label: t("settings.storage.daysLabel", { count: days }) }));

const HEADINGS = {
  workspace: () => t("settings.storage.cleanUp.workspaceHeading"),
  leftover: () => t("settings.storage.cleanUp.leftoverHeading"),
  branch: () => t("settings.storage.cleanUp.branchHeading"),
  folder: () => t("settings.storage.cleanUp.folderHeading"),
} as const;

/**
 * Clean up under Storage (FLOW-CLEAN-UP, steps 5 to 7; docs/design/SETTINGS.md, Clean up): the setting whose days
 * choose which sessions count as inactive, the list Core gives in groups, each item with what it is, the space it
 * frees and what removing it loses, ticked when that is nothing (R16), and Clean up…, which asks first (R17). What
 * the last clean-up did is said above the list (R18).
 */
export function CleanUp({
  view,
  actions,
  names,
  offline,
  now,
  forceItem,
}: {
  view: CleanUpView;
  actions: CleanUpActions;
  names: CleanUpNames;
  /** Ersa can't reach Core: what was read stays, and nothing can be chosen or removed. */
  offline?: boolean;
  /** For the gallery: the time "Last active" is counted from. */
  now?: Date;
  /** For the gallery: the first item's pointer or keyboard state. */
  forceItem?: "hover" | "focus" | "pressed";
}) {
  const running = view.run !== undefined;
  const unavailable = offline === true || running;
  const ready = view.list.state === "ready" ? view.list : undefined;
  const count = view.ticked.size;
  // The gallery's forced state goes to the first item shown, in group order.
  const first = ready && GROUPS.flatMap((kind) => ready.items.filter((v) => v.item.kind === kind))[0]?.item.id;

  // What the list holds arrives late and changes its height: it eases to the new one, so nothing pops in (DESIGN.md).
  const body = useRef<HTMLDivElement>(null);
  const ease = useRef(new HeightEase()).current;
  useLayoutEffect(() => {
    if (body.current) ease.change(body.current, true, { clipGrowth: true });
  });

  // When the question closes, focus comes back to Clean up…, or, while the list is read again after a clean-up and the
  // button is unavailable, to the list, which then says what was done.
  const button = useRef<HTMLButtonElement>(null);
  const wasRunning = useRef(running);
  useEffect(() => {
    if (wasRunning.current && !running) (button.current && !button.current.disabled ? button.current : body.current)?.focus({ preventScroll: true });
    wasRunning.current = running;
  }, [running]);

  return (
    <div className="setting clean-up">
      <div className="setting-row">
        <div className="setting-words">
          <span className="setting-label">{t("settings.storage.cleanUp.inactiveLabel")}</span>
          <span className="setting-description">{t("settings.storage.cleanUp.inactiveDescription")}</span>
        </div>
        <Chip
          name={t("settings.storage.cleanUp.inactiveLabel")}
          value={String(view.days)}
          options={DAYS_OPTIONS}
          onChoose={(id) => {
            const days = CLEAN_UP_DAYS.find((d) => String(d) === id);
            if (days !== undefined) actions.choose(days);
          }}
          disabled={unavailable}
        />
      </div>
      <div ref={body} tabIndex={-1} className="clean-up-body" aria-label={t("settings.storage.cleanUp.listLabel")} role="region" aria-busy={view.list.state === "loading" || undefined}>
        {view.result && <ResultNote result={view.result} names={names} />}
        {view.list.state === "loading" && <Checking />}
        {view.list.state === "error" && (
          <div className="clean-up-problem">
            <Problem lead={t("settings.storage.cleanUp.listError")} reason={view.list.message} />
          </div>
        )}
        {ready?.unread.map((u) => (
          <div key={u.project_id} className="clean-up-problem">
            <Problem lead={t("settings.storage.cleanUp.unreadError", { project: projectName(names, u.project_id) })} reason={u.message} />
          </div>
        ))}
        {ready && ready.items.length === 0 && ready.unread.length === 0 && <p className="clean-up-empty">{t("settings.storage.cleanUp.nothingEmpty")}</p>}
        {ready &&
          GROUPS.map((kind) => {
            const items = ready.items.filter((v) => v.item.kind === kind);
            if (items.length === 0) return null;
            return (
              <Group key={kind} heading={HEADINGS[kind]()}>
                {items.map((v) => (
                  <Item
                    key={v.item.id}
                    view={v}
                    ticked={view.ticked.has(v.item.id)}
                    onTick={(on) => actions.tick(v.item.id, on)}
                    disabled={unavailable}
                    names={names}
                    now={now}
                    force={v.item.id === first ? forceItem : undefined}
                  />
                ))}
              </Group>
            );
          })}
        {ready && ready.items.length > 0 && (
          <div className="clean-up-foot">
            <span className="clean-up-chosen">{t("settings.storage.cleanUp.chosenLabel", { count })}</span>
            <Button ref={button} className="clean-up-button" onClick={actions.ask} disabled={unavailable || count === 0}>
              {t("settings.storage.cleanUp.cleanUpButton")}
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}

function projectName(names: CleanUpNames, id: string): string {
  return names.projects.find((p) => p.id === id)?.name ?? id;
}

/** "Checking…" led by the ring that turns for work of unknown length; it holds still with reduced motion. */
function Checking() {
  return (
    <span className="clean-up-checking" role="status">
      <ContextRing turning size="ring-sm" />
      {t("settings.storage.cleanUp.checkingProgress")}
    </span>
  );
}

function Group({ heading, children }: { heading: string; children: ReactNode }) {
  const id = useId();
  return (
    <section className="clean-up-group" aria-labelledby={id}>
      <h3 id={id} className="clean-up-heading">
        {heading}
      </h3>
      <ul className="clean-up-items">{children}</ul>
    </section>
  );
}

/**
 * An item: a checkbox whose label is the whole row, its name, beneath it its project, when it was last active, its
 * branch and size, then what removing it loses (R16, R28, R29), "Checking…" until it is inspected, or why it couldn't
 * be (R19). Only a checked item can be ticked: removing one needs what its inspect gave.
 */
function Item({
  view,
  ticked,
  onTick,
  disabled,
  names,
  now,
  force,
}: {
  view: ItemView;
  ticked: boolean;
  onTick: (on: boolean) => void;
  disabled: boolean;
  names: CleanUpNames;
  now?: Date;
  force?: "hover" | "focus" | "pressed";
}) {
  const id = useId();
  const { item } = view;
  const inspected = view.state === "inspected" ? view.inspected : undefined;
  const off = disabled || inspected === undefined;
  // A branch and a folder are named as they are written, in mono; a session's folder by its sessions' titles, its
  // branch beside its project.
  const named = item.kind === "workspace" && (item.session_ids?.length ?? 0) > 0 ? "title" : "code";
  const bytes = inspected?.size_bytes;
  const when = item.kind === "workspace" && item.last_activity_at ? lastActive({ created_at: item.last_activity_at, last_activity_at: item.last_activity_at }, now ?? new Date()) : undefined;
  return (
    <li className="clean-up-item">
      <label className="clean-up-row" data-force={force} data-disabled={off || undefined}>
        <input
          className="clean-up-input"
          type="checkbox"
          checked={ticked}
          disabled={off}
          aria-labelledby={`${id}-name`}
          aria-describedby={`${id}-meta ${id}-path ${id}-loss`}
          onChange={(event) => onTick(event.target.checked)}
        />
        <span className="clean-up-box" data-checked={ticked || undefined} aria-hidden="true">
          <Icon icon={ticked ? SquareCheck : Square} size="icon-sm" />
        </span>
        <span className="clean-up-words">
          <span id={`${id}-name`} className="clean-up-name" data-named={named}>
            {itemName(item, names.sessions)}
          </span>
          <span id={`${id}-meta`} className="clean-up-meta">
            <span>{projectName(names, item.project_id)}</span>
            {when && <span>{when}</span>}
            {named === "title" && item.branch && <span className="clean-up-code">{item.branch}</span>}
            {bytes !== undefined && <span className="clean-up-size">{size(bytes)}</span>}
          </span>
          {/* A folder named by its own name says where it is, on a line of its own. */}
          {named === "code" && item.path && (
            <span id={`${id}-path`} className="clean-up-code clean-up-path">
              {item.path}
            </span>
          )}
          <span id={`${id}-loss`} className="clean-up-loss">
            {view.state === "checking" && <Checking />}
            {view.state === "uninspectable" && <Problem lead={t("settings.storage.cleanUp.inspectError")} reason={view.message} />}
            {inspected && <Loss inspected={inspected} />}
          </span>
        </span>
      </label>
    </li>
  );
}

/** What removing a checked item loses: nothing, its changes already merged, or each loss led by the caution glyph. */
function Loss({ inspected }: { inspected: CleanupInspectResult }) {
  if (inspected.kind === "folder") {
    return <Caution>{unfinished(inspected) ? t("settings.storage.cleanUp.unfinishedDetail") : t("settings.storage.cleanUp.folderDetail")}</Caution>;
  }
  if (inspected.kind === "branch" && inspected.merged === true) return <span className="clean-up-nothing">{t("settings.storage.cleanUp.mergedDetail")}</span>;
  if (!losesWork(inspected)) return <span className="clean-up-nothing">{t("settings.storage.cleanUp.nothingLostDetail")}</span>;
  const parts = lostParts(lostCounts(inspected));
  return (
    <>
      {parts && <Caution>{t("settings.storage.cleanUp.lossDetail", { parts })}</Caution>}
      {inspected.unrecorded === true && <Caution>{t("settings.storage.cleanUp.unrecordedDetail")}</Caution>}
      {inspected.holds_repository === true && <Caution>{t("settings.storage.cleanUp.repositoryDetail")}</Caution>}
    </>
  );
}

function Caution({ children }: { children: ReactNode }) {
  return (
    <span className="clean-up-caution">
      <span className="glyph" data-tone="caution" aria-hidden="true" />
      <span>{children}</span>
    </span>
  );
}

/** What the last clean-up did: how many it removed, that it stopped, and each item it could not remove and why (R18). */
function ResultNote({ result, names }: { result: NonNullable<CleanUpView["result"]>; names: CleanUpNames }) {
  return (
    <div className="clean-up-result" role="status">
      <p className="clean-up-result-line">
        <span className="glyph" data-tone={result.failures.length === 0 && !result.stopped ? "positive" : undefined} aria-hidden="true" />
        <span>
          {t("settings.storage.cleanUp.resultDetail", { count: result.count, removed: result.removed })}
          {result.stopped && <> {t("settings.storage.cleanUp.stoppedDetail")}</>}
        </span>
      </p>
      {result.failures.map((f) => (
        <Problem key={f.item.id} lead={t("settings.storage.cleanUp.notRemovedError", { name: itemName(f.item, names.sessions) })} reason={f.message} />
      ))}
    </div>
  );
}
