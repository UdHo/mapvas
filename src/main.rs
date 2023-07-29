mod mapvas;
#[tokio::main]
async fn main() {
    env_logger::init();
    mapvas::MapVas::new().await;
}
