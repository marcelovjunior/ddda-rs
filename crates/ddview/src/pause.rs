//! Viewer-only pause UI and cursor ownership, not the original game's menu.
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

#[derive(Resource, Default)]
pub struct PauseState {
    pub paused: bool,
    enabled: bool,
    /// Explicit unattended demos keep running when another viewer gets focus.
    auto_pause: bool,
}

impl PauseState {
    /// Automated captures never take ownership of the maintainer's mouse.
    pub fn mouse_look(&self, right_held: bool) -> bool {
        self.auto_pause || right_held
    }
}

#[derive(Component)]
pub struct PauseMenu;
#[derive(Component, Clone, Copy)]
pub enum Action {
    Resume,
    Quit,
}

#[allow(dead_code)] // Also used by the opening viewer integration.
pub fn playing(pause: Res<PauseState>) -> bool {
    !pause.paused
}

pub fn animate_world(pause: Res<PauseState>, net: Option<Res<crate::net::NetRes>>) -> bool {
    !pause.paused || net.is_some()
}

pub fn setup(mut commands: Commands, args: Res<crate::Args>, mut pause: ResMut<PauseState>) {
    pause.enabled = args.play;
    pause.auto_pause = !(args.unattended || args.demo || args.shot.is_some() || args.shots.is_some());
    pause.paused = args.paused;
    if !args.play {
        return;
    }
    commands
        .spawn((
            PauseMenu,
            GlobalZIndex(100),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.85)),
        ))
        .with_children(|c| {
            c.spawn((
                Text::new(if args.host.is_some() || args.join.is_some() {
                    "Viewer menu — network session continues"
                } else {
                    "Paused — viewer menu"
                }),
                TextFont {
                    font_size: bevy::text::FontSize::Px(28.0),
                    ..default()
                },
            ));
            for (label, action) in [("Resume (Esc)", Action::Resume), ("Quit", Action::Quit)] {
                c.spawn((
                    Button,
                    action,
                    Node {
                        padding: UiRect::axes(Val::Px(32.0), Val::Px(16.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.15, 0.2, 0.28)),
                ))
                .with_children(|b| {
                    b.spawn((
                        Text::new(label),
                        TextFont {
                            font_size: bevy::text::FontSize::Px(22.0),
                            ..default()
                        },
                    ));
                });
            }
        });
}

pub fn update(
    mut pause: ResMut<PauseState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut window: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut menus: Query<&mut Node, With<PauseMenu>>,
    mut buttons: Query<(&Interaction, &Action, &mut BackgroundColor), With<Button>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !pause.enabled {
        return;
    }
    let Ok((window, mut cursor)) = window.single_mut() else {
        return;
    };
    let was_paused = pause.paused;
    if !window.focused && pause.auto_pause {
        pause.paused = true;
    } else if keys.just_pressed(KeyCode::Escape) {
        pause.paused = !pause.paused;
    }
    for (interaction, action, mut color) in &mut buttons {
        *color = BackgroundColor(if *interaction == Interaction::Hovered {
            Color::srgb(0.25, 0.35, 0.48)
        } else {
            Color::srgb(0.15, 0.2, 0.28)
        });
        if was_paused && *interaction == Interaction::Pressed {
            match action {
                Action::Resume => pause.paused = false,
                Action::Quit => {
                    exit.write(AppExit::Success);
                }
            }
        }
    }
    if pause.paused != was_paused {
        // A menu click must not become an attack; held movement must not
        // survive losing focus or resuming the viewer.
        keys.reset_all();
        mouse.reset_all();
    }
    cursor.visible = pause.paused || !pause.auto_pause;
    cursor.grab_mode = if pause.paused || !pause.auto_pause {
        CursorGrabMode::None
    } else {
        CursorGrabMode::Locked
    };
    for mut menu in &mut menus {
        menu.display = if pause.paused {
            Display::Flex
        } else {
            Display::None
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_focus_and_buttons_control_pause_and_cursor() {
        let mut app = App::new();
        app.insert_resource(PauseState {
            enabled: true,
            paused: false,
            auto_pause: true,
        })
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<AppExit>()
        .add_systems(Update, update);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                CursorOptions::default(),
                PrimaryWindow,
            ))
            .id();
        let menu = app.world_mut().spawn((PauseMenu, Node::default())).id();
        let resume = app
            .world_mut()
            .spawn((
                Button,
                Action::Resume,
                Interaction::None,
                BackgroundColor::default(),
            ))
            .id();
        let quit = app
            .world_mut()
            .spawn((
                Button,
                Action::Quit,
                Interaction::None,
                BackgroundColor::default(),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            CursorGrabMode::Locked
        );
        assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<PauseState>().paused);
        assert_eq!(app.world().get::<Node>(menu).unwrap().display, Display::Flex);
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            CursorGrabMode::None
        );
        assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<PauseState>().paused);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert!(app.world().resource::<PauseState>().paused);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        *app.world_mut().get_mut::<Interaction>(resume).unwrap() = Interaction::Pressed;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(!app.world().resource::<PauseState>().paused);
        assert!(!app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left));
        *app.world_mut().get_mut::<Interaction>(resume).unwrap() = Interaction::None;
        app.world_mut().resource_mut::<PauseState>().auto_pause = false;
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert!(
            !app.world().resource::<PauseState>().paused,
            "unattended demo ignores focus loss"
        );
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            CursorGrabMode::None
        );
        assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
        assert!(!app.world().resource::<PauseState>().mouse_look(false));
        assert!(app.world().resource::<PauseState>().mouse_look(true));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(
            app.world().resource::<PauseState>().paused,
            "Esc still pauses an unattended demo"
        );

        app.world_mut().resource_mut::<PauseState>().paused = true;
        *app.world_mut().get_mut::<Interaction>(quit).unwrap() = Interaction::Pressed;
        app.update();
        assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
    }
}
