use std::sync::LazyLock;

use fontdue::Font;
use tiny_skia::{Color, Pixmap};

// Embedded font - using a basic sans-serif font
static FONT_DATA: &[u8] = include_bytes!("../../../assets/fonts/Roboto-Regular.ttf");

// Lazy-loaded font instance
pub static FONT: LazyLock<Font> = LazyLock::new(|| {
  Font::from_bytes(FONT_DATA, fontdue::FontSettings::default())
    .expect("Failed to load embedded font")
});

/// Renders text onto a pixmap at the given position.
/// Returns the width of the rendered text for positioning purposes.
#[allow(
  clippy::cast_precision_loss,
  clippy::cast_possible_truncation,
  clippy::cast_possible_wrap,
  clippy::cast_sign_loss
)]
pub fn render_text(
  pixmap: &mut Pixmap,
  text: &str,
  x: f32,
  y: f32,
  font_size: f32,
  color: Color,
) -> f32 {
  let font = &*FONT;

  // Get metrics for a typical capital letter to establish baseline
  // We'll use this to calculate a consistent baseline for all characters
  let (ref_metrics, _) = font.rasterize('A', font_size);
  let baseline_offset = ref_metrics.height as f32 + ref_metrics.ymin as f32;

  let mut total_width = 0.0_f32;
  let mut cursor_x = x;

  for ch in text.chars() {
    let (metrics, bitmap) = font.rasterize(ch, font_size);

    if bitmap.is_empty() {
      // Space or non-printable character
      cursor_x += metrics.advance_width;
      total_width += metrics.advance_width;
      continue;
    }

    // Calculate position with consistent baseline
    // All characters align to the same baseline derived from 'A'
    let glyph_x = (cursor_x + metrics.xmin as f32).round() as i32;
    let glyph_y =
      (y + baseline_offset - metrics.height as f32 - metrics.ymin as f32).round() as i32;

    // Draw glyph pixels
    for (i, &alpha) in bitmap.iter().enumerate() {
      if alpha == 0 {
        continue;
      }

      let px = glyph_x + (i % metrics.width) as i32;
      let py = glyph_y + (i / metrics.width) as i32;

      // Bounds check
      if px < 0 || py < 0 || px >= pixmap.width() as i32 || py >= pixmap.height() as i32 {
        continue;
      }

      // Blend the glyph with the existing pixel
      let pixel_idx = (py as u32 * pixmap.width() + px as u32) as usize * 4;
      if let Some(pixel_slice) = pixmap.data_mut().get_mut(pixel_idx..pixel_idx + 4) {
        let alpha_f = f32::from(alpha) / 255.0;
        let src_r = (color.red() * 255.0) as u8;
        let src_g = (color.green() * 255.0) as u8;
        let src_b = (color.blue() * 255.0) as u8;

        // Alpha blend
        pixel_slice[0] =
          ((1.0 - alpha_f) * f32::from(pixel_slice[0]) + alpha_f * f32::from(src_r)) as u8;
        pixel_slice[1] =
          ((1.0 - alpha_f) * f32::from(pixel_slice[1]) + alpha_f * f32::from(src_g)) as u8;
        pixel_slice[2] =
          ((1.0 - alpha_f) * f32::from(pixel_slice[2]) + alpha_f * f32::from(src_b)) as u8;
        pixel_slice[3] = 255; // Fully opaque
      }
    }

    cursor_x += metrics.advance_width;
    total_width += metrics.advance_width;
  }

  total_width
}

/// Calculate the total length of a path
pub fn calculate_path_length(coords: &[(f32, f32)]) -> f32 {
  coords
    .windows(2)
    .map(|w| {
      let dx = w[1].0 - w[0].0;
      let dy = w[1].1 - w[0].1;
      (dx * dx + dy * dy).sqrt()
    })
    .sum()
}

