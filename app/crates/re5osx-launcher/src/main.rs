//! re5OSX, one app: when the game is made on this Mac it opens straight
//! away; until then (the first time, or after an update) a setup window
//! finds the game folder, makes the menus' art and sounds, and makes the
//! game app (see install.rs).
//!
//!     re5OSX [--setup] [--port 8000] [--no-window]

mod gamefiles;
mod http;
mod install;
mod paths;
mod server;
mod setup;

use std::net::TcpListener;
use std::rc::Rc;
use std::sync::Arc;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::platform::macos::WindowBuilderExtMacOS;
use tao::window::{Theme, WindowBuilder};
use wry::WebViewBuilder;

/// The setup window has done its job (the game is open) or was told to close.
pub static QUIT: AtomicBool = AtomicBool::new(false);

/// Opens the game app with the game folder; this process then ends.
pub fn open_game(game: &Path) {
    let _ = std::process::Command::new("/usr/bin/open").arg("-a").arg(paths::game_app()).arg("--args").arg(game).spawn();
    QUIT.store(true, Ordering::Relaxed);
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    // Made and set up: straight into the game, no window.
    if !flag("--setup")
        && paths::game_app_current()
        && let Some(game) = server::ready_game()
    {
        open_game(&game);
        return Ok(());
    }
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok())
        .or_else(|| std::env::var("RE5OSX_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(8000);

    let server = server::Server::new();
    // Already running (another re5OSX window): this one only shows it.
    if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
        let handler = server.clone();
        http::serve(listener, Arc::new(move |r| handler.handle(r)));
    }
    let url = format!("http://localhost:{port}/");
    eprintln!("re5OSX: {url}");

    if flag("--no-window") {
        while !QUIT.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(500));
        }
        return Ok(());
    }

    let event_loop = EventLoop::new();
    // The page draws to the window's edges, under the traffic lights (as
    // re5OSX's start page does), and moves it by its top edge.
    let window = Rc::new(
        WindowBuilder::new()
            .with_title("re5OSX")
            .with_inner_size(LogicalSize::new(480.0, 560.0))
            .with_resizable(false)
            .with_theme(Some(Theme::Dark))
            .with_titlebar_transparent(true)
            .with_title_hidden(true)
            .with_fullsize_content_view(true)
            .build(&event_loop)?,
    );
    let dragged = window.clone();
    let _webview = WebViewBuilder::new()
        .with_url(&url)
        .with_background_color((0, 0, 0, 255))
        .with_ipc_handler(move |message| {
            if message.body() == "drag" {
                let _ = dragged.drag_window();
            }
        })
        // Links that open a window of their own go to the default browser.
        .with_new_window_req_handler(|target, _| {
            let _ = std::process::Command::new("/usr/bin/open").arg(target).spawn();
            wry::NewWindowResponse::Deny
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500));
        let closed = matches!(event, Event::WindowEvent { event: WindowEvent::CloseRequested, .. });
        if closed || QUIT.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
        }
    });
}
