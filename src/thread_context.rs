use std::sync::Mutex;
use std::{sync::Arc, thread_local};

use super::reactor::Reactor;

pub struct ThreadContext {
    pub reactor: Arc<Reactor>,
}

pub struct ThreadContextGuard {
    old_context: Option<ThreadContext>,
}

thread_local! {
    pub static CONTEXT: Mutex<Option<ThreadContext>> = Mutex::new(None);
}

pub fn set_thread_context() -> ThreadContextGuard {
    let old_context = CONTEXT.with(|reactor| {
        let old = reactor.lock().unwrap().take();
        reactor.lock().unwrap().replace(ThreadContext {
            reactor: Arc::new(Reactor::new()),
        });
        old
    });

    ThreadContextGuard { old_context }
}

fn reset_thread_context(old_context: Option<ThreadContext>) {
    CONTEXT.with(|reactor| {
        let mut guard = reactor.lock().unwrap();
        *guard = old_context;
    });
}

impl Drop for ThreadContextGuard {
    fn drop(&mut self) {
        reset_thread_context(self.old_context.take());
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_thread_context() {
        let guard = set_thread_context();
        assert!(CONTEXT.with(|c| c.lock().unwrap().is_some()));

        drop(guard);
        assert!(CONTEXT.with(|c| c.lock().unwrap().is_none()));
    }

    #[test]
    fn test_thread_context_guard_drop() {
        {
            let _guard = set_thread_context();
            assert!(CONTEXT.with(|c| c.lock().unwrap().is_some()));
        }
        assert!(CONTEXT.with(|c| c.lock().unwrap().is_none()));
    }
}
