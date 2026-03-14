use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use eframe::egui::Color32;

use crate::gui::processing::run_processing_loop;
use crate::OxideRuntime;

/// Custom dark theme matching Kali Linux style
fn kali_theme() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = Color32::from_rgb(0x1e, 0x1e, 0x1e);
    visuals
}

pub struct AngryOxideGui {
    oxide_runtime: Arc<Mutex<OxideRuntime>>,
    running: Arc<AtomicBool>,
    processing_thread: Option<std::thread::JoinHandle<()>>,
}

impl AngryOxideGui {
    pub fn new(oxide_runtime: Arc<Mutex<OxideRuntime>>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let oxide_runtime_clone = oxide_runtime.clone();
        let running_clone = running.clone();

        // Spawn the processing thread
        let processing_thread = Some(std::thread::spawn(move || {
            run_processing_loop(oxide_runtime_clone, running_clone);
        }));

        Self {
            oxide_runtime,
            running,
            processing_thread,
        }
    }

    pub fn stop_processing(&mut self) {
        // Signal the processing thread to stop
        self.running.store(false, Ordering::SeqCst);

        // Join the thread if it exists
        if let Some(handle) = self.processing_thread.take() {
            if let Err(e) = handle.join() {
                eprintln!("Error joining processing thread: {:?}", e);
            }
        }
    }
}

impl eframe::App for AngryOxideGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply custom theme
        ctx.set_visuals(kali_theme());

        // Lock the oxide runtime to get a snapshot of the state
        let interface_name;
        let channel;
        let status_messages;
        let mut access_points;
        let mut unassoc_clients;
        let handshake_count;

        {
            let oxide = match self.oxide_runtime.lock() {
                Ok(oxide) => oxide,
                Err(_) => {
                    // If we can't get the lock, show an error message
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.label("Error: Could not access runtime data");
                    });
                    return;
                }
            };

            // Clone the data we need for the UI to avoid holding the lock too long
            access_points = oxide.access_points.clone();
            unassoc_clients = oxide.unassoc_clients.clone();
            handshake_count = oxide.handshake_storage.count();
            interface_name = match String::from_utf8(
                oxide.if_hardware.interface.name.clone().unwrap_or_default(),
            ) {
                Ok(name) => name,
                Err(_) => "Unknown".to_string(),
            };
            channel = oxide.if_hardware.interface.frequency.channel.unwrap_or(0);
            status_messages = oxide.status_log.get_all_messages();
        } // Drop the lock here

        // Main panel
        egui::CentralPanel::default().show(ctx, |ui| {
            // Title and controls
            ui.horizontal(|ui| {
                ui.heading("AngryOxide GUI");

                // Stop button
                if ui.button("⏹ Stop Processing").clicked() {
                    self.stop_processing();
                }

                // Status indicator
                if self.processing_thread.is_some() {
                    ui.label(egui::RichText::new("● Running").color(Color32::GREEN));
                } else {
                    ui.label(egui::RichText::new("● Stopped").color(Color32::RED));
                }
            });

            ui.separator();

            // Interface info
            ui.horizontal(|ui| {
                ui.label("Interface:");
                ui.label(interface_name);
                ui.separator();
                ui.label("Channel:");
                ui.label(format!("{}", channel));
                ui.separator();
            });
            ui.separator();

            // Statistics
            ui.horizontal(|ui| {
                ui.label(format!("APs: {}", access_points.size()));
                ui.label(format!("Clients: {}", unassoc_clients.size()));
                ui.label(format!("Handshakes: {}", handshake_count));
            });
            ui.separator();

            // Recent messages
            ui.label("Recent Messages:");
            egui::ScrollArea::vertical().show(ui, |ui| {
                let messages = status_messages;
                for msg in messages.iter().rev().take(10) {
                    let msg_text = msg.content.to_string();
                    ui.colored_label(
                        match msg.message_type {
                            crate::status::MessageType::Error => egui::Color32::RED,
                            crate::status::MessageType::Warning => egui::Color32::YELLOW,
                            crate::status::MessageType::Info => egui::Color32::LIGHT_BLUE,
                            crate::status::MessageType::Status => egui::Color32::GREEN,
                            crate::status::MessageType::Priority => egui::Color32::LIGHT_GREEN,
                        },
                        msg_text,
                    );
                }
            });

            ui.separator();

            // Show some APs (limited to 10 for brevity)
            ui.label("Access Points (showing up to 10):");
            egui::Grid::new("ap_grid")
                .striped(true)
                .num_columns(4)
                .spacing([10.0, 2.0])
                .show(ui, |ui| {
                    ui.label("MAC");
                    ui.label("SSID");
                    ui.label("Chan");
                    ui.label("Power");
                    ui.end_row();

                    let mut count = 0;
                    for ap in access_points.get_devices().values() {
                        if count >= 10 {
                            break;
                        }
                        let channel_str = match &ap.channel {
                            Some((band, chan)) => format!("{} {}", band.to_u8(), *chan),
                            None => "N/A".to_string(),
                        };
                        let power_str = if ap.last_signal_strength.value != 0 {
                            format!("{}", ap.last_signal_strength.value)
                        } else {
                            "N/A".to_string()
                        };
                        ui.label(format!("{}", ap.mac_address));
                        ui.label(format!(
                            "{}",
                            ap.ssid.clone().unwrap_or_else(|| "<hidden>".to_string())
                        ));
                        ui.label(channel_str);
                        ui.label(power_str);
                        ui.end_row();
                        count += 1;
                    }
                });

            ui.separator();

            // Show some clients (limited to 10)
            ui.label("Clients (showing up to 10):");
            egui::Grid::new("client_grid")
                .striped(true)
                .num_columns(4)
                .spacing([10.0, 2.0])
                .show(ui, |ui| {
                    ui.label("MAC");
                    ui.label("Probes");
                    ui.label("Power");
                    ui.label("Last Seen");
                    ui.end_row();

                    let mut count = 0;
                    for client in unassoc_clients.get_devices().values() {
                        if count >= 10 {
                            break;
                        }
                        let probes = client.probes.as_ref().map_or(0, |v| v.len());
                        let power_str = if client.last_signal_strength.value != 0 {
                            format!("{}", client.last_signal_strength.value)
                        } else {
                            "N/A".to_string()
                        };
                        // Simple age calculation
                        let age_secs = ((std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs())
                        .saturating_sub(client.last_recv))
                            as u64;
                        ui.label(format!("{}", client.mac_address));
                        ui.label(format!("{}", probes));
                        ui.label(power_str);
                        ui.label(format!("{}s ago", age_secs));
                        ui.end_row();
                        count += 1;
                    }
                });
        });

        // Request repaint for smooth animations
        ctx.request_repaint_after(Duration::from_millis(16)); // ~60 FPS
    }
}

impl Drop for AngryOxideGui {
    fn drop(&mut self) {
        // Ensure the processing thread is stopped when the GUI is closed
        self.stop_processing();
    }
}