/// Find a point along the path at a given distance from the start
/// Returns (x, y, angle) where angle is the direction of the path at that point
pub fn point_along_path(coords: &[(f32, f32)], distance: f32) -> Option<(f32, f32, f32)> {
  if coords.len() < 2 {
    return None;
  }

  let mut remaining = distance;

  for window in coords.windows(2) {
    let (x1, y1) = window[0];
    let (x2, y2) = window[1];

    let dx = x2 - x1;
    let dy = y2 - y1;
    let segment_length = (dx * dx + dy * dy).sqrt();

    if remaining <= segment_length {
      // Point is on this segment
      let t = if segment_length > 0.0 {
        remaining / segment_length
      } else {
        0.0
      };
      let x = x1 + t * dx;
      let y = y1 + t * dy;
      let angle = dy.atan2(dx);
      return Some((x, y, angle));
    }

    remaining -= segment_length;
  }

  // Past the end - return the last point with the angle of the last segment
  if coords.len() >= 2 {
    let (x1, y1) = coords[coords.len() - 2];
    let (x2, y2) = coords[coords.len() - 1];
    let angle = (y2 - y1).atan2(x2 - x1);
    Some((x2, y2, angle))
  } else {
    None
  }
}

/// Render a single glyph bitmap rotated by the given angle around (cx, cy)
#[allow(
  clippy::cast_precision_loss,
  clippy::cast_possible_truncation,
  clippy::cast_possible_wrap,
  clippy::cast_sign_loss,
  clippy::too_many_arguments
)]
pub fn render_rotated_glyph(
  pixmap: &mut Pixmap,
  bitmap: &[u8],
  glyph_width: usize,
  glyph_height: usize,
  cx: f32,
  cy: f32,
  angle: f32,
  color: Color,
) {
  let cos_a = angle.cos();
  let sin_a = angle.sin();

  // Glyph center offset
  let half_w = glyph_width as f32 / 2.0;
  let half_h = glyph_height as f32 / 2.0;

  for gy in 0..glyph_height {
    for gx in 0..glyph_width {
      let alpha = bitmap[gy * glyph_width + gx];
      if alpha == 0 {
        continue;
      }

      // Position relative to glyph center
      let rel_x = gx as f32 - half_w;
      let rel_y = gy as f32 - half_h;

      // Rotate around center
      let rot_x = rel_x * cos_a - rel_y * sin_a;
      let rot_y = rel_x * sin_a + rel_y * cos_a;

      // Final position
      let px = (cx + rot_x).round() as i32;
      let py = (cy + rot_y).round() as i32;

      // Bounds check
      if px < 0 || py < 0 || px >= pixmap.width() as i32 || py >= pixmap.height() as i32 {
        continue;
      }

      // Blend the pixel
      let pixel_idx = (py as u32 * pixmap.width() + px as u32) as usize * 4;
      if let Some(pixel_slice) = pixmap.data_mut().get_mut(pixel_idx..pixel_idx + 4) {
        let alpha_f = f32::from(alpha) / 255.0;
        let src_r = (color.red() * 255.0) as u8;
        let src_g = (color.green() * 255.0) as u8;
        let src_b = (color.blue() * 255.0) as u8;

        // Alpha blend
        pixel_slice[0] =
          ((1.0 - alpha_f) * f32::from(pixel_slice[0]) + alpha_f * f32::from(src_r)) as u8;
        pixel_slice[1] =
          ((1.0 - alpha_f) * f32::from(pixel_slice[1]) + alpha_f * f32::from(src_g)) as u8;
        pixel_slice[2] =
          ((1.0 - alpha_f) * f32::from(pixel_slice[2]) + alpha_f * f32::from(src_b)) as u8;
        pixel_slice[3] = 255;
      }
    }
  }
}

/// Render text along a path, with each character rotated to follow the path direction
pub fn render_text_along_path(
  pixmap: &mut Pixmap,
  text: &str,
  path: &[(f32, f32)],
  start_offset: f32,
  font_size: f32,
  text_color: Color,
) {
  let font = &*FONT;

  let mut current_offset = start_offset.max(0.0);

  for ch in text.chars() {
    let (metrics, bitmap) = font.rasterize(ch, font_size);

    if bitmap.is_empty() {
      // Space or non-printable - just advance
      current_offset += metrics.advance_width;
      continue;
    }

    // Find position and angle at current offset
    let Some((x, y, angle)) = point_along_path(path, current_offset + metrics.advance_width / 2.0)
    else {
      break;
    };

    // Render the glyph rotated around its center
    render_rotated_glyph(
      pixmap,
      &bitmap,
      metrics.width,
      metrics.height,
      x,
      y,
      angle,
      text_color,
    );

    current_offset += metrics.advance_width;
  }
}
