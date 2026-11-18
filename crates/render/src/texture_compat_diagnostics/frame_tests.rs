//! Contract between the production array diagnostics and the QA capture lane.
//!
//! The QA driver accepts a case only when its record carries the whole mapped target range, so
//! every sampled case in this report publishes one frame whose byte length is exactly
//! `stride * height`. The negative control proves a validation error and stores no pixels, so it
//! publishes no frame.

use serde_json::{Value, json};

use crate::texture_compat_diagnostics::production_arrays;

/// Authored oracle for the sRGB transfer path, computed from the atlas texel `128,64,32` that
/// `textureLoad` decodes before the `Rgba8Unorm` witness target stores it:
/// `srgb_to_linear(128/255) = 0.21596 -> 55`, `(64/255) = 0.05130 -> 13`, `(32/255) = 0.01445 ->
/// 3.69 -> 4`, alpha undecoded.
const DROPPED_ITEM_ORACLE: [u8; 4] = [55, 13, 4, 255];

/// Authored oracle for the path with no decode: the UI bucket is an `Rgba8Unorm` array, so the
/// texel written into it reaches the witness target unchanged.
const UI_ORACLE: [u8; 4] = [128, 64, 32, 255];

/// Both production transfer paths publish a driver-shaped frame with the pixels they sampled.
#[test]
fn production_arrays_publish_frame_bytes_for_every_sampled_case() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("production array frame bytes") else {
        return;
    };
    let report = bevy::tasks::block_on(production_arrays(&gpu.device, &gpu.queue, false));
    let frames = report
        .get("frames")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "production_arrays publishes no `frames` array; report keys: {:?}",
                report
                    .as_object()
                    .map(|report| report.keys().cloned().collect::<Vec<_>>())
            )
        });
    assert_eq!(
        frames.len(),
        2,
        "one frame for each sampled case and none for the negative control: {frames:?}"
    );
    for (id, oracle) in [
        ("dropped-item-one-layer", DROPPED_ITEM_ORACLE),
        ("ui-one-layer", UI_ORACLE),
    ] {
        let frame = frames
            .iter()
            .find(|frame| frame["id"] == json!(id))
            .unwrap_or_else(|| {
                panic!(
                    "no frame for {id}; published ids: {:?}",
                    frames
                        .iter()
                        .map(|frame| frame["id"].clone())
                        .collect::<Vec<_>>()
                )
            });
        let width = frame["width"].as_u64().expect("frame width");
        let height = frame["height"].as_u64().expect("frame height");
        let stride = frame["stride"].as_u64().expect("frame stride");
        assert_eq!(
            (width, height),
            (1, 1),
            "{id}: the witness target is one pixel"
        );
        assert_eq!(
            stride, 256,
            "{id}: the stride is the mapped readback row, not the pixel width"
        );
        assert_eq!(frame["format"], json!("rgba8"), "{id}: witness format");
        assert_eq!(frame["origin"], json!("top-left"), "{id}: frame origin");
        assert_eq!(
            frame["transfer"],
            json!("linear"),
            "{id}: a UNORM witness target stores the linear value it sampled"
        );
        assert_eq!(frame["alphaMode"], json!("straight"), "{id}: alpha mode");
        assert_eq!(
            frame["premultiplySpace"],
            json!("none"),
            "{id}: premultiply space"
        );
        let bytes = frame["bytes"]
            .as_array()
            .and_then(|bytes| bytes.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .unwrap_or_else(|| {
                panic!(
                    "{id}: `bytes` must be one integer per byte: {}",
                    frame["bytes"]
                )
            });
        assert_eq!(
            bytes.len() as u64,
            stride * height,
            "{id}: the frame is the whole mapped range, so every reported pixel is measured"
        );
        for (channel, (actual, expected)) in bytes[..4].iter().zip(oracle).enumerate() {
            if channel == 3 {
                assert_eq!(*actual, u64::from(expected), "{id}: raw alpha must be exact");
                continue;
            }
            assert!(
                actual.abs_diff(u64::from(expected)) <= 1,
                "{id}: channel {channel} is {actual}, the authored oracle {expected} allows one byte; \
                 first pixel {:?}",
                &bytes[..4]
            );
        }
    }
}
