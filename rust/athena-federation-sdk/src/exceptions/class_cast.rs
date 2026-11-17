const JAVA_BASE_PACKAGES: [&str; 7] = [
    "java.lang.",
    "java.math.",
    "java.util.",
    "java.time.",
    "java.io.",
    "java.nio.",
    "java.net.",
];
const OTHER_MODULE_PACKAGES: [&str; 3] =
    ["java.util.logging.", "java.util.prefs.", "java.net.http."];

fn java_base_class(name: &str) -> bool {
    if let Some(element) = name.strip_prefix('[') {
        let element = element.trim_start_matches('[');
        return match element.strip_prefix('L') {
            Some(class) => class
                .strip_suffix(';')
                .is_some_and(|class| java_base_class(class)),
            None => matches!(element, "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D"),
        };
    }
    JAVA_BASE_PACKAGES.iter().any(|package| {
        name.strip_prefix(package)
            .is_some_and(|rest| !rest.is_empty())
    }) && !OTHER_MODULE_PACKAGES
        .iter()
        .any(|package| name.starts_with(package))
}

pub fn class_cast_message(from: &str, to: &str) -> String {
    if java_base_class(from) && java_base_class(to) {
        format!(
            "class {from} cannot be cast to class {to} ({from} and {to} are in module java.base of loader 'bootstrap')"
        )
    } else {
        format!("class {from} cannot be cast to class {to}")
    }
}

#[cfg(test)]
mod tests {
    use super::class_cast_message;

    #[test]
    fn appends_the_jvm_suffix_when_both_classes_are_in_java_base() {
        assert_eq!(
            class_cast_message("java.lang.String", "java.lang.Integer"),
            "class java.lang.String cannot be cast to class java.lang.Integer (java.lang.String and java.lang.Integer are in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("[B", "java.lang.String"),
            "class [B cannot be cast to class java.lang.String ([B and java.lang.String are in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("[Ljava.lang.String;", "java.lang.Integer"),
            "class [Ljava.lang.String; cannot be cast to class java.lang.Integer ([Ljava.lang.String; and java.lang.Integer are in module java.base of loader 'bootstrap')"
        );
        assert_eq!(
            class_cast_message("java.util.ImmutableCollections$List12", "java.lang.Integer"),
            "class java.util.ImmutableCollections$List12 cannot be cast to class java.lang.Integer (java.util.ImmutableCollections$List12 and java.lang.Integer are in module java.base of loader 'bootstrap')"
        );
    }

    #[test]
    fn leaves_the_suffix_off_when_a_class_has_a_loader_specific_location() {
        assert_eq!(
            class_cast_message("org.apache.calcite.util.TimestampString", "java.lang.Long"),
            "class org.apache.calcite.util.TimestampString cannot be cast to class java.lang.Long"
        );
        assert_eq!(
            class_cast_message("java.lang.String", "java.sql.Date"),
            "class java.lang.String cannot be cast to class java.sql.Date"
        );
        assert_eq!(
            class_cast_message("java.util.logging.Level", "java.lang.Integer"),
            "class java.util.logging.Level cannot be cast to class java.lang.Integer"
        );
        assert_eq!(
            class_cast_message("[Lorg.example.Thing;", "java.lang.Integer"),
            "class [Lorg.example.Thing; cannot be cast to class java.lang.Integer"
        );
    }
}
