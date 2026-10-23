//! Watch the runs, redrawing the screen until they're all done.

use std::io::{self, Write};
use std::process::ExitCode;

use chrono::{Local, Utc};

use crate::bucketer::Bucketer;
use crate::data::get_events;
use crate::draw::{Outcome, Renderer, draw_events};
use crate::http::Http;
use crate::term::{self, Wake};
use crate::utils::{Interval, Result};

pub struct Watcher {
    urls: Vec<String>,
    http: Http,
    only: Option<Vec<String>>,
    message: Option<String>,
    bucketer: Bucketer,
    status: Outcome,
}

impl Watcher {
    pub fn new(
        urls: Vec<String>,
        http: Http,
        only: Option<Vec<String>>,
        message: Option<String>,
    ) -> Self {
        Self {
            urls,
            http,
            only,
            message,
            bucketer: Bucketer::new(5.0),
            status: Outcome::default(),
        }
    }

    /// Watch the runs until they're all done.
    pub fn run(&mut self, wait_for_start: bool, poll_secs: u64) -> ExitCode {
        let signals = match term::Signals::install() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(1);
            }
        };
        let renderer = Renderer::detect();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        let mut output = String::new();
        let mut errors = Vec::new();
        let mut interrupted = false;

        'top: {
            // Keep getting the runs until the ones we're waiting for have started.
            loop {
                if signals.interrupted() {
                    interrupted = true;
                    break 'top;
                }
                let display = runtime.block_on(self.get_display(&renderer));
                match display {
                    Ok((status, text)) => {
                        self.status = status;
                        output = text;
                    }
                    Err(e) => {
                        errors.push(e);
                        break 'top;
                    }
                }
                if !wait_for_start || !self.status.done {
                    break;
                }
            }

            if !self.status.done {
                let _screen = match term::Screen::enter() {
                    Ok(s) => s,
                    Err(e) => {
                        errors.push(e);
                        break 'top;
                    }
                };
                let mut interval = Interval::new(poll_secs);
                while !self.status.done {
                    term::redraw(&output);
                    self.update_progress();
                    let left = interval.remaining();
                    interval.reset();
                    match term::wait(left, &signals) {
                        Wake::Timeout => {}
                        Wake::Resize => {
                            term::redraw(&output);
                            continue;
                        }
                        Wake::Interrupted => {
                            interrupted = true;
                            break 'top;
                        }
                    }
                    let display = runtime.block_on(self.get_display(&renderer));
                    match display {
                        Ok((status, text)) => {
                            self.status = status;
                            output = text;
                        }
                        Err(e) => {
                            errors.push(e);
                            break 'top;
                        }
                    }
                }
            }
        }

        term::clear_progress();
        if let Some(e) = errors.first() {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
        print!("{output}");
        let _ = io::stdout().flush();
        if interrupted {
            eprintln!("** interrupted **");
            return ExitCode::from(2);
        }
        if self.status.succeeded {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    async fn get_display(&mut self, r: &Renderer) -> Result<(Outcome, String)> {
        let now = Local::now();
        let Watcher {
            urls,
            http,
            only,
            message,
            bucketer,
            ..
        } = self;
        let events = get_events(
            now.with_timezone(&Utc),
            urls,
            http,
            only.as_deref(),
            bucketer,
        )
        .await?;
        let mut text = String::new();
        let outcome = draw_events(r, now, &events, &mut text);
        let text = match message {
            Some(m) if !m.is_empty() => format!("{m}\n{text}"),
            _ => text,
        };
        Ok((outcome, text))
    }

    fn update_progress(&self) {
        let total = self.status.total;
        let finished = self.status.num_succeeded + self.status.num_failed;
        let percent: u16 = finished.saturating_mul(100).saturating_div(total) as u16;
        let state = if self.status.num_failed != 0 { 2 } else { 1 };
        term::progress(state, percent);
    }
}
