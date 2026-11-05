use super::*;
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CameraDeviceDto {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) primary: bool,
    pub(super) stream_url: String,
}
#[derive(Debug, Clone, Deserialize)]
pub(super) struct CameraStreamQuery {
    pub(super) device: Option<String>,
    pub(super) width: Option<u32>,
    pub(super) height: Option<u32>,
    pub(super) fps: Option<u32>,
}
pub(super) async fn camera_devices() -> ApiResult<Vec<CameraDeviceDto>> {
    Ok(Json(discover_camera_devices()))
}
pub(super) async fn camera_stream(
    Query(query): Query<CameraStreamQuery>,
) -> Result<Response, ApiError> {
    stop_camera_live_processes().await;
    let device = select_camera_device(&query)?;
    let width = query.width.unwrap_or(640).clamp(160, 1280);
    let height = query.height.unwrap_or(480).clamp(120, 720);
    let fps = query.fps.unwrap_or(10).clamp(1, 20);
    let fps_text = fps.to_string();
    let device_arg = format!("device={}", device.path);
    let caps_arg = format!(
        "video/x-raw,format=NV12,width={width},height={height},framerate={fps_text}/1"
    );
    let capture_id = format!(
        "park-gateway-camera-live-{}-{}", std::process::id(), Local::now()
        .timestamp_millis()
    );
    let output_pattern = format!("/tmp/{capture_id}-%05d.jpg");
    let _ = TokioCommand::new("v4l2-ctl")
        .args([
            "-d",
            &device.path,
            "-c",
            "auto_exposure=0,gain_automatic=1,white_balance_automatic=1",
        ])
        .output()
        .await;
    let mut command = TokioCommand::new("gst-launch-1.0");
    command
        .args([
            "-q",
            "v4l2src",
            &device_arg,
            "!",
            &caps_arg,
            "!",
            "jpegenc",
            "!",
            "multifilesink",
            &format!("location={output_pattern}"),
            "max-files=32",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|error| ApiError::unavailable(
            format!("摄像头流启动失败: {error}"),
        ))?;
    struct CameraLiveState {
        child: tokio::process::Child,
        capture_id: String,
        last_index: u32,
    }
    let stream = stream::unfold(
        CameraLiveState {
            child,
            capture_id,
            last_index: 2,
        },
        |mut state| async move {
            loop {
                if matches!(state.child.try_wait(), Ok(Some(_))) {
                    return None;
                }
                if let Some((index, path)) = latest_camera_live_frame(
                    &state.capture_id,
                    state.last_index,
                ) {
                    let image = fs::read(&path).unwrap_or_default();
                    let _ = fs::remove_file(&path);
                    cleanup_camera_live_frames(&state.capture_id, index);
                    state.last_index = index;
                    if !image.is_empty() {
                        let mut part = Vec::with_capacity(image.len() + 128);
                        part.extend_from_slice(b"--park-gateway-camera\r\n");
                        part.extend_from_slice(b"Content-Type: image/jpeg\r\n");
                        part.extend_from_slice(
                            format!("Content-Length: {}\r\n\r\n", image.len()).as_bytes(),
                        );
                        part.extend_from_slice(&image);
                        part.extend_from_slice(b"\r\n");
                        return Some((
                            Ok::<Bytes, std::io::Error>(Bytes::from(part)),
                            state,
                        ));
                    }
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        },
    );
    let body = Body::from_stream(stream);
    Ok(
        (
            [
                (
                    "content-type",
                    "multipart/x-mixed-replace; boundary=park-gateway-camera",
                ),
                ("cache-control", "no-store"),
                ("x-accel-buffering", "no"),
            ],
            body,
        )
            .into_response(),
    )
}
pub(super) fn latest_camera_live_frame(
    capture_id: &str,
    last_index: u32,
) -> Option<(u32, PathBuf)> {
    let prefix = format!("{capture_id}-");
    fs::read_dir("/tmp")
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !file_name.starts_with(&prefix) || !file_name.ends_with(".jpg") {
                return None;
            }
            let index = file_name
                .trim_start_matches(&prefix)
                .trim_end_matches(".jpg")
                .parse::<u32>()
                .ok()?;
            (index > last_index).then_some((index, entry.path()))
        })
        .max_by_key(|(index, _)| *index)
}
pub(super) fn cleanup_camera_live_frames(capture_id: &str, keep_from: u32) {
    let prefix = format!("{capture_id}-");
    let Ok(entries) = fs::read_dir("/tmp") else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.starts_with(&prefix) || !file_name.ends_with(".jpg") {
            continue;
        }
        let Ok(index) = file_name
            .trim_start_matches(&prefix)
            .trim_end_matches(".jpg")
            .parse::<u32>() else {
            continue;
        };
        if index < keep_from {
            let _ = fs::remove_file(entry.path());
        }
    }
}
pub(super) async fn camera_snapshot(
    Query(query): Query<CameraStreamQuery>,
) -> Result<Response, ApiError> {
    stop_camera_live_processes().await;
    let image = capture_camera_frame(&query, 12).await?;
    Ok(
        ([("content-type", "image/jpeg"), ("cache-control", "no-store")], image)
            .into_response(),
    )
}
pub(super) async fn stop_camera_live_processes() {
    let _ = TokioCommand::new("sh")
        .args([
            "-c",
            "for pid in $(pgrep -x gst-launch-1.0 2>/dev/null); do args=$(tr '\\0' ' ' < /proc/$pid/cmdline 2>/dev/null || true); case \"$args\" in *'park-gateway-camera-live-'*) kill \"$pid\" 2>/dev/null || true ;; esac; done",
        ])
        .output()
        .await;
}
pub(super) fn select_camera_device(
    query: &CameraStreamQuery,
) -> Result<CameraDeviceDto, ApiError> {
    let devices = discover_camera_devices();
    if devices.is_empty() {
        return Err(ApiError::unavailable("未发现可用摄像头设备"));
    }
    query
        .device
        .as_deref()
        .and_then(|path| devices.iter().find(|item| item.path == path))
        .or_else(|| devices.iter().find(|item| item.primary))
        .or_else(|| devices.first())
        .cloned()
        .ok_or_else(|| ApiError::unavailable("未发现可用摄像头设备"))
}
