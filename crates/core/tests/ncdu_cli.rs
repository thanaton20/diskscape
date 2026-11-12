//! Cross-checks against the real `ncdu` binary. Skipped when it is not installed.
#![cfg(unix)]

use diskscape_core::ncdu;
use diskscape_core::scan::{scan, CancelToken, ScanOptions};
use diskscape_core::tree::Tree;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn ncdu_available() -> bool {
    let found = Command::new("ncdu")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !found {
        eprintln!("ncdu is not installed; skipping");
    }
    found
}

fn sample() -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    fs::create_dir_all(p.join("sub/.hidden/deeper")).unwrap();
    fs::create_dir_all(p.join("empty")).unwrap();
    fs::write(p.join("a.txt"), "hello").unwrap();
    fs::write(p.join(".dotfile"), vec![7u8; 10_000]).unwrap();
    fs::write(p.join("sub/.hidden/b.bin"), vec![1u8; 70_000]).unwrap();
    fs::write(p.join("sub/.hidden/deeper/c"), "").unwrap();
    fs::write(p.join("sub/we\"ird\nname"), "x").unwrap();
    fs::hard_link(p.join("a.txt"), p.join("sub/hard.txt")).unwrap();
    std::os::unix::fs::symlink(p.join("sub"), p.join("link")).unwrap();
    tmp
}

fn ours(root: &Path) -> Tree {
    let opts = ScanOptions { root: root.to_path_buf(), threads: 2 };
    scan(&opts, &CancelToken::default(), &|_| {}).unwrap()
}

fn totals(t: &Tree) -> (u64, u64, u64) {
    let root = t.node(t.root()).unwrap();
    (root.dsize, root.asize, root.items)
}

#[test]
fn root_totals_match_ncdu() {
    if !ncdu_available() {
        return;
    }
    let tmp = sample();
    let out = Command::new("ncdu").args(["-o", "-"]).arg(tmp.path()).output().unwrap();
    assert!(out.status.success(), "ncdu failed: {}", String::from_utf8_lossy(&out.stderr));
    let theirs = ncdu::import(&out.stdout[..]).unwrap();
    assert_eq!(totals(&ours(tmp.path())), totals(&theirs));
}

#[test]
fn ncdu_accepts_our_export() {
    if !ncdu_available() {
        return;
    }
    let tmp = sample();
    let tree = ours(tmp.path());
    let file = tempfile::NamedTempFile::new().unwrap();
    ncdu::export(&tree, file.as_file()).unwrap();

    let out = Command::new("ncdu").arg("-f").arg(file.path()).args(["-o", "-"]).output().unwrap();
    assert!(out.status.success(), "ncdu rejected our export: {}", String::from_utf8_lossy(&out.stderr));
    let reexported = ncdu::import(&out.stdout[..]).unwrap();
    assert_eq!(totals(&reexported), totals(&tree));
}
