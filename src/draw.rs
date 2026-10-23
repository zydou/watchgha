//! Render the runs, jobs, and steps for the terminal.

use std::io::IsTerminal;

use chrono::{DateTime, Local};

use crate::data::{Job, Run};
use crate::utils::{nice_time, pad, scrub};

/// The number of jobs and their results, and whether everything is done.
#[derive(Debug, Default, Clone, Copy)]
pub struct Outcome {
    pub done: bool,
    pub succeeded: bool,
    pub total: usize,
    pub num_succeeded: usize,
    pub num_failed: usize,
}

/// Whether to use color and hyperlinks.
pub struct Renderer {
    color: bool,
    links: bool,
}

impl Renderer {
    /// No color and no hyperlinks, for tests and non-terminal output.
    pub const PLAIN: Renderer = Renderer {
        color: false,
        links: false,
    };

    /// Whether the terminal supports color and hyperlinks.
    pub fn detect() -> Renderer {
        if !std::io::stdout().is_terminal()
            || std::env::var_os("NO_COLOR").is_some()
            || std::env::var_os("CI").is_some()
            || std::env::var("TERM").is_ok_and(|t| t == "dumb")
        {
            return Self::PLAIN;
        }
        Self {
            color: true,
            links: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Style {
    color: Option<Color>,
    bold: bool,
    dim: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Red,
    Green,
    Blue,
    White,
}

const RED_BOLD: Style = Style {
    color: Some(Color::Red),
    bold: true,
    dim: false,
};
const GREEN_BOLD: Style = Style {
    color: Some(Color::Green),
    bold: true,
    dim: false,
};
const DIM: Style = Style {
    color: None,
    bold: false,
    dim: true,
};
const RED: Style = Style {
    color: Some(Color::Red),
    bold: false,
    dim: false,
};
const GREEN: Style = Style {
    color: Some(Color::Green),
    bold: false,
    dim: false,
};
const BLUE: Style = Style {
    color: Some(Color::Blue),
    bold: false,
    dim: false,
};
const WHITE: Style = Style {
    color: Some(Color::White),
    bold: false,
    dim: false,
};
const WHITE_BOLD: Style = Style {
    color: Some(Color::White),
    bold: true,
    dim: false,
};
const DIM_WHITE: Style = Style {
    color: Some(Color::White),
    bold: false,
    dim: true,
};

/// Whether a run has reached a terminal state, whatever its conclusion.
fn is_finished(status: &str) -> bool {
    matches!(status, "completed" | "skipped")
}

/// Whether the jobs of a run need to be shown.
fn no_jobs(summary: &str) -> bool {
    matches!(summary, "skipped" | "success")
}

/// Whether a conclusion counts as a failure.
fn is_bad(conclusion: &str) -> bool {
    matches!(
        conclusion,
        "failure" | "startup_failure" | "cancelled" | "timed_out"
    )
}

/// The summary, style, and icon for a run, job, or step.
fn status_icon<'a>(status: &'a str, conclusion: &'a str) -> (&'a str, Style, char) {
    let summary = if status == "completed" {
        conclusion
    } else {
        status
    };
    let style = match summary {
        "failure" | "startup_failure" => RED_BOLD,
        "pending" | "queued" => DIM,
        "success" => GREEN_BOLD,
        _ => Style::default(),
    };
    let icon = match summary {
        "cancelled" => '†',
        "failure" | "startup_failure" => '✗',
        "in_progress" => '↻',
        "pending" | "queued" => '⏲',
        "skipped" => '⏭',
        "success" => '✓',
        "waiting" => '🛑',
        _ => ' ',
    };
    (summary, style, icon)
}

/// The dot for one step, in a row of step dots.
fn step_dot(summary: &str) -> (Style, char) {
    match summary {
        "cancelled" => (RED, '†'),
        "failure" => (RED, '•'),
        "in_progress" => (WHITE, '•'),
        "pending" | "queued" => (DIM_WHITE, '•'),
        "skipped" => (Style::default(), '◦'),
        "success" => (GREEN, '•'),
        _ => (Style::default(), '?'),
    }
}

/// The style, icon, current step, and step dots for a job.
struct JobStatus {
    style: Style,
    icon: char,
    current: String,
    dots: Vec<(Style, char)>,
    running: bool,
}

/// Figure out what to show for a job's current step.
fn job_status(job: &Job) -> JobStatus {
    let (summary, style, icon) = status_icon(&job.status, &job.conclusion);
    let mut current = summary.to_string();
    let mut dots = Vec::new();
    let mut running = false;
    if current != "success" {
        if job.status == "queued" {
            current = "queued".to_string();
            running = true;
        } else {
            let mut failed: Option<&str> = None;
            let mut in_progress: Option<&str> = None;
            for step in &job.steps {
                if step.status == "completed" && step.conclusion == "failure" {
                    failed = Some(step.name.as_str());
                    break;
                }
                if step.status == "in_progress" {
                    in_progress = Some(step.name.as_str());
                    break;
                }
            }
            match (failed, in_progress) {
                (Some(name), _) => current = format!("failure {name}"),
                (None, Some(name)) => {
                    running = true;
                    for step in &job.steps {
                        let s = status_icon(&step.status, &step.conclusion).0;
                        dots.push(step_dot(s));
                    }
                    current = format!(" {name}");
                }
                _ => {
                    if let Some(last) = job.steps.last() {
                        current = last.name.clone();
                    } else if job.status == "completed" && job.conclusion == "skipped" {
                        current = "skipped".to_string();
                    }
                }
            }
        }
    }
    JobStatus {
        style,
        icon,
        current,
        dots,
        running,
    }
}

/// Draw the events, and report how they're doing.
pub fn draw_events(
    r: &Renderer,
    now: DateTime<Local>,
    events: &[Vec<Run>],
    out: &mut String,
) -> Outcome {
    let mut o = Outcome::default();
    for event in events {
        for run in event {
            for job in &run.jobs {
                o.total += 1;
                if job.status == "completed" {
                    if is_bad(&job.conclusion) {
                        o.num_failed += 1;
                    } else {
                        o.num_succeeded += 1;
                    }
                }
            }
        }
    }

    let mut done = true;
    let mut succeeded = true;
    for event in events {
        let e = &event[0];
        let mut l = Line::new(r.color, r.links);
        l.styled(WHITE_BOLD, &e.display_title);
        l.text(&format!(" {} [{}]   ", e.head_branch, e.event));
        let sha: String = e.head_sha.chars().take(12).collect();
        l.styled(DIM, &format!("{sha}  @{}", nice_time(e.started_dt, now)));
        l.end(out);

        for run in event {
            let (summary, style, icon) = status_icon(&run.status, &run.conclusion);
            if !is_finished(&run.status) {
                done = false;
            }
            let mut l = Line::new(r.color, r.links);
            l.text("   ");
            l.styled(style, &format!("{icon} {}", pad(summary, 12)));
            l.text(" ");
            l.styled(WHITE_BOLD, &pad(&run.name, 16));
            l.text("   ");
            let id = run.html_url.rsplit('/').next().unwrap_or("");
            l.link(BLUE, &run.html_url, &format!("view {id}"));
            l.end(out);

            if no_jobs(summary) {
                continue;
            }
            succeeded = false;
            for job in &run.jobs {
                let js = job_status(job);
                if js.running {
                    done = false;
                }
                let mut l = Line::new(r.color, r.links);
                l.text("      ");
                l.text(&pad(&job.name, 30));
                l.text(" ");
                l.styled(js.style, &js.icon.to_string());
                l.text(" ");
                for (style, dot) in &js.dots {
                    l.styled(*style, &dot.to_string());
                }
                l.styled(js.style, &js.current);
                l.end(out);
            }
        }
    }
    o.done = done;
    o.succeeded = succeeded;
    o
}

/// One line of output, built from styled pieces.
struct Line {
    s: String,
    color: bool,
    links: bool,
}

impl Line {
    fn new(color: bool, links: bool) -> Self {
        Self {
            s: String::new(),
            color,
            links,
        }
    }

    fn text(&mut self, t: &str) {
        self.s.push_str(&scrub(t));
    }

    fn styled(&mut self, style: Style, t: &str) {
        let styled = self.color && style != Style::default();
        if styled {
            self.open(style);
        }
        self.s.push_str(&scrub(t));
        if styled {
            self.s.push_str("\x1b[0m");
        }
    }

    fn link(&mut self, style: Style, url: &str, t: &str) {
        let (url, text) = (scrub(url), scrub(t));
        if self.links {
            self.s.push_str(&format!("\x1b]8;;{url}\x1b\\"));
        }
        let styled = self.color && style != Style::default();
        if styled {
            self.open(style);
        }
        self.s.push_str(&text);
        if styled {
            self.s.push_str("\x1b[0m");
        }
        if self.links {
            self.s.push_str("\x1b]8;;\x1b\\");
        }
    }

    fn open(&mut self, style: Style) {
        if let Some(c) = style.color {
            self.s.push_str(match c {
                Color::Red => "\x1b[31m",
                Color::Green => "\x1b[32m",
                Color::Blue => "\x1b[34m",
                Color::White => "\x1b[37m",
            });
        }
        if style.bold {
            self.s.push_str("\x1b[1m");
        }
        if style.dim {
            self.s.push_str("\x1b[2m");
        }
    }

    fn end(&mut self, out: &mut String) {
        out.push_str(&self.s);
        out.push('\n');
    }
}

#[cfg(test)]
mod test {
    use chrono::Local;

    use crate::data::Run;
    use crate::draw::{Renderer, draw_events};

    fn completed(conclusion: &str) -> Run {
        Run {
            id: 1,
            name: "Workflow".into(),
            status: "completed".into(),
            conclusion: conclusion.into(),
            html_url: "https://github.com/me/repo/actions/runs/1".into(),
            display_title: "fix: stuff".into(),
            head_branch: "main".into(),
            head_sha: "0123456789ab".into(),
            event: "push".into(),
            run_started_at: String::new(),
            jobs_url: String::new(),
            started_dt: chrono::Utc::now(),
            jobs: Vec::new(),
        }
    }

    /// A completed run is done whatever GitHub reports as its conclusion, so a
    /// conclusion we don't recognize can't make watchgha poll forever.
    #[test]
    fn every_conclusion_of_a_completed_run_is_finished() {
        for conclusion in [
            "success",
            "failure",
            "cancelled",
            "startup_failure",
            "skipped",
            "timed_out",
            "neutral",
            "action_required",
        ] {
            let events = vec![vec![completed(conclusion)]];
            let o = draw_events(&Renderer::PLAIN, Local::now(), &events, &mut String::new());
            assert!(o.done, "conclusion {conclusion}");
        }
    }
}
