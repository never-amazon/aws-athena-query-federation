#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatorType {
    UNARY,
    BINARY,
    VARARG,
}
