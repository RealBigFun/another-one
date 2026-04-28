# Slint Platform Capability Matrix

| Capability | Linux | macOS | Android | iOS | Windows |
| --- | --- | --- | --- | --- | --- |
| Slint window launches | implemented | compile-profile only | package metadata present | profile only | unsupported |
| App identity | `com.anotherone.Slint` | `com.anotherone.Slint` | `com.anotherone.slint` | `com.anotherone.slint` | unsupported |
| Input policy | desktop keyboard | desktop keyboard | touch IME | touch IME | unsupported |
| Terminal transport | Iroh daemon ticket | Iroh daemon ticket | Iroh daemon ticket planned | Iroh daemon ticket planned | unsupported |
| System appearance | env-backed seam | env-backed seam | env-backed seam | env-backed seam | unsupported |
| Open-In actions | shared core available | shared core available | platform hook required | platform hook required | unsupported |
| Resource sampling | shared core available | shared core available | procfs shared path | Darwin shared path | unsupported for Slint |
| Packaging proof | cargo run/dev-watch | cargo build target required | cargo-apk path exists | macOS/Xcode proof required | no |

The matrix is backed by `slint-poc/src/platform.rs` and `slint-poc/src/style.rs`.
