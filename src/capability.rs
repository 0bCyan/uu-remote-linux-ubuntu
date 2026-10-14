//! Wire payloads reconstructed from the official libstreamer ARM64 serializers.
//! See analysis/protocol-fix-20261013/datachannel for address-bound evidence.
//! These payloads do not claim server acceptance or invent missing handshake steps.

use serde_json::{json, Value};

pub const ICE_FINISHED_EVENT: &str = "ice_finished_log";
pub const FORWARD_SETTING_EVENT: &str = "forward_setting";
pub const VIDEO_CODEC_H264: i32 = 1;
pub const CODEC_IMPL_NVENC: i32 = 0;

/// Official serializer emits exactly two string fields (0x107190).
/// The event is telemetry emitted for official pc::State 6; it is not an ACK.
pub fn ice_finished(ice_id: &str, app_control_id: &str) -> Value {
    json!({"ice_id": ice_id, "app_control_id": app_control_id})
}

/// Supply the stable id of a display that this host can actually capture.
/// Official Mac display construction uses type 0 and HDR disabled value 0.
/// `codec_impl` is caller-selected: the observed names are 0 NVENC, 1 AMD AMF,
/// 2 Intel QSV, 3 VideoToolbox, 4 MediaCodec; values >=5 log as SoftWare.
/// Using 5 as software is an inference and must be runtime-validated by caller.
pub fn h264_device_capability(
    client_id: &str,
    ice_id: &str,
    display_id: i32,
    fps: i32,
    codec_impl: i32,
) -> Value {
    json!({
        "client_id": client_id,
        "type": "device_capability",
        "data": {
            "display_info": [{"id": display_id, "fps": fps, "type": 0, "hdr": 0}],
            "video_codec_capability": [{
                "video_codec": VIDEO_CODEC_H264,
                "chroma_sampling": 1,
                "codec_impl": codec_impl,
                "bit_depth": 8
            }],
            "ice_id": ice_id
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ice_finished_keeps_string_identifiers_and_exact_fields() {
        let payload = ice_finished("ice-example", "control-example");
        assert_eq!(payload.as_object().unwrap().len(), 2);
        assert!(payload["ice_id"].is_string());
        assert_eq!(payload["app_control_id"], "control-example");
        assert_eq!(ICE_FINISHED_EVENT, "ice_finished_log");
    }

    #[test]
    fn device_capability_uses_object_data_and_string_ice_id() {
        let payload = h264_device_capability("client-example", "ice-example", 11, 60, 0);
        assert_eq!(payload["type"], "device_capability");
        assert!(payload["data"].is_object());
        assert_eq!(payload["data"]["ice_id"], "ice-example");
        let display = &payload["data"]["display_info"][0];
        assert_eq!(display.as_object().unwrap().len(), 4);
        assert_eq!(display["id"], 11);
        assert_eq!(display["fps"], 60);
        let codec = &payload["data"]["video_codec_capability"][0];
        assert_eq!(codec.as_object().unwrap().len(), 4);
        assert_eq!(codec["video_codec"], 1);
        assert_eq!(codec["codec_impl"], 0);
        assert_eq!(codec["bit_depth"], 8);
    }
}
