/*!
Ends the process when its terminal is gone and the main thread can't notice. crossterm's read loop
spins without returning on a tty whose other end closed, so the main loop's signal checks never run
again. A thread watches the signals, the terminal and a heartbeat the main thread stores, and exits
[`GRACE`] after the first sign.
*/

use std::io::{stdout, IsTerminal};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(250);
const STALE: Duration = Duration::from_secs(5);
pub const GRACE: Duration = Duration::from_secs(2);
/// 128 + SIGHUP, also for a closed terminal and a main thread that stopped beating.
const HANGUP: i32 = 129;
/// 128 + SIGTERM.
const TERMINATED: i32 = 143;
/// The longest the full restore may take before the process exits without it.
const RESTORE_WAIT: Duration = Duration::from_millis(500);

/// The moment beats count from.
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// Milliseconds from [`EPOCH`] to the main thread's last beat; 0 while not armed.
static BEAT: AtomicU64 = AtomicU64::new(0);
static PAUSED: AtomicUsize = AtomicUsize::new(0);
static PROBE: AtomicU8 = AtomicU8::new(Probe::None as u8);

/// How the watchdog tells that the terminal is still there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// The heartbeat and the signals only: the secret prompt, and the time outside a TUI.
    None,
    /// stdout also has to stay a terminal, because ratatui draws there.
    Stdout,
}

/// What the watchdog puts back before it exits.
#[derive(Debug, PartialEq, Eq)]
pub enum Restore {
    /// Everything a TUI changed: raw mode, the alternate screen, the cursor and mouse reports.
    Full,
    /// Raw mode only, a terminal setting, so nothing is written to a terminal that may be gone.
    RawOnly,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    Arm(Instant),
    Exit { code: i32, restore: Restore },
}

/// Starts the watchdog once per process and sets how it checks the terminal; the next beat arms the staleness check.
pub fn arm(probe: Probe) {
    PROBE.store(probe as u8, Ordering::Relaxed);
    watch_terminal();
}

/// Stops the staleness and terminal checks when a TUI leaves; the signals stay watched.
pub fn disarm() {
    BEAT.store(0, Ordering::Relaxed);
    PROBE.store(Probe::None as u8, Ordering::Relaxed);
}

/// Tells the watchdog the main thread is still going round its loop.
pub fn beat() {
    let epoch = EPOCH.get_or_init(Instant::now);
    let millis = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
    BEAT.store(millis.max(1), Ordering::Relaxed);
}

/// Holds off the staleness check around a main-thread call that may block longer than [`STALE`].
#[allow(dead_code, reason = "the $EDITOR handoff in a later wave holds it")]
pub struct Pause(());

#[allow(dead_code, reason = "the $EDITOR handoff in a later wave holds it")]
impl Pause {
    #[allow(
        clippy::new_without_default,
        reason = "a pause increments a counter, so it is not a default value"
    )]
    pub fn new() -> Self {
        PAUSED.fetch_add(1, Ordering::Relaxed);
        Pause(())
    }
}

impl Drop for Pause {
    fn drop(&mut self) {
        beat();
        PAUSED.fetch_sub(1, Ordering::Relaxed);
    }
}

fn probe() -> Probe {
    if PROBE.load(Ordering::Relaxed) == Probe::Stdout as u8 {
        Probe::Stdout
    } else {
        Probe::None
    }
}

fn beat_age(now: Instant) -> Option<Duration> {
    let beat = BEAT.load(Ordering::Relaxed);
    let epoch = EPOCH.get()?;
    (beat != 0).then(|| now.saturating_duration_since(*epoch + Duration::from_millis(beat)))
}

fn watch_terminal() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let hup = Arc::new(AtomicBool::new(false));
        let term = Arc::new(AtomicBool::new(false));
        let _ = signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&hup));
        let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term));
        thread::spawn(move || {
            let mut first = None;
            loop {
                thread::sleep(TICK);
                let now = Instant::now();
                let probe = probe();
                let step = watchdog_step(
                    probe == Probe::None || stdout().is_terminal(),
                    hup.load(Ordering::Relaxed),
                    term.load(Ordering::Relaxed),
                    beat_age(now),
                    PAUSED.load(Ordering::Relaxed) > 0,
                    first,
                    now,
                );
                match step {
                    Step::Wait => {}
                    Step::Arm(at) => first = Some(at),
                    Step::Exit { code, restore } => {
                        if restore == Restore::Full && probe == Probe::Stdout {
                            restore_then_exit(
                                code,
                                || crate::tui::leave(crate::tui::ENHANCED.load(Ordering::Relaxed)),
                                |code| std::process::exit(code),
                            );
                        } else {
                            let _ = crossterm::terminal::disable_raw_mode();
                        }
                        std::process::exit(code);
                    }
                }
            }
        });
    });
}

