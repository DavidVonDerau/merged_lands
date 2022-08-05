pub trait RoundTo<T> {
    fn round_to(self) -> T;
}

impl RoundTo<i32> for f32 {
    fn round_to(self) -> i32 {
        self as i32
    }
}

impl RoundTo<i8> for f32 {
    fn round_to(self) -> i8 {
        self as i8
    }
}

impl RoundTo<u8> for f32 {
    fn round_to(self) -> u8 {
        self as u8
    }
}

impl RoundTo<u16> for f32 {
    fn round_to(self) -> u16 {
        self as u16
    }
}
