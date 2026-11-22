//! Publishes canvas samples to the shared gameplay input owners.
use crate::{runtime::world, semantic_controls};
use crate::ui_runtime::gameplay_touch::GameplayTouchGate;
use bevy::{
    input::{mouse::AccumulatedMouseScroll, touch::Touches},
    prelude::*,
    window::PrimaryWindow,
};
use client_shell::ui_resources::UiPresentationResource;
use semantic_input::SurfaceInputFrame;

/// In-memory settings used by gameplay until a menu host replaces them.
#[derive(Resource, Default)]
pub(crate) struct BrowserOptions(
    pub(crate) std::sync::Arc<launcher::menu::settings_options::SettingsOptions>,
);

/// Canvas observations owned by the DOM edge, sampled once per game update.
pub(crate) struct InputSource(pub Box<dyn FnMut() -> SurfaceInputFrame>);

/// Raw observations for this update, along with the canvas's gameplay context.
#[derive(Resource, Default)]
pub(crate) struct CanvasCapture {
    surface: SurfaceInputFrame,
    gameplay_available: bool,
}

/// Stage one DOM sample before the native touch driver consumes this update's contacts.
pub(crate) fn prepare_canvas_input(
    mut source: NonSendMut<InputSource>,
    mut capture: ResMut<CanvasCapture>,
    mut gate: ResMut<GameplayTouchGate>,
    mut presentation: ResMut<UiPresentationResource>,
    mut scroll: ResMut<AccumulatedMouseScroll>,
    world: Res<world::ClientWorld>,
) {
    let surface = (source.0)();
    let mouse = !surface.device.window_focus_lost && !surface.touch_active && !surface.reset_touch;
    let touch_hud = surface.touch_capable && !mouse;
    presentation.set_touch_gameplay_only(true);
    presentation.enable_touch_controls(touch_hud);
    gate.0 = touch_hud && surface.touch_active && !surface.reset_touch
        && !surface.device.window_focus_lost && world.stream.is_some()
        && presentation.loading_stage().is_none();
    if !mouse {
        scroll.delta = Vec2::ZERO;
    }
    // Establish gameplay authority before the first finger lands. The gate still owns admission.
    capture.gameplay_available = mouse || touch_hud;
    capture.surface = surface;
}

/// Publishes UI ownership and canvas capture for every gameplay input consumer.
pub(crate) fn screen_focus(
    capture: Res<CanvasCapture>,
    player: Res<client_state::PlayerRuntime>,
    ui: Res<client_shell::ui_resources::UiRuntimeResource>,
    presentation: Res<client_shell::ui_resources::UiPresentationResource>,
    world: Res<world::ClientWorld>,
    mut focus: ResMut<client_state::ScreenFocus>,
) {
    let focused =
        ui.ui_focused(&player) || presentation.loading_stage().is_some() || world.stream.is_none();
    focus.set_if_neq(client_state::ScreenFocus {
        ui_focused: focused,
        absorbs_input: focused || !capture.gameplay_available,
        steals_mouse: Some(focused),
        ..Default::default()
    });
}

/// Inputs collected by the shared native touch driver earlier in this update.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CanvasTouchInputs<'w, 's> {
    gate: Res<'w, GameplayTouchGate>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    touches: Res<'w, Touches>,
    targets: ResMut<'w, semantic_controls::SemanticTouchTargets>,
}

/// Merges the native contact stream into the staged browser sample.
pub(crate) fn sample_input(
    mut capture: ResMut<CanvasCapture>,
    mut touch: CanvasTouchInputs,
    mut pending: ResMut<semantic_controls::PendingDeviceFrame>,
    world: Res<world::ClientWorld>,
    options: Res<BrowserOptions>,
    mut controls: ResMut<client_state::GameplayScreenControls>,
) {
    let surface = std::mem::take(&mut capture.surface);
    let mut frame = surface.device;
    if surface.reset_touch || world.stream.is_none() || (surface.touch_active && !touch.gate.0) {
        frame = semantic_input::DeviceFrame { window_focus_lost: true, ..Default::default() };
    } else if touch.gate.0 && let Ok(window) = touch.windows.single() {
        (frame.touches, _) = semantic_controls::physical::collect_touch_contacts(
            window, &touch.touches, &mut touch.targets,
        );
    }
    let pressed = |control| match control {
        semantic_input::PhysicalControl::KeyboardUsage(key) => frame
            .keyboard_mouse
            .as_ref()
            .is_some_and(|keyboard| keyboard.key_edges.pressed.contains(&key)),
        semantic_input::PhysicalControl::MouseButton(button) => frame
            .keyboard_mouse
            .as_ref()
            .is_some_and(|keyboard| keyboard.mouse_edges.pressed.contains(&button)),
        _ => false,
    };
    let binding = launcher::menu::settings_options::BindingId::PickItem;
    controls.pick_block_pressed = !frame.window_focus_lost
        && [
            options.0.named_key_control(binding),
            options.0.secondary_key_control(binding),
        ]
        .into_iter()
        .flatten()
        .any(pressed);
    controls.hide_hud = options
        .0
        .value(launcher::menu::settings_options::SettingId::HideHud)
        != 0;
    pending.replace(frame);
}

#[cfg(test)]
mod tests;
