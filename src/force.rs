use enum_map::Enum;


#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum Force {
    White,
    Black,
}

impl Force {
    pub fn opponent(self) -> Force {
        match self {
            Force::White => Force::Black,
            Force::Black => Force::White,
        }
    }
}
