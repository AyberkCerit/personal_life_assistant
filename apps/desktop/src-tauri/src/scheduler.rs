//! The scheduler thread (FR-SCH, FR-TSK-012…016): reminders on time, missed reminders after a
//! start or a sleep, and the daily maintenance when the user is away and the PC is on mains power.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, TimeDelta};
use pla_core::jobs::{backup, maintenance_due, record_failure, record_success, DAILY, MAX_ATTEMPTS_PER_DAY};
use pla_core::reminders::{complete, due_reminders, local_minute, mark_notified, mark_seen, snooze, unanswered, DueReminder};
use rusqlite::Connection;
use serde::Serialize;

pub use crate::system::SystemProbe;

const IDLE_FOR_MAINTENANCE_SECS: u64 = 300;
/// A reminder older than this when it is first seen was missed (PLA closed, the PC asleep, a past
/// time, a catch-up extraction): it joins one list instead of popping up (decision 2, FR-TSK-016).
const LATE_AFTER_MINUTES: i64 = 5;

pub trait Notifier: Send {
    fn reminder(&self, reminder: &DueReminder);
    fn missed(&self, reminders: &[DueReminder]);
    fn problem(&self, message: &str);
    fn tasks_changed(&self);
    /// A backup ran (or failed), so the settings screen can refresh its status.
    fn backup_changed(&self) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedCommand {
    Done(String),
    Snooze(String),
    BackupNow,
    Shutdown,
}

type Clock = Box<dyn Fn() -> DateTime<FixedOffset> + Send>;

/// What the in-app banner lists (FR-TSK-015/016).
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct PendingLists {
    pub due: Vec<DueReminder>,
    pub missed: Vec<DueReminder>,
}

/// Which shown reminders the banner lists. The reminders themselves come from the database
/// (`unanswered`), so neither a late window (final review C1) nor a restart (user test finding 3)
/// loses one; this only remembers which of them came on time in this session. Everything else
/// unanswered, e.g. shown before a restart, is "missed".
#[derive(Default)]
pub struct Pending(Mutex<HashSet<String>>);

impl Pending {
    fn add(&self, reminders: &[DueReminder], missed: bool) {
        let mut on_time = self.0.lock().expect("pending lock");
        for r in reminders {
            if missed {
                on_time.remove(&r.task_id);
            } else {
                on_time.insert(r.task_id.clone());
            }
        }
    }

    pub fn current(&self, conn: &Connection) -> PendingLists {
        let mut on_time = self.0.lock().expect("pending lock");
        let all = unanswered(conn).unwrap_or_default();
        on_time.retain(|id| all.iter().any(|r| &r.task_id == id));
        let (due, missed) = all.into_iter().partition(|r| on_time.contains(&r.task_id));
        PendingLists { due, missed }
    }

    /// "Close" on the missed list: remembered in the database.
    pub fn dismiss_missed(&self, conn: &Connection, now: DateTime<FixedOffset>) {
        for r in self.current(conn).missed {
            let _ = mark_seen(conn, &r.task_id, now);
        }
    }
}

pub struct Scheduler {
    vault_root: PathBuf,
    pub(crate) conn: Connection,
    notifier: Box<dyn Notifier>,
    probe: Box<dyn SystemProbe>,
    clock: Clock,
    pending: Arc<Pending>,
    every: Duration,
}

impl Scheduler {
    pub fn new(vault_root: PathBuf, conn: Connection, notifier: Box<dyn Notifier>, probe: Box<dyn SystemProbe>, clock: Clock) -> Self {
        Self { vault_root, conn, notifier, probe, clock, pending: Arc::default(), every: Duration::from_secs(30) }
    }

    pub fn pending(&self) -> Arc<Pending> {
        Arc::clone(&self.pending)
    }

