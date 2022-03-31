use crate::firmware::FirmwareDevice;
use anyhow::anyhow;
use async_trait::async_trait;

// 1 MB flash images supported
pub const FLASH_SIZE: usize = 128;

// Device simulator where the first 4 bytes is used as version string. When swapped, new version is updated
pub struct DeviceSimulator {
    version: String,
    flash: [u8; FLASH_SIZE],
}

impl DeviceSimulator {
    pub fn new() -> Self {
        Self {
            version: "0000".to_string(),
            flash: [0x30; FLASH_SIZE],
        }
    }
}

#[async_trait]
impl FirmwareDevice for DeviceSimulator {
    const MTU: u32 = 7;
    async fn version(&mut self) -> Result<String, anyhow::Error> {
        Ok(self.version.clone())
    }

    async fn start(&mut self, _: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn status(&mut self) -> Result<Option<(u32, String)>, anyhow::Error> {
        Ok(None)
    }

    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), anyhow::Error> {
        let offset = offset as usize;
        if offset + data.len() >= self.flash.len() {
            Err(anyhow!("Writing outside flash limits"))
        } else {
            println!("Writing {} bytes to {}", data.len(), offset);
            self.flash[offset..offset + data.len()].copy_from_slice(data);
            Ok(())
        }
    }

    async fn swap(&mut self, _: [u8; 32]) -> Result<(), anyhow::Error> {
        self.version = core::str::from_utf8(&self.flash[0..4])?.to_string();
        Ok(())
    }

    async fn synced(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
