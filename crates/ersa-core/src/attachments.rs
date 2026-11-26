//! Attachments on `session.send`, and `attachment.inspect`
//! (PROTO-CORE-API, Turns and attachments).
//!
//! Everything here is blocking file system work. Core runs it on a blocking
//! thread, one job per message ([`prepare`]) or per inspect ([`inspect`]),
//! and watches it from the async side through a [`Work`]: the job reports
//! each step it finishes and checks between steps whether it should stop,
//! so a send can be canceled or given up on without waiting for a file
//! system that no longer answers. A job told to stop removes what it copied
//! itself, once its current step returns.
//!
//! The paths a backend gets for files and folders are relative to the
//! session's workspace. One inside the workspace (judged by where a link
//! points) goes as it is; one outside is copied first into the session's
//! scratch folder, `.ersa/attachments/<session id>/<n>/<name>`.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ersa_adapter::{ImageLimits, OutgoingAttachment};
use ersa_protocol::core_api::{Attachment, AttachmentKind, Inspected};

/// The most bytes of inline images one message may carry together, which
/// leaves room under the line cap for the words.
pub const INLINE_TOTAL: u64 = 10 * 1024 * 1024;
/// The most bytes one copy may hold.
pub const COPY_BYTES: u64 = 100 * 1024 * 1024;
/// The most entries (files and folders) one copy may hold.
pub const COPY_ENTRIES: u64 = 10_000;
/// Core counts lines only in a text file of at most this many bytes.
pub const LINE_COUNT_MAX: u64 = 10 * 1024 * 1024;

/// How much of a file is read to tell an image by its first bytes.
const SNIFF_BYTES: usize = 16;
/// How much a copy reads and writes at a time, between checks.
const CHUNK: usize = 64 * 1024;

/// The scratch folder's parts under the workspace's root.
const ERSA: &str = ".ersa";
const ATTACHMENTS: &str = "attachments";
const PARTIAL: &str = ".partial";

/// What a job and the task watching it share.
#[derive(Default)]
pub struct Work {
    /// Raised by the watcher: canceled, or a time limit reached.
    stop: AtomicBool,
    /// Counts the job's finished steps, so the watcher can tell one step
    /// that has taken too long.
    steps: AtomicU64,
    /// The attachment the job is on, for a refusal that names it.
    at: AtomicUsize,
    /// Raised once checking is done and copying begins.
    copying: AtomicBool,
    copied: AtomicU64,
    total: AtomicU64,
    /// A pause after each chunk a copy writes: zero, except for tests that
    /// need a copy to take a while ([`crate::CoreConfig::copy_pause`]).
    chunk_pause: Duration,
    /// For tests: stop at this step (counting from 1), as a cancel arriving
    /// then would. 0 never stops.
    #[cfg(test)]
    stop_at: AtomicU64,
}

impl Work {
    /// A job that pauses `chunk_pause` after each chunk it copies.
    pub fn pausing(chunk_pause: Duration) -> Self {
        Work {
            chunk_pause,
            ..Work::default()
        }
    }

    /// Tells the job to stop at its next step.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn steps(&self) -> u64 {
        self.steps.load(Ordering::SeqCst)
    }

    /// The index of the attachment the job is on.
    pub fn at(&self) -> usize {
        self.at.load(Ordering::SeqCst)
    }

    /// `Some((copied, total))` once copying has begun.
    pub fn copying(&self) -> Option<(u64, u64)> {
        self.copying.load(Ordering::SeqCst).then(|| {
            (
                self.copied.load(Ordering::SeqCst),
                self.total.load(Ordering::SeqCst),
            )
        })
    }

    /// One step done; fails when the job has been told to stop.
    fn step(&self) -> Result<(), Fail> {
        let _step = self.steps.fetch_add(1, Ordering::SeqCst) + 1;
        #[cfg(test)]
        if _step == self.stop_at.load(Ordering::SeqCst) {
            self.stop();
        }
        if self.stop.load(Ordering::SeqCst) {
            Err(Fail::Stopped)
        } else {
            Ok(())
        }
    }
}

/// Why a message's attachments can't be sent.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The attachment at `index` can't be sent, for `reason`.
    At { index: usize, reason: String },
    /// The job was told to stop.
    Stopped,
}

/// A failure inside the job, before it knows which attachment to name.
enum Fail {
    Stopped,
    Reason(String),
}

impl Fail {
    /// Names the attachment at `index` as the one that failed.
    fn at(index: usize) -> impl Fn(Fail) -> Refused {
        move |fail| match fail {
            Fail::Stopped => Refused::Stopped,
            Fail::Reason(reason) => Refused::At { index, reason },
        }
    }
}

impl From<io::Error> for Fail {
    fn from(e: io::Error) -> Self {
        Fail::Reason(e.to_string())
    }
}

/// A message's attachments, ready to hand to the adapter.
#[derive(Debug)]
pub struct Prepared {
    pub attachments: Vec<OutgoingAttachment>,
    /// The copies moved into the scratch folder for this message, removed
    /// again by [`Prepared::discard`] when the message is not sent.
    copies: Vec<PathBuf>,
}