    pub fn tick(&mut self) {
        let now = (self.clock)();
        if let Ok(due) = due_reminders(&self.conn, now) {
            let late = local_minute(now - TimeDelta::minutes(LATE_AFTER_MINUTES));
            // Marked before anyone hears of it: the banner asks for the list as soon as the event comes.
            let shown: Vec<DueReminder> = due.into_iter().filter(|r| mark_notified(&self.conn, &r.task_id, &r.notify_at, now).is_ok()).collect();
            let (missed, on_time): (Vec<DueReminder>, Vec<DueReminder>) = shown.into_iter().partition(|r| r.notify_at < late);
            // Both kinds are sorted before anyone hears of either, so no list shows one in the wrong place.
            self.pending.add(&missed, true);
            self.pending.add(&on_time, false);
            if !missed.is_empty() {
                self.notifier.missed(&missed);
            }
            for r in &on_time {
                self.notifier.reminder(r);
            }
        }

        if maintenance_due(&self.conn, now.date_naive()).unwrap_or(false)
            && self.probe.idle_seconds() >= IDLE_FOR_MAINTENANCE_SECS
            && self.probe.on_ac_power()
        {
            self.run_maintenance(now);
        }
    }

    fn run_maintenance(&mut self, now: DateTime<FixedOffset>) {
        match backup(&self.conn, &self.vault_root, now.date_naive()) {
            Ok(_) => {
                let _ = record_success(&self.conn, DAILY, now);
            }
            Err(e) => {
                let attempts = record_failure(&self.conn, DAILY, now, &e.to_string()).unwrap_or(MAX_ATTEMPTS_PER_DAY);
                if attempts >= MAX_ATTEMPTS_PER_DAY {
                    self.notifier.problem(&e.to_string());
                }
            }
        }
    }

    pub fn handle(&mut self, command: SchedCommand) {
        let now = (self.clock)();
        let changed = match command {
            SchedCommand::Done(id) => complete(&self.conn, &id, now).is_ok(),
            SchedCommand::Snooze(id) => snooze(&self.conn, &id, now).is_ok(),
            SchedCommand::BackupNow => {
                self.run_maintenance(now);
                self.notifier.backup_changed();
                false
            }
            SchedCommand::Shutdown => false,
        };
        if changed {
            self.notifier.tasks_changed();
        }
    }

    pub fn spawn(mut self, every: Duration) -> SchedulerHandle {
        self.every = every;
        let (tx, rx) = mpsc::channel();
        let join = std::thread::Builder::new().name("pla-scheduler".into()).spawn(move || self.run(rx)).expect("spawn scheduler");
        SchedulerHandle { tx, join: Some(join) }
    }

    fn run(mut self, rx: Receiver<SchedCommand>) {
        self.tick();
        let mut next = Instant::now() + self.every;
        loop {
            // A deadline, so commands from the notifications do not postpone the next tick.
            match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                Ok(SchedCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {
                    self.tick();
                    next = Instant::now() + self.every;
                }
            }
        }
    }
}

pub struct SchedulerHandle {
    tx: Sender<SchedCommand>,
    join: Option<JoinHandle<()>>,
}

impl SchedulerHandle {
    pub fn send(&self, command: SchedCommand) {
        let _ = self.tx.send(command);
    }

