#![windows_subsystem = "windows"] // Hide console window on Windows
#![deny(arithmetic_overflow)]
use chrono::prelude::*; // Brings DateTime, Utc, etc. into scope
use chrono::Timelike; // Brings `.minute()`, `.hour()`, `.second()` into scope
use chrono::Utc;
use eframe::egui::{
    self, Align, Color32, Frame, Label, Layout, RichText, Rounding, ScrollArea, Stroke, Vec2,
    ViewportBuilder,
};
use jsonwebtoken::{encode, EncodingKey, Header};
use lettre::{
    message::header::ContentType as LettreContentType, // Renamed to avoid conflict
    transport::smtp::authentication::Credentials,
    Message, SmtpTransport, Transport,
    transport::smtp::client::{Tls, TlsParameters},
};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::blocking::multipart;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{create_dir_all, read_to_string, remove_file, write, File};
use std::io::copy;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use toml::Value as TomlValue;
use url::Url;

mod default_config;

#[derive(Default, Deserialize)]
struct UrlEntry {
    description: String,
    url: String,
    #[serde(skip)]
    is_ok: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct LogEntry {
    filename: String,
    timestamp: String,
    size: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct InternalLogEntry {
    message: String,
    timestamp: String,
}

#[derive(Deserialize, Serialize)]
struct InternalLog {
    entries: Vec<InternalLogEntry>,
}

#[derive(Deserialize, Serialize)]
struct Log {
    entries: Vec<LogEntry>,
}

#[derive(Clone, Deserialize)]
pub struct SmtpConfig {
    pub server: String,
    pub port: u16, // 0-65535
    pub username: String,
    pub password: String,
    pub from: String,
}

#[derive(Default, Deserialize, Serialize, Clone)]
struct BackupEntry {
    description: String,
    url: String,
    restore: String,
    max: u32,
    interval: String,
    time: u32,
    #[serde(skip)] // <-- Important
    #[serde(default)]
    logs: Vec<LogEntry>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct WarningSettings {
    use_email: bool,
    send_post_request: bool,
    post_request_routes: Vec<String>,
    email: String,
    daily_max: u32,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct UptimeUrlSettings {
    interval_minutes: u32,
    downtime_tolerance: u32,
}

struct StatusChecker {
    uptime_url_settings: UptimeUrlSettings,
    uptime_fails: u32,
    internal_log: Vec<InternalLogEntry>,
    warning_settings: WarningSettings,
    uptime_urls: Vec<UrlEntry>,
    backups: Vec<BackupEntry>,
    secret: String,
    token: String,
    jwt_expiry: u64,
    payload: HashMap<String, TomlValue>,
    backup_enabled: bool,
    backup_trigger_rx: Receiver<()>,
    smtp_config: SmtpConfig,
    warnings_sent: u32,
}

impl Default for StatusChecker {
    fn default() -> Self {
        let (_tx, rx) = std::sync::mpsc::channel();
        Self {
            uptime_url_settings: UptimeUrlSettings {
                interval_minutes: 5,
                downtime_tolerance: 3,
            },
            uptime_fails: 0,
            internal_log: vec![],
            warning_settings: WarningSettings {
                use_email: false,
                send_post_request: false,
                post_request_routes: vec![],
                email: "test@example.com".to_string(),
                daily_max: 5,
            },
            uptime_urls: vec![UrlEntry {
                description: "google.com".to_string(),
                url: "https://google.com".to_string(),
                is_ok: false,
            }],
            backups: vec![BackupEntry {
                description: "https://nosite.com".to_string(),
                url: "https://nosite.com".to_string(),
                restore: "https://nosite.com".to_string(),
                max: 10,
                interval: "d".to_string(),
                time: 800,
                logs: Vec::new(),
            }],
            // backup_logs: vec![],
            token: "".to_string(),
            secret: "".to_string(),
            jwt_expiry: 600,
            payload: HashMap::new(),
            backup_enabled: false,
            backup_trigger_rx: rx,
            smtp_config: SmtpConfig {
                server: "smtp.example.com".to_string(),
                port: 587,
                username: "nouser".to_string(),
                password: "nopassword".to_string(),
                from: "nobody".to_string(),
            },
            warnings_sent: 0,
        }
    }
}

impl From<Config> for StatusChecker {
    fn from(cfg: Config) -> Self {
        let (_tx, rx) = std::sync::mpsc::channel();
        Self {
            uptime_url_settings: cfg.url_uptime_settings,
            uptime_fails: 0,
            internal_log: vec![],
            warning_settings: cfg.warning_settings,
            uptime_urls: cfg.urls,
            backups: cfg.backups,
            token: cfg.token,
            secret: cfg.secret,
            jwt_expiry: cfg.jwt_expiry,
            payload: cfg.payload,
            backup_enabled: false,
            backup_trigger_rx: rx,
            smtp_config: cfg.smtp,
            warnings_sent: 0,
        }
    }
}

impl StatusChecker {
    /** we assume this runs once a minute */
    fn auto_backup(&mut self) {
        let current_time = Utc::now();
        let minute = current_time.minute();
        let hour = current_time.hour() * 60;
        let day = current_time.weekday() as u32 * 24 * 60;
        let month = current_time.day() * 24 * 60;

        let mut to_backup = Vec::new();

        for (i, backup) in self.backups.iter().enumerate() {
            let interval = &backup.interval;
            let time = backup.time;

            let should_backup = if interval == "h" {
                let hour_time = time % 60;
                minute == hour_time
            } else if interval == "d" {
                let day_minute = hour + minute;
                let day_time = time % (24 * 60);
                day_minute == day_time
            } else if interval == "w" {
                let week_minute = day + hour + minute;
                let week_time = time % (7 * 24 * 60);
                week_minute == week_time
            } else if interval == "m" {
                let month_minute = month + hour + minute;
                let month_time = time % (31 * 24 * 60);
                month_minute == month_time
            } else {
                false
            };

            if should_backup {
                to_backup.push(i);
            }
        }

        for i in to_backup {
            self.attempt_backup(i);
        }
    }

    fn uptime_check(&mut self) {
        let url_length = self.uptime_urls.len();

        for i in 0..url_length {
            let url_test: &str = &self.uptime_urls[i].url;

            match send_request(url_test) {
                Ok(()) => {
                    self.uptime_urls[i].is_ok = true;
                }
                Err(_err) => {
                    self.uptime_urls[i].is_ok = false;
                    self.uptime_fails += 1;
                    self.internal_log.push(InternalLogEntry {
                        message: format!("{} is down", self.uptime_urls[i].description),
                        timestamp: Utc::now().to_rfc3339(),
                    });

                    print_to_internal_log_file(InternalLog {
                        entries: self.internal_log.clone(),
                    });

                }
            }
        }

        if self.uptime_fails > self.uptime_url_settings.downtime_tolerance {
            let mut message_for_email = "Uptime check failed for the following URLs:\n".to_string();
            let mut failed_url_descriptions = Vec::new();

            for i in 0..url_length {
                if !self.uptime_urls[i].is_ok {
                    message_for_email.push_str(&format!("{}\n", self.uptime_urls[i].description));
                    failed_url_descriptions.push(self.uptime_urls[i].description.as_str());
                }
            }
            
            let log_lines: Vec<String> = self.internal_log
                .iter()
                .rev() // Reverse the order to get the latest entries first...
                .take(50)
                .map(|entry| format!("{} - {}", entry.timestamp, entry.message))
                .collect();

            message_for_email.push_str(&format!(
                "\nThese are the last {} lines of the internal log:\n{}",
                log_lines.len(),
                join_with_line_breaks(log_lines.clone()) // Clone for email
            ));



            let mut has_sent_warning = false;
            let is_over_daily_limit = self.warnings_sent >= self.warning_settings.daily_max;

            if is_over_daily_limit {
                self.internal_log.push(InternalLogEntry {
                    message: "Warning limit exceeded".to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                });

                print_to_internal_log_file(InternalLog {
                    entries: self.internal_log.clone(),
                });

            }



            if self.warning_settings.use_email && !is_over_daily_limit {

                has_sent_warning = true;

                let smtp = &self.smtp_config;
                let email_result = try_to_send_email(
                    &self.warning_settings.email,
                    "Uptime check failed",
                    &message_for_email,
                    smtp,
                );
                match email_result {
                    Ok(_) => println!("Warning email sent successfully!"),
                    Err(e) => println!("Failed to send warning email: {}", e),
                };
            }

            if self.warning_settings.send_post_request && !is_over_daily_limit {

                has_sent_warning = true;

                let warning_payload = json!({
                    "time": Utc::now().to_rfc3339(),
                    "description": format!("Uptime check failed. URLs down: {}", failed_url_descriptions.join(", ")),
                    "logs": log_lines // Use the already collected log_lines
                });
                let json_string = warning_payload.to_string();

                let token_to_use = if self.token.is_empty() {
                    match create_jwt(&self.payload, &self.secret, &self.jwt_expiry) {
                        Ok(jwt) => jwt,
                        Err(e) => {
                            println!("Failed to create JWT for warning POST: {}", e);
                            String::new() // Use empty string if JWT creation fails
                        }
                    }
                } else {
                    self.token.clone()
                };
                
                // Proceed even if token_to_use is empty, as the server might not require auth
                // or an empty Bearer token might be acceptable in some scenarios.
                // If a token is absolutely required and JWT creation fails, this will likely fail at the server.
                for route_url in &self.warning_settings.post_request_routes {
                    match send_warning_post_request(&token_to_use, &json_string, route_url) {
                        Ok(_) => println!("Successfully sent POST warning to {}", route_url),
                        Err(e) => println!("Failed to send POST warning to {}: {}", route_url, e),
                    }
                }
            }


            if has_sent_warning {
                self.warnings_sent += 1;
            }


            self.uptime_fails = 0; // Reset fails after warnings are sent
        } else {
            // Optional: Log that no warning was sent if needed for debugging
            // println!("Uptime checks passed or tolerance not exceeded. No warning sent.");
        }
    }





    
    fn import_internal_log(&mut self) {
        let log = load_internal_log().unwrap_or_else(|_| InternalLog { entries: vec![] });
        self.internal_log = log.entries;
    }



    fn from_config() -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = load_config()?;
        let mut backups = config.backups;


        if config.url_uptime_settings.interval_minutes == 0 {
            // Option 1: Log and use a default
            eprintln!("Warning: url_uptime_settings.interval_minutes is 0. Using default of 60 minutes.");
            config.url_uptime_settings.interval_minutes = 60; 
        }



        //loads the log for each backup.
        for entry in &mut backups {
            let logs = load_log(&entry.description).unwrap_or_else(|_| Log { entries: vec![] });
            entry.logs = logs.entries;
        }

        let (_tx, rx) = std::sync::mpsc::channel();

        let mut app = Self {
            uptime_url_settings: config.url_uptime_settings,
            internal_log: vec![],
            warning_settings: config.warning_settings,
            uptime_urls: config.urls,
            backups,
            token: config.token,
            secret: config.secret,
            jwt_expiry: config.jwt_expiry,
            payload: config.payload,
            backup_enabled: false,
            backup_trigger_rx: rx,
            smtp_config: config.smtp,
            uptime_fails: 0,
            warnings_sent: 0,
        };

        app.import_internal_log();

        Ok(app)
    }

    fn attempt_backup(&mut self, i: usize) {
        println!("Attempting backup of {}", self.backups[i].url);

        let save_path = &self.backups[i].description;

        let token = "";

        let backup_attempt = download_file(&self.backups[i].url, save_path, token);

        match backup_attempt {
            Ok(filename) => {
                println!("It worked: {}", filename);

                let _ = add_to_backup_log(&filename, &self.backups[i].description);

                // Re-read logs after successful backup
                match load_log(&save_path) {
                    Ok(log) => {
                        self.backups[i].logs = log.entries;

                        let filename = self.backups[i].description.clone();

                        println!("Trying to remove: {}", filename);

                        self.remove_backups_over_limit(&filename);
                    }
                    Err(err) => {
                        println!("Could not reload log after backup: {}", err);
                        self.backups[i].logs = vec![];
                    }
                }
            }
            Err(err) => {

                let error_message = format!("Backup failed for URL: {}. Error: {}", self.backups[i].url, err);
                println!("{}", error_message);
                self.internal_log.push(InternalLogEntry {
                    message: error_message.clone(),
                    timestamp: Utc::now().to_rfc3339(),
                });

                // Save the internal log after adding the new entry

                print_to_internal_log_file(InternalLog {
                    entries: self.internal_log.clone(),
                });



                let mut has_sent_warning = false;
                let is_over_daily_limit = self.warnings_sent >= self.warning_settings.daily_max;

                if is_over_daily_limit {
                    self.internal_log.push(InternalLogEntry {
                        message: "Warning limit exceeded".to_string(),
                        timestamp: Utc::now().to_rfc3339(),
                    });
    
                    print_to_internal_log_file(InternalLog {
                        entries: self.internal_log.clone(),
                    });
    
                }

                
                if self.warning_settings.use_email && !is_over_daily_limit  {


                        has_sent_warning = true;


                    println!("Sending backup failure warning email...");
                    let smtp = &self.smtp_config;
                    let email_result = try_to_send_email(
                        &self.warning_settings.email,
                        "Backup failed",
                        &error_message,
                        smtp,
                    );
                    match email_result {
                        Ok(_) => println!("Warning email sent successfully!"),
                        Err(e) => println!("Failed to send warning email: {}", e),
                    }
                }

                if self.warning_settings.send_post_request && !is_over_daily_limit {


                        has_sent_warning = true;
                    


                     let log_lines: Vec<String> = self.internal_log
                        .iter()
                        .rev()
                        .take(50)
                        .map(|entry| format!("{} - {}", entry.timestamp, entry.message))
                        .collect();

                    let warning_payload = json!({
                        "time": Utc::now().to_rfc3339(),
                        "description": error_message, // Use the detailed error message
                        "logs": log_lines
                    });
                    let json_string = warning_payload.to_string();
                    
                    // Reuse token logic from above or re-evaluate if needed for this specific POST
                    // For simplicity, let's assume the same token logic applies.
                    let post_token = if self.token.is_empty() {
                        create_jwt(&self.payload, &self.secret, &self.jwt_expiry).unwrap_or_default()
                    } else {
                        self.token.clone()
                    };

                    for route_url in &self.warning_settings.post_request_routes {
                        match send_warning_post_request(&post_token, &json_string, route_url) {
                            Ok(_) => println!("Successfully sent POST warning for backup failure to {}", route_url),
                            Err(e) => println!("Failed to send POST warning for backup failure to {}: {}", route_url, e),
                        }
                    }
                }


                if has_sent_warning{
                    self.warnings_sent += 1;

                }


            }
        }
    }

    fn remove_backups_over_limit(&mut self, description: &str) {
        for backup in &mut self.backups {
            if backup.description == description {
                let number_over_limit = backup.logs.len() as i32 - backup.max as i32;

                if number_over_limit > 0 {
                    println!("There are {} backups over limit", number_over_limit);

                    let mut j = 0;

                    loop {
                        if j >= number_over_limit || j > 5 {
                            break;
                        }

                        let filename = &backup.logs[0].filename;

                        let delete_attempt = delete_file(&filename, &backup.description);

                        match delete_attempt {
                            Ok(()) => {
                                println!("file delete success");

                                //remove the first log entry
                                backup.logs.remove(0);

                                //save the log file again
                                let log_path = Path::new(&backup.description).join("log.toml");
                                let log = Log {
                                    entries: backup.logs.clone(),
                                };
                                if let Ok(toml_str) = toml::to_string(&log) {
                                    // ignore write errors here; handle them if you care
                                    let _ = write(&log_path, toml_str);
                                } else {
                                    println!("Failed to write log file!");
                                }
                            }
                            // Err(err) => println!("file delete fail{}: {}", err),
                            Err(err) => println!("file delete fail: {}", err),
                        }

                        j += 1;
                    }
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct Config {
    url_uptime_settings: UptimeUrlSettings,
    warning_settings: WarningSettings,
    #[serde(default)] // If "urls" is missing, it will default to an empty Vec<UrlEntry>
    urls: Vec<UrlEntry>,
    #[serde(default)] // If "backups" is missing, it will default to an empty Vec<BackupEntry>
    backups: Vec<BackupEntry>,
    token: String,
    secret: String,
    jwt_expiry: u64,
    #[serde(default)] // For HashMap, default is an empty map
    payload: HashMap<String, TomlValue>,
    smtp: SmtpConfig,
}



fn main() -> eframe::Result<()> {


    let config_path = Path::new("config.toml");
    let app_config_result = load_config();

    if app_config_result.is_err() {
        eprintln!(
            "Warning: Could not load 'config.toml': {}",
            app_config_result.as_ref().err().unwrap() // Show the error
        );

        if !config_path.exists() {
            eprintln!("'config.toml' not found. Attempting to create a default one.");
            match write(config_path, default_config::DEFAULT_CONFIG_TOML) {
                Ok(_) => {
                    eprintln!("Successfully created 'config.toml' with default settings.");
                    eprintln!("Please review and edit 'config.toml' then restart the application.");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Error: Could not write default 'config.toml': {}", e);
                }
            }
        } else {
            eprintln!("'config.toml' exists but is malformed. Please fix it or delete it to generate a default.");
        }
    }
 


    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size(Vec2::new(800.0, 600.0)),
        ..Default::default()
    };

    eframe::run_native(
        "WebSync Station",
        options,
        Box::new(|_cc| {
            let mut app = StatusChecker::from_config().unwrap_or_else(|err| {
                eprintln!("Failed to load config: {}", err);
                StatusChecker::default()
            });



            if app.internal_log.is_empty(){
                app.internal_log.push(InternalLogEntry {
                    message: "Welcome to WebSync Station. If this is your first time using WWS remember to edit the config.toml file and then restart the app.".to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                });
            }


            let (tx, rx) = std::sync::mpsc::channel();
            app.backup_trigger_rx = rx;

            thread::spawn(move || {
                loop {
                    let now = Utc::now();
                    let next_min = (now.minute() + 1) % 60; // wraps 59→0
                    let next_time = NaiveTime::from_hms_opt(now.hour(), next_min, 0)
                        .expect("hour/minute within valid range");
                    let next_tick = now
                        .date_naive()
                        .and_time(next_time)
                        .and_local_timezone(Utc)
                        .unwrap();

                    let sleep_dur = (next_tick - now)
                        .to_std()
                        .unwrap_or_else(|_| Duration::from_secs(60));

                    thread::sleep(sleep_dur);

                    // 4) poke the UI
                    if tx.send(()).is_err() {
                        break; // if the receiver was dropped, exit the loop
                    }
                }
            });

            Box::new(app)
        }),
    )
}

impl eframe::App for StatusChecker {
    //this runs several times a second
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                while let Ok(()) = self.backup_trigger_rx.try_recv() {

                    let current_time = Utc::now();
                    let minute = current_time.minute();
                    let hour = current_time.hour() * 60;
                    let total_minutes = hour + minute;

                    if minute == 0 && hour == 0{

                        // Reset the warnings sent counter at the start of a new day
                        self.warnings_sent = 0;

                    }
                    


                    if self.backup_enabled {
                        self.auto_backup();
                    }


                    if total_minutes % self.uptime_url_settings.interval_minutes == 0 {
                        self.uptime_check();
                    }
                }

                ctx.request_repaint_after(Duration::from_secs(1)); // keep UI responsive

                // if ui.button("Test autobackup").clicked() {
                //     self.auto_backup();
                // }

                // if ui.button("Test internal log").clicked() {
                //     let entry = InternalLogEntry {
                //         message: "Test message".to_string(),
                //         timestamp: Utc::now().to_rfc3339(),
                //     };

                //     self.internal_log.push(entry);

                //     print_to_internal_log_file(InternalLog {
                //         entries: self.internal_log.clone(),
                //     });
                // }

                //This is inside the closure, and it is the main loop of the app.

                ui.heading("WebSync Station");

                ui.add_space(10.0);
                let url_length = self.uptime_urls.len();

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let mut i = 0;

                        loop {

                            if i >= url_length {
                                break;
                            }


                            ui.horizontal(|ui| {
                                // ui.add_space(ui.available_width() - 80.0); // push the button right

                                ui.add_space(10.0);

                                let color = if self.uptime_urls[i].is_ok {
                                    Color32::from_rgb(0, 200, 0) // Green
                                } else {
                                    Color32::from_rgb(200, 0, 0) // Red
                                };

                                let text = if self.uptime_urls[i].is_ok {
                                    "✅"
                                } else {
                                    "❌"
                                };
                                let button = egui::Button::new(text).fill(color);

                                ui.add(button);
                                ui.label(self.uptime_urls[i].description.to_string());
                            });

                            i += 1;
                            if i >= url_length {
                                break;
                            };
                        }
                    });

                    ui.add_space(10.0);

                    Frame::none()
                        .fill(Color32::from_rgb(30, 30, 30))
                        .stroke(Stroke::new(1.0, Color32::WHITE))
                        .rounding(Rounding::same(4.0))
                        .inner_margin(Vec2::splat(6.0))
                        .show(ui, |ui_frame| {
                            let dynamic_content_width = ui_frame.available_width();
                            let desired_scroll_area_size = egui::vec2(dynamic_content_width, 200.0);

                            ui_frame.allocate_ui_with_layout(
                                desired_scroll_area_size,
                                Layout::top_down(Align::Min),
                                |ui_for_scroll_area| {
                                    // Start building the ScrollArea
                                    let scroll_area_builder = ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .stick_to_bottom(true);

                                    // Now show the (potentially modified) ScrollArea
                                    scroll_area_builder.show(
                                        ui_for_scroll_area,
                                        |ui_scroll_content| {
                                            let interal_log_length = self.internal_log.len();
                                            for i in 0..interal_log_length {
                                                ui_scroll_content.add(
                                                    Label::new(
                                                        RichText::new(format!(
                                                            "{} - {}",
                                                            self.internal_log[i].timestamp,
                                                            self.internal_log[i].message
                                                        ))
                                                        .monospace()
                                                        .color(Color32::LIGHT_GREEN),
                                                    )
                                                    .wrap(true),
                                                );
                                            }
                                        },
                                    );
                                },
                            );
                        });
                });

                /* Add a button for manually checking all the URLs */

                ui.add_space(10.0);

                if ui.button("Manually check all urls").clicked() {
                    self.uptime_check();
                }

                //for testing and making the compliler shut up...

                // let jwt_string: String;

                // if ui.button("Click to make JWT").clicked() {
                //     let jwt_result = create_jwt(&self.payload, &self.secret, &self.jwt_expiry);

                //     match jwt_result {
                //         Ok(jwt) => {
                //             jwt_string = jwt;
                //         }

                //         Err(_err) => {
                //             jwt_string = String::from("error");
                //         }
                //     }

                //     println!("{}", &jwt_string)
                // }

                ui.add_space(10.0);

                ui.separator();

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.heading("Backup system");

                    let enable_caption = if self.backup_enabled {
                        "Disable backup schedule"
                    } else {
                        "Enable backup schedule"
                    };

                    if ui.button(enable_caption).clicked() {
                        self.backup_enabled = !self.backup_enabled;
                    }

                    let caption = if self.backup_enabled {
                        RichText::new("Backup schedules enabled").color(Color32::GREEN)
                    } else {
                        RichText::new("Backup schedules disabled").color(Color32::RED)
                    };

                    ui.add_space(10.0);

                    ui.label(caption);
                });

                ui.separator();
                //Backup system ui

                let backup_length = self.backups.len();
                let max_backups = 10;
                // println!("There are {} backups", backup_length);
                let mut i = 0;

                loop {
                    if i >= backup_length || i > max_backups {
                        break;
                    }

                    let log_entries_length = self.backups[i].logs.len();

                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&self.backups[i].description).strong());

