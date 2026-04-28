slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let app = AppWindow::new()?;
    slint::set_xdg_app_id("com.anotherone.SlintPoc")?;
    apply_tokens(&app);
    app.on_close_requested(|| std::process::exit(0));
    app.run()
}

fn apply_tokens(app: &AppWindow) {
    app.set_chrome_bg(argb(0xff, 0x27, 0x29, 0x2e));
    app.set_card_bg(argb(0xff, 0x2b, 0x2d, 0x31));
    app.set_terminal_bg(argb(0xff, 0x17, 0x19, 0x1d));
    app.set_overlay_hover(argb(0x0f, 0xff, 0xff, 0xff));
    app.set_overlay_active(argb(0x1a, 0xff, 0xff, 0xff));
    app.set_focus_ring(argb(0xff, 0x5d, 0x7a, 0xd5));
    app.set_text_primary(argb(0xff, 0xeb, 0xeb, 0xeb));
    app.set_text_secondary(argb(0xff, 0xc7, 0xc7, 0xc7));
    app.set_text_muted(argb(0xff, 0x94, 0x94, 0x94));
}

fn argb(a: u8, r: u8, g: u8, b: u8) -> slint::Color {
    slint::Color::from_argb_u8(a, r, g, b)
}
