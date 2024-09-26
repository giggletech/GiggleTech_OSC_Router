/*
    data_processing.rs - Processing Sensor Data for Giggletech Devices

    This module is responsible for processing proximity signals from devices, generating visual 
    representations of proximity data, and calculating motor transmission (Tx) values for headpats
    based on proximity. It handles both basic and advanced headpat processing, adjusting motor speed
    based on proximity and velocity signals. 

    **Key Features:**

    1. **Proximity Graph (`proximity_graph`)**:
       - Converts a proximity signal into a simple string-based graph. The closer the proximity, 
         the longer the "dash" graph, which visually represents the proximity level.
       - Returns a string like "----->" to indicate proximity strength.

    2. **Speed Limit Printer (`print_speed_limit`)**:
       - Displays the current headpat maximum speed percentage along with an indicator of the level 
         (e.g., "!!! SO MUCH !!!" for high speeds).
       - Helps visualize the intensity of the motor speed.

    3. **Pat Processor (`process_pat`)**:
       - Processes the proximity signal and calculates the motor transmission (Tx) value. This value 
         is scaled by the configured device speed scale and the constant motor scaling factor.
       - Ensures the motor starts with enough power if transitioning from an idle state.
       - Logs the proximity value and motor transmission for debugging.

    4. **Advanced Pat Processor (`process_pat_advanced`)**:
       - A more advanced version of the `process_pat` function, taking into account the velocity of the 
         proximity signal change over time (`delta_t`) to calculate a velocity-based motor transmission.
       - Used for finer control over motor behavior based on how fast the proximity signal changes 
         (e.g., if a headpat is being applied quickly or slowly).
       - Logs proximity, velocity, and motor transmission for debugging and visualization.

    **Motor Speed Scaling**:
    - The constant `MOTOR_SPEED_SCALE` (0.66) is used to scale the motor speed transmission. Going higher 
      than this value may reduce the life of the motor, as it's designed for over-voltage control.

    **Usage**:
    - The module processes proximity signals in real-time, calculating motor values that are then used 
      to control vibrational feedback devices in VRChat.
    - Both basic and advanced pat processing functions are available, depending on the complexity of the 
      behavior needed.

    **Example Functionality**:
    - `process_pat`: Basic proximity-based motor control.
    - `process_pat_advanced`: Velocity-sensitive motor control based on proximity changes.

    **Logging and Debugging**:
    - Each function logs proximity values, motor Tx values, and velocity (for advanced processing) to help 
      visualize and debug motor behavior in real-time.
*/


use std::time::Duration;

use crate::config::DeviceConfig;


pub fn proximity_graph(proximity_signal: f32) -> String {
    let num_dashes = (proximity_signal * 10.0) as usize;
    let graph = "-".repeat(num_dashes) + ">";

    graph
}

pub fn print_speed_limit(headpat_max_rx: f32) {
    let headpat_max_rx_print = (headpat_max_rx * 100.0).round() as i32;
    let max_meter = match headpat_max_rx_print {
        91..=i32::MAX => "!!! SO MUCH !!!",
        76..=90 => "!! ",
        51..=75 => "!  ",
        _ => "   ",
    };
    println!("Speed Limit: {}% {}", headpat_max_rx_print, max_meter);
}

// Pat Processor
const MOTOR_SPEED_SCALE: f32 = 0.66; // Overvolt   Here, OEM config 0.66 going higher than this value will reduce your vibrator motor life

pub fn process_pat(proximity_signal: f32, device: &DeviceConfig, prev_signal: f32) -> i32 {
    let graph_str = proximity_graph(proximity_signal);
    let headpat_tx = (((device.max_speed - device.min_speed) * proximity_signal + device.min_speed) * MOTOR_SPEED_SCALE * device.speed_scale * 255.0).round() as i32;
    let headpat_tx = if prev_signal == 0.0 && proximity_signal > 0.0 && headpat_tx < device.start_tx {
        device.start_tx
    } else {
        headpat_tx
    };

    let proximity_signal = format!("{:.2}", proximity_signal);
    eprintln!("{} Prox: {:5} Motor Tx: {:3} |{:11}|", device.proximity_parameter.trim_start_matches("/avatar/parameters/") , proximity_signal, headpat_tx, graph_str);

    headpat_tx
}

pub fn process_pat_advanced(proximity_signal: f32, prev_signal: f32, delta_t: Duration, device: &DeviceConfig) -> i32 {
    let graph_str = proximity_graph(proximity_signal);
    let mut headpat_tx: i32 = 0;
    let mut vel: f32 = 0.0;
    if proximity_signal > device.outer_proximity && proximity_signal < device.inner_proximity && prev_signal > 0.0 && proximity_signal > prev_signal {
        vel = f32::max(0.0, (proximity_signal - prev_signal) / delta_t.as_secs_f32() * device.velocity_scalar);
        headpat_tx = (((device.max_speed - device.min_speed) * vel * device.min_speed) * MOTOR_SPEED_SCALE * device.speed_scale * 255.0).round() as i32;
    }
    eprintln!("{} Prox: {:5} Vel: {:5} Motor Tx: {:3} |{:11}|", device.proximity_parameter.trim_start_matches("/avatar/parameters/") , format!("{:.2}", proximity_signal), format!("{:.2}", vel), headpat_tx, graph_str);

    return headpat_tx;
}