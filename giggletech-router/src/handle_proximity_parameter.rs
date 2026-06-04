/*
    handle_proximity_parameter.rs - Handling Proximity Data for GiggleTech Devices

    This module processes proximity sensor data and controls device actions (like motors) based on 
    the proximity values. It tracks the last proximity signal for each device and manages sending 
    commands to the device via OSC.

    **Key Features:**

    1. **Proximity Handling (`handle_proximity_parameter`)**:
       - Receives proximity data (`value`) and determines if the device should stop or continue operating.
       - If proximity is zero, it sends stop commands to the device.
       - If proximity is non-zero, it processes the proximity data and sends motor control values to the device.

    2. **Velocity Control**:
       - If the device uses velocity control, the module calculates the change in proximity over time and adjusts the motor speed accordingly.
       - Otherwise, it simply scales the motor value based on proximity.

    3. **Timeout and Signal Tracking**:
       - Updates the last signal time and last proximity value for each device, ensuring proper handling of timeouts and avoiding stale data.

    **Usage**:
    - This function is typically called when proximity data is received and determines the appropriate action (start, stop, or adjust motor) for the device.
*/

use async_osc::Result;
use async_std::sync::{Arc, Mutex};
use std::{
    sync::atomic::{AtomicBool},
    time::{Instant, Duration}, collections::HashMap,
};


use crate::osc_timeout;
use crate::terminator;
use crate::giggletech_osc;
use crate::data_processing;
use crate::log_ui;
use crate::stop_pats;
use lazy_static::lazy_static;
use crate::config::DeviceConfig;


lazy_static! {
    pub static ref DEVICE_LAST_VALUE: Arc<Mutex<HashMap<String, f32>>> =
        Arc::new(Mutex::new(HashMap::new()));
    pub static ref DEVICE_VELOCITY_EMA: Arc<Mutex<HashMap<String, f32>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub(crate) async fn handle_proximity_parameter(
    running: Arc<AtomicBool>,
    value: f32,
    device: DeviceConfig
) -> Result<()> {
    terminator::stop(running.clone()).await?;

    let device_ip = Arc::new(device.device_uri.clone());

    // Update Last Signal Time for timeout clock 
    let mut device_last_signal_times = osc_timeout::DEVICE_LAST_SIGNAL_TIME.lock().unwrap();
    // let last_signal_time: Option<Instant> = device_last_signal_times.get(&device_ip.to_string()).copied();
    let last_signal_time = device_last_signal_times.insert(device_ip.to_string(), Instant::now());
    let mut device_last_values = DEVICE_LAST_VALUE.lock().await;
    let last_val = device_last_values.insert(device_ip.to_string(), value).unwrap_or(0.0);

    let pat_graph = if value > 0.0 {
        data_processing::proximity_graph(value)
    } else {
        String::new()
    };
    log_ui::notify_pat_bar(device.proximity_parameter.as_str(), &pat_graph);
    log_ui::notify_prox_signal(device.device_uri.as_str(), device.proximity_parameter.as_str(), value);

    if value == 0.0 {
        // Reset smoothing when proximity fully clears.
        DEVICE_VELOCITY_EMA.lock().await.remove(device_ip.as_str());
        if let Ok(json) = serde_json::to_string(&serde_json::json!({
            "pre": 0.0,
            "damped": 0.0,
            "smooth": 0.0,
            "motor": 0.0,
        })) {
            log_ui::notify_headpat_telemetry(device.device_uri.as_str(), device.proximity_parameter.as_str(), &json);
        }
        stop_pats::stop_device_with_terminator(device_ip.as_str(), running.clone()).await?;
    } else {
        if !device.use_velocity_control {
            let headpat_tx = data_processing::process_pat(value, &device, last_val);
            let motor_norm = data_processing::motor_norm_from_tx(headpat_tx, &device);
            if let Ok(json) = serde_json::to_string(&serde_json::json!({
                "pre": 0.0,
                "damped": 0.0,
                "smooth": 0.0,
                "motor": motor_norm,
            })) {
                log_ui::notify_headpat_telemetry(device.device_uri.as_str(), device.proximity_parameter.as_str(), &json);
            }
            if headpat_tx == 0 {
                stop_pats::stop_device_immediate(device_ip.as_str(), running.clone()).await?;
            } else {
                giggletech_osc::send_data(&device_ip, headpat_tx).await?;
            }
        } else {
            let delta_t = match last_signal_time {
                None => Duration::new(0, 0),
                Some(t_prev) => Instant::now().duration_since(t_prev),
            };

            // Simple smoothing: EMA on computed velocity (per device).
            // Higher tau => smoother but more latency.
            let vel_smooth_tau_secs = (device.velocity_smoothing_ms as f32) / 1000.0;
            let breakdown =
                data_processing::compute_proximity_velocity_breakdown(value, last_val, delta_t, &device);
            let damped_vel = breakdown.post_softcap;
            let dt = delta_t.as_secs_f32();
            let alpha = if dt <= 0.0 || vel_smooth_tau_secs <= 0.0 {
                1.0
            } else {
                1.0 - (-dt / vel_smooth_tau_secs).exp()
            }
            .clamp(0.0, 1.0);

            let mut ema_map = DEVICE_VELOCITY_EMA.lock().await;
            let prev_ema = *ema_map.get(device_ip.as_str()).unwrap_or(&0.0);
            let ema_vel = prev_ema + alpha * (damped_vel - prev_ema);
            if ema_vel <= 0.0 {
                ema_map.remove(device_ip.as_str());
            } else {
                ema_map.insert(device_ip.to_string(), ema_vel);
            }
            drop(ema_map);

            let headpat_tx = data_processing::motor_tx_from_velocity(ema_vel, &device);
            let motor_norm = data_processing::motor_norm_from_tx(headpat_tx, &device);
            if let Ok(json) = serde_json::to_string(&serde_json::json!({
                "pre": breakdown.pre_softcap,
                "damped": damped_vel,
                "smooth": ema_vel,
                "motor": motor_norm,
            })) {
                log_ui::notify_headpat_telemetry(device.device_uri.as_str(), device.proximity_parameter.as_str(), &json);
            }
            if headpat_tx == 0 {
                // Proximity still non-zero but no velocity pulse — latch motor off (single 0 is often not enough).
                DEVICE_VELOCITY_EMA.lock().await.remove(device_ip.as_str());
                stop_pats::stop_device_immediate(device_ip.as_str(), running.clone()).await?;
            } else {
                giggletech_osc::send_data(&device_ip, headpat_tx).await?;
            }
        }
    }
    Ok(())
}
