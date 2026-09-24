//! Watch discovery for a while and report whether updates keep flowing.
//!
//! The failure this guards against is silent: the socket stays up, requests
//! still work, and room updates simply stop. So the useful signal is a
//! per-room update count that keeps climbing.
//!
//!     cargo run --example watch -- [seconds]

use dutchdutch_ascend::{Discovery, DiscoveryEvent};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();

    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(70);

    let mut discovery = Discovery::new();
    let mut events = discovery.subscribe();
    discovery.start().await?;

    let start = Instant::now();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut last_report = Instant::now();

    while start.elapsed() < Duration::from_secs(secs) {
        match tokio::time::timeout(Duration::from_secs(1), events.recv()).await {
            Ok(Ok(DiscoveryEvent::RoomRemoved(id))) => {
                println!("  [{:>5.1}s] REMOVED {id}", start.elapsed().as_secs_f32());
            }
            Ok(Ok(DiscoveryEvent::RoomAdded(id))) => {
                println!("  [{:>5.1}s] ADDED   {id}", start.elapsed().as_secs_f32());
                *counts.entry(id.to_string()).or_default() += 1;
            }
            Ok(Ok(DiscoveryEvent::RoomUpdated(id))) => {
                *counts.entry(id.to_string()).or_default() += 1;
            }
            _ => {}
        }

        if last_report.elapsed() >= Duration::from_secs(10) {
            last_report = Instant::now();
            println!("t={:>3}s  updates so far: {:?}", start.elapsed().as_secs(), counts);
        }
    }

    println!("\nfinal after {secs}s: {counts:?}");
    discovery.stop().await;
    Ok(())
}
