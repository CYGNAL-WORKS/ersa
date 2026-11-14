//! What a build of the Shell uses when it starts (ADR 0035): which Core it
//! reaches, where that Core keeps its data, and the window's one-per-user
//! claim, and where its webview keeps its data (FLOW-CLOSE-AND-QUIT-R02,
//! R08, R09, R10).
//!
//! - The daily build (a release build) runs its own Core on the endpoint
//!   label `daily`, with its data in the `daily` folder of Ersa's
//!   application data folder. Both are fixed when it is built: nothing in
//!   the environment moves them.
//! - A development build (a debug build) uses the user's default endpoint
//!   and data folder, or attaches to a labelled endpoint when told to
//!   (`ERSA_ATTACH`), but never to the daily build's.

use std::path::{Path, PathBuf};

use crate::launch::CoreLocation;

/// The daily build's endpoint label and data folder name.
pub const DAILY: &str = "daily";

/// The daily build's window claim. Development windows claim `window` or
/// `window-<label>`, so the two never meet.
const DAILY_WINDOW: &str = "daily-window";

/// Which build this is: a release build is the daily build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    Development,
    Daily,
}

impl Build {
    /// The build this executable is, from the cargo profile it was built in.
    pub fn current() -> Build {
        if cfg!(debug_assertions) {
            Build::Development
        } else {
            Build::Daily
        }
    }
}

/// Which Core the window reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Core {
    /// The user's default endpoint and data folder: launched when nothing
    /// serves it, shut down on Quit.
    Default,
    /// A Core of the window's own on a labelled endpoint, with its data in
    /// `data_dir`: launched when nothing serves it, shut down on Quit.
    Own { label: String, data_dir: PathBuf },
    /// Someone else's Core on a labelled endpoint: attach only.
    AttachOnly(String),
    /// None: the window says why, in the user's terms.
    Refused(String),
}

impl Core {
    /// Where this Core is served and how it is started, or why the window
    /// has none, in the user's terms.
    pub fn location(&self) -> Result<CoreLocation, String> {
        let unlocated = |e: std::io::Error| format!("Ersa can't work out where to connect. {e}");
        match self {
            Core::Default => CoreLocation::default_for_user().map_err(unlocated),
            Core::Own { label, data_dir } => CoreLocation::own(label, data_dir).map_err(unlocated),
            Core::AttachOnly(label) => CoreLocation::attach_only(label).map_err(unlocated),
            Core::Refused(why) => Err(why.clone()),
        }
    }
}

/// What [`startup`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Startup {
    /// The label of the window's one-per-user claim.
    pub claim: String,
    /// The label the window was told to attach to, when it was: the window
    /// says so in the rail's foot, and Quit leaves that Core running.
    pub attached: Option<String>,
    pub core: Core,
    /// The folder the window's webview keeps its own data in (cookies,
    /// local storage, caches), or `None` for the webview's default. Both
    /// builds share one bundle identifier, and the default folder is named
    /// after it, so the daily build moves its own into `daily`
    /// (FLOW-CLOSE-AND-QUIT-R09).
    pub web_data: Option<PathBuf>,
}

/// The labels a development build may not attach to: the daily build's
/// Core and window. Compared ignoring letter case, since Windows pipe names
/// do (`Daily` is the same pipe as `daily`).
fn belongs_to_daily(label: &str) -> bool {
    [DAILY, DAILY_WINDOW]
        .iter()
        .any(|daily| label.eq_ignore_ascii_case(daily))
}