impl Prepared {
    /// Removes the copies this message made, for a message that was not
    /// sent. Never follows a link.
    pub fn discard(&self) {
        for copy in &self.copies {
            if let Err(e) = remove_entry(copy) {
                tracing::warn!(path = %copy.display(), error = %e, "removing a copy failed");
            }
        }
    }
}

/// Where the attachments of one message go.
pub struct Target<'a> {
    /// The session's workspace on its host.
    pub workspace: &'a Path,
    pub session_id: &'a str,
    pub images: ImageLimits,
}

/// Checks every attachment, then copies the ones from outside the
/// workspace into the session's scratch folder (PROTO-CORE-API-R21, R23, R110).
/// On any failure nothing this message copied remains.
///
/// The caller holds the session's scratch folder for the whole job, so no
/// two jobs of one session copy at once.
pub fn prepare(
    target: &Target,
    attachments: &[Attachment],
    work: &Work,
) -> Result<Prepared, Refused> {
    let workspace = fs::canonicalize(target.workspace).map_err(|e| Refused::At {
        index: 0,
        reason: format!("the session's workspace can't be read: {e}"),
    })?;
    let mut checked = Vec::with_capacity(attachments.len());
    let mut inline = 0u64;
    for (index, attachment) in attachments.iter().enumerate() {
        work.at.store(index, Ordering::SeqCst);
        let one = check(attachment, &workspace, target.images, &mut inline, work)
            .map_err(Fail::at(index))?;
        checked.push(one);
        work.step().map_err(Fail::at(index))?;
    }

    let total: u64 = checked
        .iter()
        .map(|c| match c {
            Checked::Copy { bytes, .. } => *bytes,
            _ => 0,
        })
        .sum();
    let first_copy = checked
        .iter()
        .position(|c| matches!(c, Checked::Copy { .. }));
    let mut copies = Vec::new();
    let result = match first_copy {
        None => Ok(()),
        Some(first) => {
            work.total.store(total, Ordering::SeqCst);
            work.copying.store(true, Ordering::SeqCst);
            copy_all(target, &workspace, &mut checked, first, &mut copies, work)
        }
    };
    if let Err(refused) = result {
        // Nothing this message copied stays (PROTO-CORE-API-R23).
        Prepared {
            attachments: Vec::new(),
            copies,
        }
        .discard();
        return Err(refused);
    }
    let attachments = checked
        .into_iter()
        .map(|c| match c {
            Checked::Ready(a) => a,
            Checked::Copy { .. } => unreachable!("every copy is made or the message refused"),
        })
        .collect();
    Ok(Prepared {
        attachments,
        copies,
    })
}

/// One attachment after checking.
enum Checked {
    Ready(OutgoingAttachment),
    /// From outside the workspace: to copy from `source` (a link's target)
    /// under `name` (the link's own).
    Copy {
        folder: bool,
        source: PathBuf,
        name: String,
        bytes: u64,
    },
}

fn check(
    attachment: &Attachment,
    workspace: &Path,
    images: ImageLimits,
    inline: &mut u64,
    work: &Work,
) -> Result<Checked, Fail> {
    let reason = |s: String| Fail::Reason(s);
    // Said first, for a pasted image and one by path alike: otherwise the
    // size check below would name a limit of 0 bytes.
    if attachment.kind == AttachmentKind::Image && images.media_types.is_empty() {
        return Err(reason("this session's backend doesn't take images".into()));
    }
    if attachment.kind == AttachmentKind::Image && attachment.path.is_none() {
        // A pasted image: its bytes are in the message.
        let (Some(media_type), Some(data)) = (&attachment.media_type, &attachment.data) else {
            return Err(reason(
                "a pasted image needs its media_type and data".into(),
            ));
        };
        let bytes = BASE64
            .decode(data)
            .map_err(|e| reason(format!("the image's data is not base64: {e}")))?;
        let sniffed = sniff_image(&bytes, images.media_types);
        if sniffed != Some(media_type.as_str()) {
            return Err(reason(format!(
                "the image is not a {media_type} this session's backend takes"
            )));
        }
        return image(media_type, bytes, images, inline);
    }
    let Some(path) = attachment.path.as_deref() else {
        return Err(reason("the attachment has no path".into()));
    };
    let given = named_path(path).map_err(reason)?;
    // A link is judged by its target: `metadata` follows it.
    let meta = match fs::metadata(given) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(reason(format!("nothing is at {path}")));
        }
        Err(e) => return Err(reason(format!("{path} can't be read: {e}"))),
    };
    work.step()?;
    match attachment.kind {
        AttachmentKind::Image => {
            if !meta.is_file() {
                return Err(reason(format!("{path} is not a file")));
            }
            if meta.len() > images.max_bytes {
                return Err(reason(format!(
                    "the image is larger than this session's backend takes ({} bytes at most)",
                    images.max_bytes
                )));
            }
            let mut bytes = Vec::with_capacity(meta.len() as usize);
            open_file(given)?
                .take(images.max_bytes + 1)
                .read_to_end(&mut bytes)?;
            work.step()?;
            let Some(media_type) = sniff_image(&bytes, images.media_types) else {
                return Err(reason(format!(
                    "{path} is not an image this session's backend takes"
                )));
            };
            image(media_type, bytes, images, inline)
        }
        AttachmentKind::File if !meta.is_file() => Err(reason(format!("{path} is not a file"))),
        AttachmentKind::Folder if !meta.is_dir() => Err(reason(format!("{path} is not a folder"))),
        AttachmentKind::File | AttachmentKind::Folder => {
            let folder = attachment.kind == AttachmentKind::Folder;
            let resolved = fs::canonicalize(given)?;
            work.step()?;
            if let Some(inside) = locate(&resolved, workspace, folder, path).map_err(reason)? {
                let path = relative(inside);
                return Ok(Checked::Ready(if folder {
                    OutgoingAttachment::Folder { path }
                } else {
                    OutgoingAttachment::File { path }
                }));
            }
            // A link keeps its own name.
            let name = given
                .file_name()
                .or_else(|| resolved.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| reason(format!("{path} has no name to copy under")))?;
            let bytes = if folder {
                measure(&resolved, work)?
            } else if meta.len() > COPY_BYTES {
                return Err(too_big());
            } else {
                meta.len()
            };
            Ok(Checked::Copy {
                folder,
                source: resolved,
                name,
                bytes,
            })
        }
    }
}

