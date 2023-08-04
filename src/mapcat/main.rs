use atty::Stream;
use futures::TryFutureExt;
use log::{error, trace};
use tokio::io;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

fn main() {
  env_logger::init();

  if atty::is(Stream::Stdin) {
    error!("Only stdin supported at the moment.");
    return;
  }
  let mut reader = BufReader::new(io::stdin());
  let mut buffer = Vec::new();

  let fut = reader
    .read_until(b'\n', &mut buffer)
    .and_then(|(stdin, buffer)| trace!("read {}", buffer));

  tokio::task::spawn_blocking(fut);
}
