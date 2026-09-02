use std::time::Instant;
use weatherglass::radar::RadarClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    let frames = RadarClient::new()?
        .animation(45.8202, -88.0659, 6, 0, 4)
        .await?;
    let bytes: usize = frames
        .iter()
        .flat_map(|frame| &frame.tiles)
        .map(|tile| tile.base_png.len() + tile.radar_png.len())
        .sum();
    anyhow::ensure!(frames.len() == 4 && frames.iter().all(|frame| frame.tiles.len() == 1));
    println!(
        "PASS live radar: {} frames, {} KiB in {:.2?}",
        frames.len(),
        bytes / 1024,
        started.elapsed()
    );
    Ok(())
}
