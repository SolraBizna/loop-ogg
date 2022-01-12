use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc::{SyncSender, sync_channel, TryRecvError},
};

use anyhow::anyhow;
use log::{info,warn};
use portaudio::{
    PortAudio,
    stream::{Parameters, OutputSettings, OutputCallbackArgs},
    StreamCallbackResult,
};

use crate::Terminator;

fn print_progress(cur: usize, loop_left: usize, loop_right: &Arc<AtomicUsize>,
		  time_unit: usize, loop_phase: bool, terminator: &Terminator)
{
    let cols = 80;
    // we... don't expect that this display will be... useful... for a multi-
    // hour recording.
    // TODO: replace ANSI sequences
    let mut bar = format!("  {}:{:02}", cur / 60, cur % 60);
    let rem_cols = cols - bar.len() as i32 - 3;
    if rem_cols > 1 {
	bar.reserve(rem_cols as usize * 2 + 50);
	bar.push(' ');
	let loop_right = loop_right.load(Ordering::Relaxed) / time_unit;
	let fill_amt = if cur <= loop_left || loop_right == 0 { 0 }
	else if cur >= loop_right { rem_cols }
	else {
	    ((cur - loop_left) * (rem_cols as usize)
	     / (loop_right - loop_left).max(1)) as i32
	};
	let left_bracket = if cur < loop_left { '<' }
	else { '>' };
	let right_bracket = if !terminator.should_loop() { '>' }
	else if loop_right == 0 { '?' }
	else { '<' };
	bar.push(left_bracket);
	if loop_phase {
	    bar.push_str("\x1b[2m");
	    for _ in 0 .. fill_amt { bar.push('═'); }
	    bar.push_str("\x1b[0;1m");
	    for _ in fill_amt .. rem_cols { bar.push('═'); }
	    bar.push_str("\x1b[0m");
	}
	else {
	    bar.push_str("\x1b[1m");
	    for _ in 0 .. fill_amt { bar.push('═'); }
	    bar.push_str("\x1b[0;2m");
	    for _ in fill_amt .. rem_cols { bar.push('═'); }
	    bar.push_str("\x1b[0m");
	}
	bar.push(right_bracket);
    }
    eprint!("\r\x1B[0K{}\r", bar);
}

fn end_progress() {
    eprint!("\r\x1B[0K");
}

pub fn start_playback(sample_rate: u32, channel_count: u32,
		      time_unit: usize, loop_left: usize,
		      loop_right: Arc<AtomicUsize>,
		      terminator: Terminator,
		      volume: f32, progress: bool) -> anyhow::Result<(u32,SyncSender<(usize, Vec<f32>)>, Box<dyn Fn() -> bool>)> {
    let loop_left = loop_left / time_unit;
    let pa = PortAudio::new().expect("initializing portaudio");
    let output_device = pa.default_output_device().unwrap();
    let parameters = Parameters::new(output_device, channel_count as i32,
				     true, // interleaved
				     1.0);
    let flags = portaudio::stream_flags::Flags::empty();
    let sample_rate = match pa.device_info(output_device)?.default_sample_rate{
	x if x < 1.0 || x >= 1048576.0 => {
	    info!("no default sample rate, using input rate of {}",
		  sample_rate);
	    sample_rate
	},
	x => (x + 0.5).floor() as u32,
    };
    let settings = OutputSettings::with_flags(parameters, sample_rate as f64,
					      0, flags);
    let (tx, rx) = sync_channel::<(usize, Vec<f32>)>(crate::NUM_PACKETS_BUFFERED);
    let mut leftovers: Vec<f32> = Vec::with_capacity(32768); // sure!
    let mut last_pos = None;
    let mut loop_phase = false;
    let callback = move |args: OutputCallbackArgs<f32>| {
	let OutputCallbackArgs {
	    buffer,
	    ..
	} = args;
	let mut rem = &mut buffer[..];
	if terminator.should_terminate() {
	    rem.fill(0.0);
	    while let Ok(_) = rx.try_recv() {}
	    if progress {
		end_progress();
	    }
	    return StreamCallbackResult::Complete
	}
	if leftovers.len() > 0 {
	    if rem.len() >= leftovers.len() {
		(&mut rem[..leftovers.len()]).copy_from_slice(&leftovers);
		rem = &mut rem[leftovers.len()..];
		leftovers.resize(0, 0.0);
	    }
	    else {
		rem.copy_from_slice(&leftovers[..rem.len()]);
		leftovers.copy_within(rem.len().., 0);
		leftovers.resize(leftovers.len()-rem.len(), 0.0);
		rem = &mut[];
	    }
	}
	let mut cur_pos = None;
	while rem.len() > 0 {
	    assert!(leftovers.len() == 0);
	    let (pos, mut next_packet) = match rx.try_recv() {
		Ok(x) => x,
		Err(TryRecvError::Empty) => break,
		Err(TryRecvError::Disconnected) => {
		    rem.fill(0.0);
		    if progress {
			end_progress();
		    }
		    return StreamCallbackResult::Complete
		},
	    };
	    if volume != 1.0 {
		for x in next_packet.iter_mut() {
		    *x *= volume;
		}
	    }
	    if next_packet.len() <= rem.len() {
		(&mut rem[..next_packet.len()]).copy_from_slice(&next_packet);
		rem = &mut rem[next_packet.len()..];
	    }
	    else {
		rem.copy_from_slice(&next_packet[..rem.len()]);
		leftovers.extend_from_slice(&next_packet[rem.len()..]);
		rem = &mut [];
	    }
	    cur_pos = Some(pos);
	}
	if rem.len() > 0 {
	    rem.fill(0.0);
	    warn!("playback buffer underrun!");
	}
	if let Some(cur_pos) = cur_pos {
	    let cur_pos = cur_pos / time_unit;
	    if Some(cur_pos) != last_pos {
		if let Some(last_pos) = last_pos {
		    if cur_pos < last_pos {
			loop_phase = !loop_phase;
		    }
		}
		last_pos = Some(cur_pos);
		if progress {
		    print_progress(cur_pos, loop_left, &loop_right, time_unit,
				   loop_phase, &terminator);
		}
	    }
	}
	StreamCallbackResult::Continue
    };
    let mut stream = pa.open_non_blocking_stream(settings, callback)
	.or_else(|x| Err(anyhow!("Unable to open audio stream: {}", x)))?;
    stream.start()
	.or_else(|x| Err(anyhow!("Unable to start audio stream: {}", x)))?;
    let is_active = move || stream.is_active().ok().unwrap_or(false);
    Ok((sample_rate, tx, Box::new(is_active)))
}
