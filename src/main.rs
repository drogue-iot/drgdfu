use std::path::PathBuf;
use clap::{Parser, Subcommand};

mod firmware;
mod gatt;
mod serial;

use firmware::*;
use gatt::*;
use serial::*;

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
        version: String,

        /// Firmware to generate metadata for
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
        Mode::Generate { version, file } => {
            // Generate metadata
            let firmware = FirmwareFileMeta::new(&version, &file)?;
            println!("{}", serde_json::to_string(&firmware)?);
        }
        Mode::Upload { transport } => match transport {
            Transport::BleGatt { device, source } => {
                let session = bluer::Session::new().await?;
                let adapter = session.default_adapter().await?;
                adapter.set_powered(true).await?;

                let mut s = GattBoard::new(&device, adapter);
                let updater = FirmwareUpdater::new(&source)?;
                updater.run(&mut s).await?;
            }
            Transport::Serial { port, source } => {
                let mut s = SerialUpdater::new(&port)?;
                let updater = FirmwareUpdater::new(&source)?;
                updater.run(&mut s).await?;
            }
        },
    }
    Ok(())
}
