use crate::error::Result;
use gpui::{
    Application, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, prelude::*,
    px, rgb, size,
};

pub fn run(width: u32, height: u32) -> Result<()> {
    log::info!("Launching workbench with dimensions: {}x{}", width, height);

    Application::new().run(move |cx| {
        // Initialize gpui-component
        gpui_component::init(cx);

        // Calculate window bounds
        let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);

        // Open main window
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Group Vibe Workbench".into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| WorkbenchView {}),
        )
        .expect("Failed to open window");

        // Activate the application
        cx.activate(true);
    });

    Ok(())
}

struct WorkbenchView {}

impl Render for WorkbenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e2e))
            // Top Menu Bar
            .child(
                div()
                    .flex()
                    .h(px(40.0))
                    .w_full()
                    .bg(rgb(0x313244))
                    .border_b_1()
                    .border_color(rgb(0x45475a))
                    .px_4()
                    .items_center()
                    .gap_4()
                    .child(menu_item("File"))
                    .child(menu_item("Edit"))
                    .child(menu_item("View"))
                    .child(menu_item("Help")),
            )
            // Main Content Area with WebView placeholder
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .p_8()
                    .child(
                        div()
                            .w(px(800.0))
                            .h(px(500.0))
                            .bg(rgb(0x313244))
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(0x45475a))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_2xl()
                                    .text_color(rgb(0xcdd6f4))
                                    .child("🌐 WebView Area")
                            )
                            .child(
                                div()
                                    .text_color(rgb(0xbac2de))
                                    .child("WebView will be integrated here")
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .px_4()
                                    .py_2()
                                    .bg(rgb(0x45475a))
                                    .rounded_md()
                                    .text_color(rgb(0xa6e3a1))
                                    .child("✅ Layout Complete")
                            ),
                    ),
            )
    }
}

fn menu_item(label: &str) -> impl IntoElement {
    div()
        .px_3()
        .py_1()
        .rounded_sm()
        .text_color(rgb(0xcdd6f4))
        .hover(|style| style.bg(rgb(0x45475a)))
        .cursor_pointer()
        .child(label.to_string())
}
