//! One tray instance per user session; a second launch signals the first to show its window.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::thread;
use std::time::Duration;

use winapi::shared::minwindef::{FALSE, TRUE};
use winapi::shared::winerror::ERROR_ALREADY_EXISTS;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::{
  CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject,
};
use winapi::um::winnt::HANDLE;

const MUTEX_NAME: &str = r"Local\GiggleTechOSCRouter.SingleInstance";
const SHOW_OUTPUT_EVENT_NAME: &str = r"Local\GiggleTechOSCRouter.ShowOutput";

fn to_wide(s: &str) -> Vec<u16> {
  OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

/// Hold for the process lifetime so the named mutex stays acquired.
pub struct PrimaryInstance {
  _mutex: HANDLE,
  show_event: HANDLE,
}

impl PrimaryInstance {
  /// Returns `None` if another instance already owns the mutex.
  pub fn acquire() -> Option<Self> {
    unsafe {
      let name = to_wide(MUTEX_NAME);
      let mutex = CreateMutexW(ptr::null_mut(), TRUE, name.as_ptr());
      if mutex.is_null() {
        return None;
      }

      let already_running = GetLastError() == ERROR_ALREADY_EXISTS;
      if already_running {
        CloseHandle(mutex);
        return None;
      }

      let show_event = create_show_event()?;
      Some(Self {
        _mutex: mutex,
        show_event,
      })
    }
  }

  /// Background thread: each second launch pulses `show_event` → `on_show` on the UI thread.
  pub fn spawn_show_listener<F>(&self, on_show: F)
  where
    F: Fn() + Send + 'static,
  {
    let event = self.show_event as usize;
    thread::spawn(move || loop {
      unsafe {
        WaitForSingleObject(event as HANDLE, winapi::um::winbase::INFINITE);
      }
      on_show();
    });
  }
}

impl Drop for PrimaryInstance {
  fn drop(&mut self) {
    unsafe {
      if !self.show_event.is_null() {
        CloseHandle(self.show_event);
      }
      if !self._mutex.is_null() {
        CloseHandle(self._mutex);
      }
    }
  }
}

/// Called by a second process before exit: wake the running instance to show its window.
pub fn request_show_output_from_running_instance() {
  const RETRIES: u32 = 20;
  const RETRY_DELAY: Duration = Duration::from_millis(50);

  for attempt in 0..RETRIES {
    unsafe {
      let name = to_wide(SHOW_OUTPUT_EVENT_NAME);
      let event = OpenEventW(winapi::um::winnt::EVENT_MODIFY_STATE, FALSE, name.as_ptr());
      if !event.is_null() {
        SetEvent(event);
        CloseHandle(event);
        return;
      }
    }
    if attempt + 1 < RETRIES {
      thread::sleep(RETRY_DELAY);
    }
  }
}

unsafe fn create_show_event() -> Option<HANDLE> {
  let name = to_wide(SHOW_OUTPUT_EVENT_NAME);
  let event = CreateEventW(ptr::null_mut(), FALSE, FALSE, name.as_ptr());
  if event.is_null() {
    None
  } else {
    Some(event)
  }
}