fn image(
    media_type: &str,
    bytes: Vec<u8>,
    images: ImageLimits,
    inline: &mut u64,
) -> Result<Checked, Fail> {
    let size = bytes.len() as u64;
    if !images.allows(media_type, size) {
        return Err(Fail::Reason(format!(
            "this session's backend does not take a {media_type} of {size} bytes"
        )));
    }
    *inline += size;
    if *inline > INLINE_TOTAL {
        return Err(Fail::Reason(format!(
            "the images together are larger than {INLINE_TOTAL} bytes"
        )));
    }
    Ok(Checked::Ready(OutgoingAttachment::Image {
        media_type: media_type.to_string(),
        data: BASE64.encode(bytes),
    }))
}

fn too_big() -> Fail {
    Fail::Reason(format!(
        "the copy would be larger than {COPY_BYTES} bytes or hold more than {COPY_ENTRIES} entries"
    ))
}

/// A path inside the workspace as the backend gets it: relative, with `/`
/// between its parts whatever the host.
fn relative(inside: &Path) -> String {
    let parts: Vec<String> = inside
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// The bytes a copy of folder `root` would hold, refusing one past the
/// limits. Links are left out, as the copy leaves them out.
fn measure(root: &Path, work: &Work) -> Result<u64, Fail> {
    // The folder itself is an entry of the copy, as the copy counts it.
    let (mut bytes, mut entries) = (0u64, 1u64);
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            work.step()?;
            if kind.is_symlink() {
                continue;
            }
            entries += 1;
            if kind.is_dir() {
                folders.push(entry.path());
            } else if kind.is_file() {
                bytes += entry.metadata()?.len();
            }
            if bytes > COPY_BYTES || entries > COPY_ENTRIES {
                return Err(too_big());
            }
        }
    }
    Ok(bytes)
}

/// Makes every copy, in order, each in the session's `.partial/` and moved
/// into place when whole. `copies` gathers what was moved, for the caller
/// to remove on failure.
fn copy_all(
    target: &Target,
    workspace: &Path,
    checked: &mut [Checked],
    first: usize,
    copies: &mut Vec<PathBuf>,
    work: &Work,
) -> Result<(), Refused> {
    work.at.store(first, Ordering::SeqCst);
    let scratch = scratch_folder(workspace, target.session_id, work).map_err(Fail::at(first))?;
    let partial = scratch.join(PARTIAL);
    let mut n = next_number(&scratch).map_err(Fail::at(first))?;
    for (index, one) in checked.iter_mut().enumerate() {
        let Checked::Copy {
            folder,
            source,
            name,
            ..
        } = one
        else {
            continue;
        };
        work.at.store(index, Ordering::SeqCst);
        let aside = partial.join(n.to_string());
        let made = (|| -> Result<(), Fail> {
            fs::create_dir(&aside)?;
            let mut copy = Copy {
                work,
                bytes: 0,
                entries: 0,
            };
            if *folder {
                copy.folder(source, &aside.join(&*name))
            } else {
                copy.file(source, &aside.join(&*name))
            }
        })();
        if let Err(fail) = made {
            let _ = remove_entry(&aside);
            return Err(Fail::at(index)(fail));
        }
        let placed = scratch.join(n.to_string());
        fs::rename(&aside, &placed)
            .map_err(Fail::from)
            .map_err(Fail::at(index))?;
        copies.push(placed);
        let path = format!("{ERSA}/{ATTACHMENTS}/{}/{n}/{name}", target.session_id);
        *one = Checked::Ready(if *folder {
            OutgoingAttachment::Folder { path }
        } else {
            OutgoingAttachment::File { path }
        });
        n += 1;
        work.step().map_err(Fail::at(index))?;
    }
    Ok(())
}

