mod decode;
mod playback;
mod resample;
mod terminate;
use terminate::Terminator;

const NUM_PACKETS_BUFFERED: usize = 30; // thirty? dirty

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 && args.len() != 3 {
	eprintln!("Usage: {} path/to/file.ogg [volume]\n\
		   volume is either a floating point amplification factor\n\
		   (e.g. 1.0 for 100% volume, 2.0 for doubled volume, etc.)\n\
		   or an equal sign followed by a command to pipe so-called\n\
		   infinite-WAV data to (e.g. '=play -q -t wav - reverb')",
		  args.get(0).map(|x| x.as_str()).unwrap_or("loop-ogg"));
	std::process::exit(1);
    }
    let terminator = Terminator::new();
    let (sample_rate_in, channel_count, loop_left, loop_right,
	 decoded_stuff_rx)
	= decode::start_decoding(&args[1], terminator.clone())?;
    let time_unit = (sample_rate_in as usize)
	.saturating_mul(channel_count as usize);
    let (sample_rate_out, resampled_stuff_tx, is_active)
	= playback::start_playback(sample_rate_in, channel_count,
				   time_unit, loop_left, loop_right,
				   terminator.clone(),
				   args.get(2).map(|x| x.as_str())
				   .unwrap_or("1.0"))?;
    resample::resample(sample_rate_in, sample_rate_out, channel_count,
		       decoded_stuff_rx, resampled_stuff_tx,
		       terminator)?;
    while is_active() {
	std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}
