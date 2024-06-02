use anyhow::bail;
use byteorder::{BigEndian, ByteOrder};
use bytes::{Buf, BufMut, BytesMut};
use crc32fast::Hasher;
use hex::FromHex;
use std::time::Duration;
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Seek, Write},
    mem::size_of,
    path::PathBuf,
    u16,
};

const BOARDINFO_SIZE: usize = 50;
const HEADER_VER: u16 = 1u16;

pub struct BoardInfo {
    _reserved: u16,
    crc32: u32,
    hdr_version: u16,
    hw_version: u16,
    factory_date: u16,
    factory_serial: [u8; 16],
    product_name: [u8; 16],
    mac: [u8; 6],
}

impl BoardInfo {
    pub fn verify_eeprom(&self) -> anyhow::Result<()> {
        use io::Read;
        let eeprom = Self::find_i2c_device()?;
        let mut file = OpenOptions::new().read(true).open(eeprom)?;
        let mut bytes = BytesMut::zeroed(BOARDINFO_SIZE);
        file.read_exact(bytes.as_mut())?;

        let mut hasher = Hasher::new();
        hasher.update(&bytes[6..]);
        let cksum = hasher.finalize();

        if self.crc32 != cksum {
            bail!(
                "EEPROM checksum mismatch! read {:x}, expected {:x}",
                self.crc32,
                cksum
            );
        }
        Ok(())
    }

    pub fn load() -> io::Result<Self> {
        let eeprom = Self::find_i2c_device()?;
        let file = OpenOptions::new().read(true).open(eeprom)?;
        Self::from_reader(file)
    }

    pub fn from_reader(mut reader: impl io::Read) -> io::Result<Self> {
        let mut bytes = BytesMut::zeroed(BOARDINFO_SIZE);
        reader.read_exact(bytes.as_mut())?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(mut bytes: BytesMut) -> io::Result<Self> {
        let _reserved = bytes.get_u16();
        let crc32 = bytes.get_u32();
        let hdr_version = bytes.get_u16_le();
        let hw_version = bytes.get_u16_le();
        let factory_date = bytes.get_u16_le();
        let mut factory_serial = [0u8; 16];
        bytes.copy_to_slice(&mut factory_serial);
        let mut product_name = [0u8; 16];
        bytes.copy_to_slice(&mut product_name);
        let mut mac = [0u8; 6];
        bytes.copy_to_slice(&mut mac);

        Ok(BoardInfo {
            _reserved,
            crc32,
            hdr_version,
            hw_version,
            factory_date,
            factory_serial,
            product_name,
            mac,
        })
    }

    fn find_i2c_device() -> io::Result<PathBuf> {
        for entry in fs::read_dir("/sys/bus/i2c/devices/")? {
            let eeprom = entry?.path().join("eeprom");
            if eeprom.exists() {
                return Ok(eeprom);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing eeprom i2c device in /sys",
        ))
    }

    pub fn hw_version(&mut self, hw_version: u16) {
        self.hw_version = hw_version;
    }

    /// days since May 1th 2024
    pub fn factory_date(&mut self, days: u16) {
        self.factory_date = days;
    }

    pub fn factory_serial(&mut self, serial: impl AsRef<str>) {
        let trimmed = serial.as_ref().as_bytes().take(16);
        let mut buffer = BytesMut::zeroed(16);
        buffer.as_mut().put(trimmed);
        self.factory_serial.copy_from_slice(&buffer)
    }

    pub fn product_name(&mut self, name: impl AsRef<str>) {
        let name = name.as_ref().as_bytes().take(16);
        let mut buffer = BytesMut::zeroed(16);
        buffer.as_mut().put(name);
        self.product_name.copy_from_slice(&buffer);
    }

    pub fn mac(&mut self, mac: impl AsRef<str>) -> anyhow::Result<()> {
        let bytes = <[u8; 6]>::from_hex(mac.as_ref())?;
        self.mac.copy_from_slice(&bytes);
        Ok(())
    }

    pub fn write_back(&mut self) -> io::Result<()> {
        let eeprom = Self::find_i2c_device()?;
        let mut file = OpenOptions::new().write(true).truncate(true).open(eeprom)?;
        file.seek(io::SeekFrom::Start(0))?;

        let mut bytes = BytesMut::with_capacity(BOARDINFO_SIZE);
        bytes.put_u16(self._reserved);
        bytes.put_u32(self.crc32);
        // currently we only have one version
        bytes.put_u16_le(HEADER_VER);
        bytes.put_u16_le(self.hw_version);
        bytes.put_u16_le(self.factory_date);
        bytes.put_slice(&self.factory_serial);
        bytes.put_slice(&self.product_name);
        bytes.put_slice(&self.mac);

        // calculate crc
        let mut hasher = Hasher::new();
        hasher.update(&bytes[6..]);
        self.crc32 = hasher.finalize();
        BigEndian::write_u32(&mut bytes.as_mut()[2..6], self.crc32);

        println!(
            "writing to eeprom:\n{:#?}",
            BoardInfo::from_bytes(bytes.clone())
        );

        // workaround for buggy i2c bus
        for byte in bytes {
            file.write_all(&[byte])?;
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

/// Returns the semver version pointed to by `version_ptr` as a char*, prefixed
/// with 'v'. e.g. v2.5.1
///
/// The version field is encoded inside an uint16_t.
/// they are grouped as followed:
/// 5 bits for the major version
/// 5 bits for the minor version
/// 6 bits for the patch version
///
/// therefore the maximum supported version is 32.32.64
fn parse_version_field(version: u16) -> String {
    let major = version >> 11;
    let minor = (version >> 6) & 0x1F;
    let patch = version & 0x3F;

    format!("v{}.{}.{}", major, minor, patch)
}

impl fmt::Debug for BoardInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hw_version = parse_version_field(self.hw_version);
        let start_date = chrono::NaiveDate::from_ymd_opt(2023, 5, 1).expect("a valid date");

        let reserved = format!("0x{:x}", self._reserved);
        let crc = format!("0x{:04x}", self.crc32);
        let date = start_date + chrono::Duration::days(self.factory_date as i64);
        let product_name = String::from_utf8_lossy(&self.product_name);
        let factory_serial = String::from_utf8_lossy(&self.factory_serial);
        let mac: String = self.mac.iter().map(|b| format!("{:02x}", b)).collect();

        f.debug_struct("BoardInfo")
            .field("reserved", &reserved)
            .field("crc32", &crc)
            .field("header version", &self.hdr_version)
            .field("hardware version", &hw_version)
            .field("factory date", &date)
            .field("factory serial", &factory_serial)
            .field("product name", &product_name)
            .field("mac", &mac)
            .finish()
    }
}
