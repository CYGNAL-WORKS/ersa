import { Folder, Monitor, Plus, Square, SquareCheck } from "lucide-react";
import { t } from "../catalog";
import type { BackendInfo, Choice, CoreHelloResult, ModelChoice, Project } from "../protocol/core-api.gen";
import {
  type SessionValues,
  available,
  effortsFor,
  listedModelFor,
  workspaceKinds,
} from "../session/controls";
import { isChangeable } from "../session/change";
import { Chip, type ChipOption } from "./Chip";
import { Icon } from "./Icon";
import { ContextRing } from "./ContextRing";
import type { UsageWindow } from "../session/limits";
import "./SessionControls.css";

export type Host = CoreHelloResult["host"];

/** The chips, by the value each sets. */
export type ControlKey = "host" | "project" | "workspace" | "backend" | "model" | "effort" | "permission_mode";

/** What every chip takes from the session controls. */
interface ChipHandlers {
  onChoose: (id: string) => void;
  onOpen?: () => void;
  disabled?: boolean;
  readOnly?: boolean;
  keepsChevron?: boolean;
  waiting?: boolean;
  initialOpen: boolean;
  force?: "hover" | "focus" | "pressed";
}



interface SessionControlsProps {
  hosts: readonly Host[];
  /** The host Ersa runs on, whose chip reads Local. */
  localHostId: string;
  /** Most recently used first: the project chip lists them in this order (FLOW-NEW-SESSION-R14). */
  projects: readonly Project[];
  /** The backends on the values' host, from `backend.list`; absent while Core is still detecting them. */
  backends?: readonly BackendInfo[];
  /** Why the host could not list its backends: the backend chip says detection failed (FLOW-NEW-SESSION-R11). */
  detectionFailed?: boolean;
  values: SessionValues;
  onChoose: (key: ControlKey, id: string) => void;
  /** Add project…, which ends the project chip's list; absent while it cannot be taken, as on a remote host (FLOW-NEW-SESSION-R27). */
  onAddProject?: () => void;
  /** While the first message is on its way, nothing changes. */
  disabled?: boolean;
  /** Under a session: its values, as pills that do not open (docs/design/COMPOSER.md, The session controls). */
  readOnly?: boolean;
  /** Under a session that is running: its model, effort and permission mode chips open (FLOW-NEW-SESSION-R30). */
  changeable?: boolean;
  /** The chips whose change is on its way to Core: each reads its old value and does not open. */
  waiting?: readonly ControlKey[];
  /** A chip's list opened. */
  onOpen?: () => void;
  /** The context ring's share used; absent while none is reported. */
  used?: number;
  /**
   * The usage windows the session's backend last reported, which the ring reads with the context (LAY-SHELL,
   * Context); none on the new-session screen and before a session's first report.
   */
  windows?: readonly UsageWindow[];
  /** For the gallery: one chip shown open, or in a pointer or keyboard state. */
  open?: ControlKey;
  force?: { key: ControlKey; state: "hover" | "focus" | "pressed" };
}

const backendOption = (b: BackendInfo): ChipOption => ({
  id: b.name,
  label: b.label,
  status: b.status === "missing" ? "Not installed" : b.status === "error" ? "Detection failed" : b.version,
  failed: b.status === "error",
  unavailable: b.status !== "available",
});

/**
 * A project as the project chip lists it (FLOW-NEW-SESSION-R14): its name, its folder's path beneath, its kind, and
 * whether it is trusted. Git is the kind Ersa makes workspaces for; any other folder is a folder.
 */
const projectOption = (p: Project): ChipOption => ({
  id: p.id,
  label: p.name,
  detail: p.path,
  status: p.vcs === "git" ? t("sessionControls.project.withGitKindLabel") : t("sessionControls.project.withoutGitKindLabel"),
  caution: p.trusted ? undefined : t("trust.notTrustedStatus"),
});

const choiceOptions = (choices: readonly Choice[]): ChipOption[] => choices.map(({ id, label }) => ({ id, label }));

/**
 * The model chip's value and options (FLOW-NEW-SESSION-R46). Where a listed model not marked default resolves to the
 * same model as the backend's default, the default has no row of its own: while the default is the value, the chip reads
 * and marks the first such model, so choosing that row changes nothing (the chip passes on only a different row) and a
 * session started so still starts with the default. A default the backend says nothing more of, or one no listed model
 * stands for, keeps its row and the backend's own label: Ersa never renames what it cannot explain.
 */
