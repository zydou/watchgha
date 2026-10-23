//! The `watchgha` command.

use std::process::ExitCode;

use clap::Parser;

use watchgha::git::GitCli;
use watchgha::http::Http;
use watchgha::urls::gha_urls;
use watchgha::watcher::Watcher;

#[derive(Parser)]
#[command(name = "watchgha", version)]
#[command(about = "Watch GitHub Action runs.")]
#[command(long_about = ABOUT)]
struct Args {
    /// The commit SHA to use. Must be a full SHA.
    #[arg(long)]
    sha: Option<String>,

    /// How many seconds between refreshes.
    #[arg(long, default_value = "15")]
    poll: u64,

    /// Wait for jobs to start.
    #[arg(long, visible_alias = "wait-for-start")]
    wait: bool,

    /// Words to limit the workflows shown.
    ///
    /// Only workflows with these comma separated case insensitive substrings
    /// in their names will be shown.
    #[arg(long)]
    only: Option<String>,

    /// A message to display at the top of the screen.
    #[arg(long)]
    message: Option<String>,

    /// A local directory or GitHub URL, defaulting to "."
    #[arg(default_value = ".", hide_default_value = true)]
    repo: String,

    /// The branch to watch, defaulted from the git repo.
    branch: Option<String>,
}

const ABOUT: &str = "\
Watch GitHub Action runs.

Repeatedly gets the latest status and redraws the screen, until all of the jobs \
are complete.

REPO is a local directory or GitHub URL, defaulting to \".\".

BRANCH is defaulted from the git repo.";

fn main() -> ExitCode {
    let args = Args::parse();

    let only = args.only.map(|s| {
        s.split(',')
            .map(|w| w.trim().to_lowercase())
            .collect::<Vec<_>>()
    });
    let server_url =
        std::env::var("GITHUB_SERVER_URL").unwrap_or_else(|_| "https://github.com".into());
    let api_url =
        std::env::var("GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".into());
    let token = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty());
    // Any non-zero value of SAVE_DATA turns on saving the fetched data.
    let save = std::env::var("SAVE_DATA")
        .map(|s| !matches!(s.as_str(), "" | "0"))
        .unwrap_or(false);

    let urls = match gha_urls(
        &GitCli,
        &server_url,
        &api_url,
        &args.repo,
        args.branch.as_deref(),
        args.sha.as_deref(),
    ) {
        Ok(urls) => urls,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let http = match Http::new(token, save) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    Watcher::new(urls, http, only, args.message).run(args.wait, args.poll)
}
