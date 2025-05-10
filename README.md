![image](https://github.com/user-attachments/assets/e3340f65-08f4-4164-81ec-6479c4c49f35)

# WebSync Station 🛰️

WebSync Station is a Rust-based desktop application designed to help you monitor the uptime of your web services and automate the backup of remote files. It provides a simple graphical user interface (GUI) built with `egui` and offers configurable warnings via email and POST requests.

[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)

## ✨ Features

*   **Uptime Monitoring:**
    *   Periodically checks a list of user-defined URLs.
    *   Configurable check interval and downtime tolerance.
*   **Automated Backups:**
    *   Schedule backups from remote URLs (e.g., database dump endpoints).
    *   Supports hourly, daily, weekly, and monthly backup intervals.
    *   Configurable time-of-day for scheduled backups.
    *   Manages a maximum number of stored backups (automatic rotation).
    *   Manual backup triggering.
    *   Logs backup activity per source.
*   **Warning System:**
    *   Sends email notifications (via SMTP) for uptime failures or backup issues.
    *   Sends POST requests to specified webhook URLs for failures.
    *   Optional JWT (HS256) authentication for POST requests.
    *   Configurable daily limit for warnings to prevent spam.
*   **Graphical User Interface (GUI):**
    *   Built with `egui` for a responsive and straightforward experience.
    *   Displays current uptime status and an internal event log.
*   **Configuration:**
    *   All settings managed via a `config.toml` file.
    *   Automatically creates a default `config.toml` if one doesn't exist on startup.
*   **Logging:**
    *   Maintains an `internal_log.toml` for application-wide events and errors.
    *   Each backup source has its own `log.toml` within its backup directory.

---




## 🚀 Getting Started

### Prerequisites

*   **For running pre-compiled binaries:** None, just download the appropriate file for your OS from the [Releases page](https://github.com/YOUR_USERNAME/YOUR_REPO/releases).
*   **For building from source:**
    *   [Rust toolchain](https://www.rust-lang.org/tools/install) (stable version recommended)
    *   Git

### Installation

1.  **Using Pre-compiled Binaries (Recommended for most users):**
    *   Go to the [Releases page](https://github.com/AndreasAanestad/websync_station/releases) of this repository.
    *   Download the latest binary for your operating system (currently only windows, but more will come)
    *   Place the executable in a directory of your choice.
    *   (Optional) Create a `config.toml` file in the same directory or let the application create a default one on first run.

2.  **Building from Source:**
    ```bash
    # 1. Clone the repository
    git clone https://github.com/AndreasAanestad/websync_station.git
    cd YOUR_REPO

    # 2. Build the application
    cargo build --release

    # The executable will be located at ./target/release/websync-station
    ```

---

## 📖 Usage

1.  Ensure you have a `config.toml` file in the same directory as the `websync-station` executable. If not, run the application once to generate a default `config.toml`, then edit it to your needs and restart the app.
2.  Run the executable:
    *   On Windows: Double-click `websync-station.exe`. The console window will be hidden.
3.  The main window will appear, showing:
    *   **Uptime Status:** A list of your configured URLs with a green check (✅) for OK or a red cross (❌) for down.
    *   **Internal Log:** A scrolling view of recent application events, errors, and backup attempts.
    *   **"Manually check all urls" button:** Triggers an immediate uptime check for all configured URLs.
    *   **Backup System:**
        *   **Enable/Disable backup schedule:** Toggles the automated backup scheduler.
        *   Status indicator for the backup schedule.
        *   For each configured backup:
            *   Description and number of available restore points.
            *   **"Backup manually now" button:** Triggers an immediate backup for that specific source.
            *   **"Restore [description]" (Collapsible):** Lists available backup files with timestamps and sizes.
            *   Estimated time until the next scheduled backup.

---

## 🔧 How It Works (Briefly)

*   **Main Loop:** The application runs an event loop, primarily driven by a once-per-minute timer tick.
*   **Configuration Loading:** On startup, `config.toml` is parsed. If it's missing or invalid, a default one is attempted to be created, or the app uses default internal values.
*   **Uptime Checks:** At configured intervals, `reqwest` sends GET requests to each URL. The status code determines if the site is "up." Failures increment a counter; if it exceeds `downtime_tolerance`, warnings are triggered.
*   **Automated Backups:** The `auto_backup` function checks the current time against each backup's schedule (`interval` and `time`). If a backup is due:
    *   A GET request (potentially with a Bearer token/JWT) is sent to the backup `url`.
    *   The response (expected to be a file) is downloaded and saved into a directory named after the backup's `description`.
    *   Filenames are derived from the URL or `Content-Disposition` header, with versioning for conflicts (e.g., `file_0.sql`, `file_1.sql`).
    *   A `log.toml` file in the backup directory tracks successful backups.
    *   Old backups are removed if the count exceeds `max`.
*   **Warnings:** If an uptime check fails beyond tolerance or a backup attempt fails:
    *   **Email:** `lettre` is used to send an email via the configured SMTP server.
    *   **POST Request:** `reqwest` sends a JSON payload (error details + recent logs) to configured webhook URLs, with optional Bearer token/JWT.
*   **Logging:**
    *   `internal_log.toml`: Stores general application messages, errors, and notable events.
    *   `<backup_description>/log.toml`: Stores metadata (filename, timestamp, size) for each successful backup file for a specific source.

---

## 🛠️ Troubleshooting

*   **Application doesn't start / `config.toml` errors:**
    *   Ensure `config.toml` is correctly formatted. TOML is sensitive to syntax.
    *   Delete `config.toml` and let the application generate a fresh default one, then carefully re-apply your settings.
*   **Emails not sending:**
    *   Double-check SMTP server, port, username, and password in `config.toml`.
    *   **CRITICAL for Gmail/Outlook.com etc.:** You likely need to generate an "App Password" for WebSync Station instead of using your regular account password. Search your email provider's help for "app password".
    *   Check your spam/junk folder.
    *   Ensure your firewall or antivirus isn't blocking outgoing connections on the SMTP port.
*   **Backups failing:**
    *   Verify the backup `url` is correct and accessible.
    *   If the backup URL requires authentication, ensure your `token` or JWT `secret` and `payload` in `config.toml` are correctly configured.
    *   Check the `internal_log.toml` for specific error messages.
    *   Ensure the application has write permissions to the directory where it's running (to create backup folders and files).
*   **Uptime checks consistently failing for a specific URL:**
    *   Verify the URL is correct and accessible from the machine running WebSync Station (e.g., try opening it in a browser or using `curl`).
    *   Some services might block frequent automated requests.

---



## 📜 License

This project is licensed under the MIT License.

---

## 🙏 Acknowledgements

*   [eframe/egui](https://github.com/emilk/egui) for the easy-to-use GUI framework.
*   The Rust community for fantastic libraries like `reqwest`, `serde`, `toml`, `chrono`, `lettre`, and `jsonwebtoken`.

```
