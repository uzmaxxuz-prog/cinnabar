use super::*;
use client_shell::ui_resources::UiRuntimeResource;

use crate::ui_runtime::gameplay_touch::drive_gameplay_touch_targets;
use bevy::input::touch::{TouchInput, TouchPhase};
use client_state::{PlayerRuntime, SemanticInputSnapshot};
use semantic_input::{Action, DeviceFrame, touch};

fn canvas_app() -> App {
    let mut app = App::new();
    app.init_resource::<CanvasCapture>()
        .init_resource::<BrowserOptions>()
        .init_resource::<GameplayTouchGate>()
        .init_resource::<Touches>()
        .init_resource::<semantic_controls::SemanticTouchTargets>()
        .init_resource::<semantic_controls::SemanticInputRuntime>()
        .init_resource::<semantic_controls::PendingDeviceFrame>()
        .init_resource::<client_state::GameplayScreenControls>()
        .init_resource::<client_state::ScreenFocus>()
        .init_resource::<client_state::SemanticInputSnapshot>()
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
        .insert_resource(UiPresentationResource(
            client_ui::test_support::touch_hud_presentation(),
        ))
        .insert_resource(world::ClientWorld {
            stream: Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            })),
            ..Default::default()
        })
        .init_resource::<AccumulatedMouseScroll>();
    app
}

#[test]
fn browser_canvas_capture_gates_raw_hotbar_scroll() {
    let mut app = canvas_app();
    app.add_systems(
            Update,
            (
                prepare_canvas_input,
                sample_input,
                screen_focus,
                client_interaction::hotbar::queue_hotbar_selection,
            )
                .chain(),
        );

    // Capture release and focus loss must block the raw wheel until capture returns.
    for (captured, touch_active, ui_open, loaded, expected) in [
        (false, false, false, true, None),
        (true, false, false, true, Some(1)),
        (false, false, false, true, None),
        (false, true, false, true, None),
        (true, false, false, true, Some(1)),
        (true, false, true, true, None),
        (true, false, false, false, None),
    ] {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        player.inventory.set_local_selected_slot(0);
        let mut ui = client_ui::ui_runtime::UiRuntime::new(1);
        if ui_open {
            ui.open_chat(&mut player);
        }
        app.insert_resource(player)
            .insert_resource(UiRuntimeResource(ui))
            .insert_non_send(InputSource(Box::new(move || SurfaceInputFrame {
                device: DeviceFrame { window_focus_lost: !(captured || touch_active), ..Default::default() },
                touch_capable: touch_active,
                touch_active,
                reset_touch: false,
            })));
        app.world_mut().resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, -1.0);
        if !loaded {
            app.world_mut().resource_mut::<world::ClientWorld>().stream = None;
        }
        app.update();
        let player = app.world().resource::<client_state::PlayerRuntime>();
        assert_eq!(
            player.inventory.pending_hotbar_selection(),
            expected,
            "capture={captured}, ui={ui_open}, loaded={loaded}"
        );
        assert_eq!(player.selected_hotbar_slot(), Some(expected.unwrap_or(0)));
    }
}

/// Finds a control in the published frame, then places the contact at its painted center.
fn touch_center(app: &App, target: u16) -> Vec2 {
    let view = app.world().resource::<UiPresentationResource>();
    let size = [1280.0, 720.0];
    for y in (0..720).step_by(8) {
        for x in (0..1280).step_by(8) {
            if let Some(region) = view.gameplay_touch_region([x as f32 / size[0], y as f32 / size[1]], size)
                && region.hit_id == target
            {
                return Vec2::new(
                    (region.bounds.min[0] + region.bounds.max[0]) * size[0] / 2.,
                    (region.bounds.min[1] + region.bounds.max[1]) * size[1] / 2.,
                );
            }
        }
    }
    panic!("no painted target for {target}");
}

fn gameplay_app() -> (App, Entity) {
    use bevy::input::touch::touch_screen_input_system;
    use semantic_controls::{
        SemanticRouteState, finalize_semantic_input_after_ui_authority,
        route_semantic_input, synchronize_semantic_input_authority,
    };
    let mut app = canvas_app();
    let mut player = PlayerRuntime(player_state::PlayerState::new(1));
    player.inventory.set_local_selected_slot(0);
    player.facts.publish_player_game_mode(protocol::PlayerGameMode::Survival);
    app.insert_resource(player)
        .init_resource::<SemanticRouteState>()
        .init_resource::<client_state::RuntimeSettings>()
        .add_message::<TouchInput>()
        .add_systems(PreUpdate, touch_screen_input_system)
        .add_systems(Update, (
            prepare_canvas_input, drive_gameplay_touch_targets, sample_input, route_semantic_input,
            screen_focus, synchronize_semantic_input_authority, finalize_semantic_input_after_ui_authority,
            client_interaction::hotbar::queue_hotbar_selection,
        ).chain());
    let window = app.world_mut().spawn((Window {
        resolution: (1280, 720).into(),
        focused: true,
        ..Default::default()
    }, PrimaryWindow)).id();
    (app, window)
}

