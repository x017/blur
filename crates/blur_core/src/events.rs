//! Lightweight thread-safe publish/subscribe event bus.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use parking_lot::Mutex;

/// Identifier for a registered callback subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

type Callback<E> = Arc<dyn Fn(&E) + Send + Sync + 'static>;

struct Inner<E> {
    channels: Vec<Sender<E>>,
    callbacks: Vec<(SubscriptionId, Callback<E>)>,
    next_id: AtomicU64,
}

/// Thread-safe event bus supporting both `mpsc::Receiver` subscribers and callbacks.
pub struct EventBus<E: Clone + Send + 'static> {
    inner: Arc<Mutex<Inner<E>>>,
}

impl<E: Clone + Send + 'static> Clone for EventBus<E> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<E: Clone + Send + 'static> Default for EventBus<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Clone + Send + 'static> std::fmt::Debug for EventBus<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.lock();
        f.debug_struct("EventBus")
            .field("channel_subscribers", &guard.channels.len())
            .field("callback_subscribers", &guard.callbacks.len())
            .finish()
    }
}

impl<E: Clone + Send + 'static> EventBus<E> {
    /// Creates a new empty event bus.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                channels: Vec::new(),
                callbacks: Vec::new(),
                next_id: AtomicU64::new(1),
            })),
        }
    }

    /// Subscribes to events using a standard library channel receiver.
    pub fn subscribe(&self) -> Receiver<E> {
        let (tx, rx) = mpsc::channel();
        self.inner.lock().channels.push(tx);
        rx
    }

    /// Registers a callback invoked whenever an event is published.
    pub fn on<F>(&self, callback: F) -> SubscriptionId
    where
        F: Fn(&E) + Send + Sync + 'static,
    {
        let mut guard = self.inner.lock();
        let id = SubscriptionId(guard.next_id.fetch_add(1, Ordering::Relaxed));
        guard.callbacks.push((id, Arc::new(callback)));
        id
    }

    /// Unregisters a previously registered callback.
    pub fn unsubscribe(&self, id: SubscriptionId) {
        self.inner.lock().callbacks.retain(|(sub_id, _)| *sub_id != id);
    }

    /// Publishes an event to all active channel and callback subscribers.
    pub fn publish(&self, event: E) {
        let callbacks: Vec<Callback<E>> = {
            let mut guard = self.inner.lock();
            guard.channels.retain(|tx| tx.send(event.clone()).is_ok());
            guard.callbacks.iter().map(|(_, cb)| Arc::clone(cb)).collect()
        };
        for cb in callbacks {
            cb(&event);
        }
    }
}
