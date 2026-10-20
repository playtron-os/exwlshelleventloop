use iced_layershell::to_layer_message;

#[test]
fn test_layer_message_macro() {
    #[to_layer_message]
    #[derive(Debug, Clone)]
    enum TestEnum {
        TestA,
    }
    let e = TestEnum::SizeChange((10, 10));
    let _ = e.clone();
}

#[test]
fn test_layer_message_macro_multi() {
    #[to_layer_message(multi)]
    #[derive(Debug, Clone)]
    enum TestEnum {
        TestA,
    }
    use layershellev::*;
    let (_id, _message) = TestEnum::layershell_open(NewLayerShellSettings::default());
}

#[test]
fn drag_icon_change_reaches_the_drag_not_a_window() {
    use iced_layershell::actions::{
        DragIcon, LayershellCustomAction, LayershellCustomActionWithId,
    };
    #[to_layer_message(multi)]
    #[derive(Debug, Clone)]
    enum TestEnum {
        TestA,
    }
    let icon = DragIcon {
        width: 2,
        height: 1,
        scale: 2,
        pixels: vec![0; 8],
    };
    let action: LayershellCustomActionWithId = TestEnum::DragIconChange(icon).try_into().unwrap();
    let LayershellCustomActionWithId(None, LayershellCustomAction::DragIconChange(icon)) = action
    else {
        panic!("a drag icon is not any window's: {action:?}");
    };
    assert_eq!((icon.width, icon.height, icon.pixels.len()), (2, 1, 8));
    assert!(
        !format!("{icon:?}").contains("pixels"),
        "the pixels stay out of logs"
    );
}
