use super::build::{classify, container, find_app, parse_list, selection, BuildStatus, Diagnostic};
use super::*;
use serde_json::json;

const DEVICES: &str = r#"{"devices":{
  "com.apple.CoreSimulator.SimRuntime.iOS-26-4":[
    {"udid":"OLD-PHONE","name":"iPhone 17","state":"Shutdown","isAvailable":true}],
  "com.apple.CoreSimulator.SimRuntime.iOS-27-1":[
    {"udid":"NEW-PAD","name":"iPad Air 11-inch (M4)","state":"Shutdown","isAvailable":true},
    {"udid":"NEW-PHONE","name":"iPhone 18 Pro","state":"Shutdown","isAvailable":true},
    {"udid":"GONE","name":"iPhone 16","state":"Shutdown","isAvailable":false}],
  "com.apple.CoreSimulator.SimRuntime.watchOS-12-0":[
    {"udid":"WATCH","name":"Apple Watch","state":"Booted","isAvailable":true}],
  "com.apple.CoreSimulator.SimRuntime.iOS-27-0":[
    {"udid":"RUNNING","name":"iPhone Air","state":"Booted","isAvailable":true}]
}}"#;

fn udids(devices: &[Device]) -> Vec<&str> {
    devices.iter().map(|d| d.udid.as_str()).collect()
}

#[test]
fn lists_ios_devices_booted_first_then_the_newest_iphone() {
    let devices = parse_devices(DEVICES).unwrap();
    assert_eq!(
        udids(&devices),
        ["RUNNING", "NEW-PHONE", "NEW-PAD", "OLD-PHONE"]
    );
    assert_eq!(devices[0].runtime, "iOS 27.0");
    assert_eq!(devices[1].runtime, "iOS 27.1");
}

#[test]
fn picks_by_udid_or_name_and_defaults_to_the_booted_device() {
    let devices = parse_devices(DEVICES).unwrap();
    assert_eq!(pick(&devices, None).unwrap().udid, "RUNNING");
    assert_eq!(pick(&devices, Some("  ")).unwrap().udid, "RUNNING");
    assert_eq!(pick(&devices, Some("new-phone")).unwrap().udid, "NEW-PHONE");
    assert_eq!(pick(&devices, Some("iphone 17")).unwrap().udid, "OLD-PHONE");
    assert!(pick(&devices, Some("Apple Watch")).is_err());
    assert!(pick(&[], None).unwrap_err().contains("downloadPlatform"));

    let idle = parse_devices(&DEVICES.replace("\"Booted\"", "\"Shutdown\"")).unwrap();
    assert_eq!(pick(&idle, None).unwrap().udid, "NEW-PHONE");
}

#[test]
fn names_the_fix_for_each_xcode_problem() {
    assert!(diagnose(None, false, false)
        .unwrap()
        .contains("Install Xcode"));
    assert!(
        diagnose(Some("/Library/Developer/CommandLineTools"), true, true)
            .unwrap()
            .contains("Command Line Tools")
    );
    assert!(diagnose(
        Some("/Applications/Xcode.app/Contents/Developer"),
        false,
        true
    )
    .unwrap()
    .contains("xcode-select -s"));
    assert!(diagnose(
        Some("/Applications/Xcode.app/Contents/Developer"),
        true,
        false
    )
    .unwrap()
    .contains("runFirstLaunch"));
    assert_eq!(
        diagnose(
            Some("/Applications/Xcode.app/Contents/Developer"),
            true,
            true
        ),
        None
    );
}

#[test]
fn reads_the_pid_simctl_launch_prints() {
    assert_eq!(launched_pid("com.example.app: 4821\n"), Some(4821));
    assert_eq!(launched_pid("warning\ncom.example.app: 77"), Some(77));
    assert_eq!(launched_pid(""), None);
}

#[test]
fn shrinks_screenshots_for_the_agent_keeping_their_shape() {
    let phone = image::DynamicImage::new_rgb8(1206, 2622);
    let (jpeg, width, height) = agent_jpeg(&phone, 1280).unwrap();
    assert_eq!(height, 1280);
    assert_eq!(width, 589);
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);

    let small = image::DynamicImage::new_rgb8(300, 200);
    let (_, width, height) = agent_jpeg(&small, 1280).unwrap();
    assert_eq!((width, height), (300, 200));
}

#[test]
fn keeps_the_newest_end_of_long_output() {
    assert_eq!(tail("short", 100), "short");
    assert_eq!(tail("old line\nnew line", 12), "new line");
    let accented = "ééééé\nfin";
    assert_eq!(tail(accented, 5), "fin");
}

