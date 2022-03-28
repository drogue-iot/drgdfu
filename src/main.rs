use clap::{Parser, Subcommand};
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
    #[cfg(feature = "bluez")]
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
    /// Fake transport simulating a device. Convenient for testing the protocol
    Simulated {
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
    fn into_updater(self) -> Result<FirmwareUpdater, anyhow::Error> {
        match self {
            FirmwareSource::File { file } => {
                let metadata = FirmwareFileMeta::from_file(&file)?;
                let mut file = File::open(&metadata.file)?;
                let mut data = Vec::new();
                file.read_to_end(&mut data)?;
                Ok(FirmwareUpdater::File { metadata, data })
            }
            FirmwareSource::Cloud {
                http,
                application,
                device,
                password,
            } => Ok(FirmwareUpdater::Cloud {
                client: reqwest::Client::new(),
                url: format!("{}/v1/dfu", http),
                user: format!("{}@{}", device, application),
                password: password.to_string(),
                timeout: std::time::Duration::from_secs(30),
            }),
        }
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
            #[cfg(feature = "bluez")]
            Transport::BleGatt { device, source } => {
                use std::sync::Arc;
                let session = bluer::Session::new().await?;
                let adapter = session.default_adapter().await?;
                adapter.set_powered(true).await?;

                let mut s = GattBoard::new(&device, Arc::new(adapter));
                let updater: FirmwareUpdater = source.into_updater()?;
                updater.run(&mut s).await?;
            }
            Transport::Serial { port, source } => {
                let mut s = SerialUpdater::new(&port)?;
                let updater: FirmwareUpdater = source.into_updater()?;
                updater.run(&mut s).await?;
            }
            Transport::Simulated { source } => {
                let mut s = DeviceSimulator::new();
                let updater: FirmwareUpdater = source.into_updater()?;
                updater.run(&mut s).await?;
            }
        },
    }
    Ok(())
}
