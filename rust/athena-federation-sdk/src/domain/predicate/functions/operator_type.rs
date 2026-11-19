#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatorType {
    Unary,
    Binary,
    Vararg,
}
