#![feature(await_macro, futures_api, async_await, integer_atomics)]

use {
    moxie::*,
    moxie_wrench::{
        color::Color,
        position::Position,
        size::Size,
        surface::{CursorMoved, Surface},
    },
};

#[props]
struct SimpleApp;

impl Component for SimpleApp {
    fn compose(scp: Scope, props: Self) {
        let initial_size = Size::new(1920.0, 1080.0);

        let color = state! { scp <- Color::new(0.0, 0.0, 0.3, 1.0) };
        let color_hdl: Handle<Color> = color.handle();

        let (send_mouse_positions, mut mouse_positions): (Sender<CursorMoved>, _) = channel!(scp);

        task! { scp <-
            while let Some(cursor_moved) = await!(mouse_positions.next()) {
                color_hdl.set(|_prev_color| {
                    fun_color_from_mouse_position(initial_size, cursor_moved.position)
                });
            }
        };

        mox! { scp <- Surface { initial_size, send_mouse_positions, background_color: *color } };
    }
}

fn fun_color_from_mouse_position(window_size: Size, pos: Position) -> Color {
    let x_factor = (pos.x / window_size.width).raw() as f32;
    let y_factor = (pos.y / window_size.height).raw() as f32;

    Color::new(x_factor, x_factor, y_factor, y_factor)
}

fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Debug)
        .default_format_timestamp(true)
        .default_format_level(true)
        .default_format_module_path(true)
        .filter(Some("webrender"), log::LevelFilter::Warn)
        .filter(Some("salsa"), log::LevelFilter::Warn)
        .init();
    log::debug!("logger initialized");

    let mut executor = futures::executor::ThreadPool::new().unwrap();
    executor.run(Runtime::go(executor.clone(), SimpleApp));
}