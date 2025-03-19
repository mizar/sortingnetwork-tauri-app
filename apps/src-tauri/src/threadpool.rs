// based on https://doc.rust-lang.org/stable/book/ch21-03-graceful-shutdown-and-cleanup.html
#![allow(unused)]

use std::sync::{Arc, Mutex, mpsc};
use std::thread;
type Job = Box<dyn FnOnce() + Send + 'static>;

struct Worker(Option<thread::JoinHandle<()>>);

impl Worker {
    fn new(receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || {
            loop {
                let message = receiver.lock().unwrap().recv();
                match message {
                    Ok(job) => {
                        job();
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        });

        Worker(Some(thread))
    }
}

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::Sender<Job>>,
}

impl ThreadPool {
    /// Create a new ThreadPool.
    ///
    /// The size is the number of threads in the pool.
    ///
    /// # Panics
    ///
    /// The `new` function will panic if the size is zero.
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);
        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));
        ThreadPool {
            workers: (0..size)
                .map(|_| Worker::new(Arc::clone(&receiver)))
                .collect(),
            sender: Some(sender),
        }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.as_ref().unwrap().send(job).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());
        for worker in &mut self.workers {
            if let Some(thread) = worker.0.take() {
                thread.join().unwrap();
            }
        }
    }
}
