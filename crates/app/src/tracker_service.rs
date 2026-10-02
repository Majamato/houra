//! Owns the storage thread; all database access runs on it.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use houra_core::{
    Activity, ActivityId, Project, ProjectId, SystemClock, TimeEntry, TrackerCommand,
    TrackerEngine, TrackerSnapshot, TrackerState, Transition,
};

use crate::AppError;
use crate::backup::BackupDocument;
use crate::storage::Store;

/// One-shot channel the worker answers on.
type Reply<T> = Sender<Result<T, AppError>>;

/// Called on the storage thread after the tracker state changes. Heartbeats
/// don't count: they only record that the timer is alive, which nothing shows.
pub type ChangeObserver = Box<dyn Fn() + Send + 'static>;

/// Everything the worker can be asked. Each variant carries its reply channel.
enum Request {
    Apply(TrackerCommand, Reply<Transition>),
    Continue(houra_core::EntryId, Reply<Transition>),
    Snapshot(Reply<TrackerSnapshot>),
    LiveElapsed(Reply<std::time::Duration>),
    Entries(i64, i64, Reply<Vec<TimeEntry>>),
    Entry(houra_core::EntryId, Reply<TimeEntry>),
    AddEntry(TimeEntry, Reply<houra_core::EntryId>),
    UpdateEntry(TimeEntry, Reply<()>),
    DeleteEntry(houra_core::EntryId, Reply<()>),
    Projects(bool, Reply<Vec<Project>>),
    Activities(bool, Reply<Vec<Activity>>),
    CreateProject(String, String, i64, Reply<ProjectId>),
    CreateActivity(String, i64, Reply<ActivityId>),
    ArchiveProject(ProjectId, bool, i64, Reply<()>),
    ArchiveActivity(ActivityId, bool, i64, Reply<()>),
    Backup(i64, Reply<BackupDocument>),
    Restore(Box<BackupDocument>, Reply<()>),
    Shutdown(Reply<()>),
}

/// Cheap, cloneable, `Send` client of the storage thread.
/// Every call blocks until the worker answers.
#[derive(Clone)]
pub struct TrackerHandle {
    sender: Sender<Request>,
}

impl TrackerHandle {
    /// Applies a tracker command on the worker and returns the transition.
    pub fn apply(&self, command: TrackerCommand) -> Result<Transition, AppError> {
        self.request(|reply| Request::Apply(command, reply))
    }

    /// Resolves and continues a stored entry by identity on the storage worker.
    pub fn continue_entry(&self, entry_id: houra_core::EntryId) -> Result<Transition, AppError> {
        self.request(|reply| Request::Continue(entry_id, reply))
    }

    /// Returns the worker's current tracker state.
    pub fn snapshot(&self) -> Result<TrackerSnapshot, AppError> {
        self.request(Request::Snapshot)
    }

    /// Returns how long the current timer has run.
    pub fn live_elapsed(&self) -> Result<std::time::Duration, AppError> {
        self.request(Request::LiveElapsed)
    }

    /// Lists entries overlapping the given half-open range.
    pub fn entries(&self, start_ms: i64, end_ms: i64) -> Result<Vec<TimeEntry>, AppError> {
        self.request(|reply| Request::Entries(start_ms, end_ms, reply))
    }

    /// Returns one entry by identity.
    pub fn entry(&self, id: houra_core::EntryId) -> Result<TimeEntry, AppError> {
        self.request(|reply| Request::Entry(id, reply))
    }

    /// Stores a new entry and returns its identity.
    pub fn add_entry(&self, entry: TimeEntry) -> Result<houra_core::EntryId, AppError> {
        self.request(|reply| Request::AddEntry(entry, reply))
    }

    /// Replaces a stored entry.
    pub fn update_entry(&self, entry: TimeEntry) -> Result<(), AppError> {
        self.request(|reply| Request::UpdateEntry(entry, reply))
    }

    /// Deletes a stored entry and its tracked intervals.
    pub fn delete_entry(&self, id: houra_core::EntryId) -> Result<(), AppError> {
        self.request(|reply| Request::DeleteEntry(id, reply))
    }

