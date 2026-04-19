
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc}; 

use std::env;
use dotenvy::dotenv;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;

mod mock;
mod handlers;
mod telemetry;

use handlers::helper_foo::{start_server};
use telemetry::data::Telemetry;
use handlers::sockets_handler::{AppState};

// Only include the hardware module if we are NOT mocking
#[cfg(not(feature = "mock"))]
mod hardware_cfg;
#[cfg(not(feature = "mock"))]
pub use crate::hardware_cfg::{Hardware, HardwareError};
#[cfg(not(feature = "mock"))]
use embedded_hal::digital::v2::InputPin;
#[cfg(not(feature = "mock"))]
use tokio::time::Duration;

#[tokio::main]
async fn main() {
    dotenv().ok();
    
    // Fetch the settings from the .env, or use defaults if they are missing
    let host = env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_string());
    let bind_address = format!("{}:{}", host, port);
    
    let (telemetry_tx, _) = broadcast::channel::<Telemetry>(100);
    let (command_tx, mut command_rx) = mpsc::channel::<String>(32);

    // Initialize Tracing
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .init();

    // CSV LOGGER TASK
    let mut logger_rx = telemetry_tx.subscribe();
    tokio::spawn(async move {
        info!("Logger initialized. Saving to 'flight_data.csv'...");
        let mut wtr = match csv::Writer::from_path("flight_data.csv") { //
            Ok(w) => w,
            Err(e) => {
                error!("Failed to initialize CSV logger: {}", e);
                return;
            }
        };

        while let Ok(data) = logger_rx.recv().await {
            if let Err(e) = wtr.serialize(&data) {
                error!("CSV Write Error: {}", e);
            } else {
                let _ = wtr.flush(); 
            }
        }
    });

    // MOCK MODE
    #[cfg(feature = "mock")]
    {
        info!("Starting Groundstation in COMPILE-TIME MOCK MODE...");

        let shared_state = Arc::new(AppState {
            telemetry_tx: telemetry_tx.clone(),
            command_tx,
        });

        //Start Mock Ingest
        mock::spawn_mock_telemetry_task(shared_state.telemetry_tx.clone());

        // Start Mock Transmit Task
        tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                info!("[MOCK TX] Pretending to send command: {}", cmd);
            }
        });

        start_server(shared_state, &bind_address).await;
    }

    // HARDWARE MODE (Compiled on Pi)
    #[cfg(not(feature = "mock"))]
    {
        info!("Starting Groundstation in REAL HARDWARE MODE...");

        const CS_GPIO: u64 = 10; 
        const RST_GPIO: u64 = 5; 

        match Hardware::new(CS_GPIO.into(), RST_GPIO.into()) {
            Ok(mut hw) => {
                if let Err(e) = hw.init_radio() { //
                    error!("RFM95 initialization failed: {}", e);
                    std::process::exit(1);
                }
                info!("LoRa initialized!");

                let shared_state = Arc::new(AppState {
                    telemetry_tx: telemetry_tx.clone(),
                    command_tx,
                });

                let actor_telemetry_tx = telemetry_tx.clone();

                // THE HARDWARE ACTOR
               
                tokio::spawn(async move {
                    info!("🎭 Hardware Manager Actor online.");
                    
                    // Replaces your thread::sleep
                    let mut poll_interval = tokio::time::interval(Duration::from_millis(10));

                    loop {
                        tokio::select! {
                            // UPLINK 
                            Some(cmd) = command_rx.recv() => {
                                if let Err(e) = hw.transmit_command(&cmd) {
                                    error!("TX Error: {}", e);
                                } else {
                                    info!("Command '{}' transmitted successfully.", cmd);
                                }
                            }

                            // DOWNLINK
                            _ = poll_interval.tick() => {
                                if hw.dio0_pin.is_high().unwrap_or(false) {
                                    if let Ok(ref buffer) = hw.read_packet() {
                                        if let Ok(data) = bincode::deserialize::<Telemetry>(buffer) {
                                            info!("Received Telemetry! Pressure: {}", data.pressure);
                                            let _ = actor_telemetry_tx.send(data);
                                        }
                                    }
                                }
                            }
                        }
                    }
                });

                start_server(shared_state, &bind_address).await;
            }
            Err(e) => {
                error!("Hardware init failed: {}", e);
                std::process::exit(1);
            }
        }
    }
}

