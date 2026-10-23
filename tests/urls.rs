use watchgha::git::Git;
use watchgha::urls::gha_urls;
use watchgha::utils::Result;

const GITHUB: &str = "https://github.com";
const API: &str = "https://api.github.com";

struct Fake;

impl Git for Fake {
    fn is_dir(&self, dir: &str) -> bool {
        dir == "." || dir.starts_with("dir")
    }

    fn repo_urls(&self, dir: &str) -> Result<Vec<String>> {
        Ok(repo(dir).0.iter().map(|s| s.to_string()).collect())
    }

    fn branch(&self, dir: &str) -> Result<String> {
        Ok(repo(dir).1.unwrap_or("master").to_string())
    }
}

fn repo(dir: &str) -> (&'static [&'static str], Option<&'static str>) {
    match dir {
        "." => (&["https://github.com/owner/repo.git"], Some("mainbranch")),
        "dir1" => (
            &["git@github.com:somebody/therepo.git"],
            Some("joe/feature1"),
        ),
        "dir-many-remotes" => (
            &[
                "https://gitlab.com/maintainer/project.git",
                "https://github.com/contributor1/project",
                "https://github.com/contributor2/project.git",
            ],
            Some("main"),
        ),
        "dir-only-gitlab" => (&["https://gitlab.com/maintainer/project.git"], None),
        "dir-enterprise" => (&["https://mygithub.enterprise.com/me/myproject"], None),
        "dir-bug22" => (&["git@git.mydomain.com:davidszotten/davidrepo"], None),
        _ => (&[], None),
    }
}

type Case = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    &'static [&'static str],
);

#[test]
fn gha_urls_builds_the_api_urls() {
    let cases: [Case; 9] = [
        (
            GITHUB,
            API,
            ".",
            None,
            None,
            &[
                "https://api.github.com/repos/owner/repo/actions/runs?per_page=100&branch=mainbranch",
            ],
        ),
        (
            GITHUB,
            API,
            "dir1",
            None,
            None,
            &[
                "https://api.github.com/repos/somebody/therepo/actions/runs?per_page=100&branch=joe%2Ffeature1",
            ],
        ),
        (
            GITHUB,
            API,
            "dir1",
            Some("another-branch"),
            None,
            &[
                "https://api.github.com/repos/somebody/therepo/actions/runs?per_page=100&branch=another-branch",
            ],
        ),
        (
            GITHUB,
            API,
            "dir1",
            None,
            Some("sha12345678"),
            &[
                "https://api.github.com/repos/somebody/therepo/actions/runs?per_page=100&head_sha=sha12345678",
            ],
        ),
        (
            GITHUB,
            API,
            "dir-many-remotes",
            None,
            None,
            &[
                "https://api.github.com/repos/contributor1/project/actions/runs?per_page=100&branch=main",
                "https://api.github.com/repos/contributor2/project/actions/runs?per_page=100&branch=main",
            ],
        ),
        (
            GITHUB,
            "https://theapi.nedhub.com",
            ".",
            None,
            None,
            &[
                "https://theapi.nedhub.com/repos/owner/repo/actions/runs?per_page=100&branch=mainbranch",
            ],
        ),
        (
            GITHUB,
            API,
            "https://github.com/me/myproject.git",
            Some("mybranch"),
            None,
            &[
                "https://api.github.com/repos/me/myproject/actions/runs?per_page=100&branch=mybranch",
            ],
        ),
        (
            "https://mygithub.enterprise.com",
            "https://api.mygithub.enterprise.com",
            "dir-enterprise",
            None,
            None,
            &[
                "https://api.mygithub.enterprise.com/repos/me/myproject/actions/runs?per_page=100&branch=master",
            ],
        ),
        (
            "git@git.mydomain.com",
            "https://githubapi.mydomain.com",
            "dir-bug22",
            None,
            None,
            &[
                "https://githubapi.mydomain.com/repos/davidszotten/davidrepo/actions/runs?per_page=100&branch=master",
            ],
        ),
    ];
    for (server, api, dir, branch, sha, expected) in cases {
        let got = gha_urls(&Fake, server, api, dir, branch, sha).unwrap();
        let got: Vec<&str> = got.iter().map(String::as_str).collect();
        assert_eq!(got, expected, "gha_urls({dir:?})");
    }
}

#[test]
fn gha_urls_rejects_bad_repos() {
    for (dir, branch, sha, msg) in [
        (
            "https://mygithub.com/me/myproject.git",
            None,
            None,
            "Branch is required for URL repo",
        ),
        ("what!?", None, None, "Don't understand repo \"what!?\""),
        (
            "dir-only-gitlab",
            None,
            None,
            "Couldn't find GitHub repo from remote URLs: [\"https://gitlab.com/maintainer/project.git\"]",
        ),
    ] {
        let err = gha_urls(&Fake, GITHUB, API, dir, branch, sha).unwrap_err();
        assert_eq!(err.0, msg, "gha_urls({dir:?})");
    }
}
