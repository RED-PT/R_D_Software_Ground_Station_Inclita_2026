// hardware_cfg.rs
use embedded_hal::delay::DelayNs;
use radio::{Receive, Transmit};
use radio_sx127x::{Sx127x, device::sx1276::Sx1276};
use rppal::gpio::{Gpio, InputPin, OutputPin};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::time::Duration;

// 1. Linux Delay Provider for embedded-hal 1.0
// The radio needs this to know how to "sleep" during initialization
pub struct LinuxDelay;
impl DelayNs for LinuxDelay {
    fn delay_ns(&mut self, ns: u32) {
        std::thread::sleep(Duration::from_nanos(ns as u64));
    }
}

// 2. Custom Error Types
#[derive(Debug)]
pub enum HardwareError {
    SpiCreation(String),
    GpioCreation(String),
    PinInitialization(u8, String),
    LoRaInitialization(String),
    TransmitError(String),
    ReceiveError(String),
}

impl std::fmt::Display for HardwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl std::error::Error for HardwareError {}

// 3. Concrete Type Alias - NO MORE GENERIC HELL!
// This locks the radio to exact rppal types.
pub type Rfm95Radio = Sx127x<Sx1276, Spi, OutputPin, OutputPin, LinuxDelay>;

pub struct Hardware {
    pub spi_bus: Option<Spi>,
    pub cs_pin: Option<OutputPin>,
    pub reset_pin: Option<OutputPin>,
    pub dio0_pin: InputPin,
    pub radio: Option<Rfm95Radio>,
}

impl Hardware {
    pub fn new(cs_gpio: u8, rst_gpio: u8) -> Result<Self, HardwareError> {
        let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 8_000_000, Mode::Mode0)
            .map_err(|e| HardwareError::SpiCreation(e.to_string()))?;

        let gpio = Gpio::new().map_err(|e| HardwareError::GpioCreation(e.to_string()))?;

        // DIO0 Interrupt Pin
        let dio0_pin = gpio
            .get(25)
            .map_err(|e| HardwareError::PinInitialization(25, e.to_string()))?
            .into_input();

        // Chip Select
        let mut cs_pin = gpio
            .get(cs_gpio)
            .map_err(|e| HardwareError::PinInitialization(cs_gpio, e.to_string()))?
            .into_output();
        cs_pin.set_high();

        // Reset
        let mut reset_pin = gpio
            .get(rst_gpio)
            .map_err(|e| HardwareError::PinInitialization(rst_gpio, e.to_string()))?
            .into_output();
        reset_pin.set_high();

        Ok(Self {
            spi_bus: Some(spi),
            cs_pin: Some(cs_pin),
            reset_pin: Some(reset_pin),
            dio0_pin,
            radio: None,
        })
    }

    pub fn init_radio(&mut self) -> Result<(), HardwareError> {
        // Move the pins into the driver
        let spi = self
            .spi_bus
            .take()
            .ok_or(HardwareError::LoRaInitialization("SPI missing".into()))?;
        let cs = self
            .cs_pin
            .take()
            .ok_or(HardwareError::LoRaInitialization("CS missing".into()))?;
        let rst = self
            .reset_pin
            .take()
            .ok_or(HardwareError::LoRaInitialization("RST missing".into()))?;
        let delay = LinuxDelay;

        // Initialize the SX1276 (RFM95)
        match Sx127x::spi(spi, cs, rst, delay, Sx1276::new()) {
            Ok(radio) => {
                self.radio = Some(radio);
                Ok(())
            }
            Err(_) => Err(HardwareError::LoRaInitialization(
                "Failed to init SX127x".into(),
            )),
        }
    }

    pub fn transmit_command(&mut self, cmd: &str) -> Result<(), HardwareError> {
        if let Some(ref mut r) = self.radio {
            r.transmit(cmd.as_bytes())
                .map_err(|_| HardwareError::TransmitError("TX failed".into()))?;
            Ok(())
        } else {
            Err(HardwareError::TransmitError("Radio not initialized".into()))
        }
    }

    // Abstracted Read method so main.rs doesn't need to know how the radio works
    pub fn read_packet(&mut self) -> Result<Vec<u8>, HardwareError> {
        if let Some(ref mut r) = self.radio {
            let mut buffer = [0u8; 255];

            // The radio crate provides standard receive traits
            match r.receive(&mut buffer) {
                Ok(_) => Ok(buffer.to_vec()),
                Err(_) => Err(HardwareError::ReceiveError("RX failed".into())),
            }
        } else {
            Err(HardwareError::ReceiveError("Radio not initialized".into()))
        }
    }
}