/**
Runs `restore` on a helper thread and calls `exit` once it returns or [`RESTORE_WAIT`] passes. The
full restore takes the stdout lock, which a main thread stuck mid-write still holds, so the exit
can't wait on it.
*/
fn restore_then_exit(code: i32, restore: impl FnOnce() + Send + 'static, exit: impl FnOnce(i32)) {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        restore();
        let _ = done.send(());
    });
    let _ = finished.recv_timeout(RESTORE_WAIT);
    exit(code);
}

/**
One check. A trigger is SIGHUP, SIGTERM, a terminal that is no longer `alive`, or a beat [`STALE`]
old while no [`Pause`] is held. The first trigger arms at `now`, and [`GRACE`] after `first` the
process exits, with 143 for SIGTERM and 129 otherwise. A signal on a terminal that still answers
gets the full restore; otherwise only raw mode is turned off.
*/
pub fn watchdog_step(
    alive: bool,
    hup: bool,
    term: bool,
    beat_age: Option<Duration>,
    paused: bool,
    first: Option<Instant>,
    now: Instant,
) -> Step {
    let stale = !paused && beat_age.is_some_and(|age| age >= STALE);
    match first {
        Some(at) if now.saturating_duration_since(at) >= GRACE => Step::Exit {
            code: if term { TERMINATED } else { HANGUP },
            restore: if (hup || term) && alive && !stale {
                Restore::Full
            } else {
                Restore::RawOnly
            },
        },
        Some(_) => Step::Wait,
        None if hup || term || !alive || stale => Step::Arm(now),
        None => Step::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first step after a trigger and the step [`GRACE`] later.
    fn armed_then(alive: bool, hup: bool, term: bool, beat_age: Option<Duration>) -> (Step, Step) {
        let now = Instant::now();
        let first = watchdog_step(alive, hup, term, beat_age, false, None, now);
        let last = watchdog_step(alive, hup, term, beat_age, false, Some(now), now + GRACE);
        (first, last)
    }

    fn exit(code: i32, restore: Restore) -> Step {
        Step::Exit { code, restore }
    }

    #[test]
    fn the_watchdog_waits_while_the_terminal_is_alive() {
        let now = Instant::now();
        let beat = Some(Duration::from_millis(4900));
        assert_eq!(
            watchdog_step(true, false, false, beat, false, None, now),
            Step::Wait
        );
        assert_eq!(
            watchdog_step(true, false, false, None, false, None, now),
            Step::Wait
        );
    }

    #[test]
    fn a_closed_terminal_exits_129_after_the_grace_without_writing() {
        let now = Instant::now();
        assert_eq!(
            watchdog_step(false, false, false, None, false, None, now),
            Step::Arm(now)
        );
        let soon = now + Duration::from_millis(1750);
        assert_eq!(
            watchdog_step(false, false, false, None, false, Some(now), soon),
            Step::Wait
        );
        let (_, last) = armed_then(false, false, false, None);
        assert_eq!(last, exit(129, Restore::RawOnly));
    }

    #[test]
    fn sigterm_exits_143_and_sighup_129_with_the_full_restore() {
        let (first, last) = armed_then(true, false, true, None);
        assert!(matches!(first, Step::Arm(_)));
        assert_eq!(last, exit(143, Restore::Full));
        let (first, last) = armed_then(true, true, false, None);
        assert!(matches!(first, Step::Arm(_)));
        assert_eq!(last, exit(129, Restore::Full));
        let (_, last) = armed_then(false, true, false, None);
        assert_eq!(last, exit(129, Restore::RawOnly));
    }

    #[test]
    fn a_restore_that_never_returns_still_exits_in_time() {
        let (exited, codes) = mpsc::channel();
        let start = Instant::now();
        restore_then_exit(
            129,
            || loop {
                thread::park();
            },
            move |code| {
                let _ = exited.send((code, start.elapsed()));
            },
        );
        let (code, after) = codes.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(code, 129);
        assert!(after < Duration::from_millis(600), "{after:?}");
    }

    #[test]
    fn a_beat_five_seconds_old_exits_129_unless_paused() {
        let stale = Some(Duration::from_secs(5));
        let (first, last) = armed_then(true, false, false, stale);
        assert!(matches!(first, Step::Arm(_)));
        assert_eq!(last, exit(129, Restore::RawOnly));
        let now = Instant::now();
        for age in [5, 60, 3600] {
            let age = Some(Duration::from_secs(age));
            assert_eq!(
                watchdog_step(true, false, false, age, true, None, now),
                Step::Wait
            );
        }
    }
}