fn surface(app: &mut App, capable: bool, active: bool, reset: bool) {
    app.insert_non_send(InputSource(Box::new(move || SurfaceInputFrame {
        device: DeviceFrame { window_focus_lost: !active || reset, ..Default::default() },
        touch_capable: capable,
        touch_active: active,
        reset_touch: reset,
    })));
}

fn send(app: &mut App, window: Entity, id: u64, phase: TouchPhase, position: Vec2) {
    app.world_mut().write_message(TouchInput { window, id, phase, position, force: None });
}

fn paint_hud(app: &mut App) {
    app.world_mut().resource_scope(|world, mut view: Mut<UiPresentationResource>| {
        view.build(world.resource::<PlayerRuntime>(), world.resource::<UiRuntimeResource>(), 0,
            [1280, 720], ui::DpiScale::new(1.).unwrap()).unwrap();
    });
}

#[test]
fn browser_contacts_route_two_fingers_short_taps_and_reset_reused_ids() {
    let (mut app, window) = gameplay_app();
    surface(&mut app, true, false, false);
    app.update();
    assert!(!app.world().resource::<client_state::ScreenFocus>().absorbs_input);
    paint_hud(&mut app);
    let stick = touch_center(&app, touch::JOYSTICK);
    let jump = touch_center(&app, touch::JUMP);
    let first = touch_center(&app, touch::HOTBAR_1);
    let last = touch_center(&app, touch::HOTBAR_9);
    surface(&mut app, true, true, false);
    send(&mut app, window, 19, TouchPhase::Started, stick);
    send(&mut app, window, 7, TouchPhase::Started, jump);
    send(&mut app, window, 19, TouchPhase::Moved, stick - Vec2::Y * 60.);
    app.update();
    let snapshot = app.world().resource::<SemanticInputSnapshot>();
    assert!(snapshot.movement()[1] > 0.);
    assert!(snapshot.phase(Action::Jump).pressed);
    let targets = app.world().resource::<semantic_controls::SemanticTouchTargets>();
    assert_eq!(targets.target(19), Some(touch::JOYSTICK));
    assert_eq!(targets.target(7), Some(touch::JUMP));
    send(&mut app, window, 7, TouchPhase::Canceled, jump);
    app.update();
    let snapshot = app.world().resource::<SemanticInputSnapshot>();
    assert!(snapshot.movement()[1] > 0.);
    assert!(!snapshot.phase(Action::Jump).held);

    // A backgrounded page may return with the next press queued before the reset was sampled.
    surface(&mut app, true, true, true);
    send(&mut app, window, 19, TouchPhase::Ended, stick);
    send(&mut app, window, 19, TouchPhase::Started, jump);
    app.update();
    assert_eq!(app.world().resource::<Touches>().iter().count(), 0);
    surface(&mut app, true, true, false);
    send(&mut app, window, 19, TouchPhase::Ended, jump);
    app.update();
    let snapshot = app.world().resource::<SemanticInputSnapshot>();
    assert_eq!(snapshot.movement(), [0.; 2]);
    assert!(!snapshot.phase(Action::Jump).pressed);
    for (point, action, slot) in [(last, Action::Hotbar9, 8), (first, Action::Hotbar1, 0)] {
        send(&mut app, window, 19, TouchPhase::Started, point);
        send(&mut app, window, 19, TouchPhase::Ended, point);
        app.update();
        assert!(app.world().resource::<SemanticInputSnapshot>().phase(action).pressed);
        assert_eq!(app.world().resource::<PlayerRuntime>().selected_hotbar_slot(), Some(slot));
    }
}

