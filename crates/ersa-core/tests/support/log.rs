//! The diagnostic log a test binary's warnings go to, for tests that
//! assert what Core wrote there.

use std::sync::Arc;

/// What the test binary's subscriber wrote, every test's together: each
/// test picks its own lines by session id.
#[derive(Clone, Default)]
pub struct Log(Arc<std::sync::Mutex<Vec<u8>>>);

impl Log {
    /// The log, its subscriber installed as the binary's global default on
    /// first use. Global, not per thread: `tracing` caches whether a span
    /// is wanted across threads, so a per-thread subscriber can miss the
    /// span that names the session when another test's thread, with no
    /// subscriber, reached it first.
    pub fn installed() -> Log {
        static LOG: std::sync::OnceLock<Log> = std::sync::OnceLock::new();
        LOG.get_or_init(|| {
            let log = Log::default();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(log.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::WARN)
                .finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
            log
        })
        .clone()
    }

    /// The lines written at warning level.
    pub fn warnings(&self) -> Vec<String> {
        let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
        text.lines()
            .filter(|line| line.contains(" WARN "))
            .map(str::to_string)
            .collect()
    }

    /// Whether a warning holds `text` and the field `field`, as
    /// `workspace=w-…`.
    pub fn warned(&self, text: &str, field: &str) -> bool {
        self.warnings()
            .iter()
            .any(|line| line.contains(text) && line.contains(field))
    }

    /// The one warning holding `text` and `session`'s id.
    pub fn warning(&self, text: &str, session: &str) -> String {
        let warnings = self.warnings();
        warnings
            .iter()
            .find(|line| line.contains(text) && line.contains(&format!("session={session}")))
            .cloned()
            .unwrap_or_else(|| panic!("no warning {text:?} for {session}: {warnings:#?}"))
    }
}

impl std::io::Write for Log {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Log;
    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}
