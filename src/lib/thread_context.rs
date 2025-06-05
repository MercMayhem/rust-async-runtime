use std::sync::Mutex;
use std::{sync::Arc, thread_local};

use super::reactor::Reactor;

pub struct ThreadContext {
    reactor: Arc<Reactor>,
}

pub struct ThreadContextGuard {
    old_context: Option<ThreadContext>,
}

thread_local! {
    pub static REACTOR: Mutex<Option<ThreadContext>> = Mutex::new(None);
}

pub fn set_thread_context() -> ThreadContextGuard {
    let old_context = REACTOR.with(|reactor| {
        let old = reactor.lock().unwrap().take();
        reactor.lock().unwrap().replace(ThreadContext {
            reactor: Arc::new(Reactor::new()),
        });
        old
    });

    ThreadContextGuard { old_context }
}

fn reset_thread_context(old_context: Option<ThreadContext>) {
    REACTOR.with(|reactor| {
        let mut guard = reactor.lock().unwrap();
        *guard = old_context;
    });
}

impl Drop for ThreadContextGuard {
    fn drop(&mut self) {
        reset_thread_context(self.old_context.take());
    }
}