    /// Lists projects, optionally including archived ones.
    pub fn projects(&self, include_archived: bool) -> Result<Vec<Project>, AppError> {
        self.request(|reply| Request::Projects(include_archived, reply))
    }

    /// Lists activities, optionally including archived ones.
    pub fn activities(&self, include_archived: bool) -> Result<Vec<Activity>, AppError> {
        self.request(|reply| Request::Activities(include_archived, reply))
    }

    /// Creates a project and returns its identity.
    pub fn create_project(
        &self,
        name: String,
        color: String,
        now_ms: i64,
    ) -> Result<ProjectId, AppError> {
        self.request(|reply| Request::CreateProject(name, color, now_ms, reply))
    }

    /// Creates an activity and returns its identity.
    pub fn create_activity(&self, name: String, now_ms: i64) -> Result<ActivityId, AppError> {
        self.request(|reply| Request::CreateActivity(name, now_ms, reply))
    }

    /// Archives or unarchives a project.
    pub fn set_project_archived(
        &self,
        id: ProjectId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        self.request(|reply| Request::ArchiveProject(id, archived, now_ms, reply))
    }

    /// Archives or unarchives an activity.
    pub fn set_activity_archived(
        &self,
        id: ActivityId,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), AppError> {
        self.request(|reply| Request::ArchiveActivity(id, archived, now_ms, reply))
    }

    /// Snapshots the whole database into a backup document.
    pub fn backup(&self, exported_at_ms: i64) -> Result<BackupDocument, AppError> {
        self.request(|reply| Request::Backup(exported_at_ms, reply))
    }

    /// Replaces the database with a backup document.
    pub fn restore(&self, document: BackupDocument) -> Result<(), AppError> {
        self.request(|reply| Request::Restore(Box::new(document), reply))
    }

    /// Asks the worker to stop and waits for its answer.
    pub fn shutdown(&self) -> Result<(), AppError> {
        self.request(Request::Shutdown)
    }

    fn request<T>(&self, build: impl FnOnce(Reply<T>) -> Request) -> Result<T, AppError> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(build(sender))
            .map_err(|_| AppError::WorkerStopped)?;
        receiver.recv().map_err(|_| AppError::WorkerStopped)?
    }
}

/// Owns the storage thread; `shutdown` joins it.
pub struct TrackerService {
    /// Client for talking to the worker thread.
    pub handle: TrackerHandle,
    join: Option<JoinHandle<()>>,
}

impl TrackerService {
    /// Opens the database and spawns the storage worker.
    /// Recovers the tracker when the last shutdown was unclean.
    /// `on_change` runs after each reply that changed the tracker state.
    pub fn start(path: PathBuf, on_change: ChangeObserver) -> Result<Self, AppError> {
        let mut store = Store::open(&path)?;
        let snapshot = store.load_snapshot()?;
        let unclean = !store.previous_shutdown_clean() && snapshot.state.active().is_some();
        let engine = TrackerEngine::restore(SystemClock::default(), snapshot, unclean);
        let (sender, receiver) = mpsc::channel();
        let join = thread::Builder::new()
            .name("houra-storage".into())
            .spawn(move || worker_loop(&mut store, engine, receiver, on_change))
            .map_err(|source| AppError::io("storage worker", source))?;
        Ok(Self {
            handle: TrackerHandle { sender },
            join: Some(join),
        })
    }

    /// Asks the worker to finish, then waits for the thread to end.
    pub fn shutdown(mut self) -> Result<(), AppError> {
        let result = self.handle.shutdown();
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| AppError::WorkerStopped)?;
        }
        result
    }
}

