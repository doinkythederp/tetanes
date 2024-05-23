//! Platform-specific time and date methods.

use cfg_if::cfg_if;
use rand::Rng;

cfg_if! {
    if #[cfg(target_vendor = "vex")] {
        pub fn rng() -> impl Rng {
            rand_pcg::Pcg32::new(unsafe { vex_sdk::vexSystemPowerupTimeGet() }, unsafe { vex_sdk::vexSystemPowerupTimeGet() })
        }
    } else {
        pub fn rng() -> impl Rng {
            rand::thread_rng()
        }
    }
}
