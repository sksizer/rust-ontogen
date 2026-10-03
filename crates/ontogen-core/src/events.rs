//! Runtime support for generated event ops.
//!
//! An api fn that returns `broadcast::Receiver<T>` is an event op. The
//! generated SSE handler and IPC subscribe command both turn the receiver
//! into a sequence of [`EventFrame`]s with [`next_frame`], so a lagged
//! receiver becomes an explicit [`EventFrame::Lag`] instead of a silent drop.
//!
//! Enabled by the `events` feature. A crate that declares event ops depends
//! on `ontogen-core` with that feature.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::task::AbortHandle;

/// An event item that carries its own sequence id.
///
/// A resumable event op (one that declares `resume: Option<String>`) must
/// return items that implement this. The id becomes the SSE `id:` line and
/// the `id` of an IPC [`EventFrame::Event`]. A client that reconnects sends
/// the last id back as `resume`; what resuming means is up to the event fn.
pub trait EventSeq {
    /// The id a client can resume after.
    fn event_id(&self) -> String;
}

/// One message on an event subscription, on both transports.
///
/// IPC sends it as JSON: `{"kind":"event","id":…,"data":…}` or
/// `{"kind":"lag","skipped":n}`. SSE maps it onto `id:` / `event:` / `data:`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EventFrame<T> {
    /// An item from the event fn's receiver.
    Event {
        /// The item's [`EventSeq`] id, or `None` for an op that is not resumable.
        id: Option<String>,
        /// The item.
        data: T,
    },
    /// The receiver fell behind and `skipped` items were dropped. The
    /// subscription stays open.
    Lag {
        /// How many items were dropped.
        skipped: u64,
    },
}

/// How a generated handler reads an item's id.
pub type IdFn<T> = fn(&T) -> Option<String>;

/// The id of a resumable op's item.
pub fn seq_id<T: EventSeq>(item: &T) -> Option<String> {
    Some(item.event_id())
}

/// The id of an item from an op that is not resumable: always `None`.
pub fn no_id<T>(_: &T) -> Option<String> {
    None
}

/// Wait for the next frame. `None` once every sender is dropped.
pub async fn next_frame<T: Clone>(rx: &mut broadcast::Receiver<T>, id: IdFn<T>) -> Option<EventFrame<T>> {
    match rx.recv().await {
        Ok(data) => Some(EventFrame::Event { id: id(&data), data }),
        Err(RecvError::Lagged(skipped)) => Some(EventFrame::Lag { skipped }),
        Err(RecvError::Closed) => None,
    }
}

/// Send every frame from `rx` through `send` until the senders close or a
/// send fails.
///
/// A failed send means the subscriber is gone (for IPC, a closed webview:
/// a Tauri `Channel` has no close callback, so the first failed send is how
/// the task learns of it).
pub async fn forward<T, E>(
    mut rx: broadcast::Receiver<T>,
    id: IdFn<T>,
    mut send: impl FnMut(EventFrame<T>) -> Result<(), E>,
) where
    T: Clone,
{
    while let Some(frame) = next_frame(&mut rx, id).await {
        if send(frame).is_err() {
            break;
        }
    }
}

/// The IPC subscriptions that are live, by id.
///
/// Generated code holds one in a `static`, so a consumer wires nothing: a
/// subscription id is only meaningful inside the process that issued it.
/// Each task removes itself when it ends; [`Subscriptions::cancel`] aborts
/// one early.
#[derive(Debug, Default)]
pub struct Subscriptions {
    next_id: AtomicU64,
    tasks: Mutex<BTreeMap<u64, AbortHandle>>,
}

impl Subscriptions {
    /// An empty registry.
    pub const fn new() -> Self {
        Self { next_id: AtomicU64::new(1), tasks: Mutex::new(BTreeMap::new()) }
    }

    /// Spawn `task` on the current tokio runtime and return its id.
    ///
    /// # Panics
    ///
    /// Panics when called outside a tokio runtime. Tauri async commands run
    /// inside one.
    pub fn spawn<F>(&'static self, task: F) -> u64
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // Held across the spawn so a task that ends at once cannot remove its
        // entry before it is inserted.
        let mut tasks = self.lock();
        let handle = tokio::spawn(async move {
            task.await;
            self.lock().remove(&id);
        });
        tasks.insert(id, handle.abort_handle());
        id
    }

    /// Abort the subscription `id`. Returns `false` when it already ended.
    pub fn cancel(&self, id: u64) -> bool {
        match self.lock().remove(&id) {
            Some(handle) => {
                handle.abort();
                true
            }
            None => false,
        }
    }

    /// Whether the subscription `id` is still running.
    pub fn is_live(&self, id: u64) -> bool {
        self.lock().contains_key(&id)
    }

