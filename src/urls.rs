//! Figure out which GitHub API URLs to use for a repo and branch.

use regex::Regex;
use url::form_urlencoded;

use crate::git::Git;
use crate::utils::Error;
use crate::utils::Result;

/// Figure out the GitHub API URLs to use for `repo`, `branch`, and `sha`.
pub fn gha_urls(
    git: &dyn Git,
    server_url: &str,
    api_url: &str,
    repo: &str,
    branch: Option<&str>,
    sha: Option<&str>,
) -> Result<Vec<String>> {
    let (repo_urls, branch) = if git.is_dir(repo) {
        let urls = git.repo_urls(repo)?;
        let branch = match (branch, sha) {
            (Some(b), _) => Some(b.to_string()),
            // `--sha` selects the runs, so no branch is needed.
            (None, Some(_)) => None,
            (None, None) => Some(git.branch(repo)?),
        };
        (urls, branch)
    } else if repo.contains(':') {
        let branch = match branch {
            Some(b) => b.to_string(),
            None => return Err(Error::msg("Branch is required for URL repo")),
        };
        (vec![repo.to_string()], Some(branch))
    } else {
        return Err(Error::msg(format!("Don't understand repo {repo:?}")));
    };

    let re = repo_url_re(server_url);
    let mut urls = Vec::new();
    for repo_url in &repo_urls {
        if let Some(caps) = re.captures(repo_url) {
            let path = caps.get(1).unwrap().as_str();
            urls.push(format!(
                "{api_url}/repos/{path}/actions/runs?{}",
                query(branch.as_deref(), sha)
            ));
        }
    }
    if urls.is_empty() {
        return Err(Error::msg(format!(
            "Couldn't find GitHub repo from remote URLs: {repo_urls:?}"
        )));
    }
    Ok(urls)
}

/// Match a repo URL for `server_url`, like "https://github.com/owner/repo.git"
/// or "git@github.com:owner/repo.git", capturing "owner/repo".
fn repo_url_re(server_url: &str) -> Regex {
    Regex::new(&format!(
        r"\A(?:(?:{}|git@github\.com)[/:]?([^/]+/[^/]+?)(?:\.git|/)?)\z",
        regex::escape(server_url)
    ))
    .unwrap()
}

fn query(branch: Option<&str>, sha: Option<&str>) -> String {
    let mut s = form_urlencoded::Serializer::new(String::new());
    s.append_pair("per_page", "100");
    if let Some(sha) = sha {
        s.append_pair("head_sha", sha);
    } else if let Some(branch) = branch {
        s.append_pair("branch", branch);
    }
    s.finish()
}
