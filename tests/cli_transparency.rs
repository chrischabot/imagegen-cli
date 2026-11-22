use std::process::Command;

#[test]
fn transparent_gpt_image_2_does_not_recommend_retired_models() {
    let result = Command::new(env!("CARGO_BIN_EXE_imagegen"))
        .args([
            "generate",
            "a simple icon",
            "--model",
            "gpt-image-2",
            "--background",
            "transparent",
            "--format",
            "png",
        ])
        // Stop at credential validation: the diagnostic must be correct without
        // an API key, a live provider request, or an image generation charge.
        .env_remove("OPENAI_API_KEY")
        .output()
        .expect("run imagegen");
    let stderr = String::from_utf8_lossy(&result.stderr);

    assert_eq!(result.status.code(), Some(2));
    assert!(stderr.contains("no API key found"), "{stderr}");
    assert!(!stderr.contains("gpt-image-1"), "{stderr}");
}
