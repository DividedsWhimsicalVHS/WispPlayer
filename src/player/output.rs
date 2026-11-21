//! Audio output. A tiny backend trait lets the engine run against a fake device in tests; the
//! only production implementation is cpal (WASAPI on Windows).

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, StreamConfig};

use super::pipe::{Renderer, Shared};

/// The device format the engine must produce.
#[derive(Clone, Debug)]
pub struct Format {
    pub rate: u32,
    pub channels: usize,
    pub name: String,
}

pub trait Backend {
    /// Opens the device (`None` = system default), builds the real-time renderer for its format
    /// through `make`, and starts streaming. On success returns the device format.
    fn open(&mut self, id: Option<&str>, make: &mut dyn FnMut(&Format) -> Renderer) -> Result<Format, String>;
    fn close(&mut self);
}

pub struct Cpal {
    stream: Option<cpal::Stream>,
    shared: Arc<Shared>,
}

impl Cpal {
    pub fn new(shared: Arc<Shared>) -> Cpal {
        Cpal { stream: None, shared }
    }
}

/// Frames converted per pass for devices whose native format is not f32.
const BLOCK_FRAMES: usize = 1024;

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut renderer: Renderer,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = config.channels as usize;
    // Allocated here, once: the callback itself never allocates.
    let mut scratch = vec![0.0f32; BLOCK_FRAMES * ch];
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], _| {
            for block in data.chunks_mut(BLOCK_FRAMES * ch) {
                let s = &mut scratch[..block.len()];
                renderer.render(s);
                for (o, i) in block.iter_mut().zip(s.iter()) {
                    *o = T::from_sample(*i);
                }
            }
        },
        on_error,
        None,
    )
}

impl Backend for Cpal {
    fn open(&mut self, id: Option<&str>, make: &mut dyn FnMut(&Format) -> Renderer) -> Result<Format, String> {
        self.close();
        let host = cpal::default_host();
        let wanted = id
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse::<cpal::DeviceId>().ok())
            .and_then(|d| host.device_by_id(&d));
        let device = wanted
            .or_else(|| host.default_output_device())
            .ok_or_else(|| "no audio output device found".to_string())?;
        let name = device.description().map_or_else(|_| "Audio device".to_string(), |d| d.name().to_string());
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = Format { rate: supported.sample_rate(), channels: supported.channels() as usize, name };
        let renderer = make(&format);
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();

        let shared = self.shared.clone();
        let on_error = move |e: cpal::Error| {
            // Under-runs are survivable; anything else (unplugged, route change) needs a reopen.
            if e.kind() != ErrorKind::Xrun {
                shared.device_lost.store(true, Relaxed);
            }
        };
        let stream = match sample_format {
            SampleFormat::F32 => build::<f32>(&device, config, renderer, on_error),
            SampleFormat::I16 => build::<i16>(&device, config, renderer, on_error),
            SampleFormat::I32 => build::<i32>(&device, config, renderer, on_error),
            SampleFormat::U16 => build::<u16>(&device, config, renderer, on_error),
            other => return Err(format!("unsupported device sample format {other}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(format)
    }

    fn close(&mut self) {
        self.stream = None;
    }
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}

/// Output devices for the settings page. Call from a throw-away thread: it initialises COM.
pub fn list_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let Ok(devices) = host.output_devices() else { return Vec::new() };
    devices
        .filter_map(|d| {
            Some(DeviceInfo { id: d.id().ok()?.to_string(), name: d.description().ok()?.name().to_string() })
        })
        .collect()
}
