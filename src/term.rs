//! Terminal control: the alternate screen, redraws, progress, and signals.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::execute;
use crossterm::style::Print;
use crossterm::terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use signal_hook::SigId;
use signal_hook::consts::{SIGINT, SIGWINCH};
use signal_hook::flag;

use crate::utils::Result;

/// The signals we watch for, with atomic flags that the handlers set.
pub struct Signals {
    winch: Arc<AtomicBool>,
    interrupted: Arc<AtomicBool>,
    _ids: Vec<SigId>,
}

impl Signals {
    /// Install handlers for a window resize and for ctrl-C.
    pub fn install() -> Result<Self> {
        let winch = Arc::new(AtomicBool::new(false));
        let interrupted = Arc::new(AtomicBool::new(false));
        let winch_id = flag::register(SIGWINCH, Arc::clone(&winch))?;
        let int_id = flag::register(SIGINT, Arc::clone(&interrupted))?;
        Ok(Self {
            winch,
            interrupted,
            _ids: vec![winch_id, int_id],
        })
    }

    /// Whether ctrl-C has been pressed, clearing the flag as we read it.
    pub fn interrupted(&self) -> bool {
        self.interrupted.swap(false, Ordering::Relaxed)
    }
}

/// The alternate screen, left when dropped.
pub struct Screen;

impl Screen {
    pub fn enter() -> Result<Screen> {
        execute!(std::io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(Screen)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), Show, LeaveAlternateScreen);
    }
}

/// Redraw the whole screen with `text`, which may contain ANSI escapes.
pub fn redraw(text: &str) {
    let _ = execute!(
        std::io::stdout(),
        MoveTo(0, 0),
        Clear(ClearType::All),
        Print(text)
    );
}

/// Show progress with an OSC 9;4 escape sequence, which some terminals use.
pub fn progress(state: u8, percent: u16) {
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]9;4;{state};{percent}\x1b\\");
    let _ = out.flush();
}

pub fn clear_progress() {
    progress(0, 0);
}

pub enum Wake {
    Timeout,
    Resize,
    Interrupted,
}

/// Sleep for `dur`, but wake up early for a window resize or ctrl-C.
pub fn wait(dur: Duration, signals: &Signals) -> Wake {
    let deadline = Instant::now() + dur;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Wake::Timeout;
        }
        if signals.interrupted() {
            return Wake::Interrupted;
        }
        if signals.winch.swap(false, Ordering::Relaxed) {
            return Wake::Resize;
        }
        std::thread::sleep(Duration::from_millis(25).min(deadline - now));
    }
}
