// The string catalog's US English source (docs/design/CONTENT.md, "The string
// catalog"). Every string the Interface shows lives here, keyed by area and
// purpose, and a key's suffix names its kind (`Button`, `Error`, `Progress`,
// `Status`, `Placeholder`, `Empty`, `Confirmation`), which the catalog test
// checks. Another locale is a sibling file with the same keys.
//
// After a change, run `node scripts/gen-catalog.mjs` to rewrite the text
// export, en-US.gen.md, which is what review and Vale read.
//
// Plain data with no runtime imports, so that script can load it directly.

import type { Section } from "./lookup";

export const enUS = {
  rail: {
    // The sessions page (LAY-SHELL, The sessions page). A row's time reads "Just now", "5 minutes ago", "Yesterday", then a date.
    sessionsPage: {
      sessionsHeading: "Sessions",
      tabsLabel: "Which sessions to list",
      activeTabButton: "Active",
      archivedTabButton: "Archived",
      searchLabel: "Search sessions",
      noMatchEmpty: "No sessions match your search.",
      // Why the sessions can't be read while Ersa can't reach them.
      unavailableReason: "Ersa isn't connected to your sessions.",
      justNowLabel: "Just now",
      minutesAgoLabel: { one: "{count} minute ago", other: "{count} minutes ago" },
      hoursAgoLabel: { one: "{count} hour ago", other: "{count} hours ago" },
      yesterdayLabel: "Yesterday",
      // Names the page when it fails to draw: "The sessions page could not be shown:".
      boundaryName: "The sessions page",
    },
    // The rail's top row.
    top: {
      newSessionButton: "New session",
    },
    // The arrange button and its menu (LAY-SHELL, The arrange button; FLOW-ORGANIZE-SESSIONS, step 4). An option
    // names the current choice before its submenu's chevron too.
    arrange: {
      // The arrange button's name and its menu's, for assistive technology and as its tooltip.
      label: "Arrange sessions",
      groupByButton: "Group by",
      sortByButton: "Sort by",
      showButton: "Show",
      resetButton: "Reset to defaults",
      allSessionsButton: "All sessions",
      groupByProjectButton: "Project",
      groupByDateButton: "Date",
      groupByNoneButton: "None",
      sortByCreatedButton: "Date created",
      sortByLastActivityButton: "Last activity",
      sortByNameButton: "Name",
      showActiveButton: "Active",
      showArchivedButton: "Archived",
      showAllButton: "All",
    },
    // The rail's tree (docs/design/RAIL.md, The rail): its name and its notes.
    tree: {
      // The tree's name for assistive technology.
      label: "Projects and sessions",
      loadingProgress: "Loading…",
      noProjectsEmpty: "No projects yet.",
      // A project is added from the new-session screen (LAY-SHELL, The rail).
      noProjectsHint: "Choose a folder in New session to start.",
      projectsError: "Ersa can't read your projects.",
      sessionsError: "Ersa can't read your sessions.",
      // The arrangement could not be read, so the rail is arranged by the defaults; or a change could not be saved and was undone.
      arrangeReadError: "Ersa can't read how you arranged the rail.",
      arrangeSaveError: "Ersa couldn't save that change to the rail.",
      // Nothing the arrange menu's Show lets through (FLOW-ORGANIZE-SESSIONS, Nothing to show).
      noActiveEmpty: "No active sessions.",
      noArchivedEmpty: "No archived sessions.",
      noSessionsEmpty: "No sessions.",
      // Names the tree when it fails to draw: "The projects and sessions could not be shown:".
      boundaryName: "The projects and sessions",
    },
    // The Pinned group's heading and the date groups' (docs/design/RAIL.md, A group heading).
    group: {
      pinnedHeading: "Pinned",
      todayHeading: "Today",
      yesterdayHeading: "Yesterday",
      previous7DaysHeading: "Previous 7 days",
      olderHeading: "Older",
    },
    // A project heading (docs/design/RAIL.md, A project heading) and its More menu.
    heading: {
      newSessionLabel: "New session in {project}",
      // A collapsed heading: how many sessions it holds.
      sessionCountStatus: { one: "{count} session", other: "{count} sessions" },
      moreLabel: "More for {project}",
      // In the More menu. Trusting asks first, so it ends in an ellipsis; withdrawing trust does not ask.
      trustButton: "Trust…",
      distrustButton: "Don't trust",
      savingProgress: "Saving…",
      trustError: "Ersa can't change this project's trust.",
      workspacesError: "Ersa can't read this project's workspaces.",
      // Beneath a project whose trust was withdrawn while sessions that started trusted still run.
      keepTrust: "Running sessions keep their current settings until they are stopped.",
    },
    foot: {
      attachedLabel: "Attached to {label}",
      reconnectingError: "Ersa can't reach your sessions. It's trying again.",
      unreachableError: "Ersa can't reach your sessions.",
      retryButton: "Retry",
      // Heard, not shown: assistive technology is told when Ersa reaches the sessions again after a problem.
      reconnectedStatus: "Reconnected",
      // The reason beneath the foot's line when the handshake can't be read.
      handshakeError: "Ersa can't read the connection's handshake. This window may need updating.",
      // Names the foot when it fails to draw: "The connection status could not be shown:".
      boundaryName: "The connection status",
    },
    // A session's tile (docs/design/RAIL.md, A tile): line two's phrase, and the names of its marks.
    tile: {
      untitledLabel: "Untitled session",
      // The tile's More button and its menu (docs/design/RAIL.md, A tile: More).
      moreLabel: "More for {title}",
      pinButton: "Pin",
      unpinButton: "Unpin",
      // Pin or Unpin while Core stores the last choice.
      savingProgress: "Saving…",
      // Followed by Core's reason, beneath the tile.
      pinError: "Ersa can't pin this session.",
      unpinError: "Ersa can't unpin this session.",
      renameButton: "Rename",
      // The key that does the same as Rename, at the item's trailing edge.
      renameKey: "F2",
      // A new session from this one's conversation (FLOW-FORK-SESSION), and why it can't be chosen now.
      forkButton: "Fork",
      forkBusyReason: "Available once the reply finishes",
      forkingProgress: "Forking…",
      // Beneath the tile, before Core's reason, when Core refuses or the fork can't be made.
      forkError: "Ersa can't fork this session.",
      // Why an item that goes through Core, such as Rename, Pin, Archive or Delete, is unavailable.
      offlineReason: "Available once Ersa reaches your sessions",
      // In the tile's menu, apart in its own group. It asks first, so it ends in an ellipsis.
      deleteButton: "Delete…",
      // Archive or Unarchive, by whether the session is archived (FLOW-ARCHIVE-DELETE-SESSION).
      archiveButton: "Archive",
      unarchiveButton: "Unarchive",
      // Beneath the tile, before Core's reason, when Core refuses.
      archiveError: "Ersa can't archive this session.",
      unarchiveError: "Ersa can't unarchive this session.",
      // The field the title becomes while it is renamed on the tile.
      titleLabel: "Session title",
      // Names a tile when it fails to draw: "This session could not be shown:".
      boundaryName: "This session",
      startingStatus: "Starting",
      // Heard, not shown: the working mark's name, since line two has no phrase for it.
      workingStatus: "Working",
      disconnectedStatus: "Disconnected",
      errorStatus: "Error",
      endedStatus: "Ended",
      unknownStateStatus: "Unknown state",
      // A session Ersa is deleting, and one whose deletion failed (LAY-SHELL, Being deleted).
      deletingStatus: "Deleting",
      deleteFailedStatus: "Couldn't delete",
      // Heard, not shown: the archive mark's name, on an archived session's tile.
      archivedStatus: "Archived",
      // What a session that needs the user wants, in the tone of its chevron.
      permissionWant: "Needs permission",
      questionWant: "Has a question",
      needsYouWant: "Needs you",
      needsYouCountWant: "Needs you · {count}",
    },
    // The delete question (docs/design/RAIL.md, The delete question; FLOW-ARCHIVE-DELETE-SESSION, step 4).
    delete: {
      // Beneath Delete… in the tile's menu, while the session is starting, working or waiting on the user.
      busyReason: "Stop the session to delete it",
      // Beneath Delete… while Ersa is already deleting the session.
      deletingReason: "Ersa is deleting this session",
      title: "Delete {title}?",
      // Always said: the conversation goes from the backend's own tools too ("Claude Code").
      conversationDetail: "Its conversation is deleted for good, in {backend} too.",
      folderAndBranchDetail: "Its folder and its branch {branch} are deleted with it.",
      folderDetail: "Its folder is deleted with it.",
      // A branch that holds commits no other branch or remote holds is kept; the next line says why.
      folderBranchKeptDetail: "Its folder is deleted with it. Its branch {branch} is kept.",
      branchKeptDetail: "The branch holds commits that exist nowhere else.",
      // The project's own folder, or a folder another session also uses.
      filesStayDetail: "Its files stay in {path}.",
      // What goes with the folder and exists nowhere else, joined into one list: "3 uncommitted or untracked files and
      // 12 ignored files will be lost."
      uncommittedPart: { one: "{count} uncommitted or untracked file", other: "{count} uncommitted or untracked files" },
      ignoredPart: { one: "{count} ignored file", other: "{count} ignored files" },
      strandedPart: { one: "{count} commit no branch holds", other: "{count} commits no branch holds" },
      lossDetail: "{parts} will be lost.",
      // When git has lost the workspace's record (FLOW-ARCHIVE-DELETE-SESSION-R10), in place of a count of them.
      unrecordedDetail: "Commits made on a detached checkout can't be counted and may be lost with the folder.",
      // Asked again because what would be lost changed since the question was first asked.
      changedDetail: "What would be lost changed since you were asked.",
      readingProgress: "Checking what would be lost…",
      cancelButton: "Cancel",
      deleteButton: "Delete",
      // When files or commits would be lost with the folder: the button says so.
      deleteAndLoseButton: "Delete and lose them",
      deletingProgress: "Deleting…",
      tryAgainButton: "Try again",
      failedError: "Ersa couldn't delete this session.",
      // Core refused because another request holds the session's workspace, such as a delete from another window.
      busyError: "Something else is changing this session or its folder. Try again in a moment.",
      // The session's workspace hasn't been read yet, so the question can't say what goes with it.
      workspaceUnknownError: "Ersa can't tell yet where this session's files are. Try again in a moment.",
    },
    // The shelf (docs/design/RAIL.md, The shelf; FLOW-APPROVE-PERMISSION): a session's request, pulled out of its tile.
    shelf: {
      // Heard, not shown: the shelf's name, and its chevron's.
      nameLabel: "Request from {title}",
      permissionHeading: "Permission request",
      questionHeading: "Question",
      // The shown request's place among the session's requests, at the heading line's trailing end.
      placeLabel: "{n} of {m}",
      // Heard, not shown, as the shelf moves to the next request: "Permission request, 2 of 3: Run the tests".
      nextLabel: "{kind}, {n} of {m}: {asked}",
      // Heard as the shelf moves to a request it can't show: "2 of 3: This session is waiting on…".
      nextUnknownLabel: "{n} of {m}: {note}",
      pathLabel: "Path:",
      loadingProgress: "Loading…",
      readError: "Ersa can't read this session's requests.",
      retryButton: "Retry",
      unknownNote: "This session is waiting on a request Ersa can't show here.",
      // Names the shelf's contents when they fail to draw: "This request could not be shown:".
      boundaryName: "This request",
      // A permission request's answers: the transcript's permission card offers the same ones (R27).
      denyButton: "Deny",
      allowButton: "Allow",
      // A question's answers (R16, R28): the transcript's question card offers the same ones (R17).
      declineButton: "Decline",
      answerButton: "Answer",
      // The text field for the user's own words, beneath a question's options; named by its placeholder.
      ownWordsPlaceholder: "Answer in your own words",
      // Followed by Core's reason, beneath.
      answerRefusedError: "Your answer wasn't accepted.",
      reconnectingNote: "Reconnecting to Core…",
    },
  },
  transcript: {
    // A question's card: its name, its title once answered or declined, and its state once the session ended without
    // an answer.
    question: {
      name: "Question",
      answeredTitle: "Question, answered",
      declinedTitle: "Question, declined",
      unansweredStatus: "Not answered",
    },
    attachment: {
      listLabel: "Attachments",
      imageLabel: "Attached image",
    },
  },
  // The composer (docs/design/COMPOSER.md): the box's parts, and the status line's note while backends are detected
  // (FLOW-NEW-SESSION-R23). That Ersa can't reach the sessions is said in the rail foot's words.
  composer: {
    name: "Composer",
    // In place of the box for an archived session, which opens read-only (FLOW-ARCHIVE-DELETE-SESSION, step 2).
    archived: {
      note: "This session is archived. Resume it to send a message.",
      resumeButton: "Resume",
      resumingProgress: "Resuming…",
      // Before Core's reason, when Core refuses the resume.
      resumeError: "Ersa can't resume this session.",
    },
    messageLabel: "Message",
    writePlaceholder: "Write to the agent",
    attachButton: "Attach",
    sendButton: "Send",
    stopButton: "Stop",
    detectingProgress: "Detecting the backends on this host…",
    // The host has no backend to run: none installed, on the status line and as why a start is refused; or one
    // that failed detection, by the label backend.list gives it.
    noBackendError: "No backend is available on this host. Install a backend on it to start a session.",
    backendFailedError: "{label} isn't working on this host. Check that it's installed and signed in, then open New session again.",
    // The status line while a message is on its way (FLOW-SEND-MESSAGE-R16), and when Ersa can't tell whether it
    // reached the agent (R15), which never says it failed.
    copyingProgress: "Copying attachments, {percent} percent done…",
    startingProgress: "Starting the backend…",
    unknownNote: "Ersa can't tell whether your message reached the agent. If it did, the transcript shows it.",
    // The attach menu (docs/design/COMPOSER.md, The attach menu; FLOW-SEND-MESSAGE-R07).
    attach: {
      menuLabel: "Attach",
      filesButton: "Add files…",
      imagesButton: "Add images…",
      folderButton: "Add a folder…",
      pasteNote: "You can also paste attachments, or drop them on the box.",
      imagesFilterLabel: "Images",
    },
    // An attachment above the words (docs/design/COMPOSER.md, Attachments; FLOW-SEND-MESSAGE-R09, R17).
    attachment: {
      listLabel: "Attachments",
      bytesDetail: { one: "{count} byte", other: "{count} bytes" },
      linesDetail: { one: "{count} line", other: "{count} lines" },
      folderDetail: "Folder",
      copiedNote: "Will be copied into the workspace's .ersa folder",
      kilobytesDetail: "{size} KB",
      megabytesDetail: "{size} MB",
      gigabytesDetail: "{size} GB",
      removeLabel: "Remove {name}",
      pastedName: "Pasted image",
      pastedTooLargeError: "This image is larger than 10 MB, the most Ersa sends in one message.",
      pastedUnreadableError: "Ersa couldn't read this image.",
    },
  },
  newSession: {},
  sessionControls: {
    // The host chip's name for the host Ersa runs on.
    localHostLabel: "Local",
    // A host in the host chip's list that has no project to start a session in.
    noProjectsStatus: "No projects",
    // A project in the project chip's list (docs/design/COMPOSER.md, The project list): its kind, a git
    // repository or any other folder, and the action that ends the list.
    project: {
      withGitKindLabel: "Git",
      withoutGitKindLabel: "Folder",
      addProjectButton: "Add project…",
      // The keys that choose Add project from anywhere on the new-session screen.
      addProjectShortcutLabel: "Ctrl+O",
      // The project chip while the local host has no project: choosing it, or sending, opens the folder dialog.
      chooseFolderButton: "Choose a folder",
    },
    // The workspace chip (docs/design/COMPOSER.md, The workspace list): a new workspace or the project's own folder,
    // each with what it means, and for a git project the base branch the new workspace starts from
    // (FLOW-NEW-SESSION-R21, R28, R29).
    workspace: {
      newWorkspaceLabel: "New workspace",
      isolatedStatus: "Isolated",
      projectFolderLabel: "Project folder",
      sharedStatus: "Shared",
      // What the chip reads with a new workspace and its base branch chosen.
      newFromLabel: "New workspace from {branch}",
      baseBranchHeading: "Base branch",
      searchBranchesPlaceholder: "Search branches",
      // The status of the repository's default branch in the list.
      defaultBranchStatus: "Default",
      noBranchMatchesEmpty: "No branch matches.",
      readingBranchesProgress: "Reading the branches…",
      // The project's branches could not be listed; the new workspace starts at the repository's default branch.
      branchesFailedError: "Ersa couldn't read this project's branches. The new workspace will start at the default branch.",
    },
    // The context ring's name, in the session controls and on a tile.
    contextRing: {
      noneLabel: "Context: none used yet",
      usedLabel: "Context: {percent} percent used",
    },
  },
  quit: {},
  trust: {
    // A project the user has not trusted, on its heading in the rail and in the project chip's list.
    notTrustedStatus: "Not trusted",
    // The trust question (ADR 0028), asked when a folder becomes a project and when the user trusts one later. It reads
    // like the Claude Code CLI's folder-trust prompt: the safety check and what agents can do there
    // (FLOW-NEW-SESSION-R24), answered by Cancel or Trust (R25).
    question: {
      title: "Trust {name}?",
      safetyDetail:
        "Is this a project you created or one you trust, like your own code, a well-known open source project, or work from your team? If not, review what's in this folder first.",
      agentsDetail: "Agents will be able to read, edit, and run files here.",
      trustButton: "Trust",
      cancelButton: "Cancel",
    },
  },
  errors: {},
} as const satisfies Section;
