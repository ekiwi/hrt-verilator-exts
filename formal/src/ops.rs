/// The operation performed by [`FsmOps::create_comparison`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Equals,
    NotEquals,
    LessThan,
    GreaterThan,
    LessThanOrEqual,
    GreaterThanOrEqual,
}

/// The operation performed by [`FsmOps::create_shifter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftOperation {
    ShiftLeft,
    ShiftRight,
    ShiftRightArithmetic,
    RotateLeft,
    RotateRight,
}
