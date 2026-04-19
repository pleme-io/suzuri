mod config;
mod errors;
mod input;
mod pty;
mod renderer;
mod terminal;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use config::Config;
use errors::Result;
use pty::{Pty, PtyEvent};
use renderer::Renderer;
use terminal::Terminal;

/// Application state, constructed after the event loop starts.
struct App {
    window: Arc<Window>,
    renderer: Renderer,
    terminal: Terminal,
    pty: Pty,
    pty_rx: std::sync::mpsc::Receiver<PtyEvent>,
    vte_parser: vte::Parser,
    config: Config,
    config_store: shikumi::ConfigStore<Config>,
    should_close: bool,
}

/// Handler for winit's `ApplicationHandler` trait. Manages window lifecycle.
struct AppHandler {
    app: Option<App>,
    config_store: Option<shikumi::ConfigStore<Config>>,
}

impl AppHandler {
    fn new() -> Result<Self> {
        let config_store = config::load_config()?;
        Ok(Self {
            app: None,
            config_store: Some(config_store),
        })
    }
}

impl ApplicationHandler for AppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_some() {
            return;
        }

        let config_store = self.config_store.take().expect("config_store consumed twice");
        let config = config_store.get().clone();

        let window_attrs = WindowAttributes::default()
            .with_title(&config.window.title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                config.window.width,
                config.window.height,
            ));

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("Failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };

        let renderer = match pollster::block_on(Renderer::new(window.clone(), &config)) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to create renderer: {e}");
                event_loop.exit();
                return;
            }
        };

        let (cols, rows) = renderer.grid_size();
        let terminal = Terminal::new(cols, rows, config.terminal.scrollback_lines);

        let (pty, pty_rx) = match Pty::spawn(
            &config.shell.program,
            &config.shell.args,
            cols as u16,
            rows as u16,
        ) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to spawn PTY: {e}");
                event_loop.exit();
                return;
            }
        };

        tracing::info!(
            "Suzuri started: {}x{} grid, shell={}",
            cols,
            rows,
            config.shell.program
        );

        self.app = Some(App {
            window,
            renderer,
            terminal,
            pty,
            pty_rx,
            vte_parser: vte::Parser::new(),
            config: config.as_ref().clone(),
            config_store,
            should_close: false,
        });

        // Request first redraw
        if let Some(app) = &self.app {
            app.window.request_redraw();
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            if let Some(app) = &self.app {
                app.window.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(app) = self.app.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                app.renderer.resize(size.width, size.height);
                let (cols, rows) = app.renderer.grid_size();
                if cols != app.terminal.cols || rows != app.terminal.rows {
                    app.terminal.resize(cols, rows);
                    if let Err(e) = app.pty.resize(cols as u16, rows as u16) {
                        tracing::warn!("PTY resize failed: {e}");
                    }
                    tracing::debug!("Resized to {cols}x{rows}");
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(bytes) =
                    input::key_to_pty_bytes(&event.logical_key, event.state, &Default::default())
                {
                    if let Err(e) = app.pty.write(&bytes) {
                        tracing::warn!("PTY write failed: {e}");
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                // Drain PTY output
                while let Ok(event) = app.pty_rx.try_recv() {
                    match event {
                        PtyEvent::Output(data) => {
                            app.vte_parser.advance(&mut app.terminal, &data);
                        }
                        PtyEvent::Exit(code) => {
                            tracing::info!("Shell exited with code {code}");
                            app.should_close = true;
                        }
                    }
                }

                if app.should_close {
                    event_loop.exit();
                    return;
                }

                // Check for config hot-reload via ArcSwap generation
                {
                    let new_config = app.config_store.get();
                    let new_ref: &Config = &new_config;
                    // Compare font size as a cheap change-detection heuristic
                    if (new_ref.font.size - app.config.font.size).abs() > f32::EPSILON
                        || new_ref.font.family != app.config.font.family
                        || new_ref.window.padding != app.config.window.padding
                    {
                        app.config = new_ref.clone();
                        app.renderer.update_config(&app.config);
                        tracing::info!("Config reloaded");
                    }
                }

                // Render
                if let Err(e) = app.renderer.render(&app.terminal) {
                    tracing::error!("Render error: {e}");
                }
                app.terminal.dirty = false;

                // Schedule next frame (~120 FPS when dirty, lower when idle)
                let next = Instant::now() + Duration::from_millis(8);
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            }

            _ => {}
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(
            fmt::layer()
                .with_level(true)
                .with_line_number(true)
                .with_file(true)
                .with_target(true)
                .with_writer(std::io::stderr)
                .compact(),
        )
        .init();

    let event_loop = EventLoop::new().map_err(|e| errors::SuzuriError::Window(e.to_string()))?;
    let mut handler = AppHandler::new()?;

    event_loop
        .run_app(&mut handler)
        .map_err(|e| errors::SuzuriError::Window(e.to_string()))?;

    Ok(())
}
