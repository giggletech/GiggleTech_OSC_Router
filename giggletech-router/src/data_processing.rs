// data_processing.rs

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