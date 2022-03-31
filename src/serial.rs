use crate::firmware::FirmwareDevice;
use anyhow::anyhow;
use async_trait::async_trait;
use postcard::{from_bytes, to_slice};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::SerialStream;

pub struct SerialUpdater {
    port: SerialStream,
    buffer: [u8; FRAME_SIZE],
}

impl SerialUpdater {
    pub fn new(port: &PathBuf) -> Result<Self, anyhow::Error> {
        let p: String = port.to_str().unwrap().to_string();
        let builder = tokio_serial::new(p, 115200);
        Ok(Self {
            port: SerialStream::open(&builder)?,
            buffer: [0; FRAME_SIZE],
        })
    }

    async fn request<'m>(
        &'m mut self,
        command: SerialCommand<'m>,
    ) -> Result<Option<SerialResponse<'m>>, anyhow::Error> {
        to_slice(&command, &mut self.buffer)?;
        self.port.write(&self.buffer).await?;

        self.port.read_exact(&mut self.buffer).await?;
        let response: Result<Option<SerialResponse>, SerialError> = from_bytes(&self.buffer)?;
        match response {
            Ok(r) => Ok(r),
            Err(e) => Err(anyhow!("Error frame: {:?}", e)),
        }
    }
}

#[async_trait]
impl FirmwareDevice for SerialUpdater {
    const MTU: u32 = 768;
    async fn version(&mut self) -> Result<String, anyhow::Error> {
        let response = self.request(SerialCommand::Version).await?;
        if let Some(SerialResponse::Version(v)) = response {
            Ok(core::str::from_utf8(&v[..])?.to_string())
        } else {
            Err(anyhow!("Error reading version"))
        }
    }

    async fn status(&mut self) -> Result<Option<(u32, String)>, anyhow::Error> {
        Ok(None)
    }

    async fn start(&mut self, _: &str) -> Result<(), anyhow::Error> {
        self.request(SerialCommand::Start).await?;
        Ok(())
    }
    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), anyhow::Error> {
        self.request(SerialCommand::Write(offset, data)).await?;
        Ok(())
    }

    async fn swap(&mut self, _: [u8; 32]) -> Result<(), anyhow::Error> {
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
