use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use serde::{Deserialize, Serialize};
use tauri::async_runtime::JoinHandle;
#[cfg(feature = "real-audio")]
use tracing::warn;
use tracing::{debug, info};

const DEFAULT_SAMPLE_RATE: u32 = 16_000;
const DEFAULT_FRAME_LEN: usize = 320;
const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioPipelineConfig {
    pub device_id: Option<String>,
}

impl Default for AudioPipelineConfig {
    fn default() -> Self {
        Self { device_id: None }
    }
}

#[derive(Debug)]
pub enum AudioEvent {
    Frame(Vec<f32>),
    Stopped,
}

pub struct AudioPipeline {
    #[cfg(feature = "real-audio")]
    _real_audio: Option<RealAudioHandle>,
    _worker: JoinHandle<()>,
    receiver: Receiver<AudioEvent>,
    device_id: Option<String>,
    sample_rate: u32,
    synthetic: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

impl AudioPipeline {
    pub fn spawn(config: AudioPipelineConfig) -> Self {
        let (tx, rx) = bounded(16);
        let (out_tx, out_rx) = bounded(64);
        let config = Arc::new(config);
        #[cfg(feature = "real-audio")]
        let (real_audio, sample_rate) =
            match RealAudioHandle::spawn(Arc::clone(&config), tx.clone()) {
                Ok(handle) => {
                    let rate = handle.sample_rate();
                    info!("real audio capture started (sample_rate={rate}Hz)");
                    (Some(handle), rate)
                }
                Err(error) => {
                    warn!("real audio capture failed, falling back to synthetic: {error:?}");
                    (None, DEFAULT_SAMPLE_RATE)
                }
            };

        #[cfg(not(feature = "real-audio"))]
        let real_audio: Option<RealAudioHandle> = None;
        #[cfg(not(feature = "real-audio"))]
        let sample_rate: u32 = DEFAULT_SAMPLE_RATE;

        let use_synthetic = real_audio.is_none();
        let worker = tauri::async_runtime::spawn(async move {
            info!("audio pipeline worker started (synthetic={use_synthetic})");
            let mut phase = 0.0f32;
            let mut frame = Vec::with_capacity(DEFAULT_FRAME_LEN);
            let mut tick = tokio::time::interval(DEFAULT_FRAME_INTERVAL);

            loop {
                if let Ok(event) = rx.try_recv() {
                    let _ = out_tx.send(event);
                }

                if use_synthetic {
                    tick.tick().await;
                    frame.clear();
                    for _ in 0..DEFAULT_FRAME_LEN {
                        let sample = (phase * 2.0 * std::f32::consts::PI).sin() * 0.03;
                        frame.push(sample);
                        phase = (phase + 0.01) % 1.0;
                    }
                    if out_tx.try_send(AudioEvent::Frame(frame.clone())).is_err() {
                        debug!("audio frame dropped (backpressure)");
                    }
                } else {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        });

        Self {
            #[cfg(feature = "real-audio")]
            _real_audio: real_audio,
            _worker: worker,
            receiver: out_rx,
            device_id: config.device_id.clone(),
            sample_rate,
            synthetic: use_synthetic,
        }
    }

    pub fn subscribe(&self) -> Receiver<AudioEvent> {
        self.receiver.clone()
    }

    pub fn device_id(&self) -> Option<String> {
        self.device_id.clone()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn is_synthetic(&self) -> bool {
        self.synthetic
    }
}

pub fn list_input_devices() -> Vec<AudioDeviceInfo> {
    #[cfg(feature = "real-audio")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};

        let host = get_preferred_host();
        let default_name = host
            .default_input_device()
            .and_then(|device| device.name().ok());

        host.input_devices()
            .map(|devices| {
                devices
                    .filter_map(|device| {
                        let name = device.name().ok()?;
                        let is_default = default_name
                            .as_ref()
                            .map(|default| default == &name)
                            .unwrap_or(false);
                        Some(AudioDeviceInfo {
                            id: name.clone(),
                            name,
                            is_default,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(not(feature = "real-audio"))]
    {
        Vec::new()
    }
}

/// Get the preferred audio host, avoiding JACK on Linux to reduce startup noise
#[cfg(feature = "real-audio")]
fn get_preferred_host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    {
        // Try ALSA first to avoid JACK connection errors
        if let Ok(host) = cpal::host_from_id(cpal::HostId::Alsa) {
            return host;
        }
    }
    cpal::default_host()
}

#[cfg(feature = "real-audio")]
struct RealAudioHandle {
    stop: Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
    sample_rate: u32,
}

#[cfg(feature = "real-audio")]
impl RealAudioHandle {
    fn spawn(config: Arc<AudioPipelineConfig>, sender: Sender<AudioEvent>) -> anyhow::Result<Self> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let (stop_tx, stop_rx) = bounded::<()>(1);
        let (ready_tx, ready_rx) = bounded::<Result<u32, anyhow::Error>>(1);

        let thread = std::thread::spawn(move || {
            let startup = || -> anyhow::Result<()> {
                let host = get_preferred_host();
                let device = if let Some(device_id) = &config.device_id {
                    host.input_devices()
                        .ok()
                        .and_then(|devices| {
                            devices
                                .into_iter()
                                .find(|d| d.name().ok().as_ref() == Some(device_id))
                        })
                        .or_else(|| host.default_input_device())
                } else {
                    host.default_input_device()
                }
                .ok_or_else(|| anyhow::anyhow!("no input device available"))?;

                let desired_sample_rate = DEFAULT_SAMPLE_RATE;
                let stream_config = device
                    .supported_input_configs()
                    .ok()
                    .and_then(|mut configs| {
                        configs.find(|cfg| {
                            cfg.sample_format() == cpal::SampleFormat::F32
                                && cfg.min_sample_rate().0 <= desired_sample_rate
                                && cfg.max_sample_rate().0 >= desired_sample_rate
                        })
                    })
                    .map(|cfg| {
                        cfg.with_sample_rate(cpal::SampleRate(desired_sample_rate))
                            .config()
                    })
                    .or_else(|| device.default_input_config().ok().map(|cfg| cfg.config()))
                    .unwrap_or(cpal::StreamConfig {
                        channels: 1,
                        sample_rate: cpal::SampleRate(desired_sample_rate),
                        buffer_size: cpal::BufferSize::Default,
                    });

                let channels = stream_config.channels as usize;
                let frame_samples = ((stream_config.sample_rate.0 as usize) * 20) / 1000;
                let mut buffer = Vec::with_capacity(frame_samples);
                let sender_clone = sender.clone();

                let stream = device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        for frame in data.chunks(channels) {
                            let sample = frame.get(0).copied().unwrap_or(0.0);
                            buffer.push(sample);
                            if buffer.len() >= frame_samples {
                                let mut out = Vec::with_capacity(frame_samples);
                                out.extend_from_slice(&buffer[..frame_samples]);
                                buffer.drain(..frame_samples);
                                if sender_clone.try_send(AudioEvent::Frame(out)).is_err() {
                                    buffer.clear();
                                }
                            }
                        }
                    },
                    |err| warn!("audio input error: {err}"),
                    None,
                )?;

                stream.play()?;
                let _ = ready_tx.send(Ok(stream_config.sample_rate.0));

                while stop_rx.recv_timeout(Duration::from_millis(200)).is_err() {}

                let _ = sender.try_send(AudioEvent::Stopped);
                drop(stream);
                Ok(())
            };

            if let Err(error) = startup() {
                let _ = ready_tx.send(Err(error));
            }
        });

        match ready_rx.recv() {
            Ok(Ok(sample_rate)) => Ok(Self {
                stop: stop_tx,
                thread: Some(thread),
                sample_rate,
            }),
            Ok(Err(error)) => {
                let _ = stop_tx.send(());
                let _ = thread.join();
                Err(error)
            }
            Err(err) => {
                let _ = stop_tx.send(());
                let _ = thread.join();
                Err(anyhow::anyhow!("audio thread initialization failed: {err}"))
            }
        }
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

#[cfg(feature = "real-audio")]
impl Drop for RealAudioHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                warn!("audio capture thread exited with panic");
            }
        }
    }
}
