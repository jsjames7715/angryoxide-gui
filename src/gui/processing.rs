use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::process_frame;
use crate::read_frame;
use crate::OxideRuntime;
use nl80211_ng::{get_interface_info_idx, set_interface_chan};

pub fn run_processing_loop(oxide_runtime: Arc<Mutex<OxideRuntime>>, running: Arc<AtomicBool>) {
    println!("[GUI Processing Thread] Started.");

    let mut last_hop_time = Instant::now();
    let mut _hop_cycle: u32 = 0;
    let mut first_channel = (0u8, 0u32);
    // Setup hopper from initial hop channels
    let channels_binding: Vec<(u8, u32)> = {
        let oxide = oxide_runtime.lock().expect("Runtime lock poisoned (init)");
        oxide.if_hardware.hop_channels.clone()
    };
    let mut cycle_iter = channels_binding.iter().cycle();
    if let Some(&(band, channel)) = cycle_iter.next() {
        first_channel = (band, channel);
    }

    while running.load(Ordering::SeqCst) {
        // Lock and operate
        let mut oxide = match oxide_runtime.lock() {
            Ok(o) => o,
            Err(poisoned) => {
                eprintln!(
                    "[GUI Processing Thread] Runtime lock poisoned: {}",
                    poisoned
                );
                break;
            }
        };

        // Update interface
        let idx = oxide.if_hardware.interface.index.unwrap();
        oxide.if_hardware.interface = match get_interface_info_idx(idx) {
            Ok(i) => i,
            Err(e) => {
                oxide
                    .status_log
                    .add_message(crate::status::StatusMessage::new(
                        crate::status::MessageType::Error,
                        format!("Error getting interface info: {}", e),
                    ));
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };

        // Channel hopping
        if last_hop_time.elapsed() >= oxide.if_hardware.hop_interval {
            if let Some(&(band, channel)) = cycle_iter.next() {
                if (band, channel) == first_channel {
                    _hop_cycle += 1;
                }
                if let Err(e) = set_interface_chan(idx, channel, band) {
                    oxide
                        .status_log
                        .add_message(crate::status::StatusMessage::new(
                            crate::status::MessageType::Error,
                            format!("Channel Switch Error: {:?}", e),
                        ));
                }
                oxide.if_hardware.current_channel = channel;
                last_hop_time = Instant::now();
            }
        }

        // Read frame (non-blocking)
        match read_frame(&mut oxide) {
            Ok(packet) => {
                if !packet.is_empty() {
                    if let Err(e) = process_frame(&mut oxide, &packet) {
                        oxide
                            .status_log
                            .add_message(crate::status::StatusMessage::new(
                                crate::status::MessageType::Error,
                                format!("Frame processing error: {}", e),
                            ));
                    }
                }
            }
            Err(code) => {
                if code.kind().to_string() == "network down" {
                    oxide.if_hardware.netlink.set_interface_up(idx).ok();
                } else {
                    oxide
                        .status_log
                        .add_message(crate::status::StatusMessage::new(
                            crate::status::MessageType::Error,
                            format!("Error reading frame: {}", code.kind()),
                        ));
                }
            }
        }

        // Cleanup old devices
        if oxide.config.timeout > 0 {
            let timeout = oxide.config.timeout;
            oxide.access_points.remove_old_devices(timeout);
            oxide.unassoc_clients.remove_old_devices(timeout);
        }

        std::thread::sleep(Duration::from_micros(1));
    }

    println!("[GUI Processing Thread] Stopped.");
}
