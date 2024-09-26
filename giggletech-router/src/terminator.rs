/*
    terminator.rs - Control Start/Stop Worker for Device Shutdown

    This module is responsible for managing a worker that sends stop signals (`0`) to devices in regular intervals 
    when certain conditions are met (such as proximity signals stopping). It can start or stop the worker as needed.

    **Key Features:**

    1. **Start Worker (`start`)**:
       - Spawns a worker task that continuously sends a stop signal (`0`) to a device every second.
       - Ensures the worker is not started if it’s already running by checking the `AtomicBool`.

    2. **Stop Worker (`stop`)**:
       - Stops the worker by setting the `AtomicBool` to `false`, halting the periodic stop signal transmission.

    3. **Worker Task**:
       - The worker function runs in a loop, sending stop signals to the device while the worker is active.
       - It sleeps for 1 second between each stop signal, ensuring the device continues to receive stop commands 
         until the worker is stopped.

    **Usage**:
    - This module is used to continuously send stop signals to devices when needed, for instance, when a device 
      should halt due to inactivity or the end of a proximity event.
    - The worker can be started or stopped based on the system state, using the `start` and `stop` functions.

*/



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

async fn worker(running: Arc<AtomicBool>, device_ip: Arc<String>) -> Result<()> {
    while running.load(Ordering::Relaxed) {
        //println!("Worker is running");
        giggletech_osc::send_data(&device_ip, 0i32).await?;
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