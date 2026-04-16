// hardware_cfg.rs
#[cfg(feature = "lora")]
use sx127x_lora::{LoRa, Error as LoRaError};
use rppal::gpio::{Gpio, Trigger, OutputPin as RppalOutputPin};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::sync::Arc;
use std::time::Duration;

// IMPORTANT: sx127x_lora 0.3.1 requires embedded-hal 0.2.x traits (the v2 module)
use embedded_hal::digital::v2::OutputPin; 

/// Custom error types for hardware operations
#[derive(Debug)]
pub enum HardwareError {
    SpiCreation(String),
    GpioCreation(String),
    InterruptSetup(String),
    PinInitialization(u8, String),
    #[cfg(feature = "lora")]
    // The LoRa error needs 3 generics: SPI, CS, and RESET types.
    LoRaInitialization(LoRaError<Spi, RppalOutputPin, RppalOutputPin>),
    Deserialization(String), 
    Io(std::io::Error),
}

impl std::fmt::Display for HardwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HardwareError::SpiCreation(msg) => write!(f, "SPI creation: {}", msg),
            HardwareError::GpioCreation(msg) => write!(f, "GPIO creation: {}", msg),
            HardwareError::InterruptSetup(msg) => write!(f, "Interrupt setup: {}", msg),
            HardwareError::PinInitialization(pin, msg) => write!(f, "Pin {} init: {}", pin, msg),
            #[cfg(feature = "lora")]
            HardwareError::LoRaInitialization(err) => write!(f, "LoRa init error: {:?}", err),
            HardwareError::Deserialization(err) => write!(f, "Deserialization error: {}", err),
            HardwareError::Io(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for HardwareError {}

#[cfg(feature = "lora")]
pub struct LoRaHandle {
    // We define the concrete types here so you don't have to carry generics everywhere
    pub radio: LoRa<Spi, RppalOutputPin, RppalOutputPin, Duration>,
}

pub struct Hardware {
    pub spi: Spi,
    pub dio0_pin: rppal::gpio::InputPin,
    pub cs_pin: Option<RppalOutputPin>,
    pub reset_pin: Option<RppalOutputPin>,
    pub radio: Option<LoRaHandle>,
}

impl Hardware {
    pub fn new(cs_gpio: u8, rst_gpio: u8) -> Result<Self, HardwareError> {
        let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 8_000_000, Mode::Mode0)
            .map_err(|e| HardwareError::SpiCreation(e.to_string()))?;

        let gpio = Gpio::new()
            .map_err(|e| HardwareError::GpioCreation(e.to_string()))?;

        let dio0_pin = gpio.get(25)
            .map_err(|e| HardwareError::PinInitialization(25, e.to_string()))?
            .into_input();
        
        let mut cs_pin = gpio.get(cs_gpio)
            .map_err(|e| HardwareError::PinInitialization(cs_gpio, e.to_string()))?
            .into_output();
        cs_pin.set_high();

        let mut reset_pin = gpio.get(rst_gpio)
            .map_err(|e| HardwareError::PinInitialization(rst_gpio, e.to_string()))?
            .into_output();
        reset_pin.set_high();

        Ok(Self {
            spi,
            dio0_pin,
            cs_pin: Some(cs_pin),
            reset_pin: Some(reset_pin),
            radio: None,
        })
    }

    #[cfg(feature = "lora")]
    pub fn init_radio(&mut self) -> Result<(), HardwareError> {
        // Move pins out of Option to give ownership to the LoRa driver
        let cs = self.cs_pin.take().ok_or(HardwareError::PinInitialization(0, "CS missing".into()))?;
        let rst = self.reset_pin.take().ok_or(HardwareError::PinInitialization(0, "RST missing".into()))?;

        // rppal::Spi implements Clone, so we can use it here
        match LoRa::new(self.spi.clone(), cs, rst, 868, Duration::from_millis(10)) {
            Ok(mut radio) => {
                let _ = radio.set_signal_bandwidth(500_000);
                let _ = radio.set_spreading_factor(7);
                let _ = radio.set_tx_power(17, 1);
                
                self.radio = Some(LoRaHandle { radio });
                Ok(())
            }
            Err(e) => Err(HardwareError::LoRaInitialization(e)),
        }
    }

    pub fn transmit_command(&mut self, cmd: &str) -> Result<(), HardwareError> {
        if let Some(ref mut handle) = self.radio {
            handle.radio.transmit(cmd.as_bytes())
                .map_err(|e| HardwareError::LoRaInitialization(e))?;
            Ok(())
        } else {
            Err(HardwareError::PinInitialization(0, "Radio not initialized".into()))
        }
    }
}