/// Makes sure `.ersa/attachments/<session id>/.partial/` are real folders
/// inside the workspace, never links, that `.ersa/attachments/` is ignored
/// by version control (and `.ersa/` too, unless the repository ships its own
/// `.ersa/.gitignore`), and that `.partial/` is empty. Returns the session's
/// folder.
fn scratch_folder(workspace: &Path, session_id: &str, work: &Work) -> Result<PathBuf, Fail> {
    let ersa = workspace.join(ERSA);
    real_folder(&ersa)?;
    work.step()?;
    // A repository may ship its own `.ersa/.gitignore`, which may ignore
    // less than everything: it is the user's, so it is only made when absent
    // (PROTO-CORE-API-R111).
    ignore_everything(&ersa.join(".gitignore"), None)?;
    work.step()?;
    let mut folder = ersa;
    for part in [ATTACHMENTS, session_id, PARTIAL] {
        folder = folder.join(part);
        real_folder(&folder)?;
        if part == ATTACHMENTS {
            // Core's own ignore file, in Core's own folder, keeps every copy
            // ignored whatever `.ersa/.gitignore` says: git applies a deeper
            // `.gitignore` over a shallower one. Being Core's, it is read and
            // made again when it says anything else, staged under a name of
            // this session's own.
            let staging = folder.join(format!(".gitignore.{session_id}"));
            ignore_everything(&folder.join(".gitignore"), Some(&staging))?;
        }
        work.step()?;
    }
    // What a crash or a stopped job left aside goes first: the folder is
    // removed whole and made again, rather than listed and emptied, so a
    // link put in its place meanwhile is removed as a link (which
    // `remove_dir_all` does), never followed.
    remove_entry(&folder)?;
    real_folder(&folder)?;
    work.step()?;
    Ok(folder
        .parent()
        .expect("under the session's folder")
        .to_path_buf())
}

/// What an ignore file of Core's holds: every entry beside it ignored.
const IGNORE_ALL: &[u8] = b"*\n";

/// Makes a `.gitignore` at `path` that ignores everything beside it, when
/// none is there; one that is not a plain file (a link, a folder) is
/// refused. One already there is left as it is, unless Core owns it, which
/// the `staging` path Core makes it under says: then one that says anything
/// else is made again.
///
/// Sessions of one workspace share Core's file and nothing orders their
/// jobs, so Core's file is never removed or made in place: it is written
/// whole at `staging`, a name only this session's job uses, and renamed
/// over `path`. A rename replaces what is there (a link as the link itself,
/// never followed) or makes it, so two jobs at once both succeed.
fn ignore_everything(path: &Path, staging: Option<&Path>) -> Result<(), Fail> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_file() => {
            return Err(Fail::Reason(format!(
                "{} is not a plain file in the workspace",
                path.display()
            )));
        }
        Ok(_) => {
            let Some(_) = staging else {
                return Ok(());
            };
            // Read through `open_file`, so a FIFO swapped in meanwhile never
            // holds the job.
            match open_file(path) {
                Ok(file) => {
                    let mut held = Vec::new();
                    file.take(IGNORE_ALL.len() as u64 + 1)
                        .read_to_end(&mut held)?;
                    if held == IGNORE_ALL {
                        return Ok(());
                    }
                }
                // Another session's job may have replaced the file since it
                // was checked: Windows then refuses the open as "Access is
                // denied", the file the name led to being deleted on the way
                // (#606). A file that can't be read can't be shown to hold
                // `*`, so it is made again below, as one holding anything
                // else is.
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let Some(staging) = staging else {
        // The user's file: made only when absent. `create_new` never writes
        // through something that appeared meanwhile, and one another job
        // made meanwhile is left as it is.
        return match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => Ok(file.write_all(IGNORE_ALL)?),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e.into()),
        };
    };
    // What a crash left at `staging` goes first; `create_new` then never
    // writes through something put there meanwhile.
    remove_entry(staging)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging)?
        .write_all(IGNORE_ALL)?;
    fs::rename(staging, path).map_err(|e| {
        let _ = remove_entry(staging);
        Fail::from(e)
    })
}
/// Opens the file at `path` to read, checked again by what was opened: it
/// may have been swapped for something else (a FIFO, a folder) since it was
/// checked, and a FIFO opened the usual way waits for a writer that may
/// never come, holding the session's scratch folder all the while. Its error
/// keeps the kind the system gave, for a caller that tells them apart.
fn open_file(path: &Path) -> io::Result<fs::File> {
    let file = crate::platform::open_without_waiting(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(format!(
            "{} changed after it was checked and is no longer a file",
            path.display()
        )));
    }
    Ok(file)
}

