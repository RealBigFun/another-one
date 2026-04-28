use makepad_widgets::*;

live_design! {
    use link::theme::*;
    use link::widgets::*;

    COLOR_CHROME = #x27292eFF
    COLOR_CARD = #x2b2d31FF
    COLOR_TERMINAL = #x17191dFF
    COLOR_HOVER = #x36383dFF
    COLOR_ACTIVE = #x404248FF
    COLOR_FOCUS = #x5d7ad5FF
    COLOR_TEXT = #xebebebFF
    COLOR_TEXT_MUTED = #x949494FF

    App = {{App}} {
        ui: <Root> {
            main_window = <Window> {
                window: {inner_size: vec2(1180, 760), title: "AnotherOne Makepad POC"}
                body = <View> {
                    width: Fill, height: Fill
                    flow: Overlay
                    show_bg: true
                    draw_bg: {color: (COLOR_TERMINAL)}

                    chrome = <View> {
                        width: Fill, height: Fill
                        flow: Down

                        titlebar = <View> {
                            width: Fill, height: 44
                            padding: {left: 18, right: 18, top: 8, bottom: 8}
                            flow: Right
                            spacing: 12
                            show_bg: true
                            draw_bg: {color: (COLOR_CHROME)}

                            title = <Label> {
                                width: Fill, height: Fit
                                text: "AnotherOne / Makepad POC"
                                draw_text: {color: (COLOR_TEXT), text_style: {font_size: 10.5}}
                            }

                            close_button = <Button> {
                                width: 36, height: 28
                                text: "x"
                            }
                        }

                        content = <View> {
                            width: Fill, height: Fill
                            flow: Right

                            sidebar = <View> {
                                width: 296, height: Fill
                                padding: {left: 12, right: 12, top: 16}
                                spacing: 8
                                flow: Down
                                show_bg: true
                                draw_bg: {color: (COLOR_CHROME)}

                                sidebar_heading = <Label> {
                                    width: Fill, height: 24
                                    text: "PROJECTS"
                                    draw_text: {color: (COLOR_TEXT_MUTED), text_style: {font_size: 9.0}}
                                }

                                row_active = <View> {
                                    width: Fill, height: 58
                                    padding: {left: 12, top: 9}
                                    flow: Down
                                    show_bg: true
                                    draw_bg: {color: (COLOR_ACTIVE)}
                                    <Label> {text: "appimage-daemon", draw_text: {color: (COLOR_TEXT), text_style: {font_size: 11.0}}}
                                    <Label> {text: "daemon-transport-foundation", draw_text: {color: (COLOR_TEXT_MUTED), text_style: {font_size: 9.0}}}
                                }

                                row_hover = <View> {
                                    width: Fill, height: 58
                                    padding: {left: 12, top: 9}
                                    flow: Down
                                    show_bg: true
                                    draw_bg: {color: (COLOR_HOVER)}
                                    <Label> {text: "another-one", draw_text: {color: (COLOR_TEXT), text_style: {font_size: 11.0}}}
                                    <Label> {text: "poc/slint-makepad-ui-eval", draw_text: {color: (COLOR_TEXT_MUTED), text_style: {font_size: 9.0}}}
                                }

                                row_idle = <View> {
                                    width: Fill, height: 58
                                    padding: {left: 12, top: 9}
                                    flow: Down
                                    show_bg: true
                                    draw_bg: {color: (COLOR_CHROME)}
                                    <Label> {text: "mobile-eval", draw_text: {color: (COLOR_TEXT), text_style: {font_size: 11.0}}}
                                    <Label> {text: "iroh-only terminal baseline", draw_text: {color: (COLOR_TEXT_MUTED), text_style: {font_size: 9.0}}}
                                }
                            }

                            terminal_fixture = <View> {
                                width: Fill, height: Fill
                                padding: {left: 28, top: 28}
                                spacing: 10
                                flow: Down
                                show_bg: true
                                draw_bg: {color: (COLOR_TERMINAL)}
                                <Label> {text: "$ cargo run -p makepad-poc", draw_text: {color: (COLOR_TEXT), text_style: {font_size: 10.0}}}
                                <Label> {text: "fixture grid: iroh + alacritty_terminal wiring lands after chrome parity", draw_text: {color: (COLOR_TEXT_MUTED), text_style: {font_size: 9.0}}}
                            }
                        }
                    }

                    modal_scrim = <View> {
                        width: Fill, height: Fill
                        align: {x: 0.5, y: 0.5}
                        show_bg: true
                        draw_bg: {color: #x00000080}

                        modal_card = <View> {
                            width: 430, height: 250
                            padding: {left: 24, right: 24, top: 22, bottom: 20}
                            spacing: 12
                            flow: Down
                            show_bg: true
                            draw_bg: {color: (COLOR_CARD)}

                            <Label> {text: "New task", draw_text: {color: (COLOR_TEXT), text_style: {font_size: 15.0}}}
                            <Label> {text: "Fixture modal used to compare form ergonomics.", draw_text: {color: (COLOR_TEXT_MUTED), text_style: {font_size: 9.0}}}
                            task_name = <TextInput> {width: Fill, height: 36, empty_message: "Task name"}
                            branch_name = <TextInput> {width: Fill, height: 36, empty_message: "Branch name"}
                            create_button = <Button> {width: 104, height: 34, text: "Create"}
                        }
                    }
                }
            }
        }
    }
}

app_main!(App);

#[derive(Live, LiveHook)]
pub struct App {
    #[live]
    ui: WidgetRef,
}

impl LiveRegister for App {
    fn live_register(cx: &mut Cx) {
        makepad_widgets::live_design(cx);
    }
}

impl AppMain for App {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.ui.handle_event(cx, event, &mut Scope::empty());
    }
}