    /// How many subscriptions are running.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether no subscription is running.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<u64, AbortHandle>> {
        // A panic while holding the lock leaves the map consistent; keep going.
        self.tasks.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The resume token a reconnecting SSE client sent: the `Last-Event-ID`
/// header value, when present and valid UTF-8.
pub fn last_event_id(header: Option<&[u8]>) -> Option<String> {
    header.and_then(|v| std::str::from_utf8(v).ok()).filter(|s| !s.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    struct Change {
        seq: u64,
    }

    impl EventSeq for Change {
        fn event_id(&self) -> String {
            format!("0:{}", self.seq)
        }
    }

    #[tokio::test]
    async fn lagged_receiver_yields_one_lag_frame_and_keeps_streaming() {
        let (tx, mut rx) = broadcast::channel(2);
        for seq in 0..5 {
            tx.send(Change { seq }).unwrap();
        }
        assert_eq!(next_frame(&mut rx, seq_id).await, Some(EventFrame::Lag { skipped: 3 }));
        assert_eq!(
            next_frame(&mut rx, seq_id).await,
            Some(EventFrame::Event { id: Some("0:3".into()), data: Change { seq: 3 } })
        );
        assert_eq!(
            next_frame(&mut rx, seq_id).await,
            Some(EventFrame::Event { id: Some("0:4".into()), data: Change { seq: 4 } })
        );
        tx.send(Change { seq: 5 }).unwrap();
        assert!(matches!(next_frame(&mut rx, seq_id).await, Some(EventFrame::Event { .. })));
        drop(tx);
        assert_eq!(next_frame(&mut rx, seq_id).await, None);
    }

    #[tokio::test]
    async fn no_id_leaves_the_frame_id_empty() {
        let (tx, mut rx) = broadcast::channel(4);
        tx.send(7u32).unwrap();
        assert_eq!(next_frame(&mut rx, no_id).await, Some(EventFrame::Event { id: None, data: 7 }));
    }

    #[test]
    fn frames_serialize_with_a_kind_tag() {
        let event = EventFrame::Event { id: Some("1:2".to_string()), data: Change { seq: 2 } };
        assert_eq!(serde_json::to_string(&event).unwrap(), r#"{"kind":"event","id":"1:2","data":{"seq":2}}"#);
        let lag: EventFrame<Change> = EventFrame::Lag { skipped: 9 };
        assert_eq!(serde_json::to_string(&lag).unwrap(), r#"{"kind":"lag","skipped":9}"#);
    }

    #[test]
    fn last_event_id_ignores_missing_empty_and_non_utf8_headers() {
        assert_eq!(last_event_id(None), None);
        assert_eq!(last_event_id(Some(b"")), None);
        assert_eq!(last_event_id(Some(&[0xff, 0xfe])), None);
        assert_eq!(last_event_id(Some(b"3:14")), Some("3:14".to_string()));
    }

    /// Poll `cond` until it holds or a second passes.
    async fn eventually(cond: impl Fn() -> bool) -> bool {
        for _ in 0..100 {
            if cond() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cond()
    }

    #[tokio::test]
    async fn forwarding_ends_on_the_first_failed_send() {
        static SUBS: Subscriptions = Subscriptions::new();
        let (tx, rx) = broadcast::channel(8);
        let sent = Arc::new(AtomicUsize::new(0));
        let counter = sent.clone();
        // A subscriber that accepts one frame and then is gone.
        let id = SUBS.spawn(forward(rx, seq_id, move |_frame: EventFrame<Change>| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 { Ok(()) } else { Err("closed") }
        }));
        assert!(SUBS.is_live(id));
        tx.send(Change { seq: 1 }).unwrap();
        tx.send(Change { seq: 2 }).unwrap();
        assert!(eventually(|| !SUBS.is_live(id)).await, "task ends after the failed send");
        assert_eq!(sent.load(Ordering::SeqCst), 2);
        assert!(tx.send(Change { seq: 3 }).is_err(), "the ended task dropped its receiver");
        assert_eq!(sent.load(Ordering::SeqCst), 2, "nothing is sent after the task ends");
        assert!(!SUBS.cancel(id), "an ended task is already gone");
    }

    #[tokio::test]
    async fn cancel_ends_the_task_at_once() {
        static SUBS: Subscriptions = Subscriptions::new();
        let (tx, rx) = broadcast::channel(8);
        let sent = Arc::new(AtomicUsize::new(0));
        let counter = sent.clone();
        let id = SUBS.spawn(forward(rx, seq_id, move |_frame: EventFrame<Change>| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>(())
        }));
        tx.send(Change { seq: 1 }).unwrap();
        assert!(eventually(|| sent.load(Ordering::SeqCst) == 1).await);
        assert!(SUBS.cancel(id));
        assert!(!SUBS.is_live(id));
        // The aborted task holds no receiver, so the sender sees no subscriber.
        assert!(eventually(|| tx.receiver_count() == 0).await, "the aborted task dropped its receiver");
        assert!(tx.send(Change { seq: 2 }).is_err());
        assert_eq!(sent.load(Ordering::SeqCst), 1);
        assert!(!SUBS.cancel(id));
    }

    #[tokio::test]
    async fn forwarding_ends_when_the_senders_close() {
        static SUBS: Subscriptions = Subscriptions::new();
        let (tx, rx) = broadcast::channel::<Change>(8);
        let id = SUBS.spawn(forward(rx, seq_id, |_frame| Ok::<_, ()>(())));
        drop(tx);
        assert!(eventually(|| !SUBS.is_live(id)).await);
        assert!(SUBS.is_empty());
    }
}
