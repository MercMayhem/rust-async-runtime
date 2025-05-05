use std::{pin::Pin, task::{Context, Poll}};

use lib::executor::Executor;

mod lib;

enum State {
    Step1,
    Step2,
    Done,
}

pub struct MultiStep {
    state: State,
}

impl MultiStep {
    pub fn new() -> Self {
        Self { state: State::Step1 }
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
                self.state = Done;
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

fn main() {
    let multi_step = MultiStep::new();
    Executor::init(Box::pin(multi_step));
}
