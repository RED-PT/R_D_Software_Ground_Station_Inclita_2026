use axum::{Router, routing::get};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tower_http::services::ServeFile;
use std::env;
use dotenvy::dotenv;
use tracing::{info, error};
use tracing_subscriber::{EnvFilter};


mod mock;
mod handlers;
mod telemetry;

use telemetry::data::Telemetry;
use handlers::sockets_handler::{AppState, ws_handler};



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
    
    // Fetch the settings from the .env,  or use defaults if they are missing
    let host = env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_string());
    let bind_address = format!("{}:{}", host, port);
    let (telemetry_tx, _) = broadcast::channel::<Telemetry>(100);
    let (command_tx, mut command_rx) = mpsc::channel::<String>(32);


    // This reads the RUST_LOG environment variable. If missing, it defaults to "info"
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false) // Hides the module path to keep the console clean
        .with_thread_ids(false)
        .with_file(true) // Shows which file the log came from
        .with_line_number(true) // Shows the exact line number!
        .init();

    // CSV LOGGER TASK
    let mut logger_rx = telemetry_tx.subscribe();
    tokio::spawn(async move {
        info!("Logger initialized. Saving to 'flight_data.csv'...");
        
        // Open or create the CSV file
        let mut wtr = match csv::Writer::from_path("flight_data.csv") {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to initialize CSV logger: {}", e);
                return;
            }
        };

        // Listen for new telemetry packets forever
        while let Ok(data) = logger_rx.recv().await {
            // Serialize the struct directly into a CSV row
            if let Err(e) = wtr.serialize(&data) {
                error!("CSV Write Error: {}", e);
            } else {
                // Force flush to disk immediately! 
                // This prevents data loss if the Pi suddenly loses power.
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

        // 1. Start Mock Ingest
        mock::spawn_mock_telemetry_task(shared_state.telemetry_tx.clone());

        // 2. Start Mock Transmit Task
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
                if let Err(e) = hw.init_radio() {
                    error!("RFM95 initialization failed: {}", e);
                    std::process::exit(1);
                }
                info!("LoRa initialized!");

                let shared_state = Arc::new(AppState {
                    telemetry_tx: telemetry_tx.clone(),
                    command_tx,
                    hardware: Arc::new(Mutex::new(hw)),
                });

                // 1. Start Hardware Ingest
                let ingest_state = shared_state.clone();
                tokio::task::spawn_blocking(move || {
                    loop {
                        {
                            let mut hw_lock = ingest_state.hardware.blocking_lock();
                            if hw_lock.dio0_pin.is_high().unwrap_or(false) {
                                if let Ok(ref buffer) = hw_lock.read_packet() {
                                    if let Ok(data) = bincode::deserialize::<Telemetry>(buffer) {
                                        info!("Received Telemetry! Pressure: {}", data.pressure);
                                        let _ = ingest_state.telemetry_tx.send(data);
                                    }
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                });

                // 2. Start Hardware Transmit
                let tx_state = shared_state.clone();
                tokio::spawn(async move {
                    while let Some(cmd) = command_rx.recv().await {
                        let mut hw_lock = tx_state.hardware.lock().await;
                        if let Err(e) = hw_lock.transmit_command(&cmd) {
                            error!("TX Error: {}", e);
                        } else {
                            info!("Command '{}' transmitted successfully.", cmd);
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





// Extracted server startup so we don't write it twice
async fn start_server(shared_state: Arc<AppState>, bind_address: &str) {
    let app = Router::new()
        .nest_service("/", ServeFile::new("html/index.html"))
        .route("/ws", get(ws_handler))
        .with_state(shared_state);

    let listener = tokio::net::TcpListener::bind(&bind_address).await.unwrap();
    info!("Ground Station UI available at: http://{}", &bind_address);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

pub async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.unwrap();
    info!("Shutdown signal received. Powering down.");
}