    pub fn sender(&self) -> Sender<SchedCommand> {
        self.tx.clone()
    }
}

impl Drop for SchedulerHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(SchedCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use pla_core::db::open_databases;
    use pla_core::tasks::{add_task, TaskInput};
    use std::sync::{Arc, Mutex};

    #[derive(Default, Clone)]
    struct Seen(Arc<Mutex<Vec<String>>>);
    impl Notifier for Seen {
        fn reminder(&self, r: &DueReminder) {
            self.0.lock().unwrap().push(format!("reminder {}", r.title));
        }
        fn missed(&self, list: &[DueReminder]) {
            self.0.lock().unwrap().push(format!("missed {}", list.len()));
        }
        fn problem(&self, message: &str) {
            self.0.lock().unwrap().push(format!("problem {message}"));
        }
        fn tasks_changed(&self) {
            self.0.lock().unwrap().push("changed".into());
        }
    }

    struct Probe(Arc<Mutex<(u64, bool)>>);
    impl SystemProbe for Probe {
        fn idle_seconds(&self) -> u64 {
            self.0.lock().unwrap().0
        }
        fn on_ac_power(&self) -> bool {
            self.0.lock().unwrap().1
        }
    }

    struct Rig {
        _tmp: tempfile::TempDir,
        vault: PathBuf,
        sched: Scheduler,
        seen: Seen,
        clock: Arc<Mutex<DateTime<FixedOffset>>>,
        probe: Arc<Mutex<(u64, bool)>>,
    }

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn rig(start: &str) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("kasa");
        std::fs::create_dir_all(&vault).unwrap();
        let conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        let seen = Seen::default();
        let clock = Arc::new(Mutex::new(at(start)));
        let probe = Arc::new(Mutex::new((0, true)));
        let c = Arc::clone(&clock);
        let sched = Scheduler::new(vault.clone(), conn, Box::new(seen.clone()), Box::new(Probe(Arc::clone(&probe))), Box::new(move || *c.lock().unwrap()));
        Rig { _tmp: tmp, vault, sched, seen, clock, probe }
    }

    impl Rig {
        fn at(&mut self, s: &str) {
            *self.clock.lock().unwrap() = at(s);
            self.sched.tick();
        }
        /// Moves the clock forward in real 30 s ticks, like the running scheduler.
        fn walk(&mut self, s: &str) {
            let target = at(s);
            loop {
                let next = (*self.clock.lock().unwrap() + chrono::TimeDelta::seconds(30)).min(target);
                *self.clock.lock().unwrap() = next;
                self.sched.tick();
                if next >= target {
                    break;
                }
            }
        }
        fn remind(&self, title: &str, date: &str, time: &str) -> String {
            let input = TaskInput { title: title.into(), date: Some(date.into()), time: Some(time.into()), remind: Some(true), ..Default::default() };
            add_task(&self.sched.conn, &input, at("2026-10-06T08:00:00+03:00")).unwrap()
        }
        fn seen(&self) -> Vec<String> {
            std::mem::take(&mut *self.seen.0.lock().unwrap())
        }
    }

    #[test]
    fn due_reminders_are_shown_once_on_time() {
        // Review Focus 1
        let mut r = rig("2026-10-06T09:00:00+03:00");
        r.remind("İlaç", "2026-10-06", "09:01");
        r.at("2026-10-06T09:00:00+03:00");
        assert!(r.seen().is_empty());
        r.walk("2026-10-06T09:01:10+03:00");
        assert_eq!(r.seen(), vec!["reminder İlaç"]);
        r.walk("2026-10-06T09:01:40+03:00");
        assert!(r.seen().is_empty(), "not again");
    }

    #[test]
    fn reminders_due_before_start_or_during_sleep_are_one_missed_list() {
        // Review Focus 2
        let mut r = rig("2026-10-06T12:00:00+03:00");
        r.remind("Sabah", "2026-10-06", "08:00");
        r.remind("Öğlen", "2026-10-06", "11:00");
        r.at("2026-10-06T12:00:00+03:00");
        assert_eq!(r.seen(), vec!["missed 2"]);
        r.remind("Akşam", "2026-10-06", "18:00");
        r.at("2026-10-06T12:00:30+03:00");
        r.at("2026-10-06T19:30:00+03:00"); // the laptop slept in between
        assert_eq!(r.seen(), vec!["missed 1"]);
    }

