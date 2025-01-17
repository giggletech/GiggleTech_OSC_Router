use async_std::task::sleep;
use async_std::sync::Mutex;
use std::sync::{Arc, atomic::{AtomicBool}};
use std::time::{Duration, Instant};
use std::collections::HashMap;
use lazy_static::lazy_static;
use crate::osc_timeout;
use crate::terminator;
use crate::giggletech_osc;
use crate::config::DeviceConfig;

lazy_static! {
    pub static ref DEVICE_LAST_VALUE: Arc<Mutex<HashMap<String, f32>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub(crate) async fn pool_toy_logic(
    running: Arc<AtomicBool>,
    value: f32,
    device: DeviceConfig,
) -> Result<(), async_osc::Error> {
    terminator::stop(running.clone()).await?;

    let device_ip = Arc::new(device.device_uri.clone());
    let mut device_last_signal_times = osc_timeout::DEVICE_LAST_SIGNAL_TIME.lock().unwrap();
    let mut device_last_values = DEVICE_LAST_VALUE.lock().await;
    
    let bladder_level = Arc::new(Mutex::new(0.0));
    let tfull = 10.0;
    let tempty = 5.0;
    let step_duration = Duration::from_millis(100);
    
    if value == 0.0 {
        println!("Stopping pats...");
        terminator::start(running.clone(), &device_ip).await?;

        for _ in 0..5 {
            giggletech_osc::send_data(&device_ip, 0i32).await?;  
        }
    } else {
        let signal_out = (value.clamp(0.0, 1.0) * 255.0).round() as i32;
        println!("{}", signal_out);
        giggletech_osc::send_data(&device_ip, signal_out).await?;
        
        // Adjust bladder level dynamically without blocking
        let bladder_level_clone = Arc::clone(&bladder_level);
        let par_belly = value as f64;
        async_std::task::spawn(async move {
            let mut bladder = bladder_level_clone.lock().await;
            while (par_belly - *bladder).abs() > 0.01 {
                let change_rate = if par_belly > *bladder {
                    1.0 / tfull
                } else {
                    -1.0 / tempty
                };
                
                let step_change = change_rate * step_duration.as_secs_f64();
                *bladder = (*bladder + step_change).clamp(0.0, 1.0);
                println!("Bladder adjusting: Target = {:.2}, Current = {:.2}", par_belly, *bladder);
                sleep(step_duration).await;
            }
            println!("Bladder adjusted to: {:.2}", *bladder);
        });
    }
    Ok(())
}