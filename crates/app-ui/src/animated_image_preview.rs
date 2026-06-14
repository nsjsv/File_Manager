use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::image as iced_image;
use image::codecs::gif::GifDecoder;
use image::AnimationDecoder;

const MIN_ANIMATED_IMAGE_FRAME_DELAY: Duration = Duration::from_millis(10);

#[derive(Debug, Clone)]
pub(crate) struct AnimatedImagePreview {
    path: PathBuf,
    frames: Vec<AnimatedImageFrame>,
    current_frame: usize,
    width: u32,
    height: u32,
}

impl AnimatedImagePreview {
    pub(crate) fn new(
        path: PathBuf,
        frames: Vec<AnimatedImageFrame>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        if frames.is_empty() {
            return Err("Animated image preview has no frames".to_owned());
        }
        if width == 0 || height == 0 {
            return Err("Animated image preview has invalid dimensions".to_owned());
        }

        Ok(Self {
            path,
            frames,
            current_frame: 0,
            width,
            height,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn current_frame_handle(&self) -> &iced_image::Handle {
        &self.frames[self.current_frame].handle
    }

    pub(crate) fn current_frame_delay(&self) -> Option<Duration> {
        (self.frames.len() > 1).then_some(self.frames[self.current_frame].delay)
    }

    pub(crate) fn advance_frame(&mut self) {
        if self.frames.len() <= 1 {
            return;
        }
        self.current_frame = (self.current_frame + 1) % self.frames.len();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AnimatedImageFrame {
    handle: iced_image::Handle,
    delay: Duration,
}

impl AnimatedImageFrame {
    pub(crate) fn new(handle: iced_image::Handle, delay: Duration) -> Self {
        Self { handle, delay }
    }
}

pub(crate) fn is_animated_image_preview_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"))
}

pub(crate) async fn load_animated_image_preview(
    path: PathBuf,
) -> Result<AnimatedImagePreview, String> {
    tokio::task::spawn_blocking(move || load_animated_image_preview_blocking(path))
        .await
        .map_err(|error| format!("could not read animated image preview: {error}"))?
}

fn load_animated_image_preview_blocking(path: PathBuf) -> Result<AnimatedImagePreview, String> {
    let file = File::open(&path)
        .map_err(|error| format!("could not open animated image preview: {error}"))?;
    let decoder = GifDecoder::new(BufReader::new(file))
        .map_err(|error| format!("could not decode animated image preview: {error}"))?;
    let decoded_frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|error| format!("could not decode animated image frames: {error}"))?;

    let mut frames = Vec::with_capacity(decoded_frames.len());
    let mut dimensions = None;
    for decoded_frame in decoded_frames {
        let delay = normalized_frame_delay(decoded_frame.delay());
        let rgba_buffer = decoded_frame.into_buffer();
        let width = rgba_buffer.width();
        let height = rgba_buffer.height();
        dimensions.get_or_insert((width, height));
        frames.push(AnimatedImageFrame::new(
            iced_image::Handle::from_rgba(width, height, rgba_buffer.into_raw()),
            delay,
        ));
    }

    let (width, height) = dimensions.unwrap_or((0, 0));
    AnimatedImagePreview::new(path, frames, width, height)
}

fn normalized_frame_delay(delay: image::Delay) -> Duration {
    let (numerator, denominator) = delay.numer_denom_ms();
    let nanos = u128::from(numerator) * 1_000_000 / u128::from(denominator);
    let duration = Duration::from_nanos(nanos as u64);
    duration.max(MIN_ANIMATED_IMAGE_FRAME_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_frame_delay_is_clamped() {
        let delay = image::Delay::from_numer_denom_ms(0, 1);

        assert_eq!(
            normalized_frame_delay(delay),
            MIN_ANIMATED_IMAGE_FRAME_DELAY
        );
    }

    #[test]
    fn animated_image_preview_advances_and_wraps() {
        let first = AnimatedImageFrame::new(
            iced_image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
            Duration::from_millis(20),
        );
        let second = AnimatedImageFrame::new(
            iced_image::Handle::from_rgba(1, 1, vec![255, 0, 0, 255]),
            Duration::from_millis(30),
        );
        let mut preview =
            AnimatedImagePreview::new(PathBuf::from("/tmp/sample.gif"), vec![first, second], 1, 1)
                .expect("animated preview");

        assert_eq!(preview.current_frame, 0);
        assert_eq!(
            preview.current_frame_delay(),
            Some(Duration::from_millis(20))
        );

        preview.advance_frame();
        assert_eq!(preview.current_frame, 1);
        assert_eq!(
            preview.current_frame_delay(),
            Some(Duration::from_millis(30))
        );

        preview.advance_frame();
        assert_eq!(preview.current_frame, 0);
    }
}
