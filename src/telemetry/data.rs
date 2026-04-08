use serde::{Deserialize, Serialize};

///Struct of data to be received from the rocket
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Telemetry {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub temperature: u16,
    pub pressure: u16,
    pub accel_z: f32,
    pub gyro_x: f32,
    pub gyro_y: f32,
    pub gyro_z: f32,
    pub quat_x: f32,
    pub quat_y: f32,
    pub quat_z: f32,
    pub quat_s: f32,
    pub lat: f32,
    pub lon: f32,
    pub state: u8,
}

///Enum of the CMDS for future use, possible commands to be sent from the gs
//TODO
#[derive(Deserialize, Debug, Serialize, Clone)]
pub enum Cmds {
    ABORT,
    ARM,
    PING,
    LAUCH,
}
