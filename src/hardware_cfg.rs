// hardware.rs - WITH RFM95 INITIALIZATION (PROPER ERROR HANDLING)

use rppal::gpio::{Gpio, Trigger};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::time::Duration;
use sx127x_lora::LoRa;
/// Custom error types for hardware operations
#[derive(Debug)]
pub enum HardwareError {
    SpiCreation(String),
    GpioCreation(String),
    InterruptSetup(String),
    PinInitialization(u8, String),
    LoRaInitialization(String),
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
            HardwareError::LoRaInitialization(msg) => {
                write!(f, "LoRa initialization: {}", msg)
            }
            HardwareError::Deserialization(err) => {
                write!(f, "Deserialization error: {:?}", err)
            }
            HardwareError::Io(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for HardwareError {}

/// LoRa module handle (after initialization)
pub struct LoRaHandle {
    radio: LoRa<SPI, CS, RESET, DELAY> , // Replace with actual type from your crate
}

impl Drop for LoRaHandle {
    fn drop(&mut self) {
        // Optional: Cleanup resources before dropping
        println!("LoRa hardware cleanup triggered");
    }
}

/// Hardware abstraction with safe error handling
pub struct Hardware {
    spi: Option<Spi<Bus, SlaveSelect>>,
    dio0_pin: Gpio25Input,
    cs_pin: Option<GpioOutput<u8>>,
    reset_pin: Option<GpioOutput<u8>>,
    radio: Option<LoRaHandle>, // Optional - initialized only when requested
}

impl Hardware {
    /// Create hardware instance with optional pin numbers
    pub fn new(cs_gpio: u8, rst_gpio: u8) -> Result<Self, HardwareError> {
        // ------------------------------------------------------------------
        // 1. SPI Initialization (return error instead of unwrap)
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
            HardwareError::GpioCreation(format!(
                "Failed to create GPIO: {}", e
            ))
        })?;

        // DIO0 Interrupt Pin (always required for RFM95)
        let dio0_pin: Gpio25Input = gpio.get(25).or_else(|_| {
            HardwareError::PinInitialization(
                25, 
                "GPIO 25 not available for RFM95 DIO0 interrupt".to_string()
            )
        })?.into_input();
        
        dio0_pin.set_interrupt(Trigger::RisingEdge)
            .map_err(|e| HardwareError::InterruptSetup(format!(
                "Failed to set DIO0 interrupt: {}", e
            )))?;

        // ------------------------------------------------------------------
        // 3. CHIP SELECT PIN (CS) - Must be output, set HIGH by default
        // ------------------------------------------------------------------
        let cs_gpio_obj = gpio.get(cs_gpio).or_else(|_| {
            HardwareError::PinInitialization(
                cs_gpio, 
                "Chip Select pin not available for RFM95".to_string()
            )
        })?;
        let mut cs_pin: GpioOutput<u8> = cs_gpio_obj.into_output();
        
        // Ensure CS is HIGH (inactive) before initialization
        let _ = cs_pin.set_high().map_err(|e| HardwareError::Io(e));

        // ------------------------------------------------------------------
        // 4. RESET PIN - Must be output, set HIGH by default (active-high reset)
        // ------------------------------------------------------------------
        let rst_gpio_obj = gpio.get(rst_gpio).or_else(|_| {
            HardwareError::PinInitialization(
                rst_gpio, 
                "Reset pin not available for RFM95".to_string()
            )
        })?;
        let mut reset_pin: GpioOutput<u8> = rst_gpio_obj.into_output();
        
        // Ensure Reset is HIGH (active) before initialization
        let _ = reset_pin.set_high().map_err(|e| HardwareError::Io(e));

        Ok(Self {
            spi: Some(spi),
            dio0_pin,
            cs_pin: Some(cs_pin),
            reset_pin: Some(reset_pin),
            radio: None, // Will be initialized on first use
        })
    }

    /// Initialize LoRa module (calls your provided init code)
    pub fn init_radio(&mut self) -> Result<LoRaHandle, HardwareError> {
        const FREQUENCY_MHZ: f64 = 868.0; // Match your i32 frequency
        let delay = Duration::from_millis(10); // Typical initialization delay
        
        match LoRa::new(
            self.spi.as_ref().unwrap(),
            &self.cs_pin.unwrap(),
            &self.reset_pin.unwrap(),
            FREQUENCY_MHZ,
            delay
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
                
                self.radio = Some(LoRaHandle { radio });
                Ok(self.radio.take().unwrap())
            }
            Err(e) => {
                // Use proper error handling instead of panic/unwrap
                log::error!("Failed to initialize RFM95 LoRa module: {:?}", e);
                Err(HardwareError::LoRaInitialization(
                    format!("Failed to initialize RFM95: {}", e)
                ))
            }
        }
    }

    /// Check if LoRa radio is initialized
    pub fn is_radio_initialized(&self) -> bool {
        self.radio.is_some()
    }

    /// Get reference to LoRa handle (for TX/RX operations)
    pub fn get_radio(&self) -> Option<&LoRaHandle> {
        self.radio.as_ref()
    }
}

