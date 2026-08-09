//! Channel resolution (generic / lab / rescue) and target metadata.
//!
//! Replaces `ragc/lib/publish.sh` helpers (target_channel,
//! canonical_channel, channel_default_target, etc).

use crate::cli::{Channel, ImageTarget};

/// Map a target to its canonical channel.
pub fn target_channel(target: ImageTarget) -> Channel {
    match target {
        ImageTarget::DesktopGeneric => Channel::Generic,
        ImageTarget::DesktopLab | ImageTarget::HypervDebug => Channel::Lab,
        ImageTarget::RescueMinimal => Channel::Rescue,
    }
}

/// Map a target to its hardware class.
pub fn target_hardware_class(target: ImageTarget) -> &'static str {
    match target {
        ImageTarget::DesktopGeneric => "physical-generic",
        ImageTarget::DesktopLab => "physical-lab",
        ImageTarget::HypervDebug => "hyperv",
        ImageTarget::RescueMinimal => "rescue",
    }
}

/// Default target for a channel.
pub fn channel_default_target(channel: Channel) -> ImageTarget {
    match channel {
        Channel::Generic => ImageTarget::DesktopGeneric,
        Channel::Lab => ImageTarget::DesktopLab,
        Channel::Rescue => ImageTarget::RescueMinimal,
    }
}

/// Pointer name for the current generation of a channel.
pub fn channel_current_pointer(channel: Channel) -> &'static str {
    match channel {
        Channel::Generic => "current-generic",
        Channel::Lab => "current-lab",
        Channel::Rescue => "current-rescue",
    }
}

/// Pointer name for the previous generation of a channel.
pub fn channel_previous_pointer(channel: Channel) -> &'static str {
    match channel {
        Channel::Generic => "previous-generic",
        Channel::Lab => "previous-lab",
        Channel::Rescue => "previous-rescue",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_channel_mapping() {
        assert_eq!(
            target_channel(ImageTarget::DesktopGeneric),
            Channel::Generic
        );
        assert_eq!(target_channel(ImageTarget::DesktopLab), Channel::Lab);
        assert_eq!(target_channel(ImageTarget::HypervDebug), Channel::Lab);
        assert_eq!(target_channel(ImageTarget::RescueMinimal), Channel::Rescue);
    }

    #[test]
    fn test_hardware_class_mapping() {
        assert_eq!(
            target_hardware_class(ImageTarget::DesktopGeneric),
            "physical-generic"
        );
        assert_eq!(target_hardware_class(ImageTarget::RescueMinimal), "rescue");
    }

    #[test]
    fn test_channel_default_target_roundtrip() {
        for ch in [Channel::Generic, Channel::Lab, Channel::Rescue] {
            let t = channel_default_target(ch);
            assert_eq!(target_channel(t), ch);
        }
    }

    #[test]
    fn test_channel_pointer_names_unique() {
        let ptrs = [
            channel_current_pointer(Channel::Generic),
            channel_current_pointer(Channel::Lab),
            channel_current_pointer(Channel::Rescue),
        ];
        let unique: std::collections::HashSet<_> = ptrs.iter().collect();
        assert_eq!(unique.len(), 3);
    }
}
