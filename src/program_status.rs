//! Bounded, content-free OSC 7501 reports for native terminal hosts such as Rex.

use std::io::{self, IsTerminal, Write};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, ThreadId};

use base64::{engine::general_purpose::STANDARD, Engine};

use crate::net::Network;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Command,
    Loading,
    Posting,
    SigningIn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Auth,
    Permission,
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Idle,
    Working(Activity),
    Done,
    Error,
    Blocked(Kind),
}

impl Status {
    fn fields(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Idle => ("idle", "", "Ready"),
            Self::Working(Activity::Command) => ("working", "", "Running command"),
            Self::Working(Activity::Loading) => ("working", "", "Loading posts"),
            Self::Working(Activity::Posting) => ("working", "", "Publishing post"),
            Self::Working(Activity::SigningIn) => ("working", "", "Signing in"),
            Self::Done => ("done", "", "Command finished"),
            Self::Error => ("error", "", "Command failed"),
            Self::Blocked(Kind::Auth) => ("blocked", ":kind=auth", "Complete sign-in"),
            Self::Blocked(Kind::Permission) => ("blocked", ":kind=permission", "Confirm action"),
            Self::Blocked(Kind::Question) => ("blocked", ":kind=question", "Answer required"),
        }
    }

    fn sequence(self, progress: Option<u8>) -> String {
        let (state, kind, message) = self.fields();
        let progress = progress.map_or_else(String::new, |value| format!(":progress={value}"));
        format!(
            "\x1b]7501;state={state}:app=socli{kind}{progress}:title={}:msg={}\x1b\\",
            STANDARD.encode("SoCLI"),
            STANDARD.encode(message)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskKind {
    Read,
    Post,
    Action,
    Auth,
}

impl TaskKind {
    const ALL: [Self; 4] = [Self::Read, Self::Post, Self::Action, Self::Auth];

    fn index(self) -> usize {
        match self {
            Self::Read => 0,
            Self::Post => 1,
            Self::Action => 2,
            Self::Auth => 3,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Post => "post",
            Self::Action => "action",
            Self::Auth => "auth",
        }
    }

    fn message(self, state: TaskState) -> &'static str {
        match (self, state) {
            (_, TaskState::Blocked(kind)) => Status::Blocked(kind).fields().2,
            (Self::Read, TaskState::Working) => "Loading posts",
            (Self::Read, TaskState::Done) => "Posts loaded",
            (Self::Read, TaskState::Error) => "Read failed",
            (Self::Post, TaskState::Working) => "Publishing post",
            (Self::Post, TaskState::Done) => "Post published",
            (Self::Post, TaskState::Error) => "Post not confirmed",
            (Self::Action, TaskState::Working) => "Updating account",
            (Self::Action, TaskState::Done) => "Action completed",
            (Self::Action, TaskState::Error) => "Action not confirmed",
            (Self::Auth, TaskState::Working) => "Signing in",
            (Self::Auth, TaskState::Done) => "Signed in",
            (Self::Auth, TaskState::Error) => "Sign-in failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskState {
    Working,
    Blocked(Kind),
    Done,
    Error,
}

impl TaskState {
    fn complete(self) -> bool {
        matches!(self, Self::Done | Self::Error)
    }

    fn fields(self) -> (&'static str, &'static str) {
        match self {
            Self::Working => ("working", ""),
            Self::Blocked(Kind::Auth) => ("blocked", ":kind=auth"),
            Self::Blocked(Kind::Permission) => ("blocked", ":kind=permission"),
            Self::Blocked(Kind::Question) => ("blocked", ":kind=question"),
            Self::Done => ("done", ""),
            Self::Error => ("error", ""),
        }
    }
}

const TASK_SLOTS: usize = Network::ALL.len() * TaskKind::ALL.len();

#[derive(Clone, Copy)]
struct TaskSlot {
    generation: Option<u64>,
    state: Option<TaskState>,
    owner: Option<ThreadId>,
}

impl TaskSlot {
    const EMPTY: Self = Self {
        generation: None,
        state: None,
        owner: None,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TaskToken {
    slot: usize,
    generation: u64,
}

fn task_slot(net: Network, kind: TaskKind) -> usize {
    let network = match net {
        Network::X => 0,
        Network::Bluesky => 1,
        Network::Devnull => 2,
        Network::Threads => 3,
        Network::Mastodon => 4,
    };
    network * TaskKind::ALL.len() + kind.index()
}

fn task_identity(slot: usize) -> (Network, TaskKind) {
    (
        Network::ALL[slot / TaskKind::ALL.len()],
        TaskKind::ALL[slot % TaskKind::ALL.len()],
    )
}

fn task_sequence(slot: usize, state: TaskState) -> String {
    let (net, kind) = task_identity(slot);
    let (state_name, blocked) = state.fields();
    let title = format!("{} {}", net.title(), kind.slug());
    let message = format!("{}: {}", net.title(), kind.message(state));
    format!(
        "\x1b]7501;state={state_name}:id=network/{}/{}:app=socli{blocked}:title={}:msg={}\x1b\\",
        net.slug(),
        kind.slug(),
        STANDARD.encode(title),
        STANDARD.encode(message)
    )
}

fn clear_sequence(slot: usize) -> String {
    let (net, kind) = task_identity(slot);
    format!(
        "\x1b]7501;state=clear:id=network/{}/{}:app=socli:title={}\x1b\\",
        net.slug(),
        kind.slug(),
        STANDARD.encode(format!("{} {}", net.title(), kind.slug()))
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RootReport {
    status: Status,
    progress: Option<u8>,
}

struct BatchState {
    generation: u64,
    total: u8,
    completed: u8,
}

impl BatchState {
    fn progress(&self) -> u8 {
        ((u16::from(self.completed) * 100) / u16::from(self.total)) as u8
    }
}

struct Reporter {
    enabled: bool,
    last: Option<RootReport>,
    cancelled: bool,
    cancel_generation: Option<u64>,
    waits: usize,
    tasks: [TaskSlot; TASK_SLOTS],
    task_generation: u64,
    batch: Option<BatchState>,
    batch_generation: u64,
}

impl Reporter {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            last: None,
            cancelled: false,
            cancel_generation: Some(0),
            waits: 0,
            tasks: [TaskSlot::EMPTY; TASK_SLOTS],
            task_generation: 0,
            batch: None,
            batch_generation: 0,
        }
    }

    fn emit(&mut self, status: Status, out: &mut impl Write) {
        if self.cancelled && status != Status::Idle {
            return;
        }
        let progress = if matches!(status, Status::Working(Activity::Loading)) {
            self.batch.as_ref().map(BatchState::progress)
        } else {
            None
        };
        let report = RootReport { status, progress };
        if self.last == Some(report) {
            return;
        }
        self.last = Some(report);
        if self.enabled {
            self.write(&status.sequence(progress), out);
        }
    }

    fn write(&mut self, sequence: &str, out: &mut impl Write) {
        if self.enabled
            && out
                .write_all(sequence.as_bytes())
                .and_then(|()| out.flush())
                .is_err()
        {
            self.enabled = false;
        }
    }

    fn finish(&mut self, success: bool, out: &mut impl Write) {
        self.batch = None;
        let status = if self.cancelled {
            Status::Idle
        } else if success {
            Status::Done
        } else {
            Status::Error
        };
        self.emit(status, out);
    }

    fn set(&mut self, status: Status, out: &mut impl Write) {
        if self.waits > 0 && matches!(status, Status::Working(_) | Status::Idle) {
            return;
        }
        self.emit(status, out);
    }

    fn start_task(
        &mut self,
        net: Network,
        kind: TaskKind,
        owner: ThreadId,
        out: &mut impl Write,
    ) -> Option<TaskToken> {
        if self.cancelled {
            return None;
        }
        let slot = task_slot(net, kind);
        let Some(generation) = self.task_generation.checked_add(1) else {
            self.clear_task(slot, out);
            return None;
        };
        self.task_generation = generation;
        self.tasks[slot] = TaskSlot {
            generation: Some(generation),
            state: Some(TaskState::Working),
            owner: Some(owner),
        };
        if self.enabled {
            self.write(&task_sequence(slot, TaskState::Working), out);
        }
        Some(TaskToken { slot, generation })
    }

    fn emit_task(&mut self, token: TaskToken, state: TaskState, out: &mut impl Write) {
        if self.cancelled {
            return;
        }
        let slot = &mut self.tasks[token.slot];
        if slot.generation != Some(token.generation)
            || slot.state.is_none()
            || slot.state == Some(state)
        {
            return;
        }
        slot.state = Some(state);
        if self.enabled {
            self.write(&task_sequence(token.slot, state), out);
        }
    }

    fn clear_task(&mut self, index: usize, out: &mut impl Write) {
        let occupied = self.tasks[index].state.is_some();
        self.tasks[index] = TaskSlot::EMPTY;
        if occupied && self.enabled {
            self.write(&clear_sequence(index), out);
        }
    }

    fn acknowledge(&mut self, out: &mut impl Write) {
        self.cancelled = false;
        for index in 0..self.tasks.len() {
            if self.tasks[index].state.is_some_and(TaskState::complete) {
                self.clear_task(index, out);
            }
        }
    }

    fn cancel(&mut self, out: &mut impl Write) {
        self.cancelled = true;
        self.cancel_generation = self
            .cancel_generation
            .and_then(|generation| generation.checked_add(1));
        self.batch = None;
        for index in 0..self.tasks.len() {
            self.clear_task(index, out);
        }
        self.emit(Status::Idle, out);
    }

    fn start_batch(&mut self, total: usize, out: &mut impl Write) -> Option<u64> {
        if self.cancelled || total == 0 || total > Network::ALL.len() {
            return None;
        }
        let Some(generation) = self.batch_generation.checked_add(1) else {
            self.batch = None;
            self.refresh_root(out);
            return None;
        };
        self.batch_generation = generation;
        self.batch = Some(BatchState {
            generation,
            total: total as u8,
            completed: 0,
        });
        if self.waits == 0
            && !self
                .last
                .is_some_and(|report| matches!(report.status, Status::Blocked(_)))
        {
            self.emit(Status::Working(Activity::Loading), out);
        } else {
            self.refresh_root(out);
        }
        Some(generation)
    }

    fn advance_batch(&mut self, generation: u64, out: &mut impl Write) {
        let Some(batch) = self
            .batch
            .as_mut()
            .filter(|batch| batch.generation == generation)
        else {
            return;
        };
        batch.completed = batch.completed.saturating_add(1).min(batch.total);
        self.refresh_root(out);
    }

    fn end_batch(&mut self, generation: u64, out: &mut impl Write) {
        if self
            .batch
            .as_ref()
            .is_some_and(|batch| batch.generation == generation)
        {
            self.batch = None;
            self.refresh_root(out);
        }
    }

    fn refresh_root(&mut self, out: &mut impl Write) {
        if let Some(report) = self.last {
            self.emit(report.status, out);
        }
    }

    fn wait_task(
        &mut self,
        owner: ThreadId,
        kind: Kind,
        out: &mut impl Write,
    ) -> Option<(TaskToken, TaskState)> {
        if self.cancelled {
            return None;
        }
        let candidate = self
            .tasks
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let state = slot.state?;
                let generation = slot.generation?;
                if task_identity(index).1 == TaskKind::Auth
                    && slot.owner == Some(owner)
                    && !state.complete()
                {
                    Some((
                        TaskToken {
                            slot: index,
                            generation,
                        },
                        state,
                    ))
                } else {
                    None
                }
            })
            .max_by_key(|(token, _)| token.generation);
        if let Some((token, _)) = candidate {
            self.emit_task(token, TaskState::Blocked(kind), out);
        }
        candidate
    }

    fn start_wait(&mut self, kind: Kind, owner: ThreadId, out: &mut impl Write) -> WaitState {
        let previous = self
            .last
            .map_or(Status::Working(Activity::Command), |report| report.status);
        self.waits = self.waits.saturating_add(1);
        let generation = if self.cancelled {
            None
        } else {
            self.cancel_generation
        };
        let child = self.wait_task(owner, kind, out);
        self.emit(Status::Blocked(kind), out);
        WaitState {
            previous,
            kind,
            child,
            generation,
        }
    }

    fn end_wait(&mut self, waiting: &WaitState, out: &mut impl Write) {
        self.waits = self.waits.saturating_sub(1);
        // A cancelled guard must not restore its prompt after new input resumes the reader.
        if self.cancelled
            || waiting.generation.is_none()
            || waiting.generation != self.cancel_generation
        {
            return;
        }
        if let Some((token, previous)) = waiting.child {
            if self.tasks[token.slot].state == Some(TaskState::Blocked(waiting.kind)) {
                self.emit_task(token, previous, out);
            }
        }
        if self
            .last
            .is_some_and(|report| report.status == Status::Blocked(waiting.kind))
        {
            self.emit(waiting.previous, out);
        }
    }

    fn acknowledge_wait(&mut self, waiting: &mut WaitState, owner: ThreadId, out: &mut impl Write) {
        let current = !self.cancelled
            && waiting.generation.is_some()
            && waiting.generation == self.cancel_generation;
        self.acknowledge(out);
        if !current {
            waiting.previous = Status::Working(if waiting.kind == Kind::Auth {
                Activity::SigningIn
            } else {
                Activity::Command
            });
        }
        let child = self.wait_task(owner, waiting.kind, out);
        waiting.child = match (child, waiting.child) {
            (Some((token, TaskState::Blocked(kind))), Some((previous_token, previous)))
                if current && token == previous_token && kind == waiting.kind =>
            {
                Some((token, previous))
            }
            (child, _) => child,
        };
        waiting.generation = self.cancel_generation;
        self.emit(Status::Blocked(waiting.kind), out);
    }
}

fn enabled(value: Option<&str>, terminal: bool, agent: bool, dumb: bool) -> bool {
    match value.map(str::trim) {
        Some("on" | "1") => true,
        Some("off" | "0") => false,
        _ => terminal && !agent && !dumb,
    }
}

fn reporter() -> &'static Mutex<Reporter> {
    static REPORTER: OnceLock<Mutex<Reporter>> = OnceLock::new();
    REPORTER.get_or_init(|| {
        Mutex::new(Reporter::new(enabled(
            std::env::var("SOCLI_PROGRAM_STATUS").ok().as_deref(),
            io::stderr().is_terminal(),
            crate::agent::active(),
            std::env::var("TERM").is_ok_and(|value| value == "dumb"),
        )))
    })
}

fn update<T>(f: impl FnOnce(&mut Reporter, &mut io::StderrLock<'_>) -> T) -> T {
    let mut reporter = reporter().lock().unwrap_or_else(|err| err.into_inner());
    f(&mut reporter, &mut io::stderr().lock())
}

pub(crate) fn set(status: Status) {
    update(|reporter, out| reporter.set(status, out));
}

pub(crate) fn working(activity: Activity) {
    update(|reporter, out| {
        // A slow key worker may be prompting while the parent still paints its busy footer.
        if reporter.waits == 0 {
            reporter.set(Status::Working(activity), out);
        }
    });
}

pub(crate) fn cancel() {
    update(|reporter, out| reporter.cancel(out));
}

pub(crate) fn finish(success: bool) {
    update(|reporter, out| reporter.finish(success, out));
}

pub(crate) fn acknowledge() {
    update(|reporter, out| reporter.acknowledge(out));
}

#[must_use = "retain the task until the operation returns"]
pub(crate) struct Task {
    token: Option<TaskToken>,
}

pub(crate) fn task(net: Network, kind: TaskKind) -> Task {
    let owner = thread::current().id();
    Task {
        token: update(|reporter, out| reporter.start_task(net, kind, owner, out)),
    }
}

impl Task {
    pub(crate) fn finish<T, E>(mut self, result: Result<T, E>) -> Result<T, E> {
        self.complete(result.is_ok());
        result
    }

    fn complete(&mut self, success: bool) {
        if let Some(token) = self.token.take() {
            update(|reporter, out| {
                reporter.emit_task(
                    token,
                    if success {
                        TaskState::Done
                    } else {
                        TaskState::Error
                    },
                    out,
                );
            });
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.complete(false);
    }
}

#[must_use = "retain the batch while network loads are running"]
pub(crate) struct Batch {
    generation: Option<u64>,
}

pub(crate) fn batch(total: usize) -> Batch {
    Batch {
        generation: update(|reporter, out| reporter.start_batch(total, out)),
    }
}

impl Batch {
    pub(crate) fn advance(&self) {
        if let Some(generation) = self.generation {
            update(|reporter, out| reporter.advance_batch(generation, out));
        }
    }

    pub(crate) fn finish(mut self) {
        self.complete();
    }

    fn complete(&mut self) {
        if let Some(generation) = self.generation.take() {
            update(|reporter, out| reporter.end_batch(generation, out));
        }
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        self.complete();
    }
}

struct WaitState {
    previous: Status,
    kind: Kind,
    child: Option<(TaskToken, TaskState)>,
    generation: Option<u64>,
}

pub(crate) struct Waiting {
    state: WaitState,
}

pub(crate) fn waiting(kind: Kind) -> Waiting {
    let owner = thread::current().id();
    Waiting {
        state: update(|reporter, out| reporter.start_wait(kind, owner, out)),
    }
}

impl Waiting {
    pub(crate) fn acknowledge(&mut self) {
        let owner = thread::current().id();
        update(|reporter, out| reporter.acknowledge_wait(&mut self.state, owner, out));
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        update(|reporter, out| reporter.end_wait(&self.state, out));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reports(out: &[u8]) -> Vec<std::collections::HashMap<&str, &str>> {
        std::str::from_utf8(out)
            .unwrap()
            .split("\x1b]7501;")
            .skip(1)
            .map(|tail| {
                tail.strip_suffix("\x1b\\")
                    .unwrap()
                    .split(':')
                    .map(|pair| pair.split_once('=').unwrap())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn every_report_is_complete_and_fits_protocol_limits() {
        for status in [
            Status::Idle,
            Status::Working(Activity::Command),
            Status::Working(Activity::Loading),
            Status::Working(Activity::Posting),
            Status::Working(Activity::SigningIn),
            Status::Done,
            Status::Error,
            Status::Blocked(Kind::Auth),
            Status::Blocked(Kind::Permission),
            Status::Blocked(Kind::Question),
        ] {
            let sequence = status.sequence(None);
            assert!(sequence.len() <= 4096);
            let body = sequence
                .strip_prefix("\x1b]7501;")
                .unwrap()
                .strip_suffix("\x1b\\")
                .unwrap();
            assert!(body.contains(":app=socli:"));
            let title = body
                .split(":title=")
                .nth(1)
                .unwrap()
                .split(':')
                .next()
                .unwrap();
            assert_eq!(STANDARD.decode(title).unwrap(), b"SoCLI");
            let encoded = body.split(":msg=").nth(1).unwrap();
            assert!(encoded.len() <= 2732);
            let message = String::from_utf8(STANDARD.decode(encoded).unwrap()).unwrap();
            assert!(message.len() <= 2048);
            assert!(!message.chars().any(char::is_control));
            assert_eq!(message, status.fields().2);
            assert_eq!(
                body.contains(":kind="),
                matches!(status, Status::Blocked(_))
            );
        }
    }

    #[test]
    fn transitions_are_deduplicated_and_replace_blocked_keys() {
        let mut reporter = Reporter::new(true);
        let mut out = Vec::new();
        reporter.emit(Status::Blocked(Kind::Auth), &mut out);
        let blocked = out.len();
        reporter.emit(Status::Blocked(Kind::Auth), &mut out);
        assert_eq!(out.len(), blocked);
        reporter.emit(Status::Working(Activity::SigningIn), &mut out);
        assert_eq!(
            &out[blocked..],
            Status::Working(Activity::SigningIn)
                .sequence(None)
                .as_bytes()
        );
        assert!(!String::from_utf8_lossy(&out[blocked..]).contains("kind="));
    }

    #[test]
    fn cancellation_survives_finalization() {
        let mut reporter = Reporter::new(true);
        let mut out = Vec::new();
        reporter.cancelled = true;
        reporter.finish(true, &mut out);
        reporter.finish(false, &mut out);
        assert_eq!(out, Status::Idle.sequence(None).as_bytes());
    }

    #[test]
    fn disabled_or_failed_output_does_not_affect_the_command() {
        let mut reporter = Reporter::new(false);
        let mut out = Vec::new();
        reporter.emit(Status::Done, &mut out);
        assert!(out.is_empty());
        reporter.enabled = true;
        reporter.emit(Status::Error, &mut io::Cursor::new([0_u8; 0]));
        assert!(!reporter.enabled);
    }

    #[test]
    fn automatic_output_is_terminal_only_and_overrides_are_explicit() {
        assert!(enabled(None, true, false, false));
        assert!(!enabled(None, false, false, false));
        assert!(!enabled(Some("auto"), true, true, false));
        assert!(!enabled(None, true, false, true));
        assert!(!enabled(Some("off"), true, false, false));
        assert!(enabled(Some("on"), false, true, true));
        assert!(!enabled(Some("invalid"), false, false, false));
    }

    #[test]
    fn child_records_use_only_fixed_identities_and_fit_protocol_limits() {
        for net in Network::ALL {
            for kind in TaskKind::ALL {
                let slot = task_slot(net, kind);
                assert_eq!(task_identity(slot), (net, kind));
                for state in [
                    TaskState::Working,
                    TaskState::Blocked(Kind::Auth),
                    TaskState::Blocked(Kind::Question),
                    TaskState::Blocked(Kind::Permission),
                    TaskState::Done,
                    TaskState::Error,
                ] {
                    let sequence = task_sequence(slot, state);
                    assert!(sequence.len() <= 4096);
                    let parsed = reports(sequence.as_bytes());
                    let report = &parsed[0];
                    assert_eq!(report["app"], "socli");
                    assert_eq!(
                        report["id"],
                        format!("network/{}/{}", net.slug(), kind.slug())
                    );
                    assert!(report["id"].len() <= 128);
                    assert!(report["id"].split('/').count() <= 8);
                    for segment in report["id"].split('/') {
                        assert!(!segment.is_empty() && segment.len() <= 32);
                        assert!(segment
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte)));
                    }
                    for (key, encoded, decoded) in [("title", 256, 192), ("msg", 2732, 2048)] {
                        assert!(report[key].len() <= encoded);
                        let text =
                            String::from_utf8(STANDARD.decode(report[key]).unwrap()).unwrap();
                        assert!(text.len() <= decoded);
                        assert!(!text.chars().any(char::is_control));
                    }
                    let title = STANDARD.decode(report["title"]).unwrap();
                    assert_eq!(
                        String::from_utf8(title).unwrap(),
                        format!("{} {}", net.title(), kind.slug())
                    );
                    assert_eq!(
                        report.contains_key("kind"),
                        matches!(state, TaskState::Blocked(_))
                    );
                }
            }
        }
    }

    #[test]
    fn repeated_tasks_reuse_twenty_slots_and_old_guards_cannot_overwrite_new_ones() {
        let mut reporter = Reporter::new(true);
        let mut sink = io::sink();
        let owner = thread::current().id();
        for _ in 0..64 {
            for net in Network::ALL {
                for kind in TaskKind::ALL {
                    reporter.start_task(net, kind, owner, &mut sink);
                }
            }
        }
        assert_eq!(reporter.tasks.len(), 20);
        assert_eq!(
            reporter
                .tasks
                .iter()
                .filter(|slot| slot.state.is_some())
                .count(),
            20
        );
        let mut out = Vec::new();
        let old = reporter
            .start_task(Network::Bluesky, TaskKind::Read, owner, &mut out)
            .unwrap();
        let current = reporter
            .start_task(Network::Bluesky, TaskKind::Read, owner, &mut out)
            .unwrap();
        let before = out.len();
        reporter.emit_task(old, TaskState::Done, &mut out);
        assert_eq!(out.len(), before);
        reporter.emit_task(current, TaskState::Error, &mut out);
        assert_eq!(reporter.tasks[current.slot].state, Some(TaskState::Error));
        assert_eq!(reports(&out).last().unwrap()["state"], "error");
    }

    #[test]
    fn root_completion_keeps_children_and_acknowledgment_clears_only_finished_records() {
        let mut reporter = Reporter::new(true);
        let owner = thread::current().id();
        let mut out = Vec::new();
        let active = reporter
            .start_task(Network::X, TaskKind::Auth, owner, &mut out)
            .unwrap();
        let complete = reporter
            .start_task(Network::Mastodon, TaskKind::Read, owner, &mut out)
            .unwrap();
        reporter.emit_task(complete, TaskState::Error, &mut out);
        reporter.finish(false, &mut out);
        assert_eq!(reporter.tasks[complete.slot].state, Some(TaskState::Error));
        let before = out.len();
        reporter.acknowledge(&mut out);
        let cleared = reports(&out[before..]);
        assert_eq!(cleared.len(), 1);
        assert_eq!(cleared[0]["state"], "clear");
        assert_eq!(cleared[0]["id"], "network/mastodon/read");
        assert!(cleared[0].contains_key("app") && cleared[0].contains_key("title"));
        assert_eq!(reporter.tasks[active.slot].state, Some(TaskState::Working));
        assert!(reporter.tasks[complete.slot].state.is_none());
        reporter.emit_task(active, TaskState::Done, &mut out);
        assert_eq!(reporter.tasks[active.slot].state, Some(TaskState::Done));
    }

    #[test]
    fn cancellation_clears_owned_records_and_invalidates_task_and_batch_updates() {
        let mut reporter = Reporter::new(true);
        let owner = thread::current().id();
        let mut out = Vec::new();
        let active = reporter
            .start_task(Network::Bluesky, TaskKind::Auth, owner, &mut out)
            .unwrap();
        let complete = reporter
            .start_task(Network::X, TaskKind::Post, owner, &mut out)
            .unwrap();
        reporter.emit_task(complete, TaskState::Done, &mut out);
        let batch = reporter.start_batch(2, &mut out).unwrap();
        reporter.wait_task(owner, Kind::Auth, &mut out);
        let before = out.len();
        reporter.cancel(&mut out);
        let cancelled = reports(&out[before..]);
        assert_eq!(
            cancelled
                .iter()
                .filter(|report| report["state"] == "clear")
                .count(),
            2
        );
        assert!(cancelled
            .iter()
            .filter(|report| report["state"] == "clear")
            .all(|report| report
                .get("id")
                .is_some_and(|id| id.starts_with("network/"))));
        assert_eq!(cancelled.last().unwrap()["state"], "idle");
        assert!(reporter.tasks.iter().all(|slot| slot.state.is_none()));
        let after_cancel = out.len();
        reporter.emit_task(active, TaskState::Error, &mut out);
        reporter.emit_task(active, TaskState::Working, &mut out);
        reporter.advance_batch(batch, &mut out);
        reporter.end_batch(batch, &mut out);
        reporter.finish(false, &mut out);
        assert_eq!(out.len(), after_cancel);
    }

    #[test]
    fn cancellation_rejects_late_work_until_user_input_rearms_the_wait() {
        let mut reporter = Reporter::new(true);
        let owner = thread::current().id();
        let mut out = Vec::new();
        reporter.set(Status::Working(Activity::Loading), &mut out);
        reporter.cancel(&mut out);
        let cancelled = out.len();
        for kind in TaskKind::ALL {
            assert!(reporter
                .start_task(Network::Bluesky, kind, owner, &mut out)
                .is_none());
        }
        assert!(reporter.start_batch(2, &mut out).is_none());
        for status in [
            Status::Working(Activity::Command),
            Status::Working(Activity::Loading),
            Status::Working(Activity::Posting),
            Status::Working(Activity::SigningIn),
            Status::Blocked(Kind::Auth),
            Status::Done,
            Status::Error,
        ] {
            reporter.set(status, &mut out);
        }
        let mut waiting = reporter.start_wait(Kind::Auth, owner, &mut out);
        assert_eq!(reporter.waits, 1);
        reporter.finish(false, &mut out);
        assert_eq!(out.len(), cancelled);
        assert!(reporter.cancelled);
        reporter.acknowledge(&mut out);
        reporter.set(Status::Working(Activity::Loading), &mut out);
        assert_eq!(out.len(), cancelled);
        reporter.acknowledge_wait(&mut waiting, owner, &mut out);
        assert_eq!(reports(&out).last().unwrap()["state"], "blocked");
        let blocked = out.len();
        reporter.set(Status::Working(Activity::Loading), &mut out);
        assert_eq!(out.len(), blocked);
        reporter.end_wait(&waiting, &mut out);
        assert_eq!(reporter.waits, 0);
        assert_eq!(
            reporter.last.unwrap().status,
            Status::Working(Activity::SigningIn)
        );
        assert!(reporter
            .start_task(Network::Bluesky, TaskKind::Auth, owner, &mut out)
            .is_some());
        assert!(reporter.start_batch(2, &mut out).is_some());
        assert_eq!(reports(&out).last().unwrap()["progress"], "0");
    }

    #[test]
    fn cancelled_waits_cannot_restore_over_a_new_prompt_after_acknowledgment() {
        let mut reporter = Reporter::new(true);
        let owner = thread::current().id();
        let mut out = Vec::new();
        reporter.set(Status::Working(Activity::SigningIn), &mut out);
        let old_task = reporter
            .start_task(Network::Bluesky, TaskKind::Auth, owner, &mut out)
            .unwrap();
        let old_wait = reporter.start_wait(Kind::Auth, owner, &mut out);
        reporter.cancel(&mut out);
        reporter.acknowledge(&mut out);
        let new_task = reporter
            .start_task(Network::Bluesky, TaskKind::Auth, owner, &mut out)
            .unwrap();
        let new_wait = reporter.start_wait(Kind::Auth, owner, &mut out);
        let before = out.len();
        reporter.end_wait(&old_wait, &mut out);
        reporter.emit_task(old_task, TaskState::Done, &mut out);
        assert_eq!(out.len(), before);
        assert_eq!(reporter.waits, 1);
        assert_eq!(reporter.last.unwrap().status, Status::Blocked(Kind::Auth));
        assert_eq!(
            reporter.tasks[new_task.slot].state,
            Some(TaskState::Blocked(Kind::Auth))
        );
        reporter.end_wait(&new_wait, &mut out);
        assert_eq!(reporter.waits, 0);
        assert_eq!(
            reporter.tasks[new_task.slot].state,
            Some(TaskState::Working)
        );
        assert_eq!(reporter.last.unwrap().status, Status::Idle);
    }

    #[test]
    fn repeated_prompt_input_preserves_the_working_root_and_child_restore_states() {
        let mut reporter = Reporter::new(true);
        let owner = thread::current().id();
        let mut out = Vec::new();
        reporter.set(Status::Working(Activity::SigningIn), &mut out);
        let task = reporter
            .start_task(Network::Bluesky, TaskKind::Auth, owner, &mut out)
            .unwrap();
        let mut waiting = reporter.start_wait(Kind::Auth, owner, &mut out);
        let before = out.len();
        for _ in 0..3 {
            reporter.acknowledge_wait(&mut waiting, owner, &mut out);
        }
        assert_eq!(out.len(), before);
        reporter.end_wait(&waiting, &mut out);
        assert_eq!(
            reporter.last.unwrap().status,
            Status::Working(Activity::SigningIn)
        );
        assert_eq!(reporter.tasks[task.slot].state, Some(TaskState::Working));
        let root = reports(&out).pop().unwrap();
        assert_eq!(root["state"], "working");
        assert!(!root.contains_key("kind"));
    }

    #[test]
    fn auth_waits_follow_the_latest_task_on_the_current_thread() {
        let mut reporter = Reporter::new(true);
        let owner = thread::current().id();
        let other = thread::spawn(|| thread::current().id()).join().unwrap();
        let mut out = Vec::new();
        let older = reporter
            .start_task(Network::X, TaskKind::Auth, owner, &mut out)
            .unwrap();
        let auth = reporter
            .start_task(Network::Bluesky, TaskKind::Auth, owner, &mut out)
            .unwrap();
        reporter.start_task(Network::Mastodon, TaskKind::Auth, other, &mut out);
        let (waiting, previous) = reporter.wait_task(owner, Kind::Auth, &mut out).unwrap();
        assert_eq!(waiting, auth);
        assert_eq!(previous, TaskState::Working);
        assert_eq!(
            reporter.tasks[auth.slot].state,
            Some(TaskState::Blocked(Kind::Auth))
        );
        assert_eq!(reporter.tasks[older.slot].state, Some(TaskState::Working));
        let newer = reporter
            .start_task(Network::Bluesky, TaskKind::Auth, owner, &mut out)
            .unwrap();
        let before = out.len();
        reporter.emit_task(waiting, previous, &mut out);
        assert_eq!(out.len(), before);
        reporter.emit_task(newer, TaskState::Done, &mut out);
        let report = reports(&out).pop().unwrap();
        assert_eq!(report["state"], "done");
        assert!(!report.contains_key("kind"));
    }

    #[test]
    fn batch_progress_counts_completed_loads_and_survives_loading_frames() {
        let mut reporter = Reporter::new(true);
        let mut out = Vec::new();
        let batch = reporter.start_batch(3, &mut out).unwrap();
        assert_eq!(reports(&out).last().unwrap()["progress"], "0");
        reporter.advance_batch(batch, &mut out);
        assert_eq!(reports(&out).last().unwrap()["progress"], "33");
        let before = out.len();
        reporter.set(Status::Working(Activity::Loading), &mut out);
        assert_eq!(out.len(), before);
        reporter.advance_batch(batch, &mut out);
        reporter.advance_batch(batch, &mut out);
        assert_eq!(reports(&out).last().unwrap()["progress"], "100");
        let complete = out.len();
        reporter.advance_batch(batch, &mut out);
        assert_eq!(out.len(), complete);
        reporter.end_batch(batch, &mut out);
        assert!(!reports(&out).last().unwrap().contains_key("progress"));
    }

    #[test]
    fn batch_progress_preserves_prompt_state_and_drops_outside_measured_work() {
        let mut reporter = Reporter::new(true);
        let mut out = Vec::new();
        let batch = reporter.start_batch(2, &mut out).unwrap();
        reporter.waits = 1;
        reporter.emit(Status::Blocked(Kind::Auth), &mut out);
        reporter.advance_batch(batch, &mut out);
        let parsed = reports(&out);
        let blocked = parsed.last().unwrap();
        assert_eq!(blocked["state"], "blocked");
        assert_eq!(blocked["kind"], "auth");
        assert!(!blocked.contains_key("progress"));
        let before = out.len();
        reporter.set(Status::Working(Activity::Loading), &mut out);
        reporter.set(Status::Idle, &mut out);
        assert_eq!(out.len(), before);
        reporter.waits = 0;
        reporter.set(Status::Working(Activity::Loading), &mut out);
        assert_eq!(reports(&out).last().unwrap()["progress"], "50");
        reporter.emit(Status::Blocked(Kind::Auth), &mut out);
        reporter.end_batch(batch, &mut out);
        let parsed = reports(&out);
        let blocked = parsed.last().unwrap();
        assert_eq!(blocked["state"], "blocked");
        assert_eq!(blocked["kind"], "auth");
        assert!(!blocked.contains_key("progress"));
        reporter.set(Status::Working(Activity::Posting), &mut out);
        assert!(!reports(&out).last().unwrap().contains_key("progress"));
    }

    #[test]
    fn invalid_and_stale_batches_cannot_invent_or_overwrite_progress() {
        let mut reporter = Reporter::new(true);
        let mut out = Vec::new();
        assert!(reporter.start_batch(0, &mut out).is_none());
        assert!(reporter
            .start_batch(Network::ALL.len() + 1, &mut out)
            .is_none());
        assert!(out.is_empty());
        let old = reporter.start_batch(3, &mut out).unwrap();
        reporter.advance_batch(old, &mut out);
        let current = reporter.start_batch(2, &mut out).unwrap();
        let before = out.len();
        reporter.advance_batch(old, &mut out);
        reporter.end_batch(old, &mut out);
        assert_eq!(out.len(), before);
        reporter.advance_batch(current, &mut out);
        assert_eq!(reports(&out).last().unwrap()["progress"], "50");
        reporter.finish(true, &mut out);
        let finished = out.len();
        reporter.advance_batch(current, &mut out);
        assert_eq!(out.len(), finished);
        assert_eq!(reports(&out).last().unwrap()["state"], "done");
    }

    #[test]
    fn exhausted_generations_disable_updates_without_reusing_live_tokens() {
        let mut reporter = Reporter::new(true);
        let mut out = Vec::new();
        let owner = thread::current().id();
        let old = reporter
            .start_task(Network::X, TaskKind::Action, owner, &mut out)
            .unwrap();
        reporter.task_generation = u64::MAX;
        assert!(reporter
            .start_task(Network::X, TaskKind::Action, owner, &mut out)
            .is_none());
        let cleared = out.len();
        reporter.emit_task(old, TaskState::Error, &mut out);
        assert_eq!(out.len(), cleared);
        let batch = reporter.start_batch(2, &mut out).unwrap();
        reporter.batch_generation = u64::MAX;
        assert!(reporter.start_batch(2, &mut out).is_none());
        let ended = out.len();
        reporter.advance_batch(batch, &mut out);
        assert_eq!(out.len(), ended);
        assert!(!reports(&out).last().unwrap().contains_key("progress"));
    }

    #[test]
    fn task_completion_returns_the_original_value_or_error_without_serializing_it() {
        let result: Result<_, String> = Ok(Box::new("private post text"));
        let pointer = result.as_ref().unwrap().as_ref() as *const &str;
        let returned = Task { token: None }.finish(result).unwrap();
        assert_eq!(returned.as_ref() as *const &str, pointer);
        let result: Result<(), _> = Err(Box::new("private token and request URL"));
        let pointer = result.as_ref().unwrap_err().as_ref() as *const &str;
        let returned = Task { token: None }.finish(result).unwrap_err();
        assert_eq!(returned.as_ref() as *const &str, pointer);
    }
}
