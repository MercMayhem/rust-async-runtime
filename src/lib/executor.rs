use std::sync::mpsc::{Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::task::{Context, Wake, Waker};
use std::pin::Pin;

use polling::Poller;

struct Task{
    future: Mutex<Option<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>>,
    sender: Sender<Arc<Task>>,
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        // Notify the executor to poll this task again
        let _ = self.sender.send(self.clone());
    }
}

struct Executor {
    task_queue: Receiver<Arc<Task>>,
    poller: Poller
}

impl Executor {
    fn new(task_queue: Receiver<Arc<Task>>) -> Self {
        let poller = Poller::new().expect("Failed to create poller");
        Executor { task_queue, poller }
    }

    fn run(&mut self) {
        loop {
            // Wait for tasks to be available
            if let Ok(task) = self.task_queue.try_recv() {
                let mut locked_future = task.future.lock().unwrap();
                if let Some(mut owned_task) = locked_future.take() {
                    let waker = Waker::from(task.clone());
                    let mut context = Context::from_waker(&waker);

                    let _result = owned_task.as_mut().poll(&mut context);
                }
            }
            
            // TODO Need to poll for events
        }
    }
}
