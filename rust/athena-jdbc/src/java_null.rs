pub const JAVA_NULL: &str = "null";

pub trait OrJavaNull {
    type Output;

    fn or_java_null(self) -> Self::Output;
}

impl<'a> OrJavaNull for Option<&'a str> {
    type Output = &'a str;

    fn or_java_null(self) -> &'a str {
        self.unwrap_or(JAVA_NULL)
    }
}

impl<'a> OrJavaNull for Option<&'a String> {
    type Output = &'a str;

    fn or_java_null(self) -> &'a str {
        self.map_or(JAVA_NULL, String::as_str)
    }
}

impl OrJavaNull for Option<String> {
    type Output = String;

    fn or_java_null(self) -> String {
        self.unwrap_or_else(|| JAVA_NULL.to_string())
    }
}
