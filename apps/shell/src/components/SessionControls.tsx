import { Folder, Plus } from "lucide-react";
import { t } from "../catalog";
import type { CoreHelloResult, Project } from "../protocol/core-api.gen";
import { type BackendChoices, type Choice, type SessionValues, type WorkspaceKind, available, showsBackend, workspaceKinds } from "../session/controls";
import { Chip, type ChipOption } from "./Chip";
import { ContextRing } from "./ContextRing";
import "./SessionControls.css";

export type Host = CoreHelloResult["host"];

/** The chips, by the value each sets. */
export type ControlKey = "host" | "project" | "workspace" | "backend" | "model" | "effort" | "permission_mode";

interface SessionControlsProps {
  hosts: readonly Host[];
  /** The host Ersa runs on, whose chip reads Local. */
  localHostId: string;
  /** Most recently used first: the project chip lists them in this order (FLOW-NEW-SESSION-R14). */
  projects: readonly Project[];
  /** The backends on the values' host, from `backend.list`; absent while Core is still detecting them. */
  backends?: readonly BackendChoices[];
  values: SessionValues;
  /** The workspace as a running session's controls name it: its branch, or the root. */
  workspaceLabel?: string;
  onChoose: (key: ControlKey, id: string) => void;
  /** Add project…, which ends the project chip's list; absent while it cannot be taken, as on a remote host (FLOW-NEW-SESSION-R16). */
  onAddProject?: () => void;
  /** While the first message is on its way, nothing changes. */
  disabled?: boolean;
  /** The context ring's share used; absent before the session has used any. */
  used?: number;
  /** For the gallery: one chip shown open, or in a pointer or keyboard state. */
  open?: ControlKey;
  force?: { key: ControlKey; state: "hover" | "focus" | "pressed" };
}

const WORKSPACE_LABELS: Record<WorkspaceKind, string> = { isolated: "New workspace", root: "Root workspace" };

const backendOption = (b: BackendChoices): ChipOption => ({
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
 * The session controls (GLOSSARY): a chip for each of host, project, workspace, backend, model, effort and
 * permission mode, with the context ring at the row's end (LAY-SHELL). The backend chip shows only when the host
 * has more than one backend, or none to run, and lists a missing backend with its status (FLOW-NEW-SESSION-R07).
 */
export function SessionControls({ hosts, localHostId, projects, backends, values, workspaceLabel, onChoose, onAddProject, disabled, used, open, force }: SessionControlsProps) {
  const project = projects.find((p) => p.id === values.project_id);
  const backend = backends?.find((b) => b.name === values.backend);
  const chip = (key: ControlKey) => ({
    onChoose: (id: string) => onChoose(key, id),
    disabled,
    initialOpen: open === key,
    force: force?.key === key ? force.state : undefined,
  });

  return (
    <div className="session-controls" role="group" aria-label="Session controls">
      <div className="session-controls-chips">
        <Chip
          name="Host"
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
          wide
          // The only way to add a project (FLOW-NEW-SESSION-R16, R20); its shortcut shows while it can be taken.
          action={{
            label: t("sessionControls.project.addProjectButton"),
            icon: Plus,
            shortcut: t("sessionControls.project.addProjectShortcutLabel"),
            onChoose: onAddProject,
          }}
          {...chip("project")}
        />
        {project && (
          <Chip
            name="Workspace"
            value={values.workspace}
            options={
              workspaceLabel === undefined
                ? workspaceKinds(project).map((kind) => ({ id: kind, label: WORKSPACE_LABELS[kind] }))
                : [{ id: values.workspace, label: workspaceLabel }]
            }
            {...chip("workspace")}
          />
        )}
        {backends === undefined ? (
          <Chip name="Backend" placeholder="Detecting backends…" pending options={[]} onChoose={() => {}} disabled />
        ) : (
          <>
            {showsBackend(backends) && (
              <Chip
                name="Backend"
                value={values.backend}
                placeholder="No backend"
                problem={available(backends).length === 0}
                options={backends.map(backendOption)}
                {...chip("backend")}
                // A host with no backend at all has nothing to list; the chip says so itself.
                disabled={disabled || backends.length === 0}
              />
            )}
            {/* A backend that offers no choice of one has no chip for it: the backend decides. */}
            {backend && backend.models.length > 0 && (
              <Chip name="Model" value={values.model} options={choiceOptions(backend.models)} {...chip("model")} />
            )}
            {backend && backend.efforts.length > 0 && (
              <Chip name="Effort" prefix="Effort" value={values.effort} options={choiceOptions(backend.efforts)} {...chip("effort")} />
            )}
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
      </div>
      <ContextRing used={used} />
    </div>
  );
}
