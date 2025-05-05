use std::sync::mpsc::{Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::pin::Pin;

use futures::task::{waker, ArcWake};
use crate::lib::reactor::Reactor;

pub struct Task{
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
    reactor: Reactor
}

impl Executor {
    fn new(task_queue: Receiver<Arc<Task>>) -> Self {
        let reactor = Reactor::new();
        let task_queue = Mutex::new(task_queue);
        Executor { task_queue, reactor }
    }

    pub fn init(future: Pin<Box<dyn Future<Output = ()> + Send + Sync>>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut executor = Arc::new(Executor::new(receiver));
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
                            },

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
            
            if self.reactor.event_map.lock().unwrap().len() > 0 {
                self.reactor.wait_and_wake();
            } else {
                break;
            }
        }
    }
}

// TODO: Unit tests for Executor
#[cfg(test)]
mod tests {
    use std::{io::{Read, Write}, os::unix::net::UnixStream, pin::Pin, task::{Context, Poll}, thread};

    use crate::lib::reactor::IoEventType;

    use super::{Executor, Task};

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
        key: Option<usize>
    }

    impl MultiStep {
        pub fn new(fd: UnixStream) -> Self {
            Self {
                fd,
                state: State::Step1,
                key: None
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
                    cx.waker().wake_by_ref();  // Simulate readiness for next step
                    Poll::Pending
                }
                Step2 => {
                    println!("Step 2 in progress...");
                    self.state = Step3;
                    cx.waker().wake_by_ref();  // Simulate readiness again
                    Poll::Pending
                }
                Step3 => {
                    println!("Step 3 in progress...");
                    self.state = Step4;
                    let task: *const Task = cx.waker().data().cast();
                    if !task.is_aligned(){
                        panic!("Task is not aligned");
                    }

                    // Register Read event in reactor
                    unsafe {
                        let executor = (*task).executor.clone();
                        self.key = Some(executor.reactor.register(
                            &self.fd,
                            IoEventType::Readable,
                            cx.waker().clone()
                        ).unwrap());
                    }

                    Poll::Pending
                }
                Step4 => {
                    println!("Step 4 in progress...");
                    // Simulate a read operation
                    let mut buf = [0; 1024];
                    self.fd.read_exact(&mut buf).unwrap();

                    println!("Read data: {}", String::from_utf8_lossy(&buf));
                    self.state = Done;

                    let task: *const Task = cx.waker().data().cast();
                    if !task.is_aligned(){
                        panic!("Task is not aligned");
                    }
                    // Unregister the event
                    unsafe {
                        let executor = (*task).executor.clone();
                        executor.reactor.unregister(
                            self.key.unwrap(),
                            &self.fd
                        ).unwrap();
                    }

                    cx.waker().wake_by_ref();  // Simulate readiness again
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
}
