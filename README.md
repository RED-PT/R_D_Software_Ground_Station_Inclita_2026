# INCLITA Ground Station (2026)

**RED-PT R&D 202&**

The Ground Station for the Inclita Mission. Built in Rust, this Ground Station acts as the bridge between the rocket's LoRa radio downlink and the mission control. It features a lightweight asynchronous backend and a real-time WebSocket dashboard.

## Features

* **Real-Time Telemetry:** Streams IMU orientation, environmental data, and GPS tracking from the rocket to the browser.
* **Bi-Directional Uplink:** Send commands (ARM, ABORT, PING) from the dashboard directly to the rocket over the LoRa hardware interface.
* **Hardware & Mock Timelines:** Run the server for UI development using the `mock` feature, or deploy it to the Raspberry Pi for live SPI hardware communication.
* **Catppuccin UI:** Its cute

## Technology Stack

* **Backend:** Rust, Tokio (Async runtime), Axum (Web server)
* **Real-Time Link:** WebSockets (`axum::extract::ws`)
* **Hardware Interface:** `linux-embedded-hal`, `sx127x_lora` (SPI communication)
* **Frontend:** Vanilla HTML/CSS/JS (Catppuccin Mocha Theme)

---

## Architecture Overview (as of now)

```
── handlers
│   ├── mod.rs
│   └── sockets_handler.rs
├── hardware_cfg.rs
├── main.rs
├── mock.rs
└── telemetry
    ├── data.rs
    └── mod.rs

```

The system is designed with concurrent Tokio tasks communicating via channels:

1. **LoRa Receiver Task:** Listens to the SPI hardware for incoming packets, deserializes them into `Telemetry` structs, and pushes them to a broadcast channel.
2. **WebSocket Handler:** Subscribes to the telemetry broadcast and instantly forwards the JSON data to all connected web browsers.
3. **LoRa Transmitter Task:** Listens to a Multi-Producer Single-Consumer (MPSC) channel for command strings coming from the web UI and transmits them over the radio.

---

## Getting Started

### Prerequisites

* [Rust & Cargo](https://rustup.rs/) installed.
* If deploying to hardware: A Raspberry Pi with SPI enabled (`raspi-config -> Interfacing Options -> SPI`).

### 1. Mock Mode (Development & UI Testing)

You can run the entire Ground Station on the RaspberryMac, Windows, or Linux machine without needing the physical radio hardware. This mode generates fake telemetry data to test the UI.

```bash
git clone [https://github.com/RED-PT/R_D_Software_Ground_Station_Inclita_2026.git](https://github.com/RED-PT/R_D_Software_Ground_Station_Inclita_2026.git)
cd R_D_Software_Ground_Station_Inclita_2026

# Run with the mock feature flag enabled

cargo run --features mock
```

### 2. Hardware Mode (Live Deployment)

When deploying to the Raspberry Pi at the launch site, run the standard build. This will bind to /dev/spidev0.0 and communicate with the SX127x LoRa module.

```
cargo run
```

### 3. Accessing the Dashboard

Once the server is running, open your web browser (Safari or Incognito recommended to bypass local network HTTPS overrides, I have skill issue) and navigate to:

Local Machine: <http://127.0.0.1:3000>

Across Network (e.g., Pi to Mac): http://<RASPBERRY_PI_IP>:3000

### Packet Structure (as of now)

Data is serialized/deserialized over the radio using the following JSON structure (defined in `src/telemetry/data.rs`):

```JSON
{
  "yaw": 0.0,
  "pitch": 0.0,
  "roll": 0.0,
  "temperature": 25,
  "pressure": 1013,
  "accel_z": 9.81,
  "gyro_x": 0.0,
  "gyro_y": 0.0,
  "gyro_z": 0.0,
  "quat_x": 0.0,
  "quat_y": 0.0,
  "quat_z": 0.0,
  "quat_s": 1.0,
  "lat": 38.9360,
  "lon": -9.3361,
  "state": 1
}
```