/// Decides, for `build`, the window's claim and its Core. `attach` is
/// `ERSA_ATTACH`, which only a development build reads; `app_data` is Ersa's
/// application data folder (`%LOCALAPPDATA%\Ersa` on Windows).
pub fn startup(build: Build, attach: Option<&str>, app_data: Option<&Path>) -> Startup {
    match build {
        Build::Daily => Startup {
            claim: DAILY_WINDOW.to_string(),
            attached: None,
            // Without the application data folder there is no `daily`
            // folder either, and the window only says why it has no Core.
            // Its web data still never goes to the webview's default folder:
            // that one comes from the system rather than LOCALAPPDATA, so it
            // is the development build's even then.
            web_data: Some(match app_data {
                Some(dir) => dir.join(DAILY),
                None => std::env::temp_dir().join("ersa-daily-web"),
            }),
            core: match app_data {
                Some(dir) => Core::Own {
                    label: DAILY.to_string(),
                    data_dir: dir.join(DAILY),
                },
                // Never fall back to the default folder: the daily build's
                // sessions stay apart from a development build's.
                None => Core::Refused(
                    "Ersa can't find the folder where it keeps your sessions. Make sure LOCALAPPDATA is set, then open Ersa again."
                        .to_string(),
                ),
            },
        },
        Build::Development => match attach.filter(|label| !label.is_empty()) {
            None => Startup {
                claim: "window".to_string(),
                attached: None,
                web_data: None,
                core: Core::Default,
            },
            Some(label) => Startup {
                claim: format!("window-{label}"),
                attached: Some(label.to_string()),
                web_data: None,
                core: if belongs_to_daily(label) {
                    Core::Refused(format!(
                        "This development build can't attach to {label}: it belongs to the daily build, so your daily sessions stay out of reach. Attach to another label, or open the daily build."
                    ))
                } else {
                    Core::AttachOnly(label.to_string())
                },
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_data() -> PathBuf {
        PathBuf::from("app-data").join("Ersa")
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R08
    #[test]
    fn the_daily_build_runs_its_own_core_on_daily_whatever_the_environment_says() {
        for attach in [None, Some("replay"), Some("daily"), Some("")] {
            let daily = startup(Build::Daily, attach, Some(&app_data()));
            assert_eq!(
                daily.core,
                Core::Own {
                    label: "daily".to_string(),
                    data_dir: app_data().join("daily"),
                },
                "attach {attach:?}"
            );
            // Not attached: the foot stays empty, and Quit stops this Core.
            assert_eq!(daily.attached, None);
            assert_eq!(daily.claim, "daily-window");
        }
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R08
    #[test]
    fn the_daily_builds_core_is_served_on_daily_with_its_data_and_log_in_daily() {
        let daily = startup(Build::Daily, None, Some(&app_data()));
        let location = daily.core.location().expect("a location");
        let folder = app_data().join("daily");
        assert_eq!(
            location.endpoint,
            ersa_core::platform::labelled_endpoint("daily").expect("an endpoint")
        );
        let serve_args: Vec<std::ffi::OsString> = vec![
            "--endpoint".into(),
            "daily".into(),
            "--data-dir".into(),
            folder.clone().into_os_string(),
        ];
        assert_eq!(location.serve_args, serve_args);
        assert_eq!(location.log_file, Some(folder.join("core.log")));
        // Its own: launched when nothing serves it.
        assert!(location.launch);
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R08
    #[test]
    fn a_refused_core_says_why_instead_of_locating_one() {
        let dev = startup(Build::Development, Some("daily"), None);
        let why = dev.core.location().expect_err("refused");
        assert!(
            why.starts_with("This development build can't attach to daily"),
            "{why}"
        );
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R08
    #[test]
    fn the_daily_build_without_a_data_folder_never_uses_the_default_one() {
        let daily = startup(Build::Daily, None, None);
        assert!(matches!(daily.core, Core::Refused(_)), "{daily:?}");
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R08
    #[test]
    fn a_development_build_keeps_the_default_core() {
        let dev = startup(Build::Development, None, Some(&app_data()));
        assert_eq!(dev.core, Core::Default);
        assert_eq!(dev.attached, None);
        assert_eq!(dev.claim, "window");
        assert_eq!(startup(Build::Development, Some(""), None), dev);
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R08
    #[test]
    fn a_development_build_refuses_the_daily_builds_labels_in_any_case() {
        for label in ["daily", "Daily", "DAILY", "daily-window", "Daily-Window"] {
            let dev = startup(Build::Development, Some(label), Some(&app_data()));
            let Core::Refused(why) = &dev.core else {
                panic!("{label} should be refused, got {dev:?}");
            };
            assert!(why.contains("daily build"), "{why}");
            assert!(!why.contains("Core"), "the user's terms: {why}");
            // Its own window, so it never shows the daily build's instead.
            assert_eq!(dev.claim, format!("window-{label}"));
            assert_ne!(dev.claim, "daily-window");
            assert_eq!(dev.attached.as_deref(), Some(label));
        }
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R08
    #[test]
    fn a_development_build_attaches_to_other_labels() {
        let dev = startup(Build::Development, Some("replay"), None);
        assert_eq!(dev.core, Core::AttachOnly("replay".to_string()));
        assert_eq!(dev.claim, "window-replay");
        assert_eq!(dev.attached.as_deref(), Some("replay"));
        // A label that only starts like the daily build's is not its.
        let near = startup(Build::Development, Some("daily2"), None);
        assert_eq!(near.core, Core::AttachOnly("daily2".to_string()));
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R10
    #[test]
    fn the_daily_and_development_windows_claim_apart() {
        let daily = startup(Build::Daily, None, Some(&app_data())).claim;
        for attach in [None, Some("replay"), Some("daily"), Some("live-1")] {
            let dev = startup(Build::Development, attach, Some(&app_data())).claim;
            assert_ne!(dev, daily, "attach {attach:?}");
        }
    }

    // verifies: FLOW-CLOSE-AND-QUIT-R09
    #[test]
    fn the_daily_builds_window_keeps_its_web_data_in_daily() {
        let daily = startup(Build::Daily, None, Some(&app_data()));
        assert_eq!(daily.web_data, Some(app_data().join("daily")));
        // A development build keeps the webview's default folder, whatever
        // it attaches to.
        for attach in [None, Some("replay"), Some("daily")] {
            let dev = startup(Build::Development, attach, Some(&app_data()));
            assert_eq!(dev.web_data, None, "attach {attach:?}");
        }
        // Without an application data folder, never the webview's default
        // (the development build's) folder either.
        let lost = startup(Build::Daily, None, None);
        assert_eq!(
            lost.web_data,
            Some(std::env::temp_dir().join("ersa-daily-web"))
        );
    }
}
