//! user imports
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};

use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tower_http::services::ServeFile;
use std::fs::OpenOptions;
use std::io::Write; // You will also need this for the writeln! macro to work!

use rppal::gpio::{Gpio, Trigger};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::time::Duration;

mod handlers;
mod telemetry;
use telemetry::data;
use handlers::sockets_handler::{AppState, ws_handler};

#[tokio::main]
async fn main() {
    println!("Starting Cyberdeck Groundstation...");

    let (telemetry_tx, _) = broadcast::channel::<data::Telemetry>(100);
    let (command_tx, mut command_rx) = mpsc::channel::<String>(32);

    let shared_state = Arc::new(AppState {
        telemetry_tx: telemetry_tx.clone(), // Clone for the Ingest thread to use
        command_tx,
    });

    // ------------------------------------------------------------------
    // 1. HARDWARE SETUP: Replace Serial with SPI & GPIO
    // ------------------------------------------------------------------
    let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 8_000_000, Mode::Mode0).unwrap();
    let gpio = Gpio::new().unwrap();

    // Set up the DIO0 pin (usually GPIO 25) for the RX interrupt
    let mut dio0_pin = gpio.get(25).unwrap().into_input();
    dio0_pin.set_interrupt(Trigger::RisingEdge, None).unwrap();

    // Initialize your LoRa driver (Pseudo-code, depends on the crate you use)
    // let mut lora = LoRa::new(spi, cs_pin, reset_pin, freq_868).unwrap();

    println!("LoRa Radio initialized on SPI0.");

    // ------------------------------------------------------------------
    // 2. THE INGEST TASK (Replaces your std::thread::spawn serial loop)
    // ------------------------------------------------------------------
    // We use spawn_blocking because rppal's poll_interrupt blocks the thread
    let ingest_tx = telemetry_tx.clone();
    tokio::task::spawn_blocking(move || {
        let mut log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("lora_telemetry.log")
            .expect("Failed to open log file");

        writeln!(log_file, "\n--- NEW LAUNCH SESSION ---").unwrap();

        loop {
            // Wait for the LoRa module to pull DIO0 high (Packet Received!)
            // Timeout of 1 second so the thread isn't locked forever
            if let Ok(Some(_)) = dio0_pin.poll_interrupt(true, Some(Duration::from_secs(1))) {

                // 1. Read data from SPI (Depends on your LoRa crate)
                // let buffer = lora.read_packet().unwrap();

                // 2. Deserialize your payload bytes into the Telemetry struct
                // let telemetry: protocol::Telemetry = bincode::deserialize(&buffer).unwrap();

                // 3. Log it
                // writeln!(log_file, "{:?}", telemetry).unwrap();

                // 4. Send it to the Dashboard
                // let _ = ingest_tx.send(telemetry);
            }
        }
    });

    // ------------------------------------------------------------------
    // 3. THE TRANSMIT TASK (Replaces your serial write task)
    // ------------------------------------------------------------------
    tokio::spawn(async move {
        while let Some(cmd) = command_rx.recv().await {
            println!("RADIO LINK: Sending command '{}' to rocket", cmd);

            // Because SPI is blocking, if you send commands while receiving,
            // you might need a Mutex around the SPI bus, or handle transmission
            // in a dedicated blocking thread that safely shares the radio instance.

            // let payload = cmd.as_bytes();
            // lora.transmit_payload(payload).unwrap();
        }
    });

    // ------------------------------------------------------------------
    // 4. THE WEB SERVER (Stays exactly the same)
    // ------------------------------------------------------------------
    let app = Router::new()
        .nest_service("/", ServeFile::new("html/index.html"))
        .route("/ws", get(ws_handler))
        .route("/mock-ingest", post(mock_ingest))
        .with_state(shared_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Ground Station Server live on port 3000");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
    println!("\nShutting down Ground Station server gracefully...");
}

// ... your mock_ingest function stays the same below ...

///Mock for simulating  data comming from the rocket
async fn mock_ingest(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<data::Telemetry>,
) -> &'static str {
    // JSON data is into the broadcast pipe
    // It doesn't care who is listening; it just broadcasts the signal
    let _ = state.telemetry_tx.send(payload);
    "Data Received"
}

