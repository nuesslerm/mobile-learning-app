//! The preview tabs of the threads and the numbered `preview` stream: every open, report, resize, adjustment and close is an event, numbered from
//! one in each run (`epoch`), and the last `KEEP` are kept so a caller that lost the stream resumes by `seq`.
use crate::ports::{Clock, Ids};
use agent_api_preview::sessions::{self, code, message, AdjustParams, CloseParams, CloseResult, ListParams, ListResult, Nav, OpenParams, ReportParams, ResizeParams, SessionView, Viewport};
use agent_api_preview::watch::{self, Request, SubscribeResult};
use agent_api_proto::Event;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::watch as signal;

/// Events kept.
pub const KEEP: usize = 256;

/// What a use case refuses with.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

struct Log {
    seq: u64,
    events: VecDeque<Event>,
    changed: signal::Sender<u64>,
    keep: usize,
}

impl Log {
    fn oldest(&self) -> u64 {
        self.events.front().map_or(self.seq + 1, |e| e.seq)
    }

    fn push(&mut self, make: impl FnOnce(u64) -> Event) {
        self.seq += 1;
        self.events.push_back(make(self.seq));
        if self.events.len() > self.keep {
            self.events.pop_front();
        }
        self.changed.send_replace(self.seq);
    }
}

#[derive(Default)]
struct State {
    tabs: Vec<SessionView>,
}

/// Events after a `seq`, or that they are no longer all kept.
pub struct Since {
    pub events: Vec<Event>,
    pub lost: bool,
}

/// A subscription as opened: what to answer, where the stream continues from, and the wake-ups of the stream.
pub struct Opened {
    pub result: SubscribeResult,
    pub from: u64,
    pub changes: signal::Receiver<u64>,
}

pub struct Previews {
    clock: Arc<dyn Clock>,
    ids: Arc<dyn Ids>,
    epoch: String,
    /// Tabs and the log change together, so an event is never numbered before the change it records is visible.
    inner: Mutex<(State, Log)>,
}

impl Previews {
    /// `base` is the `seq` the stream counts on from: above any an earlier run of the service gave out, so a caller of an earlier run that names
    /// no epoch is still told to start again.
    pub fn new(clock: Arc<dyn Clock>, ids: Arc<dyn Ids>, base: u64) -> Arc<Previews> {
        Previews::keeping(clock, ids, base, KEEP)
    }

    pub fn keeping(clock: Arc<dyn Clock>, ids: Arc<dyn Ids>, base: u64, keep: usize) -> Arc<Previews> {
        let epoch = ids.id("epoch");
        let log = Log { seq: base, events: VecDeque::new(), changed: signal::channel(base).0, keep: keep.max(1) };
        Arc::new(Previews { clock, ids, epoch, inner: Mutex::new((State::default(), log)) })
    }

