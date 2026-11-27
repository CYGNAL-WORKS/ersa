import { useId, useLayoutEffect, useRef } from "react";
import { t } from "../catalog";
import { ARCHIVE_DAYS, type ArchiveDays, type ArchiveView } from "../session/archiveInactive";
import type { Sending } from "../session/follow";
import { Chip } from "./Chip";
import { Problem } from "./RailTree";
import "./Page.css";
import "./SettingsPage.css";

/** Never's id in the chip; the days are their numbers. */
const NEVER = "never";

const ARCHIVE_OPTIONS = [
  { id: NEVER, label: t("settings.storage.archiveNeverLabel") },
  ...ARCHIVE_DAYS.map((days) => ({ id: String(days), label: t("settings.storage.archiveDaysLabel", { count: days }) })),
];

const daysOf = (id: string): ArchiveDays | undefined => ARCHIVE_DAYS.find((d) => String(d) === id);

/**
 * The settings page (LAY-SHELL, The settings page; docs/design/SETTINGS.md): the heading, the names of its sections,
 * and the chosen section's settings. Storage is the only section so far, with Archive inactive sessions
 * (FLOW-CLEAN-UP, step 4; R15). A setting takes effect when it is changed: the page has no Save.
 */
export function SettingsPage({
  archive,
  changing,
  offline,
  onArchive,
  forceChip,
  forceSection,
  initialOpen,
}: {
  /** Archive inactive sessions as last read from Core: a number of days, or undefined for Never. */
  archive: ArchiveView;
  /** A change on its way, or why Core refused the last one. */
  changing?: Sending;
  /** Ersa can't reach Core: the settings keep what was read and can't be changed. */
  offline?: boolean;
  /** Sends a choice to Core: a number of days, or undefined for Never. */
  onArchive: (days: ArchiveDays | undefined) => void;
  /** For the gallery: the chip's pointer or keyboard state, the section name's keyboard focus, and the chip's list open. */
  forceChip?: "hover" | "focus" | "pressed";
  forceSection?: "focus";
  initialOpen?: boolean;
}) {
  const storageId = useId();
  const sending = changing?.state === "sending";
  const ready = archive.state === "ready";
  const unavailable = !ready || offline === true;

  // The chip turns unavailable when Ersa loses Core, and goes when the setting can't be read. With keyboard focus on it
  // (or in its list), focus would fall to the window, so the chosen section name takes it, as the sessions page's
  // chosen tab does. Focus the user has put elsewhere stays there.
  const row = useRef<HTMLDivElement>(null);
  const chosenSection = useRef<HTMLButtonElement>(null);
  const focusInRow = useRef(false);
  useLayoutEffect(() => {
    if (!unavailable || !focusInRow.current) return;
    focusInRow.current = false;
    const now = document.activeElement;
    if (now !== null && now !== document.body && !row.current?.contains(now)) return;
    chosenSection.current?.focus({ preventScroll: true });
  }, [unavailable]);

  return (
    <section className="page settings-page">
      <div className="page-column settings-page-column">
        <h1 className="page-heading">{t("settings.settingsHeading")}</h1>
        <div className="settings-page-body">
          {/* The section names, Storage first and the one the page opens on. There is one so far. */}
          <nav className="settings-page-sections" aria-label={t("settings.sectionsLabel")}>
            <button ref={chosenSection} type="button" className="page-tab" aria-current="true" data-force={forceSection}>
              {t("settings.storage.storageHeading")}
            </button>
          </nav>
          <section className="settings-section" aria-labelledby={storageId}>
            <h2 id={storageId} className="settings-section-heading">
              {t("settings.storage.storageHeading")}
            </h2>
            <div className="setting">
              <div
                ref={row}
                className="setting-row"
                onFocus={() => {
                  focusInRow.current = true;
                }}
                onBlur={(event) => {
                  // Focus moved on by the user; one that went to nothing may be the chip turning unavailable.
                  if (event.relatedTarget !== null && !row.current?.contains(event.relatedTarget)) focusInRow.current = false;
                }}
              >
                <div className="setting-words">
                  <span className="setting-label">{t("settings.storage.archiveInactiveLabel")}</span>
                  <span className="setting-description">{t("settings.storage.archiveInactiveDescription")}</span>
                </div>
                {/* A setting that can not be read has no value to show: the problem beneath says why. */}
                {archive.state !== "error" && (
                <Chip
                  name={t("settings.storage.archiveInactiveLabel")}
                  // The value in force, or the choice on its way (docs/design/SETTINGS.md, Changing).
                  value={ready ? (archive.value === undefined ? NEVER : String(archive.value)) : undefined}
                  placeholder={archive.state === "loading" ? t("settings.loadingProgress") : undefined}
                  pending={archive.state === "loading"}
                  options={ARCHIVE_OPTIONS}
                  onChoose={(id) => onArchive(daysOf(id))}
                  waiting={sending}
                  disabled={!ready || offline}
                  force={forceChip}
                  initialOpen={initialOpen}
                />
                )}
              </div>
              {archive.state === "error" && (
                <div className="setting-problem">
                  <Problem lead={t("settings.readError")} reason={archive.message} />
                </div>
              )}
              {ready && changing?.state === "failed" && (
                <div className="setting-problem">
                  <Problem lead={t("settings.changeError")} reason={changing.message} />
                </div>
              )}
            </div>
          </section>
        </div>
      </div>
    </section>
  );
}
