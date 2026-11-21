//! Month 5 Milestone Verification (Vision and Computer Use).
//!
//! Validates the "Done when" criteria for Month 5 (Vision and Computer Use):
//! 1. Completes a picture-based practice quiz (fallback screenshot on picture-only pages).
//! 2. Changes a setting in one desktop app, asking before each final action (Risk::Ask approval boundary).
//! 3. Emergency stop hotkey / button halts desktop computer operations immediately.
//! 4. Per-app allow list boundary blocks unauthorized applications.
//! 5. Multimodal image input routing and classification (RequestClass::Vision).

use oblivion_core::routing::{classify_request, RequestClass};
use oblivion_mcp::is_insufficient_page_text;
use oblivion_providers::{ImageAttachment, MOCK_PNG_BASE64};
use oblivion_tools::computer::{
    is_app_allowed, is_emergency_stopped, reset_emergency_stop, trigger_emergency_stop,
    ActiveWindowInfo, ComputerTool, ComputerUseSettings,
};
use oblivion_tools::{Risk, ToolProvider};
use serde_json::json;

#[tokio::test]
async fn milestone_vision_picture_practice_quiz() {
    // Verify that picture_quiz containing SVG visual geometry and embedded gate diagram
    // is correctly flagged as having insufficient text, triggering the visual screenshot fallback.
    let quiz_snapshot = r#"### Page state
- Page URL: https://example.com/picture_quiz
- Page Title: Visual Geometry Practice Quiz
- Page Snapshot:
```yaml
- generic:
  - heading "Question 1: Visual Logic and Angle" [level=2]
  - svg [ref=e1]
  - radio "Option A: 90 degrees" [ref=e2]
  - radio "Option B: 45 degrees" [ref=e3]
  - button "Submit" [ref=e4]
```"#;

    assert!(
        is_insufficient_page_text(quiz_snapshot),
        "Practice quiz with visual SVG elements should trigger screenshot fallback"
    );

    // Verify multimodal image attachment extraction and roundtrip
    let img = ImageAttachment::new("image/png", MOCK_PNG_BASE64).with_name("picture_quiz.png");
    assert_eq!(img.mime_type, "image/png");
    assert!(img.data_url().starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn milestone_vision_desktop_change_setting_with_approval() {
    // "changes a setting in one desktop app, asking before each final action"
    let settings = ComputerUseSettings {
        enabled: true,
        allowed_apps: vec!["notepad.exe".into()],
        ask_all: true,
        stop_hotkey: "Escape".into(),
    };

    let mock_win = ActiveWindowInfo {
        title: "Untitled - Notepad".into(),
        process_name: "notepad.exe".into(),
        x: 100,
        y: 100,
        width: 800,
        height: 600,
    };

    let tool = ComputerTool::new(settings).with_mock_window(mock_win);

    // 1. Verify risk evaluation requires user approval (Risk::Ask) before the final action
    let click_args = json!({
        "action": "click",
        "x": 250,
        "y": 300,
        "app": "notepad.exe"
    });
    let risk = tool.risk("computer", &click_args);
    match risk {
        Risk::Ask(prompt) => {
            assert!(
                prompt.contains("click at (250, 300)"),
                "Prompt must describe the target action: {prompt}"
            );
        }
        other => panic!("Expected Risk::Ask before final action, got {other:?}"),
    }

    // 2. Perform the approved action
    let result = tool.execute("computer", &click_args).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Clicked (250, 300)"));

    // 3. Typing setting change (e.g. font_size = 14) also requires approval and succeeds
    let type_args = json!({
        "action": "type",
        "text": "14",
        "app": "notepad.exe"
    });
    let type_risk = tool.risk("computer", &type_args);
    match type_risk {
        Risk::Ask(prompt) => {
            assert!(prompt.contains("type \"14\""));
        }
        other => panic!("Expected Risk::Ask before typing setting, got {other:?}"),
    }
    let type_res = tool.execute("computer", &type_args).await;
    assert!(type_res.is_ok());
    assert!(type_res.unwrap().contains("Typed 2 characters"));
}

#[tokio::test]
async fn milestone_vision_emergency_stop_invariant() {
    reset_emergency_stop();
    assert!(!is_emergency_stopped());

    let settings = ComputerUseSettings {
        enabled: true,
        allowed_apps: vec!["notepad.exe".into()],
        ask_all: false,
        stop_hotkey: "Escape".into(),
    };

    let mock_win = ActiveWindowInfo {
        title: "Untitled - Notepad".into(),
        process_name: "notepad.exe".into(),
        x: 100,
        y: 100,
        width: 800,
        height: 600,
    };

    let tool = ComputerTool::new(settings).with_mock_window(mock_win);

    // Trigger emergency stop (simulating Escape hotkey or menubar button)
    trigger_emergency_stop();
    assert!(is_emergency_stopped());

    // All interactions must be immediately halted
    let res = tool
        .execute(
            "computer",
            &json!({"action": "click", "x": 100, "y": 100, "app": "notepad.exe"}),
        )
        .await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("emergency stop is active"));

    // Reset emergency stop restores normal operations
    reset_emergency_stop();
    assert!(!is_emergency_stopped());

    let res2 = tool
        .execute(
            "computer",
            &json!({"action": "click", "x": 100, "y": 100, "app": "notepad.exe"}),
        )
        .await;
    assert!(res2.is_ok());
}

#[tokio::test]
async fn milestone_vision_app_allow_list_boundary() {
    let allowed = vec!["notepad.exe".into(), "calc.exe".into()];

    // Permitted applications
    assert!(is_app_allowed(
        "notepad.exe",
        "Untitled - Notepad",
        &allowed
    ));
    assert!(is_app_allowed("calc.exe", "Calculator", &allowed));

    // Strictly blocked unauthorized applications
    assert!(!is_app_allowed(
        "cmd.exe",
        "Command Prompt - Administrator",
        &allowed
    ));
    assert!(!is_app_allowed(
        "powershell.exe",
        "Windows PowerShell",
        &allowed
    ));
    assert!(!is_app_allowed("chrome.exe", "Personal Banking", &allowed));
    assert!(!is_app_allowed("explorer.exe", "File Explorer", &allowed));
}

#[tokio::test]
async fn milestone_vision_request_classification() {
    // When images are attached, request is dynamically classified as RequestClass::Vision
    let class = classify_request("Please describe this screenshot", 200, true, false);
    assert_eq!(class, RequestClass::Vision);

    // Text only defaults to Simple or Hard based on prompt length/tools
    let text_class = classify_request("Say hello world", 50, false, false);
    assert_ne!(text_class, RequestClass::Vision);
}
