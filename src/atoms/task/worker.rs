//! Background threads that report back over one channel.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// One channel, and the threads that send on it. `M` is what they report.
pub struct Worker<M> {
    tx: Sender<M>,
    rx: Receiver<M>,
}

impl<M: Send + 'static> Default for Worker<M> {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }
}

impl<M: Send + 'static> Worker<M> {
    /// Runs `work` on its own thread. It reports through the sender.
    pub fn spawn(&self, work: impl FnOnce(&Sender<M>) + Send + 'static) {
        let tx = self.tx.clone();
        thread::spawn(move || work(&tx));
    }

    /// Everything reported since the last call, oldest first. Never waits.
    pub fn drain(&self) -> Vec<M> {
        let mut messages = Vec::new();
        while let Ok(message) = self.rx.try_recv() {
            messages.push(message);
        }
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn what_a_thread_sends_arrives_in_order() {
        let worker: Worker<u32> = Worker::default();
        worker.spawn(|tx| {
            for number in 1..=3 {
                let _ = tx.send(number);
            }
        });
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while got.len() < 3 && Instant::now() < deadline {
            got.extend(worker.drain());
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, [1, 2, 3]);
    }

    #[test]
    fn draining_with_nothing_sent_gives_nothing() {
        let worker: Worker<u32> = Worker::default();
        assert!(worker.drain().is_empty());
    }
}
