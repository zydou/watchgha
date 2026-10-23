//! Read information about a git repo.
//!
//! `git` is called as a command, rather than linked in.  It's called once when
//! the program starts, and it handles the corner cases (worktrees, config
//! includes, bare repos) that a hand-rolled config parser would get wrong.

use std::process::Command;

use crate::utils::Error;
use crate::utils::Result;

/// How to get information about a git repo.
pub trait Git {
    /// Whether `dir` is a directory.
    fn is_dir(&self, dir: &str) -> bool;

    /// All the remote URLs of the repo at `dir`.
    fn repo_urls(&self, dir: &str) -> Result<Vec<String>>;

    /// The name of the current branch of the repo at `dir`.
    fn branch(&self, dir: &str) -> Result<String>;
}

/// Get repo information by calling the `git` command.
pub struct GitCli;

impl Git for GitCli {
    fn is_dir(&self, dir: &str) -> bool {
        std::path::Path::new(dir).is_dir()
    }

    fn repo_urls(&self, dir: &str) -> Result<Vec<String>> {
        // `git config --get-regexp` exits 1 when nothing matched.
        let out = git(
            dir,
            &["config", "--null", "--get-regexp", r"^remote\..*\.url$"],
            &[0, 1],
        )?;
        let urls: Vec<String> = out
            .split('\0')
            .filter_map(|record| record.split_once('\n'))
            .map(|(_, url)| url.to_string())
            .collect();
        Ok(urls)
    }

    fn branch(&self, dir: &str) -> Result<String> {
        let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"], &[0])?
            .trim()
            .to_string();
        if branch == "HEAD" {
            return Err(Error::msg(format!("Not on a branch in {dir:?}")));
        }
        Ok(branch)
    }
}

/// Run `git` in `dir`, treating the given exit codes as success.
fn git(dir: &str, args: &[&str], ok: &[i32]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| Error::msg(format!("Couldn't run git in {dir:?}: {e}")))?;
    if out.status.code().is_some_and(|c| ok.contains(&c)) {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(Error::msg(format!(
        "git {} failed in {dir}: {}",
        args.join(" "),
        err.trim()
    )))
}
