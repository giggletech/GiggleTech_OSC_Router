use async_std::task::sleep;
use async_std::sync::Mutex;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub struct PneumaticSystem {
    tfull: f64,  // Time to fully fill the bladder (seconds)
    tempty: f64, // Time to fully empty the bladder (seconds)
    bladder_level: Arc<Mutex<f64>>, // Current bladder fullness (0.0 - 1.0)
}

impl PneumaticSystem {
    pub fn new(tfull: f64, tempty: f64) -> Self {
        Self {
            tfull,
            tempty,
            bladder_level: Arc::new(Mutex::new(0.0)),
        }
    }

    pub async fn set_bladder_level(&self, par_belly: f64) {
        let mut bladder = self.bladder_level.lock().await;
        let current_level = *bladder;
        
        if par_belly > current_level {
            let fill_time = (par_belly - current_level) * self.tfull;
            println!("Filling bladder: Target = {:.2}, Current = {:.2}, Time = {:.2}s", par_belly, current_level, fill_time);
            sleep(Duration::from_secs_f64(fill_time)).await;
        } else if par_belly < current_level {
            let empty_time = (current_level - par_belly) * self.tempty;
            println!("Emptying bladder: Target = {:.2}, Current = {:.2}, Time = {:.2}s", par_belly, current_level, empty_time);
            sleep(Duration::from_secs_f64(empty_time)).await;
        }
        
        *bladder = par_belly;
        println!("Bladder adjusted to: {:.2}", *bladder);
    }

    pub async fn get_bladder_level(&self) -> f64 {
        *self.bladder_level.lock().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_std::task;

    #[test]
    fn test_pneumatic_system() {
        task::block_on(async {
            let pneumatic = PneumaticSystem::new(10.0, 5.0);
            pneumatic.set_bladder_level(0.5).await;
            assert_eq!(pneumatic.get_bladder_level().await, 0.5);
        });
    }
}
