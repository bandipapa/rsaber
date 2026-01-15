use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::Client;
use tokio::runtime::Runtime;

#[cfg(target_os = "android")]
use reqwest::Certificate;

use crate::{APP_NAME, APP_VERSION};

const IN_PROG_MAX: u32 = 16;
const TIMEOUT: u64 = 60; // [s]

pub trait Request {
    type Response: Send;
    type Error: Send;

    fn exec(self, client: Client) -> impl Future<Output = Result<Self::Response, Self::Error>> + Send; // TODO: Would be nice if we could use async fn.
}

type ExecFut = Pin<Box<dyn Future<Output = ()> + Send>>; // TODO: Use BoxFuture?
type CancelFlag = Arc<AtomicBool>;

pub struct NetManager {
    inner: InnerRc,
}

type InnerRc = Arc<Inner>;

struct Inner {
    async_runtime: Arc<Runtime>,
    client: Client,
    queue_info_mutex: Mutex<QueueInfo>,
}

struct QueueInfo {
    queue: VecDeque<Box<dyn FnOnce() -> ExecFut + Send>>,
    in_prog: u32,
}

impl NetManager {
    pub fn new(async_runtime: Arc<Runtime>) -> Self {
        let client = Client::builder() // TODO: add response limit: https://github.com/seanmonstar/reqwest/issues/1234
            .user_agent(format!("{}/{}", APP_NAME, *APP_VERSION))
            .timeout(Duration::from_secs(TIMEOUT));

        // Android: according to https://github.com/seanmonstar/reqwest/pull/2891 , the
        // preferred way to do certificate validation is to use rustls-platform-verifier.
        // TODO: Call rustls_platform_verifier::android::init*.

        #[cfg(target_os = "android")]
        let client = client.tls_certs_only(webpki_root_certs::TLS_SERVER_ROOT_CERTS.into_iter().map(|cert| Certificate::from_der(cert).expect("Failed to parse certificate")));

        let client = client.build().expect("Unable to build client");

        let queue_info = QueueInfo {
            queue: VecDeque::new(),
            in_prog: 0,
        };

        let inner = Arc::new(Inner {
            async_runtime,
            client,
            queue_info_mutex: Mutex::new(queue_info),
        });

        Self {
            inner,
        }
    }

    pub fn create_executor<T: NetManagerRunner + Clone + Send + 'static>(&self, runner: T) -> NetManagerExecutor<T> {
        // Intented usage: the done_func parameter of NetManagerExecutor.submit() are supposed to be
        // executed on the same thread which called NetManagerExecutor.submit().

        NetManagerExecutor::new(Arc::clone(&self.inner), runner)
    }
}

pub trait NetManagerRunner {
    fn exec_done<T: FnOnce() + Send + 'static>(&self, func: T);
}

#[derive(Clone)]
pub struct NetManagerExecutor<T> {
    inner: InnerRc,
    runner: T,
}

impl<T: NetManagerRunner + Clone + Send + 'static> NetManagerExecutor<T> {
    fn new(inner: InnerRc, runner: T) -> Self {
        Self {
            inner,
            runner,
        }
    }

    pub fn submit<R: Request + Send + 'static, D: FnOnce(Result<R::Response, R::Error>) + Send + 'static>(&self, req: R, done_func: D) -> Handle {
        // TODO: It would be nice if we could remove Send requirement on done_func, as
        // it would simplify the consumers as well.
        // Push into queue.

        let cancel_flag = Arc::new(AtomicBool::new(false));

        {
            let mut queue_info = self.inner.queue_info_mutex.lock().unwrap();

            let func = {
                let client = self.inner.client.clone();
                let runner = self.runner.clone();
                let cancel_flag = Arc::clone(&cancel_flag);

                || {
                    Box::pin(async move {
                        if cancel_flag.load(Ordering::Relaxed) { // Cancel: before check.
                            return;
                        }

                        let r = req.exec(client).await; // TODO: Drop future if cancelled during await.

                        runner.exec_done(move || {
                            if !cancel_flag.load(Ordering::Relaxed) { // Cancel: after check.
                                done_func(r);
                            }
                        });
                    }) as ExecFut
                }
            };

            queue_info.queue.push_back(Box::new(func));
        }

        // Execute if there is a free slot.

        Self::run(Arc::clone(&self.inner), false); // TODO: Avoid double lock (above + run())

        Handle::new(cancel_flag)
    }

    fn run(inner: InnerRc, finished: bool) {
        let mut queue_info = inner.queue_info_mutex.lock().unwrap();

        if finished {
            queue_info.in_prog -= 1;
        }

        if queue_info.in_prog < IN_PROG_MAX && let Some(func) = queue_info.queue.pop_front() {
            inner.async_runtime.spawn({
                let inner = Arc::clone(&inner);
                async move {
                    func().await;
                    Self::run(inner, true);
                }
            });

            queue_info.in_prog += 1;
        }
    }
}

pub struct Handle {
    cancel_flag: CancelFlag,
}

impl Handle {
    fn new(cancel_flag: CancelFlag) -> Self {
        Self {
            cancel_flag,
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }
}
