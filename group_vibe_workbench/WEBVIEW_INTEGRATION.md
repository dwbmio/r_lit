# GPUI + WebView Integration Example

This file contains a complete example of GPUI + WebView integration for future reference.

## Current Status

The project is configured with:
- ✅ gpui-component 0.5.1 (with webview feature)
- ✅ gpui 0.2.2
- ✅ All dependencies installed
- ✅ Metal Toolchain installed

## API Version Issue

`gpui-component 0.5.1` uses `gpui 0.2.2`, which has different APIs than the latest GPUI from Zed's repository.

## Working Example (Conceptual)

```rust
use gpui_component::{Root, webview::WebView, wry::WebViewBuilder};

// This is the conceptual approach - actual API may differ
// Check gpui-component source code for exact API

struct WorkbenchView {
    webview: Entity<WebView>,
}

impl WorkbenchView {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        // 1. Get raw window handle
        let raw_handle = window.raw_window_handle();

        // 2. Create Wry WebView
        let wry_webview = WebViewBuilder::new()
            .with_html("<h1>Hello from WebView!</h1>")
            .build_as_child(&raw_handle)
            .expect("Failed to create WebView");

        // 3. Wrap in gpui-component WebView
        let webview = cx.new_entity(|cx| {
            WebView::new(wry_webview, window, cx)
        });

        Self { webview }
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Root::new(self.view(), window, cx)
            .child(self.webview.clone())
    }
}
```

## Next Steps

1. **Study gpui-component examples**: Check if there are examples in the repository
2. **Match API versions**: Ensure all API calls match gpui 0.2.2
3. **Test incrementally**: Start with simple window, then add WebView
4. **JavaScript communication**: Use `webview.evaluate_script()` for Rust → JS
5. **Custom protocols**: Use Wry's custom protocol handler for JS → Rust

## Resources

- gpui-component: https://github.com/longbridge/gpui-component
- gpui-component docs: https://lib.rs/crates/gpui-component
- Wry documentation: https://docs.rs/wry/
- GPUI guide: https://typevar.dev/articles/longbridge/gpui-component

## HTML Template

See `src/subcmd/launch.rs` for a complete HTML template with:
- Modern gradient design
- Feature cards
- Responsive layout
- JavaScript integration example