export function modelChip(models: readonly ModelChoice[], model: string | undefined): { value: string | undefined; options: ChipOption[] } {
  const fallback = models.find((m) => m.default);
  const standIn = fallback?.resolved !== undefined ? listedModelFor(models, fallback.resolved) : undefined;
  const options = models.filter((m) => !standIn || m !== fallback).map(({ id, label }) => ({ id, label }));
  return { value: standIn && model === fallback?.id ? standIn.id : model, options };
}

/** The effort chip's Default row, which chooses no level: its id, which the screen turns back into none. */
export const DEFAULT_EFFORT = "";

/**
 * The effort chip's value and options (FLOW-NEW-SESSION-R47), from the levels the model takes. With none chosen it reads
 * and marks the level the backend marks as its default, in a list of the levels alone, and `value` stays absent so that
 * choosing that level names it: a session started so names none, and the backend runs at its own, which a project's
 * settings may set. Where none of the levels is marked it reads Default, and before a session starts Default is the
 * list's first row, which chooses no level; a running session's list holds only the levels.
 */
export function effortChip(levels: readonly Choice[], effort: string | undefined, running: boolean): { value?: string; marked?: string; reads?: string; options: ChipOption[] } {
  const options = choiceOptions(levels);
  const marked = levels.find((c) => c.default);
  if (marked) return { value: effort, marked: marked.id, options };
  const defaultRow: ChipOption = { id: DEFAULT_EFFORT, label: t("sessionControls.effort.defaultLabel") };
  return {
    value: effort ?? DEFAULT_EFFORT,
    reads: effort === undefined ? t("sessionControls.effort.defaultLabel") : undefined,
    options: running ? options : [defaultRow, ...options],
  };
}

/**
 * The effort chip (docs/design/COMPOSER.md, The session controls): the level alone on the chip as Claude Desktop shows
 * it, while its accessible name still says it is the effort, its value and list as `effortChip` gives them. A model that
 * takes no effort leaves the chip in its place, unavailable, reading None.
 */
function EffortChip({ backend, values, readOnly, chip }: { backend: BackendInfo; values: SessionValues; readOnly?: boolean; chip: ChipHandlers }) {
  const levels = effortsFor(backend, values.model);
  if (levels.length === 0) {
    return <Chip name="Effort" reads={t("sessionControls.effort.noneLabel")} options={[]} {...chip} disabled />;
  }
  return <Chip name="Effort" {...effortChip(levels, values.effort, readOnly ?? false)} {...chip} />;
}

/**
 * The Worktree checkbox (issue 412), in the row as Claude Desktop's: checked, the session runs in a new isolated workspace
 * made from the repository's default branch; unchecked, in the project's own folder. A native checkbox inside its
 * label, so Space toggles it and assistive technology hears a checkbox; the box is drawn with Lucide's square, as the
 * agent's task lists are, and the label is itself a chip.
 */
function WorktreeCheckbox({ checked, onChange, disabled, force }: { checked: boolean; onChange: (checked: boolean) => void; disabled?: boolean; force?: "hover" | "focus" | "pressed" }) {
  const description = t("sessionControls.worktree.description");
  return (
    <label className="chip worktree" title={description} data-force={force} data-disabled={disabled || undefined}>
      <input
        className="worktree-input"
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-description={description}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="worktree-box" data-checked={checked || undefined} aria-hidden="true">
        <Icon icon={checked ? SquareCheck : Square} size="icon-xs" />
      </span>
      {t("sessionControls.worktree.label")}
    </label>
  );
}

/**
 * The session controls (GLOSSARY): a chip for each of host and project, the Worktree checkbox for a project Ersa
 * can make worktrees for, and a chip for each of backend, model, effort and permission mode, with the context ring at
 * the row's end (LAY-SHELL). The backend chip shows whenever the host's backends are listed, with one backend as with
 * several (FLOW-NEW-SESSION-R48), and lists a missing backend with its status (FLOW-NEW-SESSION-R07).
 */
