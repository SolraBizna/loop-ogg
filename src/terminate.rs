use std::{
    sync::{
	Arc,
	atomic::{AtomicU32, Ordering},
    }
};

#[derive(Debug,Clone)]
pub struct Terminator {
    ctrlc_count: Arc<AtomicU32>,
}

impl Terminator {
    pub fn new() -> Terminator {
	let ctrlc_count = Arc::new(AtomicU32::new(0));
	let ctrlc_count_clone = ctrlc_count.clone();
	ctrlc::set_handler(move || {
	    let n = ctrlc_count_clone.load(Ordering::Relaxed);
	    let n = n + 1;
	    if n >= 5 {
		// TODO: replace with crossterm
		eprintln!("\r\x1b[0K\rSUDOKU!");
		std::process::exit(1)
	    };
	    ctrlc_count_clone.store(n, Ordering::Relaxed);
	}).expect("unable to set control-C handler");
	Terminator { ctrlc_count }
    }
    fn fetch(&self) -> u32 {
	self.ctrlc_count.load(Ordering::Relaxed)
    }
    pub fn should_loop(&self) -> bool {
	self.fetch() == 0
    }
    pub fn should_terminate(&self) -> bool {
	self.fetch() > 1
    }
}
