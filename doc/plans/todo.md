# TODO


## Fixes

* In loop mode the elapsed time counter doesn't reset when the sample loops, leading to displays like: '0:52/0:09'


## Logging & Error Handling

* Add logging with user-configurable verbosity: this crate LGTM but feel free to suggest alternatives: https://crates.io/crates/tracing-logfmt
* I'd like to improve riffgrep's error reporting 
```
#[derive(Error, Debug)]
pub enum IndexingError {
    #[error("Collision detected for ID {0} between {1} and {2}")]
    IdCollision(u64, String, String),
}
```

## UI

* Playback Scroll
  - Add toggle action to enable waveform display to scroll with the playhead
  - When waveform zoom is at unity this should have no effect
  - When zoom magnification is greater than 1 the waveform should scroll smoothly from right to left, with the playhead appearing stationary
  - Playhead position on the screen should be conserved: i.e. regardless of where the playhead is when Scroll is enabled that's where it should stay
* User Themes:
  - Add ability to customize themes with a theme.toml spec
  - Spec should include everything in the Theme struct
  - Spec should include column order (currently in config.toml)
  - Spec should include column widths
  - Refactor theme.rs so it contains only a single default theme as a fallback
* (Low Priority) Support for multiple terminal types: ANSI, xterm-256color 
* (Low Priority) Add fun TUI bling like https://www.nerdfonts.com/

## Actions & Workflows

* Use toggle actions rather than e.g. o/O
* (Low Priority) Presets
  - Saveable and loadable at startup. Define as TOML object.
  - Should incorporate everything configurable via command line arguments at startup
  - include: starting directory, plugin, audio interface, clock master/follower, session BPM, initial query, initial batch actions, color theme, keymappings
* (Low Priority) Workflow DSL


## Search

* Repurpose the TXXX/Reserved field in our schema for embedding vectors, pursuant to a similarity search feature (see EMBEDDING.md)

## Audio

* Audio Device
  - Allow user to select an audio output device, 2 channels on the device, and a sample rate
  - Expose CLI flag to print available interfaces, channels, and sample rates
  - Display current output device and sample rate at the bottom right of the screen, right-justified (i.e. to the right of all other info)
  - Expose an action that records the riffgrep playhead output to disk. The file location should be user-definable, and should default to the current working directory if undefined. File format is stereo, session sample rate, and user-definable bit depth (16, 24, 32, 32-float) defaulting to 32 or 32-float. File naming convention is 'riffgrep 0001 [2026-02-08 203709]'.
* (Low Priority) Session Clock 
  - This is an expansion of the Session BPM feature
  - Ability to act as transport/clock leader or follower
  - Can receive bpm via MIDI clock, MMC, other session transport protocols (e.g. Ableton Link, etc)
  - Display current clock status & session BPM at the bottom of the screen, right-justified, after the current sample info.
* (Low Priority) Marker Rendering
  - Add an action to render the current bank as a new sample. 
  - The new sample should be named as the original, with an addended '-n' at the end, where 'n' is the number of renders that have been done (i.e. if the original sample is named 'foo-2.wav' the new sample should be named 'foo-3.wav' not 'foo-2-1.wav')
  - The new sample should be written to the same directory as the original
  - The new sample should have the marker data currently in memory _for the bank which is not dictating rendering_. Marker data for the rendering bank should be set to the default.
  - The new sample should have the metadata currently in memory (which for now should be identical to the original since we aren't enabling metadata writes yet)
  - If in SQLITE mode should we update the database? I think it would be better not to (i.e. to force the user to do that via the 'index' command), but am open to correction on this.
* (Low Priority) Plugins
  - Ability to load an AU/VST/VST3/CLAP plugin at startup by providing a path.
  - Expose CLI flag to list available plugins

## Open Sourcing

* (Low Priority) Test code on a Linux box and make any necessary adjustments
* (Low Priority) Review code for features that are idiosyncratic to me and consider refactoring options
* (Low Priority) Update README with screenshots
* (Low Priority) Publish as Rust crate?
