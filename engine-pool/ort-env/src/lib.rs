//! Process-wide ONNX Runtime environment shared by every session builder.
//!
//! `ort` 2.0.0-rc.14+ allows only one [`Environment`] per process and requires
//! passing it to every `Session::builder` call. Keeping the singleton here (and
//! out of the individual engine crates) means every engine builds its sessions
//! under the same environment.

use std::sync::{Mutex, OnceLock};

use ort::environment::Environment;

static ENVIRONMENT: OnceLock<Environment> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Returns the process-wide ONNX Runtime environment, creating it on first use.
pub fn environment() -> Result<&'static Environment, ort::Error> {
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment);
    }

    let _guard = INIT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment);
    }

    let environment = ort::init().build()?;
    let _ = ENVIRONMENT.set(environment);
    Ok(ENVIRONMENT
        .get()
        .expect("ONNX Runtime environment set under lock"))
}
