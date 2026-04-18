// hardware_cfg.rs
use sx127x_lora::LoRa;
use linux_embedded_hal::{Spidev, Pin, Delay};
use linux_embedded_hal::spidev::{SpidevOptions, SpiModeFlags};
use linux_embedded_hal::sysfs_gpio::Direction;

#[derive(Debug)]
pub enum HardwareError {
    SpiCreation(String),
    PinInitialization(u64, String),
    LoRaInitialization(String),
    TransmitError(String),
    ReceiveError(String),
}

impl std::fmt::Display for HardwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}
impl std::error::Error for HardwareError {}

pub struct Hardware {
    pub spi: Option<Spidev>,
    pub dio0_pin: Pin,
    pub cs_pin: Option<Pin>,
    pub reset_pin: Option<Pin>,
    pub radio: Option<LoRa<Spidev, Pin, Pin, Delay>>, 
}

impl Hardware {
    pub fn new(cs_gpio: u64, rst_gpio: u64) -> Result<Self, HardwareError> {
        // 1. Initialize Linux SPI (/dev/spidev0.0 is the standard Pi SPI0 port)
        let mut spi = Spidev::open("/dev/spidev0.0")
            .map_err(|e| HardwareError::SpiCreation(e.to_string()))?;
        
        let options = SpidevOptions::new()
             .bits_per_word(8)
             .max_speed_hz(8_000_000)
             .mode(SpiModeFlags::SPI_MODE_0)
             .build();
        spi.configure(&options).map_err(|e| HardwareError::SpiCreation(e.to_string()))?;

        // 2. Initialize GPIO 25 (DIO0) as Input
        let dio0_pin = Pin::new(25);
        dio0_pin.export().map_err(|e| HardwareError::PinInitialization(25, e.to_string()))?;
        dio0_pin.set_direction(Direction::In).map_err(|e| HardwareError::PinInitialization(25, e.to_string()))?;

        // 3. Initialize Chip Select as Output High
        let cs_pin = Pin::new(cs_gpio);
        cs_pin.export().map_err(|e| HardwareError::PinInitialization(cs_gpio, e.to_string()))?;
        cs_pin.set_direction(Direction::High).map_err(|e| HardwareError::PinInitialization(cs_gpio, e.to_string()))?;

        // 4. Initialize Reset as Output High
        let reset_pin = Pin::new(rst_gpio);
        reset_pin.export().map_err(|e| HardwareError::PinInitialization(rst_gpio, e.to_string()))?;
        reset_pin.set_direction(Direction::High).map_err(|e| HardwareError::PinInitialization(rst_gpio, e.to_string()))?;

        Ok(Self {
            spi: Some(spi),
            dio0_pin,
            cs_pin: Some(cs_pin),
            reset_pin: Some(reset_pin),
            radio: None,
        })
    }

    pub fn init_radio(&mut self) -> Result<(), HardwareError> {
        let spi = self.spi.take().ok_or(HardwareError::LoRaInitialization("SPI missing".into()))?;
        let cs = self.cs_pin.take().ok_or(HardwareError::LoRaInitialization("CS missing".into()))?;
        let rst = self.reset_pin.take().ok_or(HardwareError::LoRaInitialization("RST missing".into()))?;

        // Initialize sx127x_lora with linux_embedded_hal's Delay
        match LoRa::new(spi, cs, rst, 868, Delay) {
            Ok(mut radio) => {
                let _ = radio.set_tx_power(17, 1);
                self.radio = Some(radio);
                Ok(())
            }
            Err(_) => Err(HardwareError::LoRaInitialization("Failed to init LoRa".into())),
        }
    }

    pub fn transmit_command(&mut self, cmd: &str) -> Result<(), HardwareError> {
        if let Some(ref mut r) = self.radio {
            let bytes = cmd.as_bytes();
            let mut buffer = [0u8; 255];
            
            // Get the length, making sure we don't exceed 255 bytes
            let len = bytes.len().min(255);
            
            // Copy the command string into our fixed-size 255-byte array
            buffer[..len].copy_from_slice(&bytes[..len]);
            
            // Pass the fixed array and the usize length
            r.transmit_payload(buffer, len)
                .map_err(|_| HardwareError::TransmitError("TX failed".into()))?;
            Ok(())
        } else {
            Err(HardwareError::TransmitError("Radio not initialized".into()))
        }
    }
    
    pub fn read_packet(&mut self) -> Result<Vec<u8>, HardwareError> {
        if let Some(ref mut r) = self.radio {
            match r.read_packet() {
                // Convert the fixed [u8; 255] array into a flexible Vec<u8>
                Ok(buffer) => Ok(buffer.to_vec()), 
                Err(_) => Err(HardwareError::ReceiveError("RX failed".into()))
            }
        } else {
            Err(HardwareError::ReceiveError("Radio not initialized".into()))
        }
    }
}