#[test]
fn browser_first_frame_after_reset_accepts_a_fresh_drag_reusing_the_held_id() {
    let (mut app, window) = gameplay_app();
    surface(&mut app, true, false, false);
    app.update();
    paint_hud(&mut app);
    let stick = touch_center(&app, touch::JOYSTICK);
    let forward = stick - Vec2::Y * 60.0;
    surface(&mut app, true, true, false);
    send(&mut app, window, 19, TouchPhase::Started, stick);
    send(&mut app, window, 19, TouchPhase::Moved, forward);
    app.update();
    assert!(app.world().resource::<SemanticInputSnapshot>().movement()[1] > 0.);
    surface(&mut app, true, false, true);
    app.update();
    assert_eq!(app.world().resource::<SemanticInputSnapshot>().movement(), [0.; 2]);
    surface(&mut app, true, true, false);
    send(&mut app, window, 19, TouchPhase::Started, stick);
    send(&mut app, window, 19, TouchPhase::Moved, forward);
    app.update();
    assert!(app.world().resource::<SemanticInputSnapshot>().movement()[1] > 0.);
}

#[test]
fn browser_first_hybrid_drag_turns_after_the_first_touch_hud_paint() {
    let (mut app, window) = gameplay_app();
    surface(&mut app, false, false, false);
    app.update();
    paint_hud(&mut app);
    let at = Vec2::new(730., 230.);
    assert!(app.world().resource::<UiPresentationResource>()
        .gameplay_touch_region([at.x / 1280., at.y / 720.], [1280., 720.]).is_none());
    surface(&mut app, true, true, false);
    send(&mut app, window, 7, TouchPhase::Started, at);
    app.update();
    paint_hud(&mut app);
    send(&mut app, window, 7, TouchPhase::Moved, at + Vec2::X * 96.);
    app.update();
    assert!(app.world().resource::<SemanticInputSnapshot>().look_delta()[0].abs() > 0.);
}

#[test]
fn browser_deferred_hybrid_start_is_discarded_by_cancel_or_reset() {
    use crate::camera::freeform_pick::FreeformPickPoint;
    for reset in [false, true] {
        let (mut app, window) = gameplay_app();
        app.init_resource::<FreeformPickPoint>();
        let at = Vec2::new(730., 230.);
        surface(&mut app, false, false, false);
        app.update();
        paint_hud(&mut app);
        surface(&mut app, true, true, false);
        send(&mut app, window, 7, TouchPhase::Started, at);
        app.update();
        paint_hud(&mut app);
        if reset {
            surface(&mut app, true, true, true);
        } else {
            send(&mut app, window, 7, TouchPhase::Canceled, at);
        }
        app.update();
        surface(&mut app, true, true, false);
        send(&mut app, window, 7, TouchPhase::Moved, at + Vec2::X * 96.);
        send(&mut app, window, 7, TouchPhase::Ended, at + Vec2::X * 96.);
        for _ in 0..4 {
            app.update();
            let snapshot = app.world().resource::<SemanticInputSnapshot>();
            assert_eq!(snapshot.look_delta(), [0.; 2], "reset={reset}");
            assert!(!snapshot.phase(Action::Use).pressed && !snapshot.phase(Action::Attack).pressed);
            assert_eq!(app.world().resource::<FreeformPickPoint>().0, None);
        }
    }
}

#[test]
fn browser_mouse_lock_hides_touch_controls_on_the_neutral_transition_frame() {
    let (mut app, _) = gameplay_app();
    surface(&mut app, true, true, false);
    app.update();
    assert!(app.world().resource::<UiPresentationResource>().touch_controls_enabled());
    app.insert_non_send(InputSource(Box::new(|| SurfaceInputFrame {
        device: DeviceFrame::default(),
        touch_capable: true,
        touch_active: false,
        reset_touch: true,
    })));
    app.update();
    assert!(!app.world().resource::<UiPresentationResource>().touch_controls_enabled());
    assert_eq!(app.world().resource::<SemanticInputSnapshot>().movement(), [0.; 2]);
}

#[test]
fn browser_draining_touch_keeps_desktop_focus_quarantine() {
    let (mut app, _) = gameplay_app();
    let desktop = |app: &mut App, held: bool| {
        app.insert_non_send(InputSource(Box::new(move || DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                keys: if held { vec![semantic_input::KeyboardKey::KeyW.usage().unwrap()] } else { Vec::new() },
                ..Default::default()
            }),
            ..Default::default()
        }.into())));
    };
    desktop(&mut app, false);
    app.update();
    desktop(&mut app, true);
    app.update();
    assert!(app.world().resource::<SemanticInputSnapshot>().movement()[1] > 0.);
    surface(&mut app, false, false, true);
    app.update();
    desktop(&mut app, true);
    app.update();
    assert_eq!(app.world().resource::<SemanticInputSnapshot>().movement(), [0.; 2]);
    desktop(&mut app, false);
    app.update();
    desktop(&mut app, true);
    app.update();
    assert!(app.world().resource::<SemanticInputSnapshot>().movement()[1] > 0.);
}
