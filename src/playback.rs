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
		  time_unit: usize, terminator: &Terminator, unicode: bool)
{
    struct Theme {
	line: char, open: char, closed_left: char, closed_right: char,
	time_left: char, time_right: char, 
    }
    let theme = if unicode {
	Theme { line: '─', open: '⋯', closed_left: '╟', closed_right: '╢',
		time_left: '┤', time_right: '├' }
    }
    else {
	Theme { line: '-', open: '+', closed_left: '[', closed_right: ']',
		time_left: '<', time_right: '>' }
    };
    let cols = terminal_size::terminal_size().map(|(w,_)| w.0).unwrap_or(80)
	as usize;
    let loop_right = loop_right.load(Ordering::Relaxed) / time_unit;
    // we... don't expect that this display will be... useful... for a multi-
    // hour recording.
    let left_pos = format!("  {}:{:02} ", loop_left / 60, loop_left % 60);
    let right_pos = if loop_right == 0 { " ?:??".to_owned() }
    else { format!(" {}:{:02}", loop_right / 60, loop_right % 60) };
    let cur_pos = format!("{}:{:02}", cur / 60, cur % 60);
    let mut bar = left_pos;
    bar.reserve(cols*2); //heh
    let rem_cols = cols - bar.len() - right_pos.len() - cur_pos.len() - 4;
    if rem_cols > 1 {
	let fill_amt = if cur <= loop_left || loop_right == 0 { 0 }
	else if cur >= loop_right { rem_cols }
	else {
	    ((cur - loop_left) * (rem_cols as usize) * 2 + 1)
		/ ((loop_right - loop_left).max(1) * 2)
	};
	let left_bracket = if cur < loop_left { theme.open }
	else { theme.closed_left };
	let right_bracket = if !terminator.should_loop() { theme.open }
	else if loop_right == 0 { '?' }
	else { theme.closed_right };
	bar.push(left_bracket);
	for _ in 0 .. fill_amt { bar.push(theme.line); }
	bar.push(theme.time_left);
	bar.push_str(&cur_pos);
	bar.push(theme.time_right);
	for _ in fill_amt .. rem_cols { bar.push(theme.line); }
	bar.push(right_bracket);
	bar.push_str(&right_pos);
	eprint!("\r{}\r", bar);
    } else { } // cowardly don't display progress if there's no room
}

fn end_progress() {
    if cfg!(target_os = "windows") {
	// on Windows, we don't bother to try.
	eprintln!("");
    }
    else {
	// on ANSI terminals this will erase the whole progress bar. on
	// incompatible terminals, it will... not do much, but we will at least
	// erase the garbage that just got outputted, probably.
	eprint!("\r\x1B[0K\r    \r");
    }
}

pub fn start_playback(sample_rate: u32, channel_count: u32,
		      time_unit: usize, loop_left: usize,
		      loop_right: Arc<AtomicUsize>,
		      terminator: Terminator,
		      volume: f32, progress: bool) -> anyhow::Result<(u32,SyncSender<(usize, Vec<f32>)>, Box<dyn Fn() -> bool>)> {
    let unicode = crate::am_unicode::am_unicode();
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
		last_pos = Some(cur_pos);
		if progress {
		    print_progress(cur_pos, loop_left, &loop_right, time_unit,
				   &terminator, unicode);
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
