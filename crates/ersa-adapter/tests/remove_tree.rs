//! [`remove_tree`], the folder deletion Core's delete and the Claude
//! adapter's record deletion share (PROTO-CORE-API-R91): it can be called
//! off between any two deletions, and it never follows a link. Every folder
//! is a temporary one the test made.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use ersa_adapter::{Removal, remove_tree};

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Every file and folder under `root`, relative to it, sorted; links are
/// listed and not entered.
fn entries(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap();
            found.push(relative.display().to_string().replace('\\', "/"));
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                stack.push(path);
            }
        }
    }
    found.sort();
    found
}

/// A folder of six entries: three files, a folder holding two files.
fn tree(root: &Path) -> PathBuf {
    let folder = root.join("folder");
    write(&folder.join("a.txt"), "a");
    write(&folder.join("b.txt"), "b");
    write(&folder.join("sub/c.txt"), "c");
    write(&folder.join("sub/d.txt"), "d");
    write(&folder.join("e.txt"), "e");
    folder
}

/// Makes `link` lead to `target`: a junction on Windows, which needs no
/// privilege, a symbolic link elsewhere.
fn link_dir(target: &Path, link: &Path) {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("mklink runs");
        assert!(status.status.success(), "{status:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

#[test]
fn a_whole_folder_goes_when_nothing_calls_it_off() {
    let temp = tempfile::tempdir().unwrap();
    let folder = tree(temp.path());
    assert_eq!(remove_tree(&folder, &|| false).unwrap(), Removal::Removed);
    assert!(!folder.exists());
}

/// Called off after `n` deletions, exactly `n` entries are gone: each
/// deletion is asked for first, and none begins once it is called off.
// verifies: PROTO-CORE-API-R91
#[test]
fn called_off_part_way_it_begins_no_further_deletion() {
    for n in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let folder = tree(temp.path());
        let before = entries(&folder);
        let asked = Cell::new(0);
        let stop = || {
            asked.set(asked.get() + 1);
            asked.get() > n
        };
        assert_eq!(remove_tree(&folder, &stop).unwrap(), Removal::CalledOff);
        let after = entries(&folder);
        assert_eq!(after.len(), before.len() - n, "after {n}: {after:?}");
        assert!(after.iter().all(|entry| before.contains(entry)));
    }
}

/// A link inside the folder is removed as a link: what it leads to, outside
/// the folder, keeps every file.
#[test]
fn a_link_inside_is_removed_and_never_followed() {
    let temp = tempfile::tempdir().unwrap();
    let folder = tree(temp.path());
    let outside = temp.path().join("outside");
    write(&outside.join("kept.txt"), "kept");
    write(&outside.join("deeper/kept.txt"), "kept");
    link_dir(&outside, &folder.join("link"));
    // Joined part by part: `mklink` reads a `/` as a switch.
    link_dir(&outside, &folder.join("sub").join("link"));

    assert_eq!(remove_tree(&folder, &|| false).unwrap(), Removal::Removed);
    assert!(!folder.exists());
    assert_eq!(entries(&outside), ["deeper", "deeper/kept.txt", "kept.txt"]);
}

/// A root that is itself a link is removed as one, as std's
/// `remove_dir_all` removes it.
#[test]
fn a_root_that_is_a_link_is_removed_as_a_link() {
    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("outside");
    write(&outside.join("kept.txt"), "kept");
    let link = temp.path().join("link");
    link_dir(&outside, &link);

    assert_eq!(remove_tree(&link, &|| false).unwrap(), Removal::Removed);
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(entries(&outside), ["kept.txt"]);
}

#[test]
fn a_root_already_gone_counts_as_removed() {
    let temp = tempfile::tempdir().unwrap();
    let gone = temp.path().join("gone");
    assert_eq!(remove_tree(&gone, &|| false).unwrap(), Removal::Removed);
}

/// A read-only file goes too, as it went with std's `remove_dir_all`.
#[test]
fn a_read_only_file_goes_too() {
    let temp = tempfile::tempdir().unwrap();
    let folder = tree(temp.path());
    let file = folder.join("sub/c.txt");
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();

    assert_eq!(remove_tree(&folder, &|| false).unwrap(), Removal::Removed);
    assert!(!folder.exists());
}
