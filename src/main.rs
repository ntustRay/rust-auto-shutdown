#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod notification;
use eframe::egui;
use rust_auto_shutdown::schedule;

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
    if args.len() == 1
        && matches!(
            args[0].as_str(),
            "--scheduled-dry-run" | "--scheduled-shutdown"
        )
    {
        let result = match schedule::prepare_reminder(args[0] == "--scheduled-dry-run") {
            Ok(Some(session)) => {
                notification::run_weekly(session).map_err(|error| error.to_string())
            }
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "--version" | "-V") {
        console_output();
        if matches!(args[0].as_str(), "--version" | "-V") {
            println!("rust-auto-shutdown {}", env!("CARGO_PKG_VERSION"));
        } else {
            println!(
                "Rust Auto Shutdown\n\nUsage: rust-auto-shutdown [--dry-run] [--help] [--version]\n\nA Windows shutdown timer with weekly schedules. Simulation is on by default."
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
        "自動關機 · Auto Shutdown",
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
