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

pub fn print_speed_limit(_headpat_max_rx: f32) {
    // Speed limit is applied in the router; live feedback uses motor bars only.
}

// Pat Processor
const MOTOR_SPEED_SCALE: f32 = 0.66; // Overvolt   Here, OEM config 0.66 going higher than this value will reduce your vibrator motor life

/// Proximity-mode band: motor off only below the far edge.
/// At/above the far edge counts as in-band (and clamps to 100% past the close edge).
pub fn proximity_in_band(proximity_signal: f32, device: &DeviceConfig) -> bool {
    proximity_signal >= device.outer_proximity
}

/// Velocity-mode band: require proximity to stay between far and close edges.
/// This prevents "stuck on" behavior when the sensor saturates at very close range.
pub fn proximity_in_band_velocity(proximity_signal: f32, device: &DeviceConfig) -> bool {
    proximity_signal >= device.outer_proximity && proximity_signal <= device.inner_proximity
}

/// Far edge → 0%, close edge → 100%, closer than close edge stays at 100%.
pub fn proximity_normalized_in_band(proximity_signal: f32, device: &DeviceConfig) -> f32 {
    let span = device.inner_proximity - device.outer_proximity;
    if span <= 0.0 {
        return 0.0;
    }
    let t = (proximity_signal - device.outer_proximity) / span;
    t.clamp(0.0, 1.0)
}

pub fn process_pat(proximity_signal: f32, device: &DeviceConfig, prev_signal: f32) -> i32 {
    if !proximity_in_band(proximity_signal, device) {
        return 0;
    }
    // 0% at far edge of band, 100% at close edge (scaled by Power / max_speed).
    let proximity_signal = proximity_normalized_in_band(proximity_signal, device);
    let headpat_tx = (device.max_speed * proximity_signal * MOTOR_SPEED_SCALE * device.speed_scale * 255.0)
        .round() as i32;
    let headpat_tx = if prev_signal == 0.0 && proximity_signal > 0.0 && headpat_tx < device.start_tx {
        device.start_tx
    } else {
        headpat_tx
    };

    headpat_tx
}

/// Minimum time between OSC samples when computing velocity (avoids divide-by-zero only).
const MIN_VELOCITY_DELTA_SECS: f32 = 0.001;

/// Ignore proximity jitter below this — treats "held still" as zero velocity.
/// Without this, `velocity_on_prox_drop` and float noise keep firing the motor.
const PROX_VELOCITY_DEADZONE: f32 = 0.002;

/// Compute the raw velocity (positive for approach). Returns 0 when inactive/invalid.
pub fn compute_proximity_velocity(
    proximity_signal: f32,
    prev_signal: f32,
    delta_t: Duration,
    device: &DeviceConfig,
) -> f32 {
    if !proximity_in_band_velocity(proximity_signal, device) || prev_signal <= 0.0 {
        return 0.0;
    }

    let delta = proximity_signal - prev_signal;
    if delta.abs() < PROX_VELOCITY_DEADZONE {
        return 0.0;
    }

    let active = if device.velocity_on_prox_drop { true } else { delta > 0.0 };
    if !active {
        return 0.0;
    }

    let delta_secs = delta_t.as_secs_f32();
    if delta_secs <= 0.0 {
        return 0.0;
    }
    let delta_secs = delta_secs.max(MIN_VELOCITY_DELTA_SECS);

    let speed = if device.velocity_on_prox_drop { delta.abs() } else { delta };
    f32::max(0.0, speed / delta_secs * device.velocity_scalar)
}

/// Convert a (possibly smoothed) velocity value to a motor tx value.
pub fn motor_tx_from_velocity(vel: f32, device: &DeviceConfig) -> i32 {
    if vel <= 0.0 {
        return 0;
    }
    let headpat_tx = (((device.max_speed - device.min_speed) * vel * device.min_speed)
        * MOTOR_SPEED_SCALE
        * device.speed_scale
        * 255.0)
        .round() as i32;
    let max_tx = (((device.max_speed - device.min_speed) + device.min_speed)
        * MOTOR_SPEED_SCALE
        * device.speed_scale
        * 255.0)
        .round() as i32;
    headpat_tx.min(max_tx)
}

pub fn process_pat_advanced(proximity_signal: f32, prev_signal: f32, delta_t: Duration, device: &DeviceConfig) -> i32 {
    let vel = compute_proximity_velocity(proximity_signal, prev_signal, delta_t, device);
    motor_tx_from_velocity(vel, device)
}
