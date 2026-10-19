//! Audio output that follows the system default device.
//!
//! librespot's rodio sink binds to whichever device is the default at startup.
//! When that device goes away (headphones unplugged, Bluetooth disconnecting,
//! a Realtek jack switching endpoints) the stream dies silently: the player
//! keeps reporting "playing" while its write loop waits forever for a buffer
//! that never drains. This sink reopens the stream on the current default
//! device whenever the stream errors, stalls, or the default device changes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};

/// About half a second of audio, same as librespot's rodio sink.
const MAX_QUEUED_CHUNKS: usize = 26;
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(1);
/// A chunk drains every ~20ms; no progress for this long means the stream is dead
/// even if the backend never reported an error.
const STALL_TIMEOUT: Duration = Duration::from_secs(2);

struct Output {
    device_name: Option<String>,
    sink: rodio::Sink,
    failed: Arc<AtomicBool>,
    _stream: rodio::OutputStream,
}

impl Output {
    fn open(host: &cpal::Host) -> Result<Self, String> {
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let device_name = device.name().ok();

        // Same config choice as librespot: stereo 44.1 kHz, else the device's
        // default rate, else whatever the device defaults to.
        let default_config = device.default_output_config().map_err(|e| e.to_string())?;
        let config = device
            .supported_output_configs()
            .map_err(|e| e.to_string())?
            .find(|c| c.channels() == cpal::ChannelCount::from(NUM_CHANNELS))
            .and_then(|c| {
                c.try_with_sample_rate(cpal::SampleRate(SAMPLE_RATE))
                    .or_else(|| c.try_with_sample_rate(default_config.sample_rate()))
            })
            .unwrap_or(default_config);

        let failed = Arc::new(AtomicBool::new(false));
        let on_error = {
            let failed = Arc::clone(&failed);
            move |err: cpal::StreamError| {
                tracing::warn!("Audio stream error: {err}");
                failed.store(true, Ordering::Relaxed);
            }
        };
        // Tries this exact config first, then the device's other supported configs.
        let mut stream = rodio::OutputStreamBuilder::default()
            .with_device(device)
            .with_config(&config.config())
            .with_sample_format(cpal::SampleFormat::I16)
            .with_error_callback(on_error)
            .open_stream_or_fallback()
            .map_err(|e| e.to_string())?;
        stream.log_on_drop(false);

        tracing::info!(
            "Using audio device: {}",
            device_name.as_deref().unwrap_or("[unknown name]")
        );
        Ok(Self {
            device_name,
            sink: rodio::Sink::connect_new(stream.mixer()),
            failed,
            _stream: stream,
        })
    }

    /// Waits until at most `max_queued` chunks are queued.
    /// Returns `false` if the stream failed or stopped draining meanwhile.
    fn wait_for_drain(&self, max_queued: usize) -> bool {
        let mut last_len = self.sink.len();
        let mut last_progress = Instant::now();
        while self.sink.len() > max_queued {
            if self.failed.load(Ordering::Relaxed) {
                return false;
            }
            let len = self.sink.len();
            if len < last_len {
                last_len = len;
                last_progress = Instant::now();
            } else if last_progress.elapsed() > STALL_TIMEOUT {
                tracing::warn!("Audio stream stopped draining");
                self.failed.store(true, Ordering::Relaxed);
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
        true
    }
}

pub struct DefaultDeviceSink {
    host: cpal::Host,
    output: Option<Output>,
    playing: bool,
    last_device_check: Instant,
}

impl DefaultDeviceSink {
    pub fn new() -> Self {
        let host = cpal::default_host();
        tracing::info!("Using audio host: {}", host.id().name());
        let output = Output::open(&host)
            .inspect_err(|e| tracing::error!("Failed to open audio output: {e}"))
            .ok();
        Self {
            host,
            output,
            playing: false,
            last_device_check: Instant::now(),
        }
    }

    /// Returns a working output, reopening it if the stream failed or the
    /// default device changed since it was opened.
    fn ensure_output(&mut self, force_device_check: bool) -> SinkResult<&Output> {
        let needs_reopen = match &self.output {
            None => true,
            Some(out) if out.failed.load(Ordering::Relaxed) => true,
            Some(out) => {
                if force_device_check || self.last_device_check.elapsed() >= DEVICE_CHECK_INTERVAL {
                    self.last_device_check = Instant::now();
                    let current = self
                        .host
                        .default_output_device()
                        .and_then(|d| d.name().ok());
                    current.is_some() && current != out.device_name
                } else {
                    false
                }
            }
        };
        if needs_reopen {
            self.reopen()?;
        }
        Ok(self.output.as_ref().expect("output is open after reopen"))
    }

    fn reopen(&mut self) -> SinkResult<()> {
        // Release the old stream before opening a new one on the same device.
        self.output = None;
        self.last_device_check = Instant::now();
        let out = Output::open(&self.host).map_err(SinkError::ConnectionRefused)?;
        if self.playing {
            out.sink.play();
        } else {
            out.sink.pause();
        }
        self.output = Some(out);
        Ok(())
    }
}

impl Sink for DefaultDeviceSink {
    fn start(&mut self) -> SinkResult<()> {
        self.playing = true;
        self.ensure_output(true)?.sink.play();
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        // librespot exits the process when `stop` fails, so never return an error.
        self.playing = false;
        if let Some(out) = &self.output {
            out.wait_for_drain(0);
            out.sink.pause();
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet
            .samples()
            .map_err(|e| SinkError::OnWrite(e.to_string()))?;
        let samples_f32: &[f32] = &converter.f64_to_f32(samples);
        let source = rodio::buffer::SamplesBuffer::new(
            cpal::ChannelCount::from(NUM_CHANNELS),
            SAMPLE_RATE,
            samples_f32,
        );

        let out = self.ensure_output(false)?;
        out.sink.append(source);
        if !out.wait_for_drain(MAX_QUEUED_CHUNKS) {
            // The queued half second is lost; playback resumes on the new device.
            self.reopen()?;
        }
        Ok(())
    }
}
