//! Stamps the build with the commit it was made from (`KC_BUILD`), so the
//! home screen shows which version is running.

use std::{path::Path, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok().filter(|o| o.status.success())?;
    let s = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!s.is_empty()).then_some(s)
}

fn main() {
    // Rebuilt when HEAD moves: a new commit or another branch. The paths
    // come from git, so linked worktrees (where `.git` is a file) work too.
    println!("cargo:rerun-if-changed=build.rs");
    let common = git(&["rev-parse", "--git-common-dir"]);
    let watched = [git(&["rev-parse", "--git-path", "HEAD"])]
        .into_iter()
        .flatten()
        .chain(common.iter().flat_map(|c| [format!("{c}/refs/heads"), format!("{c}/packed-refs")]));
    for path in watched.filter(|p| Path::new(p).exists()) {
        println!("cargo:rerun-if-changed={path}");
    }
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "dev".to_owned());
    println!("cargo:rustc-env=KC_BUILD={commit}");
}