#[test]
fn log_predicates_take_only_executable_names() {
    assert!(valid_process("MyApp"));
    assert!(valid_process("My App-2.beta_1"));
    assert!(!valid_process(""));
    assert!(!valid_process("x\" OR 1 == 1"));
    assert!(!valid_process(&"a".repeat(101)));
}

#[test]
fn builds_need_an_absolute_project_or_workspace() {
    assert!(container("App.xcodeproj").unwrap_err().contains("absolute"));
    let dir = tempfile::tempdir().unwrap();
    assert!(container(&dir.path().join("App.swift").to_string_lossy())
        .unwrap_err()
        .contains("find_projects"));
    let project = dir.path().join("App.xcworkspace");
    std::fs::create_dir(&project).unwrap();
    let found = container(&project.to_string_lossy()).unwrap();
    assert!(found.workspace);
    assert!(container(&dir.path().join("Missing.xcodeproj").to_string_lossy()).is_err());
}

#[test]
fn names_the_scheme_destination_and_its_own_derived_data() {
    let container = super::build::Container {
        path: "/work/App.xcodeproj".into(),
        workspace: false,
    };
    let args = selection(
        &container,
        "App",
        "Debug",
        "platform=iOS Simulator,id=ABC",
        std::path::Path::new("/cache/DerivedData"),
    );
    assert_eq!(
        args,
        [
            "-project",
            "/work/App.xcodeproj",
            "-scheme",
            "App",
            "-configuration",
            "Debug",
            "-destination",
            "platform=iOS Simulator,id=ABC",
            "-derivedDataPath",
            "/cache/DerivedData",
            "-skipMacroValidation",
        ]
    );
    let a = super::build::build_root(std::path::Path::new("/work/A.xcodeproj"));
    let b = super::build::build_root(std::path::Path::new("/work/B.xcodeproj"));
    assert_ne!(a, b);
    assert!(a
        .to_string_lossy()
        .contains("com.monocode.desktop/simulator-builds"));
}

#[test]
fn finds_projects_but_not_inside_bundles_or_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    for folder in [
        "ios/App.xcworkspace",
        "ios/App.xcodeproj/project.xcworkspace",
        "node_modules/lib/Lib.xcodeproj",
        ".hidden/Secret.xcodeproj",
        "Tool.xcodeproj",
    ] {
        std::fs::create_dir_all(dir.path().join(folder)).unwrap();
    }
    let root = dir.path().to_string_lossy().into_owned();
    let found = find_projects(&root).unwrap();
    let names: Vec<String> = found
        .iter()
        .map(|p| {
            p.strip_prefix(&root)
                .unwrap()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
        .collect();
    assert_eq!(
        names,
        [
            "/ios/App.xcworkspace",
            "/Tool.xcodeproj",
            "/ios/App.xcodeproj"
        ]
        .map(str::to_owned)
    );
    assert!(find_projects("relative").is_err());
}

#[test]
fn reads_schemes_from_a_project_or_a_workspace() {
    let project = r#"{"project":{"name":"App","schemes":["App","AppTests"],"targets":["App"],"configurations":["Debug","Release"]}}"#;
    assert_eq!(
        parse_list(project).unwrap()["schemes"],
        json!(["App", "AppTests"])
    );
    let workspace = r#"{"workspace":{"name":"App","schemes":["App"]}}"#;
    let list = parse_list(workspace).unwrap();
    assert_eq!(list["schemes"], json!(["App"]));
    assert_eq!(list["targets"], json!(null));
    assert!(parse_list("{}").is_err());
}

#[test]
fn finds_the_simulator_app_in_the_build_settings() {
    let settings = json!([
        {"target":"AppTests","buildSettings":{"PRODUCT_TYPE":"com.apple.product-type.bundle.unit-test","TARGET_BUILD_DIR":"/d/Debug-iphonesimulator","FULL_PRODUCT_NAME":"AppTests.xctest"}},
        {"target":"App","buildSettings":{"PRODUCT_TYPE":"com.apple.product-type.application","PLATFORM_NAME":"iphonesimulator","TARGET_BUILD_DIR":"/d/Debug-iphonesimulator","FULL_PRODUCT_NAME":"App.app","PRODUCT_BUNDLE_IDENTIFIER":"com.example.app"}}
    ]);
    let app = std::path::Path::new("/d/Debug-iphonesimulator")
        .join("App.app")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        find_app(&settings.to_string()),
        Some((app, Some("com.example.app".into())))
    );
    assert_eq!(find_app("[]"), None);
    assert_eq!(find_app("not json"), None);
}

