use enum_map::Enum;
pub use winit::keyboard::KeyCode;

// for the gamepad we emulate nintendo switch controller (because that's what I was reversing =))

#[derive(Debug, Hash, Ord, PartialOrd, PartialEq, Eq, Clone, Copy, Enum)]
pub enum GamepadAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTrigger,
    RightTrigger,
}

impl GamepadAxis {
    pub fn from_gilrs(axis: gilrs::Axis) -> Option<Self> {
        use gilrs::Axis::*;
        Some(match axis {
            LeftStickX => GamepadAxis::LeftStickX,
            LeftStickY => GamepadAxis::LeftStickY,
            RightStickX => GamepadAxis::RightStickX,
            RightStickY => GamepadAxis::RightStickY,
            LeftZ => GamepadAxis::LeftTrigger,
            RightZ => GamepadAxis::RightTrigger,
            _ => return None,
        })
    }
}

#[derive(Debug, Hash, Ord, PartialOrd, PartialEq, Eq, Clone, Copy, Enum)]
pub enum GamepadButton {
    // actual buttons
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    Plus,
    Minus,
    L,
    R,
    ZL,
    ZR,
    StickL,
    StickR,
}

#[derive(Debug, Hash, Ord, PartialOrd, PartialEq, Eq, Clone, Copy, Enum)]
pub enum VirtualGamepadButton {
    StickLUp,
    StickLDown,
    StickLLeft,
    StickLRight,
    StickRUp,
    StickRDown,
    StickRLeft,
    StickRRight,
}

impl GamepadButton {
    pub fn from_gilrs(button: gilrs::Button) -> Option<Self> {
        use gilrs::Button::*;
        Some(match button {
            South => GamepadButton::B,
            East => GamepadButton::A,
            West => GamepadButton::Y,
            North => GamepadButton::X,
            Start => GamepadButton::Plus,
            Select => GamepadButton::Minus,
            LeftTrigger => GamepadButton::L,
            RightTrigger => GamepadButton::R,
            LeftTrigger2 => GamepadButton::ZL,
            RightTrigger2 => GamepadButton::ZR,
            LeftThumb => GamepadButton::StickL,
            RightThumb => GamepadButton::StickR,
            DPadUp => GamepadButton::Up,
            DPadDown => GamepadButton::Down,
            DPadLeft => GamepadButton::Left,
            DPadRight => GamepadButton::Right,
            _ => return None,
        })
    }
}

/// Which maker's buttons a gamepad has, for the button prompts (the buttons themselves map by position either way)
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum GamepadFamily {
    /// Switch Pro Controller, Joy-Con
    Nintendo,
    /// DualShock, DualSense
    PlayStation,
    /// Xbox and other XInput pads
    Xbox,
    /// Anything else: shown with the Xbox prompts
    #[default]
    Generic,
}

impl GamepadFamily {
    /// From what gilrs reports: the USB vendor first (Nintendo 0x057E, Sony 0x054C, Microsoft 0x045E), then the name
    pub fn detect(name: &str, vendor: Option<u16>, _product: Option<u16>) -> Self {
        match vendor {
            Some(0x057e) => return Self::Nintendo,
            Some(0x054c) => return Self::PlayStation,
            Some(0x045e) => return Self::Xbox,
            _ => {}
        }
        let name = name.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
        if has(&["nintendo", "switch", "joy-con", "joycon", "pro controller"]) {
            Self::Nintendo
        } else if has(&[
            "playstation",
            "dualshock",
            "dualsense",
            "sony",
            "ps3",
            "ps4",
            "ps5",
        ]) {
            Self::PlayStation
        } else if has(&["xbox", "xinput", "x-box", "microsoft"]) {
            Self::Xbox
        } else {
            Self::Generic
        }
    }
}

/// The device the player used last: the button prompts follow it
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum InputDevice {
    #[default]
    KeyboardMouse,
    Gamepad(GamepadFamily),
}

#[derive(Debug, Hash, Ord, PartialOrd, PartialEq, Eq, Clone, Copy, Enum)]
pub enum MouseButton {
    /// Left mouse button
    Left,
    /// Right mouse button
    Right,
    /// Middle mouse button (pressing the scroll wheel)
    Middle,
    /// Wheel up pseudo-button (scrolling up, discrete)
    WheelUp,
    /// Wheel down pseudo-button (scrolling down, discrete)
    WheelDown,
    // Ignore "other" mouse buttons for the sake of simplicity
    // Other(u16),
}

#[cfg(test)]
mod tests {
    use super::GamepadFamily::{self, *};

    #[test]
    fn detects_gamepad_families() {
        let detect = GamepadFamily::detect;
        assert_eq!(
            detect("Pro Controller", Some(0x057e), Some(0x2009)),
            Nintendo
        );
        assert_eq!(
            detect("Wireless Controller", Some(0x054c), Some(0x0ce6)),
            PlayStation
        );
        assert_eq!(detect("Controller", Some(0x045e), Some(0x02ea)), Xbox);
        // the XInput backend reports no ids
        assert_eq!(detect("Xbox Controller", None, None), Xbox);
        assert_eq!(detect("XInput Controller #1", None, None), Xbox);
        assert_eq!(
            detect("DualSense Wireless Controller", None, None),
            PlayStation
        );
        assert_eq!(
            detect("Nintendo Switch Joy-Con (L)", Some(0x1234), None),
            Nintendo
        );
        assert_eq!(detect("8BitDo SN30", Some(0x2dc8), Some(0x6001)), Generic);
    }
}
