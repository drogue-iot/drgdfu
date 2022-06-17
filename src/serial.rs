use anyhow::anyhow;
use core::future::Future;
use embedded_update::*;
use postcard::{from_bytes, to_slice};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::SerialStream;

pub struct SerialUpdater {
    serial: embedded_update::device::Serial<SerialStream>,
}

impl SerialUpdater {
    pub fn new(port: &PathBuf) -> Result<Self, anyhow::Error> {
        let p: String = port.to_str().unwrap().to_string();
        let builder = tokio_serial::new(p, 115200);
        Ok(Self {
            serial: embedded_update::device::Serial::new(SerialStream::open(&builder)?),
        })
    }
}

impl FirmwareDevice for SerialUpdater {
    const MTU: usize = 768;
    type Version = Vec<u8>;
    type Error = anyhow::Error;

    type StatusFuture<'m> = impl Future<Output = Result<FirmwareStatus<Vec<u8>>, Self::Error>> + 'm
    where
        Self: 'm;

    fn status(&mut self) -> Self::StatusFuture<'_> {
        async move {
            self.
        }
    }

    type StartFuture<'m> = impl Future<Output = Result<(), Self::Error>> + 'm
    where
        Self: 'm;
    fn start(&mut self, _: &str) -> Result<(), anyhow::Error> {
        async move {
            self.request(SerialCommand::Start).await?;
            Ok(())
        }
    }

    type WriteFuture<'m>
    where
        Self: 'm;

    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), anyhow::Error> {
        self.request(SerialCommand::Write(offset, data)).await?;
        Ok(())
    }

    type UpdateFuture<'m>
    where
        Self: 'm;

    fn update<'m>(&'m mut self, version: &'m [u8], checksum: &'m [u8]) -> Self::UpdateFuture<'m> {
        todo!()
    }

    async fn swap(&mut self, _: &str, _: [u8; 32]) -> Result<(), anyhow::Error> {
        self.request(SerialCommand::Swap).await?;
        match self.port.read_exact(&mut self.buffer).await {
            Ok(_) => {
                let response: Result<Option<SerialResponse>, SerialError> =
                    from_bytes(&self.buffer)?;
                match response {
                    Ok(_) => Ok(()),
                    Err(e) => Err(anyhow!("Error during swap: {:?}", e)),
                }
            }
            Err(_) => {
                Err(anyhow!("Serial port error. Rerun command once port has reappeared to mark firmware as swapped"))
            }
        }
    }

    type SyncedFuture<'m>
    where
        Self: 'm;

    async fn synced(&mut self) -> Result<(), anyhow::Error> {
        self.request(SerialCommand::Sync).await?;
        Ok(())
    }
}

/// Defines a serial protocol for DFU
use serde::{Deserialize, Serialize};
pub const FRAME_SIZE: usize = 1024;

#[derive(Serialize, Deserialize)]
pub enum SerialCommand<'a> {
    Version,
    Start,
    Write(u32, &'a [u8]),
    Swap,
    Sync,
}

#[derive(Serialize, Deserialize)]
pub enum SerialResponse<'a> {
    Version(&'a [u8]),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum SerialError {
    Flash,
    Busy,
    Memory,
    Protocol,
}
