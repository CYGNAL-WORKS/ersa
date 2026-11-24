//! Putting a put-away workspace back before a session there starts again
//! (FLOW-CLEAN-UP, Putting away and putting back): by `session.resume`, and
//! by a send to a session that is not running.
//!
//! The folder is made again at the same path, as `workspace.create` makes
//! one, with what it had checked out when it was put away: the branch at its
//! tip, or the commit. A branch that is gone, or checked out in another
//! worktree, refuses the resume with the reason, and the workspace stays put
//! away (FLOW-CLEAN-UP-R04).
//!
//! The work runs as a task of its own, so a send that is stopped, or a
//! request whose connection goes, answers without leaving a worktree half
//! made: git finishes, and the workspace is either back or still put away.

use std::time::Instant;

use super::*;
use crate::admission::{self, end_held};
use crate::workspace::{self, Checkout, FolderTries, RemovalChecks, VcsError, Worktrees};

/// Where a session's workspace comes back, and how its git work runs: what
/// putting it back takes that the session manager does not keep. Looked up
/// with the rest of a [`Resumption`].
#[derive(Clone)]
pub struct PutBack {
    pub project_id: String,
    /// The project's repository, as a path on its host.
    pub repo: String,
    pub workspace_id: String,
    /// Where the folder comes back: the path it had.
    pub path: String,
    /// How long the put-back waits for a put-away of the workspace to end
    /// and then for its first turn at the project's worktrees, together,
    /// and for each later turn (PROTO-CORE-API-R124, R132).
    pub turn_wait: Duration,
    /// How tries at what a put-away left at the path run; `held` is
    /// replaced by the put-back's own hold.
    pub folder_tries: FolderTries,
}

/// The project's turns, each waited for at most `wait`.
struct Turns {
    admission: Arc<Admission>,
    project: String,
    wait: Duration,
}

#[ersa_adapter::async_trait]
impl Worktrees for Turns {
    async fn turn(&self) -> Result<Box<dyn Send>, VcsError> {
        workspace::project_turn(&self.admission, &self.project, self.wait).await
    }
}

impl SessionManager {
    /// Puts back `handle`'s workspace when it is put away, before anything
    /// looks at its folder or starts the backend (FLOW-CLEAN-UP-R03).
    /// `there_is` runs once there is something to put back or to wait for,
    /// so a send can say so (FLOW-CLEAN-UP-R05). With `canceled`, the
    /// caller answers `send_stopped` as soon as it rises, the put-back going
    /// on to its end by itself.
    pub(super) async fn put_back(
        &self,
        handle: &Handle,
        how: &Resumption,
        there_is: impl FnOnce() + Send + 'static,
        canceled: Option<&mut watch::Receiver<bool>>,
    ) -> Result<(), SessionError> {
        let work = tokio::spawn(put_back(
            Arc::clone(&self.admission),
            self.store.clone(),
            Arc::clone(&how.transport),
            how.put_back.clone(),
            handle.id.clone(),
            there_is,
        ));
        let answer = match canceled {
            None => work.await,
            Some(canceled) => tokio::select! {
                answer = work => answer,
                Ok(_) = canceled.wait_for(|c| *c) => return Err(send::stopped()),
            },
        };
        answer.unwrap_or_else(|e| {
            Err(SessionError::Internal(format!(
                "putting back panicked: {e}"
            )))
        })
    }
}

