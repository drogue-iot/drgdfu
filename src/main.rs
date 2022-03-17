use std::path::PathBuf;

use bluer::{AdapterEvent, Address};
use clap::{Parser, Subcommand};
use core::str::FromStr;
use futures::{pin_mut, StreamExt};
use std::time::Duration;
use tokio::time::sleep;

mod firmware;
mod gatt;

use firmware::*;

#[derive(Parser, Debug)]
struct Args {
    /// Adjust the output verbosity.
    #[clap(short, long, parse(from_occurrences))]
    verbose: usize,

    /// The DFU mode to use for updating firmware.
    #[clap(subcommand)]
    mode: Mode,
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// GATT mode for DFU using BLE GATT
    BleGatt {
        /// The MAC address of the device to update.
        #[clap(long)]
        device: String,

        /// The source to use for firmware.
        #[clap(subcommand)]
        source: FirmwareSource,
    },
    /// Serial mode for DFU using serial protocol
    Serial {
        /// The serial port to use
        #[clap(long)]
        port: PathBuf,

        /// The source to use for firmware.
        #[clap(subcommand)]
        source: FirmwareSource,
    },
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FirmwareSource {
    /// File based firmware source for updating from a file
    File { file: PathBuf },
    /// Cloud based firmware source for updating from Drogue IoT
    Cloud {
        /// Url to the HTTP endpoint of Drogue IoT Cloud
        url: String,

        /// The application to use.
        application: String,

        /// The device name to use.
        device: String,

        /// Password to use for device.
        password: String,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    stderrlog::new().verbosity(args.verbose).init().unwrap();

    match args.mode {
        Mode::BleGatt { device, mut source } => {
            let session = bluer::Session::new().await?;
            let adapter = session.default_adapter().await?;
            adapter.set_powered(true).await?;
            let discover = adapter.discover_devices().await?;
            pin_mut!(discover);

            let addr = Address::from_str(&device)?;
            let mut updated = false;

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
                        let current_version = board.read_firmware_version().await?;
                        println!("Connected to board! Running version {}", current_version);

                        let mut source: FirmwareProducer = FirmwareProducer::new(&source)?;
                        let mut report = Report::first(&current_version);
                        loop {
                            let cmd = source.report(&report).await?;
                            match cmd {
                                Command::Write {
                                    version,
                                    offset,
                                    data,
                                } => {
                                    if offset == 0 {
                                        board.start_firmware_update().await?;
                                    }
                                    board.write_firmware(offset, &data).await?;
                                    report = Report::status(
                                        &current_version,
                                        offset + data.len() as u32,
                                        &version,
                                    );
                                }
                                Command::Sync { version, poll } => {
                                    log::info!("Firmware in sync");
                                    if updated {
                                        log::info!("Marking new firmware as booted");
                                        board.mark_booted().await?;
                                    }
                                    return Ok(());
                                }
                                Command::Swap { version, checksum } => {
                                    log::info!("Swap operation");
                                    board.swap_firmware().await?;
                                    updated = true;
                                    adapter.remove_device(board.address()).await?;
                                    break;
                                }
                            }
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
