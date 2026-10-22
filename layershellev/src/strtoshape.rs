use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;

pub(crate) fn str_to_shape(shape_name: &str) -> Option<Shape> {
    match shape_name {
        "default" => Some(Shape::Default),
        "contenx_menu" => Some(Shape::ContextMenu),
        "help" => Some(Shape::Help),
        "pointer" => Some(Shape::Pointer),
        "progress" => Some(Shape::Progress),
        "wait" => Some(Shape::Wait),
        "cell" => Some(Shape::Cell),
        "crosshair" => Some(Shape::Crosshair),
        "text" => Some(Shape::Text),
        "vertical_text" => Some(Shape::VerticalText),
        "alias" => Some(Shape::Alias),
        "copy" => Some(Shape::Copy),
        "move" => Some(Shape::Move),
        "no_drop" => Some(Shape::NoDrop),
        "not_allowed" => Some(Shape::NotAllowed),
        "grab" => Some(Shape::Grab),
        "grabbing" => Some(Shape::Grabbing),
        "e_resize" => Some(Shape::EResize),
        "n_resize" => Some(Shape::NResize),
        "ne_resize" => Some(Shape::NeResize),
        "nw_resize" => Some(Shape::NwResize),
        "s_resize" => Some(Shape::SResize),
        "se_resize" => Some(Shape::SeResize),
        "sw_resize" => Some(Shape::SwResize),
        "w_resize" => Some(Shape::WResize),
        "ew_resize" => Some(Shape::EwResize),
        "ns_resize" => Some(Shape::NsResize),
        "nesw_resize" => Some(Shape::NeswResize),
        "nwse_resize" => Some(Shape::NwseResize),
        "col_resize" => Some(Shape::ColResize),
        "row_resize" => Some(Shape::RowResize),
        "all_scroll" => Some(Shape::AllScroll),
        "zoom_in" => Some(Shape::ZoomIn),
        "zoom_out" => Some(Shape::ZoomOut),
        _ => None,
    }
}

pub trait ShapeName {
    fn name(&self) -> &str;
}

impl ShapeName for Shape {
    fn name(&self) -> &str {
        match self {
            Self::Default => "default",
            Self::ContextMenu => "contenx_menu",
            Self::Help => "help",
            Self::Pointer => "pointer",
            Self::Progress => "progress",
            Self::Wait => "wait",
            Self::Cell => "cell",
            Self::Crosshair => "crosshair",
            Self::Text => "text",
            Self::VerticalText => "vertical_text",
            Self::Alias => "alias",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::NoDrop => "no_drop",
            Self::NotAllowed => "not_allowed",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::EResize => "e_resize",
            Self::NResize => "n_resize",
            Self::NeResize => "ne_resize",
            Self::EwResize => "ew_resize",
            Self::NwResize => "nw_resize",
            Self::SResize => "s_resize",
            Self::SeResize => "se_resize",
            Self::SwResize => "sw_resize",
            Self::WResize => "w_resize",
            Self::NsResize => "ns_resize",
            Self::NeswResize => "nesw_resize",
            Self::NwseResize => "nwse_resize",
            Self::ColResize => "col_resize",
            Self::RowResize => "row_resize",
            Self::AllScroll => "all_scroll",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            _ => "default",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_name_reads_back_as_the_same_shape() {
        let shapes = [
            Shape::Default,
            Shape::ContextMenu,
            Shape::Help,
            Shape::Pointer,
            Shape::Progress,
            Shape::Wait,
            Shape::Cell,
            Shape::Crosshair,
            Shape::Text,
            Shape::VerticalText,
            Shape::Alias,
            Shape::Copy,
            Shape::Move,
            Shape::NoDrop,
            Shape::NotAllowed,
            Shape::Grab,
            Shape::Grabbing,
            Shape::EResize,
            Shape::NResize,
            Shape::NeResize,
            Shape::NwResize,
            Shape::SResize,
            Shape::SeResize,
            Shape::SwResize,
            Shape::WResize,
            Shape::EwResize,
            Shape::NsResize,
            Shape::NeswResize,
            Shape::NwseResize,
            Shape::ColResize,
            Shape::RowResize,
            Shape::AllScroll,
            Shape::ZoomIn,
            Shape::ZoomOut,
        ];
        for shape in shapes {
            assert_eq!(str_to_shape(shape.name()), Some(shape), "{}", shape.name());
        }
    }
}