/// The put-back itself, in steps:
///
/// 1. Admission lets it begin when the workspace is put away, and holds it
///    while a put-away of it, or another session's put-back, is under way
///    (PROTO-CORE-API-R132). A workspace with its folder has nothing to put
///    back.
/// 2. The project's turn, within what is left of the same wait
///    (PROTO-CORE-API-R124).
/// 3. The stored checkout must still exist, and a branch must not be
///    checked out in another worktree (FLOW-CLEAN-UP-R04): refused with the
///    reason, nothing changed.
/// 4. What a put-away left at the path goes: git's record of the worktree
///    and any files, every branch kept.
/// 5. The worktree is made again, and `ready` stored, then said.
///
/// Every way out before step 5's store leaves the workspace put away, as
/// the store has had it throughout.
async fn put_back(
    admission: Arc<Admission>,
    store: Store,
    transport: Arc<dyn Transport>,
    spec: PutBack,
    session: String,
    there_is: impl FnOnce(),
) -> Result<(), SessionError> {
    let id = spec.workspace_id.clone();
    let began = Instant::now();
    // (1)
    let putting_back = match admission.put_back(&id, spec.turn_wait, there_is).await {
        Ok(None) => return Ok(()),
        Ok(Some(putting_back)) => putting_back,
        Err(Refusal::GaveUp) => {
            return Err(SessionError::Vcs(VcsError::Failed {
                message: format!(
                    "the session's folder was still being put away after {} seconds; try again",
                    spec.turn_wait.as_secs()
                ),
                stderr: String::new(),
            }));
        }
        Err(refusal) => return Err(refused_session(refusal, &id, &session)),
    };
    let refused = || admission::Report::PutBackRefused {
        workspace: id.clone(),
    };
    // (2) The rest of the wait that began with step 1.
    let left = spec.turn_wait.saturating_sub(began.elapsed());
    let turn = match workspace::project_turn(&admission, &spec.project_id, left).await {
        Ok(turn) => turn,
        Err(e) => {
            putting_back.end(refused());
            return Err(SessionError::Vcs(e));
        }
    };
    // (3) Read in the turn, so nothing changes the worktrees meanwhile.
    let checked = check_checkout(&store, transport.as_ref(), &spec).await;
    let checkout = match checked {
        Ok(checkout) => checkout,
        Err(e) => {
            putting_back.end(refused());
            return Err(e);
        }
    };

    // Shared with each try at the folder, which keeps it until its thread
    // returns: a try given up on that is still stuck holds the workspace,
    // so nothing else removes it or puts it back beside that try.
    let putting_back = Arc::new(putting_back);
    let tries = FolderTries {
        held: Arc::clone(&putting_back) as _,
        ..spec.folder_tries.clone()
    };
    let turns = workspace::HeldFirst::new(
        turn,
        Turns {
            admission: Arc::clone(&admission),
            project: spec.project_id.clone(),
            wait: spec.turn_wait,
        },
    );
    let made = make_again(transport.as_ref(), &spec, &checkout, tries, &turns).await;
    if let Err(e) = made {
        tracing::warn!(workspace = %id, error = %e, "the workspace was not put back");
        end_held(putting_back, refused());
        return Err(SessionError::Vcs(e));
    }
    // (5) Stored, then said. Not stored, it stays put away as the store
    // has it, and the next put-back takes the folder just made as what was
    // left at the path.
    if let Err(e) = store.set_workspace_ready(id.clone()).await {
        end_held(putting_back, refused());
        return Err(e.into());
    }
    end_held(
        putting_back,
        admission::Report::WorkspacePutBack {
            workspace: id.clone(),
        },
    );
    tracing::info!(workspace = %id, path = %spec.path, "the workspace was put back");
    Ok(())
}

/// What the workspace had checked out when it was put away, refused with
/// `branch_gone` when it no longer exists and `branch_in_use` when another
/// worktree has the branch (FLOW-CLEAN-UP-R04).
async fn check_checkout(
    store: &Store,
    transport: &dyn Transport,
    spec: &PutBack,
) -> Result<Checkout, SessionError> {
    let stored = store.put_away_checkout(spec.workspace_id.clone()).await?;
    let checkout = stored.ok_or_else(|| {
        SessionError::Internal(format!(
            "workspace {} is put away with nothing stored of its checkout",
            spec.workspace_id
        ))
    })?;
    let named = match &checkout {
        Checkout::Branch(branch) => format!("the branch {branch}"),
        Checkout::Commit(commit) => format!("the commit {commit}"),
    };
    let exists = workspace::git_checkout_exists(transport, &spec.repo, &checkout).await;
    if !exists.map_err(SessionError::Vcs)? {
        return Err(SessionError::refused(
            ErrorReason::BranchGone,
            format!(
                "{named} that the session's folder had checked out no longer exists, so the folder can't be put back"
            ),
        ));
    }
    if let Checkout::Branch(branch) = &checkout {
        let elsewhere =
            workspace::git_branch_checked_out_elsewhere(transport, &spec.repo, branch, &spec.path)
                .await;
        if elsewhere.map_err(SessionError::Vcs)? {
            return Err(SessionError::refused(
                ErrorReason::BranchInUse,
                format!(
                    "{named} is checked out in another folder, so the session's folder can't be put back"
                ),
            ));
        }
    }
    Ok(checkout)
}

/// Removes what a put-away left at the path (step 4), then makes the
/// worktree again (step 5).
async fn make_again(
    transport: &dyn Transport,
    spec: &PutBack,
    checkout: &Checkout,
    tries: FolderTries,
    turns: &dyn Worktrees,
) -> Result<(), VcsError> {
    let (repo, path) = (spec.repo.as_str(), spec.path.as_str());
    // (4) Whatever is there is the workspace's own, left by a put-away that
    // stopped part way, so it goes unchecked (FLOW-CLEAN-UP-R03), every
    // branch kept. An add of an earlier put-back that was cut off leaves
    // its lock, which goes too.
    if !workspace::git_worktree_gone(transport, repo, path).await? {
        let checks = RemovalChecks {
            force: true,
            after_failed_add: true,
        };
        workspace::git_worktree_delete(transport, repo, path, "", checks, tries, turns).await?;
    }
    // (5) The turn held since step 2 when nothing was removed, else a new
    // one.
    let _turn = turns.turn().await?;
    let made = workspace::git_worktree_checkout(transport, repo, path, checkout).await;
    if let Err(e) = made {
        // When git timed out or its output was lost, a half-made worktree
        // may be there: discarded as `workspace.create` discards one, and
        // whatever is left goes with the next put-back.
        if let VcsError::Incomplete(_) = e
            && let Err(discard) = workspace::git_worktree_discard(transport, repo, path).await
        {
            tracing::warn!(path, error = %discard, "could not clean up a worktree half put back");
        }
        return Err(e);
    }
    Ok(())
}
