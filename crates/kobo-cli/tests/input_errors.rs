//! CLI failures must be ordinary nonzero exits, not unwinding panics.

use std::{fs, process::Command};

#[test]
fn malformed_header_and_graphics_pointer_fail_without_panicking() {
    let directory = std::env::temp_dir().join(format!("kobo-input-errors-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("synthetic.sfc");
    for (gfx, message) in [
        (false, "invalid ROM size code"),
        (true, "could not be read"),
    ] {
        let mut bytes = vec![0; 0x8000];
        bytes[0x7FD5] = 0x20;
        if gfx {
            bytes[0x39C4] = 0x80;
            bytes[0x39F6] = 2;
        } else {
            bytes[0x7FD7] = 0xFF;
        }
        fs::write(&path, bytes).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_kobo"))
            .args(if gfx {
                ["gfx", "list"]
            } else {
                ["rom", "info"]
            })
            .arg("-r")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{stderr}");
        assert!(!stderr.contains("panicked"), "{stderr}");
    }
    fs::remove_dir_all(directory).unwrap();
}
