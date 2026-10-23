use std::sync::{Arc, PoisonError, RwLock};

use anyhow::{Result, anyhow};
use ffmpeg_next::{Packet, codec, frame, software::scaling, util::format::pixel::Pixel};

use crate::compositor::overlay::{OverlayFrame, chroma_alpha};

/// Externally rendered full-frame graphics (for example a VizRS `/output`
/// page captured by a headless browser), blended over every video frame.
///
/// The producer replaces the frame whenever the graphics change; the playout
/// only takes a cheap `Arc` snapshot per frame. The stored frame is cropped to
/// the visible (non-transparent) area so idle or small graphics cost little.
#[derive(Clone, Debug, Default)]
pub struct GraphicsLayer {
    slot: Arc<RwLock<Option<Arc<OverlayFrame>>>>,
}

impl std::fmt::Debug for OverlayFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayFrame")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

impl GraphicsLayer {
    /// Decodes a PNG with alpha, scales it to the output size and makes it the
    /// current graphics. A fully transparent image clears the layer.
    pub fn set_png(&self, png: &[u8], output_width: u32, output_height: u32) -> Result<()> {
        let overlay = decode_png_overlay(png, output_width, output_height)?;
        self.set(overlay);

        Ok(())
    }

    pub fn clear(&self) {
        self.set(None);
    }

    pub(crate) fn current(&self) -> Option<Arc<OverlayFrame>> {
        self.slot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, overlay: Option<OverlayFrame>) {
        *self.slot.write().unwrap_or_else(PoisonError::into_inner) = overlay.map(Arc::new);
    }
}

fn decode_png_overlay(
    png: &[u8],
    output_width: u32,
    output_height: u32,
) -> Result<Option<OverlayFrame>> {
    if output_width < 2 || output_height < 2 {
        return Err(anyhow!("graphics output size must be at least 2x2"));
    }

    let codec = codec::decoder::find(codec::Id::PNG).ok_or_else(|| anyhow!("no PNG decoder"))?;
    let mut decoder = codec::context::Context::new_with_codec(codec)
        .decoder()
        .video()?;
    let mut decoded = frame::Video::empty();

    decoder.send_packet(&Packet::copy(png))?;
    decoder.send_eof()?;
    decoder.receive_frame(&mut decoded)?;

    let mut scaler = scaling::Context::get(
        decoded.format(),
        decoded.width(),
        decoded.height(),
        Pixel::YUVA420P,
        output_width,
        output_height,
        scaling::flag::Flags::BILINEAR,
    )?;
    let mut yuva = frame::Video::empty();
    scaler.run(&decoded, &mut yuva)?;

    let Some(bounds) = visible_bounds(&yuva) else {
        return Ok(None);
    };

    Ok(Some(crop(&yuva, bounds)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bounds {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Smallest even-aligned rectangle containing all pixels with alpha > 0.
fn visible_bounds(frame: &frame::Video) -> Option<Bounds> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let alpha = frame.data(3);
    let stride = frame.stride(3);
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);

    for y in 0..height {
        let row = &alpha[y * stride..y * stride + width];
        let Some(first) = row.iter().position(|a| *a > 0) else {
            continue;
        };
        let last = row.iter().rposition(|a| *a > 0).unwrap_or(first);

        left = left.min(first);
        right = right.max(last + 1);
        top = top.min(y);
        bottom = y + 1;
    }

    if left >= right {
        return None;
    }

    let x = left & !1;
    let y = top & !1;
    let right = (right + 1).min(width) & !1;
    let bottom = (bottom + 1).min(height) & !1;

    (right > x && bottom > y).then_some(Bounds {
        x: x as u32,
        y: y as u32,
        width: (right - x) as u32,
        height: (bottom - y) as u32,
    })
}

fn crop(source: &frame::Video, bounds: Bounds) -> OverlayFrame {
    let mut target = frame::Video::new(Pixel::YUVA420P, bounds.width, bounds.height);

    for (plane, subsampled) in [(0, false), (1, true), (2, true), (3, false)] {
        let shift = usize::from(subsampled);
        let src_x = (bounds.x as usize) >> shift;
        let src_y = (bounds.y as usize) >> shift;
        let row_len = (bounds.width as usize) >> shift;
        let rows = (bounds.height as usize) >> shift;
        let src_stride = source.stride(plane);
        let dst_stride = target.stride(plane);
        let src = source.data(plane);
        let dst = target.data_mut(plane);

        for row in 0..rows {
            let from = (src_y + row) * src_stride + src_x;
            let to = row * dst_stride;
            dst[to..to + row_len].copy_from_slice(&src[from..from + row_len]);
        }
    }

    OverlayFrame {
        chroma_alpha: chroma_alpha(&target, bounds.width, bounds.height),
        frame: target,
        x: bounds.x as i32,
        y: bounds.y as i32,
        width: bounds.width,
        height: bounds.height,
        opacity: 255,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transparent(width: u32, height: u32) -> frame::Video {
        let mut frame = frame::Video::new(Pixel::YUVA420P, width, height);
        let stride = frame.stride(3);
        let alpha = frame.data_mut(3);

        for y in 0..height as usize {
            alpha[y * stride..y * stride + width as usize].fill(0);
        }

        frame
    }

    #[test]
    fn transparent_frame_has_no_bounds() {
        assert_eq!(visible_bounds(&transparent(16, 8)), None);
    }

    #[test]
    fn bounds_are_even_aligned_around_visible_pixels() {
        let mut frame = transparent(16, 8);
        let stride = frame.stride(3);
        frame.data_mut(3)[3 * stride + 5] = 255;
        frame.data_mut(3)[4 * stride + 9] = 10;

        let bounds = visible_bounds(&frame).unwrap();

        assert_eq!(
            bounds,
            Bounds {
                x: 4,
                y: 2,
                width: 6,
                height: 4,
            }
        );

        let overlay = crop(&frame, bounds);

        assert_eq!((overlay.x, overlay.y), (4, 2));
        assert_eq!(overlay.frame.data(3)[overlay.frame.stride(3) + 1], 255);
    }

    #[test]
    fn layer_starts_empty_and_clears() {
        let layer = GraphicsLayer::default();
        assert!(layer.current().is_none());

        layer.set(Some(crop(
            &transparent(4, 4),
            Bounds {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            },
        )));
        assert!(layer.current().is_some());

        layer.clear();
        assert!(layer.current().is_none());
    }

    #[test]
    fn invalid_png_is_an_error() {
        let layer = GraphicsLayer::default();

        assert!(layer.set_png(b"not a png", 64, 36).is_err());
        assert!(layer.current().is_none());
    }
}
