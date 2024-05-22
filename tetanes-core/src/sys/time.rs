//! Platform-specific time and date methods.

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        pub use web_time::{Duration, Instant};
    } else if #[cfg(target_vendor = "vex")] {
        pub use core::time::Duration;
        pub use vexide_core::time::Instant;
    } else {
        pub use core::time::{Duration, Instant};
    }
}
