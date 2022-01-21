use std::sync::mpsc::{SyncSender, Receiver};

use libsoxr::Soxr;

use crate::Terminator;

pub fn resample(sample_rate_in: u32, sample_rate_out: u32, channel_count: u32,
		in_rx: Receiver<(usize, Vec<f32>)>,
		out_tx: SyncSender<(usize, Vec<f32>)>,
		terminator: Terminator)
		-> anyhow::Result<()> {
    if sample_rate_in == sample_rate_out {
	// Easy!
	while let Ok((pos, x)) = in_rx.recv() {
	    if terminator.should_terminate() { break }
	    out_tx.send((pos, x))?;
	}
    }
    else {
	let soxr = Soxr::create(sample_rate_in as f64, sample_rate_out as f64,
				channel_count, None, None, None)?;
	let mut last_pos = 0;
	while let Ok((pos, in_buf)) = in_rx.recv() {
	    if terminator.should_terminate() { break }
            assert!(in_buf.len() > 0);
	    let capacity = in_buf.len()
		.checked_mul(sample_rate_out as usize)
		.and_then(|x| x.checked_add(sample_rate_out as usize - 1))
		.expect("arithmetic overflow caught, buffer overrun averted")
		/ (sample_rate_in as usize);
	    let mut out_buf = vec![0.0f32; capacity];
	    let (processed_in, processed_out)
		= soxr.process(Some(&in_buf), &mut out_buf[..])?;
	    assert_eq!(processed_in, in_buf.len() / channel_count as usize);
	    let processed_out_floats
		= processed_out.checked_mul(channel_count as usize)
		.expect("arithmetic overflow caught, buffer overrun averted");
	    assert!(out_buf.len() >= processed_out_floats);
	    out_buf.resize(processed_out_floats, 0.0);
	    out_tx.send((pos, out_buf))?;
	    last_pos = pos;
	}
	let mut out_buf = vec![0.0f32; 1024];
	let (_processed_in, processed_out)
	    = soxr.process::<f32,_>(None, &mut out_buf)?;
	let processed_out_floats
	    = processed_out.checked_mul(channel_count as usize)
	    .expect("arithmetic overflow caught, buffer overrun averted");
	assert!(out_buf.len() >= processed_out_floats);
	out_buf.resize(processed_out.checked_mul(channel_count as usize)
		       .expect("arithmetic overflow caught, buffer \
				overrun averted"),
		       0.0);
	out_tx.send((last_pos, out_buf))?;
    }
    Ok(())
}
