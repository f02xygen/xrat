use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[derive(Clone)]
pub struct CancellationFlag {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Default for CancellationFlag {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationFlag {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for CancellationFlag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CancellationFlag")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

#[derive(Clone)]
pub struct CancellationReceiver {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Default for CancellationReceiver {
    fn default() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }
}

impl CancellationReceiver {
    pub fn from_flag(flag: &CancellationFlag) -> Self {
        Self {
            flag: flag.flag.clone(),
            notify: flag.notify.clone(),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

impl std::fmt::Debug for CancellationReceiver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CancellationReceiver")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{CancellationFlag, CancellationReceiver};

    #[test]
    fn flag_starts_uncancelled() {
        let flag = CancellationFlag::new();
        assert!(!flag.is_cancelled());
    }

    #[test]
    fn cancel_marks_flag_and_receiver() {
        let flag = CancellationFlag::new();
        let receiver = CancellationReceiver::from_flag(&flag);

        flag.cancel();

        assert!(flag.is_cancelled());
        assert!(receiver.is_cancelled());
    }

    #[test]
    fn receiver_outlives_flag() {
        let flag = CancellationFlag::new();
        let receiver = CancellationReceiver::from_flag(&flag);
        drop(flag);

        assert!(!receiver.is_cancelled());
    }

    #[test]
    fn clones_share_state() {
        let flag = CancellationFlag::new();
        let cloned = flag.clone();

        flag.cancel();

        assert!(cloned.is_cancelled());
    }

    #[tokio::test]
    async fn receiver_cancelled_future_resolves_when_flag_cancelled() {
        let flag = CancellationFlag::new();
        let receiver = CancellationReceiver::from_flag(&flag);

        let handle = tokio::spawn(async move {
            receiver.cancelled().await;
            true
        });

        tokio::task::yield_now().await;
        assert!(!handle.is_finished());

        flag.cancel();
        assert!(handle.await.unwrap());
    }

    #[tokio::test]
    async fn receiver_cancelled_resolves_immediately_if_already_cancelled() {
        let flag = CancellationFlag::new();
        flag.cancel();
        let receiver = CancellationReceiver::from_flag(&flag);

        tokio::time::timeout(std::time::Duration::from_millis(50), receiver.cancelled())
            .await
            .expect("should resolve immediately");
    }
}