                        ui.add_space(10.0);

                        ui.label(format!("Restore points available: {}", log_entries_length));
                        ui.add_space(10.0);

                        if ui.button("Backup manually now").clicked() {
                            self.attempt_backup(i);
                        };
                    });

                    ui.horizontal(|ui| {
                        if log_entries_length > 0 {
                            ui.collapsing(
                                format!("Restore {}", self.backups[i].description),
                                |ui| {
                                    let mut j = 0;

                                    loop {
                                        if j >= self.backups[i].logs.len() {
                                            break;
                                        }

                                        ui.horizontal(|ui| {
                                            let time_stamp = format_timestamp(
                                                &self.backups[i].logs[j].timestamp,
                                            );

                                            let size_kb =
                                                self.backups[i].logs[j].size as f64 / 1000.0;
                                            let size_str = format!("{:.1} KB", size_kb);

                                            ui.label(format!("{}- Size:{}", time_stamp, size_str));

                                            if ui.button("Restore").clicked() {


                                                let path = format!(
                                                    "{}/{}",
                                                    self.backups[i].description,
                                                    self.backups[i].logs[j].filename
                                                );


                                                let token_to_use = if self.token.is_empty() {
                                                    match create_jwt(&self.payload, &self.secret, &self.jwt_expiry) {
                                                        Ok(jwt) => jwt,
                                                        Err(e) => {
                                                            println!("Failed to create JWT for warning POST: {}", e);
                                                            String::new() // Use empty string if JWT creation fails
                                                        }
                                                    }
                                                } else {
                                                    self.token.clone()
                                                };




                                                let restore_attempt = restore_backup(
                                                    &self.backups[i].restore,
                                                    &path,
                                                    &token_to_use
                                                );

                                                match restore_attempt {
                                                    Ok(_) => {
                                                        println!("Restored file successfully");

                                                        //add the restored file to the internal log

                                                        let log_entry = InternalLogEntry {
                                                            message: format!(
                                                                "Successfully restored file {} from {}",
                                                                self.backups[i].logs[j].filename,
                                                                self.backups[i].description
                                                            ),
                                                            timestamp: Utc::now().to_rfc3339(),
                                                        };

                                                        self.internal_log.push(log_entry);

  
                                                    }
                                                    Err(err) => {
                                                        println!("Restore failed: {}", err);

                                                        //add the error to the internal log

                                                        let log_entry = InternalLogEntry {
                                                            message: format!(
                                                                "Failed to restore file {} from {}: {}",
                                                                self.backups[i].logs[j].filename,
                                                                self.backups[i].description,
                                                                err
                                                            ),
                                                            timestamp: Utc::now().to_rfc3339(),
                                                        };

                                                        self.internal_log.push(log_entry);




                                                    }
                                                }





                                                println!(
                                                    "Restoring {}",
                                                    self.backups[i].logs[j].filename
                                                )
                                            }
                                        });

                                        j += 1;
                                    }
                                },
                            );
                        }

                        ui.add_space(10.0);

                        let time_left =
                            calc_time_to_backup(&self.backups[i].time, &self.backups[i].interval);

                        ui.vertical(|ui| ui.label(format!("Next backup in {}", time_left)));
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);

                    i += 1;
                }
            })
        });
    }
}

