
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

pub fn process_pat(proximity_signal: f32, max_speed: f32, min_speed: f32, speed_scale: f32, proximity_parameter: &String) -> i32 {
    let graph_str = proximity_graph(proximity_signal);
    let headpat_tx = (((max_speed - min_speed) * proximity_signal + min_speed) * MOTOR_SPEED_SCALE * speed_scale * 255.0).round() as i32;
    let proximity_signal = format!("{:.2}", proximity_signal);
    eprintln!("{} Prox: {:5} Motor Tx: {:3} |{:11}|", proximity_parameter.trim_start_matches("/avatar/parameters/") , proximity_signal, headpat_tx, graph_str);

    headpat_tx
}


