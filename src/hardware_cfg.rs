// hardware_cfg.rs - WITH PROPER GENERIC TYPES

#[cfg(feature = "lora")]
use sx127x_lora::{LoRa, Error as LoRaError};
use rppal::gpio::{Gpio, Trigger};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// sx127x_lora requires embedded-hal types with specific traits!

/// Custom error types for hardware operations (extends LoRaError)
#[derive(Debug)]
pub enum HardwareError {
    SpiCreation(String),
    GpioCreation(String),
    InterruptSetup(String),
    PinInitialization(u8, String),
    #[cfg(feature = "lora")]
    LoRaInitialization(LoRaError), // Use LoRa's Error type!
    Deserialization(bincode::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for HardwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HardwareError::SpiCreation(msg) => write!(f, "SPI creation: {}", msg),
            HardwareError::GpioCreation(msg) => write!(f, "GPIO creation: {}", msg),
            HardwareError::InterruptSetup(msg) => write!(f, "Interrupt setup: {}", msg),
            HardwareError::PinInitialization(pin, msg) => {
                write!(f, "Pin {} init: {}", pin, msg)
            }
            #[cfg(feature = "lora")]
            HardwareError::LoRaInitialization(err) => {
                write!(f, "LoRa initialization error: {:?}", err)
            }
            HardwareError::Deserialization(err) => {
                write!(f, "Deserialization error: {:?}", err)
            }
            HardwareError::Io(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for HardwareError {}

/// LoRa module handle (after initialization) - WITH PROPER GENERICS!
#[cfg(feature = "lora")]
pub struct LoRaHandle<SPI, CS, RESET, DELAY> {
    radio: LoRa<SPI, CS, RESET, DELAY>,  // ✅ Generic parameters required!
}

impl Drop for LoRaHandle<_, _, _, _> {
    fn drop(&mut self) {
        println!("LoRa hardware cleanup triggered");
    }
}

/// Hardware abstraction with safe error handling
pub struct Hardware {
    spi: Spi,  // rppal::Spi (we'll convert to LoRa's SPI type)
    dio0_pin: Gpio,   // Generic GPIO pin for interrupt
    cs_pin: Option<Gpio>,
    reset_pin: Option<Gpio>,
    radio: Option<LoRaHandle<Spi, Spi, Spi, std::time::Duration>>,  // Placeholder
}

impl Hardware {
    /// Create hardware instance with optional pin numbers
    pub fn new(cs_gpio: u8, rst_gpio: u8) -> Result<Self, HardwareError> {
        // ------------------------------------------------------------------
        // 1. SPI Initialization
        // ------------------------------------------------------------------
        let spi = Spi::new(
            Bus::Spi0, 
            SlaveSelect::Ss0, 
            8_000_000, 
            Mode::Mode0
        ).map_err(|e| HardwareError::SpiCreation(format!(
            "Failed to create SPI: {}", e
        )))?;

        // ------------------------------------------------------------------
        // 2. GPIO Pin Creation (with better error handling)
        // ------------------------------------------------------------------
        let gpio = Gpio::new().map_err(|e| {
            HardwareError::GpioCreation(format!("Failed to create GPIO: {}", e))
        })?;

        // DIO0 Interrupt Pin (always required for RFM95) - Use generic GPIO
        let dio_pin_result: Result<Gpio, rppal::gpio::Error> = gpio.get(25);
        let dio0_pin = match dio_pin_result {
            Ok(pin) => pin.into_input(),  // Convert to input immediately
            Err(e) => return Err(HardwareError::PinInitialization(
                25, 
                format!("GPIO 25 not available for RFM95 DIO0 interrupt: {}", e)
            )),
        };
        
        // Configure interrupt on the GPIO pin
        dio0_pin.set_interrupt(Trigger::RisingEdge).map_err(|e| HardwareError::InterruptSetup(format!(
            "Failed to set DIO0 interrupt: {}", e
        )))?;

        // ------------------------------------------------------------------
        // 3. CHIP SELECT PIN (CS) - Must be output, set HIGH by default
        // ------------------------------------------------------------------
        let cs_gpio_result: Result<Gpio, rppal::gpio::Error> = gpio.get(cs_gpio);
        let mut cs_pin: Option<Gpio> = match cs_gpio_result {
            Ok(pin) => Some(pin.into_output()),  // Convert to output immediately
            Err(e) => return Err(HardwareError::PinInitialization(
                cs_gpio, 
                format!("Chip Select pin not available for RFM95: {}", e)
            )),
        };

        // Ensure CS is HIGH (inactive) before initialization
        if let Some(ref mut pin) = cs_pin {
            pin.set_high().map_err(|e| HardwareError::Io(e))?;
        }

        // ------------------------------------------------------------------
        // 4. RESET PIN - Must be output, set HIGH by default (active-high reset)
        // ------------------------------------------------------------------
        let rst_gpio_result: Result<Gpio, rppal::gpio::Error> = gpio.get(rst_gpio);
        let mut reset_pin: Option<Gpio> = match rst_gpio_result {
            Ok(pin) => Some(pin.into_output()),  // Convert to output immediately
            Err(e) => return Err(HardwareError::PinInitialization(
                rst_gpio, 
                format!("Reset pin not available for RFM95: {}", e)
            )),
        };

        // Ensure Reset is HIGH (active) before initialization
        if let Some(ref mut pin) = reset_pin {
            pin.set_high().map_err(|e| HardwareError::Io(e))?;
        }

        Ok(Self {
            spi,
            dio0_pin,
            cs_pin: Some(cs_pin.unwrap()),  // Keep as Option for now
            reset_pin: Some(reset_pin.unwrap()),
            radio: None, // Will be initialized on first use
        })
    }

    /// Initialize LoRa module (calls your provided init code)
    #[cfg(feature = "lora")]
    pub fn init_radio(&mut self) -> Result<LoRaHandle<Spi, Spi, Spi, Duration>, HardwareError> {
        const FREQUENCY_MHZ: i64 = 868; // Match crate's expected i64 frequency
        
        // Convert GPIO pins to sx127x_lora compatible types!
        let cs_pin: OutputPin = self.cs_pin.unwrap().into();  // Use embedded-hal trait
        let reset_pin: OutputPin = self.reset_pin.unwrap().into();

        match LoRa::new(
            &self.spi,  // Pass rppal's Spi (needs to implement Transfer/Write traits)
            cs_pin,     // OutputPin
            reset_pin,  // OutputPin
            FREQUENCY_MHZ,
            Duration::from_millis(10),  // Delay implementation
        ) {
            Ok(radio) => {
                // 1. Set Maximum Bandwidth (500 kHz)
                let _ = radio.set_signal_bandwidth(500_000);
                
                // 2. Set Lowest Spreading Factor (SF7)
                let _ = radio.set_spreading_factor(7);
                
                // 3. Set the lowest Coding Rate (4/5)
                let _ = radio.set_coding_rate_4(5);
                
                // 17 dBm - max power
                let _ = radio.set_tx_power(17, 1);
                
                log::info!("RFM95 LoRa Module initialized successfully!");
                
                Ok(LoRaHandle { radio })
            }
            Err(e) => {
                log::error!("Failed to initialize RFM95 LoRa module: {:?}", e);
                Err(HardwareError::LoRaInitialization(e))
            }
        }
    }

    /// Check if LoRa radio is initialized
    pub fn is_radio_initialized(&self) -> bool {
        self.radio.is_some()
    }

    /// Get reference to LoRa handle (for TX/RX operations)
    #[cfg(feature = "lora")]
    pub fn get_radio(&self) -> Option<&LoRaHandle<_, _, _, _>> {
        self.radio.as_ref()
    }
    
    // Placeholder for transmit_command until implemented
    pub fn transmit_command<SPI, CS, RESET, DELAY>(
        &mut self,
        radio: &mut LoRa<SPI, CS, RESET, DELAY>,
        cmd: &[u8]
    ) -> Result<(), HardwareError> {
        match radio.transmit(cmd) {
            Ok(_) => Ok(()),
            Err(e) => Err(HardwareError::LoRaInitialization(e.into())),
        }
    }
}