fn worker_loop(
    store: &mut Store,
    mut engine: TrackerEngine<SystemClock>,
    receiver: Receiver<Request>,
    on_change: ChangeObserver,
) {
    while let Ok(request) = receiver.recv() {
        let should_stop = matches!(request, Request::Shutdown(_));
        let revision_before = engine.snapshot().revision;
        let mut changed = false;
        match request {
            Request::Apply(command, reply) => {
                let heartbeat = matches!(command, TrackerCommand::Heartbeat);
                let mut candidate = engine.clone();
                let result =
                    candidate
                        .apply(command)
                        .map_err(AppError::from)
                        .and_then(|mut transition| {
                            if transition.snapshot.revision != engine.snapshot().revision
                                && let Some(adopted) = store.persist_transition(&transition)?
                            {
                                candidate.adopt_paused_entry_id(adopted);
                                transition.snapshot = candidate.snapshot().clone();
                            }
                            engine = candidate;
                            Ok(transition)
                        });
                changed = !heartbeat
                    && result
                        .as_ref()
                        .is_ok_and(|transition| transition.snapshot.revision != revision_before);
                let _ignored = reply.send(result);
            }
            Request::Continue(entry_id, reply) => {
                let result = store.entry(entry_id).and_then(|entry| {
                    let mut candidate = engine.clone();
                    let transition = candidate.apply(TrackerCommand::Continue {
                        entry_id,
                        project_id: entry.project_id,
                        activity_id: entry.activity_id,
                        note: entry.note,
                    })?;
                    if transition.snapshot.revision != engine.snapshot().revision {
                        store.persist_transition(&transition)?;
                        engine = candidate;
                    }
                    Ok(transition)
                });
                changed = result
                    .as_ref()
                    .is_ok_and(|transition| transition.snapshot.revision != revision_before);
                let _ignored = reply.send(result);
            }
            Request::Snapshot(reply) => {
                let _ignored = reply.send(Ok(engine.snapshot().clone()));
            }
            Request::LiveElapsed(reply) => {
                let _ignored = reply.send(Ok(engine.live_elapsed()));
            }
            Request::Entries(start, end, reply) => {
                let _ignored = reply.send(store.list_entries(start, end));
            }
            Request::Entry(id, reply) => {
                let _ignored = reply.send(store.entry(id));
            }
            Request::AddEntry(entry, reply) => {
                let _ignored = reply.send(store.add_entry(&entry));
            }
            Request::UpdateEntry(entry, reply) => {
                let _ignored = reply.send(store.update_entry(&entry));
            }
            Request::DeleteEntry(id, reply) => {
                let _ignored = reply.send(store.delete_entry(id));
            }
            Request::Projects(include_archived, reply) => {
                let _ignored = reply.send(store.list_projects(include_archived));
            }
            Request::Activities(include_archived, reply) => {
                let _ignored = reply.send(store.list_activities(include_archived));
            }
            Request::CreateProject(name, color, now, reply) => {
                let _ignored = reply.send(store.create_project(&name, &color, now));
            }
            Request::CreateActivity(name, now, reply) => {
                let _ignored = reply.send(store.create_activity(&name, now));
            }
            Request::ArchiveProject(id, archived, now, reply) => {
                let _ignored = reply.send(store.set_project_archived(id, archived, now));
            }
            Request::ArchiveActivity(id, archived, now, reply) => {
                let _ignored = reply.send(store.set_activity_archived(id, archived, now));
            }
            Request::Backup(now, reply) => {
                let _ignored = reply.send(store.backup(now));
            }
            Request::Restore(document, reply) => {
                let result = store.restore(&document).and_then(|()| {
                    let snapshot = store.load_snapshot()?;
                    engine = TrackerEngine::restore(SystemClock::default(), snapshot, false);
                    Ok(())
                });
                changed = result.is_ok();
                let _ignored = reply.send(result);
            }
            Request::Shutdown(reply) => {
                // A paused timer is fully persisted and needs no recovery.
                let needs_recovery = matches!(
                    engine.snapshot().state,
                    TrackerState::Running(_)
                        | TrackerState::IdlePending(_)
                        | TrackerState::RecoveryPending(_)
                );
                let result = if needs_recovery {
                    Ok(())
                } else {
                    store.mark_clean_shutdown()
                };
                let _ignored = reply.send(result);
            }
        }
        // After replying, so the caller never waits for the observer.
        if changed {
            on_change();
        }
        if should_stop {
            break;
        }
    }
}
