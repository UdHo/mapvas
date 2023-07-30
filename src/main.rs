#![feature(async_closure)]
#![feature(async_fn_in_trait)]
mod coordinates;
mod mapvas;
mod tile_loader;
#[tokio::main]
async fn main() {
    env_logger::init();
    mapvas::MapVas::new().await;
}
