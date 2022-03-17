use bluer::{
    gatt::remote::{Characteristic, Service},
    Device,
};
use std::str::FromStr;

pub struct GattBoard {
    device: Device,
}

const FIRMWARE_SERVICE_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000186100001000800000805f9b34fb);
const CONTROL_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000123600001000800000805f9b34fb);
const OFFSET_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000123500001000800000805f9b34fb);
const FIRMWARE_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000123400001000800000805f9b34fb);
const VERSION_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000123700001000800000805f9b34fb);

impl GattBoard {
    pub fn new(device: Device) -> Self {
        Self { device }
    }

    async fn read_firmware_offset(&self) -> bluer::Result<u32> {
        let data = self
            .read_char(FIRMWARE_SERVICE_UUID, OFFSET_CHAR_UUID)
            .await?;
        Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    pub async fn read_firmware_version(&self) -> bluer::Result<String> {
        let data = self
            .read_char(FIRMWARE_SERVICE_UUID, VERSION_CHAR_UUID)
            .await?;
        Ok(String::from_str(core::str::from_utf8(&data).unwrap()).unwrap())
    }

    pub async fn mark_booted(&self) -> bluer::Result<()> {
        // Trigger DFU process
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[4])
            .await
    }

    pub fn address(&self) -> bluer::Address {
        self.device.address()
    }

    pub async fn start_firmware_update(&self) -> Result<(), anyhow::Error> {
        let mut buf = [0; 16];

        // Trigger DFU process
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[1])
            .await?;

        println!("Triggered DFU init sequence");
        // Wait until firmware offset is reset
        while self.read_firmware_offset().await? != 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        Ok(())
    }

    pub async fn write_firmware(
        &self,
        mut offset: u32,
        firmware: &[u8],
    ) -> Result<(), anyhow::Error> {
        let mut buf = [0; 16];
        for chunk in firmware.chunks(16) {
            buf[0..chunk.len()].copy_from_slice(chunk);
            if chunk.len() < buf.len() {
                buf[chunk.len()..].fill(0);
            }
            self.write_char(FIRMWARE_SERVICE_UUID, FIRMWARE_CHAR_UUID, &buf)
                .await?;
            log::info!("Write {} bytes at offset {}", buf.len(), offset);
            offset += buf.len() as u32;
            if offset % 4096 == 0 {
                println!("{} bytes written", offset)
            }

            // Wait until firmware offset is incremented
            while self.read_firmware_offset().await? != offset {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
        Ok(())
    }

    pub async fn swap_firmware(&self) -> Result<(), anyhow::Error> {
        // Write signal that DFU process is done and should be applied
        log::debug!("DFU process done, setting reset");
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[2])
            .await?;

        Ok(())
    }

    /*
    pub async fn update_firmware_from_file(&self, firmware: &Path) -> Result<(), anyhow::Error> {
        println!("Updating firmware from file {:?}", firmware);
        let mut f = File::open(firmware)?;

        let mut buffer = Vec::new();
        // read the whole file
        f.read_to_end(&mut buffer)?;
        self.update_firmware(&buffer[..]).await
    }
    */

    async fn read_char(&self, service: uuid::Uuid, c: uuid::Uuid) -> bluer::Result<Vec<u8>> {
        let service = self.find_service(service).await?.unwrap();
        let c = self.find_char(&service, c).await?.unwrap();

        let value = c.read().await?;
        Ok(value)
    }

    async fn write_char(
        &self,
        service: uuid::Uuid,
        c: uuid::Uuid,
        value: &[u8],
    ) -> bluer::Result<()> {
        let service = self.find_service(service).await?.unwrap();
        let c = self.find_char(&service, c).await?.unwrap();

        c.write(value).await
    }

    async fn find_char(
        &self,
        service: &Service,
        characteristic: uuid::Uuid,
    ) -> bluer::Result<Option<Characteristic>> {
        for c in service.characteristics().await? {
            let uuid = c.uuid().await?;
            if uuid == characteristic {
                return Ok(Some(c));
            }
        }
        Ok(None)
    }

    async fn find_service(&self, service: uuid::Uuid) -> bluer::Result<Option<Service>> {
        for s in self.device.services().await? {
            let uuid = s.uuid().await?;
            if uuid == service {
                return Ok(Some(s));
            }
        }
        Ok(None)
    }
}
