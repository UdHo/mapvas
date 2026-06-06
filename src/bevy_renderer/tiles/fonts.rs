use std::path::Path;

use bevy::prelude::*;

const EMBEDDED_MAP_LABEL_FONT_DATA: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
const PREFERRED_MAP_LABEL_FONT_PATHS: &[&str] = &[
  "/System/Library/Fonts/Supplemental/NotoSans-Regular.ttf",
  "/Library/Fonts/NotoSans-Regular.ttf",
  "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
  "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
  "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
  "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
  "/System/Library/Fonts/SFNS.ttf",
  "C:\\Windows\\Fonts\\arial.ttf",
  "C:\\Windows\\Fonts\\segoeui.ttf",
];

#[derive(Resource)]
pub(super) struct BevyTileLabelFonts {
  pub(super) map_label: Handle<Font>,
}

pub(super) fn load_bevy_tile_label_fonts(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
  commands.insert_resource(BevyTileLabelFonts {
    map_label: load_preferred_map_label_font(&mut fonts),
  });
}

fn load_preferred_map_label_font(fonts: &mut Assets<Font>) -> Handle<Font> {
  for path in PREFERRED_MAP_LABEL_FONT_PATHS {
    if let Some(font) = load_font_from_path(path) {
      return fonts.add(font);
    }
  }

  fonts.add(
    Font::try_from_bytes(EMBEDDED_MAP_LABEL_FONT_DATA.to_vec())
      .expect("embedded Roboto map label font must be valid"),
  )
}

fn load_font_from_path(path: &str) -> Option<Font> {
  if !Path::new(path).exists() {
    return None;
  }
  std::fs::read(path)
    .ok()
    .and_then(|bytes| Font::try_from_bytes(bytes).ok())
}
