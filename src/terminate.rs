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
    // TODO: replace hardcoded ANSI sequences with appropriate crate
    pub fn new() -> Terminator {
	let ctrlc_count = Arc::new(AtomicU32::new(0));
	let ctrlc_count_clone = ctrlc_count.clone();
	ctrlc::set_handler(move || {
	    let n = ctrlc_count_clone.load(Ordering::Relaxed);
	    let n = match n {
		0 => {
		    eprintln!("\r\x1b[0KCeasing loop...");
		    1
		},
		1 => {
		    eprintln!("\r\x1b[0KStopping!");
		    2
		},
		2 => 3,
		3 => 4,
		_ => {
		    eprintln!("\r\x1b[0KSUDOKU!");
		    std::process::exit(1)
		},
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
