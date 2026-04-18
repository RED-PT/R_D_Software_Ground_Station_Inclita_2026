use std::time::Duration;
use tokio::sync::broadcast::Sender;
use crate::telemetry::data::Telemetry; // Updated import path

pub fn spawn_mock_telemetry_task(tx: Sender<Telemetry>) {
    tokio::spawn(async move {
        println!("Starting Mock Telemetry Generator at 5Hz...");
        
        let mut current_pressure: u16 = 1013; 
        
        loop {
            // Use the new struct fields, and default the rest to 0
            let fake_data = Telemetry {
                pressure: current_pressure,
                yaw: 15.0, // Throw in some fake flight data
                pitch: 45.0,
                ..Default::default() 
            };

            let _ = tx.send(fake_data);
            
            // Simulate pressure dropping as the rocket goes up
            current_pressure = current_pressure.saturating_sub(1); 
            
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
}
