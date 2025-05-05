use std::{collections::HashMap, time::Duration};
use std::sync::Arc;
use std::task::Waker;

use std::sync::Mutex;
use polling::{AsRawSource, AsSource, Event, Events, Poller};

pub enum IoEventType {
    Readable,
    Writable,
    ReadableWritable,
}

pub struct Reactor{
    poller: Poller,
    event_map: Arc<Mutex<HashMap<usize, Waker>>>,
}

impl Reactor {
    pub fn new() -> Self {
        let poller = Poller::new().expect("Failed to create poller");
        let event_map = Arc::new(Mutex::new(HashMap::new()));
        Reactor { poller, event_map }
    }

    pub fn register(
        &self,
        fd: impl AsRawSource,
        event_type: IoEventType,
        waker: Waker,
    ) -> Result<usize, std::io::Error> {
        let mut event_map = self.event_map.lock().unwrap();
        // Generate key using max key value + 1
        let key = event_map.keys()
            .max()
            .map(|max| *max)
            .unwrap_or(0) + 1;

        let event = match event_type {
            IoEventType::Readable => Event::readable(key),
            IoEventType::Writable => Event::writable(key),
            IoEventType::ReadableWritable => Event::all(key)
        };
        unsafe {
            self.poller.add(fd, event)?;
        }

        event_map.insert(key, waker);

        Ok(key)
    }

    // TODO: Find a way to only use key not both key and fd
    pub fn unregister(
        &self,
        key: usize,
        fd: impl AsSource,
    ) -> Result<(), std::io::Error> {
        let mut event_map = self.event_map.lock().unwrap();
        if let Some(_) = event_map.remove(&key) {
            self.poller.delete(fd)?;
        }

        event_map.remove(&key);

        Ok(())
    }

    pub fn wait_and_wake(&self) -> Result<(), std::io::Error> {
        let mut events = Events::new();
        // TODO: Need to look into if timeout is required
        self.poller.wait(&mut events, None)?;

        let mut event_map = self.event_map.lock().unwrap();
        for event in events.iter() {
            if let Some(waker) = event_map.get(&event.key) {
                waker.wake_by_ref();
                event_map.remove(&event.key);
            }
        }

        Ok(())
    }
}

// TODO: Unit tests for Reactor