fn send_request(url: &str) -> Result<(), Box<dyn Error>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10)) // Add a timeout
        .build()?;
    let response = client.get(url).send()?;

    if !response.status().is_success() {
        return Err(format!("Request to {} failed with status: {}", url, response.status()).into());
    }

    Ok(())
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let content = read_to_string("config.toml")?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

fn create_jwt(
    payload: &HashMap<String, TomlValue>,
    secret: &str,
    expiry: &u64,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut json_map = JsonMap::new();

    for (k, v) in payload {
        let json_val = toml_to_json_value(v)?;
        json_map.insert(k.clone(), json_val);
    }

    let iat = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let exp = iat + expiry;

    json_map.insert("iat".to_string(), json!(iat));
    json_map.insert("exp".to_string(), json!(exp));

    let json_payload = JsonValue::Object(json_map);

    let token = encode(
        &Header::default(),
        &json_payload,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    Ok(token)
}

fn toml_to_json_value(val: &TomlValue) -> Result<JsonValue, Box<dyn Error>> {
    Ok(serde_json::to_value(val)?)
}

fn download_file(
    url_str: &str,
    save_folder: &str,
    token: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = Url::parse(url_str)?;
    
    let filename_from_url = url
        .path_segments()
        .and_then(|segments| segments.last())
        .filter(|name| !name.is_empty())
        .map(|s| s.to_string()) // Convert to String
        .ok_or_else(|| format!("Cannot extract filename from URL path: {}", url_str))?;

    let folder_path = Path::new(save_folder);
    create_dir_all(folder_path)?;

    let client = Client::builder()
        .timeout(Duration::from_secs(300)) // 5 min timeout for download
        .build()?;
    
    let mut request_builder = client.get(url.clone()); // Clone URL for request
    if !token.is_empty() {
        request_builder = request_builder.header(AUTHORIZATION, format!("Bearer {}", token));
    }
    
    let mut response = request_builder.send()?;

    if !response.status().is_success() {
        return Err(format!("Request to {} failed with status: {}", url_str, response.status()).into());
    }

    // Try to get filename from Content-Disposition header first
    let mut final_filename = if let Some(cd_header) = response.headers().get("Content-Disposition") {
        if let Ok(cd_str) = cd_header.to_str() {
            extract_filename_from_cd(cd_str).unwrap_or(filename_from_url)
        } else {
            filename_from_url // Fallback if header is not valid UTF-8
        }
    } else {
        filename_from_url // Fallback if header is not present
    };
    
    // Sanitize filename to prevent path traversal or invalid characters
    final_filename = sanitize_filename::sanitize(&final_filename);
    if final_filename.is_empty() { // if sanitize results in empty, use a default
        final_filename = "downloaded_file".to_string();
    }


    // Handle filename conflicts by appending a number
    let mut candidate_path = folder_path.join(&final_filename);
    if candidate_path.exists() {
        let stem = Path::new(&final_filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let extension = Path::new(&final_filename)
            .extension()
            .and_then(|e| e.to_str());

        for i in 0.. { // Loop indefinitely until a unique name is found
            let versioned_filename = match extension {
                Some(ext) => format!("{}_{}.{}", stem, i, ext),
                None => format!("{}_{}", stem, i),
            };
            candidate_path = folder_path.join(&versioned_filename);
            if !candidate_path.exists() {
                final_filename = versioned_filename;
                break;
            }
            if i > 1000 { // Safety break
                 return Err("Could not find a unique filename after 1000 attempts.".into());
            }
        }
    }
    
    let mut dest_file = File::create(&candidate_path)?;
    copy(&mut response, &mut dest_file)?;

    Ok(final_filename)
}

fn load_log(foldername: &str) -> Result<Log, Box<dyn std::error::Error>> {
    let folder = Path::new(foldername);
    let log_path = folder.join("log.toml");

    let content: String = read_to_string(log_path)?;
    let log: Log = toml::from_str(&content)?;
    Ok(log)
}

fn add_to_backup_log(filename: &str, foldername: &str) -> Result<(), Box<dyn std::error::Error>> {
    // makes sure there is a log file

    let folder = Path::new(foldername);

    let mut candidate_path = folder.join("log");

    candidate_path.set_extension("toml");

    let log_exists = candidate_path.exists();

    let mut logs: Log = Log {
        entries: Vec::new(),
    };

    if !log_exists {
        let _ = File::create(&candidate_path);
    } else {
        let logs_load = load_log(foldername);

        match logs_load {
            Ok(log_entries) => {
                logs = log_entries;
            }

            Err(_err) => {}
        }
    }

    //add the new entry to the log

    let new_entry = LogEntry {
        filename: filename.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        size: 12345,
    };

    logs.entries.push(new_entry);

    //write to the log file

    let toml_string = toml::to_string(&logs)?;
    write(candidate_path, toml_string)?;

    Ok(())
}

fn load_internal_log() -> Result<InternalLog, Box<dyn std::error::Error>> {
    let log_path = Path::new("internal_log.toml");

    let content: String = read_to_string(log_path)?;
    let log: InternalLog = toml::from_str(&content)?;
    Ok(log)
}

fn extract_filename_from_cd(cd: &str) -> Option<String> {
    //no regex, just a simple split
    let parts: Vec<&str> = cd.split(';').collect();
    for part in parts {
        let trimmed = part.trim();
        if trimmed.starts_with("filename=") {
            let filename = trimmed[9..].trim_matches('"').to_string();
            return Some(filename);
        }
    }
    None
}

fn format_timestamp(ts: &str) -> String {
    use chrono::{DateTime, Local};

    match DateTime::parse_from_rfc3339(ts) {
        Ok(parsed) => {
            let local = parsed.with_timezone(&Local);
            local.format("%d.%m.%Y %H:%M").to_string()
        }
        Err(_) => "Invalid timestamp".to_string(),
    }
}

fn calc_time_to_backup(time: &u32, interval: &str) -> String {
    let current_time = Utc::now();
    let mut time_to_backup: i32 = 10000;
    let mut wrap_constant = 0;

    if interval == "h" {
        time_to_backup = (*time as i32 % 60) - current_time.minute() as i32;

        wrap_constant = 60;
    }

    if interval == "d" {
        let current_minutes = (current_time.hour() * 60 + current_time.minute()) as i32;
        time_to_backup = *time as i32 - current_minutes;
        wrap_constant = 1440;
    }

    if interval == "w" {
        let weekday = current_time.weekday().num_days_from_monday(); // 0 = Monday, 6 = Sunday
        let current_minutes =
            (weekday * 1440 + current_time.hour() * 60 + current_time.minute()) as i32;
        time_to_backup = *time as i32 - current_minutes;
        wrap_constant = 10080;
    }

    if interval == "m" {
        let days_in = current_time.day() - 1; // day() is 1-based
        let current_minutes =
            (days_in * 1440 + current_time.hour() * 60 + current_time.minute()) as i32;
        time_to_backup = *time as i32 - current_minutes;
        // rough wraparound (assuming all months have at least 28 days)
        let minutes_in_month = 31 * 1440;
        wrap_constant = minutes_in_month;
    }

    if time_to_backup < 0 {
        time_to_backup = wrap_constant + time_to_backup;
    }

    time_to_backup_to_text(time_to_backup)
}

fn time_to_backup_to_text(time_to_backup: i32) -> String {
    let time_string: String;

    if time_to_backup < 60 {
        time_string = format!("{} minutes.", time_to_backup);
    } else if time_to_backup < 24 * 60 {
        time_string = format!("{} hours.", time_to_backup / 60);
    } else if time_to_backup < 7 * 24 * 60 {
        time_string = format!("{} days.", time_to_backup / (24 * 60));
    } else {
        time_string = format!("{} weeks.", time_to_backup / (7 * 24 * 60));
    }

    time_string
}

/// Sends a plain-text e-mail. Return `Result` so callers can bubble up errors.
fn try_to_send_email(
    address: &str,
    subject: &str,
    content: &str,
    smtp: &SmtpConfig,
) -> Result<(), Box<dyn std::error::Error>> {


    //log the parameters

    println!("Sending email to: {}", address);
    println!("Subject: {}", subject);
    println!("Content: {}", content);
    println!("SMTP server: {}", smtp.server);
    println!("SMTP port: {}", smtp.port);
    println!("SMTP username: {}", smtp.username);
    println!("SMTP password: {}", "<hidden>");
    println!("SMTP from: {}", smtp.from);




    let email = Message::builder()
        .from(smtp.from.parse()?)
        .to(address.parse()?)
        .subject(subject)
        .header(LettreContentType::TEXT_PLAIN) // Use the renamed import
        .body(String::from(content))?;

    let creds = Credentials::new(smtp.username.to_owned(), smtp.password.to_owned());

    let tls_parameters = TlsParameters::new(smtp.server.clone())?;

    let mailer = SmtpTransport::relay(&smtp.server)?
        .port(smtp.port)
        .credentials(creds)
        .tls(Tls::Opportunistic(tls_parameters)) // Use Tls::Opportunistic for STARTTLS on port 587
        .timeout(Some(Duration::from_secs(20)))  // Connection/operation timeout
        .build(); // Builds a synchronous transport

    mailer.send(&email)?;
    println!("Email sent successfully to {} with subject '{}'", address, subject);
    Ok(())

}

pub fn delete_file(filename: &str, folder_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let folder = Path::new(folder_name);

    if !folder.exists() {
        return Err(format!("Folder `{}` does not exist", folder_name).into());
    }
    if !folder.is_dir() {
        return Err(format!("`{}` is not a directory", folder_name).into());
    }

    let path: PathBuf = folder.join(filename);

    if !path.exists() {
        return Err(format!("File `{}` not found in `{}`", filename, folder_name).into());
    }
    if path.is_dir() {
        return Err(format!("`{}` is a directory, not a file", path.display()).into());
    }

    remove_file(&path)?;

    Ok(())
}

fn print_to_internal_log_file(internal_log: InternalLog) {
    let log_path = Path::new("internal_log.toml");
    let toml_str = toml::to_string(&internal_log).unwrap();

    let result = write(&log_path, &toml_str);

    match result {
        Ok(_) => println!("Log written successfully!"),
        Err(e) => println!("Failed to write log: {}", e),
    }
}

fn join_with_line_breaks(lines: Vec<String>) -> String {
    lines.join("\n")
}



fn send_warning_post_request(
    token: &str,
    json_payload_string: &str,
    url: &str,
) -> Result<(), Box<dyn Error>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(15)) // Set a reasonable timeout
        .build()?;

    let mut request_builder = client.post(url)
        .header(CONTENT_TYPE, "application/json")
        .body(json_payload_string.to_owned()); // .to_owned() because body takes Into<Body>

    if !token.is_empty() {
        request_builder = request_builder.header(AUTHORIZATION, format!("Bearer {}", token));
    }

    let response = request_builder.send()?;

    if !response.status().is_success() {
        let status = response.status();
        // Try to get the error body, but don't fail if it's not available or not text
        let error_body = response.text().unwrap_or_else(|e| format!("Could not retrieve error body: {}", e));
        return Err(format!(
            "POST request to {} failed with status: {}. Response: {}",
            url, status, error_body
        ).into());
    }

    Ok(())
}


fn restore_backup(url: &str, filename: &str, token: &str) -> Result<(), Box<dyn Error>> {
    let part = multipart::Part::file(filename)?
                   .mime_str("application/octet-stream")?;
    let form = multipart::Form::new()
                   .part("file", part);

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let mut req = client.post(url)
        .multipart(form);

    if !token.is_empty() {
        req = req.header(AUTHORIZATION, format!("Bearer {}", token));
    }

    let resp = req.send()?;
    if !resp.status().is_success() {
        return Err(format!(
            "POST to {} failed: {}",
            url, resp.status()
        ).into());
    }
    Ok(())
}