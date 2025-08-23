use std::{
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(msg: impl Into<String>) -> Spinner {
        let msg = msg.into();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = thread::spawn(move || {
            let frames = ["-", "\\", "|", "/"];
            let mut i = 0usize;
            let mut out = io::stderr(); // write to stderr
            while !stop2.load(Ordering::Relaxed) {
                let _ = write!(out, "\r{} {}", frames[i % frames.len()], msg);
                let _ = out.flush();
                i = (i + 1) % frames.len();
                thread::sleep(Duration::from_millis(80));
            }
            // Clear the line
            let _ = write!(out, "\r\x1b[2K");
            let _ = out.flush();
        });
        Spinner {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
