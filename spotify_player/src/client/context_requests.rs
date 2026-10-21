use std::{collections::HashMap, sync::Arc, time::Duration};

use parking_lot::Mutex;
use tokio::time::Instant;

#[derive(Default)]
struct Attempt {
    running: bool,
    failures: u32,
    retry_at: Option<Instant>,
}

#[derive(Clone, Default)]
pub(super) struct ContextRequests(Arc<Mutex<HashMap<String, Attempt>>>);

pub(super) struct ContextRequest {
    requests: ContextRequests,
    uri: String,
    succeeded: bool,
}

impl ContextRequests {
    pub(super) fn begin(&self, uri: String) -> Option<ContextRequest> {
        let now = Instant::now();
        let mut requests = self.0.lock();
        requests.retain(|_, attempt| {
            attempt.running
                || attempt.retry_at.is_some_and(|retry_at| {
                    now.saturating_duration_since(retry_at) < Duration::from_secs(600)
                })
        });
        let attempt = requests.entry(uri.clone()).or_default();
        if attempt.running || attempt.retry_at.is_some_and(|retry_at| retry_at > now) {
            return None;
        }
        attempt.running = true;
        Some(ContextRequest {
            requests: self.clone(),
            uri,
            succeeded: false,
        })
    }
}

impl ContextRequest {
    pub(super) fn complete(&mut self, succeeded: bool) {
        self.succeeded = succeeded;
    }
}

impl Drop for ContextRequest {
    fn drop(&mut self) {
        let mut requests = self.requests.0.lock();
        if self.succeeded {
            requests.remove(&self.uri);
        } else if let Some(attempt) = requests.get_mut(&self.uri) {
            // Cancellation and panics must also release the in-flight slot.
            attempt.running = false;
            attempt.failures = attempt.failures.saturating_add(1);
            let delay = Duration::from_secs(30 * 2_u64.pow(attempt.failures.min(5) - 1))
                .min(Duration::from_secs(300));
            attempt.retry_at = Some(Instant::now() + delay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ContextRequests;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn suppresses_duplicates_and_backs_off_failed_loads() {
        let requests = ContextRequests::default();
        let uri = "spotify:artist:abba".to_string();
        let first = requests.begin(uri.clone()).unwrap();
        tokio::time::advance(Duration::from_secs(90)).await;
        assert!(requests.begin(uri.clone()).is_none());
        assert!(requests.begin("spotify:artist:other".to_string()).is_some());
        drop(first);
        assert!(requests.begin(uri.clone()).is_none());
        tokio::time::advance(Duration::from_secs(30)).await;
        drop(requests.begin(uri.clone()).unwrap());
        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(requests.begin(uri.clone()).is_none());
        tokio::time::advance(Duration::from_secs(30)).await;
        let mut successful = requests.begin(uri.clone()).unwrap();
        successful.complete(true);
        drop(successful);
        assert!(requests.begin(uri).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn caps_backoff_and_releases_cancelled_loads() {
        let requests = ContextRequests::default();
        let uri = "spotify:artist:abba".to_string();
        for seconds in [30, 60, 120, 240, 300, 300] {
            let request = requests.begin(uri.clone()).unwrap();
            let task = tokio::spawn(async move {
                let _request = request;
                std::future::pending::<()>().await;
            });
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
            tokio::time::advance(Duration::from_secs(seconds - 1)).await;
            assert!(requests.begin(uri.clone()).is_none());
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert!(requests.begin(uri).is_some());
    }
}