    #[test]
    fn a_reminder_already_past_on_a_normal_tick_joins_the_missed_list() {
        // Final review I1: a catch-up extraction or a past time must not pop up as a toast
        let mut r = rig("2026-10-06T12:00:00+03:00");
        r.at("2026-10-06T12:00:00+03:00");
        r.remind("Sabah ilacı", "2026-10-06", "08:00");
        r.walk("2026-10-06T12:00:30+03:00");
        assert_eq!(r.seen(), vec!["missed 1"]);
    }

    #[test]
    fn a_tick_delayed_by_a_slow_backup_still_shows_the_reminder() {
        // Final review I1: a late tick is not a sleep
        let mut r = rig("2026-10-06T22:00:00+03:00");
        r.remind("Çöp", "2026-10-06", "22:01");
        r.at("2026-10-06T22:00:00+03:00");
        r.at("2026-10-06T22:03:00+03:00");
        assert_eq!(r.seen(), vec!["reminder Çöp"]);
    }

    #[test]
    fn shown_reminders_stay_listed_until_answered() {
        // Final review C1: the window may open (or the banner mount) after the first tick
        let mut r = rig("2026-10-06T12:00:00+03:00");
        let a = r.remind("Sabah", "2026-10-06", "08:00");
        r.remind("Öğlen", "2026-10-06", "11:00");
        r.at("2026-10-06T12:00:00+03:00");
        let pending = r.sched.pending();
        assert_eq!(pending.current(&r.sched.conn).missed.len(), 2);
        r.sched.handle(SchedCommand::Done(a));
        assert_eq!(pending.current(&r.sched.conn).missed.len(), 1);
        pending.dismiss_missed(&r.sched.conn, at("2026-10-06T12:00:40+03:00"));
        assert!(pending.current(&r.sched.conn).missed.is_empty());
        let b = r.remind("Akşam", "2026-10-06", "12:01");
        r.walk("2026-10-06T12:01:00+03:00");
        assert_eq!(pending.current(&r.sched.conn).due.iter().map(|d| d.task_id.clone()).collect::<Vec<_>>(), vec![b.clone()]);
        r.sched.handle(SchedCommand::Snooze(b));
        assert!(pending.current(&r.sched.conn).due.is_empty(), "snoozed: gone until it fires again");
    }

    /// Asks for the pending list the moment it is notified, like the banner does.
    struct Asks {
        pending: Arc<Mutex<Option<Arc<Pending>>>>,
        db: PathBuf,
        got: Arc<Mutex<Vec<(usize, usize)>>>,
    }
    impl Asks {
        fn ask(&self) {
            let conn = rusqlite::Connection::open(&self.db).unwrap();
            let lists = self.pending.lock().unwrap().as_ref().unwrap().current(&conn);
            self.got.lock().unwrap().push((lists.due.len(), lists.missed.len()));
        }
    }
    impl Notifier for Asks {
        fn reminder(&self, _: &DueReminder) {
            self.ask();
        }
        fn missed(&self, _: &[DueReminder]) {
            self.ask();
        }
        fn problem(&self, _: &str) {}
        fn tasks_changed(&self) {}
    }

    #[test]
    fn the_list_is_ready_when_the_event_arrives() {
        // Found in the real window: the banner asked before the reminder was marked and lost it
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        for (title, time) in [("Sabah", "08:00"), ("Öğlen", "12:00")] {
            let input = TaskInput { title: title.into(), date: Some("2026-10-06".into()), time: Some(time.into()), remind: Some(true), ..Default::default() };
            add_task(&conn, &input, at("2026-10-06T07:00:00+03:00")).unwrap();
        }
        let (slot, got) = (Arc::new(Mutex::new(None)), Arc::new(Mutex::new(Vec::new())));
        let asks = Asks { pending: Arc::clone(&slot), db: tmp.path().join(".data/pla.db"), got: Arc::clone(&got) };
        let clock = Arc::new(Mutex::new(at("2026-10-06T12:00:00+03:00")));
        let c = Arc::clone(&clock);
        let probe = Probe(Arc::new(Mutex::new((0, true))));
        let mut sched = Scheduler::new(tmp.path().join("kasa"), conn, Box::new(asks), Box::new(probe), Box::new(move || *c.lock().unwrap()));
        *slot.lock().unwrap() = Some(sched.pending());
        sched.tick();
        assert_eq!(*got.lock().unwrap(), vec![(1, 1), (1, 1)], "each event finds both lists complete and in place");
    }

