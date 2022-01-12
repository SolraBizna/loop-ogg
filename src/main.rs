use std::{
    path::PathBuf,
};

use clap::Parser;

mod decode;
mod playback;
mod resample;
mod terminate;
use terminate::Terminator;

#[derive(Parser, Debug)]
#[clap(author = "Solra Bizna <solra@bizna.name>", version,
       about = "A simple command-line utility for playing back a single Ogg \
		Vorbis file on loop.",
       long_about = "\n\
		     This is a simple command-line utility that does one \
		     thing: play back a single Ogg Vorbis file, over and \
		     over.\n\
		     \n\
		     It supports looping metadata of a few different \
		     standards, so that Vorbis files designed to be looped in \
		     a certain way will sound correct. Even for Vorbis files \
		     that lack such data, looping may be smoother as this \
		     utility has no gaps between loops.\n\
		     \n\
		     When you first interrupt this program with control-C, it \
		     will disengage the loop, bringing the song to its \
		     natural conclusion. If you interrupt it more times, it \
		     will make increasingly desperate attempts to exit \
		     immediately.")]
struct Invocation {
    /// The path to the Ogg Vorbis file to play.
    path: PathBuf,
    /// A volume control that multiplies the amplitude. 1.0 = no change, 2.0 =
    /// double amplitude (+6dB), 0.5 = half amplitude (-6dB).
    #[clap(short, long, default_value_t = 1.0)]
    volume: f32,
    /// Hide the progress bar. (Default if standard error is not a terminal.)
    #[clap(short, long)]
    quiet: bool,
    /// Show the progress bar. (Default if standard error is a terminal.)
    #[clap(short, long)]
    progress: bool,
}

const NUM_PACKETS_BUFFERED: usize = 30; // thirty? dirty

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let invocation = Invocation::parse();
    let progress = match (invocation.quiet, invocation.progress) {
	(false, false) => atty::is(atty::Stream::Stderr),
	(true, false) => false,
	(false, true) => true,
	(true, true) => {
	    eprintln!("Both --quiet and --progress were specified. Pick one \
		       and specify only that one.");
	    std::process::exit(1)
	},
    };
    let terminator = Terminator::new();
    let (sample_rate_in, channel_count, loop_left, loop_right,
	 decoded_stuff_rx)
	= decode::start_decoding(&invocation.path, terminator.clone())?;
    let time_unit = (sample_rate_in as usize)
	.saturating_mul(channel_count as usize);
    let (sample_rate_out, resampled_stuff_tx, is_active)
	= playback::start_playback(sample_rate_in, channel_count,
				   time_unit, loop_left, loop_right,
				   terminator.clone(),
				   invocation.volume,
				   progress)?;
    resample::resample(sample_rate_in, sample_rate_out, channel_count,
		       decoded_stuff_rx, resampled_stuff_tx,
		       terminator)?;
    while is_active() {
	std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}
