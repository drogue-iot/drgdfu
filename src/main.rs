use std::path::PathBuf;

use bluer::{AdapterEvent, Address};
use clap::{Parser, Subcommand};
use core::str::FromStr;
use futures::lock::Mutex;
use futures::{pin_mut, StreamExt};
use serde_json::json;
use std::process::exit;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

mod firmware;
mod gatt;

use firmware::*;

#[derive(Parser, Debug)]
struct Args {
    #[clap(short, long, parse(from_occurrences))]
    verbose: usize,

    #[clap(subcommand)]
    mode: Mode,
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    BleGatt {
        #[clap(long)]
        device: String,

        #[clap(subcommand)]
        source: FirmwareSource,
    },
    Serial {
        #[clap(long)]
        port: PathBuf,

        #[clap(subcommand)]
        source: FirmwareSource,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    stderrlog::new().verbosity(args.verbose).init().unwrap();

    match args.mode {
        Mode::BleGatt { device, source } => {
            let session = bluer::Session::new().await?;
            let adapter = session.default_adapter().await?;
            adapter.set_powered(true).await?;
            let discover = adapter.discover_devices().await?;
            pin_mut!(discover);

            let addr = Address::from_str(&device)?;

            while let Some(evt) = discover.next().await {
                match evt {
                    AdapterEvent::DeviceAdded(a) if a == addr => {
                        let device = adapter.device(a)?;

                        sleep(Duration::from_secs(2)).await;
                        if !device.is_connected().await? {
                            log::debug!("Connecting...");
                            let mut retries = 2;
                            loop {
                                match device.connect().await {
                                    Ok(()) => break,
                                    Err(err) if retries > 0 => {
                                        println!("Connect error: {}", &err);
                                        retries -= 1;
                                    }
                                    Err(err) => return Err(err.into()),
                                }
                            }
                            log::debug!("Connected");
                        } else {
                            log::debug!("Already connected");
                        }
                        let board = gatt::GattBoard::new(device);
                        let version = board.read_firmware_version().await?;
                        println!("Connected to board! Running version {}", version);

                        loop {
                            let operation = source.report(version, None, None).await;
                        }
                    }
                    AdapterEvent::DeviceRemoved(a) if a == addr => {
                        log::info!("Device removed: {}", a);
                    }
                    _ => {}
                }
            }
        }
        Mode::Serial { port, source } => {}
    }
    Ok(())
}
