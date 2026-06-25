pub mod notifications;
pub mod session;
pub mod shortcuts;

pub const CMUX_PLATFORM: &str = "windows-m1-core";

pub fn milestone() -> &'static str {
    "M1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_m1_marker() {
        assert_eq!(milestone(), "M1");
        assert_eq!(CMUX_PLATFORM, "windows-m1-core");
    }
}
