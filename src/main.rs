#![feature(type_alias_impl_trait)]
use clap::{Parser, Subcommand};
use core::future::Future;
use embedded_io::adapters::FromTokio;
use embedded_update::{
    device::{Serial, Simulator},
    service::InMemory,
    DeviceStatus, FirmwareDevice, FirmwareUpdater, UpdaterConfig,
};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use drgdfu::*;

#[derive(Parser, Debug)]
struct Args {
    /// Adjust the output verbosity.
    #[clap(short, long, parse(from_occurrences))]
    verbose: usize,

    /// The tool mode
    #[clap(subcommand)]
    mode: Mode,
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    /// Generate firmware metadata
    Generate {
        /// Version of firmware
        #[clap(long)]
        version: String,

        /// Firmware to generate metadata for
        #[clap(long)]
        file: PathBuf,
    },
    /// Upload a new firmware to device
    Upload {
        /// The transport mode to use for updating firmware.
        #[clap(subcommand)]
        transport: Transport,
    },
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Transport {
    /// GATT mode for DFU using BLE GATT
    #[cfg(feature = "ble")]
    BleGatt {
        /// Enable device discovery
        #[clap(long)]
        enable_discovery: bool,

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
    /// Fake transport simulating a device. Convenient for testing the protocol
    Simulated {
        /// The initial version to use for the firmware
        #[clap(long)]
        version: String,

        /// The source to use for firmware.
        #[clap(subcommand)]
        source: FirmwareSource,
    },
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FirmwareSource {
    /// File based firmware source for updating from a file
    File {
        #[clap(long)]
        firmware: PathBuf,

        #[clap(long)]
        metadata: PathBuf,
    },
    /// Cloud based firmware source for updating from Drogue IoT
    Cloud {
        /// Url to the HTTP endpoint of Drogue IoT Cloud
        #[clap(long)]
        http: String,

        /// The application to use.
        #[clap(long)]
        application: String,

        /// The device name to use.
        #[clap(long)]
        device: String,

        /// Password to use for device.
        #[clap(long)]
        password: String,
    },
}

impl FirmwareSource {
    async fn run<F: FirmwareDevice>(&mut self, mut d: F) -> Result<(), anyhow::Error> {
        match self {
            FirmwareSource::File { firmware, metadata } => {
                let metadata = FirmwareFileMeta::from_file(&metadata)?;
                let mut file = File::open(&firmware)?;
                let mut data = Vec::new();
                file.read_to_end(&mut data)?;
                let service = InMemory::new(metadata.version.as_bytes(), &data[..]);

                let mut updater = FirmwareUpdater::new(service, Default::default());
                loop {
                    if let Ok(DeviceStatus::Synced(_)) = updater.run(&mut d, &mut Timer).await {
                        break;
                    }
                }
            }
            FirmwareSource::Cloud {
                http,
                application,
                device,
                password,
            } => {
                let user = format!("{}@{}", device, application);
                let timeout = std::time::Duration::from_secs(30);
                let service = DrogueFirmwareService::new(http, &user, password, timeout);

                let mut updater = FirmwareUpdater::new(
                    service,
                    UpdaterConfig {
                        timeout_ms: 30_000,
                        backoff_ms: 5_000,
                    },
                );
                loop {
                    if let Ok(DeviceStatus::Synced(_)) = updater.run(&mut d, &mut Timer).await {
                        break;
                    }
                }
            }
        }

        println!("Firmware updated");
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    stderrlog::new().verbosity(args.verbose).init().unwrap();

    match args.mode {
        Mode::Generate { version, file } => {
            // Generate metadata
            let firmware = FirmwareFileMeta::new(&version, &file)?;
            println!("{}", serde_json::to_string(&firmware)?);
        }
        Mode::Upload { transport } => match transport {
            #[cfg(feature = "ble")]
            Transport::BleGatt {
                enable_discovery,
                device,
                mut source,
            } => {
                use btleplug::api::{Central, Manager as _, ScanFilter};
                use btleplug::platform::Manager;
                let manager = Manager::new().await?;
                let central = manager
                    .adapters()
                    .await?
                    .into_iter()
                    .nth(0)
                    .ok_or(anyhow::anyhow!("no adapter found"))?;

                if enable_discovery {
                    central.start_scan(ScanFilter::default()).await?;
                }

                let s = GattBoard::new(&device, central);
                source.run(s).await?;
            }
            Transport::Serial { port, mut source } => {
                let p: String = port.to_str().unwrap().to_string();
                let builder = tokio_serial::new(p, 115200);
                let s = Serial::new(FromTokio::new(tokio_serial::SerialStream::open(&builder)?));
                source.run(s).await?;
            }
            Transport::Simulated {
                version,
                mut source,
            } => {
                let s = Simulator::new(version.as_bytes());
                source.run(s).await?;
            }
        },
    }
    Ok(())
}

pub struct Timer;

impl embedded_hal_async::delay::DelayUs for Timer {
    type Error = core::convert::Infallible;
    type DelayUsFuture<'m> = impl Future<Output = Result<(), Self::Error>> + 'm where Self: 'm;
    fn delay_us(&mut self, i: u32) -> Self::DelayUsFuture<'_> {
        async move {
            tokio::time::sleep(tokio::time::Duration::from_micros(i as u64)).await;
            Ok(())
        }
    }

    type DelayMsFuture<'m> = impl Future<Output = Result<(), Self::Error>> + 'm where Self: 'm;
    fn delay_ms(&mut self, i: u32) -> Self::DelayMsFuture<'_> {
        async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(i as u64)).await;
            Ok(())
        }
    }
}
