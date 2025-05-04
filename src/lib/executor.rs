use std::sync::mpsc::{Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::pin::Pin;

use futures::task::{waker, ArcWake};
use crate::lib::reactor::Reactor;

pub struct Task{
    future: Mutex<Option<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>>,
    sender: Sender<Arc<Task>>,
    executor: Arc<Executor>,
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        // Notify the executor to poll this task again
        let _ = arc_self.sender.send(arc_self.clone());
    }
}

struct Executor {
    task_queue: Mutex<Receiver<Arc<Task>>>,
    reactor: Reactor
}

impl Executor {
    fn new(task_queue: Receiver<Arc<Task>>) -> Self {
        let reactor = Reactor::new();
        let task_queue = Mutex::new(task_queue);
        Executor { task_queue, reactor }
    }

    // TODO: Add tracing
    fn run(&mut self) {
        loop {
            // Wait for tasks to be available
            if let Ok(task) = self.task_queue.lock().unwrap().try_recv() {

                // Get MutexGuard for the task's future. Need the mutex to mutate Arc.
                let mut locked_future = task.future.lock().unwrap();

                // Temporarily take the future out of Option
                if let Some(mut owned_task) = locked_future.take() {
                    let waker = waker(task.clone());
                    let mut context = Context::from_waker(&waker);

                    *locked_future = match owned_task.as_mut().poll(&mut context) {
                        Poll::Pending => {
                            // Task is not ready, put it back in the queue
                            Some(owned_task)
                        },

                        Poll::Ready(_) => {
                            // Task is complete, drop it
                            None
                        }
                    };
                }
            }
            
            // TODO: Need to poll for events
            self.reactor.wait_and_wake();
        }
    }
}