    #[test]
    fn unanswered_reminders_are_listed_again_after_a_restart() {
        // User test finding 3: the list lived only in memory
        let mut r = rig("2026-10-06T09:00:00+03:00");
        r.remind("İlaç", "2026-10-06", "09:01");
        r.remind("Kapatılan", "2026-10-06", "08:00");
        r.at("2026-10-06T09:00:00+03:00");
        r.sched.pending().dismiss_missed(&r.sched.conn, at("2026-10-06T09:00:10+03:00"));
        r.walk("2026-10-06T09:01:00+03:00");
        assert_eq!(r.sched.pending().current(&r.sched.conn).due.len(), 1);
        let after_restart = Pending::default().current(&r.sched.conn);
        let titles: Vec<&str> = after_restart.missed.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, vec!["İlaç"], "shown before the restart, never answered; the closed one stays closed");
        assert!(after_restart.due.is_empty());
    }

    #[test]
    fn done_and_snooze_from_the_notification() {
        let mut r = rig("2026-10-06T09:00:00+03:00");
        let id = r.remind("İlaç", "2026-10-06", "09:01");
        r.at("2026-10-06T09:00:00+03:00");
        r.walk("2026-10-06T09:01:00+03:00");
        r.seen();
        r.sched.handle(SchedCommand::Snooze(id.clone()));
        assert_eq!(r.seen(), vec!["changed"]);
        r.walk("2026-10-06T09:11:00+03:00");
        assert_eq!(r.seen(), vec!["reminder İlaç"], "back after 10 minutes");
        r.sched.handle(SchedCommand::Done(id));
        assert_eq!(r.seen(), vec!["changed"]);
    }

    #[test]
    fn maintenance_waits_for_idle_and_mains_power() {
        // Review Focus 3: FR-SCH-005
        let mut r = rig("2026-10-06T22:00:00+03:00");
        let backup = r.vault.join(".pla/backup/pla-2026-10-06.db");
        r.at("2026-10-06T22:00:00+03:00");
        assert!(!backup.exists(), "user is active");
        *r.probe.lock().unwrap() = (600, false);
        r.at("2026-10-06T22:00:30+03:00");
        assert!(!backup.exists(), "on battery");
        *r.probe.lock().unwrap() = (600, true);
        r.at("2026-10-06T22:01:00+03:00");
        assert!(backup.exists());
        std::fs::remove_file(&backup).unwrap();
        r.at("2026-10-06T22:01:30+03:00");
        assert!(!backup.exists(), "once a day");
    }

    #[test]
    fn a_second_maintenance_failure_is_reported() {
        // Review Focus 3: FR-SCH-015
        let mut r = rig("2026-10-06T22:00:00+03:00");
        std::fs::write(r.vault.join(".pla"), "a file where the folder should be").unwrap();
        *r.probe.lock().unwrap() = (600, true);
        r.at("2026-10-06T22:00:00+03:00");
        assert!(r.seen().is_empty(), "first failure: silent retry later");
        r.at("2026-10-06T22:00:30+03:00");
        let seen = r.seen();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].starts_with("problem"), "{seen:?}");
        r.at("2026-10-06T22:01:00+03:00");
        assert!(r.seen().is_empty(), "given up for today");
    }

    #[test]
    fn the_thread_stops_on_shutdown() {
        // Review Focus 4: FR-SCH-003
        let r = rig("2026-10-06T09:00:00+03:00");
        let handle = r.sched.spawn(Duration::from_millis(20));
        let started = std::time::Instant::now();
        drop(handle);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
