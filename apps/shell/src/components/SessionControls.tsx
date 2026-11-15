import { Folder, Monitor, Plus, Square, SquareCheck } from "lucide-react";
import { t } from "../catalog";
import type { BackendInfo, Choice, CoreHelloResult, ModelChoice, Project } from "../protocol/core-api.gen";
import {
  type SessionValues,
  available,
  effortsFor,
  listedModelFor,
  showsBackend,
  workspaceKinds,
} from "../session/controls";
import { isChangeable } from "../session/change";
import { Chip, type ChipOption } from "./Chip";
import { Icon } from "./Icon";
import { ContextRing } from "./ContextRing";
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
 * The model chip's options (FLOW-NEW-SESSION-R36): each model by the backend's label, except a default the backend
 * says stands for a listed model, which reads Default and that model's label, as Claude Desktop's chip does. A
 * default the backend says nothing more of keeps the backend's own label: Ersa never renames what it cannot explain.
 */
export function modelOptions(models: readonly ModelChoice[]): ChipOption[] {
  return models.map(({ id, label, default: isDefault, resolved }) => {
    const listed = isDefault && resolved !== undefined ? listedModelFor(models, resolved) : undefined;
    return { id, label: listed ? t("sessionControls.model.defaultOfLabel", { model: listed.label }) : label };
  });
}

/** The effort chip's Default row, which chooses no level: its id, which the screen turns back into none. */
export const DEFAULT_EFFORT = "";

/**
 * The effort chip (docs/design/COMPOSER.md, The session controls): the levels the chosen model takes, the level alone
 * on the chip as Claude Desktop shows it, while its accessible name still says it is the effort. With none chosen
 * it reads Default, and before the session starts Default is its list's first row; a running session's list holds only
 * the levels. A model that takes no effort leaves the chip in its place, unavailable, reading None.
 */
function EffortChip({ backend, values, readOnly, chip }: { backend: BackendInfo; values: SessionValues; readOnly?: boolean; chip: ChipHandlers }) {
  const levels = choiceOptions(effortsFor(backend, values.model));
  if (levels.length === 0) {
    return <Chip name="Effort" reads={t("sessionControls.effort.noneLabel")} options={[]} {...chip} disabled />;
  }
  const defaultRow: ChipOption = { id: DEFAULT_EFFORT, label: t("sessionControls.effort.defaultLabel") };
  return (
    <Chip
      name="Effort"
      value={values.effort ?? DEFAULT_EFFORT}
      reads={values.effort === undefined ? t("sessionControls.effort.defaultLabel") : undefined}
      options={readOnly ? levels : [defaultRow, ...levels]}
      {...chip}
    />
  );
}

/**
 * The Worktree checkbox (issue 412), in the row as Claude Desktop's: checked, the session runs in a new isolated workspace
 * made from the repository's default branch; unchecked, in the project's own folder. A native checkbox inside its
 * label, so Space toggles it and assistive technology hears a checkbox; the box is drawn with Lucide's square, as the
 * agent's task lists are, and the label is a pill shaped like a chip.
 */
function WorktreeCheckbox({ checked, onChange, disabled, force }: { checked: boolean; onChange: (checked: boolean) => void; disabled?: boolean; force?: "hover" | "focus" | "pressed" }) {
  const description = t("sessionControls.worktree.description");
  return (
    <label className="worktree" title={description} data-force={force} data-disabled={disabled || undefined}>
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
 * The session controls (GLOSSARY): a chip for each of host, project, workspace, backend, model, effort and
 * permission mode, with the context ring at the row's end (LAY-SHELL). The backend chip shows only when the host
 * has more than one backend, or none to run, and lists a missing backend with its status (FLOW-NEW-SESSION-R07).
 */
export function SessionControls({ hosts, localHostId, projects, backends, detectionFailed, values, onChoose, onAddProject, disabled, readOnly, changeable, waiting, onOpen, used, open, force }: SessionControlsProps) {
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
            {showsBackend(backends) && (
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
            )}
            {/* A backend that offers no choice of one has no chip for it: the backend decides. */}
            {backend && backend.models.length > 0 && (
              <Chip name="Model" value={values.model} options={modelOptions(backend.models)} {...chip("model")} />
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
        <ContextRing used={used} beforeSession={!readOnly} />
      </div>
    </div>
  );
}
