// TODO: Convert it to lockless (e.g. to not block audio mixer thread)
// and use thread park/unpark on sender-side?
use std::iter;
use std::sync::Arc;

use crate::util::MuCo;

pub fn circbuf<T: Copy + Default>(len: usize) -> (Sender<T>, Receiver<T>) {
    assert!(len > 0);

    let buf = Box::from_iter(iter::repeat_n(T::default(), len));

    let inner = Inner {
        buf,
        send_i: 0,
        recv_i: 0,
        filled: 0,
        send_alive: true,
        recv_alive: true,
    };

    let inner_muco = Arc::new(MuCo::new(inner));

    let sender = Sender::new(Arc::clone(&inner_muco));
    let receiver = Receiver::new(Arc::clone(&inner_muco));

    (sender, receiver)
}

type InnerMuCo<T> = Arc<MuCo<Inner<T>>>;

struct Inner<T> {
    buf: Box<[T]>,
    send_i: usize, // TODO: It can be moved to Sender struct.
    recv_i: usize, // TODO: It can be moved to Receiver struct.
    filled: usize,
    send_alive: bool,
    recv_alive: bool
}

pub struct Sender<T: Copy> { // TODO: why do we need Copy here?
    inner_muco: InnerMuCo<T>,
}

impl<T: Copy> Sender<T> {
    fn new(inner_muco: InnerMuCo<T>) -> Self {
        Self {
            inner_muco,
        }
    }

    pub fn send(&self, buf: &[T]) -> bool {
        let buf_len = buf.len();
        let mut i: usize = 0;

        loop {
            let mut todo = buf_len - i;
            if todo == 0 {
                break;
            }

            let inner = self.inner_muco.mutex.lock().unwrap();
            let inner_buf_len = inner.buf.len();

            let mut inner = self.inner_muco.cond.wait_while(inner, |inner| inner.recv_alive && inner.filled == inner_buf_len).unwrap(); // While full, we need to wait.

            if !inner.recv_alive { // Receiver has been dropped.
                return false;
            }

            todo = [inner_buf_len - inner.filled,     // available space in circ buffer
                    todo].into_iter().min().unwrap(); // available data in source buffer
            assert!(todo > 0);

            inner.filled += todo;

            while todo > 0 {
                let inner_send_i = inner.send_i;
                let copy = [inner_buf_len - inner_send_i, // can be copied without wrap
                                   todo].into_iter().min().unwrap();
                assert!(copy > 0);

                let inner_buf_chunk = &mut inner.buf[inner_send_i..inner_send_i + copy];
                inner_buf_chunk.copy_from_slice(&buf[i..i + copy]);

                inner.send_i += copy;
                if inner.send_i == inner_buf_len {
                    inner.send_i = 0;
                }

                i += copy;
                todo -= copy;
            }

            self.notify();
        }

        true
    }

    fn notify(&self) {
        self.inner_muco.cond.notify_all(); // Notify receiver.
    }
}

impl<T: Copy> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut inner = self.inner_muco.mutex.lock().unwrap();
        inner.send_alive = false;

        self.notify();
    }
}

pub struct Receiver<T: Copy> { // TODO: why do we need Copy here?
    inner_muco: InnerMuCo<T>,
} 

impl<T: Copy> Receiver<T> {
    fn new(inner_muco: InnerMuCo<T>) -> Self {
        Self {
            inner_muco,
        }
    }

    pub fn recv(&self, buf: &mut [T]) -> usize {
        let buf_len = buf.len();
        let mut i: usize = 0;

        loop {
            let mut todo = buf_len - i;
            if todo == 0 {
                break;
            }

            let inner = self.inner_muco.mutex.lock().unwrap();
            let inner_buf_len = inner.buf.len();

            let mut inner = self.inner_muco.cond.wait_while(inner, |inner| inner.send_alive && inner.filled == 0).unwrap(); // While empty, we need to wait.

            todo = [inner.filled,                     // available data in circ buffer
                    todo].into_iter().min().unwrap(); // available space in destination buffer

            // If the sender has been dropped, we can still consume remaining data.

            if todo == 0 {
                assert!(!inner.send_alive);
                break;
            }

            inner.filled -= todo;

            while todo > 0 {
                let inner_recv_i = inner.recv_i;
                let copy = [inner_buf_len - inner_recv_i, // can be copied without wrap
                                   todo].into_iter().min().unwrap();
                assert!(copy > 0);

                let buf_chunk = &mut buf[i..i + copy];
                buf_chunk.copy_from_slice(&inner.buf[inner_recv_i..inner_recv_i + copy]);

                inner.recv_i += copy;
                if inner.recv_i == inner_buf_len {
                    inner.recv_i = 0;
                }

                i += copy;
                todo -= copy;
            }

            self.notify();
        }

        i
    }

    pub fn wait_full(&self) {
        let inner = self.inner_muco.mutex.lock().unwrap();
        let inner_buf_len = inner.buf.len();

        let _inner = self.inner_muco.cond.wait_while(inner, |inner| inner.send_alive && inner.filled < inner_buf_len).unwrap(); // Wait until full.
    }

    fn notify(&self) {
        self.inner_muco.cond.notify_all(); // Notify sender.
    }
}

impl<T: Copy> Drop for Receiver<T> {
    fn drop(&mut self) {
        let mut inner = self.inner_muco.mutex.lock().unwrap();
        inner.recv_alive = false;

        self.notify();
    }
}
