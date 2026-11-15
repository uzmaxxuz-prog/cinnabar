//! Real wgpu browser entry point; JavaScript observes descriptors, never recreates them.
use wasm_bindgen::prelude::*;

/// Runs the actual renderer allocators on the browser's requested device.
/// The compatibility runner removes core capability advertisement before this entry point.
#[wasm_bindgen]
pub async fn run_texture_fixture(mode: String) -> Result<String, JsValue> {
    console_error_panic_hook::set_once();
    if mode != "core" && mode != "compat" {
        return Err(JsValue::from_str("mode must be core or compat"));
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let compatibility = render::device_settings::webgpu_compatibility(&adapter);
    if compatibility != (mode == "compat") {
        return Err(JsValue::from_str(
            "actual adapter mode differs from requested fixture mode",
        ));
    }
    let flags = adapter.get_downlevel_capabilities().flags;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("production texture browser fixture"),
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let report =
        render::texture_compat_diagnostics::production_arrays(&device, &queue, compatibility).await;
    Ok(serde_json::json!({
        "mode":mode,
        "adapter":{
            "compatibility":compatibility,
            "downlevelFlags":format!("{flags:?}"),
            "viewFormats":flags.contains(wgpu::DownlevelFlags::VIEW_FORMATS),
            "cubeArrayTextures":flags.contains(wgpu::DownlevelFlags::CUBE_ARRAY_TEXTURES),
            "surfaceViewFormats":flags.contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        },
        "tests":report["tests"],
        "descriptorExpectations":[
            {"label":"dropped item texture layers","viewDimension":"2d-array"},
            {"label":"bounded UI dimension bucket","viewDimension":"2d-array"},
            {"label":"none-array-negative-control","viewDimension":null},
            {"label":"array sample readback target","viewDimension":null}
        ],
        "pixelComparisons":report["pixelComparisons"]
    })
    .to_string())
}