/// Makes `path` a folder when nothing is there, and refuses anything there
/// that is not a real folder (a link, a junction, a file).
fn real_folder(path: &Path) -> Result<(), Fail> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(()),
        Ok(_) => Err(Fail::Reason(format!(
            "{} is not a real folder in the workspace",
            path.display()
        ))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => match fs::create_dir(path) {
            Ok(()) => Ok(()),
            // Made meanwhile: check what is there now.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => real_folder(path),
            Err(e) => Err(e.into()),
        },
        Err(e) => Err(e.into()),
    }
}

/// One more than the highest number already in the session's folder, so a
/// number is never used twice, across restarts too.
fn next_number(scratch: &Path) -> Result<u64, Fail> {
    let mut highest = 0;
    for entry in fs::read_dir(scratch)? {
        if let Some(n) = entry?
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u64>().ok())
        {
            highest = highest.max(n);
        }
    }
    Ok(highest + 1)
}

/// One copy under way, enforcing its limits as it goes, so a file that
/// grows meanwhile still stops it.
struct Copy<'a> {
    work: &'a Work,
    bytes: u64,
    entries: u64,
}

impl Copy<'_> {
    fn count_entry(&mut self) -> Result<(), Fail> {
        self.entries += 1;
        if self.entries > COPY_ENTRIES {
            return Err(too_big());
        }
        Ok(())
    }

    fn file(&mut self, from: &Path, to: &Path) -> Result<(), Fail> {
        self.count_entry()?;
        let mut source = open_file(from)?;
        let mut dest = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(to)?;
        let mut buffer = vec![0u8; CHUNK];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            self.bytes += read as u64;
            if self.bytes > COPY_BYTES {
                return Err(too_big());
            }
            dest.write_all(&buffer[..read])?;
            self.work.copied.fetch_add(read as u64, Ordering::SeqCst);
            self.work.step()?;
            if !self.work.chunk_pause.is_zero() {
                std::thread::sleep(self.work.chunk_pause);
            }
        }
        dest.sync_all()?;
        Ok(())
    }

    /// Copies a folder, leaving out its links (and anything that is neither
    /// a file nor a folder), never following them. Folders wait on a list
    /// of their own rather than in nested calls, so a folder nested
    /// thousands deep never runs the thread out of stack.
    fn folder(&mut self, from: &Path, to: &Path) -> Result<(), Fail> {
        let mut folders = vec![(from.to_path_buf(), to.to_path_buf())];
        while let Some((from, to)) = folders.pop() {
            self.count_entry()?;
            fs::create_dir(&to)?;
            for entry in fs::read_dir(&from)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                self.work.step()?;
                if kind.is_symlink() {
                    continue;
                }
                let dest = to.join(entry.file_name());
                if kind.is_dir() {
                    folders.push((entry.path(), dest));
                } else if kind.is_file() {
                    self.file(&entry.path(), &dest)?;
                }
            }
        }
        Ok(())
    }
}

