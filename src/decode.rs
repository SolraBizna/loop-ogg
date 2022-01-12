use std::{
    collections::VecDeque,
    sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
	mpsc::{Receiver, sync_channel, TrySendError},
    },
    fs::File,
    path::Path,
};

use anyhow::anyhow;
use lewton::inside_ogg::OggStreamReader;
use log::trace;

use crate::Terminator;

const DESIRED_CROSSLAP_AMOUNT: usize = 32;

fn crosslap_onto(o: &mut[f32], i: &[f32], channel_count: u32) {
    let lap_len = o.len() / channel_count as usize;
    for (n, (o, i)) in o.chunks_mut(channel_count as usize)
	.zip(i.chunks(channel_count as usize)).enumerate() {
	    let o_scale = (n as f32 + 0.5) / (lap_len as f32);
	    let i_scale = 1.0 - o_scale;
	    for channel in 0 .. o.len() {
		o[channel] = o[channel] * o_scale + i[channel] * i_scale;
	    }
    }
}

fn mix_onto(o: &mut[f32], i: &[f32]) {
    assert_eq!(o.len(), i.len());
    for (o, i) in o.iter_mut().zip(i.iter()) {
	*o += *i;
    }
}

pub fn start_decoding(path: &Path, terminator: Terminator)
		      -> anyhow::Result<(u32, u32, usize, Arc<AtomicUsize>, Receiver<(usize,Vec<f32>)>)> {
    let file = File::open(path)?;
    let mut osr = OggStreamReader::new(file)?;
    let channel_count = match osr.ident_hdr.audio_channels {
	1 | 2 => osr.ident_hdr.audio_channels as u32,
	x => return Err(anyhow!("unhandled channel count: {}", x)),
    };
    let sample_rate = match osr.ident_hdr.audio_sample_rate {
	0 => return Err(anyhow!("stream says it's 0Hz, that unpossible")),
	x => x,
    };
    trace!("Vendor: {}", osr.comment_hdr.vendor);
    let mut loop_start = None;
    let mut loop_end = None;
    let mut loopstart = None;
    let mut looplength = None;
    let mut loop_mix = None;
    for (key, value) in osr.comment_hdr.comment_list.iter() {
	let key = key.to_lowercase();
	trace!("{}={}", key, value);
	match key.as_str() {
	    "loop_start" => loop_start = Some(value),
	    "loop_end" => loop_end = Some(value),
	    "loopstart" => loopstart = Some(value),
	    "looplength" => looplength = Some(value),
	    "loop_mix" => loop_mix = Some(value),
	    _ => (),
	}
    }
    let loop_mix = loop_mix.is_some();
    let loop_left = if let Some(x) = loop_start {
	let result = (x.parse::<f64>()? * sample_rate as f64).ceil() as usize;
	trace!("LOOP_START={} → {}", x, result);
	result
    }
    else if let Some(x) = loopstart {
	let result = x.parse::<usize>()?;
	trace!("LOOPSTART={} → {}", x, result);
	result
    }
    else { 0 };
    let loop_right = if let Some(x) = loop_end {
	let result = (x.parse::<f64>()? * sample_rate as f64).ceil() as usize;
	trace!("LOOP_END={} → {}", x, result);
	result
    }
    else if let Some(x) = looplength {
	let result = loop_left.saturating_add(x.parse::<usize>()?);
	trace!("LOOPLENGTH={} → {}", x, result);
	result
    }
    else { usize::MAX };
    let loop_left_i: usize = loop_left.saturating_mul(channel_count as usize);
    let loop_right_i: usize =loop_right.saturating_mul(channel_count as usize);
    let loop_right_atom = Arc::new(AtomicUsize::new(
	if loop_right_i == usize::MAX { 0 }
	else { loop_right_i }
    ));
    let loop_right_atom_clone = loop_right_atom.clone();
    let (decode_tx, decode_rx) = sync_channel(crate::NUM_PACKETS_BUFFERED);
    let _ = std::thread::Builder::new().name("decode thread".to_string())
	.spawn(move || {
	    loop {
		let pkt
		    = match osr.read_dec_packet_generic::<Vec<Vec<f32>>>()
		    .expect("error while decoding stream") {
			Some(x) if x.len() == 0 || x[0].len() == 0 => {
			    continue
			},
			Some(x) => x,
			None => break,
		    };
		let buf_to_send = match channel_count {
		    1 => {
			assert_eq!(pkt.len(), 1);
			pkt.into_iter().next().unwrap()
		    },
		    2 => {
			assert_eq!(pkt.len(), 2);
			assert_eq!(pkt[0].len(), pkt[1].len());
			let mut out_buf
			    = Vec::with_capacity(pkt[0].len()*2);
			for (&l, &r) in pkt[0].iter().zip(pkt[1].iter()) {
			    out_buf.push(l);
			    out_buf.push(r);
			}
			out_buf
		    },
		    _ => unreachable!(),
		};
		if let Err(_) = decode_tx.send(buf_to_send) { break }
	    }
	    trace!("Decoding completed");
	})?;
    let (loop_tx, loop_rx) = sync_channel(crate::NUM_PACKETS_BUFFERED);
    let _ = std::thread::Builder::new().name("loop thread".to_string())
	.spawn(move || {
	    assert!(loop_right_i > loop_left_i); // not >=!
	    let mut floats_left_till_start = loop_left_i;
	    let mut floats_left_till_end = loop_right_i - loop_left_i;
	    let mut loop_buf = if loop_right_i == usize::MAX { Vec::new() }
	    else { Vec::with_capacity(floats_left_till_end) };
	    // the position of the next DECODED BUFFER we receive
	    let mut pos = 0;
	    while floats_left_till_start > 0 {
		let mut floats = match decode_rx.recv() {
		    Ok(x) => x,
		    Err(_) => return,
		};
		if floats.len() <= floats_left_till_start {
		    let floats_len = floats.len();
		    floats_left_till_start -= floats_len;
		    if let Err(_) = loop_tx.send((pos, floats)) { return }
		    pos += floats_len;
		}
		else {
		    loop_buf
			.extend_from_slice(&floats[floats_left_till_start..]);
		    let floats_len = floats.len();
		    floats.resize(floats_left_till_start, 0.0);
		    if let Err(_) = loop_tx.send((pos, floats)) { return }
		    pos += floats_len;
		    break
		}
	    }
	    // once we've hit the left loop point, we want to race ahead
	    // and find the right loop point as soon as possible. so, we
	    // start buffering our sends.
	    let mut buffered_sends = VecDeque::new();
	    let mut rest = if loop_buf.len() > floats_left_till_end {
		let rest: Vec<f32> = loop_buf[floats_left_till_end..]
		    .iter().map(|x| *x).collect();
		loop_buf.resize(floats_left_till_end, 0.0);
		if let Err(_) = loop_tx.send((loop_left_i, loop_buf.clone())) {
		    return
		}
		rest
	    }
	    else {
		if let Err(_) = loop_tx.send((loop_left_i, loop_buf.clone())) {
		    return
		}
		floats_left_till_end -= loop_buf.len();
		loop {
		    if floats_left_till_end == 0 { break vec![] }
		    let mut floats = match decode_rx.recv() {
			Ok(x) => x,
			Err(_) => break vec![],
		    };
		    if floats.len() <= floats_left_till_end {
			floats_left_till_end -= floats.len();
			loop_buf.extend_from_slice(&floats[..]);
			let floats_len = floats.len();
			buffered_sends.push_back((pos, floats));
			pos += floats_len;
		    }
		    else {
			loop_buf.extend_from_slice
			    (&floats[..floats_left_till_end]);
			let rest: Vec<f32> = floats[floats_left_till_end..]
			    .iter().map(|x| *x).collect();
			let floats_len = floats.len();
			floats.resize(floats_left_till_end, 0.0);
			buffered_sends.push_back((pos, floats));
			pos += floats_len;
			break rest;
		    }
		    while let Some(buffered_send) = buffered_sends.pop_front(){
			match loop_tx.try_send(buffered_send) {
			    Ok(_) => (),
			    Err(TrySendError::Full(buffered_send)) => { 
				buffered_sends.push_front(buffered_send);
				break;
			    },
			    Err(_) => return,
			}
		    }
		}
	    };
	    // we now know for sure the length of the loop!
	    loop_right_atom.store(loop_left_i + loop_buf.len(),
				  Ordering::Relaxed);
	    // drain our buffered sends before we do any more work
	    for buffered_send in buffered_sends.into_iter() {
		if let Err(_) = loop_tx.send(buffered_send) { return }
	    }
	    if loop_mix {
		// obscure feature, never before supported by any other imp-
		// lementation of this "standard"!
		//
		// if `LOOP_MIX` is requested, then all audio after the loop
		// gets back-mixed into the loop
		let mut new_floats = rest.clone();
		let mut old_floats = &mut loop_buf[..];
		let mut pos = loop_left_i;
		'outer: loop {
		    // we've hit the loop point (or just barely started—cont-
		    // inue only if looping is desired
		    if !terminator.should_loop() { break }
		    old_floats = &mut loop_buf[..];
		    pos = loop_left_i;
		    while old_floats.len() > 0 {
			if old_floats.len() < new_floats.len() {
			    mix_onto(old_floats, &new_floats[..old_floats.len()]);
			    let blah = old_floats.to_owned();
			    if let Err(_) = loop_tx.send((pos, blah)) { return }
			    pos += old_floats.len();
			    new_floats.copy_within(old_floats.len().., 0);
			    new_floats.resize(new_floats.len()-old_floats.len(), 0.0);
			    old_floats = &mut[];
			}
			else {
			    mix_onto(&mut old_floats[..new_floats.len()], &new_floats);
			    let blah = old_floats[..new_floats.len()].to_owned();
			    if let Err(_) = loop_tx.send((pos, blah)) { return }
			    pos += new_floats.len();
			    old_floats = &mut old_floats[new_floats.len()..];
			    match decode_rx.recv() {
				Ok(x) => {
				    new_floats = x;
				    rest.extend_from_slice(&new_floats);
				},
				Err(_) => break 'outer,
			    };
			}
		    }
		}
		if old_floats.len() > 0 {
		    for chunk in old_floats.chunks(4096) {
			if let Err(_) = loop_tx.send((pos, chunk.to_owned())) { return }
			pos += chunk.len();
		    }
		}
	    }
	    else {
		// without `LOOP_MIX`, cross-lap up to a few dozen samples
		// around the loop point to remove the "pop"
		let crosslap_amount = (DESIRED_CROSSLAP_AMOUNT
				       * channel_count as usize)
		    .min(loop_buf.len());
		while rest.len() < crosslap_amount {
		    if let Ok(x) = decode_rx.recv() {
			rest.extend_from_slice(&x);
		    } else { break }
		}
		if rest.len() >= crosslap_amount {
		    crosslap_onto(&mut loop_buf[..crosslap_amount],
				  &rest[..crosslap_amount],
				  channel_count);
		}
	    }
	    while terminator.should_loop() {
		// four thousand ninety six? okay
		let mut pos = loop_left_i;
		for chunk in loop_buf.chunks(4096) {
		    if let Err(_) = loop_tx.send((pos, chunk.iter().map(|x| *x)
						  .collect())) { return }
		    pos += chunk.len();
		}
	    }
	    if let Err(_) = loop_tx.send((loop_right_i, rest)) { return }
	    while let Ok(x) = decode_rx.recv() {
		let x_len = x.len();
		if let Err(_) = loop_tx.send((pos, x)) { return }
		pos += x_len;
	    }
	})?;
    Ok((sample_rate, channel_count, loop_left_i, loop_right_atom_clone,
	loop_rx))
}
