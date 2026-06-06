use mapvas::{bevy_renderer, profiling};

fn main() {
  let _ = env_logger::try_init();
  profiling::init_profiling();

  bevy_renderer::run();
}
