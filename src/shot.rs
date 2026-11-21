//! Headless screenshots (`--features screenshot`): renders the real window through Slint's
//! software renderer into a PNG, with no display, GPU or audio device involved.

use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize, PlatformError, Rgb8Pixel};

use crate::{MainWindow, demo};

struct ShotPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for ShotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
}

pub fn run(out: &Path, scene: &str, theme: &str, width: u32, height: u32, phone: bool) -> Result<(), String> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(ShotPlatform { window: window.clone() })).map_err(|e| e.to_string())?;
    let ui = MainWindow::new().map_err(|e| e.to_string())?;
    window.set_size(PhysicalSize::new(width, height));
    demo::populate(&ui, scene, theme, phone, width);
    ui.show().map_err(|e| e.to_string())?;

    // Let animations (cross-fades, scrolls) finish on the wall clock.
    let mut buffer = vec![Rgb8Pixel::default(); (width * height) as usize];
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1400) {
        slint::platform::update_timers_and_animations();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut buffer, width as usize);
        });
        std::thread::sleep(Duration::from_millis(16));
    }
    slint::platform::update_timers_and_animations();
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, width as usize);
    });

    let file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    let raw: Vec<u8> = buffer.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    w.write_image_data(&raw).map_err(|e| e.to_string())?;
    Ok(())
}
