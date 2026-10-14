//! Native System Power Management (Reboot & Shutdown)

use std::process::Command;

pub struct PowerManager;

impl PowerManager {
    pub fn reboot() -> Result<(), String> {
        log::info!("Executing system reboot command...");
        let status = Command::new("systemctl")
            .arg("reboot")
            .status()
            .map_err(|e| format!("Failed to invoke systemctl reboot: {}", e))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("systemctl reboot exited with status {}", status))
        }
    }

    pub fn shutdown() -> Result<(), String> {
        log::info!("Executing system poweroff command...");
        let status = Command::new("systemctl")
            .arg("poweroff")
            .status()
            .map_err(|e| format!("Failed to invoke systemctl poweroff: {}", e))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("systemctl poweroff exited with status {}", status))
        }
    }
}