    fn lock(&self) -> MutexGuard<'_, (State, Log)> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn missing(thread: &str, tab: &str) -> Failure {
        Failure { code: code::SESSION_NOT_FOUND.into(), message: message::not_found(thread, tab) }
    }

    fn limit(why: String) -> Failure {
        Failure { code: code::SESSION_LIMIT.into(), message: why }
    }

    pub fn open(&self, p: OpenParams) -> Result<SessionView, Failure> {
        let now = self.clock.now_ms();
        let mut guard = self.lock();
        let (state, log) = &mut *guard;
        if state.tabs.len() >= sessions::MAX_TABS {
            return Err(Previews::limit(message::service_limit()));
        }
        if state.tabs.iter().filter(|t| t.thread == p.thread).count() >= sessions::MAX_TABS_PER_THREAD {
            return Err(Previews::limit(message::thread_limit(&p.thread)));
        }
        let view = SessionView {
            thread: p.thread,
            tab: self.ids.id("tab"),
            nav: p.url.map_or(Nav::Idle {}, |url| Nav::Loading { url, title: String::new() }),
            can_go_back: false,
            can_go_forward: false,
            viewport: p.viewport.unwrap_or(Viewport::Fill {}),
            color_scheme: None,
            zoom: None,
            profile: p.profile,
            reveal: p.reveal,
            updated_ms: now,
        };
        state.tabs.push(view.clone());
        let (epoch, shown) = (self.epoch.clone(), view.clone());
        log.push(move |seq| watch::session_event(seq, watch::kind::OPENED, &epoch, &shown, None));
        Ok(view)
    }

    /// Changes one tab and numbers the event `event` makes of it.
    fn change(&self, thread: &str, tab: &str, edit: impl FnOnce(&mut SessionView), event: impl FnOnce(u64, &str, &SessionView) -> Event) -> Result<SessionView, Failure> {
        let now = self.clock.now_ms();
        let mut guard = self.lock();
        let (state, log) = &mut *guard;
        let view = state.tabs.iter_mut().find(|t| t.thread == thread && t.tab == tab).ok_or_else(|| Previews::missing(thread, tab))?;
        edit(view);
        view.updated_ms = now;
        let (epoch, shown) = (self.epoch.clone(), view.clone());
        let recorded = shown.clone();
        log.push(move |seq| event(seq, &epoch, &recorded));
        Ok(shown)
    }

    pub fn report(&self, p: ReportParams) -> Result<SessionView, Failure> {
        let nav = p.nav.clone();
        self.change(&p.thread, &p.tab, |v| (v.nav, v.can_go_back, v.can_go_forward) = (p.nav, p.can_go_back, p.can_go_forward), move |seq, epoch, view| match &nav {
            Nav::Failed { url, title, code, description } => watch::failed_event(seq, epoch, view, url, title, *code, description),
            _ => watch::session_event(seq, watch::kind::NAVIGATED, epoch, view, None),
        })
    }

    pub fn resize(&self, p: ResizeParams) -> Result<SessionView, Failure> {
        self.change(&p.thread, &p.tab, |v| v.viewport = p.viewport, |seq, epoch, view| watch::session_event(seq, watch::kind::RESIZED, epoch, view, None))
    }

    pub fn adjust(&self, p: AdjustParams) -> Result<SessionView, Failure> {
        let request = Request { hard_reload: p.hard_reload, clear: p.clear };
        self.change(
            &p.thread,
            &p.tab,
            |v| {
                v.color_scheme = p.color_scheme.or(v.color_scheme);
                v.zoom = p.zoom.or(v.zoom);
            },
            move |seq, epoch, view| watch::session_event(seq, watch::kind::RESIZED, epoch, view, Some(&request)),
        )
    }

    /// Closes the tab named, or every tab of the thread; a tab that is not there is no failure, so a close can be repeated.
    pub fn close(&self, p: CloseParams) -> CloseResult {
        let now = self.clock.now_ms();
        let mut guard = self.lock();
        let (state, log) = &mut *guard;
        let (gone, kept): (Vec<SessionView>, Vec<SessionView>) = std::mem::take(&mut state.tabs).into_iter().partition(|t| t.thread == p.thread && p.tab.as_ref().is_none_or(|tab| *tab == t.tab));
        state.tabs = kept;
        for tab in &gone {
            let (epoch, thread, id) = (self.epoch.clone(), tab.thread.clone(), tab.tab.clone());
            log.push(move |seq| watch::closed_event(seq, &epoch, &thread, &id, now));
        }
        CloseResult { closed: gone.into_iter().map(|t| t.tab).collect() }
    }

    pub fn list(&self, p: ListParams) -> ListResult {
        let guard = self.lock();
        let mut sessions: Vec<SessionView> = guard.0.tabs.iter().filter(|t| t.thread == p.thread).cloned().collect();
        sessions.sort_by_key(|s| s.updated_ms);
        ListResult { sessions, epoch: self.epoch.clone(), revision: guard.1.seq }
    }

    /// A caller with no `after_seq` is not reset and follows from the end; one that names another run's `epoch`, one ahead of the stream and one
    /// older than the tail each get `reset` true; a caller within the tail has its missed events follow.
    pub fn open_stream(&self, epoch: Option<&str>, after_seq: Option<u64>) -> Opened {
        let guard = self.lock();
        let log = &guard.1;
        let reset = after_seq.is_some_and(|after| epoch.is_some_and(|e| e != self.epoch) || after > log.seq || after + 1 < log.oldest());
        let resumed = after_seq.filter(|_| !reset);
        Opened { result: SubscribeResult { topic: watch::TOPIC.into(), epoch: self.epoch.clone(), seq: log.seq, reset }, from: resumed.unwrap_or(log.seq), changes: log.changed.subscribe() }
    }

    pub fn since(&self, after: u64) -> Since {
        let guard = self.lock();
        let log = &guard.1;
        Since { events: log.events.iter().filter(|e| e.seq > after).cloned().collect(), lost: after + 1 < log.oldest() }
    }
}
