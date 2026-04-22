use gpui::{img, prelude::*, px, svg, AnyElement, Hsla, SharedString};

pub(crate) fn branded_icon(
    path: impl Into<SharedString>,
    size_px: f32,
    tint: Option<Hsla>,
) -> AnyElement {
    let path: SharedString = path.into();

    if is_raster_icon(path.as_ref()) {
        img(path).w(px(size_px)).h(px(size_px)).into_any_element()
    } else {
        let icon = svg().path(path).size(px(size_px));
        match tint {
            Some(color) => icon.text_color(color).into_any_element(),
            None => icon.into_any_element(),
        }
    }
}

fn is_raster_icon(path: &str) -> bool {
    matches!(
        path.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase()),
        Some(extension)
            if matches!(
                extension.as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
            )
    )
}
