mod bevy_app;
mod bevy_geometry;
mod bevy_map;
mod bevy_repaint;
mod bevy_screenshot;
mod bevy_tiles;

use mapvas::profiling;

fn main() {
  let _ = env_logger::try_init();
  profiling::init_profiling();

  bevy_app::run();
}
