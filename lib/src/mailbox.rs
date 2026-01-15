// TODO: Convert it to lockless
use std::sync::{Arc, Mutex};

pub fn mailbox<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Inner {
        value_opt: None,
        send_alive: 1,
        recv_alive: true,
    };
    let inner_mutex = Arc::new(Mutex::new(inner));

    let sender = Sender::new(Arc::clone(&inner_mutex));
    let receiver = Receiver::new(Arc::clone(&inner_mutex));

    (sender, receiver)
}

type InnerMutexRc<T> = Arc<Mutex<Inner<T>>>;

struct Inner<T> {
    value_opt: Option<T>,
    send_alive: usize,
    recv_alive: bool,
}

pub struct Sender<T> {
    inner_mutex: InnerMutexRc<T>,
}

impl<T> Sender<T> {
    fn new(inner_mutex: InnerMutexRc<T>) -> Self {
        Self {
            inner_mutex,
        }
    }

    pub fn send(&self, value: T) -> Result<(), ()> {
        let mut inner = self.inner_mutex.lock().unwrap();

        if !inner.recv_alive {
            return Err(());
        }

        inner.value_opt = Some(value);
        Ok(())
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        let mut inner = self.inner_mutex.lock().unwrap();
        inner.send_alive += 1;

        Self {
            inner_mutex: Arc::clone(&self.inner_mutex),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut inner = self.inner_mutex.lock().unwrap();
        inner.send_alive -= 1;
    }
}

pub enum TryRecvError {
    Empty,
    Disconnected,
}

pub struct Receiver<T> {
    inner_mutex: InnerMutexRc<T>,
}

impl<T> Receiver<T> {
    fn new(inner_mutex: InnerMutexRc<T>) -> Self {
        Self {
            inner_mutex,
        }
    }

    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let mut inner = self.inner_mutex.lock().unwrap();

        // If the sender has been dropped, we can still consume remaining data.

        if let Some(value) = inner.value_opt.take() {
            return Ok(value);
        }

        if inner.send_alive == 0 {
            return Err(TryRecvError::Disconnected);
        }

        Err(TryRecvError::Empty)
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let mut inner = self.inner_mutex.lock().unwrap();
        inner.recv_alive = false;
    }
}
