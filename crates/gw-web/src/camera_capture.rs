use super::*;
pub(super) async fn capture_camera_frame(
    query: &CameraStreamQuery,
    buffers: u32,
) -> Result<Vec<u8>, ApiError> {
    let _capture_guard = CAMERA_CAPTURE_LOCK.lock().await;
    let device = select_camera_device(query)?;
    let width = query.width.unwrap_or(640).clamp(160, 1280);
    let height = query.height.unwrap_or(480).clamp(120, 720);
    let fps = query.fps.unwrap_or(10).clamp(1, 20);
    let fps_text = fps.to_string();
    let device_arg = format!("device={}", device.path);
    let caps_arg = format!(
        "video/x-raw,format=NV12,width={width},height={height},framerate={fps_text}/1"
    );
    let _ = TokioCommand::new("v4l2-ctl")
        .args([
            "-d",
            &device.path,
            "-c",
            "auto_exposure=0,gain_automatic=1,white_balance_automatic=1",
        ])
        .output()
        .await;
    let capture_id = format!(
        "park-gateway-camera-{}-{}", std::process::id(), Local::now().timestamp_millis()
    );
    let output_pattern = format!("/tmp/{capture_id}-%02d.jpg");
    let mut command = TokioCommand::new("gst-launch-1.0");
    command
        .args([
            "-q",
            "v4l2src",
            &device_arg,
            &format!("num-buffers={}", buffers.max(1)),
            "!",
            &caps_arg,
            "!",
            "jpegenc",
            "!",
            "multifilesink",
            &format!("location={output_pattern}"),
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(8), command.output())
        .await
        .map_err(|_| ApiError::unavailable("摄像头抓图超时"))?
        .map_err(|error| ApiError::unavailable(
            format!("摄像头抓图失败: {error}"),
        ))?;
    let latest_path = (0..12)
        .rev()
        .map(|index| PathBuf::from(format!("/tmp/{capture_id}-{index:02}.jpg")))
        .find(|path| path.exists());
    let image = latest_path
        .as_ref()
        .and_then(|path| fs::read(path).ok())
        .unwrap_or_default();
    for index in 0..12 {
        let _ = fs::remove_file(format!("/tmp/{capture_id}-{index:02}.jpg"));
    }
    if !output.status.success() || image.is_empty() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(
            ApiError::unavailable(
                format!(
                    "摄像头没有输出画面{}", if message.trim().is_empty() {
                    String::new() } else { format!(": {}", message.trim()) }
                ),
            ),
        );
    }
    Ok(image)
}
pub(super) fn discover_camera_devices() -> Vec<CameraDeviceDto> {
    let mut devices = Vec::new();
    let entries = match fs::read_dir("/sys/class/video4linux") {
        Ok(entries) => entries,
        Err(_) => return devices,
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.starts_with("video") {
            continue;
        }
        let path = format!("/dev/{file_name}");
        let name = fs::read_to_string(entry.path().join("name"))
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|_| file_name.clone());
        let lower = name.to_ascii_lowercase();
        let looks_like_capture = lower.contains("mainpath") || lower.contains("selfpath")
            || lower.contains("camera") || lower.contains("uvc") || lower.contains("ov");
        if !looks_like_capture {
            continue;
        }
        let primary = file_name == "video0" || lower.contains("mainpath");
        devices
            .push(CameraDeviceDto {
                stream_url: format!("/api/v1/camera/stream?device={path}"),
                path,
                name,
                primary,
            });
    }
    if devices
        .iter()
        .any(|device| {
            device.path == "/dev/video0"
                || device.name.to_ascii_lowercase().contains("mainpath")
        })
    {
        devices
            .retain(|device| {
                device.path == "/dev/video0"
                    || device.name.to_ascii_lowercase().contains("mainpath")
            });
    }
    devices.sort_by_key(|device| (!device.primary, device.path.clone()));
    devices
}