#[test]
fn counts_each_compile_error_and_warning_once() {
    assert_eq!(
        classify("/w/App/View.swift:12:5: error: cannot find 'x' in scope"),
        Some(Diagnostic::Error(
            "/w/App/View.swift:12:5: error: cannot find 'x' in scope"
        ))
    );
    assert_eq!(
        classify("warning: Run script build phase 'Lint' will be run during every build"),
        Some(Diagnostic::Warning(
            "warning: Run script build phase 'Lint' will be run during every build"
        ))
    );
    assert_eq!(
        classify("CompileSwift normal arm64 /w/App/View.swift"),
        None
    );

    let mut status = BuildStatus::new(
        "build-1".into(),
        "/w/App.xcodeproj".into(),
        "App",
        "Debug",
        "generic",
        "/tmp/log".into(),
    );
    for line in [
        "/w/A.swift:1:1: error: boom",
        "/w/A.swift:1:1: error: boom",
        "/w/B.swift:2:2: warning: careful",
        "note: unrelated",
    ] {
        status.note_line(line);
    }
    assert_eq!(status.error_count, 1);
    assert_eq!(status.warning_count, 1);
    assert_eq!(status.errors, ["/w/A.swift:1:1: error: boom"]);
}

#[test]
fn unknown_builds_are_reported_as_such() {
    let error = build_status("build-404", std::time::Duration::ZERO).unwrap_err();
    assert!(error.contains("No build 'build-404'"));
}

#[test]
fn the_mcp_server_introduces_itself_and_lists_its_tools() {
    let reply = mcp::response(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}})).unwrap();
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(reply["result"]["serverInfo"]["name"], "ios_simulator");
    assert!(reply["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("build_status"));

    assert_eq!(
        mcp::response(&json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
        None
    );
    let list = mcp::response(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "status",
            "boot",
            "find_projects",
            "schemes",
            "build",
            "build_status",
            "launch",
            "screenshot",
            "logs",
            "open_url",
            "appearance",
            "describe_ui",
            "tap",
            "swipe",
            "type_text",
            "press_key",
            "press_button",
            "rotate"
        ]
    );
    // The connector allows exactly the tools the server offers.
    let connector = crate::connectors::connector("ios_simulator").unwrap();
    assert_eq!(connector.tools, names.as_slice());

    let unknown = mcp::response(&json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_projects","arguments":{"directory":"relative"}}})).unwrap();
    assert_eq!(unknown["result"]["isError"], true);
    let missing = mcp::response(&json!({"jsonrpc":"2.0","id":4,"method":"nope"})).unwrap();
    assert_eq!(missing["error"]["code"], -32601);
}

#[test]
fn runner_calls_carry_only_their_arguments_and_the_app_they_address() {
    let input = json!({"x": 10, "y": 20.5, "device": "iPhone", "element": null, "extra": true});
    assert_eq!(
        mcp::runner_body(&input, &["x", "y", "element"], None),
        json!({"x": 10, "y": 20.5})
    );
    assert_eq!(
        mcp::runner_body(
            &json!({"text": "hola"}),
            &["text"],
            Some("com.example.app".into())
        ),
        json!({"text": "hola", "bundle_id": "com.example.app", "guessed": true})
    );
    // A bundle the call names wins over the one launched last.
    assert_eq!(
        mcp::runner_body(
            &json!({"text": "hola", "bundle_id": "com.apple.springboard"}),
            &["text"],
            Some("com.example.app".into())
        ),
        json!({"text": "hola", "bundle_id": "com.apple.springboard"})
    );
}

#[test]
fn screenshots_reach_the_agent_at_the_screen_size_in_points() {
    // iPhone 18 Pro: 1206 × 2622 pixels at 3×.
    assert_eq!(agent_edge(1206, 2622, 3.0), (402, 874, 874));
    // A 13-inch iPad in points is larger than the agent's image.
    assert_eq!(agent_edge(2064, 2752, 2.0), (1032, 1376, 1280));
}

#[test]
fn a_runner_build_is_named_by_its_sources_and_xcode() {
    let a = input::build_key("Xcode 27.2\nBuild version 27B52\n");
    assert_eq!(a, input::build_key("Xcode 27.2\nBuild version 27B52"));
    assert_ne!(a, input::build_key("Xcode 27.3\nBuild version 27C1"));
    assert_eq!(a.len(), 16);
}
