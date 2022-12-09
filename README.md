This is a simple **command-line utility** that does one thing: play back a single Ogg Vorbis file, over and over.

It supports [looping metadata of a few different standards](#what), so that Vorbis files designed to be looped in a certain way will sound correct. Even for Vorbis files that lack such data, looping may be smoother as this utility has no gaps between loops.

When you first interrupt this program with control-C, it will disengage the loop, bringing the song to its natural conclusion. If you interrupt it more times, it will make increasingly desperate attempts to exit immediately.

Until then, it will play back your audio file, and (optionally) display a timeline showing the loop status, current time, and where the loop points are.

# How

This utility is written in Rust. It uses Lewton for Ogg Vorbis decoding, PortAudio for output, libsoxr for resampling, and clap for command line parsing. It should run on any operating system that both Rust and PortAudio support. Its CPU usage is ridiculously low once it ramps up, though its memory usage will slightly exceed the uncompressed size of the audio being looped.

## Compiling

(Note: Normally, it's unreasonable to expect that all users of your software will be able to compile it. However, if you're not comfortable enough with the command line to follow the below directions, you're probably not comfortable enough with the command line to *use* this utility...)

### Step 1: Get a Rust build environment

Installing a Rust build environment is pretty easy. [Instructions are available here](https://www.rust-lang.org/learn/get-started), automatically tailored to your current operating system.

### Step 2: Get the source code

Using the command line version of Git (on Windows, this might be called "Git Bash"):

```sh
git clone https://github.com/SolraBizna/loop-ogg
```

If you're using some graphical frontend for Git, use it to clone the `https://github.com/SolraBizna/loop-ogg` repository.

### Step 3: Build

```sh
cd loop-ogg
cargo build --release
```

This builds the utility in release mode, with all relevant optimizations enabled and no debug symbols. `loop-ogg` itself is written in safe Rust, and so it should be stable enough not to need debugging.

### Step 4: Install (optional)

While you could run the utility with `cargo run` every time, you're probably better off putting the built executable somewhere reasonable:

```sh
cp target/release/loop-ogg ~/bin
```

## Running

If you run `loop-ogg` without any arguments, it will print a very short usage string. `--help` will print a longer one explaining the possible options. Most of the time, you'll just do `loop-ogg path/to/SomeVorbisFile.ogg`, maybe with `-v 0.5` or something to make it quieter. There's... not a whole lot of variation available. What can I say? It's a utility that plays an Ogg Vorbis file on loop.

# What

This program supports two different standards for specifying loop metadata as Vorbis comments. As the Vorbis standard dictates, these comments are case insensitive. `LOOP_START` and `loop_start` and `Loop_Start` all mean the same thing.

If neither `LOOP_START` nor `LOOPSTART` are present, the loop will begin at the beginning. If neither `LOOP_END` nor `LOOPLENGTH` are present, the loop will end at the end. <!-- 終わりは始まり、始まりは終わり -->

## Seconds

This format gives seconds as a decimal number, e.g. `4.56`. This format will be used if present. It is preferred because the same loop metadata will remain valid even if the audio is resampled.

- `LOOP_START`: The first instant "in" the loop.
- `LOOP_END`: The first instant "not in" the loop.

If a seconds count is given as an integer, and it's greater than or equal to the sample rate, `loop-ogg` will assume that a mistake has been made and interpret it as a sample count instead. Don't rely on this. (If you really want to rely on this, and the warning annoys you, try `RUST_LOG= loop-ogg path/to/my_ogg.ogg` to suppress it.

## Samples

This format gives sample counts as a decimal integer, e.g. `456456`. This format will only be used if the corresponding seconds-based metadata is not present.

- `LOOPSTART`: The first sample "in" the loop.
- `LOOPLENGTH`: How many samples are "in" the loop.

## Loop Mix

As an additional feature, if a `LOOP_MIX` comment is present, the audio data after the loop will be mixed into the audio at the start of the loop in every loop after the first one. (I've seen this feature used exactly once.)

# Why

This is one of those utilities that I end up writing every few years, as an excuse to sharpen my skills—and also, one of those utilities that I use daily, much to the confusion of others.

# Legalese

Like many Rust components, this utility is dual-licensed under the [MIT license](http://opensource.org/licenses/MIT), and [Version 2.0 of the Apache License](http://www.apache.org/licenses/LICENSE-2.0). This utility and its source code are copyright © 2022 Solra Bizna. Dependencies are under their own respective licenses and copyrights.



