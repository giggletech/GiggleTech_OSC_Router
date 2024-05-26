// terminator.rs


use async_osc::{Result};
use async_std::{task::{self},sync::Arc,};
use std::{ time::{Duration, }};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::giggletech_osc;


 pub(crate) async fn start(running: Arc<AtomicBool>, device_ip: &Arc<String>) -> Result<()> {
    if running.load(Ordering::SeqCst) {
        //return Err("Worker is already running".into());
    }
    let worker_running = running.clone();
    let worker_device_ip = device_ip.clone();
    task::spawn(async move {
        worker(worker_running, worker_device_ip).await.unwrap();
    });
    running.store(true, Ordering::SeqCst);
    Ok(())
}

const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor"; 
const TX_OSC_GIGGLESPARK: &str = "/motor"; 
const TX_OSC_COLLAR_1: &str = "/motor";
const TX_OSC_COLLAR_2: &str = "/motor_02";
const TX_OSC_COLLAR_3: &str = "/motor_03";
const TX_OSC_COLLAR_4: &str = "/motor_04";
async fn worker(running: Arc<AtomicBool>, device_ip: Arc<String>) -> Result<()> {
    while running.load(Ordering::Relaxed) {
        //println!("Worker is running");
        giggletech_osc::send_data(&device_ip, TX_OSC_MOTOR_ADDRESS, 0i32).await?;
        giggletech_osc::send_data(&device_ip, TX_OSC_GIGGLESPARK, 0i32).await?;
        giggletech_osc::send_data(&device_ip, TX_OSC_COLLAR_1, 0i32).await?;
        giggletech_osc::send_data(&device_ip, TX_OSC_COLLAR_2, 0i32).await?;
        giggletech_osc::send_data(&device_ip, TX_OSC_COLLAR_3, 0i32).await?;
        giggletech_osc::send_data(&device_ip, TX_OSC_COLLAR_4, 0i32).await?;
        // giggletech_osc::send_data(&device_ip, 0i32).await?;
        task::sleep(Duration::from_secs(1)).await;
    }
    //println!("Worker stopped");
    Ok(())
}

pub(crate) async fn stop(running: Arc<AtomicBool>) -> Result<()> {
    if !running.load(Ordering::SeqCst) {
        //return Err("Worker is not running".into());
    }
    running.store(false, Ordering::SeqCst);
    Ok(())
}