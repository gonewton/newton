use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// Simple bounded queue with notification support for ailoop workers.
#[derive(Debug)]
pub struct BoundedQueue<T> {
    queue: Mutex<VecDeque<T>>,
    capacity: usize,
    notify: Notify,
}

impl<T> BoundedQueue<T> {
    /// Create a queue limited to the provided capacity.
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(BoundedQueue {
            queue: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
            notify: Notify::new(),
        })
    }

    /// Push an item, dropping the oldest entry when the queue is full.
    pub fn push(&self, item: T) {
        let mut guard = self.queue.lock().unwrap();
        if guard.len() >= self.capacity {
            guard.pop_front();
        }
        guard.push_back(item);
        self.notify.notify_one();
    }

    /// Attempt to take a single item without waiting.
    pub fn try_pop(&self) -> Option<T> {
        let mut guard = self.queue.lock().unwrap();
        guard.pop_front()
    }

    /// Return the current number of pending items.
    pub fn len(&self) -> usize {
        let guard = self.queue.lock().unwrap();
        guard.len()
    }

    /// Check whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Await the next queued value.
    pub async fn next(&self) -> T {
        loop {
            if let Some(item) = self.try_pop() {
                return item;
            }
            self.notify.notified().await;
        }
    }

    /// Wake a single waiting consumer.
    pub fn notify_one(&self) {
        self.notify.notify_one();
    }
}
