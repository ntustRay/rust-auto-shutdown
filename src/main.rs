#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod notification;
use eframe::egui;
use rust_auto_shutdown::schedule;

const APP_NAME: &str = "R Auto Shutdown";

fn console_output() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, System::Console::*};
        let output = GetStdHandle(STD_OUTPUT_HANDLE);
        if output.is_null() || output == INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let worker = match args.as_slice() {
        [mode] => Some((mode.as_str(), None)),
        [mode, flag, target] if flag == "--remind-now" => {
            Some((mode.as_str(), Some(target.as_str())))
        }
        _ => None,
    };
    if let Some((mode @ ("--scheduled-dry-run" | "--scheduled-shutdown"), immediate)) = worker {
        let result = match schedule::prepare_reminder(mode == "--scheduled-dry-run", immediate) {
            Ok(Some(session)) => notification::run_weekly(session, immediate.is_some())
                .map_err(|error| error.to_string()),
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "--version" | "-V") {
        console_output();
        if matches!(args[0].as_str(), "--version" | "-V") {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        } else {
            println!(
                "{APP_NAME}\n\nUsage: r-auto-shutdown [--dry-run] [--help] [--version]\n\nDesktop shutdown timer for Windows, macOS, and Linux. Weekly scheduling is available on Windows. Simulation is on by default."
            );
        }
        return Ok(());
    }
    if !args.is_empty() && args.as_slice() != ["--dry-run"] {
        console_output();
        eprintln!("Unknown arguments. Use --help.");
        std::process::exit(2);
    }
    eframe::run_native(
        APP_NAME,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([840.0, 760.0])
                .with_min_inner_size([840.0, 760.0]),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::ShutdownApp::new(cc)))),
    )
}
