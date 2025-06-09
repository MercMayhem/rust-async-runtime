use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::task::{ArcWake, waker};

use super::thread_context::{CONTEXT, set_thread_context};

pub struct Task {
    // TODO: See if it can be generalized to any type of future output
    future: Mutex<Option<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>>,
    sender: Sender<Arc<Task>>,
    pub executor: Arc<Executor>,
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        // Notify the executor to poll this task again
        let _ = arc_self.sender.send(arc_self.clone());
    }
}

pub struct Executor {
    task_queue: Mutex<Receiver<Arc<Task>>>,
}

// TODO: Create static methods for Executor to access reactor and thread context
impl Executor {
    fn new(task_queue: Receiver<Arc<Task>>) -> Self {
        let task_queue = Mutex::new(task_queue);
        Executor { task_queue }
    }

    pub fn init(future: Pin<Box<dyn Future<Output = ()> + Send + Sync>>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let executor = Arc::new(Executor::new(receiver));
        let task = Arc::new(Task {
            future: Mutex::new(Some(future)),
            sender,
            executor: executor.clone(),
        });

        // Send the task to the executor
        let _ = task.sender.send(task.clone());

        // Start the executor
        executor.run();
    }

    // TODO: Add tracing
    fn run(&self) {
        let _guard = set_thread_context();
        loop {
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
                            }

                            Poll::Ready(_) => {
                                // Task is complete, drop it
                                None
                            }
                        };
                    }
                } else {
                    break;
                }
            }

            if CONTEXT.with(|thread_context| {
                if let Some(context) = thread_context.lock().unwrap().as_ref() {
                    if context.reactor.event_map.lock().unwrap().len() > 0 {
                        // TODO: Perform better error handling
                        let _ = context.reactor.wait_and_wake();
                    } else {
                        return true;
                    }
                } else {
                    panic!("No reactor found in thread context. This is a bug.");
                }

                false
            }) {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
        pin::Pin,
        task::{Context, Poll},
        thread,
    };

    use crate::{reactor::IoEventType, thread_context::CONTEXT};

    use super::Executor;

    enum State {
        Step1,
        Step2,
        Step3,
        Step4,
        Done,
    }

    // TODO: Enforce that a compatible future must hold a key
    pub struct MultiStep {
        fd: UnixStream,
        state: State,
        key: Option<usize>,
    }

    impl MultiStep {
        pub fn new(fd: UnixStream) -> Self {
            Self {
                fd,
                state: State::Step1,
                key: None,
            }
        }
    }

    impl Future for MultiStep {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            use State::*;

            match self.state {
                Step1 => {
                    println!("Step 1 in progress...");
                    self.state = Step2;
                    cx.waker().wake_by_ref(); // Simulate readiness for next step
                    Poll::Pending
                }
                Step2 => {
                    println!("Step 2 in progress...");
                    self.state = Step3;
                    cx.waker().wake_by_ref(); // Simulate readiness again
                    Poll::Pending
                }
                Step3 => {
                    println!("Step 3 in progress...");
                    self.state = Step4;

                    CONTEXT.with(|thread_context| {
                        if let Some(context) = thread_context.lock().unwrap().as_ref() {
                            self.key = Some(
                                context
                                    .reactor
                                    .register(&self.fd, IoEventType::Readable, cx.waker().clone())
                                    .unwrap(),
                            );
                        } else {
                            panic!("No reactor found in thread context. This is a bug.");
                        }
                    });

                    Poll::Pending
                }
                Step4 => {
                    println!("Step 4 in progress...");
                    // Simulate a read operation
                    let mut buf = [0; 1024];
                    self.fd.read(&mut buf).unwrap();

                    println!("Read data: {}", String::from_utf8_lossy(&buf));
                    self.state = Done;

                    // TODO: create a generic function which automatically panics if no reactor is
                    // found. To avoid repeating this code.
                    CONTEXT.with(|thread_context| {
                        if let Some(context) = thread_context.lock().unwrap().as_ref() {
                            context
                                .reactor
                                .unregister(self.key.unwrap(), &self.fd)
                                .unwrap();
                        } else {
                            panic!("No reactor found in thread context. This is a bug.");
                        }
                    });

                    cx.waker().wake_by_ref(); // Simulate readiness again
                    Poll::Pending
                }
                Done => {
                    println!("All steps complete!");
                    Poll::Ready(())
                }
            }
        }
    }

    #[test]
    fn test_executor() {
        // Create a UnixStream for testing
        let (mut sender, receiver) = UnixStream::pair().unwrap();

        println!("Sender: {:?}", sender.local_addr().unwrap());
        println!("Receiver: {:?}", receiver.local_addr().unwrap());

        let future = Box::pin(MultiStep::new(receiver));
        let handle = thread::spawn(|| {
            Executor::init(future);
        });

        thread::sleep(std::time::Duration::from_secs(1));
        sender.write_all(b"Hello, world!").unwrap();
        println!("Data sent!");
        handle.join().unwrap();
    }

    #[test]
    fn test_thread_local_context_integration() {
        assert!(CONTEXT.with(|thread_context| thread_context.lock().unwrap().is_none()));

        Executor::init(Box::pin(async {
            CONTEXT.with(|thread_context| {
                let context = thread_context.lock().unwrap();
                assert!(context.is_some(), "Thread context should be set");
            });
        }));

        assert!(CONTEXT.with(|thread_context| thread_context.lock().unwrap().is_none()));
    }
}
