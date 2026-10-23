//! Cancellation of a dial in progress, shared by the native and the browser transports.

use std::sync::Arc;
use tokio::sync::watch;

/// Lets another thread (or task) abort a dial in progress (`connect_ticket_cancellable`): the attempt ends at once instead of at
/// `connect_timeout`. Cloning shares the same token.
#[derive(Clone, Debug)]
pub struct CancelToken(Arc<watch::Sender<bool>>);

impl Default for CancelToken {
    fn default() -> Self {
        Self(Arc::new(watch::channel(false).0))
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    pub(crate) async fn cancelled(&self) {
        let mut rx = self.0.subscribe();
        // The wrapper drops the watch guard at once (it is not `Send`).
        async {
            rx.wait_for(|c| *c).await.ok();
        }
        .await;
    }
}