/// Removes what is at `path`, never following a link: a link is removed
/// itself, never what it points at. Nothing there is not an error.
pub fn remove_entry(path: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let kind = meta.file_type();
    if kind.is_symlink() {
        // On Windows a link to a folder (or a junction) is removed as a
        // folder, and one to a file as a file; try both.
        return fs::remove_file(path).or_else(|_| fs::remove_dir(path));
    }
    if kind.is_dir() {
        // `remove_dir_all` removes links inside as links, never following
        // them.
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Removes the session's scratch folder (PROTO-CORE-API-R121), only when
/// every folder on the way to it is a real one, so a link at `.ersa` never
/// leads the removal elsewhere.
pub fn remove_scratch(workspace: &Path, session_id: &str) -> io::Result<()> {
    let ersa = workspace.join(ERSA);
    let attachments = ersa.join(ATTACHMENTS);
    for folder in [&ersa, &attachments] {
        match fs::symlink_metadata(folder) {
            Ok(meta) if meta.file_type().is_dir() => {}
            // Nothing there, or a link that is not ours to walk into:
            // nothing of this session's to remove.
            _ => return Ok(()),
        }
    }
    remove_entry(&attachments.join(session_id))
}

/// A path a user named, checked before anything touches it: absolute, with
/// no control character, and naming no pipe or device. On Windows a named
/// pipe or a device (`\\.\pipe\x`, `C:\dir\NUL`) reports itself as a file,
/// and asking about it connects to whatever serves it, so those are refused
/// by their names alone. A link is checked by its target too.
fn named_path(path: &str) -> Result<&Path, String> {
    if path.chars().any(char::is_control) {
        return Err("the path holds a control character".into());
    }
    let given = Path::new(path);
    if !given.is_absolute() {
        return Err(format!("{path} is not an absolute path"));
    }
    no_device_through_links(given, 0)?;
    Ok(given)
}

/// How many links deep a path's check follows them.
const LINK_DEPTH: usize = 8;

/// Checks `path`, and where a link anywhere along it leads, for a pipe or
/// a device, following a chain of links up to [`LINK_DEPTH`] deep and
/// refusing a deeper one.
fn no_device_through_links(path: &Path, depth: usize) -> Result<(), String> {
    if depth > LINK_DEPTH {
        return Err(format!("{} leads through too many links", path.display()));
    }
    no_device(path)?;
    for step in path.ancestors() {
        if let Ok(target) = fs::read_link(step) {
            // A relative target is relative to the link's folder.
            let target = match step.parent() {
                Some(folder) if target.is_relative() => folder.join(target),
                _ => target,
            };
            no_device_through_links(&target, depth + 1)?;
        }
    }
    Ok(())
}

fn no_device(path: &Path) -> Result<(), String> {
    if crate::platform::names_device(path) {
        Err(format!("{} is a pipe or a device", path.display()))
    } else {
        Ok(())
    }
}

/// The image type `bytes` start with, among `media_types`.
pub fn sniff_image(bytes: &[u8], media_types: &[&'static str]) -> Option<&'static str> {
    let head = &bytes[..bytes.len().min(SNIFF_BYTES)];
    let kind = if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if head.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        "image/gif"
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return None;
    };
    media_types.iter().copied().find(|&t| t == kind)
}

/// Where a file or folder resolved to `resolved` lies for `workspace`, both
/// canonical: its path inside the workspace, or `None` when it lies outside
/// and sending copies it. A folder that holds the workspace can never be sent.
/// The one placement `session.send` and `attachment.inspect` share
/// (PROTO-CORE-API-R44).
fn locate<'a>(
    resolved: &'a Path,
    workspace: &Path,
    folder: bool,
    path: &str,
) -> Result<Option<&'a Path>, String> {
    if folder && workspace.starts_with(resolved) {
        return Err(format!("{path} holds the session's workspace"));
    }
    Ok(resolved.strip_prefix(workspace).ok())
}

/// Whether sending the file or folder at `path` into `workspace` (canonical,
/// as `prepare` makes it) copies it (PROTO-CORE-API-R44): where its target
/// lies, resolved as a send resolves it, through links, junctions and `..`. It
/// measures nothing; a copy's size is the send's to refuse. Refused with the
/// reason for a folder that holds the workspace. A path that no longer
/// resolves is taken to be copied: the send judges it again.
pub fn copies(path: &str, folder: bool, workspace: &Path) -> Result<bool, String> {
    let Some(resolved) = named_path(path).ok().and_then(|p| fs::canonicalize(p).ok()) else {
        return Ok(true);
    };
    Ok(locate(&resolved, workspace, folder, path)?.is_none())
}

/// Why `attachment.inspect` can't answer.
#[derive(Debug, PartialEq, Eq)]
pub enum InspectError {
    NotFound(String),
    /// Anything but a regular file or a folder, or a path that is not one.
    Invalid(String),
}

/// What the composer shows for a path (PROTO-CORE-API-R24): its kind, size
/// and, for a text file of at most [`LINE_COUNT_MAX`] bytes, its line count.
/// An image is told by its first bytes, among `media_types`.
pub fn inspect(path: &str, media_types: &[&'static str]) -> Result<Inspected, InspectError> {
    let given = named_path(path).map_err(InspectError::Invalid)?;
    let not_found = || InspectError::NotFound(format!("nothing is at {path}"));
    let invalid = |e: io::Error| InspectError::Invalid(format!("{path} can't be read: {e}"));
    let own = match fs::symlink_metadata(given) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(not_found()),
        Err(e) => return Err(invalid(e)),
    };
    let target = if own.file_type().is_symlink() {
        Some(
            fs::read_link(given)
                .map_err(invalid)?
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        None
    };
    let meta = match fs::metadata(given) {
        Ok(meta) => meta,
        // A link whose target is gone.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(not_found()),
        Err(e) => return Err(invalid(e)),
    };
    let name = given
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let mut inspected = Inspected {
        path: path.to_string(),
        name,
        kind: AttachmentKind::Folder,
        size_bytes: None,
        lines: None,
        media_type: None,
        target,
        copied: false,
    };
    if meta.is_dir() {
        return Ok(inspected);
    }
    if !meta.is_file() {
        return Err(InspectError::Invalid(format!(
            "{path} is neither a regular file nor a folder"
        )));
    }
    inspected.size_bytes = Some(meta.len());
    let mut file = fs::File::open(given).map_err(invalid)?;
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    (&mut file)
        .take(SNIFF_BYTES as u64)
        .read_to_end(&mut head)
        .map_err(invalid)?;
    if let Some(media_type) = sniff_image(&head, media_types) {
        inspected.kind = AttachmentKind::Image;
        inspected.media_type = Some(media_type.to_string());
        return Ok(inspected);
    }
    inspected.kind = AttachmentKind::File;
    if meta.len() <= LINE_COUNT_MAX {
        let mut text = head;
        file.take(LINE_COUNT_MAX + 1 - text.len() as u64)
            .read_to_end(&mut text)
            .map_err(invalid)?;
        inspected.lines = count_lines(&text);
    }
    Ok(inspected)
}

/// The lines of a text file: `None` for one that is not text (not UTF-8,
/// or holding a NUL byte).
fn count_lines(bytes: &[u8]) -> Option<u64> {
    if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return None;
    }
    let breaks = bytes.iter().filter(|&&b| b == b'\n').count() as u64;
    let unterminated = !bytes.is_empty() && !bytes.ends_with(b"\n");
    Some(breaks + u64::from(unterminated))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

    #[test]
    fn images_are_told_by_their_first_bytes() {
        assert_eq!(
            sniff_image(b"\x89PNG\r\n\x1a\nrest", ALL),
            Some("image/png")
        );
        assert_eq!(sniff_image(b"\xff\xd8\xff\xe0", ALL), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a....", ALL), Some("image/gif"));
        assert_eq!(
            sniff_image(b"RIFF\0\0\0\0WEBPVP8 ", ALL),
            Some("image/webp")
        );
        assert_eq!(sniff_image(b"plain text", ALL), None);
        // A type the backend does not take is no image to it.
        assert_eq!(sniff_image(b"GIF89a....", &["image/png"]), None);
    }

    #[test]
    fn lines_count_a_last_line_without_a_break() {
        assert_eq!(count_lines(b""), Some(0));
        assert_eq!(count_lines(b"a\nb\n"), Some(2));
        assert_eq!(count_lines(b"a\nb"), Some(2));
        assert_eq!(count_lines(b"a\0b"), None);
        assert_eq!(count_lines(b"\xff\xfe"), None);
    }

    /// A send stopped at any step, checking or copying, leaves nothing in
    /// the scratch folder.
    // verifies: PROTO-CORE-API-R25
    #[test]
    fn a_job_stopped_at_any_step_leaves_no_copy() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(outside.join("dir").join("sub")).unwrap();
        fs::write(outside.join("a.txt"), vec![b'a'; 3 * CHUNK]).unwrap();
        fs::write(outside.join("dir").join("sub").join("b.txt"), b"b").unwrap();
        let file = |kind, name: &str| Attachment {
            kind,
            path: Some(outside.join(name).display().to_string()),
            media_type: None,
            data: None,
        };
        let attachments = [
            file(AttachmentKind::File, "a.txt"),
            file(AttachmentKind::Folder, "dir"),
        ];
        let target = Target {
            workspace: &workspace,
            session_id: "s-1",
            images: ImageLimits::default(),
        };
        let scratch = workspace.join(ERSA).join(ATTACHMENTS).join("s-1");
        let entries = |dir: &Path| -> Vec<String> {
            fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut stopped = 0;
        for stop_at in 1.. {
            let work = Work::default();
            work.stop_at.store(stop_at, Ordering::SeqCst);
            match prepare(&target, &attachments, &work) {
                Err(Refused::Stopped) => {
                    stopped += 1;
                    // Nothing moved into place, and nothing left aside.
                    let placed = entries(&scratch);
                    assert!(
                        placed.iter().all(|n| n == PARTIAL),
                        "step {stop_at}: {placed:?}"
                    );
                    let aside = entries(&scratch.join(PARTIAL));
                    assert!(aside.is_empty(), "step {stop_at}: {aside:?}");
                }
                Ok(prepared) => {
                    // Past the last step: the job finished.
                    assert_eq!(prepared.attachments.len(), 2);
                    assert!(work.copying().is_some());
                    break;
                }
                Err(other) => panic!("step {stop_at}: {other:?}"),
            }
        }
        // Stops while copying were among them: the file alone takes three
        // chunks.
        assert!(stopped > 10, "{stopped}");
    }

    /// A file swapped for a FIFO after it was checked is refused when the
    /// copy opens it, at once, rather than waiting for a writer that never
    /// comes while the job holds the session's scratch folder.
    // verifies: PROTO-CORE-API-R23
    #[cfg(unix)]
    #[test]
    fn a_source_swapped_for_a_fifo_after_the_check_is_refused_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = fs::canonicalize(dir.path()).unwrap().join("workspace");
        let outside = workspace.parent().unwrap().join("outside");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let source = outside.join("notes.md");
        fs::write(&source, b"notes").unwrap();
        let attachment = Attachment {
            kind: AttachmentKind::File,
            path: Some(source.display().to_string()),
            media_type: None,
            data: None,
        };
        let target = Target {
            workspace: &workspace,
            session_id: "s-1",
            images: ImageLimits::default(),
        };
        let work = Work::default();
        let checked = check(&attachment, &workspace, target.images, &mut 0, &work)
            .unwrap_or_else(|_| panic!("the file passes the check"));
        assert!(matches!(checked, Checked::Copy { .. }));

        // The swap: the checked file becomes a FIFO with no writer.
        fs::remove_file(&source).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(&source)
            .status()
            .unwrap();
        assert!(made.success());

        let (answer, answered) = std::sync::mpsc::channel();
        let job_workspace = workspace.clone();
        std::thread::spawn(move || {
            let target = Target {
                workspace: &job_workspace,
                session_id: "s-1",
                images: ImageLimits::default(),
            };
            let mut checked = [checked];
            let result = copy_all(
                &target,
                &job_workspace,
                &mut checked,
                0,
                &mut Vec::new(),
                &Work::default(),
            );
            let _ = answer.send(result);
        });
        // A generous limit on the copy's own answer, never a pause sized to
        // pass: an open that waits on a writer never answers at all.
        let result = answered.recv_timeout(Duration::from_secs(10));
        if result.is_err() {
            // Release the stuck open so the thread ends with the test.
            let _ = fs::OpenOptions::new().write(true).open(&source);
        }
        match result {
            Ok(Err(Refused::At { index: 0, reason })) => {
                assert!(reason.contains("notes.md"), "{reason}");
            }
            other => panic!("the copy must be refused at once: {other:?}"),
        }
        // Nothing stays in the scratch folder.
        let scratch = workspace.join(ERSA).join(ATTACHMENTS).join("s-1");
        let partial: Vec<_> = fs::read_dir(scratch.join(PARTIAL)).unwrap().collect();
        assert!(partial.is_empty());
    }

    /// What session `job`'s send does to Core's ignore file at `ignore`,
    /// staging under that session's own name, as `scratch_folder` does.
    fn make_again(ignore: &Path, job: &str) -> Result<(), String> {
        let staging = ignore.with_file_name(format!(".gitignore.{job}"));
        ignore_everything(ignore, Some(&staging)).map_err(|fail| match fail {
            Fail::Reason(reason) => reason,
            Fail::Stopped => "stopped".into(),
        })
    }

    /// Jobs of different sessions in one workspace, nothing ordering them,
    /// all find a shipped `.ersa/attachments/.gitignore` that ignores less
    /// and make it again at once: none of them is refused for it, and it ends
    /// holding `*`.
    // verifies: PROTO-CORE-API-R110
    #[test]
    fn sessions_making_cores_ignore_file_again_at_once_all_succeed() {
        const JOBS: usize = 8;
        // The race this checks (#606: one job's open finding the file
        // another job's rename just replaced) is a window inside the file
        // system, so it shows only over many rounds. At 100 rounds it failed
        // about 6 runs in 100 on CI's Windows runner. 500 rounds give each run
        // five times the chances, about 26 runs in 100 without the fix, and
        // take about as long there (some 17 seconds) as the rest of this
        // crate's tests beside them. More jobs a round does not raise the
        // rate.
        const ROUNDS: usize = 500;
        let dir = tempfile::tempdir().unwrap();
        let attachments = dir.path().join(ATTACHMENTS);
        fs::create_dir_all(&attachments).unwrap();
        let ignore = attachments.join(".gitignore");
        for round in 0..ROUNDS {
            fs::write(&ignore, b"").unwrap();
            let start = std::sync::Arc::new(std::sync::Barrier::new(JOBS));
            let jobs: Vec<_> = (0..JOBS)
                .map(|job| {
                    let (start, ignore) = (start.clone(), ignore.clone());
                    std::thread::spawn(move || {
                        start.wait();
                        make_again(&ignore, &format!("s-{job}"))
                    })
                })
                .collect();
            for job in jobs {
                let answer = job.join().unwrap();
                assert!(answer.is_ok(), "round {round}: {answer:?}");
            }
            assert_eq!(fs::read(&ignore).unwrap(), IGNORE_ALL, "round {round}");
        }
        // No staging file is left behind.
        let left: Vec<_> = fs::read_dir(&attachments)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from(".gitignore")]);
    }

    /// The other half of what the test above races, held still: a job's
    /// rename replaces Core's ignore file while another job has it open to
    /// read, and leaves no name behind that is still being deleted, so a
    /// further job's rename replaces it again. The replaced file lives on,
    /// deleted, only under the reader's handle: the state in which another
    /// job's open by name is refused on Windows (#606), and why the job
    /// makes the file again rather than refusing when its open is refused.
    // verifies: PROTO-CORE-API-R110
    #[test]
    fn a_job_replaces_cores_ignore_file_while_another_job_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let attachments = dir.path().join(ATTACHMENTS);
        fs::create_dir_all(&attachments).unwrap();
        let ignore = attachments.join(".gitignore");
        fs::write(&ignore, b"").unwrap();
        // Opened as a job's check opens it.
        let mut reader = open_file(&ignore).unwrap();
        assert_eq!(make_again(&ignore, "s-1"), Ok(()));
        assert_eq!(fs::read(&ignore).unwrap(), IGNORE_ALL);
        // The file written in place holds something else again, while the
        // replaced one is still open under the reader.
        fs::write(&ignore, b"!keep\n").unwrap();
        assert_eq!(make_again(&ignore, "s-2"), Ok(()));
        assert_eq!(fs::read(&ignore).unwrap(), IGNORE_ALL);
        let mut held = Vec::new();
        reader.read_to_end(&mut held).unwrap();
        assert_eq!(held, b"", "the reader still reads the file it opened");
    }

    #[test]
    fn relative_paths_use_forward_slashes() {
        assert_eq!(
            relative(Path::new("src").join("main.rs").as_path()),
            "src/main.rs"
        );
    }
}