export function SessionControls({ hosts, localHostId, projects, backends, detectionFailed, values, onChoose, onAddProject, disabled, readOnly, changeable, waiting, onOpen, used, windows, open, force }: SessionControlsProps) {
  const project = projects.find((p) => p.id === values.project_id);
  const backend = backends?.find((b) => b.name === values.backend);
  const chip = (key: ControlKey): ChipHandlers => ({
    onChoose: (id: string) => onChoose(key, id),
    onOpen,
    disabled,
    // Under a session, only a running one's model, effort and permission mode open; those keep the chevron's place.
    readOnly: readOnly && !(changeable && isChangeable(key)),
    keepsChevron: readOnly && isChangeable(key),
    waiting: waiting?.includes(key),
    initialOpen: open === key,
    force: force?.key === key ? force.state : undefined,
  });

  return (
    <div className="session-controls" role="group" aria-label="Session controls">
      {/* Where the session runs leads the row; how it runs, and the context ring, trail it. */}
      <div className="session-controls-where">
        <Chip
          name="Host"
          icon={Monitor}
          value={values.host_id}
          // A session runs in a project, so a host with none has nothing to start yet.
          options={hosts.map((h) => {
            const label = h.id === localHostId ? t("sessionControls.localHostLabel") : h.name;
            return projects.some((p) => p.host_id === h.id) ? { id: h.id, label } : { id: h.id, label, status: t("sessionControls.noProjectsStatus"), unavailable: true };
          })}
          {...chip("host")}
        />
        <Chip
          name="Project"
          icon={Folder}
          value={values.project_id}
          options={projects.filter((p) => p.host_id === values.host_id).map(projectOption)}
          // With no project on the host, the chip offers to choose a folder (FLOW-NEW-SESSION-R19).
          placeholder={t("sessionControls.project.chooseFolderButton")}
          invites
          wide
          // The only way to add a project (FLOW-NEW-SESSION-R27, R20); its shortcut shows while it can be taken.
          action={{
            label: t("sessionControls.project.addProjectButton"),
            icon: Plus,
            shortcut: t("sessionControls.project.addProjectShortcutLabel"),
            onChoose: onAddProject,
          }}
          {...chip("project")}
        />
        {/* Only where Ersa can make an isolated workspace for the project (FLOW-NEW-SESSION-R06). */}
        {project && workspaceKinds(project).includes("isolated") && (
          <WorktreeCheckbox
            checked={values.workspace === "isolated"}
            onChange={(checked) => onChoose("workspace", checked ? "isolated" : "root")}
            // Under a session it shows where the session runs, and does not change.
            disabled={disabled || readOnly}
            force={force?.key === "workspace" ? force.state : undefined}
          />
        )}
      </div>
      <div className="session-controls-how">
        {backends === undefined ? (
          <Chip name="Backend" placeholder={t("sessionControls.detectingBackendsProgress")} pending options={[]} onChoose={() => {}} disabled />
        ) : (
          <>
            {/* Always in the row, so nothing shifts when a listing arrives. Under a session it is the session's backend,
                which reads its id until a listing names it (FLOW-NEW-SESSION-R48, R49). */}
            <Chip
              name="Backend"
              value={values.backend}
              // A listing that failed is not a host without backends (FLOW-NEW-SESSION-R11).
              placeholder={t(detectionFailed ? "sessionControls.detectionFailedStatus" : "sessionControls.noBackendStatus")}
              problem={available(backends).length === 0}
              options={backends.map(backendOption)}
              {...chip("backend")}
              // A host with no backend at all has nothing to list; the chip says so itself.
              disabled={disabled || backends.length === 0}
            />
            {/* A backend that offers no choice of one has no chip for it: the backend decides. */}
            {backend && backend.models.length > 0 && (
              <Chip name="Model" {...modelChip(backend.models, values.model)} {...chip("model")} />
            )}
            {backend && backend.efforts.length > 0 && <EffortChip backend={backend} values={values} readOnly={readOnly} chip={chip("effort")} />}
            {backend && backend.permission_modes.length > 0 && (
              <Chip
                name="Permission mode"
                value={values.permission_mode}
                options={choiceOptions(backend.permission_modes)}
                {...chip("permission_mode")}
              />
            )}
          </>
        )}
        <ContextRing used={used} windows={windows ?? []} beforeSession={!readOnly} />
      </div>
    </div>
  );
}
